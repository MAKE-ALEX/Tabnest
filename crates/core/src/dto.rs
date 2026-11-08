//! Response payloads for `GET /api/state` and the JSON export.

use serde::Serialize;

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
