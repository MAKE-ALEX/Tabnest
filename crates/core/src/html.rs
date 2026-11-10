//! 从原始 HTML 中尽力提取网页标题和图标地址。

use std::sync::OnceLock;

use regex::Regex;
use url::Url;

/// 从页面中解析出的元信息。
#[derive(Debug, Default, Clone)]
pub struct PageMeta {
    /// `<title>` 文本；页面没有标题时为 `None`。
    pub title: Option<String>,
    /// 图标绝对地址；页面没有声明图标时为 `None`。
    pub icon: Option<String>,
}

/// 匹配 `<title>…</title>`（点号匹配换行，忽略大小写）。
fn title_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?is)<title[^>]*>(.*?)</title>").expect("title regex"))
}

/// 匹配任意 `<link …>` 标签，后续再从中挑出图标声明。
fn link_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?is)<link\b[^>]*>").expect("link regex"))
}

/// 匹配标签内的 `name="value"` / `name='value'` / `name=value` 属性。
fn attr_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?is)([a-zA-Z_:][-a-zA-Z0-9_:.]*)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'>]+))"#)
            .expect("attr regex")
    })
}

/// 匹配 HTML 实体，如 `&amp;`、`&#65;`、`&#x42;`。
fn entity_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"&(#[xX]?[0-9A-Fa-f]+|[a-zA-Z][a-zA-Z0-9]{1,31});").expect("entity regex")
    })
}

/// 从 HTML 标签中取出指定属性值。
///
/// 同时支持双引号、单引号和无引号三种写法；属性名不区分大小写，
/// 找不到时返回 `None`。
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

/// 提取页面标题：解码实体、折叠连续空白、去掉首尾空格，并截断到 300 字符。
pub fn extract_title(html: &str) -> Option<String> {
    let captures = title_re().captures(html)?;
    let raw = captures.get(1)?.as_str();
    let decoded = decode_entities(raw);
    // 把换行、制表符等连续空白压成一个空格。
    let collapsed = decoded.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = collapsed.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(truncate_chars(trimmed, 300))
    }
}

/// 找出页面声明的图标地址；都没有时回退到站点根目录的 `/favicon.ico`。
///
/// 会把相对地址按页面 URL 解析成绝对地址，并拒绝 `data:` 及非 http(s) 地址。
/// 多个候选时按 `rel` 的常见写法排序：`rel="icon"` > `shortcut icon` > 其他含
/// `icon` 的写法。
pub fn extract_icon(html: &str, page_url: &str) -> Option<String> {
    let base = Url::parse(page_url).ok()?;
    let mut candidates: Vec<(u8, String)> = Vec::new();

    for tag in link_re().find_iter(html) {
        let tag = tag.as_str();
        // 没有 rel 的 link 标签与图标无关。
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
        // 内联 data: 图标不能直接交给 favicon 代理缓存。
        if href.is_empty() || href.starts_with("data:") {
            continue;
        }
        // 相对地址转成绝对地址；解析失败则跳过这个候选。
        let Ok(icon) = base.join(href) else {
            continue;
        };
        if !matches!(icon.scheme(), "http" | "https") {
            continue;
        }
        // 标准写法优先级最高，其次是包含 shortcut 的旧写法。
        let score = if rel.split_whitespace().any(|part| part == "icon") {
            3
        } else if rel.contains("shortcut") {
            2
        } else {
            1
        };
        candidates.push((score, icon.to_string()));
    }

    // 按优先级从高到低排序，取第一个候选。
    candidates.sort_by(|left, right| right.0.cmp(&left.0));
    if let Some((_, icon)) = candidates.into_iter().next() {
        return Some(icon);
    }
    // 页面没有声明图标时，按惯例尝试根目录的 favicon.ico。
    base.join("/favicon.ico").ok().map(|url| url.to_string())
}

/// 按「字符」而不是字节截断，避免把多字节字符切成乱码；
/// 发生截断时在末尾补一个省略号。
pub fn truncate_chars(value: &str, max: usize) -> String {
    let mut chars = value.chars();
    let truncated: String = chars.by_ref().take(max).collect();
    if chars.next().is_some() {
        format!("{}…", truncated.trim_end())
    } else {
        truncated
    }
}

/// 解码常见的 HTML 命名实体，以及十进制 / 十六进制数字实体。
///
/// 无法识别的实体保持原样，避免误伤普通文本。
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
                    // 数字实体：`&#65;`（十进制）或 `&#x42;` / `&#X42;`（十六进制）。
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
