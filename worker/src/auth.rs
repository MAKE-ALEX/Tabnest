use std::collections::HashMap;

use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, HeaderValue},
    middleware::Next,
    response::Response,
};
use tabnest_core::session;

use crate::{error::AppError, state::AppState};

const MAX_FAILURES: u32 = 5;
const FAILURE_WINDOW_MS: u64 = 10 * 60 * 1000;
const LOCKOUT_STEP_MS: u64 = 30 * 1000;
const LOCKOUT_MAX_MS: u64 = 15 * 60 * 1000;
const MAX_TOKEN_LEN: usize = 512;

pub fn now_ms() -> u64 {
    worker::Date::now().as_millis()
}

fn lockout_duration(failures: u32) -> u64 {
    if failures < MAX_FAILURES {
        return 0;
    }
    let steps = (failures - MAX_FAILURES).min(5);
    let factor = 1u64 << steps;
    (LOCKOUT_STEP_MS * factor).min(LOCKOUT_MAX_MS)
}

pub enum LoginCheck {
    Allowed,
    Locked(u64),
}

#[derive(Default)]
pub struct LoginLimiter {
    attempts: HashMap<String, Attempt>,
}

struct Attempt {
    failures: u32,
    window_start: u64,
    locked_until: Option<u64>,
    last_seen: u64,
}

impl LoginLimiter {
    pub fn check(&mut self, ip: &str, now: u64) -> LoginCheck {
        self.prune(now);
        let Some(attempt) = self.attempts.get_mut(ip) else {
            return LoginCheck::Allowed;
        };
        if let Some(until) = attempt.locked_until {
            if until > now {
                return LoginCheck::Locked(until - now);
            }
            // Keep the failure count so repeated abuse escalates instead of
            // resetting to the initial 30 seconds.
            attempt.locked_until = None;
        }
        if now.saturating_sub(attempt.window_start) > FAILURE_WINDOW_MS {
            attempt.failures = 0;
            attempt.window_start = now;
        }
        LoginCheck::Allowed
    }

    pub fn record_failure(&mut self, ip: &str, now: u64) -> Option<u64> {
        let attempt = self.attempts.entry(ip.to_string()).or_insert_with(|| Attempt {
            failures: 0,
            window_start: now,
            locked_until: None,
            last_seen: now,
        });
        if now.saturating_sub(attempt.window_start) > FAILURE_WINDOW_MS {
            attempt.failures = 0;
            attempt.window_start = now;
        }
        attempt.failures = attempt.failures.saturating_add(1);
        attempt.last_seen = now;
        if attempt.failures >= MAX_FAILURES {
            let duration = lockout_duration(attempt.failures);
            attempt.locked_until = Some(now + duration);
            Some(duration)
        } else {
            None
        }
    }

    pub fn record_success(&mut self, ip: &str) {
        self.attempts.remove(ip);
    }

    fn prune(&mut self, now: u64) {
        self.attempts.retain(|_, attempt| {
            if let Some(until) = attempt.locked_until {
                until > now
            } else {
                now.saturating_sub(attempt.last_seen) < FAILURE_WINDOW_MS
            }
        });
    }
}

/// Cloudflare terminates TLS and overwrites `CF-Connecting-IP`, so it is the
/// only header worth trusting here. `X-Forwarded-For` is the fallback for
/// `wrangler dev` and other proxies in front of the Worker.
pub fn client_ip(headers: &HeaderMap) -> String {
    if let Some(value) = headers
        .get("cf-connecting-ip")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return value.to_string();
    }
    if let Some(value) = headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
    {
        if let Some(first) = value.split(',').next().map(str::trim) {
            if !first.is_empty() {
                return first.to_string();
            }
        }
    }
    "unknown".to_string()
}

pub fn is_authenticated(state: &AppState, headers: &HeaderMap) -> bool {
    if state.cfg.password.is_none() {
        return true;
    }
    let Some(raw) = headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    let Some(token) = session::extract_cookie(raw, session::cookie_name(state.cfg.cookie_secure))
    else {
        return false;
    };
    if token.len() > MAX_TOKEN_LEN {
        return false;
    }
    session::verify_token(
        &state.cfg.session_secret,
        token,
        now_ms().min(i64::MAX as u64) as i64,
    )
}

/// Issue a signed cookie value for a fresh session.
pub fn create_session(state: &AppState) -> String {
    let now = now_ms();
    let nonce = format!(
        "{:x}-{:x}",
        now,
        (worker::js_sys::Math::random() * 1e18) as u64
    );
    session::issue_token(&state.cfg.session_secret, now as i64, &nonce)
}

pub fn session_cookie(state: &AppState, token: &str) -> HeaderValue {
    let cookie = session::session_cookie(token, state.cfg.cookie_secure);
    HeaderValue::from_str(&cookie).expect("valid session cookie")
}

pub fn clear_cookie(state: &AppState) -> HeaderValue {
    let cookie = session::clear_cookie(state.cfg.cookie_secure);
    HeaderValue::from_str(&cookie).expect("valid clear cookie")
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
