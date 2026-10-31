use axum::{
    extract::Request,
    http::{header, HeaderMap, HeaderName, HeaderValue, Method},
    middleware::Next,
    response::Response,
};
use url::Url;

use crate::error::AppError;

const CSP: &str = "default-src 'self'; \
                   script-src 'self'; \
                   style-src 'self'; \
                   img-src 'self' data:; \
                   font-src 'self'; \
                   connect-src 'self'; \
                   object-src 'none'; \
                   base-uri 'none'; \
                   frame-ancestors 'none'; \
                   form-action 'self'";

const PERMISSIONS_POLICY: &str =
    "camera=(), microphone=(), geolocation=(), payment=(), usb=()";

/// Security headers for every response, including static assets and errors.
pub async fn security_headers(request: Request, next: Next) -> Response {
    let path = request.uri().path().to_string();
    let is_api = path.starts_with("/api/");
    let forwarded_https = request
        .headers()
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.eq_ignore_ascii_case("https"))
        .unwrap_or(false);

    let mut response = next.run(request).await;
    let headers = response.headers_mut();

    if is_api && path != "/api/favicon" {
        headers.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-store"),
        );
        headers.insert(
            HeaderName::from_static("cdn-cache-control"),
            HeaderValue::from_static("no-store"),
        );
    }

    if !headers.contains_key(header::CONTENT_SECURITY_POLICY) {
        headers.insert(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(CSP),
        );
    }
    headers.insert(
        header::X_FRAME_OPTIONS,
        HeaderValue::from_static("DENY"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        HeaderName::from_static("permissions-policy"),
        HeaderValue::from_static(PERMISSIONS_POLICY),
    );
    headers.insert(
        HeaderName::from_static("cross-origin-resource-policy"),
        HeaderValue::from_static("same-origin"),
    );

    if forwarded_https {
        headers.insert(
            header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static("max-age=31536000; includeSubDomains"),
        );
    }

    response
}

/// Reject cross-site state-changing requests. Browsers send `Origin` on
/// POST/PUT/PATCH/DELETE; `Sec-Fetch-Site` catches modern same-site (different
/// port) cases that `SameSite=Lax` does not cover.
pub async fn csrf_guard(request: Request, next: Next) -> Result<Response, AppError> {
    let method = request.method().clone();
    if matches!(
        method,
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    ) && !request_is_same_origin(request.headers())
    {
        return Err(AppError::forbidden("跨站请求被拒绝"));
    }
    Ok(next.run(request).await)
}

fn request_is_same_origin(headers: &HeaderMap) -> bool {
    if let Some(site) = header_str(headers, "sec-fetch-site") {
        let site = site.to_ascii_lowercase();
        if site == "cross-site" || site == "same-site" {
            return false;
        }
    }

    let Some(origin) = header_str(headers, header::ORIGIN.as_str()) else {
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
    let Some(host_header) = header_str(headers, header::HOST.as_str()) else {
        return false;
    };
    authority_matches(&origin_url, host_header, headers)
}

fn authority_matches(origin: &Url, host_header: &str, headers: &HeaderMap) -> bool {
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

    let forwarded_https = header_str(headers, "x-forwarded-proto")
        .map(|value| value.eq_ignore_ascii_case("https"))
        .unwrap_or(false);
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

fn header_str<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(*name, value.parse().unwrap());
        }
        map
    }

    #[test]
    fn accepts_same_origin() {
        let map = headers(&[("host", "192.168.20.110:8420"), ("origin", "http://192.168.20.110:8420")]);
        assert!(request_is_same_origin(&map));
    }

    #[test]
    fn rejects_cross_site_and_cross_port() {
        let map = headers(&[
            ("host", "192.168.20.110:8420"),
            ("origin", "http://192.168.20.110:3000"),
            ("sec-fetch-site", "same-site"),
        ]);
        assert!(!request_is_same_origin(&map));

        let map = headers(&[
            ("host", "192.168.20.110:8420"),
            ("origin", "http://evil.example"),
        ]);
        assert!(!request_is_same_origin(&map));
    }

    #[test]
    fn allows_cli_without_origin() {
        let map = headers(&[("host", "192.168.20.110:8420")]);
        assert!(request_is_same_origin(&map));
    }

    #[test]
    fn handles_https_proxy_and_ipv6() {
        let map = headers(&[
            ("host", "tabnest.example"),
            ("origin", "https://tabnest.example"),
            ("x-forwarded-proto", "https"),
        ]);
        assert!(request_is_same_origin(&map));

        let map = headers(&[
            ("host", "[::1]:8420"),
            ("origin", "http://[::1]:8420"),
        ]);
        assert!(request_is_same_origin(&map));
    }
}
