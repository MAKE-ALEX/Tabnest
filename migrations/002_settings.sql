-- Tabnest 数据库结构，版本 2：服务端保存的界面偏好
-- （主题 / 配色 / 分栏数 / 当前面板）以及每个面板的瀑布流布局。

-- 单行表：id 固定为 1，用 CHECK 约束防止出现第二行。
-- 各字段允许为 NULL，表示「该浏览器尚未同步过，沿用本地默认值」。
CREATE TABLE IF NOT EXISTS settings (
    id           INTEGER PRIMARY KEY CHECK (id = 1),
    theme        TEXT,
    palette      TEXT,
    columns      TEXT,
    active_board INTEGER,
    updated_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

-- 每个面板一行，layout 是 JSON：按列依次存放链接 id 的二维数组；
-- 删面板时一并删除对应布局。
CREATE TABLE IF NOT EXISTS board_layouts (
    board_id INTEGER PRIMARY KEY REFERENCES boards(id) ON DELETE CASCADE,
    layout   TEXT    NOT NULL
);
