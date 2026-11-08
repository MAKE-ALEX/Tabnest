//! URL normalisation shared by the server, the worker and the bookmark import.

use url::Url;

pub fn normalize_url(input: &str) -> Result<String, String> {
    let raw = input.trim();
    if raw.is_empty() {
        return Err("请输入网址".into());
    }
    if raw.len() > 2048 {
        return Err("网址太长（最多 2048 个字符）".into());
    }
    let candidate = if raw.starts_with("http://") || raw.starts_with("https://") {
        raw.to_string()
    } else if raw.contains("://") {
        return Err("仅支持 http / https 网址".into());
    } else {
        format!("https://{raw}")
    };
    let url = Url::parse(&candidate).map_err(|_| "网址格式不正确".to_string())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("仅支持 http / https 网址".into());
    }
    if url.host_str().unwrap_or("").is_empty() {
        return Err("网址缺少域名".into());
    }
    Ok(url.to_string())
}

pub fn host_label(url: &str) -> String {
    Url::parse(url)
        .ok()
        .and_then(|url| {
            url.host_str()
                .map(|host| host.trim_start_matches("www.").to_string())
        })
        .filter(|host| !host.is_empty())
        .unwrap_or_else(|| url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_bare_domains() {
        assert_eq!(normalize_url("example.com").unwrap(), "https://example.com/");
        assert_eq!(
            normalize_url("http://example.com/a").unwrap(),
            "http://example.com/a"
        );
        assert!(normalize_url("javascript:alert(1)").is_err());
        assert!(normalize_url("   ").is_err());
    }

    #[test]
    fn labels_hosts_without_www() {
        assert_eq!(host_label("https://www.rust-lang.org/x"), "rust-lang.org");
        assert_eq!(host_label("not a url"), "not a url");
    }
}
