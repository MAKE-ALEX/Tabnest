//! Workers 版的 HTTP API。
//!
//! 与原生服务端的 `src/api.rs` 对应：路由、校验规则和返回的 JSON 结构完全一致，
//! 区别只在存储层（D1 代替 `sqlx`）、会话（签名 Cookie 代替内存表）和
//! 图标缓存（浏览器 / CDN 代替本地目录）。

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
use tabnest_core::dto::{LayoutUpdate, SettingsDto, SettingsUpdate};
use tabnest_core::html::truncate_chars;
use tabnest_core::url::{host_label, normalize_url};

use crate::{
    auth, db,
    error::{AppError, AppResult},
    fetch, security,
    state::AppState,
};

/// 构造 Worker 的 API 路由。
///
/// `/healthz` 与登录相关接口公开；其余路由统一走鉴权中间件。
/// 最外层依次叠加请求体上限、CSRF 校验和安全响应头。
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
        .route(
            "/api/settings",
            get(settings_handler).put(save_settings_handler),
        )
        .route("/api/settings/layouts/{id}", put(save_layout_handler))
        .route("/api/import/json", post(import_json))
        .route("/api/import/html", post(import_html))
        .route("/api/export/json", get(export_json))
        .route("/api/export/html", get(export_html))
        // 这一层只作用于上面的受保护路由。
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

/// 健康检查端点。
async fn health() -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    (headers, "ok").into_response()
}

/* ------------------------------------------------------------------ */
/* 会话                                                                */
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

/// 登录：限流检查、逐字节比较密码、成功后签发签名 Cookie。
#[worker::send]
async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<LoginRequest>,
) -> AppResult<Response> {
    // 没有配置密码时直接放行。
    let Some(expected) = state.cfg.password.as_deref() else {
        return Ok(Json(json!({ "ok": true })).into_response());
    };

    let ip = auth::client_ip(&headers);
    // 超长密码直接拒绝，避免把巨大输入喂给比较逻辑。
    if body.password.len() > 256 {
        return Err(AppError::unauthorized("密码不正确"));
    }

    let now = auth::now_ms();
    // 先查该 IP 是否处于锁定期。
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

    // 逐字节 XOR 比较，不提前返回；长度差异本身已能从请求观察，
    // 所以长度不等时单独判断即可。
    let candidate = body.password.trim();
    let mut diff = 0u8;
    for (left, right) in candidate.bytes().zip(expected.bytes()) {
        diff |= left ^ right;
    }
    if diff != 0 || candidate.len() != expected.len() {
        // 失败后记录一次，触发锁定时返回 429。
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

    // 登录成功，清空该 IP 的失败计数。
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

/// 构造 429 响应，并带上标准的 `Retry-After` 头。
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

/// 注销。Worker 的会话是无状态的，服务端无需删除记录，只要让浏览器清 Cookie。
#[worker::send]
async fn logout(State(state): State<AppState>) -> Response {
    let mut response = Json(json!({ "ok": true })).into_response();
    response
        .headers_mut()
        .insert(header::SET_COOKIE, auth::clear_cookie(&state));
    response
}

/* ------------------------------------------------------------------ */
/* 面板                                                                */
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

/// 新建面板，追加到列表末尾。
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

/// 重命名面板。
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

/// 删除面板；D1 的外键级联会一并删掉分组和书签。
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

/// 按传入顺序重排面板（`ids` 的下标即新位置）。
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
/* 分组                                                                */
/* ------------------------------------------------------------------ */

#[derive(Deserialize)]
struct CreateColumn {
    board_id: i64,
    #[serde(default)]
    name: String,
}

/// 在指定面板下新建分组；名称为空时使用默认名。
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

/// 重命名分组。
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

/// 删除分组及其下的书签。
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

/// 重排分组；同时把每组挂到 `board_id` 下（支持跨面板拖动）。
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
/* 书签                                                                */
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

/// 新建书签。
///
/// 标题或图标缺失时尝试抓取网页元信息补齐；抓取失败不影响保存，
/// 用域名兜底标题。
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
        // Worker 侧由 fetch 自身超时控制，失败一律当作「没有元信息」。
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

/// 更新书签，支持部分字段更新（只改传入的字段）。
///
/// 换分组时追加到目标分组末尾；改网址且没给新图标时清掉旧图标。
#[worker::send]
async fn update_link(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateLink>,
) -> AppResult<Json<Value>> {
    let Some(current) = db::get_link(&state.db, id).await? else {
        return Err(AppError::not_found("书签不存在"));
    };

    // 网址字段：传了非空值就规范化，否则沿用旧值。
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
    // 图标：显式传了就更新；改网址则清空；否则保留原值。
    let icon = match clean_icon(body.icon) {
        Some(icon) => Some(icon),
        None if url_changed => None,
        None => current.icon.clone(),
    };

    // 只有跨分组移动时才需要重算 position。
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

/// 删除书签。
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

/// 批量调整书签所属分组和顺序（拖拽结束后调用）。
#[worker::send]
async fn arrange_links(
    State(state): State<AppState>,
    Json(body): Json<ArrangeLinks>,
) -> AppResult<Json<Value>> {
    let total: usize = body.columns.iter().map(|column| column.link_ids.len()).sum();
    if body.columns.len() > 500 || total > 5000 {
        return Err(AppError::bad_request("排序数据过多"));
    }
    // 先逐个确认分组存在，再一次性写库，避免写到一半才发现目标不存在。
    let mut columns = Vec::with_capacity(body.columns.len());
    for column in body.columns {
        ensure_column(&state, column.column_id).await?;
        columns.push((column.column_id, column.link_ids));
    }
    db::arrange_links(&state.db, &columns).await?;
    Ok(Json(json!({ "ok": true })))
}

/* ------------------------------------------------------------------ */
/* 元信息与图标                                                        */
/* ------------------------------------------------------------------ */

#[derive(Deserialize)]
struct MetaQuery {
    url: String,
}

/// 抓取网页标题和图标。
#[worker::send]
async fn meta_handler(
    State(state): State<AppState>,
    Query(query): Query<MetaQuery>,
) -> AppResult<Json<Value>> {
    let url = normalize_url(&query.url).map_err(AppError::bad_request)?;
    let meta = fetch::fetch_meta(&url, state.cfg.allow_private_network)
        .await
        .map_err(|error| {
            // 对内网地址的拒绝也走这里，日志里保留真实原因。
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

/// 书签图标，交给 favicon 模块处理缓存与解析。
#[worker::send]
async fn favicon_handler(
    State(state): State<AppState>,
    Query(query): Query<IconQuery>,
) -> AppResult<Response> {
    crate::favicon::serve(&state, &query.u).await
}

/* ------------------------------------------------------------------ */
/* 设置（多设备同步）                                                  */
/* ------------------------------------------------------------------ */

#[worker::send]
async fn settings_handler(State(state): State<AppState>) -> AppResult<Json<SettingsDto>> {
    Ok(Json(db::load_settings(&state.db).await?))
}

/// 保存外观设置；只覆盖请求里出现的字段。
#[worker::send]
async fn save_settings_handler(
    State(state): State<AppState>,
    Json(body): Json<SettingsUpdate>,
) -> AppResult<Json<Value>> {
    db::save_settings(&state.db, body).await?;
    Ok(Json(json!({ "ok": true })))
}

/// 保存某个面板的瀑布流排版。
#[worker::send]
async fn save_layout_handler(
    State(state): State<AppState>,
    Path(board_id): Path<i64>,
    Json(body): Json<LayoutUpdate>,
) -> AppResult<Json<Value>> {
    if !db::save_layout(&state.db, board_id, &body.layout).await? {
        return Err(AppError::not_found("面板不存在"));
    }
    Ok(Json(json!({ "ok": true })))
}

/* ------------------------------------------------------------------ */
/* 导入 / 导出                                                         */
/* ------------------------------------------------------------------ */

/// 导入 Tabnest 自己的 JSON 备份。
#[worker::send]
async fn import_json(State(state): State<AppState>, body: String) -> AppResult<Json<Value>> {
    let tabnest_core::import::ImportPayload {
        boards,
        appearance,
    } = tabnest_core::import::parse_json(&body)
        .map_err(|error| AppError::bad_request(format!("JSON 解析失败：{error}")))?;
    // 写库之前先做上限检查。
    if boards.len() > 200 {
        return Err(AppError::bad_request("面板数量过多（最多 200 个）"));
    }
    if boards.iter().map(|board| board.columns.len()).sum::<usize>() > 2000 {
        return Err(AppError::bad_request("分组数量过多（最多 2000 个）"));
    }
    let (stats, imported) = db::import_boards(&state.db, boards).await?;

    // 还原每个面板的排版：导出文件里是分组「名称」，这里映射到本次导入
    // 新分配的分组 id 上。
    for board in &imported {
        let Some(layout) = board.layout.as_ref() else {
            continue;
        };
        let resolved = tabnest_core::import::resolve_layout(layout, &board.columns);
        if resolved.iter().all(Vec::is_empty) {
            continue;
        }
        // 排版还原失败只记日志，不影响书签导入。
        if let Err(error) = db::save_layout(&state.db, board.id, &resolved).await {
            worker::console_warn!("layout restore failed: {}", error.message);
        }
    }

    // 外观设置同理：尽力还原，失败不报错。
    if let Some(appearance) = appearance {
        let update = SettingsUpdate {
            theme: appearance.theme,
            palette: appearance.palette,
            columns: appearance.columns,
            active_board: None,
        };
        if let Err(error) = db::save_settings(&state.db, update).await {
            worker::console_warn!("appearance restore failed: {}", error.message);
        }
    }

    let imported_ids: Vec<i64> = imported.iter().map(|board| board.id).collect();

    Ok(Json(
        json!({ "ok": true, "stats": stats, "boards": imported_ids }),
    ))
}

/// 导入浏览器导出的 Netscape 书签 HTML。
#[worker::send]
async fn import_html(State(state): State<AppState>, body: String) -> AppResult<Json<Value>> {
    if body.len() > 20 * 1024 * 1024 {
        return Err(AppError::bad_request("文件太大"));
    }
    // 先解析 Tabnest 附加的排版注释，再解析书签树。
    let meta = tabnest_core::import::parse_html_meta(&body);
    let boards = tabnest_core::import::parse_netscape(&body);
    if boards.len() > 500 {
        return Err(AppError::bad_request("面板数量过多（最多 500 个）"));
    }
    let (stats, imported) = db::import_boards(&state.db, boards).await?;

    // 还原排版与外观，失败仅记录日志。
    if let Some(meta) = meta {
        for (board_id, layout) in tabnest_core::import::match_layouts(&meta, &imported) {
            if let Err(error) = db::save_layout(&state.db, board_id, &layout).await {
                worker::console_warn!("html layout restore failed: {}", error.message);
            }
        }
        if let Some(appearance) = meta.appearance {
            let update = SettingsUpdate {
                theme: appearance.theme,
                palette: appearance.palette,
                columns: appearance.columns,
                active_board: None,
            };
            if let Err(error) = db::save_settings(&state.db, update).await {
                worker::console_warn!("html appearance restore failed: {}", error.message);
            }
        }
    }

    let imported_ids: Vec<i64> = imported.iter().map(|board| board.id).collect();

    Ok(Json(
        json!({ "ok": true, "stats": stats, "boards": imported_ids }),
    ))
}

/// 导出完整备份为 JSON 文件。
#[worker::send]
async fn export_json(State(state): State<AppState>) -> AppResult<Response> {
    let snapshot = db::load_state(&state.db).await?;
    let settings = db::load_settings(&state.db).await?;
    let exported_at = auth::now_ms() / 1000;

    let boards: Vec<Value> = snapshot
        .boards
        .iter()
        .map(|board| {
            let names: std::collections::HashMap<i64, &str> = board
                .columns
                .iter()
                .map(|column| (column.id, column.name.as_str()))
                .collect();
            // 排版按分组「名称」导出：导入到别处时 id 会重新分配。
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

/// 导出为浏览器通用的 Netscape 书签 HTML。
#[worker::send]
async fn export_html(State(state): State<AppState>) -> AppResult<Response> {
    let snapshot = db::load_state(&state.db).await?;
    let settings = db::load_settings(&state.db).await?;
    let mut html = String::from(
        "<!DOCTYPE NETSCAPE-Bookmark-file-1>\n\
         <!-- This is an automatically generated file. -->\n\
         <META HTTP-EQUIV=\"Content-Type\" CONTENT=\"text/html; charset=UTF-8\">\n\
         <TITLE>Bookmarks</TITLE>\n\
         <H1>Tabnest Bookmarks</H1>\n",
    );

    // Netscape 格式表达不了瀑布流排版，于是把排版信息塞进一段 base64 注释：
    // 浏览器导入时忽略，Tabnest 再次导入时能读回来。
    let meta = tabnest_core::import::LayoutMeta {
        appearance: Some(tabnest_core::import::ImportAppearance {
            theme: settings.theme.clone(),
            palette: settings.palette.clone(),
            columns: settings.columns.clone(),
            active_board: settings.active_board,
        }),
        boards: snapshot
            .boards
            .iter()
            .map(|board| {
                let names: std::collections::HashMap<i64, &str> = board
                    .columns
                    .iter()
                    .map(|column| (column.id, column.name.as_str()))
                    .collect();
                tabnest_core::import::BoardLayoutMeta {
                    name: board.name.clone(),
                    layout: settings
                        .layouts
                        .get(&board.id)
                        .map(|groups| {
                            groups
                                .iter()
                                .map(|group| {
                                    group
                                        .iter()
                                        .filter_map(|id| names.get(id).map(|name| name.to_string()))
                                        .collect()
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                }
            })
            .collect(),
    };
    html.push_str(&meta.to_comment());
    html.push_str("\n<DL><p>\n");

    // 书签层级：面板 -> 分组 -> 书签（备注写成 <DD>）。
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

/// 构造下载响应：指定文件名并禁止缓存。
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
/* 辅助函数                                                            */
/* ------------------------------------------------------------------ */

/// 清洗名称：控制字符替换为空格、去掉首尾空白，校验非空且不超过 60 字符。
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

/// 清洗图标字段：去空白、空串视为没有，最长 2048 字符。
fn clean_icon(icon: Option<String>) -> Option<String> {
    icon.map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(|value| truncate_chars(&value, 2048))
}

/// 确认面板存在，否则返回 404。
async fn ensure_board(state: &AppState, id: i64) -> AppResult<()> {
    if db::board_exists(&state.db, id).await? {
        Ok(())
    } else {
        Err(AppError::not_found("面板不存在"))
    }
}

/// 确认分组存在，否则返回 404。
async fn ensure_column(state: &AppState, id: i64) -> AppResult<()> {
    if db::column_exists(&state.db, id).await? {
        Ok(())
    } else {
        Err(AppError::not_found("分组不存在"))
    }
}

/// 更新 / 删除影响 0 行时视为目标不存在，返回 404。
fn ensure_affected(rows: i64) -> AppResult<()> {
    if rows == 0 {
        Err(AppError::not_found("记录不存在"))
    } else {
        Ok(())
    }
}

/// HTML 文本转义。
fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// HTML 属性值转义（在文本转义基础上再处理单引号）。
fn escape_attr(value: &str) -> String {
    escape_html(value).replace('\'', "&#39;")
}
