//! 全局安全响应头与 CSRF 防护中间件。

use axum::{
    extract::Request,
    http::{header, HeaderMap, HeaderName, HeaderValue, Method},
    middleware::Next,
    response::Response,
};

use tabnest_core::security::{is_same_origin, OriginCheck};

use crate::error::AppError;

pub use tabnest_core::security::{CSP, PERMISSIONS_POLICY};

/// 给每个响应（包括静态资源与错误页）补上安全响应头。
pub async fn security_headers(request: Request, next: Next) -> Response {
    let path = request.uri().path().to_string();
    let is_api = path.starts_with("/api/");
    // 反代会用 X-Forwarded-Proto 告诉我们是 HTTPS，据此决定是否加 HSTS。
    let forwarded_https = request
        .headers()
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.eq_ignore_ascii_case("https"))
        .unwrap_or(false);

    let mut response = next.run(request).await;
    let headers = response.headers_mut();

    // API 响应默认不缓存；图标接口自身会设置长缓存，跳过。
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

    // 图标等响应可能已经自带 CSP，不覆盖。
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

    // 只有确认走的是 HTTPS 才加 HSTS，避免把纯 HTTP 部署锁死。
    if forwarded_https {
        headers.insert(
            header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static("max-age=31536000; includeSubDomains"),
        );
    }

    response
}

/// 拒绝跨站的状态变更请求。
///
/// 具体的同源判定逻辑放在 [`tabnest_core::security::is_same_origin`]，
/// 这样 Workers 构建可以复用完全相同的规则。
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

/// 从请求头组装同源校验所需的上下文。
fn request_is_same_origin(headers: &HeaderMap) -> bool {
    let forwarded_https = header_str(headers, "x-forwarded-proto")
        .map(|value| value.eq_ignore_ascii_case("https"))
        .unwrap_or(false);
    is_same_origin(&OriginCheck {
        host: header_str(headers, header::HOST.as_str()),
        origin: header_str(headers, header::ORIGIN.as_str()),
        sec_fetch_site: header_str(headers, "sec-fetch-site"),
        forwarded_https,
    })
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
        // 同站但不同端口，也视为跨源。
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
        // 没有 Origin 的请求（curl、命令行工具）放行。
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
