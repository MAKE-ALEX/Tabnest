//! SQLite 数据访问层：建库、示例数据、导入、偏好设置与排版读写。

use std::time::Duration;

use anyhow::Context;
use serde::Serialize;
use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous,
};
use sqlx::SqlitePool;

use tabnest_core::{
    dto::{SettingsDto, SettingsUpdate},
    import::ImportedBoard,
    settings::{sanitize_layout, valid_columns, valid_palette, valid_theme},
};

use crate::{
    config::Config,
    error::{AppError, AppResult},
    fetch::{host_label, normalize_url, truncate_chars},
    import::ImportBoard,
};

/// 打开（必要时创建）SQLite 数据库，并执行内置迁移。
///
/// 使用 WAL 日志模式兼顾并发读和写入性能；`busy_timeout` 让并发写入排队
/// 而不是立刻报错。数据库版本记录在 `PRAGMA user_version` 中，逐级升级。
pub async fn connect(cfg: &Config) -> anyhow::Result<SqlitePool> {
    let options = SqliteConnectOptions::new()
        .filename(&cfg.db_path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_secs(5))
        .foreign_keys(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(10))
        .connect_with(options)
        .await
        .with_context(|| format!("无法打开数据库 {}", cfg.db_path.display()))?;

    // 版本 0 -> 1：创建面板 / 分组 / 书签基础表。
    let version: i64 = sqlx::query_scalar("PRAGMA user_version")
        .fetch_one(&pool)
        .await?;
    if version < 1 {
        sqlx::raw_sql(include_str!("../migrations/001_init.sql"))
            .execute(&pool)
            .await
            .context("初始化数据库表失败")?;
        sqlx::query("PRAGMA user_version = 1")
            .execute(&pool)
            .await?;
        tracing::info!("数据库初始化完成：{}", cfg.db_path.display());
    }
    // 版本 1 -> 2：新增服务端偏好设置与瀑布流排版表。
    if version < 2 {
        sqlx::raw_sql(include_str!("../migrations/002_settings.sql"))
            .execute(&pool)
            .await
            .context("创建设置表失败")?;
        sqlx::query("PRAGMA user_version = 2")
            .execute(&pool)
            .await?;
        tracing::info!("已升级到数据库版本 2（服务器端外观设置）");
    }

    Ok(pool)
}

/// 数据库为空时写入一组示例面板，方便第一次启动就能看到界面。
pub async fn seed_if_empty(pool: &SqlitePool, enabled: bool) -> anyhow::Result<()> {
    if !enabled {
        return Ok(());
    }
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM boards")
        .fetch_one(pool)
        .await?;
    if count > 0 {
        return Ok(());
    }

    // (面板名, [(分组名, [(标题, 网址, 备注), ...]), ...])
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

    let mut tx = pool.begin().await?;
    for (board_position, (board_name, columns)) in demo.iter().enumerate() {
        let board_id = sqlx::query("INSERT INTO boards (name, position) VALUES (?, ?)")
            .bind(board_name)
            .bind(board_position as i64)
            .execute(&mut *tx)
            .await?
            .last_insert_rowid();
        for (column_position, (column_name, links)) in columns.iter().enumerate() {
            let column_id =
                sqlx::query("INSERT INTO columns (board_id, name, position) VALUES (?, ?, ?)")
                    .bind(board_id)
                    .bind(column_name)
                    .bind(column_position as i64)
                    .execute(&mut *tx)
                    .await?
                    .last_insert_rowid();
            for (link_position, (title, url, note)) in links.iter().enumerate() {
                sqlx::query(
                    "INSERT INTO links (column_id, title, url, note, position) VALUES (?, ?, ?, ?, ?)",
                )
                .bind(column_id)
                .bind(title)
                .bind(url)
                .bind(note)
                .bind(link_position as i64)
                .execute(&mut *tx)
                .await?;
            }
        }
    }
    tx.commit().await?;
    tracing::info!("已写入示例面板（设置 SEED_DEMO=false 可关闭）");
    Ok(())
}

/// 一次导入的结果统计。
#[derive(Debug, Default, Serialize)]
pub struct ImportStats {
    pub boards: usize,
    pub columns: usize,
    pub links: usize,
    /// 因重复、非法或超出上限而被跳过的书签数。
    pub skipped: usize,
}

/// 把解析好的书签批量写入数据库，返回统计信息和已写入面板的 id 映射。
///
/// 整个导入在一个事务里完成：要么全部成功，要么全部回滚，不会留下半个面板。
pub async fn import_boards(
    pool: &SqlitePool,
    boards: Vec<ImportBoard>,
) -> AppResult<(ImportStats, Vec<ImportedBoard>)> {
    // 防止畸形文件把数据库撑爆。
    const MAX_LINKS: usize = 20_000;
    const MAX_BOARDS: usize = 500;
    const MAX_COLUMNS: usize = 2000;

    if boards.len() > MAX_BOARDS {
        return Err(AppError::bad_request("面板数量过多"));
    }

    let mut stats = ImportStats::default();
    let mut imported: Vec<ImportedBoard> = Vec::new();
    let mut tx = pool.begin().await?;

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
        // 追加到现有面板末尾。
        let position: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(position), -1) + 1 FROM boards")
            .fetch_one(&mut *tx)
            .await?;
        let board_id = sqlx::query("INSERT INTO boards (name, position) VALUES (?, ?)")
            .bind(&board_name)
            .bind(position)
            .execute(&mut *tx)
            .await?
            .last_insert_rowid();
        stats.boards += 1;

        let mut inserted_columns: Vec<(String, i64)> = Vec::new();
        for column in board.columns {
            let column_name = fallback_name(&column.name, "导入的分组");
            let position: i64 =
                sqlx::query_scalar("SELECT COALESCE(MAX(position), -1) + 1 FROM columns WHERE board_id = ?")
                    .bind(board_id)
                    .fetch_one(&mut *tx)
                    .await?;
            let column_id =
                sqlx::query("INSERT INTO columns (board_id, name, position) VALUES (?, ?, ?)")
                    .bind(board_id)
                    .bind(&column_name)
                    .bind(position)
                    .execute(&mut *tx)
                    .await?
                    .last_insert_rowid();
            stats.columns += 1;
            inserted_columns.push((column_name, column_id));

            for link in column.links {
                if stats.links >= MAX_LINKS {
                    stats.skipped += 1;
                    continue;
                }
                // 网址不合法就跳过，不因为一条坏数据让整个导入失败。
                let Ok(url) = normalize_url(&link.url) else {
                    stats.skipped += 1;
                    continue;
                };
                // 按 URL 去重：已存在的书签不重复导入。
                let exists: Option<i64> =
                    sqlx::query_scalar("SELECT id FROM links WHERE url = ? LIMIT 1")
                        .bind(&url)
                        .fetch_optional(&mut *tx)
                        .await?;
                if exists.is_some() {
                    stats.skipped += 1;
                    continue;
                }
                // 没标题时用域名兜底，保证卡片有可读文字。
                let title = if link.title.trim().is_empty() {
                    host_label(&url)
                } else {
                    truncate_chars(link.title.trim(), 300)
                };
                let note = truncate_chars(link.note.trim(), 2000);
                let position: i64 = sqlx::query_scalar(
                    "SELECT COALESCE(MAX(position), -1) + 1 FROM links WHERE column_id = ?",
                )
                .bind(column_id)
                .fetch_one(&mut *tx)
                .await?;
                sqlx::query(
                    "INSERT INTO links (column_id, title, url, note, icon, position) VALUES (?, ?, ?, ?, ?, ?)",
                )
                .bind(column_id)
                .bind(&title)
                .bind(&url)
                .bind(&note)
                .bind(link.icon.filter(|icon| !icon.trim().is_empty()))
                .bind(position)
                .execute(&mut *tx)
                .await?;
                stats.links += 1;
            }
        }

        imported.push(ImportedBoard {
            name: board_name,
            id: board_id,
            columns: inserted_columns,
            layout,
        });
    }

    tx.commit().await?;
    if stats.boards == 0 {
        return Err(AppError::bad_request("没有找到可导入的书签"));
    }
    Ok((stats, imported))
}

/// 读取全局偏好设置，以及各面板的瀑布流排版。
pub async fn load_settings(pool: &SqlitePool) -> AppResult<SettingsDto> {
    let row: Option<(Option<String>, Option<String>, Option<String>, Option<i64>)> = sqlx::query_as(
        "SELECT theme, palette, columns, active_board FROM settings WHERE id = 1",
    )
    .fetch_optional(pool)
    .await?;

    let mut settings = SettingsDto::default();
    if let Some((theme, palette, columns, active_board)) = row {
        settings.theme = theme;
        settings.palette = palette;
        settings.columns = columns;
        settings.active_board = active_board;
    }

    // 排版以 JSON 字符串存储，读出来后再解析和清洗。
    let layouts: Vec<(i64, String)> =
        sqlx::query_as("SELECT board_id, layout FROM board_layouts")
            .fetch_all(pool)
            .await?;
    for (board_id, json) in layouts {
        if let Ok(layout) = serde_json::from_str::<Vec<Vec<i64>>>(&json) {
            settings.layouts.insert(board_id, sanitize_layout(layout));
        }
    }
    Ok(settings)
}

/// 写入偏好设置（upsert）。
///
/// 非法或未知的取值会被忽略而不是报错，这样旧版本客户端不会把所有人的同步
/// 都弄坏。`active_board` 还会额外校验面板是否真实存在。
pub async fn save_settings(pool: &SqlitePool, update: SettingsUpdate) -> AppResult<()> {
    let theme = update.theme.filter(|value| valid_theme(value));
    let palette = update.palette.filter(|value| valid_palette(value));
    let columns = update.columns.filter(|value| valid_columns(value));

    let active_board = match update.active_board {
        Some(id) => {
            let exists: Option<i64> = sqlx::query_scalar("SELECT id FROM boards WHERE id = ?")
                .bind(id)
                .fetch_optional(pool)
                .await?;
            exists
        }
        None => None,
    };

    // COALESCE 保证只更新本次传入的字段，`None` 不会覆盖已有值。
    sqlx::query(
        "INSERT INTO settings (id, theme, palette, columns, active_board)
         VALUES (1, ?, ?, ?, ?)
         ON CONFLICT(id) DO UPDATE SET
             theme = COALESCE(excluded.theme, settings.theme),
             palette = COALESCE(excluded.palette, settings.palette),
             columns = COALESCE(excluded.columns, settings.columns),
             active_board = COALESCE(excluded.active_board, settings.active_board),
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
    )
    .bind(theme)
    .bind(palette)
    .bind(columns)
    .bind(active_board)
    .execute(pool)
    .await?;
    Ok(())
}

/// 保存某个面板的瀑布流排版；面板不存在时返回 `false`。
pub async fn save_layout(
    pool: &SqlitePool,
    board_id: i64,
    layout: &[Vec<i64>],
) -> AppResult<bool> {
    let exists: Option<i64> = sqlx::query_scalar("SELECT id FROM boards WHERE id = ?")
        .bind(board_id)
        .fetch_optional(pool)
        .await?;
    if exists.is_none() {
        return Ok(false);
    }
    let json = serde_json::to_string(&sanitize_layout(layout.to_vec()))?;
    sqlx::query(
        "INSERT INTO board_layouts (board_id, layout) VALUES (?, ?)
         ON CONFLICT(board_id) DO UPDATE SET layout = excluded.layout",
    )
    .bind(board_id)
    .bind(json)
    .execute(pool)
    .await?;
    Ok(true)
}

/// 名称兜底：空名称使用默认值，过长则截断到 60 字符。
fn fallback_name(name: &str, fallback: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        truncate_chars(trimmed, 60)
    }
}
