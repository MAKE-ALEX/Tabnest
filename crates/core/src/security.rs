//! Security headers and the same-origin check used by the CSRF guard.

use url::Url;

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

pub const PERMISSIONS_POLICY: &str =
    "camera=(), microphone=(), geolocation=(), payment=(), usb=()";

/// The request headers relevant to a CSRF decision.
pub struct OriginCheck<'a> {
    pub host: Option<&'a str>,
    pub origin: Option<&'a str>,
    pub sec_fetch_site: Option<&'a str>,
    pub forwarded_https: bool,
}

/// Reject cross-site state-changing requests. Browsers send `Origin` on
/// POST/PUT/PATCH/DELETE; `Sec-Fetch-Site` catches modern same-site (different
/// port) cases that `SameSite=Lax` does not cover.
pub fn is_same_origin(check: &OriginCheck<'_>) -> bool {
    if let Some(site) = check.sec_fetch_site {
        let site = site.to_ascii_lowercase();
        if site == "cross-site" || site == "same-site" {
            return false;
        }
    }

    let Some(origin) = check.origin else {
        // Non-browser clients (curl) do not send Origin. They also do not carry
        // a victim's browser session, so allow them to keep the API usable.
        return true;
    };
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

fn authority_matches(origin: &Url, host_header: &str, forwarded_https: bool) -> bool {
    let Some(origin_host) = origin.host_str() else {
        return false;
    };
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

    let default_port = if forwarded_https { 443 } else { 80 };
    let port = explicit_port.unwrap_or(default_port);
    port == origin_port
}

fn split_host_header(value: &str) -> Option<(String, Option<u16>)> {
    let value = value.trim();
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
