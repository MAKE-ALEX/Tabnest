//! HTTP API for the Workers build.
//!
//! This mirrors `src/api.rs` from the native server. The routing, validation and
//! JSON shapes are identical; only the storage layer (D1 instead of `sqlx`),
//! the session store (signed cookie instead of an in-memory map) and the
//! favicon cache (browser/CDN instead of a local directory) differ.

use axum::{
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware,
    response::{IntoResponse, Response},
    routing::{get, patch, post, put},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tabnest_core::html::truncate_chars;
use tabnest_core::url::{host_label, normalize_url};

use crate::{
    auth, db,
    error::{AppError, AppResult},
    fetch, security,
    state::AppState,
};

pub fn router(state: AppState) -> Router {
    let protected = Router::new()
        .route("/api/state", get(state_handler))
        .route("/api/boards", post(create_board))
        .route("/api/boards/order", put(order_boards))
        .route("/api/boards/{id}", patch(update_board).delete(delete_board))
        .route("/api/columns", post(create_column))
        .route("/api/columns/order", put(order_columns))
        .route("/api/columns/{id}", patch(update_column).delete(delete_column))
        .route("/api/links", post(create_link))
        .route("/api/links/arrange", put(arrange_links))
        .route("/api/links/{id}", patch(update_link).delete(delete_link))
        .route("/api/meta", get(meta_handler))
        .route("/api/favicon", get(favicon_handler))
        .route("/api/import/json", post(import_json))
        .route("/api/import/html", post(import_html))
        .route("/api/export/json", get(export_json))
        .route("/api/export/html", get(export_html))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_auth,
        ));

    Router::new()
        .route("/api/session", get(session_info))
        .route("/api/login", post(login))
        .route("/api/logout", post(logout))
        .route("/healthz", get(health))
        .merge(protected)
        .layer(DefaultBodyLimit::max(25 * 1024 * 1024))
        .layer(middleware::from_fn(security::csrf_guard))
        .layer(middleware::from_fn(security::headers))
        .with_state(state)
}

async fn health() -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    (headers, "ok").into_response()
}

/* ------------------------------------------------------------------ */
/* Session                                                             */
/* ------------------------------------------------------------------ */

#[worker::send]
async fn session_info(State(state): State<AppState>, headers: HeaderMap) -> Json<Value> {
    Json(json!({
        "auth_required": state.cfg.password.is_some(),
        "authenticated": auth::is_authenticated(&state, &headers),
    }))
}

#[derive(Deserialize)]
struct LoginRequest {
    password: String,
}

#[worker::send]
async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<LoginRequest>,
) -> AppResult<Response> {
    let Some(expected) = state.cfg.password.as_deref() else {
        return Ok(Json(json!({ "ok": true })).into_response());
    };

    let ip = auth::client_ip(&headers);
    if body.password.len() > 256 {
        return Err(AppError::unauthorized("密码不正确"));
    }

    let now = auth::now_ms();
    let lockout = {
        let Ok(mut limiter) = state.limiter.lock() else {
            return Err(AppError::internal("登录限速器不可用"));
        };
        match limiter.check(&ip, now) {
            auth::LoginCheck::Allowed => None,
            auth::LoginCheck::Locked(wait) => Some(wait),
        }
    };
    if let Some(wait) = lockout {
        return Ok(too_many_response(wait));
    }

    let candidate = body.password.trim();
    let mut diff = 0u8;
    for (left, right) in candidate.bytes().zip(expected.bytes()) {
        diff |= left ^ right;
    }
    // Length differences are already visible from the request itself; compare
    // the bytes to avoid an early-exit oracle.
    if diff != 0 || candidate.len() != expected.len() {
        let locked_for = state
            .limiter
            .lock()
            .ok()
            .and_then(|mut limiter| limiter.record_failure(&ip, now));
        if let Some(wait) = locked_for {
            return Ok(too_many_response(wait));
        }
        return Err(AppError::unauthorized("密码不正确"));
    }

    if let Ok(mut limiter) = state.limiter.lock() {
        limiter.record_success(&ip);
    }

    let token = auth::create_session(&state);
    let mut response = Json(json!({ "ok": true })).into_response();
    response
        .headers_mut()
        .insert(header::SET_COOKIE, auth::session_cookie(&state, &token));
    Ok(response)
}

fn too_many_response(retry_after_ms: u64) -> Response {
    let seconds = (retry_after_ms / 1000).max(1);
    let mut response = (
        StatusCode::TOO_MANY_REQUESTS,
        Json(json!({ "error": format!("尝试次数过多，请在 {seconds} 秒后重试") })),
    )
        .into_response();
    if let Ok(value) = HeaderValue::from_str(&seconds.to_string()) {
        response.headers_mut().insert(header::RETRY_AFTER, value);
    }
    response
}

#[worker::send]
async fn logout(State(state): State<AppState>) -> Response {
    let mut response = Json(json!({ "ok": true })).into_response();
    response
        .headers_mut()
        .insert(header::SET_COOKIE, auth::clear_cookie(&state));
    response
}

/* ------------------------------------------------------------------ */
/* Boards                                                              */
/* ------------------------------------------------------------------ */

#[worker::send]
async fn state_handler(State(state): State<AppState>) -> AppResult<Json<Value>> {
    let snapshot = db::load_state(&state.db).await?;
    Ok(Json(serde_json::to_value(snapshot)?))
}

#[derive(Deserialize)]
struct CreateBoard {
    name: String,
}

#[worker::send]
async fn create_board(
    State(state): State<AppState>,
    Json(body): Json<CreateBoard>,
) -> AppResult<Json<Value>> {
    let name = clean_name(&body.name, "面板名称")?;
    let id = db::create_board(&state.db, &name).await?;
    Ok(Json(json!({ "ok": true, "id": id })))
}

#[derive(Deserialize)]
struct UpdateBoard {
    name: String,
}

#[worker::send]
async fn update_board(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateBoard>,
) -> AppResult<Json<Value>> {
    let name = clean_name(&body.name, "面板名称")?;
    let changed = db::update_board(&state.db, id, &name).await?;
    ensure_affected(changed)?;
    Ok(Json(json!({ "ok": true })))
}

#[worker::send]
async fn delete_board(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<Value>> {
    let changed = db::delete_board(&state.db, id).await?;
    ensure_affected(changed)?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct OrderBoards {
    ids: Vec<i64>,
}

#[worker::send]
async fn order_boards(
    State(state): State<AppState>,
    Json(body): Json<OrderBoards>,
) -> AppResult<Json<Value>> {
    if body.ids.len() > 1000 {
        return Err(AppError::bad_request("排序数据过多"));
    }
    db::order_boards(&state.db, &body.ids).await?;
    Ok(Json(json!({ "ok": true })))
}

/* ------------------------------------------------------------------ */
/* Columns                                                             */
/* ------------------------------------------------------------------ */

#[derive(Deserialize)]
struct CreateColumn {
    board_id: i64,
    #[serde(default)]
    name: String,
}

#[worker::send]
async fn create_column(
    State(state): State<AppState>,
    Json(body): Json<CreateColumn>,
) -> AppResult<Json<Value>> {
    ensure_board(&state, body.board_id).await?;
    let name = if body.name.trim().is_empty() {
        "新分组".to_string()
    } else {
        clean_name(&body.name, "分组名称")?
    };
    let id = db::create_column(&state.db, body.board_id, &name).await?;
    Ok(Json(json!({ "ok": true, "id": id })))
}

#[derive(Deserialize)]
struct UpdateColumn {
    name: String,
}

#[worker::send]
async fn update_column(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateColumn>,
) -> AppResult<Json<Value>> {
    let name = clean_name(&body.name, "分组名称")?;
    let changed = db::update_column(&state.db, id, &name).await?;
    ensure_affected(changed)?;
    Ok(Json(json!({ "ok": true })))
}

#[worker::send]
async fn delete_column(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<Value>> {
    let changed = db::delete_column(&state.db, id).await?;
    ensure_affected(changed)?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct OrderColumns {
    board_id: i64,
    ids: Vec<i64>,
}

#[worker::send]
async fn order_columns(
    State(state): State<AppState>,
    Json(body): Json<OrderColumns>,
) -> AppResult<Json<Value>> {
    if body.ids.len() > 1000 {
        return Err(AppError::bad_request("排序数据过多"));
    }
    ensure_board(&state, body.board_id).await?;
    db::order_columns(&state.db, body.board_id, &body.ids).await?;
    Ok(Json(json!({ "ok": true })))
}

/* ------------------------------------------------------------------ */
/* Links                                                               */
/* ------------------------------------------------------------------ */

#[derive(Deserialize)]
struct CreateLink {
    column_id: i64,
    url: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    note: String,
    #[serde(default)]
    icon: Option<String>,
}

#[worker::send]
async fn create_link(
    State(state): State<AppState>,
    Json(body): Json<CreateLink>,
) -> AppResult<Json<Value>> {
    let url = normalize_url(&body.url).map_err(AppError::bad_request)?;
    ensure_column(&state, body.column_id).await?;

    let mut title = body.title.trim().to_string();
    let mut icon = clean_icon(body.icon);

    if title.is_empty() || icon.is_none() {
        let meta = fetch::fetch_meta(&url, state.cfg.allow_private_network)
            .await
            .unwrap_or_default();
        if title.is_empty() {
            title = meta.title.unwrap_or_else(|| host_label(&url));
        }
        if icon.is_none() {
            icon = meta.icon;
        }
    }
    if title.is_empty() {
        title = host_label(&url);
    }
    let title = truncate_chars(&title, 300);
    let note = truncate_chars(body.note.trim(), 2000);

    let id = db::create_link(
        &state.db,
        body.column_id,
        &title,
        &url,
        &note,
        icon.as_deref(),
    )
    .await?;
    Ok(Json(json!({ "ok": true, "id": id, "title": title })))
}

#[derive(Deserialize)]
struct UpdateLink {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    note: Option<String>,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    column_id: Option<i64>,
}

#[worker::send]
async fn update_link(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateLink>,
) -> AppResult<Json<Value>> {
    let Some(current) = db::get_link(&state.db, id).await? else {
        return Err(AppError::not_found("书签不存在"));
    };

    let new_url = match body.url.as_deref() {
        Some(raw) if !raw.trim().is_empty() => {
            normalize_url(raw).map_err(AppError::bad_request)?
        }
        _ => current.url.clone(),
    };
    let url_changed = new_url != current.url;

    let title = match body.title {
        Some(title) if !title.trim().is_empty() => truncate_chars(title.trim(), 300),
        _ => current.title.clone(),
    };
    let note = match body.note {
        Some(note) => truncate_chars(note.trim(), 2000),
        None => current.note.clone(),
    };
    let icon = match clean_icon(body.icon) {
        Some(icon) => Some(icon),
        None if url_changed => None,
        None => current.icon.clone(),
    };

    let mut column_id = current.column_id;
    let mut position = None;
    if let Some(target) = body.column_id {
        if target != current.column_id {
            ensure_column(&state, target).await?;
            position = Some(db::next_link_position(&state.db, target).await?);
            column_id = target;
        }
    }

    let changed = db::update_link(
        &state.db,
        id,
        &title,
        &new_url,
        &note,
        icon.as_deref(),
        column_id,
        position,
    )
    .await?;
    ensure_affected(changed)?;
    Ok(Json(json!({ "ok": true })))
}

#[worker::send]
async fn delete_link(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<Value>> {
    let changed = db::delete_link(&state.db, id).await?;
    ensure_affected(changed)?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct ArrangeLinks {
    columns: Vec<ArrangeColumn>,
}

#[derive(Deserialize)]
struct ArrangeColumn {
    column_id: i64,
    link_ids: Vec<i64>,
}

#[worker::send]
async fn arrange_links(
    State(state): State<AppState>,
    Json(body): Json<ArrangeLinks>,
) -> AppResult<Json<Value>> {
    let total: usize = body.columns.iter().map(|column| column.link_ids.len()).sum();
    if body.columns.len() > 500 || total > 5000 {
        return Err(AppError::bad_request("排序数据过多"));
    }
    let mut columns = Vec::with_capacity(body.columns.len());
    for column in body.columns {
        ensure_column(&state, column.column_id).await?;
        columns.push((column.column_id, column.link_ids));
    }
    db::arrange_links(&state.db, &columns).await?;
    Ok(Json(json!({ "ok": true })))
}

/* ------------------------------------------------------------------ */
/* Metadata & favicons                                                 */
/* ------------------------------------------------------------------ */

#[derive(Deserialize)]
struct MetaQuery {
    url: String,
}

#[worker::send]
async fn meta_handler(
    State(state): State<AppState>,
    Query(query): Query<MetaQuery>,
) -> AppResult<Json<Value>> {
    let url = normalize_url(&query.url).map_err(AppError::bad_request)?;
    let meta = fetch::fetch_meta(&url, state.cfg.allow_private_network)
        .await
        .map_err(|error| {
            worker::console_warn!("metadata fetch failed: {} {}", url, error.message);
            AppError::bad_request("无法抓取该网址的信息（内网地址或访问失败）")
        })?;
    Ok(Json(json!({
        "title": meta.title,
        "icon": meta.icon,
        "url": url,
        "host": host_label(&url),
    })))
}

#[derive(Deserialize)]
struct IconQuery {
    u: String,
}

#[worker::send]
async fn favicon_handler(
    State(state): State<AppState>,
    Query(query): Query<IconQuery>,
) -> AppResult<Response> {
    crate::favicon::serve(&state, &query.u).await
}

/* ------------------------------------------------------------------ */
/* Import / export                                                     */
/* ------------------------------------------------------------------ */

#[worker::send]
async fn import_json(State(state): State<AppState>, body: String) -> AppResult<Json<Value>> {
    let boards = tabnest_core::import::parse_json(&body)
        .map_err(|error| AppError::bad_request(format!("JSON 解析失败：{error}")))?;
    if boards.len() > 200 {
        return Err(AppError::bad_request("面板数量过多（最多 200 个）"));
    }
    if boards.iter().map(|board| board.columns.len()).sum::<usize>() > 2000 {
        return Err(AppError::bad_request("分组数量过多（最多 2000 个）"));
    }
    let stats = db::import_boards(&state.db, boards).await?;
    Ok(Json(json!({ "ok": true, "stats": stats })))
}

#[worker::send]
async fn import_html(State(state): State<AppState>, body: String) -> AppResult<Json<Value>> {
    if body.len() > 20 * 1024 * 1024 {
        return Err(AppError::bad_request("文件太大"));
    }
    let boards = tabnest_core::import::parse_netscape(&body);
    if boards.len() > 500 {
        return Err(AppError::bad_request("面板数量过多（最多 500 个）"));
    }
    let stats = db::import_boards(&state.db, boards).await?;
    Ok(Json(json!({ "ok": true, "stats": stats })))
}

#[worker::send]
async fn export_json(State(state): State<AppState>) -> AppResult<Response> {
    let snapshot = db::load_state(&state.db).await?;
    let exported_at = auth::now_ms() / 1000;

    let payload = json!({
        "app": "tabnest",
        "version": state.cfg.version,
        "exported_at": exported_at,
        "boards": snapshot.boards.iter().map(|board| json!({
            "name": board.name,
            "columns": board.columns.iter().map(|column| json!({
                "name": column.name,
                "links": column.links.iter().map(|link| json!({
                    "title": link.title,
                    "url": link.url,
                    "note": link.note,
                    "icon": link.icon,
                })).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    });

    Ok(attachment(
        serde_json::to_string_pretty(&payload)?,
        "application/json; charset=utf-8",
        "attachment; filename=\"tabnest-bookmarks.json\"",
    ))
}

#[worker::send]
async fn export_html(State(state): State<AppState>) -> AppResult<Response> {
    let snapshot = db::load_state(&state.db).await?;
    let mut html = String::from(
        "<!DOCTYPE NETSCAPE-Bookmark-file-1>\n\
         <!-- This is an automatically generated file. -->\n\
         <META HTTP-EQUIV=\"Content-Type\" CONTENT=\"text/html; charset=UTF-8\">\n\
         <TITLE>Bookmarks</TITLE>\n\
         <H1>Tabnest Bookmarks</H1>\n\
         <DL><p>\n",
    );
    for board in &snapshot.boards {
        html.push_str(&format!(
            "    <DT><H3>{}</H3>\n    <DL><p>\n",
            escape_html(&board.name)
        ));
        for column in &board.columns {
            html.push_str(&format!(
                "        <DT><H3>{}</H3>\n        <DL><p>\n",
                escape_html(&column.name)
            ));
            for link in &column.links {
                html.push_str(&format!(
                    "            <DT><A HREF=\"{}\">{}</A>\n",
                    escape_attr(&link.url),
                    escape_html(&link.title)
                ));
                if !link.note.is_empty() {
                    html.push_str(&format!("            <DD>{}\n", escape_html(&link.note)));
                }
            }
            html.push_str("        </DL><p>\n");
        }
        html.push_str("    </DL><p>\n");
    }
    html.push_str("</DL><p>\n");

    Ok(attachment(
        html,
        "text/html; charset=utf-8",
        "attachment; filename=\"tabnest-bookmarks.html\"",
    ))
}

fn attachment(body: String, content_type: &'static str, disposition: &'static str) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static(disposition),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    (headers, body).into_response()
}

/* ------------------------------------------------------------------ */
/* Helpers                                                             */
/* ------------------------------------------------------------------ */

fn clean_name(raw: &str, label: &str) -> AppResult<String> {
    let cleaned: String = raw
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect();
    let name = cleaned.trim();
    if name.is_empty() {
        return Err(AppError::bad_request(format!("{label}不能为空")));
    }
    if name.chars().count() > 60 {
        return Err(AppError::bad_request(format!("{label}最多 60 个字符")));
    }
    Ok(name.to_string())
}

fn clean_icon(icon: Option<String>) -> Option<String> {
    icon.map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(|value| truncate_chars(&value, 2048))
}

async fn ensure_board(state: &AppState, id: i64) -> AppResult<()> {
    if db::board_exists(&state.db, id).await? {
        Ok(())
    } else {
        Err(AppError::not_found("面板不存在"))
    }
}

async fn ensure_column(state: &AppState, id: i64) -> AppResult<()> {
    if db::column_exists(&state.db, id).await? {
        Ok(())
    } else {
        Err(AppError::not_found("分组不存在"))
    }
}

fn ensure_affected(rows: i64) -> AppResult<()> {
    if rows == 0 {
        Err(AppError::not_found("记录不存在"))
    } else {
        Ok(())
    }
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn escape_attr(value: &str) -> String {
    escape_html(value).replace('\'', "&#39;")
}
