//! Outbound HTTP for page metadata and the favicon proxy.
//!
//! Workers cannot pin DNS answers the way the native server does (`reqwest`
//! `resolve_to_addrs`), so the guard here is necessarily weaker: literal
//! private/loopback hosts are rejected before the request is made, but a
//! hostname that *resolves* to a private address is not re-checked by `fetch`.

use tabnest_core::html::{extract_icon, extract_title, PageMeta};
use tabnest_core::net::is_forbidden_ip;
use url::Url;

use crate::error::{AppError, AppResult};

pub const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36 Tabnest/0.1";

fn reject_private_host(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host == "localhost" || host.ends_with(".localhost") || host.ends_with(".local") || host.ends_with(".internal") {
        return true;
    }
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return is_forbidden_ip(ip);
    }
    false
}

/// Fetch a URL, refusing to buffer more than `limit` bytes.
pub async fn fetch_limited(
    url: &str,
    limit: usize,
    allow_private: bool,
) -> AppResult<(Vec<u8>, String, String)> {
    let parsed = Url::parse(url).map_err(|_| AppError::bad_request("网址格式不正确"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(AppError::bad_request("仅支持 http/https 网址"));
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(AppError::bad_request("网址不能包含用户名或密码"));
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| AppError::bad_request("网址缺少域名"))?;
    if !allow_private && reject_private_host(host) {
        return Err(AppError::bad_request(
            "出于安全考虑，不允许访问内网或本机地址",
        ));
    }

    let headers = worker::Headers::new();
    headers.set("User-Agent", USER_AGENT)?;
    headers.set(
        "Accept",
        "text/html,application/xhtml+xml,image/*;q=0.9,*/*;q=0.8",
    )?;

    let mut init = worker::RequestInit::new();
    init.with_method(worker::Method::Get)
        .with_headers(headers);
    let request = worker::Request::new_with_init(parsed.as_str(), &init)?;

    let mut response = worker::Fetch::Request(request).send().await.map_err(|error| {
        worker::console_warn!("fetch {url} failed: {error}");
        AppError::bad_request("无法抓取该网址的信息（内网地址或访问失败）")
    })?;

    let status = response.status_code();
    if !(200..400).contains(&status) {
        return Err(AppError::bad_request(format!("目标站点返回 {status}")));
    }

    let content_type = response
        .headers()
        .get("content-type")
        .ok()
        .flatten()
        .unwrap_or_default()
        .to_lowercase();

    if let Some(length) = response
        .headers()
        .get("content-length")
        .ok()
        .flatten()
        .and_then(|value| value.parse::<usize>().ok())
    {
        if length > limit {
            return Err(AppError::bad_request("目标内容过大"));
        }
    }

    let bytes = response.bytes().await?;
    if bytes.len() > limit {
        return Err(AppError::bad_request("目标内容过大"));
    }

    Ok((bytes, content_type, parsed.to_string()))
}

pub async fn fetch_meta(url: &str, allow_private: bool) -> AppResult<PageMeta> {
    let (bytes, content_type, final_url) = fetch_limited(url, 1_500_000, allow_private).await?;
    if !content_type.is_empty()
        && !content_type.contains("html")
        && !content_type.contains("text")
        && !content_type.contains("xml")
    {
        return Ok(PageMeta::default());
    }
    let html = String::from_utf8_lossy(&bytes);
    Ok(PageMeta {
        title: extract_title(&html),
        icon: extract_icon(&html, &final_url),
    })
}
