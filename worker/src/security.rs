use axum::{
    extract::Request,
    http::{header, HeaderMap, HeaderName, HeaderValue, Method},
    middleware::Next,
    response::Response,
};

use tabnest_core::security::{is_same_origin, OriginCheck, CSP, PERMISSIONS_POLICY};

use crate::error::AppError;

/// Security headers for every Worker response. Static assets get the same
/// headers through `static/_headers`, which Cloudflare applies at the edge.
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

    if path.starts_with("/api/") && path != "/api/favicon" {
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
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

/// Workers always run behind TLS, so HSTS is added even though the inbound
/// request object does not carry `X-Forwarded-Proto`.
fn request_was_https(_path: &str) -> bool {
    true
}

/// Reject cross-site state-changing requests with the same rule the native
/// server applies.
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
