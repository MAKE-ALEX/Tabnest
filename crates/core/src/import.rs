//! 浏览器导出文件与 Tabnest 自身导出数据的解析逻辑。

use std::sync::OnceLock;

use base64::Engine as _;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::html::{attr_value, decode_entities, truncate_chars};

/// 一次 JSON 导入的完整内容。
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ImportPayload {
    pub boards: Vec<ImportBoard>,
    /// Tabnest 导出文件里可能附带的外观与瀑布流排版。
    pub appearance: Option<ImportAppearance>,
}

/// 随导入一起恢复的外观偏好。
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct ImportAppearance {
    pub theme: Option<String>,
    pub palette: Option<String>,
    pub columns: Option<String>,
    pub active_board: Option<i64>,
}

/// 导入文件中的一个面板。
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ImportBoard {
    pub name: String,
    pub columns: Vec<ImportColumn>,
    /// 瀑布流排版，用分组**名称**表示：`[["网盘", "邮箱"], ["云平台"]]`。
    /// 之所以不用 id，是因为导入时数据库会重新分配 id。
    pub layout: Option<Vec<Vec<String>>>,
}

/// 导入文件中的一个分组。
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ImportColumn {
    pub name: String,
    pub links: Vec<ImportLink>,
}

/// 导入文件中的一张书签。
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ImportLink {
    pub title: String,
    pub url: String,
    pub note: String,
    pub icon: Option<String>,
}

/// 已经真正写入数据库的面板，附带恢复排版所需的信息。
#[derive(Debug, Default)]
pub struct ImportedBoard {
    pub name: String,
    /// 新分配的面板 id。
    pub id: i64,
    /// 分组「名称 -> 新 id」的对应关系。
    pub columns: Vec<(String, i64)>,
    pub layout: Option<Vec<Vec<String>>>,
}

/// 以 base64 注释形式嵌在 Netscape HTML 导出文件里的排版 + 外观信息。
/// 浏览器会忽略注释，Tabnest 重新导入时再读回来，这样 HTML 备份也能保留
/// 视觉排版。
pub const HTML_META_PREFIX: &str = "tabnest-meta:";

/// HTML 注释里携带的元信息。
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LayoutMeta {
    pub appearance: Option<ImportAppearance>,
    pub boards: Vec<BoardLayoutMeta>,
}

/// 单个面板在 HTML 注释中保存的排版。
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BoardLayoutMeta {
    pub name: String,
    pub layout: Vec<Vec<String>>,
}

impl LayoutMeta {
    /// 序列化成 `<!-- tabnest-meta:<base64> -->` 形式的 HTML 注释。
    pub fn to_comment(&self) -> String {
        let json = serde_json::to_vec(self).unwrap_or_default();
        let encoded = base64::engine::general_purpose::STANDARD.encode(json);
        format!("<!-- {HTML_META_PREFIX}{encoded} -->")
    }
}

/// 读取 [`LayoutMeta::to_comment`] 写出的元信息注释；不存在或格式错误时返回 `None`。
pub fn parse_html_meta(html: &str) -> Option<LayoutMeta> {
    let start = html.find(HTML_META_PREFIX)? + HTML_META_PREFIX.len();
    let rest = &html[start..];
    let end = rest.find("-->")?;
    let encoded = rest[..end].trim();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// 把元信息里的面板与本次实际导入的面板按名称配对。
///
/// 按顺序消费元信息，这样即使有重名面板，各自也能拿回自己的排版。
/// 返回 `(面板 id, 分组 id 排版)` 列表。
pub fn match_layouts(
    meta: &LayoutMeta,
    imported: &[ImportedBoard],
) -> Vec<(i64, Vec<Vec<i64>>)> {
    // 记录元信息中的每个面板是否已经被匹配过。
    let mut remaining: Vec<bool> = vec![true; meta.boards.len()];
    let mut result = Vec::new();

    for board in imported {
        let Some(index) = meta
            .boards
            .iter()
            .enumerate()
            .position(|(index, candidate)| remaining[index] && candidate.name == board.name)
        else {
            continue;
        };
        remaining[index] = false;
        let resolved = resolve_layout(&meta.boards[index].layout, &board.columns);
        // 排版里至少要有一个非空分组才值得写回数据库。
        if resolved.iter().any(|group| !group.is_empty()) {
            result.push((board.id, resolved));
        }
    }

    result
}

/// 解析 Tabnest 的 JSON 导出文件。
pub fn parse_json(text: &str) -> anyhow::Result<ImportPayload> {
    Ok(serde_json::from_str(text)?)
}

/// 把 `[[分组名称, ...], ...]` 映射成新插入分组对应的 id。
///
/// 重名分组按「首次出现」的顺序匹配，因此一个面板里有两个「全部」分组时，
/// 也能还原各自的准确位置。
pub fn resolve_layout(layout: &[Vec<String>], columns: &[(String, i64)]) -> Vec<Vec<i64>> {
    // 每个分组 id 只能用一次，用过就置为 None。
    let mut remaining: Vec<Option<i64>> = columns.iter().map(|(_, id)| Some(*id)).collect();

    layout
        .iter()
        .map(|group| {
            group
                .iter()
                .filter_map(|name| {
                    let position = columns
                        .iter()
                        .enumerate()
                        .position(|(index, (column_name, _))| {
                            column_name == name && remaining[index].is_some()
                        })?;
                    remaining[position].take()
                })
                .collect()
        })
        .collect()
}

/// 一次匹配 Netscape 书签文件中的一个结构：文件夹标题、书签链接、
/// `<dl>` 开始或结束。
fn tag_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?is)<dt[^>]*>\s*<h3[^>]*>(?P<h3>.*?)</h3>|<dt[^>]*>\s*<a\b(?P<attrs>[^>]*)>(?P<title>.*?)</a>|(?P<dl_open><dl\b[^>]*>)|(?P<dl_close></dl\s*>)",
        )
        .expect("netscape regex")
    })
}

/// 解析 Netscape 书签文件（所有主流浏览器都能导出的格式）。
///
/// 层级映射刻意保持简单：
///   * 一级文件夹 -> 面板；
///   * 更深层的文件夹 -> 分组（再嵌套时用 `父 / 子` 拼接名称）；
///   * 不在任何文件夹里的书签 -> 「导入的书签 / 全部」。
pub fn parse_netscape(html: &str) -> Vec<ImportBoard> {
    let mut boards: Vec<ImportBoard> = Vec::new();
    // 当前打开的文件夹路径，下标对应嵌套层级。
    let mut folders: Vec<String> = Vec::new();
    // `<dl>` 嵌套深度。
    let mut depth: usize = 0;

    for captures in tag_re().captures_iter(html) {
        if captures.name("dl_open").is_some() {
            depth += 1;
            continue;
        }
        if captures.name("dl_close").is_some() {
            depth = depth.saturating_sub(1);
            // 列表已经结束，里面的书签不可能再属于拥有该列表的文件夹。
            folders.truncate(depth.saturating_sub(1));
            continue;
        }

        if let Some(folder) = captures.name("h3") {
            let name = clean_text(folder.as_str());
            if name.is_empty() {
                continue;
            }
            // `<dl>` 的深度比文件夹层级多 1。
            let level = depth.max(1) - 1;
            // 新文件夹会覆盖同一层及更深的旧路径。
            if folders.len() > level {
                folders.truncate(level);
            }
            // 层级跳跃时用空字符串补位，保持下标与层级对应。
            while folders.len() < level {
                folders.push(String::new());
            }
            folders.push(name);
            continue;
        }

        let Some(attrs) = captures.name("attrs") else {
            continue;
        };
        let Some(href) = attr_value(attrs.as_str(), "href") else {
            continue;
        };
        let href = decode_entities(href.trim());
        // 只导入真正的网页书签，跳过 javascript: / place: 等内部地址。
        if !(href.starts_with("http://") || href.starts_with("https://")) {
            continue;
        }
        let title = captures
            .name("title")
            .map(|value| clean_text(value.as_str()))
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| href.clone());

        // 一级文件夹名就是面板名；没有文件夹时归入默认面板。
        let board_name = folders
            .first()
            .filter(|name| !name.is_empty())
            .cloned()
            .unwrap_or_else(|| "导入的书签".to_string());
        // 除一级文件夹外的所有层级拼成分组名。
        let contexts: Vec<&str> = folders
            .iter()
            .take(depth.saturating_sub(1))
            .skip(1)
            .map(String::as_str)
            .filter(|name| !name.is_empty())
            .collect();
        let column_name = if contexts.is_empty() {
            "全部".to_string()
        } else {
            contexts.join(" / ")
        };

        // 复用同名面板，不存在则新建（最多 500 个，防止异常文件撑爆内存）。
        let board_index = match boards.iter().position(|board| board.name == board_name) {
            Some(index) => index,
            None => {
                if boards.len() >= 500 {
                    continue;
                }
                boards.push(ImportBoard {
                    name: board_name,
                    columns: Vec::new(),
                    layout: None,
                });
                boards.len() - 1
            }
        };
        let board = &mut boards[board_index];
        // 同理复用同名分组。
        let column_index = match board
            .columns
            .iter()
            .position(|column| column.name == column_name)
        {
            Some(index) => index,
            None => {
                board.columns.push(ImportColumn {
                    name: column_name,
                    links: Vec::new(),
                });
                board.columns.len() - 1
            }
        };
        board.columns[column_index].links.push(ImportLink {
            title,
            url: href,
            note: String::new(),
            icon: None,
        });
    }

    boards
}

/// 清理 HTML 中的文本：解码实体、折叠空白、截断到 300 字符。
fn clean_text(raw: &str) -> String {
    let decoded = decode_entities(raw);
    let collapsed = decoded.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_chars(collapsed.trim(), 300)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_browser_export() {
        let html = r#"<!DOCTYPE NETSCAPE-Bookmark-file-1>
<DL><p>
  <DT><H3>工作</H3>
  <DL><p>
    <DT><H3>Rust</H3>
    <DL><p>
      <DT><A HREF="https://www.rust-lang.org/" ADD_DATE="1">Rust &amp; Cargo</A>
    </DL><p>
    <DT><A HREF="https://github.com/">GitHub</A>
  </DL><p>
  <DT><H3>生活</H3>
  <DL><p>
    <DT><A HREF="https://www.bilibili.com/">哔哩哔哩</A>
    <DT><A HREF="javascript:void(0)">bad</A>
  </DL><p>
</DL><p>"#;
        let boards = parse_netscape(html);
        assert_eq!(boards.len(), 2);
        assert_eq!(boards[0].name, "工作");
        assert_eq!(boards[0].columns.len(), 2);
        assert_eq!(boards[0].columns[0].name, "Rust");
        assert_eq!(boards[0].columns[0].links[0].title, "Rust & Cargo");
        assert_eq!(boards[0].columns[1].name, "全部");
        assert_eq!(boards[1].name, "生活");
        assert_eq!(boards[1].columns[0].links.len(), 1);
    }

    #[test]
    fn parses_tabnest_json() {
        let payload = r#"{"boards":[{"name":"B","columns":[{"name":"C","links":[
            {"title":"T","url":"https://example.com/","note":"n","icon":null}]}],
            "layout":[["C"]]}],
            "appearance":{"theme":"dark","palette":"ocean","columns":"4"}}"#;
        let parsed = parse_json(payload).unwrap();
        assert_eq!(parsed.boards[0].columns[0].links[0].title, "T");
        assert_eq!(
            parsed.boards[0].layout.as_deref(),
            Some([vec!["C".to_string()]].as_slice())
        );
        let appearance = parsed.appearance.expect("appearance");
        assert_eq!(appearance.theme.as_deref(), Some("dark"));
    }

    #[test]
    fn resolves_layout_names_to_ids() {
        let columns = vec![
            ("网盘".to_string(), 10i64),
            ("邮箱".to_string(), 11i64),
            ("网盘".to_string(), 12i64),
        ];
        let layout = vec![
            vec!["网盘".to_string(), "邮箱".to_string()],
            vec!["网盘".to_string()],
        ];
        assert_eq!(
            resolve_layout(&layout, &columns),
            vec![vec![10, 11], vec![12]]
        );
    }

    #[test]
    fn html_meta_round_trips() {
        let meta = LayoutMeta {
            appearance: Some(ImportAppearance {
                theme: Some("dark".to_string()),
                palette: Some("ocean".to_string()),
                columns: Some("8".to_string()),
                active_board: None,
            }),
            boards: vec![BoardLayoutMeta {
                name: "Bookmarks".to_string(),
                layout: vec![
                    vec!["网盘".to_string(), "邮箱".to_string()],
                    vec!["翻译".to_string()],
                ],
            }],
        };
        let html = format!(
            "<!DOCTYPE NETSCAPE-Bookmark-file-1>\n{}\n<DL><p></DL><p>",
            meta.to_comment()
        );
        let parsed = parse_html_meta(&html).expect("meta");
        assert_eq!(parsed.appearance.unwrap().theme.as_deref(), Some("dark"));
        assert_eq!(parsed.boards[0].name, "Bookmarks");
        assert_eq!(parsed.boards[0].layout[0], vec!["网盘", "邮箱"]);
    }

    #[test]
    fn matches_layouts_by_board_name() {
        let meta = LayoutMeta {
            appearance: None,
            boards: vec![
                BoardLayoutMeta {
                    name: "同名".to_string(),
                    layout: vec![vec!["A".to_string()]],
                },
                BoardLayoutMeta {
                    name: "同名".to_string(),
                    layout: vec![vec!["B".to_string()]],
                },
            ],
        };
        let imported = vec![
            ImportedBoard {
                name: "同名".to_string(),
                id: 1,
                columns: vec![("A".to_string(), 11)],
                layout: None,
            },
            ImportedBoard {
                name: "同名".to_string(),
                id: 2,
                columns: vec![("B".to_string(), 22)],
                layout: None,
            },
        ];
        assert_eq!(match_layouts(&meta, &imported), vec![(1, vec![vec![11]]), (2, vec![vec![22]])]);
    }
}
