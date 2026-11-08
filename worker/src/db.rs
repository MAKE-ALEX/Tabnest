//! D1 persistence layer.
//!
//! SQL mirrors the native `sqlx` queries in `src/api.rs` / `src/db.rs`; the
//! differences are D1's API shape: `RETURNING id` instead of
//! `last_insert_rowid()`, and `batch()` instead of interactive transactions.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use tabnest_core::dto::{BoardDto, ColumnDto, LinkDto, SettingsDto, SettingsUpdate, StateDto};
use tabnest_core::import::{ImportBoard, ImportedBoard};
use tabnest_core::settings::{sanitize_layout, valid_columns, valid_palette, valid_theme};
use worker::d1::{D1Database, D1PreparedStatement, D1Type};

use crate::error::{AppError, AppResult};

const MAX_LINKS: usize = 20_000;
const MAX_BOARDS: usize = 500;
const MAX_COLUMNS: usize = 2000;

fn dberr(error: worker::Error) -> AppError {
    worker::console_error!("d1 error: {error} ({error:?})");
    AppError::internal("数据库操作失败")
}

fn prepare<'a>(
    db: &D1Database,
    sql: &str,
    args: Vec<D1Type<'a>>,
) -> AppResult<D1PreparedStatement> {
    let statement = db.prepare(sql);
    if args.is_empty() {
        Ok(statement)
    } else {
        statement.bind_refs(&args).map_err(dberr)
    }
}

async fn query_all<T>(db: &D1Database, sql: &str, args: Vec<D1Type<'_>>) -> AppResult<Vec<T>>
where
    T: for<'de> Deserialize<'de>,
{
    let statement = prepare(db, sql, args)?;
    let result = statement.all().await.map_err(dberr)?;
    result.results::<T>().map_err(dberr)
}

async fn query_one<T>(db: &D1Database, sql: &str, args: Vec<D1Type<'_>>) -> AppResult<Option<T>>
where
    T: for<'de> Deserialize<'de>,
{
    let statement = prepare(db, sql, args)?;
    statement.first::<T>(None).await.map_err(dberr)
}

async fn execute(db: &D1Database, sql: &str, args: Vec<D1Type<'_>>) -> AppResult<i64> {
    let statement = prepare(db, sql, args)?;
    let result = statement.run().await.map_err(dberr)?;
    let changed = result.meta().ok().flatten().and_then(|meta| meta.changes);
    Ok(changed.unwrap_or(1) as i64)
}

async fn scalar_i64(db: &D1Database, sql: &str, args: Vec<D1Type<'_>>) -> AppResult<i64> {
    #[derive(Deserialize)]
    struct Value {
        value: i64,
    }
    let row: Option<Value> = query_one(db, sql, args).await?;
    Ok(row.map(|row| row.value).unwrap_or(0))
}

async fn insert_returning_id(db: &D1Database, sql: &str, args: Vec<D1Type<'_>>) -> AppResult<i64> {
    #[derive(Deserialize)]
    struct Id {
        id: i64,
    }
    let row: Option<Id> = query_one(db, sql, args).await?;
    row.map(|row| row.id)
        .ok_or_else(|| AppError::internal("数据库未返回自增 ID"))
}

fn int(value: i64) -> D1Type<'static> {
    D1Type::Integer(value as i32)
}

fn text(value: &str) -> D1Type<'_> {
    D1Type::Text(value)
}

/* ------------------------------------------------------------------ */
/* Schema + demo data                                                  */
/* ------------------------------------------------------------------ */

const SCHEMA: &str = include_str!("../schema.sql");

pub async fn bootstrap(db: &D1Database, seed_demo: bool) -> AppResult<()> {
    // D1's `exec()` support for multi-statement strings differs between the
    // runtime and Miniflare, so apply the schema as an atomic batch instead.
    let mut statements = Vec::new();
    for statement in SCHEMA.split(';') {
        let statement = statement.trim();
        if statement.is_empty() {
            continue;
        }
        statements.push(db.prepare(statement));
    }
    db.batch(statements).await.map_err(dberr)?;
    if !seed_demo {
        return Ok(());
    }
    let count = scalar_i64(db, "SELECT COUNT(*) AS value FROM boards", vec![]).await?;
    if count > 0 {
        return Ok(());
    }
    seed_demo_data(db).await
}

async fn seed_demo_data(db: &D1Database) -> AppResult<()> {
    let demo: Vec<(&str, Vec<(&str, Vec<(&str, &str, &str)>)>)> = vec![(
        "我的主页",
        vec![
            (
                "常用",
                vec![
                    ("GitHub", "https://github.com", "代码与开源项目"),
                    ("哔哩哔哩", "https://www.bilibili.com", ""),
                    ("知乎", "https://www.zhihu.com", ""),
                ],
            ),
            (
                "开发工具",
                vec![
                    ("Rust", "https://www.rust-lang.org", "系统级编程语言"),
                    ("SQLite", "https://sqlite.org", "嵌入式数据库"),
                    ("Docker", "https://docs.docker.com", ""),
                ],
            ),
            ("稍后阅读", vec![]),
        ],
    )];

    for (board_position, (board_name, columns)) in demo.iter().enumerate() {
        // The first board is inserted conditionally so two isolates racing on a
        // fresh database cannot create the demo twice.
        let board_id = if board_position == 0 {
            let created = execute(
                db,
                "INSERT INTO boards (name, position)
                 SELECT ?, ? WHERE NOT EXISTS (SELECT 1 FROM boards)",
                vec![text(board_name), int(0)],
            )
            .await?;
            if created == 0 {
                return Ok(());
            }
            scalar_i64(
                db,
                "SELECT id AS value FROM boards ORDER BY id DESC LIMIT 1",
                vec![],
            )
            .await?
        } else {
            insert_returning_id(
                db,
                "INSERT INTO boards (name, position) VALUES (?, ?) RETURNING id",
                vec![text(board_name), int(board_position as i64)],
            )
            .await?
        };
        for (column_position, (column_name, links)) in columns.iter().enumerate() {
            let column_id = insert_returning_id(
                db,
                "INSERT INTO columns (board_id, name, position) VALUES (?, ?, ?) RETURNING id",
                vec![
                    int(board_id),
                    text(column_name),
                    int(column_position as i64),
                ],
            )
            .await?;
            let mut statements = Vec::with_capacity(links.len());
            for (link_position, (title, url, note)) in links.iter().enumerate() {
                statements.push(
                    prepare(
                        db,
                        "INSERT INTO links (column_id, title, url, note, position) VALUES (?, ?, ?, ?, ?)",
                        vec![
                            int(column_id),
                            text(title),
                            text(url),
                            text(note),
                            int(link_position as i64),
                        ],
                    )?,
                );
            }
            if !statements.is_empty() {
                db.batch(statements).await.map_err(dberr)?;
            }
        }
    }
    worker::console_log!("seeded demo boards");
    Ok(())
}

/* ------------------------------------------------------------------ */
/* State                                                               */
/* ------------------------------------------------------------------ */

#[derive(Deserialize)]
struct BoardRow {
    id: i64,
    name: String,
    position: i64,
}

#[derive(Deserialize)]
struct ColumnRow {
    id: i64,
    board_id: i64,
    name: String,
    position: i64,
}

#[derive(Deserialize)]
struct LinkRow {
    id: i64,
    column_id: i64,
    title: String,
    url: String,
    note: String,
    icon: Option<String>,
    position: i64,
    created_at: String,
}

pub async fn load_state(db: &D1Database) -> AppResult<StateDto> {
    let boards: Vec<BoardRow> = query_all(
        db,
        "SELECT id, name, position FROM boards ORDER BY position ASC, id ASC",
        vec![],
    )
    .await?;
    let columns: Vec<ColumnRow> = query_all(
        db,
        "SELECT id, board_id, name, position FROM columns ORDER BY position ASC, id ASC",
        vec![],
    )
    .await?;
    let links: Vec<LinkRow> = query_all(
        db,
        "SELECT id, column_id, title, url, note, icon, position, created_at
         FROM links ORDER BY position ASC, id ASC",
        vec![],
    )
    .await?;

    let mut board_index = std::collections::HashMap::new();
    let mut column_index = std::collections::HashMap::new();
    let mut result = StateDto {
        boards: Vec::with_capacity(boards.len()),
    };

    for board in boards {
        board_index.insert(board.id, result.boards.len());
        result.boards.push(BoardDto {
            id: board.id,
            name: board.name,
            position: board.position,
            columns: Vec::new(),
        });
    }
    for column in columns {
        if let Some(&index) = board_index.get(&column.board_id) {
            let position = result.boards[index].columns.len();
            result.boards[index].columns.push(ColumnDto {
                id: column.id,
                board_id: column.board_id,
                name: column.name,
                position: column.position,
                links: Vec::new(),
            });
            column_index.insert(column.id, (index, position));
        }
    }
    for link in links {
        if let Some(&(board, column)) = column_index.get(&link.column_id) {
            result.boards[board].columns[column].links.push(LinkDto {
                id: link.id,
                column_id: link.column_id,
                title: link.title,
                url: link.url,
                note: link.note,
                icon: link.icon,
                position: link.position,
                created_at: link.created_at,
            });
        }
    }
    Ok(result)
}

/* ------------------------------------------------------------------ */
/* Boards                                                              */
/* ------------------------------------------------------------------ */

pub async fn create_board(db: &D1Database, name: &str) -> AppResult<i64> {
    insert_returning_id(
        db,
        "INSERT INTO boards (name, position)
         VALUES (?, (SELECT COALESCE(MAX(position), -1) + 1 FROM boards))
         RETURNING id",
        vec![text(name)],
    )
    .await
}

pub async fn update_board(db: &D1Database, id: i64, name: &str) -> AppResult<i64> {
    execute(
        db,
        "UPDATE boards SET name = ? WHERE id = ?",
        vec![text(name), int(id)],
    )
    .await
}

pub async fn delete_board(db: &D1Database, id: i64) -> AppResult<i64> {
    execute(db, "DELETE FROM boards WHERE id = ?", vec![int(id)]).await
}

pub async fn order_boards(db: &D1Database, ids: &[i64]) -> AppResult<()> {
    let mut statements = Vec::with_capacity(ids.len());
    for (index, id) in ids.iter().enumerate() {
        statements.push(prepare(
            db,
            "UPDATE boards SET position = ? WHERE id = ?",
            vec![int(index as i64), int(*id)],
        )?);
    }
    if !statements.is_empty() {
        db.batch(statements).await.map_err(dberr)?;
    }
    Ok(())
}

pub async fn board_exists(db: &D1Database, id: i64) -> AppResult<bool> {
    let count = scalar_i64(
        db,
        "SELECT COUNT(*) AS value FROM boards WHERE id = ?",
        vec![int(id)],
    )
    .await?;
    Ok(count > 0)
}

/* ------------------------------------------------------------------ */
/* Columns                                                             */
/* ------------------------------------------------------------------ */

pub async fn create_column(db: &D1Database, board_id: i64, name: &str) -> AppResult<i64> {
    insert_returning_id(
        db,
        "INSERT INTO columns (board_id, name, position)
         VALUES (?, ?, (SELECT COALESCE(MAX(position), -1) + 1 FROM columns WHERE board_id = ?))
         RETURNING id",
        vec![int(board_id), text(name), int(board_id)],
    )
    .await
}

pub async fn update_column(db: &D1Database, id: i64, name: &str) -> AppResult<i64> {
    execute(
        db,
        "UPDATE columns SET name = ? WHERE id = ?",
        vec![text(name), int(id)],
    )
    .await
}

pub async fn delete_column(db: &D1Database, id: i64) -> AppResult<i64> {
    execute(db, "DELETE FROM columns WHERE id = ?", vec![int(id)]).await
}

pub async fn order_columns(db: &D1Database, board_id: i64, ids: &[i64]) -> AppResult<()> {
    let mut statements = Vec::with_capacity(ids.len());
    for (index, id) in ids.iter().enumerate() {
        statements.push(prepare(
            db,
            "UPDATE columns SET position = ?, board_id = ? WHERE id = ?",
            vec![int(index as i64), int(board_id), int(*id)],
        )?);
    }
    if !statements.is_empty() {
        db.batch(statements).await.map_err(dberr)?;
    }
    Ok(())
}

pub async fn column_exists(db: &D1Database, id: i64) -> AppResult<bool> {
    let count = scalar_i64(
        db,
        "SELECT COUNT(*) AS value FROM columns WHERE id = ?",
        vec![int(id)],
    )
    .await?;
    Ok(count > 0)
}

/* ------------------------------------------------------------------ */
/* Links                                                               */
/* ------------------------------------------------------------------ */

#[derive(Deserialize)]
pub struct StoredLink {
    pub url: String,
    pub title: String,
    pub note: String,
    pub icon: Option<String>,
    pub column_id: i64,
}

pub async fn get_link(db: &D1Database, id: i64) -> AppResult<Option<StoredLink>> {
    query_one(
        db,
        "SELECT url, title, note, icon, column_id FROM links WHERE id = ?",
        vec![int(id)],
    )
    .await
}

pub async fn create_link(
    db: &D1Database,
    column_id: i64,
    title: &str,
    url: &str,
    note: &str,
    icon: Option<&str>,
) -> AppResult<i64> {
    insert_returning_id(
        db,
        "INSERT INTO links (column_id, title, url, note, icon, position)
         VALUES (?, ?, ?, ?, ?, (SELECT COALESCE(MAX(position), -1) + 1 FROM links WHERE column_id = ?))
         RETURNING id",
        vec![
            int(column_id),
            text(title),
            text(url),
            text(note),
            icon.map(text).unwrap_or(D1Type::Null),
            int(column_id),
        ],
    )
    .await
}

pub async fn next_link_position(db: &D1Database, column_id: i64) -> AppResult<i64> {
    scalar_i64(
        db,
        "SELECT COALESCE(MAX(position), -1) + 1 AS value FROM links WHERE column_id = ?",
        vec![int(column_id)],
    )
    .await
}

pub async fn update_link(
    db: &D1Database,
    id: i64,
    title: &str,
    url: &str,
    note: &str,
    icon: Option<&str>,
    column_id: i64,
    position: Option<i64>,
) -> AppResult<i64> {
    let icon = icon.map(text).unwrap_or(D1Type::Null);
    match position {
        Some(position) => {
            execute(
                db,
                "UPDATE links SET title = ?, url = ?, note = ?, icon = ?, column_id = ?, position = ?,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?",
                vec![
                    text(title),
                    text(url),
                    text(note),
                    icon,
                    int(column_id),
                    int(position),
                    int(id),
                ],
            )
            .await
        }
        None => {
            execute(
                db,
                "UPDATE links SET title = ?, url = ?, note = ?, icon = ?,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?",
                vec![text(title), text(url), text(note), icon, int(id)],
            )
            .await
        }
    }
}

pub async fn delete_link(db: &D1Database, id: i64) -> AppResult<i64> {
    execute(db, "DELETE FROM links WHERE id = ?", vec![int(id)]).await
}

pub async fn arrange_links(db: &D1Database, columns: &[(i64, Vec<i64>)]) -> AppResult<()> {
    let mut statements = Vec::new();
    for (column_id, link_ids) in columns {
        for (index, link_id) in link_ids.iter().enumerate() {
            statements.push(prepare(
                db,
                "UPDATE links SET column_id = ?, position = ? WHERE id = ?",
                vec![int(*column_id), int(index as i64), int(*link_id)],
            )?);
        }
    }
    if !statements.is_empty() {
        db.batch(statements).await.map_err(dberr)?;
    }
    Ok(())
}

/* ------------------------------------------------------------------ */
/* Import                                                              */
/* ------------------------------------------------------------------ */

#[derive(Debug, Default, Serialize)]
pub struct ImportStats {
    pub boards: usize,
    pub columns: usize,
    pub links: usize,
    pub skipped: usize,
}

pub async fn import_boards(
    db: &D1Database,
    boards: Vec<ImportBoard>,
) -> AppResult<(ImportStats, Vec<ImportedBoard>)> {
    if boards.len() > MAX_BOARDS {
        return Err(AppError::bad_request("面板数量过多"));
    }

    #[derive(Deserialize)]
    struct UrlRow {
        url: String,
    }
    let existing: Vec<UrlRow> = query_all(db, "SELECT url FROM links", vec![]).await?;
    let mut seen: HashSet<String> = existing.into_iter().map(|row| row.url).collect();

    let mut stats = ImportStats::default();
    let mut imported: Vec<ImportedBoard> = Vec::new();

    for board in boards {
        if stats.links >= MAX_LINKS {
            stats.skipped += 1;
            break;
        }
        if stats.columns >= MAX_COLUMNS {
            stats.skipped += board.columns.len();
            continue;
        }

        let layout = board.layout;
        let board_name = fallback_name(&board.name, "导入的面板");
        let board_id = create_board_positioned(db, &board_name).await?;
        stats.boards += 1;

        let mut inserted_columns: Vec<(String, i64)> = Vec::new();
        for column in board.columns {
            let column_name = fallback_name(&column.name, "导入的分组");
            let column_id = create_column_positioned(db, board_id, &column_name).await?;
            stats.columns += 1;
            inserted_columns.push((column_name, column_id));

            let mut statements = Vec::new();
            let mut position = 0i64;
            for link in column.links {
                if stats.links >= MAX_LINKS {
                    stats.skipped += 1;
                    continue;
                }
                let Ok(url) = tabnest_core::url::normalize_url(&link.url) else {
                    stats.skipped += 1;
                    continue;
                };
                if !seen.insert(url.clone()) {
                    stats.skipped += 1;
                    continue;
                }
                let title = if link.title.trim().is_empty() {
                    tabnest_core::url::host_label(&url)
                } else {
                    tabnest_core::html::truncate_chars(link.title.trim(), 300)
                };
                let note = tabnest_core::html::truncate_chars(link.note.trim(), 2000);
                let icon = link.icon.filter(|value| !value.trim().is_empty());
                statements.push(prepare(
                    db,
                    "INSERT INTO links (column_id, title, url, note, icon, position)
                     VALUES (?, ?, ?, ?, ?, ?)",
                    vec![
                        int(column_id),
                        text(&title),
                        text(&url),
                        text(&note),
                        icon.as_deref().map(text).unwrap_or(D1Type::Null),
                        int(position),
                    ],
                )?);
                stats.links += 1;
                position += 1;
            }
            if !statements.is_empty() {
                db.batch(statements).await.map_err(dberr)?;
            }
        }

        imported.push(ImportedBoard {
            name: board_name,
            id: board_id,
            columns: inserted_columns,
            layout,
        });
    }

    if stats.boards == 0 {
        return Err(AppError::bad_request("没有找到可导入的书签"));
    }
    Ok((stats, imported))
}

/* ------------------------------------------------------------------ */
/* Settings (shared across devices)                                    */
/* ------------------------------------------------------------------ */

pub async fn load_settings(db: &D1Database) -> AppResult<SettingsDto> {
    #[derive(Deserialize)]
    struct SettingsRow {
        theme: Option<String>,
        palette: Option<String>,
        columns: Option<String>,
        active_board: Option<i64>,
    }
    #[derive(Deserialize)]
    struct LayoutRow {
        board_id: i64,
        layout: String,
    }

    let mut settings = SettingsDto::default();
    if let Some(row) = query_one::<SettingsRow>(
        db,
        "SELECT theme, palette, columns, active_board FROM settings WHERE id = 1",
        vec![],
    )
    .await?
    {
        settings.theme = row.theme;
        settings.palette = row.palette;
        settings.columns = row.columns;
        settings.active_board = row.active_board;
    }

    let rows: Vec<LayoutRow> = query_all(db, "SELECT board_id, layout FROM board_layouts", vec![]).await?;
    for row in rows {
        if let Ok(layout) = serde_json::from_str::<Vec<Vec<i64>>>(&row.layout) {
            settings.layouts.insert(row.board_id, sanitize_layout(layout));
        }
    }
    Ok(settings)
}

pub async fn save_settings(db: &D1Database, update: SettingsUpdate) -> AppResult<()> {
    let theme = update.theme.filter(|value| valid_theme(value));
    let palette = update.palette.filter(|value| valid_palette(value));
    let columns = update.columns.filter(|value| valid_columns(value));

    let active_board = match update.active_board {
        Some(id) if board_exists(db, id).await? => Some(id),
        _ => None,
    };

    execute(
        db,
        "INSERT INTO settings (id, theme, palette, columns, active_board)
         VALUES (1, ?, ?, ?, ?)
         ON CONFLICT(id) DO UPDATE SET
             theme = COALESCE(excluded.theme, settings.theme),
             palette = COALESCE(excluded.palette, settings.palette),
             columns = COALESCE(excluded.columns, settings.columns),
             active_board = COALESCE(excluded.active_board, settings.active_board),
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        vec![
            theme.as_deref().map(text).unwrap_or(D1Type::Null),
            palette.as_deref().map(text).unwrap_or(D1Type::Null),
            columns.as_deref().map(text).unwrap_or(D1Type::Null),
            active_board.map(int).unwrap_or(D1Type::Null),
        ],
    )
    .await?;
    Ok(())
}

pub async fn save_layout(
    db: &D1Database,
    board_id: i64,
    layout: &[Vec<i64>],
) -> AppResult<bool> {
    if !board_exists(db, board_id).await? {
        return Ok(false);
    }
    let json = serde_json::to_string(&sanitize_layout(layout.to_vec()))?;
    execute(
        db,
        "INSERT INTO board_layouts (board_id, layout) VALUES (?, ?)
         ON CONFLICT(board_id) DO UPDATE SET layout = excluded.layout",
        vec![int(board_id), text(&json)],
    )
    .await?;
    Ok(true)
}

async fn create_board_positioned(db: &D1Database, name: &str) -> AppResult<i64> {
    insert_returning_id(
        db,
        "INSERT INTO boards (name, position)
         VALUES (?, (SELECT COALESCE(MAX(position), -1) + 1 FROM boards))
         RETURNING id",
        vec![text(name)],
    )
    .await
}

async fn create_column_positioned(
    db: &D1Database,
    board_id: i64,
    name: &str,
) -> AppResult<i64> {
    create_column(db, board_id, name).await
}

fn fallback_name(name: &str, fallback: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        tabnest_core::html::truncate_chars(trimmed, 60)
    }
}
