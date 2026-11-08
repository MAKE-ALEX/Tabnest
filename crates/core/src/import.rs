//! Parsing of bookmark payloads produced by browsers and by Tabnest itself.

use std::sync::OnceLock;

use base64::Engine as _;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::html::{attr_value, decode_entities, truncate_chars};

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ImportPayload {
    pub boards: Vec<ImportBoard>,
    /// Optional appearance + masonry layout written by a Tabnest export.
    pub appearance: Option<ImportAppearance>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct ImportAppearance {
    pub theme: Option<String>,
    pub palette: Option<String>,
    pub columns: Option<String>,
    pub active_board: Option<i64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ImportBoard {
    pub name: String,
    pub columns: Vec<ImportColumn>,
    /// Masonry layout as column **names**: `[["网盘", "邮箱"], ["云平台"]]`.
    /// Names are used because ids are reassigned on import.
    pub layout: Option<Vec<Vec<String>>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ImportColumn {
    pub name: String,
    pub links: Vec<ImportLink>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ImportLink {
    pub title: String,
    pub url: String,
    pub note: String,
    pub icon: Option<String>,
}

/// A board that was actually written, with the data needed to restore its
/// layout after ids have been assigned.
#[derive(Debug, Default)]
pub struct ImportedBoard {
    pub name: String,
    pub id: i64,
    pub columns: Vec<(String, i64)>,
    pub layout: Option<Vec<Vec<String>>>,
}

/// Layout + appearance embedded in the Netscape HTML export as a base64
/// comment, so HTML backups keep the visual arrangement too (browsers ignore
/// the comment, Tabnest reads it back).
pub const HTML_META_PREFIX: &str = "tabnest-meta:";

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LayoutMeta {
    pub appearance: Option<ImportAppearance>,
    pub boards: Vec<BoardLayoutMeta>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BoardLayoutMeta {
    pub name: String,
    pub layout: Vec<Vec<String>>,
}

impl LayoutMeta {
    pub fn to_comment(&self) -> String {
        let json = serde_json::to_vec(self).unwrap_or_default();
        let encoded = base64::engine::general_purpose::STANDARD.encode(json);
        format!("<!-- {HTML_META_PREFIX}{encoded} -->")
    }
}

/// Read the metadata comment written by [`LayoutMeta::to_comment`].
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

/// Pair metadata boards with the boards this import created, matching by name
/// in order (so duplicate board names keep their own layout).
pub fn match_layouts(
    meta: &LayoutMeta,
    imported: &[ImportedBoard],
) -> Vec<(i64, Vec<Vec<i64>>)> {
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
        if resolved.iter().any(|group| !group.is_empty()) {
            result.push((board.id, resolved));
        }
    }

    result
}

pub fn parse_json(text: &str) -> anyhow::Result<ImportPayload> {
    Ok(serde_json::from_str(text)?)
}

/// Map `[[column name, ...], ...]` onto the freshly inserted column ids.
///
/// Duplicate names are matched in first-seen order so a board with two
/// "全部" columns still restores its exact arrangement.
pub fn resolve_layout(layout: &[Vec<String>], columns: &[(String, i64)]) -> Vec<Vec<i64>> {
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

fn tag_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?is)<dt[^>]*>\s*<h3[^>]*>(?P<h3>.*?)</h3>|<dt[^>]*>\s*<a\b(?P<attrs>[^>]*)>(?P<title>.*?)</a>|(?P<dl_open><dl\b[^>]*>)|(?P<dl_close></dl\s*>)",
        )
        .expect("netscape regex")
    })
}

/// Parse a Netscape bookmark file (the format every browser can export).
///
/// Folder mapping is intentionally simple:
///   * first-level folder  -> board
///   * deeper folders      -> column ("parent / child" when nested further)
///   * links outside any folder end up in "导入的书签 / 全部"
pub fn parse_netscape(html: &str) -> Vec<ImportBoard> {
    let mut boards: Vec<ImportBoard> = Vec::new();
    let mut folders: Vec<String> = Vec::new();
    let mut depth: usize = 0;

    for captures in tag_re().captures_iter(html) {
        if captures.name("dl_open").is_some() {
            depth += 1;
            continue;
        }
        if captures.name("dl_close").is_some() {
            depth = depth.saturating_sub(1);
            // Links inside the list we just left can no longer belong to the
            // folder that owned it.
            folders.truncate(depth.saturating_sub(1));
            continue;
        }

        if let Some(folder) = captures.name("h3") {
            let name = clean_text(folder.as_str());
            if name.is_empty() {
                continue;
            }
            let level = depth.max(1) - 1;
            if folders.len() > level {
                folders.truncate(level);
            }
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
        if !(href.starts_with("http://") || href.starts_with("https://")) {
            continue;
        }
        let title = captures
            .name("title")
            .map(|value| clean_text(value.as_str()))
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| href.clone());

        let board_name = folders
            .first()
            .filter(|name| !name.is_empty())
            .cloned()
            .unwrap_or_else(|| "导入的书签".to_string());
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
