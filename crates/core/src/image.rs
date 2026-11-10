//! favicon 代理使用的图片类型嗅探。
//!
//! 这里故意不支持 SVG：如果把攻击者可控的 SVG 从本站域名直接返回，
//! favicon 代理就会变成同源 XSS 的入口。只放行常见的位图格式。

/// 根据文件头（magic bytes）判断图片类型；识别不了时再退回 `Content-Type`。
///
/// 返回值是可以直接写进 `Content-Type` 响应头的 MIME 字符串。
pub fn sniff_image(bytes: &[u8], content_type: &str) -> Option<&'static str> {
    // PNG：89 50 4E 47。
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        return Some("image/png");
    }
    // JPEG：FF D8 FF。
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }
    // GIF87a / GIF89a。
    if bytes.starts_with(b"GIF8") {
        return Some("image/gif");
    }
    // WebP：RIFF....WEBP，需要同时检查第 8–12 字节。
    if bytes.len() > 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    // ICO：00 00 01 00。
    if bytes.starts_with(&[0x00, 0x00, 0x01, 0x00]) {
        return Some("image/x-icon");
    }

    // 文件头不可用时，把 Content-Type 当作弱依据（去掉 charset 等参数）。
    let content_type = content_type.split(';').next().unwrap_or("").trim();
    match content_type {
        "image/png" => Some("image/png"),
        "image/jpeg" | "image/jpg" => Some("image/jpeg"),
        "image/gif" => Some("image/gif"),
        "image/webp" => Some("image/webp"),
        "image/x-icon" | "image/vnd.microsoft.icon" => Some("image/x-icon"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_svg_and_non_images() {
        assert_eq!(sniff_image(b"<svg xmlns=\"...\">", ""), None);
        assert_eq!(sniff_image(b"<html>", "text/html"), None);
        assert_eq!(
            sniff_image(b"<?xml version=\"1.0\"?><svg>", "image/svg+xml"),
            None
        );
    }

    #[test]
    fn detects_raster_images() {
        assert_eq!(sniff_image(b"\x89PNG\r\n\x1a\n", ""), Some("image/png"));
        assert_eq!(sniff_image(b"GIF89a", ""), Some("image/gif"));
        assert_eq!(sniff_image(b"data", "image/webp"), Some("image/webp"));
    }
}
