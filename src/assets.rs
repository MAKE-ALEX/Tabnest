use axum::{
    http::{header, HeaderMap, HeaderValue, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde_json::json;

const INDEX_HTML: &str = include_str!("../static/index.html");
const APP_JS: &str = include_str!("../static/app.js");
const STYLE_CSS: &str = include_str!("../static/style.css");
const FAVICON_SVG: &str = include_str!("../static/favicon.svg");
const MANIFEST: &str = include_str!("../static/manifest.webmanifest");

pub fn router() -> Router {
    Router::new()
        .route("/", get(index))
        .route("/index.html", get(index))
        .route("/app.js", get(app_js))
        .route("/style.css", get(style_css))
        .route("/favicon.svg", get(favicon))
        .route("/manifest.webmanifest", get(manifest))
        .route("/robots.txt", get(robots))
        .route("/healthz", get(health))
}

fn text_response(body: &'static str, content_type: &'static str) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-cache, must-revalidate"),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    (headers, body).into_response()
}

async fn index() -> Response {
    text_response(INDEX_HTML, "text/html; charset=utf-8")
}

async fn app_js() -> Response {
    text_response(APP_JS, "text/javascript; charset=utf-8")
}

async fn style_css() -> Response {
    text_response(STYLE_CSS, "text/css; charset=utf-8")
}

async fn favicon() -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("image/svg+xml"));
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=86400"),
    );
    (headers, FAVICON_SVG).into_response()
}

async fn manifest() -> Response {
    text_response(MANIFEST, "application/manifest+json; charset=utf-8")
}

async fn robots() -> Response {
    text_response("User-agent: *\nDisallow: /\n", "text/plain; charset=utf-8")
}

async fn health() -> Response {
    text_response("ok", "text/plain; charset=utf-8")
}

pub async fn not_found(uri: Uri) -> Response {
    if uri.path().starts_with("/api/") {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "接口不存在" })),
        )
            .into_response();
    }
    let body = "<!doctype html><html lang=\"zh-CN\"><meta charset=\"utf-8\"><title>404</title>\
                <body style=\"font-family:system-ui;padding:48px;color:#333\">\
                <h1>404</h1><p>页面不存在。</p><p><a href=\"/\">返回 Tabnest</a></p></body></html>";
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    (StatusCode::NOT_FOUND, headers, body).into_response()
}
