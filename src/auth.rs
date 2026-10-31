use std::time::{Duration, SystemTime};

use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, HeaderValue},
    middleware::Next,
    response::Response,
};

use crate::{
    error::AppError,
    state::AppState,
};

pub const COOKIE_NAME: &str = "tabnest_session";
const SESSION_TTL: Duration = Duration::from_secs(30 * 24 * 60 * 60);

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
    sessions.contains_key(&token)
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

pub fn session_cookie(token: &str) -> HeaderValue {
    let value = format!(
        "{COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}",
        SESSION_TTL.as_secs()
    );
    HeaderValue::from_str(&value).expect("valid session cookie")
}

pub fn clear_cookie() -> HeaderValue {
    HeaderValue::from_static("tabnest_session=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0")
}

/// Length-independent comparison for the single shared password.
pub fn constant_time_eq(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    if left.len() != right.len() {
        return false;
    }
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
