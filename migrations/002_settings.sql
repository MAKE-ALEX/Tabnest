-- Tabnest schema, version 2: server-side UI preferences
-- (theme / palette / columns / active board) and per-board masonry layout.

CREATE TABLE IF NOT EXISTS settings (
    id           INTEGER PRIMARY KEY CHECK (id = 1),
    theme        TEXT,
    palette      TEXT,
    columns      TEXT,
    active_board INTEGER,
    updated_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE IF NOT EXISTS board_layouts (
    board_id INTEGER PRIMARY KEY REFERENCES boards(id) ON DELETE CASCADE,
    layout   TEXT    NOT NULL
);
