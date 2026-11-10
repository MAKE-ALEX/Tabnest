//! 安全响应头，以及 CSRF 防护使用的同源判断。

use url::Url;

/// 内容安全策略：默认只允许加载本站资源，禁止内联脚本、内联样式、iframe、
/// 表单外发和 `<base>` 改写，尽量缩小 XSS 的影响面。
pub const CSP: &str = "default-src 'self'; \
                       script-src 'self'; \
                       style-src 'self'; \
                       img-src 'self' data:; \
                       font-src 'self'; \
                       connect-src 'self'; \
                       object-src 'none'; \
                       base-uri 'none'; \
                       frame-ancestors 'none'; \
                       form-action 'self'";

/// 关闭摄像头、麦克风、定位、支付、USB 等本应用不需要的浏览器能力。
pub const PERMISSIONS_POLICY: &str =
    "camera=(), microphone=(), geolocation=(), payment=(), usb=()";

/// 做同源判断所需的请求头。
pub struct OriginCheck<'a> {
    /// 请求的 `Host` 头。
    pub host: Option<&'a str>,
    /// 请求的 `Origin` 头。
    pub origin: Option<&'a str>,
    /// 请求的 `Sec-Fetch-Site` 头。
    pub sec_fetch_site: Option<&'a str>,
    /// 代理是否通过 `X-Forwarded-Proto` 声明了 HTTPS。
    pub forwarded_https: bool,
}

/// 判断一个会修改状态的请求是否来自本站。
///
/// 浏览器在 POST/PUT/PATCH/DELETE 时会带 `Origin`；`Sec-Fetch-Site` 还能覆盖
/// 「同站不同端口」这种 `SameSite=Lax` 管不到的情况。
/// 返回 `false` 表示应当拒绝该请求。
pub fn is_same_origin(check: &OriginCheck<'_>) -> bool {
    // 现代浏览器会明确标注 cross-site / same-site，直接按它拒绝。
    if let Some(site) = check.sec_fetch_site {
        let site = site.to_ascii_lowercase();
        if site == "cross-site" || site == "same-site" {
            return false;
        }
    }

    let Some(origin) = check.origin else {
        // 非浏览器客户端（如 curl）不会带 Origin，也拿不到受害者的浏览器会话，
        // 放行它们才能让 API 和脚本正常使用。
        return true;
    };
    // `Origin: null` 常见于沙箱 iframe / file://，一律不信任。
    if origin.eq_ignore_ascii_case("null") {
        return false;
    }

    let Ok(origin_url) = Url::parse(origin) else {
        return false;
    };
    let Some(host_header) = check.host else {
        return false;
    };
    authority_matches(&origin_url, host_header, check.forwarded_https)
}

/// 比较 Origin 与 Host 头中的主机名和端口是否一致。
fn authority_matches(origin: &Url, host_header: &str, forwarded_https: bool) -> bool {
    let Some(origin_host) = origin.host_str() else {
        return false;
    };
    // IPv6 地址在 URL 中带方括号，Host 头中不带，统一去掉方括号再比较。
    let origin_host = origin_host
        .trim_start_matches('[')
        .trim_end_matches(']');
    let Some(origin_port) = origin.port_or_known_default() else {
        return false;
    };
    let Some((host, explicit_port)) = split_host_header(host_header) else {
        return false;
    };
    if !host.eq_ignore_ascii_case(origin_host) {
        return false;
    }

    // Host 头没写端口时，按代理声明的协议推断默认端口。
    let default_port = if forwarded_https { 443 } else { 80 };
    let port = explicit_port.unwrap_or(default_port);
    port == origin_port
}

/// 拆分 `Host` 头，兼容 `example.com`、`example.com:8080`、`[::1]:8420` 三种形式。
fn split_host_header(value: &str) -> Option<(String, Option<u16>)> {
    let value = value.trim();
    // IPv6 字面量：方括号内是地址，方括号后可选 `:端口`。
    if let Some(rest) = value.strip_prefix('[') {
        let end = rest.find(']')?;
        let host = &rest[..end];
        let after = &rest[end + 1..];
        if after.is_empty() {
            return Some((host.to_string(), None));
        }
        let port = after.strip_prefix(':')?.parse().ok()?;
        return Some((host.to_string(), Some(port)));
    }

    // 普通主机名：最多只能有一个冒号；多个冒号说明不是合法的 Host 头。
    match value.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') => {
            Some((host.to_string(), Some(port.parse().ok()?)))
        }
        Some(_) => None,
        None => Some((value.to_string(), None)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check<'a>(
        host: &'a str,
        origin: Option<&'a str>,
        sec_fetch_site: Option<&'a str>,
        forwarded_https: bool,
    ) -> OriginCheck<'a> {
        OriginCheck {
            host: Some(host),
            origin,
            sec_fetch_site,
            forwarded_https,
        }
    }

    #[test]
    fn accepts_same_origin() {
        assert!(is_same_origin(&check(
            "192.168.20.110:8420",
            Some("http://192.168.20.110:8420"),
            None,
            false
        )));
        assert!(is_same_origin(&check(
            "tabnest.example",
            Some("https://tabnest.example"),
            None,
            true
        )));
    }

    #[test]
    fn rejects_cross_site_and_cross_port() {
        assert!(!is_same_origin(&check(
            "192.168.20.110:8420",
            Some("http://192.168.20.110:3000"),
            Some("same-site"),
            false
        )));
        assert!(!is_same_origin(&check(
            "192.168.20.110:8420",
            Some("http://evil.example"),
            None,
            false
        )));
    }

    #[test]
    fn allows_cli_without_origin() {
        assert!(is_same_origin(&check("192.168.20.110:8420", None, None, false)));
    }

    #[test]
    fn handles_ipv6_host_header() {
        assert!(is_same_origin(&check(
            "[::1]:8420",
            Some("http://[::1]:8420"),
            None,
            false
        )));
    }
}
