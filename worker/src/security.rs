//! Worker 的全局安全响应头与 CSRF 防护。

use axum::{
    extract::Request,
    http::{header, HeaderMap, HeaderName, HeaderValue, Method},
    middleware::Next,
    response::Response,
};

use tabnest_core::security::{is_same_origin, OriginCheck, CSP, PERMISSIONS_POLICY};

use crate::error::AppError;

/// 给每个 Worker 响应补上安全响应头。
///
/// 静态资源不经过 Worker，它们的安全头由 `static/_headers` 在边缘生效。
pub async fn headers(request: Request, next: Next) -> Response {
    let path = request.uri().path().to_string();
    let forwarded_https = request
        .headers()
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.eq_ignore_ascii_case("https"))
        .unwrap_or(false);

    let mut response = next.run(request).await;
    let headers = response.headers_mut();

    // API 响应默认不缓存；图标接口自身会设置长缓存，跳过。
    if path.starts_with("/api/") && path != "/api/favicon" {
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
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
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
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

    if forwarded_https || request_was_https(&path) {
        headers.insert(
            header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static("max-age=31536000; includeSubDomains"),
        );
    }

    response
}

/// Workers 一定跑在 TLS 后面，所以即使入站请求没带 `X-Forwarded-Proto`，
/// 也应当加 HSTS。保留参数是为了和原生实现保持相同签名。
fn request_was_https(_path: &str) -> bool {
    true
}

/// 拒绝跨站的状态变更请求，规则与原生服务端完全一致。
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
    // Workers 环境默认视为 HTTPS。
    let forwarded_https = header_str(headers, "x-forwarded-proto")
        .map(|value| value.eq_ignore_ascii_case("https"))
        .unwrap_or(true);
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
