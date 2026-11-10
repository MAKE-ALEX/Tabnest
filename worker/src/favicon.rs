//! 带 Cloudflare 边缘缓存的 favicon 代理。
//!
//! 接受两类输入：
//!
//! * 图标 URL（`https://site/favicon.ico`）—— 直接抓取；
//! * 页面 URL（`https://site/`）—— 解析 HTML 里的 `<link rel="icon">`，
//!   最后再尝试 `/favicon.ico`。
//!
//! 解析出的图片会存进 Workers Cache API（`caches.default`）七天，
//! 因此一个页面最多在每个数据中心、每个图标上付出一次解析成本。
//! 缓存不跨数据中心共享，也可能被淘汰——对图标来说无所谓，未命中重抓即可。

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

/// 边缘缓存有效期：7 天。
const CACHE_TTL_SECONDS: u32 = 7 * 24 * 60 * 60;
/// 缓存 key 的伪域名前缀，仅用于构造合法的缓存 URL。
const CACHE_URL_PREFIX: &str = "https://favicon-cache.tabnest.internal/";
/// 单个图标文件的大小上限。
const ICON_LIMIT: usize = 512 * 1024;
/// 页面 HTML 的大小上限。
const PAGE_LIMIT: usize = 1_500_000;

/// 由请求 URL 派生缓存 key。
///
/// 直接用原 URL 会让查询串把每次请求都变成不同的 key，所以改成哈希。
fn cache_key(url: &str) -> String {
    format!(
        "{CACHE_URL_PREFIX}{}",
        hex::encode(Sha256::digest(url.as_bytes()))
    )
}

/// 返回图标：先查边缘缓存，未命中再解析并回写。
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
            // 缓存内容不是可用的图片（例如被污染或格式过期），重新抓取。
            worker::console_warn!("favicon cache entry was not a usable image, refetching");
        }
        Ok(None) => {}
        Err(error) => worker::console_warn!("favicon cache read failed: {error}"),
    }

    let (bytes, kind) = resolve(&url, state.cfg.allow_private_network).await?;

    // 写缓存失败不影响本次响应。
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

/// 把调用方传入的东西解析成一张可用的位图图标。
async fn resolve(url: &str, allow_private: bool) -> AppResult<(Vec<u8>, &'static str)> {
    let (bytes, content_type, final_url) = fetch::fetch_limited(url, PAGE_LIMIT, allow_private)
        .await
        .map_err(|_| AppError::not_found("图标不可用"))?;

    // 直接就是图片。
    if let Some(kind) = sniff_image(&bytes, &content_type) {
        return Ok((bytes, kind));
    }

    // 是 HTML 页面：用声明的图标地址，但常常是 SVG（我们拒绝提供），
    // 所以下面还有 `/favicon.ico` 兜底。
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

/// 抓取一个候选图标地址，只接受经过校验的位图格式。
async fn fetch_raster(url: &str, allow_private: bool) -> AppResult<(Vec<u8>, &'static str)> {
    let (bytes, content_type, _) = fetch::fetch_limited(url, ICON_LIMIT, allow_private)
        .await
        .map_err(|_| AppError::not_found("图标不可用"))?;
    match sniff_image(&bytes, &content_type) {
        Some(kind) => Ok((bytes, kind)),
        None => Err(AppError::not_found("图标不是有效的图片")),
    }
}

/// 根据页面 URL 推导站点根目录的 `/favicon.ico`。
fn origin_favicon(page_url: &str) -> Option<String> {
    let parsed = Url::parse(page_url).ok()?;
    let origin = parsed.origin().ascii_serialization();
    // `about:blank`、`data:` 等非层级 URL 的 origin 是字符串 "null"。
    if origin == "null" {
        return None;
    }
    Some(format!("{origin}/favicon.ico"))
}

/// 构造写入 Cache API 的响应。
///
/// 必须带 `Cache-Control: max-age`，否则 Cloudflare 会拒绝缓存。
fn build_cacheable(bytes: &[u8], content_type: &'static str) -> worker::Result<worker::Response> {
    let headers = worker::Headers::new();
    headers.set("Content-Type", content_type)?;
    headers.set(
        "Cache-Control",
        &format!("public, max-age={CACHE_TTL_SECONDS}"),
    )?;
    Ok(worker::Response::from_bytes(bytes.to_vec())?.with_headers(headers))
}

/// 构造返回给浏览器的图标响应。
///
/// `cache_state` 会写进 `x-tabnest-cache`，方便排查是命中还是回源。
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
