//! 抓取网页元信息与 favicon 时使用的出站 HTTP。
//!
//! 解析辅助函数放在 [`tabnest_core`] 里，方便 Workers 构建复用；
//! 这个模块只负责原生环境下的 `reqwest` 传输，以及带 DNS 钉扎的 SSRF 防护。

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

/// 抓取网页时使用的 UA：伪装成常见浏览器，避免站点直接拒绝非浏览器请求。
pub const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36 Tabnest/0.1";

/// 最多跟随的重定向次数。
const MAX_REDIRECTS: usize = 3;

/// 抓取 URL，并保证写入内存的响应体不超过 `limit` 字节。
///
/// 重定向由本函数手动处理，这样每一跳都会重新经过 SSRF 校验；
/// DNS 解析结果会被钉扎到请求客户端，防止二次解析时被换成内网地址。
pub async fn fetch_limited(
    url: &str,
    limit: usize,
    allow_private: bool,
) -> Result<(Vec<u8>, String, String)> {
    let mut current = Url::parse(url).context("网址格式不正确")?;

    for _ in 0..=MAX_REDIRECTS {
        // 每一跳都重新校验协议和是否夹带账号密码。
        if !matches!(current.scheme(), "http" | "https") {
            bail!("仅支持 http/https 网址");
        }
        if !current.username().is_empty() || current.password().is_some() {
            bail!("网址不能包含用户名或密码");
        }

        let client = build_guarded_client(&current, allow_private).await?;
        let response = client.get(current.clone()).send().await?;
        let status = response.status();

        // 手动跟随重定向：解析 Location 后交给下一轮循环继续校验。
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

        // 有 Content-Length 时先做一次快速拒绝，省掉下载。
        if let Some(length) = response.content_length() {
            if length as usize > limit {
                bail!("response too large: {length} bytes");
            }
        }

        // 流式读取，边读边计数，超限立即中止。
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

/// 解析目标主机并创建「已校验」的 HTTP 客户端。
///
/// 关闭自动重定向和系统代理，先解析 DNS，再逐个检查地址是否落在禁止网段；
/// 通过校验后把解析结果钉扎给客户端，后续连接不会再走 DNS。
async fn build_guarded_client(url: &Url, allow_private: bool) -> Result<reqwest::Client> {
    let host = url.host_str().context("网址缺少域名")?;
    let port = url.port_or_known_default().context("无法确定网址端口")?;
    let addresses = resolve_host(host, port).await?;
    if addresses.is_empty() {
        bail!("域名解析失败");
    }

    // 默认禁止访问内网 / 本机；只有显式开启 ALLOW_PRIVATE_NETWORK 才放行。
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

    // 把已校验的 DNS 结果钉扎给客户端，避免第二次解析把域名解析到内网地址
    // （DNS rebinding）。主机本身就是 IP 时无需钉扎。
    if host.parse::<IpAddr>().is_err() {
        builder = builder.resolve_to_addrs(host, &addresses);
    }

    builder.build().context("无法创建 HTTP 客户端")
}

/// 解析主机名到套接字地址；如果本身是 IP 就直接使用。
async fn resolve_host(host: &str, port: u16) -> Result<Vec<SocketAddr>> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(vec![SocketAddr::new(ip, port)]);
    }
    let addresses = tokio::net::lookup_host((host, port))
        .await
        .with_context(|| format!("无法解析域名 {host}"))?;
    Ok(addresses.collect())
}

/// 抓取一个网页并提取标题和图标。
///
/// 非 HTML 类内容直接返回空元信息，避免把图片、PDF 等二进制当网页解析。
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
