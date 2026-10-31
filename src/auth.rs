use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    time::{Duration, Instant, SystemTime},
};

use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, HeaderValue},
    middleware::Next,
    response::Response,
};
use sha2::{Digest, Sha256};

use crate::{error::AppError, state::AppState};

pub const COOKIE_NAME: &str = "tabnest_session";
const SESSION_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);

const MAX_FAILURES: u32 = 5;
const FAILURE_WINDOW: Duration = Duration::from_secs(10 * 60);
const LOCKOUT: Duration = Duration::from_secs(15 * 60);

#[derive(Default)]
pub struct LoginLimiter {
    attempts: HashMap<IpAddr, Attempt>,
}

struct Attempt {
    failures: u32,
    window_start: Instant,
    locked_until: Option<Instant>,
    last_seen: Instant,
}

pub enum LoginCheck {
    Allowed,
    Locked(Duration),
}

impl LoginLimiter {
    pub fn check(&mut self, ip: IpAddr) -> LoginCheck {
        self.prune();
        let now = Instant::now();
        let Some(attempt) = self.attempts.get_mut(&ip) else {
            return LoginCheck::Allowed;
        };
        if let Some(until) = attempt.locked_until {
            if until > now {
                return LoginCheck::Locked(until.duration_since(now));
            }
            attempt.locked_until = None;
            attempt.failures = 0;
            attempt.window_start = now;
        }
        if now.duration_since(attempt.window_start) > FAILURE_WINDOW {
            attempt.failures = 0;
            attempt.window_start = now;
        }
        LoginCheck::Allowed
    }

    /// Returns the lockout duration when this failure triggered a lock.
    pub fn record_failure(&mut self, ip: IpAddr) -> Option<Duration> {
        let now = Instant::now();
        let attempt = self.attempts.entry(ip).or_insert_with(|| Attempt {
            failures: 0,
            window_start: now,
            locked_until: None,
            last_seen: now,
        });
        if now.duration_since(attempt.window_start) > FAILURE_WINDOW {
            attempt.failures = 0;
            attempt.window_start = now;
        }
        attempt.failures = attempt.failures.saturating_add(1);
        attempt.last_seen = now;
        if attempt.failures >= MAX_FAILURES {
            attempt.locked_until = Some(now + LOCKOUT);
            Some(LOCKOUT)
        } else {
            None
        }
    }

    pub fn record_success(&mut self, ip: IpAddr) {
        self.attempts.remove(&ip);
    }

    fn prune(&mut self) {
        let now = Instant::now();
        self.attempts.retain(|_, attempt| {
            if let Some(until) = attempt.locked_until {
                until > now
            } else {
                now.duration_since(attempt.last_seen) < FAILURE_WINDOW
            }
        });
    }
}

/// Resolve the client IP for login throttling. `X-Forwarded-For` is only
/// honoured when the operator explicitly enables `TRUST_PROXY`.
pub fn client_ip(peer: SocketAddr, headers: &HeaderMap, trust_proxy: bool) -> IpAddr {
    if trust_proxy {
        if let Some(value) = headers
            .get("x-forwarded-for")
            .and_then(|value| value.to_str().ok())
        {
            if let Some(first) = value.split(',').next() {
                if let Ok(ip) = first.trim().parse::<IpAddr>() {
                    return normalize_ip(ip);
                }
            }
        }
    }
    normalize_ip(peer.ip())
}

fn normalize_ip(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6
            .to_ipv4_mapped()
            .map(IpAddr::V4)
            .unwrap_or(IpAddr::V6(v6)),
        other => other,
    }
}

pub fn extract_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    let raw = headers.get(header::COOKIE)?.to_str().ok()?;
    for part in raw.split(';') {
        let mut kv = part.trim().splitn(2, '=');
        if kv.next() == Some(name) {
            return kv.next().map(|value| value.trim().to_string());
        }
    }
    None
}

pub fn is_authenticated(state: &AppState, headers: &HeaderMap) -> bool {
    if state.cfg.password.is_none() {
        return true;
    }
    let Some(token) = extract_cookie(headers, COOKIE_NAME) else {
        return false;
    };
    let Ok(mut sessions) = state.sessions.lock() else {
        return false;
    };
    let now = SystemTime::now();
    sessions.retain(|_, expiry| *expiry > now);
    if let Some(expiry) = sessions.get_mut(&token) {
        // Sliding expiration: active sessions stay alive for another 7 days.
        *expiry = now + SESSION_TTL;
        true
    } else {
        false
    }
}

pub fn create_session(state: &AppState) -> String {
    let token = uuid::Uuid::new_v4().to_string();
    if let Ok(mut sessions) = state.sessions.lock() {
        sessions.insert(token.clone(), SystemTime::now() + SESSION_TTL);
    }
    token
}

pub fn drop_session(state: &AppState, token: &str) {
    if let Ok(mut sessions) = state.sessions.lock() {
        sessions.remove(token);
    }
}

pub fn session_cookie(token: &str, secure: bool) -> HeaderValue {
    let secure_flag = if secure { "; Secure" } else { "" };
    let value = format!(
        "{COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}{secure_flag}",
        SESSION_TTL.as_secs()
    );
    HeaderValue::from_str(&value).expect("valid session cookie")
}

pub fn clear_cookie(secure: bool) -> HeaderValue {
    let secure_flag = if secure { "; Secure" } else { "" };
    let value = format!(
        "{COOKIE_NAME}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{secure_flag}"
    );
    HeaderValue::from_str(&value).expect("valid clear cookie")
}

/// Compare the SHA-256 digests instead of the raw passwords so the comparison
/// does not reveal the password length through an early return.
pub fn constant_time_eq(left: &str, right: &str) -> bool {
    let left = Sha256::digest(left.as_bytes());
    let right = Sha256::digest(right.as_bytes());
    let mut diff = 0u8;
    for (a, b) in left.iter().zip(right.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}

pub async fn require_auth(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, AppError> {
    if is_authenticated(&state, request.headers()) {
        Ok(next.run(request).await)
    } else {
        Err(AppError::unauthorized("登录状态已失效，请重新登录"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer() -> SocketAddr {
        "127.0.0.1:50000".parse().unwrap()
    }

    #[test]
    fn compares_passwords_without_length_leak() {
        assert!(constant_time_eq("correct horse", "correct horse"));
        assert!(!constant_time_eq("short", "a much longer password"));
        assert!(!constant_time_eq("", "non-empty"));
    }

    #[test]
    fn locks_after_five_failures() {
        let mut limiter = LoginLimiter::default();
        let ip: IpAddr = "203.0.113.5".parse().unwrap();
        for _ in 0..4 {
            assert!(limiter.record_failure(ip).is_none());
        }
        assert!(limiter.record_failure(ip).is_some());
        assert!(matches!(limiter.check(ip), LoginCheck::Locked(_)));

        limiter.record_success(ip);
        assert!(matches!(limiter.check(ip), LoginCheck::Allowed));
    }

    #[test]
    fn forwarded_for_is_opt_in() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", "198.51.100.9, 127.0.0.1".parse().unwrap());

        assert_eq!(client_ip(peer(), &headers, false), peer().ip());
        assert_eq!(
            client_ip(peer(), &headers, true),
            "198.51.100.9".parse::<IpAddr>().unwrap()
        );
    }
}
