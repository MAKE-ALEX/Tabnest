//! Signed, stateless session cookies.
//!
//! The native server keeps sessions in memory; the Workers build has no
//! durable process, so it carries the expiry **inside** an HMAC-signed cookie
//! instead. Forging a token requires the server-side secret.

use hmac::{Hmac, Mac};
use sha2::Sha256;

pub const COOKIE_NAME: &str = "tabnest_session";
pub const HOST_COOKIE_NAME: &str = "__Host-tabnest_session";
pub const SESSION_TTL_SECS: u64 = 7 * 24 * 60 * 60;

/// Browser caches the `__Host-` name only over HTTPS. The worker always runs
/// behind TLS, so it uses the stricter prefix.
pub fn cookie_name(secure: bool) -> &'static str {
    if secure {
        HOST_COOKIE_NAME
    } else {
        COOKIE_NAME
    }
}

pub fn session_cookie(token: &str, secure: bool) -> String {
    let secure_flag = if secure { "; Secure" } else { "" };
    format!(
        "{name}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={SESSION_TTL_SECS}{secure_flag}",
        name = cookie_name(secure)
    )
}

pub fn clear_cookie(secure: bool) -> String {
    let secure_flag = if secure { "; Secure" } else { "" };
    format!(
        "{name}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{secure_flag}",
        name = cookie_name(secure)
    )
}

fn sign(secret: &[u8], payload: &str) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("hmac accepts any key length");
    mac.update(payload.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

/// Build `v1.<expires_at_secs>.<nonce>.<hmac-hex>`.
pub fn issue_token(secret: &[u8], now_ms: i64, nonce: &str) -> String {
    let exp = now_ms / 1000 + SESSION_TTL_SECS as i64;
    let payload = format!("v1.{exp}.{nonce}");
    let signature = hex::encode(sign(secret, &payload));
    format!("{payload}.{signature}")
}

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

/// Read one cookie value out of a raw `Cookie` header.
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
