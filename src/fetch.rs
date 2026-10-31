use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::OnceLock,
    time::Duration,
};

use anyhow::{bail, Context, Result};
use regex::Regex;
use url::Url;

pub const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36 Tabnest/0.1";

const MAX_REDIRECTS: usize = 3;

#[derive(Debug, Default, Clone)]
pub struct PageMeta {
    pub title: Option<String>,
    pub icon: Option<String>,
}

fn title_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?is)<title[^>]*>(.*?)</title>").expect("title regex"))
}

fn link_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?is)<link\b[^>]*>").expect("link regex"))
}

fn attr_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?is)([a-zA-Z_:][-a-zA-Z0-9_:.]*)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'>]+))"#)
            .expect("attr regex")
    })
}

fn entity_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"&(#[xX]?[0-9A-Fa-f]+|[a-zA-Z][a-zA-Z0-9]{1,31});").expect("entity regex")
    })
}

/// Pick one attribute out of an HTML tag, accepting double, single or no quotes.
pub fn attr_value(tag: &str, name: &str) -> Option<String> {
    for captures in attr_re().captures_iter(tag) {
        if captures
            .get(1)
            .map(|m| m.as_str().eq_ignore_ascii_case(name))
            .unwrap_or(false)
        {
            return captures
                .get(2)
                .or_else(|| captures.get(3))
                .or_else(|| captures.get(4))
                .map(|m| m.as_str().to_string());
        }
    }
    None
}

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

/// Reject loopback, private, link-local, CGNAT, multicast, documentation and
/// otherwise non-public addresses. IPv4-mapped IPv6 addresses are unwrapped.
pub fn is_forbidden_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_forbidden_v4(v4),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_forbidden_v4(v4);
            }
            let segments = v6.segments();
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (segments[0] & 0xfe00) == 0xfc00 // fc00::/7 unique local
                || (segments[0] & 0xffc0) == 0xfe80 // fe80::/10 link local
                || (segments[0] == 0x2001 && segments[1] == 0x0db8) // documentation
                || segments[0] == 0x2002 // 6to4
                || (segments[0] == 0x2001 && segments[1] == 0x0000) // Teredo
        }
    }
}

fn is_forbidden_v4(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_unspecified()
        || ip.is_multicast()
        || octets[0] == 0
        || (octets[0] == 100 && (64..=127).contains(&octets[1]))
        || (octets[0] == 192 && octets[1] == 0 && octets[2] == 0)
        || (octets[0] == 192 && octets[1] == 88 && octets[2] == 99)
        || (octets[0] == 198 && (octets[1] == 18 || octets[1] == 19))
        || octets[0] >= 240
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

pub fn extract_title(html: &str) -> Option<String> {
    let captures = title_re().captures(html)?;
    let raw = captures.get(1)?.as_str();
    let decoded = decode_entities(raw);
    let collapsed = decoded.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = collapsed.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(truncate_chars(trimmed, 300))
    }
}

pub fn extract_icon(html: &str, page_url: &str) -> Option<String> {
    let base = Url::parse(page_url).ok()?;
    let mut candidates: Vec<(u8, String)> = Vec::new();

    for tag in link_re().find_iter(html) {
        let tag = tag.as_str();
        let Some(rel) = attr_value(tag, "rel").map(|value| value.to_ascii_lowercase()) else {
            continue;
        };
        if !rel.contains("icon") {
            continue;
        }
        let Some(href) = attr_value(tag, "href") else {
            continue;
        };
        let href = href.trim();
        if href.is_empty() || href.starts_with("data:") {
            continue;
        }
        let Ok(icon) = base.join(href) else {
            continue;
        };
        if !matches!(icon.scheme(), "http" | "https") {
            continue;
        }
        let score = if rel.split_whitespace().any(|part| part == "icon") {
            3
        } else if rel.contains("shortcut") {
            2
        } else {
            1
        };
        candidates.push((score, icon.to_string()));
    }

    candidates.sort_by(|left, right| right.0.cmp(&left.0));
    if let Some((_, icon)) = candidates.into_iter().next() {
        return Some(icon);
    }
    base.join("/favicon.ico").ok().map(|url| url.to_string())
}

pub fn normalize_url(input: &str) -> Result<String, String> {
    let raw = input.trim();
    if raw.is_empty() {
        return Err("请输入网址".into());
    }
    if raw.len() > 2048 {
        return Err("网址太长（最多 2048 个字符）".into());
    }
    let candidate = if raw.starts_with("http://") || raw.starts_with("https://") {
        raw.to_string()
    } else if raw.contains("://") {
        return Err("仅支持 http / https 网址".into());
    } else {
        format!("https://{raw}")
    };
    let url = Url::parse(&candidate).map_err(|_| "网址格式不正确".to_string())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("仅支持 http / https 网址".into());
    }
    if url.host_str().unwrap_or("").is_empty() {
        return Err("网址缺少域名".into());
    }
    Ok(url.to_string())
}

pub fn host_label(url: &str) -> String {
    Url::parse(url)
        .ok()
        .and_then(|url| url.host_str().map(|host| host.trim_start_matches("www.").to_string()))
        .filter(|host| !host.is_empty())
        .unwrap_or_else(|| url.to_string())
}

pub fn truncate_chars(value: &str, max: usize) -> String {
    let mut chars = value.chars();
    let truncated: String = chars.by_ref().take(max).collect();
    if chars.next().is_some() {
        format!("{}…", truncated.trim_end())
    } else {
        truncated
    }
}

pub fn decode_entities(input: &str) -> String {
    entity_re()
        .replace_all(input, |captures: &regex::Captures<'_>| {
            let name = &captures[1];
            match name {
                "amp" => "&".to_string(),
                "lt" => "<".to_string(),
                "gt" => ">".to_string(),
                "quot" => "\"".to_string(),
                "apos" => "'".to_string(),
                "nbsp" => " ".to_string(),
                "mdash" => "—".to_string(),
                "ndash" => "–".to_string(),
                "hellip" => "…".to_string(),
                "middot" => "·".to_string(),
                "copy" => "©".to_string(),
                "reg" => "®".to_string(),
                "trade" => "™".to_string(),
                _ => {
                    if let Some(rest) = name.strip_prefix('#') {
                        let code = if let Some(hex) = rest
                            .strip_prefix('x')
                            .or_else(|| rest.strip_prefix('X'))
                        {
                            u32::from_str_radix(hex, 16).ok()
                        } else {
                            rest.parse::<u32>().ok()
                        };
                        code.and_then(char::from_u32)
                            .map(|ch| ch.to_string())
                            .unwrap_or_else(|| captures[0].to_string())
                    } else {
                        captures[0].to_string()
                    }
                }
            }
        })
        .into_owned()
}

/// Detect an image type from magic bytes (used for the favicon proxy).
///
/// SVG is deliberately excluded: serving attacker-controlled SVG from the app
/// origin would turn the favicon proxy into a same-origin XSS vector.
pub fn sniff_image(bytes: &[u8], content_type: &str) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        return Some("image/png");
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }
    if bytes.starts_with(b"GIF8") {
        return Some("image/gif");
    }
    if bytes.len() > 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    if bytes.starts_with(&[0x00, 0x00, 0x01, 0x00]) {
        return Some("image/x-icon");
    }

    let content_type = content_type.split(';').next().unwrap_or("").trim();
    match content_type {
        "image/png" => Some("image/png"),
        "image/jpeg" | "image/jpg" => Some("image/jpeg"),
        "image/gif" => Some("image/gif"),
        "image/webp" => Some("image/webp"),
        "image/x-icon" | "image/vnd.microsoft.icon" => Some("image/x-icon"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_bare_domains() {
        assert_eq!(normalize_url("example.com").unwrap(), "https://example.com/");
        assert_eq!(
            normalize_url("http://example.com/a").unwrap(),
            "http://example.com/a"
        );
        assert!(normalize_url("javascript:alert(1)").is_err());
        assert!(normalize_url("   ").is_err());
    }

    #[test]
    fn reads_title_and_icon() {
        let html = r#"<html><head><title> Hello &amp; World </title>
            <link rel="icon" href="/static/icon.png"></head></html>"#;
        assert_eq!(extract_title(html).as_deref(), Some("Hello & World"));
        assert_eq!(
            extract_icon(html, "https://example.com/page").as_deref(),
            Some("https://example.com/static/icon.png")
        );
    }

    #[test]
    fn rejects_svg_and_non_images() {
        assert_eq!(sniff_image(b"<svg xmlns=\"...\">", ""), None);
        assert_eq!(sniff_image(b"<html>", "text/html"), None);
        assert_eq!(
            sniff_image(b"<?xml version=\"1.0\"?><svg>", "image/svg+xml"),
            None
        );
    }

    #[test]
    fn detects_raster_images() {
        assert_eq!(sniff_image(b"\x89PNG\r\n\x1a\n", ""), Some("image/png"));
        assert_eq!(sniff_image(b"GIF89a", ""), Some("image/gif"));
    }

    #[test]
    fn blocks_private_and_special_addresses() {
        for raw in [
            "127.0.0.1",
            "10.0.0.1",
            "172.16.0.1",
            "192.168.20.110",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "224.0.0.1",
            "255.255.255.255",
            "198.18.0.1",
            "::1",
            "::",
            "fc00::1",
            "fe80::1",
            "::ffff:127.0.0.1",
            "::ffff:192.168.1.1",
        ] {
            let ip: IpAddr = raw.parse().unwrap();
            assert!(is_forbidden_ip(ip), "{raw} should be blocked");
        }
    }

    #[test]
    fn allows_public_addresses() {
        for raw in ["1.1.1.1", "8.8.8.8", "93.184.216.34", "2606:4700:4700::1111"] {
            let ip: IpAddr = raw.parse().unwrap();
            assert!(!is_forbidden_ip(ip), "{raw} should be allowed");
        }
    }
}
