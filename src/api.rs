use std::{
    collections::HashMap,
    net::SocketAddr,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
    extract::{ConnectInfo, DefaultBodyLimit, Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware,
    response::{IntoResponse, Response},
    routing::{get, patch, post, put},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sha1::{Digest, Sha1};
use sqlx::SqlitePool;

use crate::{
    auth::{self, clear_cookie, constant_time_eq, session_cookie, LoginCheck},
    error::{AppError, AppResult},
    fetch::{
        extract_icon, fetch_limited, fetch_meta, host_label, normalize_url, sniff_image,
        truncate_chars,
    },
    import,
    state::AppState,
};

pub fn router(state: AppState) -> Router {
    let protected = Router::new()
        .route("/api/state", get(state_handler))
        .route("/api/boards", post(create_board))
        .route("/api/boards/order", put(order_boards))
        .route(
            "/api/boards/{id}",
            patch(update_board).delete(delete_board),
        )
        .route("/api/columns", post(create_column))
        .route("/api/columns/order", put(order_columns))
        .route(
            "/api/columns/{id}",
            patch(update_column).delete(delete_column),
        )
        .route("/api/links", post(create_link))
        .route("/api/links/arrange", put(arrange_links))
        .route("/api/links/{id}", patch(update_link).delete(delete_link))
        .route("/api/meta", get(meta_handler))
        .route("/api/favicon", get(favicon_handler))
        .route(
            "/api/settings",
            get(settings_handler).put(save_settings_handler),
        )
        .route(
            "/api/settings/layouts/{id}",
            put(save_layout_handler),
        )
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
        .merge(protected)
        .layer(DefaultBodyLimit::max(25 * 1024 * 1024))
        .layer(middleware::from_fn(crate::security::csrf_guard))
        .with_state(state)
}

/* ------------------------------------------------------------------ */
/* DTOs                                                                */
/* ------------------------------------------------------------------ */

pub use tabnest_core::dto::{BoardDto, ColumnDto, LinkDto, SettingsDto, StateDto};
use tabnest_core::dto::{LayoutUpdate, SettingsUpdate};

pub async fn load_state(pool: &SqlitePool) -> AppResult<StateDto> {
    let boards: Vec<(i64, String, i64)> =
        sqlx::query_as("SELECT id, name, position FROM boards ORDER BY position ASC, id ASC")
            .fetch_all(pool)
            .await?;
    let columns: Vec<(i64, i64, String, i64)> = sqlx::query_as(
        "SELECT id, board_id, name, position FROM columns ORDER BY position ASC, id ASC",
    )
    .fetch_all(pool)
    .await?;
    let links: Vec<(i64, i64, String, String, String, Option<String>, i64, String)> =
        sqlx::query_as(
            "SELECT id, column_id, title, url, note, icon, position, created_at
             FROM links ORDER BY position ASC, id ASC",
        )
        .fetch_all(pool)
        .await?;

    let mut board_index: HashMap<i64, usize> = HashMap::new();
    let mut column_index: HashMap<i64, (usize, usize)> = HashMap::new();
    let mut result = StateDto {
        boards: Vec::with_capacity(boards.len()),
    };

    for (id, name, position) in boards {
        board_index.insert(id, result.boards.len());
        result.boards.push(BoardDto {
            id,
            name,
            position,
            columns: Vec::new(),
        });
    }
    for (id, board_id, name, position) in columns {
        if let Some(&index) = board_index.get(&board_id) {
            let column_position = result.boards[index].columns.len();
            result.boards[index].columns.push(ColumnDto {
                id,
                board_id,
                name,
                position,
                links: Vec::new(),
            });
            column_index.insert(id, (index, column_position));
        }
    }
    for (id, column_id, title, url, note, icon, position, created_at) in links {
        if let Some(&(board, column)) = column_index.get(&column_id) {
            result.boards[board].columns[column].links.push(LinkDto {
                id,
                column_id,
                title,
                url,
                note,
                icon,
                position,
                created_at,
            });
        }
    }
    Ok(result)
}

/* ------------------------------------------------------------------ */
/* Session                                                             */
/* ------------------------------------------------------------------ */

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

async fn login(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<LoginRequest>,
) -> AppResult<Response> {
    let Some(expected) = state.cfg.password.as_deref() else {
        return Ok(Json(json!({ "ok": true })).into_response());
    };

    let ip = auth::client_ip(
        peer,
        &headers,
        &state.cfg.trusted_proxies,
        state.cfg.trust_proxy,
    );
    if body.password.len() > 256 {
        return Err(AppError::unauthorized("密码不正确"));
    }
    let lockout = {
        let Ok(mut limiter) = state.login_limiter.lock() else {
            return Err(AppError::internal("登录限速器不可用"));
        };
        match limiter.check(ip) {
            LoginCheck::Allowed => None,
            LoginCheck::Locked(wait) => Some(wait),
        }
    };
    if let Some(wait) = lockout {
        return Ok(too_many_response(wait));
    }

    if !constant_time_eq(body.password.trim(), expected) {
        let locked_for = state
            .login_limiter
            .lock()
            .ok()
            .and_then(|mut limiter| limiter.record_failure(ip));
        tokio::time::sleep(Duration::from_millis(300)).await;
        if let Some(wait) = locked_for {
            return Ok(too_many_response(wait));
        }
        return Err(AppError::unauthorized("密码不正确"));
    }

    if let Ok(mut limiter) = state.login_limiter.lock() {
        limiter.record_success(ip);
    }

    let token = auth::create_session(&state);
    let mut response = Json(json!({ "ok": true })).into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        session_cookie(&token, state.cfg.cookie_secure),
    );
    Ok(response)
}

fn too_many_response(retry_after: Duration) -> Response {
    let seconds = retry_after.as_secs().max(1);
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

async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(token) = auth::extract_cookie(
        &headers,
        auth::cookie_name(state.cfg.cookie_secure),
    ) {
        auth::drop_session(&state, &token);
    }
    let mut response = Json(json!({ "ok": true })).into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        clear_cookie(state.cfg.cookie_secure),
    );
    response
}

/* ------------------------------------------------------------------ */
/* Boards                                                              */
/* ------------------------------------------------------------------ */

async fn state_handler(State(state): State<AppState>) -> AppResult<Json<StateDto>> {
    Ok(Json(load_state(&state.pool).await?))
}

#[derive(Deserialize)]
struct CreateBoard {
    name: String,
}

async fn create_board(
    State(state): State<AppState>,
    Json(body): Json<CreateBoard>,
) -> AppResult<Json<Value>> {
    let name = clean_name(&body.name, "面板名称")?;
    let position: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(position), -1) + 1 FROM boards")
        .fetch_one(&state.pool)
        .await?;
    let result = sqlx::query("INSERT INTO boards (name, position) VALUES (?, ?)")
        .bind(&name)
        .bind(position)
        .execute(&state.pool)
        .await?;
    Ok(Json(json!({ "ok": true, "id": result.last_insert_rowid() })))
}

#[derive(Deserialize)]
struct UpdateBoard {
    name: String,
}

async fn update_board(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateBoard>,
) -> AppResult<Json<Value>> {
    let name = clean_name(&body.name, "面板名称")?;
    let result = sqlx::query("UPDATE boards SET name = ? WHERE id = ?")
        .bind(&name)
        .bind(id)
        .execute(&state.pool)
        .await?;
    ensure_affected(result.rows_affected())?;
    Ok(Json(json!({ "ok": true })))
}

async fn delete_board(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<Value>> {
    let result = sqlx::query("DELETE FROM boards WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await?;
    ensure_affected(result.rows_affected())?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct OrderBoards {
    ids: Vec<i64>,
}

async fn order_boards(
    State(state): State<AppState>,
    Json(body): Json<OrderBoards>,
) -> AppResult<Json<Value>> {
    if body.ids.len() > 1000 {
        return Err(AppError::bad_request("排序数据过多"));
    }
    let mut tx = state.pool.begin().await?;
    for (index, id) in body.ids.iter().enumerate() {
        sqlx::query("UPDATE boards SET position = ? WHERE id = ?")
            .bind(index as i64)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
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

async fn create_column(
    State(state): State<AppState>,
    Json(body): Json<CreateColumn>,
) -> AppResult<Json<Value>> {
    ensure_board(&state.pool, body.board_id).await?;
    let name = if body.name.trim().is_empty() {
        "新分组".to_string()
    } else {
        clean_name(&body.name, "分组名称")?
    };
    let position: i64 =
        sqlx::query_scalar("SELECT COALESCE(MAX(position), -1) + 1 FROM columns WHERE board_id = ?")
            .bind(body.board_id)
            .fetch_one(&state.pool)
            .await?;
    let result =
        sqlx::query("INSERT INTO columns (board_id, name, position) VALUES (?, ?, ?)")
            .bind(body.board_id)
            .bind(&name)
            .bind(position)
            .execute(&state.pool)
            .await?;
    Ok(Json(json!({ "ok": true, "id": result.last_insert_rowid() })))
}

#[derive(Deserialize)]
struct UpdateColumn {
    name: String,
}

async fn update_column(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateColumn>,
) -> AppResult<Json<Value>> {
    let name = clean_name(&body.name, "分组名称")?;
    let result = sqlx::query("UPDATE columns SET name = ? WHERE id = ?")
        .bind(&name)
        .bind(id)
        .execute(&state.pool)
        .await?;
    ensure_affected(result.rows_affected())?;
    Ok(Json(json!({ "ok": true })))
}

async fn delete_column(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<Value>> {
    let result = sqlx::query("DELETE FROM columns WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await?;
    ensure_affected(result.rows_affected())?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct OrderColumns {
    board_id: i64,
    ids: Vec<i64>,
}

async fn order_columns(
    State(state): State<AppState>,
    Json(body): Json<OrderColumns>,
) -> AppResult<Json<Value>> {
    if body.ids.len() > 1000 {
        return Err(AppError::bad_request("排序数据过多"));
    }
    ensure_board(&state.pool, body.board_id).await?;
    let mut tx = state.pool.begin().await?;
    for (index, id) in body.ids.iter().enumerate() {
        sqlx::query("UPDATE columns SET position = ?, board_id = ? WHERE id = ?")
            .bind(index as i64)
            .bind(body.board_id)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
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

async fn create_link(
    State(state): State<AppState>,
    Json(body): Json<CreateLink>,
) -> AppResult<Json<Value>> {
    let url = normalize_url(&body.url).map_err(AppError::bad_request)?;
    ensure_column(&state.pool, body.column_id).await?;

    let mut title = body.title.trim().to_string();
    let mut icon = clean_icon(body.icon);

    if title.is_empty() || icon.is_none() {
        let meta = tokio::time::timeout(
            Duration::from_secs(6),
            fetch_meta(&url, state.cfg.allow_private_network),
        )
        .await
        .ok()
        .and_then(|result| result.ok())
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

    let position: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(position), -1) + 1 FROM links WHERE column_id = ?",
    )
    .bind(body.column_id)
    .fetch_one(&state.pool)
    .await?;

    let result = sqlx::query(
        "INSERT INTO links (column_id, title, url, note, icon, position) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(body.column_id)
    .bind(&title)
    .bind(&url)
    .bind(&note)
    .bind(icon)
    .bind(position)
    .execute(&state.pool)
    .await?;

    Ok(Json(json!({
        "ok": true,
        "id": result.last_insert_rowid(),
        "title": title,
    })))
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

async fn update_link(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateLink>,
) -> AppResult<Json<Value>> {
    let current: Option<(String, Option<String>, i64)> =
        sqlx::query_as("SELECT url, icon, column_id FROM links WHERE id = ?")
            .bind(id)
            .fetch_optional(&state.pool)
            .await?;
    let Some((old_url, old_icon, old_column)) = current else {
        return Err(AppError::not_found("书签不存在"));
    };

    let new_url = match body.url.as_deref() {
        Some(raw) if !raw.trim().is_empty() => {
            normalize_url(raw).map_err(AppError::bad_request)?
        }
        _ => old_url.clone(),
    };
    let url_changed = new_url != old_url;

    let current_title: String = sqlx::query_scalar("SELECT title FROM links WHERE id = ?")
        .bind(id)
        .fetch_one(&state.pool)
        .await?;
    let current_note: String = sqlx::query_scalar("SELECT note FROM links WHERE id = ?")
        .bind(id)
        .fetch_one(&state.pool)
        .await?;

    let title = match body.title {
        Some(title) if !title.trim().is_empty() => truncate_chars(title.trim(), 300),
        _ => current_title,
    };
    let note = match body.note {
        Some(note) => truncate_chars(note.trim(), 2000),
        None => current_note,
    };
    let icon = match clean_icon(body.icon) {
        Some(icon) => Some(icon),
        None if url_changed => None,
        None => old_icon,
    };

    let mut column_id = old_column;
    let mut position: Option<i64> = None;
    if let Some(target) = body.column_id {
        if target != old_column {
            ensure_column(&state.pool, target).await?;
            let next: i64 = sqlx::query_scalar(
                "SELECT COALESCE(MAX(position), -1) + 1 FROM links WHERE column_id = ?",
            )
            .bind(target)
            .fetch_one(&state.pool)
            .await?;
            column_id = target;
            position = Some(next);
        }
    }

    if let Some(position) = position {
        sqlx::query(
            "UPDATE links SET title = ?, url = ?, note = ?, icon = ?, column_id = ?, position = ?,
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?",
        )
        .bind(&title)
        .bind(&new_url)
        .bind(&note)
        .bind(&icon)
        .bind(column_id)
        .bind(position)
        .bind(id)
        .execute(&state.pool)
        .await?;
    } else {
        sqlx::query(
            "UPDATE links SET title = ?, url = ?, note = ?, icon = ?,
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?",
        )
        .bind(&title)
        .bind(&new_url)
        .bind(&note)
        .bind(&icon)
        .bind(id)
        .execute(&state.pool)
        .await?;
    }

    Ok(Json(json!({ "ok": true })))
}

async fn delete_link(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> AppResult<Json<Value>> {
    let result = sqlx::query("DELETE FROM links WHERE id = ?")
        .bind(id)
        .execute(&state.pool)
        .await?;
    ensure_affected(result.rows_affected())?;
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

async fn arrange_links(
    State(state): State<AppState>,
    Json(body): Json<ArrangeLinks>,
) -> AppResult<Json<Value>> {
    let total: usize = body.columns.iter().map(|column| column.link_ids.len()).sum();
    if body.columns.len() > 500 || total > 5000 {
        return Err(AppError::bad_request("排序数据过多"));
    }
    let mut tx = state.pool.begin().await?;
    for column in &body.columns {
        ensure_column(&state.pool, column.column_id).await?;
        for (index, link_id) in column.link_ids.iter().enumerate() {
            let result = sqlx::query("UPDATE links SET column_id = ?, position = ? WHERE id = ?")
                .bind(column.column_id)
                .bind(index as i64)
                .bind(link_id)
                .execute(&mut *tx)
                .await?;
            ensure_affected(result.rows_affected())?;
        }
    }
    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

/* ------------------------------------------------------------------ */
/* Metadata & favicons                                                 */
/* ------------------------------------------------------------------ */

#[derive(Deserialize)]
struct MetaQuery {
    url: String,
}

async fn meta_handler(
    State(state): State<AppState>,
    Query(query): Query<MetaQuery>,
) -> AppResult<Json<Value>> {
    let url = normalize_url(&query.url).map_err(AppError::bad_request)?;
    let meta = tokio::time::timeout(
        Duration::from_secs(8),
        fetch_meta(&url, state.cfg.allow_private_network),
    )
    .await
    .map_err(|_| AppError::bad_request("抓取网页信息超时"))?
    .map_err(|error| {
        tracing::warn!(%error, %url, "metadata fetch rejected or failed");
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

async fn favicon_handler(
    State(state): State<AppState>,
    Query(query): Query<IconQuery>,
) -> AppResult<Response> {
    let url = normalize_url(&query.u).map_err(AppError::bad_request)?;
    let digest = hex::encode(Sha1::digest(url.as_bytes()));
    let cache_dir = state.cfg.data_dir.join("favicons");
    let path = cache_dir.join(digest);

    if let Ok(bytes) = tokio::fs::read(&path).await {
        if let Some(content_type) = sniff_image(&bytes, "") {
            return Ok(image_response(bytes, content_type));
        }
        // Drop anything that is no longer an accepted raster image (for
        // example old SVG entries cached before the whitelist changed).
        let _ = tokio::fs::remove_file(&path).await;
    }

    let (bytes, kind) = resolve_icon(&url, state.cfg.allow_private_network)
        .await
        .map_err(|error| {
            tracing::debug!(%url, "favicon resolve failed: {}", error.message);
            error
        })?;
    if let Err(error) = tokio::fs::write(&path, &bytes).await {
        tracing::warn!(%error, "favicon cache write failed");
    } else {
        prune_favicon_cache(&cache_dir).await;
    }
    Ok(image_response(bytes, kind))
}

/// Turn whatever the caller passed into a raster icon.
///
/// Imported bookmarks have no icon URL, so the front-end sends the page URL
/// instead. In that case the HTML is parsed for `<link rel="icon">` (which is
/// often an SVG we refuse to serve) and `/favicon.ico` is the last resort.
async fn resolve_icon(url: &str, allow_private: bool) -> AppResult<(Vec<u8>, &'static str)> {
    let (bytes, content_type, final_url) = fetch_limited(url, 1_500_000, allow_private)
        .await
        .map_err(|_| AppError::not_found("图标不可用"))?;

    if let Some(kind) = sniff_image(&bytes, &content_type) {
        return Ok((bytes, kind));
    }

    if content_type.is_empty()
        || content_type.contains("html")
        || content_type.contains("text")
        || content_type.contains("xml")
    {
        let html = String::from_utf8_lossy(&bytes);
        if let Some(icon) = extract_icon(&html, &final_url) {
            if let Ok((bytes, kind)) = fetch_raster(&icon, allow_private).await {
                return Ok((bytes, kind));
            }
        }
    }

    if let Some(candidate) = origin_favicon(&final_url) {
        if candidate != url {
            if let Ok((bytes, kind)) = fetch_raster(&candidate, allow_private).await {
                return Ok((bytes, kind));
            }
        }
    }

    Err(AppError::not_found("图标不可用"))
}

async fn fetch_raster(url: &str, allow_private: bool) -> AppResult<(Vec<u8>, &'static str)> {
    let (bytes, content_type, _) = fetch_limited(url, 512 * 1024, allow_private)
        .await
        .map_err(|_| AppError::not_found("图标不可用"))?;
    match sniff_image(&bytes, &content_type) {
        Some(kind) => Ok((bytes, kind)),
        None => Err(AppError::not_found("图标不是有效的图片")),
    }
}

fn origin_favicon(page_url: &str) -> Option<String> {
    let parsed = url::Url::parse(page_url).ok()?;
    let origin = parsed.origin().ascii_serialization();
    if origin == "null" {
        return None;
    }
    Some(format!("{origin}/favicon.ico"))
}

const FAVICON_CACHE_MAX_FILES: usize = 1000;

/// Keep the on-disk favicon cache bounded. Called only on a cache miss.
async fn prune_favicon_cache(dir: &std::path::Path) {
    let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
        return;
    };
    let mut files: Vec<(SystemTime, PathBuf)> = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        if let Ok(metadata) = entry.metadata().await {
            if metadata.is_file() {
                let modified = metadata.modified().unwrap_or(UNIX_EPOCH);
                files.push((modified, entry.path()));
            }
        }
    }
    if files.len() <= FAVICON_CACHE_MAX_FILES {
        return;
    }
    files.sort_by_key(|(modified, _)| *modified);
    let remove_count = files.len() - FAVICON_CACHE_MAX_FILES;
    for (_, path) in files.into_iter().take(remove_count) {
        let _ = tokio::fs::remove_file(path).await;
    }
}

fn image_response(bytes: Vec<u8>, content_type: &'static str) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=604800, immutable"),
    );
    headers.append(header::VARY, HeaderValue::from_static("Cookie"));
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'none'; sandbox"),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("inline; filename=\"favicon\""),
    );
    (headers, bytes).into_response()
}

/* ------------------------------------------------------------------ */
/* Settings (shared across devices)                                    */
/* ------------------------------------------------------------------ */

async fn settings_handler(State(state): State<AppState>) -> AppResult<Json<SettingsDto>> {
    Ok(Json(crate::db::load_settings(&state.pool).await?))
}

async fn save_settings_handler(
    State(state): State<AppState>,
    Json(body): Json<SettingsUpdate>,
) -> AppResult<Json<Value>> {
    crate::db::save_settings(&state.pool, body).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn save_layout_handler(
    State(state): State<AppState>,
    Path(board_id): Path<i64>,
    Json(body): Json<LayoutUpdate>,
) -> AppResult<Json<Value>> {
    if !crate::db::save_layout(&state.pool, board_id, &body.layout).await? {
        return Err(AppError::not_found("面板不存在"));
    }
    Ok(Json(json!({ "ok": true })))
}

/* ------------------------------------------------------------------ */
/* Import / export                                                     */
/* ------------------------------------------------------------------ */

async fn import_json(State(state): State<AppState>, body: String) -> AppResult<Json<Value>> {
    let import::ImportPayload {
        boards,
        appearance,
    } = import::parse_json(&body)
        .map_err(|error| AppError::bad_request(format!("JSON 解析失败：{error}")))?;
    if boards.len() > 200 {
        return Err(AppError::bad_request("面板数量过多（最多 200 个）"));
    }
    if boards.iter().map(|board| board.columns.len()).sum::<usize>() > 2000 {
        return Err(AppError::bad_request("分组数量过多（最多 2000 个）"));
    }
    let (stats, imported) = crate::db::import_boards(&state.pool, boards).await?;

    // Restore the masonry layout of every board that carried one, mapping the
    // exported column names onto the ids assigned by this import.
    for board in &imported {
        let Some(layout) = board.layout.as_ref() else {
            continue;
        };
        let resolved = import::resolve_layout(layout, &board.columns);
        if resolved.iter().all(Vec::is_empty) {
            continue;
        }
        if let Err(error) = crate::db::save_layout(&state.pool, board.id, &resolved).await {
            tracing::warn!("layout restore failed: {}", error.message);
        }
    }

    if let Some(appearance) = appearance {
        let update = SettingsUpdate {
            theme: appearance.theme,
            palette: appearance.palette,
            columns: appearance.columns,
            active_board: None,
        };
        if let Err(error) = crate::db::save_settings(&state.pool, update).await {
            tracing::warn!("appearance restore failed: {}", error.message);
        }
    }

    Ok(Json(json!({ "ok": true, "stats": stats })))
}

async fn import_html(State(state): State<AppState>, body: String) -> AppResult<Json<Value>> {
    if body.len() > 20 * 1024 * 1024 {
        return Err(AppError::bad_request("文件太大"));
    }
    let boards = import::parse_netscape(&body);
    if boards.len() > 500 {
        return Err(AppError::bad_request("面板数量过多（最多 500 个）"));
    }
    let (stats, _) = crate::db::import_boards(&state.pool, boards).await?;
    Ok(Json(json!({ "ok": true, "stats": stats })))
}

async fn export_json(State(state): State<AppState>) -> AppResult<Response> {
    let snapshot = load_state(&state.pool).await?;
    let settings = crate::db::load_settings(&state.pool).await?;
    let exported_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);

    let boards: Vec<Value> = snapshot
        .boards
        .iter()
        .map(|board| {
            let names: HashMap<i64, &str> = board
                .columns
                .iter()
                .map(|column| (column.id, column.name.as_str()))
                .collect();
            // Layout is exported by column *name* because ids are reassigned
            // when the file is imported somewhere else.
            let layout: Option<Vec<Vec<String>>> = settings.layouts.get(&board.id).map(|groups| {
                groups
                    .iter()
                    .map(|group| {
                        group
                            .iter()
                            .filter_map(|id| names.get(id).map(|name| name.to_string()))
                            .collect()
                    })
                    .collect()
            });
            json!({
                "name": board.name,
                "layout": layout,
                "columns": board.columns.iter().map(|column| json!({
                    "name": column.name,
                    "links": column.links.iter().map(|link| json!({
                        "title": link.title,
                        "url": link.url,
                        "note": link.note,
                        "icon": link.icon,
                    })).collect::<Vec<_>>(),
                })).collect::<Vec<_>>(),
            })
        })
        .collect();

    let payload = json!({
        "app": "tabnest",
        "version": state.cfg.version,
        "exported_at": exported_at,
        "appearance": {
            "theme": settings.theme,
            "palette": settings.palette,
            "columns": settings.columns,
            "active_board": settings.active_board,
        },
        "boards": boards,
    });

    Ok(attachment(
        serde_json::to_string_pretty(&payload)?,
        "application/json; charset=utf-8",
        "attachment; filename=\"tabnest-bookmarks.json\"",
    ))
}

async fn export_html(State(state): State<AppState>) -> AppResult<Response> {
    let snapshot = load_state(&state.pool).await?;
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
                    html.push_str(&format!(
                        "            <DD>{}\n",
                        escape_html(&link.note)
                    ));
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

async fn ensure_board(pool: &SqlitePool, id: i64) -> AppResult<()> {
    let exists: Option<i64> = sqlx::query_scalar("SELECT id FROM boards WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    exists.map(|_| ()).ok_or_else(|| AppError::not_found("面板不存在"))
}

async fn ensure_column(pool: &SqlitePool, id: i64) -> AppResult<()> {
    let exists: Option<i64> = sqlx::query_scalar("SELECT id FROM columns WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    exists
        .map(|_| ())
        .ok_or_else(|| AppError::not_found("分组不存在"))
}

fn ensure_affected(rows: u64) -> AppResult<()> {
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
