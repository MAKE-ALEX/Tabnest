//! 服务端、Worker 与书签导入共用的网址规范化逻辑。

use url::Url;

/// 把用户输入整理成合法、可存储的 http(s) 网址。
///
/// 规则：
/// - 去掉首尾空白；空输入或超过 2048 字符直接拒绝；
/// - 没写协议的输入（如 `example.com`）默认补上 `https://`；
/// - 只接受 `http` / `https`，其他协议（`javascript:`、`file:` 等）一律拒绝；
/// - 解析后必须能取到域名。
pub fn normalize_url(input: &str) -> Result<String, String> {
    // 先做基本尺寸校验，避免把超长内容带进解析器。
    let raw = input.trim();
    if raw.is_empty() {
        return Err("请输入网址".into());
    }
    if raw.len() > 2048 {
        return Err("网址太长（最多 2048 个字符）".into());
    }
    // 显式写了协议就原样使用；写了其他协议则直接拒绝；
    // 完全没写协议时按 https 处理，这是用户最常见的输入方式。
    let candidate = if raw.starts_with("http://") || raw.starts_with("https://") {
        raw.to_string()
    } else if raw.contains("://") {
        return Err("仅支持 http / https 网址".into());
    } else {
        format!("https://{raw}")
    };
    let url = Url::parse(&candidate).map_err(|_| "网址格式不正确".to_string())?;
    // 解析成功还不够：`Url` 也接受其他协议，这里再确认一次。
    if !matches!(url.scheme(), "http" | "https") {
        return Err("仅支持 http / https 网址".into());
    }
    if url.host_str().unwrap_or("").is_empty() {
        return Err("网址缺少域名".into());
    }
    Ok(url.to_string())
}

/// 从网址中提取用于展示的域名，并去掉 `www.` 前缀。
///
/// 解析失败（例如导入数据里本来就是一段普通文本）时原样返回输入，
/// 保证卡片标题区域总有内容可显示。
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
