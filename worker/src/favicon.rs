//! Favicon proxy with a Cloudflare edge cache.
//!
//! Two kinds of input are accepted:
//!
//! * an icon URL (`https://site/favicon.ico`) — fetched directly;
//! * a page URL (`https://site/`) — the HTML is parsed for `<link rel="icon">`,
//!   and `/favicon.ico` is tried as a last resort.
//!
//! The resolved image is stored in the Workers Cache API (`caches.default`) for
//! seven days, so a page load costs at most one resolution per icon per data
//! center. The cache is not shared across colos and can be evicted, which is
//! fine for icons — a miss just refetches.

use axum::http::{header, HeaderMap, HeaderValue};
use axum::response::{IntoResponse, Response};
use sha2::{Digest, Sha256};
use tabnest_core::html::extract_icon;
use tabnest_core::image::sniff_image;
use tabnest_core::url::normalize_url;
use url::Url;
use worker::Cache;

use crate::{
    error::{AppError, AppResult},
    fetch, state::AppState,
};

const CACHE_TTL_SECONDS: u32 = 7 * 24 * 60 * 60;
const CACHE_URL_PREFIX: &str = "https://favicon-cache.tabnest.internal/";
const ICON_LIMIT: usize = 512 * 1024;
const PAGE_LIMIT: usize = 1_500_000;

/// Cache key derived from the requested URL; the query string would make each
/// page load unique, so hash the URL instead of using it verbatim.
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
            worker::console_warn!("favicon cache entry was not a usable image, refetching");
        }
        Ok(None) => {}
        Err(error) => worker::console_warn!("favicon cache read failed: {error}"),
    }

    let (bytes, kind) = resolve(&url, state.cfg.allow_private_network).await?;

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

/// Turn whatever the caller passed into an actual raster icon.
async fn resolve(url: &str, allow_private: bool) -> AppResult<(Vec<u8>, &'static str)> {
    let (bytes, content_type, final_url) = fetch::fetch_limited(url, PAGE_LIMIT, allow_private)
        .await
        .map_err(|_| AppError::not_found("图标不可用"))?;

    if let Some(kind) = sniff_image(&bytes, &content_type) {
        return Ok((bytes, kind));
    }

    // HTML page: use the declared icon, which is often an SVG we refuse to
    // serve, hence the `/favicon.ico` fallback below.
    if content_type.is_empty()
        || content_type.contains("html")
        || content_type.contains("text")
        || content_type.contains("xml")
    {
        let html = String::from_utf8_lossy(&bytes);
        if let Some(icon) = extract_icon(&html, &final_url) {
            if let Ok((bytes, kind)) = fetch_raster(&icon, allow_private).await {
                return Ok((bytes, kind));
            }
        }
    }

    if let Some(candidate) = origin_favicon(&final_url) {
        if candidate != url {
            if let Ok((bytes, kind)) = fetch_raster(&candidate, allow_private).await {
                return Ok((bytes, kind));
            }
        }
    }

    Err(AppError::not_found("图标不可用"))
}

async fn fetch_raster(url: &str, allow_private: bool) -> AppResult<(Vec<u8>, &'static str)> {
    let (bytes, content_type, _) = fetch::fetch_limited(url, ICON_LIMIT, allow_private)
        .await
        .map_err(|_| AppError::not_found("图标不可用"))?;
    match sniff_image(&bytes, &content_type) {
        Some(kind) => Ok((bytes, kind)),
        None => Err(AppError::not_found("图标不是有效的图片")),
    }
}

fn origin_favicon(page_url: &str) -> Option<String> {
    let parsed = Url::parse(page_url).ok()?;
    let origin = parsed.origin().ascii_serialization();
    if origin == "null" {
        return None;
    }
    Some(format!("{origin}/favicon.ico"))
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
    headers.insert("x-tabnest-cache", HeaderValue::from_static(cache_state));
    (headers, bytes).into_response()
}
