//! Image type detection for the favicon proxy.
//!
//! SVG is deliberately excluded: serving attacker-controlled SVG from the app
//! origin would turn the favicon proxy into a same-origin XSS vector.

/// Detect an image type from magic bytes, falling back to the `Content-Type`.
pub fn sniff_image(bytes: &[u8], content_type: &str) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        return Some("image/png");
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }
    if bytes.starts_with(b"GIF8") {
        return Some("image/gif");
    }
    if bytes.len() > 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    if bytes.starts_with(&[0x00, 0x00, 0x01, 0x00]) {
        return Some("image/x-icon");
    }

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
