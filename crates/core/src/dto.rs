//! Response payloads for `GET /api/state` / `GET /api/settings` and the JSON
//! export, plus the shared preference types used by both runtimes.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct StateDto {
    pub boards: Vec<BoardDto>,
}

#[derive(Serialize)]
pub struct BoardDto {
    pub id: i64,
    pub name: String,
    pub position: i64,
    pub columns: Vec<ColumnDto>,
}

#[derive(Serialize)]
pub struct ColumnDto {
    pub id: i64,
    pub board_id: i64,
    pub name: String,
    pub position: i64,
    pub links: Vec<LinkDto>,
}

#[derive(Serialize)]
pub struct LinkDto {
    pub id: i64,
    pub column_id: i64,
    pub title: String,
    pub url: String,
    pub note: String,
    pub icon: Option<String>,
    pub position: i64,
    pub created_at: String,
}

/// Everything the UI used to keep in `localStorage`, now shared across devices.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SettingsDto {
    pub theme: Option<String>,
    pub palette: Option<String>,
    pub columns: Option<String>,
    pub active_board: Option<i64>,
    /// board id -> masonry columns (`[[column_id, ...], ...]`).
    #[serde(default)]
    pub layouts: HashMap<i64, Vec<Vec<i64>>>,
}

/// Partial update for the preference fields.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct SettingsUpdate {
    pub theme: Option<String>,
    pub palette: Option<String>,
    pub columns: Option<String>,
    pub active_board: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct LayoutUpdate {
    pub layout: Vec<Vec<i64>>,
}
