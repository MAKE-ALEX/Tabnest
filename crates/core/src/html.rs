//! Best-effort extraction of a page title and icon from raw HTML.

use std::sync::OnceLock;

use regex::Regex;
use url::Url;

#[derive(Debug, Default, Clone)]
pub struct PageMeta {
    pub title: Option<String>,
    pub icon: Option<String>,
}

fn title_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?is)<title[^>]*>(.*?)</title>").expect("title regex"))
}

fn link_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?is)<link\b[^>]*>").expect("link regex"))
}

fn attr_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?is)([a-zA-Z_:][-a-zA-Z0-9_:.]*)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'>]+))"#)
            .expect("attr regex")
    })
}

fn entity_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"&(#[xX]?[0-9A-Fa-f]+|[a-zA-Z][a-zA-Z0-9]{1,31});").expect("entity regex")
    })
}

/// Pick one attribute out of an HTML tag, accepting double, single or no quotes.
pub fn attr_value(tag: &str, name: &str) -> Option<String> {
    for captures in attr_re().captures_iter(tag) {
        if captures
            .get(1)
            .map(|m| m.as_str().eq_ignore_ascii_case(name))
            .unwrap_or(false)
        {
            return captures
                .get(2)
                .or_else(|| captures.get(3))
                .or_else(|| captures.get(4))
                .map(|m| m.as_str().to_string());
        }
    }
    None
}

pub fn extract_title(html: &str) -> Option<String> {
    let captures = title_re().captures(html)?;
    let raw = captures.get(1)?.as_str();
    let decoded = decode_entities(raw);
    let collapsed = decoded.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = collapsed.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(truncate_chars(trimmed, 300))
    }
}

pub fn extract_icon(html: &str, page_url: &str) -> Option<String> {
    let base = Url::parse(page_url).ok()?;
    let mut candidates: Vec<(u8, String)> = Vec::new();

    for tag in link_re().find_iter(html) {
        let tag = tag.as_str();
        let Some(rel) = attr_value(tag, "rel").map(|value| value.to_ascii_lowercase()) else {
            continue;
        };
        if !rel.contains("icon") {
            continue;
        }
        let Some(href) = attr_value(tag, "href") else {
            continue;
        };
        let href = href.trim();
        if href.is_empty() || href.starts_with("data:") {
            continue;
        }
        let Ok(icon) = base.join(href) else {
            continue;
        };
        if !matches!(icon.scheme(), "http" | "https") {
            continue;
        }
        let score = if rel.split_whitespace().any(|part| part == "icon") {
            3
        } else if rel.contains("shortcut") {
            2
        } else {
            1
        };
        candidates.push((score, icon.to_string()));
    }

    candidates.sort_by(|left, right| right.0.cmp(&left.0));
    if let Some((_, icon)) = candidates.into_iter().next() {
        return Some(icon);
    }
    base.join("/favicon.ico").ok().map(|url| url.to_string())
}

pub fn truncate_chars(value: &str, max: usize) -> String {
    let mut chars = value.chars();
    let truncated: String = chars.by_ref().take(max).collect();
    if chars.next().is_some() {
        format!("{}…", truncated.trim_end())
    } else {
        truncated
    }
}

pub fn decode_entities(input: &str) -> String {
    entity_re()
        .replace_all(input, |captures: &regex::Captures<'_>| {
            let name = &captures[1];
            match name {
                "amp" => "&".to_string(),
                "lt" => "<".to_string(),
                "gt" => ">".to_string(),
                "quot" => "\"".to_string(),
                "apos" => "'".to_string(),
                "nbsp" => " ".to_string(),
                "mdash" => "—".to_string(),
                "ndash" => "–".to_string(),
                "hellip" => "…".to_string(),
                "middot" => "·".to_string(),
                "copy" => "©".to_string(),
                "reg" => "®".to_string(),
                "trade" => "™".to_string(),
                _ => {
                    if let Some(rest) = name.strip_prefix('#') {
                        let code = if let Some(hex) = rest
                            .strip_prefix('x')
                            .or_else(|| rest.strip_prefix('X'))
                        {
                            u32::from_str_radix(hex, 16).ok()
                        } else {
                            rest.parse::<u32>().ok()
                        };
                        code.and_then(char::from_u32)
                            .map(|ch| ch.to_string())
                            .unwrap_or_else(|| captures[0].to_string())
                    } else {
                        captures[0].to_string()
                    }
                }
            }
        })
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_title_and_icon() {
        let html = r#"<html><head><title> Hello &amp; World </title>
            <link rel="icon" href="/static/icon.png"></head></html>"#;
        assert_eq!(extract_title(html).as_deref(), Some("Hello & World"));
        assert_eq!(
            extract_icon(html, "https://example.com/page").as_deref(),
            Some("https://example.com/static/icon.png")
        );
    }

    #[test]
    fn falls_back_to_favicon_ico() {
        assert_eq!(
            extract_icon("<html></html>", "https://example.com/a/b").as_deref(),
            Some("https://example.com/favicon.ico")
        );
    }

    #[test]
    fn decodes_numeric_entities() {
        assert_eq!(decode_entities("a&#65;&#x42;"), "aAB");
        assert_eq!(decode_entities("&unknown;"), "&unknown;");
    }

    #[test]
    fn truncates_on_char_boundaries() {
        assert_eq!(truncate_chars("abcdef", 3), "abc…");
        assert_eq!(truncate_chars("中文测试", 2), "中文…");
        assert_eq!(truncate_chars("abc", 10), "abc");
    }
}
