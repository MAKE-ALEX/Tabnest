//! `GET /api/state`、`GET /api/settings` 以及 JSON 导出使用的响应结构，
//! 外加两个运行时共享的偏好设置类型。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// `GET /api/state` 的顶层响应：包含所有面板，顺序即显示顺序。
#[derive(Serialize)]
pub struct StateDto {
    pub boards: Vec<BoardDto>,
}

/// 一个面板（侧栏中的一项）。
#[derive(Serialize)]
pub struct BoardDto {
    /// 数据库主键，前端用它标识面板。
    pub id: i64,
    /// 面板名称。
    pub name: String,
    /// 面板排序值，越小越靠前。
    pub position: i64,
    /// 面板下的全部分组。
    pub columns: Vec<ColumnDto>,
}

/// 一个分组（面板内的一列卡片）。
#[derive(Serialize)]
pub struct ColumnDto {
    pub id: i64,
    /// 所属面板的 id。
    pub board_id: i64,
    /// 分组名称。
    pub name: String,
    /// 面板内的排序值。
    pub position: i64,
    /// 分组下的书签卡片。
    pub links: Vec<LinkDto>,
}

/// 一张书签卡片。
#[derive(Serialize)]
pub struct LinkDto {
    pub id: i64,
    /// 所属分组的 id。
    pub column_id: i64,
    /// 显示标题。
    pub title: String,
    /// 规范化后的完整网址。
    pub url: String,
    /// 用户备注，可为空字符串。
    pub note: String,
    /// 图标地址；前端会回退到按域名生成的默认图标。
    pub icon: Option<String>,
    /// 分组内的排序值。
    pub position: i64,
    /// 创建时间（字符串形式，原样透传给前端）。
    pub created_at: String,
}

/// 用户偏好设置：过去只存在浏览器 `localStorage` 里，现在由服务端统一保存，
/// 让多台设备共享同一套外观与排版。
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SettingsDto {
    /// 明暗模式：`light` / `dark` / `auto`。
    pub theme: Option<String>,
    /// 配色方案，见 [`crate::settings::PALETTES`]。
    pub palette: Option<String>,
    /// 视觉列数：`auto` 或 `2`–`8`。
    pub columns: Option<String>,
    /// 上次打开的面板 id，用于刷新后恢复现场。
    pub active_board: Option<i64>,
    /// 面板 id -> 瀑布流排版（`[[分组 id, ...], ...]`）。
    #[serde(default)]
    pub layouts: HashMap<i64, Vec<Vec<i64>>>,
}

/// 偏好设置的部分更新：只带需要修改的字段，`None` 表示保持原值。
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct SettingsUpdate {
    pub theme: Option<String>,
    pub palette: Option<String>,
    pub columns: Option<String>,
    pub active_board: Option<i64>,
}

/// 单个面板的瀑布流排版更新请求体。
#[derive(Debug, Deserialize)]
pub struct LayoutUpdate {
    /// 每一列包含哪些分组 id。
    pub layout: Vec<Vec<i64>>,
}
