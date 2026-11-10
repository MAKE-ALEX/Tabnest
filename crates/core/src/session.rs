//! 带签名的无状态会话 Cookie。
//!
//! 原生服务端把会话放在内存里；Workers 构建没有常驻进程，无法在内存中保存会话，
//! 所以把过期时间放进 **HMAC 签名过的 Cookie** 里随请求携带。
//! 想伪造令牌就必须拿到服务端的签名密钥。

use hmac::{Hmac, Mac};
use sha2::Sha256;

pub const COOKIE_NAME: &str = "tabnest_session";
pub const HOST_COOKIE_NAME: &str = "__Host-tabnest_session";
/// 会话有效期：7 天。
pub const SESSION_TTL_SECS: u64 = 7 * 24 * 60 * 60;

/// 选择 Cookie 名称。
///
/// `__Host-` 前缀只在 HTTPS 下才有效，浏览器也要求它必须带 `Secure`、
/// 且不能指定 `Domain`。Worker 始终运行在 TLS 之后，因此使用更严格的名称。
pub fn cookie_name(secure: bool) -> &'static str {
    if secure {
        HOST_COOKIE_NAME
    } else {
        COOKIE_NAME
    }
}

/// 生成写入会话 Cookie 的 `Set-Cookie` 值。
pub fn session_cookie(token: &str, secure: bool) -> String {
    let secure_flag = if secure { "; Secure" } else { "" };
    format!(
        "{name}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={SESSION_TTL_SECS}{secure_flag}",
        name = cookie_name(secure)
    )
}

/// 生成清除会话 Cookie 的 `Set-Cookie` 值（`Max-Age=0` 让浏览器立即删除）。
pub fn clear_cookie(secure: bool) -> String {
    let secure_flag = if secure { "; Secure" } else { "" };
    format!(
        "{name}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{secure_flag}",
        name = cookie_name(secure)
    )
}

/// 用服务端密钥对 payload 做 HMAC-SHA256，返回原始签名字节。
fn sign(secret: &[u8], payload: &str) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("hmac accepts any key length");
    mac.update(payload.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

/// 生成令牌，格式为 `v1.<过期时间秒>.<随机串>.<签名十六进制>`。
pub fn issue_token(secret: &[u8], now_ms: i64, nonce: &str) -> String {
    let exp = now_ms / 1000 + SESSION_TTL_SECS as i64;
    let payload = format!("v1.{exp}.{nonce}");
    let signature = hex::encode(sign(secret, &payload));
    format!("{payload}.{signature}")
}

/// 校验令牌：格式正确、未过期，且签名与重新计算的结果一致。
///
/// 签名比较使用常量时间的 `verify_slice`，避免时序侧信道。
pub fn verify_token(secret: &[u8], token: &str, now_ms: i64) -> bool {
    let mut parts = token.splitn(4, '.');
    let (Some("v1"), Some(exp), Some(nonce), Some(signature)) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    let Ok(exp) = exp.parse::<i64>() else {
        return false;
    };
    // 已过期（或恰好到期）的令牌直接拒绝。
    if exp <= now_ms / 1000 {
        return false;
    }
    let Ok(signature) = hex::decode(signature) else {
        return false;
    };
    let payload = format!("v1.{exp}.{nonce}");
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("hmac accepts any key length");
    mac.update(payload.as_bytes());
    mac.verify_slice(&signature).is_ok()
}

/// 从原始 `Cookie` 请求头中取出指定名称的值。
///
/// 只做简单切分，调用方需要保证 `name` 是可信常量。
pub fn extract_cookie<'a>(cookie_header: &'a str, name: &str) -> Option<&'a str> {
    for part in cookie_header.split(';') {
        let mut kv = part.trim().splitn(2, '=');
        if kv.next() == Some(name) {
            return kv.next().map(str::trim);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = b"test-secret";

    #[test]
    fn round_trips_a_fresh_token() {
        let now = 1_700_000_000_000i64;
        let token = issue_token(SECRET, now, "abc123");
        assert!(verify_token(SECRET, &token, now));
        assert!(verify_token(SECRET, &token, now + 60_000));
    }

    #[test]
    fn rejects_tampering_and_expiry() {
        let now = 1_700_000_000_000i64;
        let token = issue_token(SECRET, now, "abc123");
        let tampered = token.replace(".abc123.", ".abc124.");
        assert!(!verify_token(SECRET, &tampered, now));
        assert!(!verify_token(b"other-secret", &token, now));
        assert!(!verify_token(SECRET, &token, now + 8 * 24 * 3600 * 1000));
        assert!(!verify_token(SECRET, "garbage", now));
    }

    #[test]
    fn reads_cookie_values() {
        let header = "a=1; tabnest_session=xyz; __Host-tabnest_session=q";
        assert_eq!(extract_cookie(header, "tabnest_session"), Some("xyz"));
        assert_eq!(extract_cookie(header, "__Host-tabnest_session"), Some("q"));
        assert_eq!(extract_cookie(header, "missing"), None);
    }

    #[test]
    fn cookie_attributes() {
        assert!(session_cookie("t", true).contains("__Host-tabnest_session=t"));
        assert!(session_cookie("t", true).contains("; Secure"));
        assert!(!session_cookie("t", false).contains("Secure"));
        assert!(clear_cookie(true).contains("Max-Age=0"));
    }
}
