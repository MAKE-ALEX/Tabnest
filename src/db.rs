use std::time::Duration;

use anyhow::Context;
use serde::Serialize;
use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous,
};
use sqlx::SqlitePool;

use crate::{
    config::Config,
    error::{AppError, AppResult},
    fetch::{host_label, normalize_url, truncate_chars},
    import::ImportBoard,
};

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

    Ok(pool)
}

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

#[derive(Debug, Default, Serialize)]
pub struct ImportStats {
    pub boards: usize,
    pub columns: usize,
    pub links: usize,
    pub skipped: usize,
}

pub async fn import_boards(pool: &SqlitePool, boards: Vec<ImportBoard>) -> AppResult<ImportStats> {
    const MAX_LINKS: usize = 20_000;

    let mut stats = ImportStats::default();
    let mut tx = pool.begin().await?;

    for board in boards {
        if stats.links >= MAX_LINKS {
            stats.skipped += 1;
            break;
        }
        let board_name = fallback_name(&board.name, "导入的面板");
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

            for link in column.links {
                if stats.links >= MAX_LINKS {
                    stats.skipped += 1;
                    continue;
                }
                let Ok(url) = normalize_url(&link.url) else {
                    stats.skipped += 1;
                    continue;
                };
                let exists: Option<i64> =
                    sqlx::query_scalar("SELECT id FROM links WHERE url = ? LIMIT 1")
                        .bind(&url)
                        .fetch_optional(&mut *tx)
                        .await?;
                if exists.is_some() {
                    stats.skipped += 1;
                    continue;
                }
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
    }

    tx.commit().await?;
    if stats.boards == 0 {
        return Err(AppError::bad_request("没有找到可导入的书签"));
    }
    Ok(stats)
}

fn fallback_name(name: &str, fallback: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        truncate_chars(trimmed, 60)
    }
}
