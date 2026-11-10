//! 抓取网页元信息和 favicon 时使用的出站 HTTP。
//!
//! Workers 无法像原生服务端那样钉住 DNS 解析结果（`reqwest`
//! `resolve_to_addrs`），所以这里的防护必然更弱：字面量形式的私有 / 回环
//! 主机名会在发请求前被拒绝，但一个「解析后」指向内网地址的域名，
//! `fetch` 不会做二次校验。

use tabnest_core::html::{extract_icon, extract_title, PageMeta};
use tabnest_core::net::is_forbidden_ip;
use url::Url;

use crate::error::{AppError, AppResult};

pub const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36 Tabnest/0.1";

/// 判断主机名是否明显指向本机 / 内网：特殊后缀，或本身就是禁止网段的 IP。
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

/// 抓取 URL，并保证读入内存的响应体不超过 `limit` 字节。
pub async fn fetch_limited(
    url: &str,
    limit: usize,
    allow_private: bool,
) -> AppResult<(Vec<u8>, String, String)> {
    let parsed = Url::parse(url).map_err(|_| AppError::bad_request("网址格式不正确"))?;
    // 只允许 http/https，并拒绝夹带账号密码的 URL。
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

    // 非 2xx/3xx 一律视为失败（Workers 的 fetch 默认会跟随重定向）。
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

    // 先用 Content-Length 快速拒绝，再在读取后复查一次实际大小。
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

/// 抓取一个网页并提取标题和图标。
///
/// 非 HTML / 文本类内容直接返回空元信息，避免把二进制当网页解析。
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
