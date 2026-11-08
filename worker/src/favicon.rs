//! Favicon proxy with a Cloudflare edge cache.
//!
//! Every favicon is fetched at most once per data center for seven days: the
//! image is stored in the Workers Cache API (`caches.default`), so repeat page
//! loads cost a cache lookup instead of an outbound subrequest and a D1 read.
//! The cache is not shared across colos and is evicted under pressure, which is
//! fine for icons — a miss just refetches.

use axum::http::{header, HeaderMap, HeaderValue};
use axum::response::{IntoResponse, Response};
use sha2::{Digest, Sha256};
use tabnest_core::image::sniff_image;
use tabnest_core::url::normalize_url;
use worker::Cache;

use crate::{
    error::{AppError, AppResult},
    fetch, state::AppState,
};

const CACHE_TTL_SECONDS: u32 = 7 * 24 * 60 * 60;
const CACHE_URL_PREFIX: &str = "https://favicon-cache.tabnest.internal/";

/// Cache key derived from the normalised icon URL; the query string would make
/// each page load unique, so hash the URL instead of using it verbatim.
fn cache_key(url: &str) -> String {
    format!(
        "{CACHE_URL_PREFIX}{}",
        hex::encode(Sha256::digest(url.as_bytes()))
    )
}

pub async fn serve(state: &AppState, raw_url: &str) -> AppResult<Response> {
    let url = normalize_url(raw_url).map_err(AppError::bad_request)?;
    let cache = Cache::default();
    let key = cache_key(&url);

    match cache.get(key.as_str(), false).await {
        Ok(Some(mut cached)) => {
            if let Ok(bytes) = cached.bytes().await {
                if let Some(kind) = sniff_image(&bytes, "") {
                    return Ok(image_response(bytes, kind, "hit"));
                }
            }
            // Corrupt or outdated entry: fall through and refresh it.
            worker::console_warn!("favicon cache entry was not a usable image, refetching");
        }
        Ok(None) => {}
        Err(error) => worker::console_warn!("favicon cache read failed: {error}"),
    }

    let (bytes, content_type, _) =
        fetch::fetch_limited(&url, 512 * 1024, state.cfg.allow_private_network)
            .await
            .map_err(|_| AppError::not_found("图标不可用"))?;
    let Some(kind) = sniff_image(&bytes, &content_type) else {
        return Err(AppError::not_found("图标不是有效的图片"));
    };

    match build_cacheable(&bytes, kind) {
        Ok(entry) => {
            if let Err(error) = cache.put(key.as_str(), entry).await {
                worker::console_warn!("favicon cache write failed: {error}");
            }
        }
        Err(error) => worker::console_warn!("favicon cache entry build failed: {error}"),
    }

    Ok(image_response(bytes, kind, "miss"))
}

/// The response stored in the Cache API. It needs `Cache-Control: max-age` or
/// Cloudflare refuses to keep it.
fn build_cacheable(bytes: &[u8], content_type: &'static str) -> worker::Result<worker::Response> {
    let headers = worker::Headers::new();
    headers.set("Content-Type", content_type)?;
    headers.set(
        "Cache-Control",
        &format!("public, max-age={CACHE_TTL_SECONDS}"),
    )?;
    Ok(worker::Response::from_bytes(bytes.to_vec())?.with_headers(headers))
}

fn image_response(bytes: Vec<u8>, content_type: &'static str, cache_state: &'static str) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=604800, immutable"),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'none'; sandbox"),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("inline; filename=\"favicon\""),
    );
    headers.insert(
        "x-tabnest-cache",
        HeaderValue::from_static(cache_state),
    );
    (headers, bytes).into_response()
}
