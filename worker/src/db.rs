//! D1 持久化层。
//!
//! 这里的 SQL 与原生端 `src/api.rs` / `src/db.rs` 中的 `sqlx` 查询一一对应，
//! 差异来自 D1 的 API 形态：用 `RETURNING id` 代替 `last_insert_rowid()`，
//! 用 `batch()` 代替可交互事务。

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use tabnest_core::dto::{BoardDto, ColumnDto, LinkDto, SettingsDto, SettingsUpdate, StateDto};
use tabnest_core::import::{ImportBoard, ImportedBoard};
use tabnest_core::settings::{sanitize_layout, valid_columns, valid_palette, valid_theme};
use worker::d1::{D1Database, D1PreparedStatement, D1Type};

use crate::error::{AppError, AppResult};

/// 单次导入允许写入的链接上限，防止一次请求撑爆 D1 写入配额。
const MAX_LINKS: usize = 20_000;
/// 单次导入允许创建的面板上限。
const MAX_BOARDS: usize = 500;
/// 单次导入允许创建的分组上限。
const MAX_COLUMNS: usize = 2000;

/// 把 `worker::Error` 统一收敛成 500，同时把原始错误打到 Workers 日志。
fn dberr(error: worker::Error) -> AppError {
    worker::console_error!("d1 error: {error} ({error:?})");
    AppError::internal("数据库操作失败")
}

/// 预编译 SQL；没有绑定参数时直接返回语句本身。
///
/// D1 的 `bind_refs` 需要借用参数，因此这里让 `args` 按值传入、由调用方持有，
/// 语句内部只保存引用。
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

/// 执行查询并把所有结果行反序列化为 `T`。
async fn query_all<T>(db: &D1Database, sql: &str, args: Vec<D1Type<'_>>) -> AppResult<Vec<T>>
where
    T: for<'de> Deserialize<'de>,
{
    let statement = prepare(db, sql, args)?;
    let result = statement.all().await.map_err(dberr)?;
    result.results::<T>().map_err(dberr)
}

/// 执行查询并取第一行；没有结果时返回 `None`。
async fn query_one<T>(db: &D1Database, sql: &str, args: Vec<D1Type<'_>>) -> AppResult<Option<T>>
where
    T: for<'de> Deserialize<'de>,
{
    let statement = prepare(db, sql, args)?;
    statement.first::<T>(None).await.map_err(dberr)
}

/// 执行写语句，返回受影响行数；D1 没给出 changes 时按 1 处理。
async fn execute(db: &D1Database, sql: &str, args: Vec<D1Type<'_>>) -> AppResult<i64> {
    let statement = prepare(db, sql, args)?;
    let result = statement.run().await.map_err(dberr)?;
    let changed = result.meta().ok().flatten().and_then(|meta| meta.changes);
    Ok(changed.unwrap_or(1) as i64)
}

/// 读取单个整数标量（例如 `COUNT(*)`），无行时按 0 处理。
async fn scalar_i64(db: &D1Database, sql: &str, args: Vec<D1Type<'_>>) -> AppResult<i64> {
    #[derive(Deserialize)]
    struct Value {
        value: i64,
    }
    let row: Option<Value> = query_one(db, sql, args).await?;
    Ok(row.map(|row| row.value).unwrap_or(0))
}

/// 执行带 `RETURNING id` 的插入并取出新行主键。
async fn insert_returning_id(db: &D1Database, sql: &str, args: Vec<D1Type<'_>>) -> AppResult<i64> {
    #[derive(Deserialize)]
    struct Id {
        id: i64,
    }
    let row: Option<Id> = query_one(db, sql, args).await?;
    row.map(|row| row.id)
        .ok_or_else(|| AppError::internal("数据库未返回自增 ID"))
}

/// D1 的 INTEGER 是 32 位有符号整数，这里统一做一次窄化。
fn int(value: i64) -> D1Type<'static> {
    D1Type::Integer(value as i32)
}

/// 文本参数的简写，省去每处都写 `D1Type::Text`。
fn text(value: &str) -> D1Type<'_> {
    D1Type::Text(value)
}

/* ------------------------------------------------------------------ */
/* 建表 + 演示数据                                                     */
/* ------------------------------------------------------------------ */

const SCHEMA: &str = include_str!("../schema.sql");

/// 建表并按需灌入演示数据。
pub async fn bootstrap(db: &D1Database, seed_demo: bool) -> AppResult<()> {
    // D1 的 `exec()` 对多语句字符串的支持在运行时和 Miniflare 之间并不一致，
    // 因此改为把 schema 拆成语句、以原子 batch 的方式提交。
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

/// 写入一套演示面板；只在库里没有任何面板时调用。
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
        // 第一个面板用条件插入，避免两个 isolate 在空库上同时冷启动时把演示数据写两遍。
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
/* 整体状态                                                            */
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

/// 一次性读出全部面板/分组/链接，再在内存里组装成嵌套的 `StateDto`。
///
/// 相比逐层 N+1 查询，这里固定只发三条 SQL；三张表都按 `position, id` 排序，
/// 所以读出来的顺序就是前端应展示的顺序。
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

    // 用两级索引把「数据库行」映射到「结果集中的下标」，避免反复线性查找。
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
        // 父面板不存在时静默丢弃该分组，保持与原生端一致。
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
/* 面板                                                                */
/* ------------------------------------------------------------------ */

/// 新建面板，位置追加到当前最大值之后。
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

/// 重命名面板，返回受影响行数（0 表示面板不存在）。
pub async fn update_board(db: &D1Database, id: i64, name: &str) -> AppResult<i64> {
    execute(
        db,
        "UPDATE boards SET name = ? WHERE id = ?",
        vec![text(name), int(id)],
    )
    .await
}

/// 删除面板；其下分组与链接由外键级联清理。
pub async fn delete_board(db: &D1Database, id: i64) -> AppResult<i64> {
    execute(db, "DELETE FROM boards WHERE id = ?", vec![int(id)]).await
}

/// 按传入顺序重写面板的 `position`。
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

/// 判断面板是否存在。
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
/* 分组                                                                */
/* ------------------------------------------------------------------ */

/// 在指定面板下新建分组，位置取该面板内的最大值 +1。
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

/// 重命名分组。
pub async fn update_column(db: &D1Database, id: i64, name: &str) -> AppResult<i64> {
    execute(
        db,
        "UPDATE columns SET name = ? WHERE id = ?",
        vec![text(name), int(id)],
    )
    .await
}

/// 删除分组；组内链接随外键级联删除。
pub async fn delete_column(db: &D1Database, id: i64) -> AppResult<i64> {
    execute(db, "DELETE FROM columns WHERE id = ?", vec![int(id)]).await
}

/// 重排分组：同时写入新的 `position` 与所属 `board_id`，
/// 因此这个接口也承担「跨面板移动分组」的职责。
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

/// 判断分组是否存在。
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
/* 链接                                                                */
/* ------------------------------------------------------------------ */

/// 更新/移动链接时需要的旧值快照。
#[derive(Deserialize)]
pub struct StoredLink {
    pub url: String,
    pub title: String,
    pub note: String,
    pub icon: Option<String>,
    pub column_id: i64,
}

/// 按 ID 取链接的当前内容。
pub async fn get_link(db: &D1Database, id: i64) -> AppResult<Option<StoredLink>> {
    query_one(
        db,
        "SELECT url, title, note, icon, column_id FROM links WHERE id = ?",
        vec![int(id)],
    )
    .await
}

/// 在分组末尾追加一条链接。
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

/// 计算分组内下一个可用位置（可能被调用方用于插入前的校验）。
pub async fn next_link_position(db: &D1Database, column_id: i64) -> AppResult<i64> {
    scalar_i64(
        db,
        "SELECT COALESCE(MAX(position), -1) + 1 AS value FROM links WHERE column_id = ?",
        vec![int(column_id)],
    )
    .await
}

/// 更新链接。传入 `position` 时一并移动到目标分组的指定位置，
/// 否则只改内容、保留原归属与顺序。
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

/// 删除链接。
pub async fn delete_link(db: &D1Database, id: i64) -> AppResult<i64> {
    execute(db, "DELETE FROM links WHERE id = ?", vec![int(id)]).await
}

/// 批量重排链接：`columns` 中每一项是「目标分组 + 该组内的链接顺序」，
/// 一次 batch 写入完成跨组拖动。
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
/* 导入                                                                */
/* ------------------------------------------------------------------ */

/// 一次导入的统计结果，直接回给前端展示。
#[derive(Debug, Default, Serialize)]
pub struct ImportStats {
    pub boards: usize,
    pub columns: usize,
    pub links: usize,
    pub skipped: usize,
}

/// 把解析好的书签批量写入 D1。
///
/// 规则与原生端一致：URL 先去重（库内已有的 + 本次已见过的），
/// 标题为空时退化成域名，标题/备注按字符数截断，达到上限后跳过并计数。
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
        // 链接总数触顶后，后面的面板一律跳过。
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

        // 记录「分组名 -> 新 ID」，供导入结果回传前端做定向跳转。
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
                // 非法 URL（含非 http(s) 协议）与重复 URL 都只计数、不入库。
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
/* 设置（跨设备共享）                                                  */
/* ------------------------------------------------------------------ */

/// 读取全局设置与各面板的排版。
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

    // 版式以 JSON 字符串存储，解析失败（脏数据/旧格式）时跳过该面板即可。
    let rows: Vec<LayoutRow> = query_all(db, "SELECT board_id, layout FROM board_layouts", vec![]).await?;
    for row in rows {
        if let Ok(layout) = serde_json::from_str::<Vec<Vec<i64>>>(&row.layout) {
            settings.layouts.insert(row.board_id, sanitize_layout(layout));
        }
    }
    Ok(settings)
}

/// 保存设置；`ON CONFLICT` 时用 `COALESCE` 合并，
/// 因此只传部分字段不会清空其它已有设置。
pub async fn save_settings(db: &D1Database, update: SettingsUpdate) -> AppResult<()> {
    let theme = update.theme.filter(|value| valid_theme(value));
    let palette = update.palette.filter(|value| valid_palette(value));
    let columns = update.columns.filter(|value| valid_columns(value));

    // active_board 指向不存在的面板时按「未设置」处理。
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

/// 保存单个面板的拖拽排版；面板不存在时返回 `false`。
pub async fn save_layout(
    db: &D1Database,
    board_id: i64,
    layout: &[Vec<i64>],
) -> AppResult<bool> {
    if !board_exists(db, board_id).await? {
        return Ok(false);
    }
    // 写入前先做一次清洗，防止越界/重复的分组下标进入前端。
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

/// 建面板并置于末尾；导入路径专用，语义上是 `create_board` 的别名，
/// 单独保留是为了让导入逻辑与原生端保持同名对应关系。
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

/// 建分组并置于末尾；同样只是 `create_column` 的语义化别名。
async fn create_column_positioned(
    db: &D1Database,
    board_id: i64,
    name: &str,
) -> AppResult<i64> {
    create_column(db, board_id, name).await
}

/// 取名字，空名退回默认值，过长的按字符截断到 60。
fn fallback_name(name: &str, fallback: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        tabnest_core::html::truncate_chars(trimmed, 60)
    }
}
