//! 静态资源路由。
//!
//! 前端没有构建步骤，`static/` 下的文件在编译期用 `include_str!` / `include_bytes!`
//! 直接打进二进制；响应带 `no-store`，避免升级后浏览器还拿着旧文件。
//! 另外保留了一组带日期后缀的别名路径，兼容之前版本发出的、已被缓存的 HTML。

use axum::{
    http::{header, HeaderMap, HeaderName, HeaderValue, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde_json::json;

const INDEX_HTML: &str = include_str!("../static/index.html");
const APP_JS: &str = include_str!("../static/app.js");
const THEME_INIT_JS: &str = include_str!("../static/theme-init.js");
const STYLE_CSS: &str = include_str!("../static/style.css");
const FAVICON_SVG: &str = include_str!("../static/favicon.svg");
const WALLPAPER: &[u8] = include_bytes!("../static/wallpaper.jpg");
const MANIFEST: &str = include_str!("../static/manifest.webmanifest");

/// 构造静态资源路由。
pub fn router() -> Router {
    Router::new()
        .route("/", get(index))
        .route("/index.html", get(index))
        // `/g` 是给导入后跳转用的直达路径，返回同一个单页应用。
        .route("/g", get(index))
        .route("/app.js", get(app_js))
        // 以下旧版本文件名继续返回最新内容，避免已缓存页面 404。
        .route("/app-20261031g.js", get(app_js))
        .route("/app-20261031h.js", get(app_js))
        .route("/app-20261031i.js", get(app_js))
        .route("/theme-init.js", get(theme_init_js))
        .route("/theme-init-20261031g.js", get(theme_init_js))
        .route("/theme-init-20261031h.js", get(theme_init_js))
        .route("/theme-init-20261031i.js", get(theme_init_js))
        .route("/style.css", get(style_css))
        .route("/style-20261031g.css", get(style_css))
        .route("/style-20261031h.css", get(style_css))
        .route("/style-20261031i.css", get(style_css))
        .route("/favicon.svg", get(favicon))
        .route("/wallpaper.jpg", get(wallpaper))
        .route("/manifest.webmanifest", get(manifest))
        .route("/robots.txt", get(robots))
        .route("/healthz", get(health))
}

/// 统一的文本响应：显式设置 Content-Type、禁用各级缓存，并加上 `nosniff`。
fn text_response(body: &'static str, content_type: &'static str) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-store"),
    );
    headers.insert(
        HeaderName::from_static("cdn-cache-control"),
        HeaderValue::from_static("no-store"),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    (headers, body).into_response()
}

/// 首页（单页应用入口）。
async fn index() -> Response {
    text_response(INDEX_HTML, "text/html; charset=utf-8")
}

/// 前端主脚本。
async fn app_js() -> Response {
    text_response(APP_JS, "text/javascript; charset=utf-8")
}

/// 首屏之前执行的主题脚本，避免页面闪白 / 闪黑。
async fn theme_init_js() -> Response {
    text_response(THEME_INIT_JS, "text/javascript; charset=utf-8")
}

/// 前端样式表。
async fn style_css() -> Response {
    text_response(STYLE_CSS, "text/css; charset=utf-8")
}

/// 站点图标（SVG 可以放心公开缓存一天）。
async fn favicon() -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("image/svg+xml"));
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=86400"),
    );
    (headers, FAVICON_SVG).into_response()
}

/// 背景壁纸（内容不会变，可以长时间 immutable 缓存）。
async fn wallpaper() -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("image/jpeg"));
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=604800, immutable"),
    );
    (headers, WALLPAPER).into_response()
}

/// PWA 清单。
async fn manifest() -> Response {
    text_response(MANIFEST, "application/manifest+json; charset=utf-8")
}

/// 禁止搜索引擎抓取个人书签站。
async fn robots() -> Response {
    text_response("User-agent: *\nDisallow: /\n", "text/plain; charset=utf-8")
}

/// 健康检查端点，供容器 / 反向代理探活。
async fn health() -> Response {
    text_response("ok", "text/plain; charset=utf-8")
}

/// 兜底 404：API 请求返回 JSON 错误，页面请求返回简单的 HTML。
pub async fn not_found(uri: Uri) -> Response {
    if uri.path().starts_with("/api/") {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "接口不存在" })),
        )
            .into_response();
    }
    let body = "<!doctype html><html lang=\"zh-CN\"><meta charset=\"utf-8\"><title>404</title>\
                <body><h1>404</h1><p>页面不存在。</p><p><a href=\"/\">返回 Tabnest</a></p></body></html>";
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    (StatusCode::NOT_FOUND, headers, body).into_response()
}
