-- Tabnest 数据库结构，版本 1。
-- 三张核心表形成 面板 → 分组 → 链接 的三级结构；
-- 时间戳统一存 UTC ISO-8601 字符串（strftime 生成），便于直接比较与排序。

CREATE TABLE IF NOT EXISTS boards (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT    NOT NULL,
    -- 面板之间、分组之间、链接之间都用 position 升序决定显示顺序。
    position    INTEGER NOT NULL DEFAULT 0,
    created_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE IF NOT EXISTS columns (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    -- 删面板时级联删除其下所有分组。
    board_id    INTEGER NOT NULL REFERENCES boards(id) ON DELETE CASCADE,
    name        TEXT    NOT NULL,
    position    INTEGER NOT NULL DEFAULT 0,
    created_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE IF NOT EXISTS links (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    -- 删分组时级联删除其下所有链接。
    column_id   INTEGER NOT NULL REFERENCES columns(id) ON DELETE CASCADE,
    title       TEXT    NOT NULL,
    url         TEXT    NOT NULL,
    -- 用户手写的备注，默认为空字符串而非 NULL，读取端无需处理 NULL。
    note        TEXT    NOT NULL DEFAULT '',
    -- 缓存的 favicon 文件名；NULL 表示尚未抓取，回退到首字母图标。
    icon        TEXT,
    position    INTEGER NOT NULL DEFAULT 0,
    created_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

-- 读列表时按 (父 id, position) 排序，这两条索引覆盖了主要查询路径。
CREATE INDEX IF NOT EXISTS idx_columns_board ON columns(board_id, position);
CREATE INDEX IF NOT EXISTS idx_links_column ON links(column_id, position);
-- 导入时按 url 去重（判断链接是否已存在）。
CREATE INDEX IF NOT EXISTS idx_links_url ON links(url);
