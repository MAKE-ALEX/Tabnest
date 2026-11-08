//! Outbound HTTP for page metadata and the favicon proxy.
//!
//! The parsing helpers live in [`tabnest_core`] so the Workers build can reuse
//! them; this module only keeps the native `reqwest` transport and the
//! DNS-pinned SSRF guard.

use std::{
    net::{IpAddr, SocketAddr},
    time::Duration,
};

use anyhow::{bail, Context, Result};
use url::Url;

pub use tabnest_core::html::{extract_icon, extract_title, truncate_chars, PageMeta};
pub use tabnest_core::image::sniff_image;
pub use tabnest_core::net::is_forbidden_ip;
pub use tabnest_core::url::{host_label, normalize_url};

pub const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36 Tabnest/0.1";

const MAX_REDIRECTS: usize = 3;

/// Fetch a URL while refusing to buffer more than `limit` bytes.
///
/// Redirects are followed manually so every hop is checked against the SSRF
/// guard, and DNS answers are pinned to the validated addresses.
pub async fn fetch_limited(
    url: &str,
    limit: usize,
    allow_private: bool,
) -> Result<(Vec<u8>, String, String)> {
    let mut current = Url::parse(url).context("网址格式不正确")?;

    for _ in 0..=MAX_REDIRECTS {
        if !matches!(current.scheme(), "http" | "https") {
            bail!("仅支持 http/https 网址");
        }
        if !current.username().is_empty() || current.password().is_some() {
            bail!("网址不能包含用户名或密码");
        }

        let client = build_guarded_client(&current, allow_private).await?;
        let response = client.get(current.clone()).send().await?;
        let status = response.status();

        if matches!(status.as_u16(), 301 | 302 | 303 | 307 | 308) {
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .context("重定向响应缺少 Location")?;
            current = current.join(location).context("无效的重定向地址")?;
            continue;
        }

        let response = response.error_for_status()?;
        let final_url = response.url().to_string();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_lowercase();

        if let Some(length) = response.content_length() {
            if length as usize > limit {
                bail!("response too large: {length} bytes");
            }
        }

        let mut response = response;
        let mut buffer = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if buffer.len() + chunk.len() > limit {
                bail!("response too large (> {limit} bytes)");
            }
            buffer.extend_from_slice(&chunk);
        }
        return Ok((buffer, content_type, final_url));
    }

    bail!("重定向次数过多")
}

async fn build_guarded_client(url: &Url, allow_private: bool) -> Result<reqwest::Client> {
    let host = url.host_str().context("网址缺少域名")?;
    let port = url.port_or_known_default().context("无法确定网址端口")?;
    let addresses = resolve_host(host, port).await?;
    if addresses.is_empty() {
        bail!("域名解析失败");
    }

    if !allow_private {
        for address in &addresses {
            if is_forbidden_ip(address.ip()) {
                bail!("出于安全考虑，不允许访问内网或本机地址");
            }
        }
    }

    let mut builder = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .connect_timeout(Duration::from_secs(5))
        .user_agent(USER_AGENT);

    // Pin the already validated DNS answers so a second lookup cannot be used
    // to rebind the hostname to a private address.
    if host.parse::<IpAddr>().is_err() {
        builder = builder.resolve_to_addrs(host, &addresses);
    }

    builder.build().context("无法创建 HTTP 客户端")
}

async fn resolve_host(host: &str, port: u16) -> Result<Vec<SocketAddr>> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(vec![SocketAddr::new(ip, port)]);
    }
    let addresses = tokio::net::lookup_host((host, port))
        .await
        .with_context(|| format!("无法解析域名 {host}"))?;
    Ok(addresses.collect())
}

pub async fn fetch_meta(url: &str, allow_private: bool) -> Result<PageMeta> {
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
