//! Parsing of bookmark payloads produced by browsers and by Tabnest itself.

use std::sync::OnceLock;

use regex::Regex;
use serde::Deserialize;

use crate::html::{attr_value, decode_entities, truncate_chars};

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ImportPayload {
    pub boards: Vec<ImportBoard>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ImportBoard {
    pub name: String,
    pub columns: Vec<ImportColumn>,
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

pub fn parse_json(text: &str) -> anyhow::Result<Vec<ImportBoard>> {
    let payload: ImportPayload = serde_json::from_str(text)?;
    Ok(payload.boards)
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
            {"title":"T","url":"https://example.com/","note":"n","icon":null}]}]}]}"#;
        let boards = parse_json(payload).unwrap();
        assert_eq!(boards[0].columns[0].links[0].title, "T");
    }
}
