//! 登录鉴权、无状态会话与登录失败限流。

use std::collections::HashMap;

use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, HeaderValue},
    middleware::Next,
    response::Response,
};
use tabnest_core::session;

use crate::{error::AppError, state::AppState};

/// 连续失败多少次后开始锁定。
const MAX_FAILURES: u32 = 5;
/// 失败计数的统计窗口。
const FAILURE_WINDOW_MS: u64 = 10 * 60 * 1000;
/// 首次锁定的时长，之后按次数翻倍。
const LOCKOUT_STEP_MS: u64 = 30 * 1000;
/// 单次锁定上限。
const LOCKOUT_MAX_MS: u64 = 15 * 60 * 1000;
/// 拒绝超长的会话令牌，避免把巨量输入喂给 HMAC 校验。
const MAX_TOKEN_LEN: usize = 512;

/// 当前时间（毫秒时间戳）。
pub fn now_ms() -> u64 {
    worker::Date::now().as_millis()
}

/// 根据累计失败次数计算锁定时间：5 次失败锁 30 秒，之后每次翻倍，最长 15 分钟。
fn lockout_duration(failures: u32) -> u64 {
    if failures < MAX_FAILURES {
        return 0;
    }
    let steps = (failures - MAX_FAILURES).min(5);
    let factor = 1u64 << steps;
    (LOCKOUT_STEP_MS * factor).min(LOCKOUT_MAX_MS)
}

/// 登录前的检查结果。
pub enum LoginCheck {
    Allowed,
    /// 仍需等待的毫秒数。
    Locked(u64),
}

/// 按来源 IP 记录登录失败的限流器。
#[derive(Default)]
pub struct LoginLimiter {
    attempts: HashMap<String, Attempt>,
}

/// 单个 IP 的失败状态。
struct Attempt {
    failures: u32,
    /// 本轮计数的开始时间，用于滚动窗口。
    window_start: u64,
    /// 锁定截止时间；未锁定时为 `None`。
    locked_until: Option<u64>,
    /// 最后一次失败时间，`prune` 用它清理过期记录。
    last_seen: u64,
}

impl LoginLimiter {
    /// 判断该 IP 当前是否允许尝试登录；顺手清理过期记录。
    pub fn check(&mut self, ip: &str, now: u64) -> LoginCheck {
        self.prune(now);
        let Some(attempt) = self.attempts.get_mut(ip) else {
            return LoginCheck::Allowed;
        };
        if let Some(until) = attempt.locked_until {
            if until > now {
                return LoginCheck::Locked(until - now);
            }
            // 锁定已过期，但保留失败次数：再次触发锁定会更久，
            // 而不是从最初的 30 秒重新开始。
            attempt.locked_until = None;
        }
        if now.saturating_sub(attempt.window_start) > FAILURE_WINDOW_MS {
            attempt.failures = 0;
            attempt.window_start = now;
        }
        LoginCheck::Allowed
    }

    /// 记录一次失败；如果这次失败触发了锁定，返回锁定时长（毫秒）。
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

    /// 登录成功后清除该 IP 的失败记录。
    pub fn record_success(&mut self, ip: &str) {
        self.attempts.remove(ip);
    }

    /// 清理「锁定已过期且长时间没有新失败」的记录，防止 HashMap 无限增长。
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

/// 解析客户端 IP。
///
/// Cloudflare 会终止 TLS 并覆写 `CF-Connecting-IP`，所以这里只信任它。
/// `X-Forwarded-For` 作为 `wrangler dev` 或其他前置代理场景的兜底。
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

/// 判断请求是否已登录。
///
/// 未配置密码时视为永远已登录；否则校验无状态会话令牌的签名与有效期。
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

/// 为新会话签发一个带签名的 Cookie 值。
pub fn create_session(state: &AppState) -> String {
    let now = now_ms();
    // 用时间戳 + JS 随机数拼 nonce，保证同一毫秒内也不会重复。
    let nonce = format!(
        "{:x}-{:x}",
        now,
        (worker::js_sys::Math::random() * 1e18) as u64
    );
    session::issue_token(&state.cfg.session_secret, now as i64, &nonce)
}

/// 生成写入会话 Cookie 的 `Set-Cookie` 值。
pub fn session_cookie(state: &AppState, token: &str) -> HeaderValue {
    let cookie = session::session_cookie(token, state.cfg.cookie_secure);
    HeaderValue::from_str(&cookie).expect("valid session cookie")
}

/// 生成清除会话 Cookie 的 `Set-Cookie` 值。
pub fn clear_cookie(state: &AppState) -> HeaderValue {
    let cookie = session::clear_cookie(state.cfg.cookie_secure);
    HeaderValue::from_str(&cookie).expect("valid clear cookie")
}

/// Axum 中间件：未登录时返回 401。
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
