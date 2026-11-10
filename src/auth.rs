//! 登录鉴权、会话管理与登录失败限流。

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
const HOST_COOKIE_NAME: &str = "__Host-tabnest_session";
/// 会话有效期：7 天，命中请求时滑动续期。
const SESSION_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// 连续失败多少次后开始锁定。
const MAX_FAILURES: u32 = 5;
/// 失败计数的统计窗口：超过这个时间没有新失败就清零。
const FAILURE_WINDOW: Duration = Duration::from_secs(10 * 60);
/// 首次锁定的时长，之后按次数翻倍。
const LOCKOUT_STEP: Duration = Duration::from_secs(30);
/// 单次锁定上限。
const LOCKOUT_MAX: Duration = Duration::from_secs(15 * 60);

/// 选择会话 Cookie 的名称。
///
/// `__Host-` 前缀要求 Cookie 必须带 `Secure`、Path 为 `/` 且不指定 Domain；
/// 只有 HTTPS 部署才能使用。
pub fn cookie_name(secure: bool) -> &'static str {
    if secure {
        HOST_COOKIE_NAME
    } else {
        COOKIE_NAME
    }
}

/// 根据累计失败次数计算锁定时间：5 次失败锁 30 秒，之后每次翻倍，最长 15 分钟。
fn lockout_duration(failures: u32) -> Duration {
    if failures < MAX_FAILURES {
        return Duration::ZERO;
    }
    let steps = (failures - MAX_FAILURES).min(5) as u32;
    let factor = 1u64 << steps;
    let seconds = (LOCKOUT_STEP.as_secs() * factor).min(LOCKOUT_MAX.as_secs());
    Duration::from_secs(seconds)
}

/// 按来源 IP 记录登录失败的限流器。
#[derive(Default)]
pub struct LoginLimiter {
    attempts: HashMap<IpAddr, Attempt>,
}

/// 单个 IP 的失败状态。
struct Attempt {
    failures: u32,
    /// 本轮计数的开始时间，用于滚动窗口。
    window_start: Instant,
    /// 锁定截止时间；未锁定时为 `None`。
    locked_until: Option<Instant>,
    /// 最后一次失败时间，`prune` 用它清理过期记录。
    last_seen: Instant,
}

/// 登录前的检查结果。
pub enum LoginCheck {
    Allowed,
    Locked(Duration),
}

impl LoginLimiter {
    /// 判断该 IP 当前是否允许尝试登录；顺手清理过期记录。
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
            // 锁定已过期，但保留失败次数：继续尝试会以更长时间重新锁定，
            // 而不是从最初的 30 秒重新开始。
            attempt.locked_until = None;
        }
        if now.duration_since(attempt.window_start) > FAILURE_WINDOW {
            attempt.failures = 0;
            attempt.window_start = now;
        }
        LoginCheck::Allowed
    }

    /// 记录一次失败；如果这次失败触发了锁定，返回锁定时长。
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
            let duration = lockout_duration(attempt.failures);
            attempt.locked_until = Some(now + duration);
            Some(duration)
        } else {
            None
        }
    }

    /// 登录成功后清除该 IP 的失败记录。
    pub fn record_success(&mut self, ip: IpAddr) {
        self.attempts.remove(&ip);
    }

    /// 清理「锁定已过期且长时间没有新失败」的记录，防止 HashMap 无限增长。
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

/// 为登录限流解析客户端 IP。
///
/// 只有 TCP 对端在 `TRUSTED_PROXIES` 内（或旧配置 `TRUST_PROXY=true`）时才信任
/// 代理头。`CF-Connecting-IP` 优先于 `X-Forwarded-For`；解析 XFF 时从右往左取
/// 第一个非代理地址，这样客户端自己伪造的最左值无法生效。
pub fn client_ip(
    peer: SocketAddr,
    headers: &HeaderMap,
    trusted_proxies: &[ipnet::IpNet],
    trust_proxy: bool,
) -> IpAddr {
    let peer_ip = normalize_ip(peer.ip());
    let trusted = trust_proxy || trusted_proxies.iter().any(|network| network.contains(&peer_ip));
    if !trusted {
        return peer_ip;
    }

    if let Some(ip) = header_ip(headers, "cf-connecting-ip") {
        return ip;
    }

    if let Some(value) = headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
    {
        // 从右往左：最右侧是离我们最近的跳，通常最可信。
        for part in value.split(',').rev() {
            if let Ok(ip) = part.trim().parse::<IpAddr>() {
                let ip = normalize_ip(ip);
                if !trusted_proxies
                    .iter()
                    .any(|network| network.contains(&ip))
                {
                    return ip;
                }
            }
        }
    }

    peer_ip
}

/// 从请求头的某个字段解析出 IP。
fn header_ip(headers: &HeaderMap, name: &str) -> Option<IpAddr> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<IpAddr>().ok())
        .map(normalize_ip)
}

/// 把 IPv4 映射的 IPv6 地址统一成 IPv4，便于网段比较。
fn normalize_ip(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6
            .to_ipv4_mapped()
            .map(IpAddr::V4)
            .unwrap_or(IpAddr::V6(v6)),
        other => other,
    }
}

/// 从 `Cookie` 请求头中取值。
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

/// 判断请求是否已登录。
///
/// 未配置密码时视为永远已登录；否则校验会话令牌，并顺带做滑动续期。
pub fn is_authenticated(state: &AppState, headers: &HeaderMap) -> bool {
    if state.cfg.password.is_none() {
        return true;
    }
    let Some(token) = extract_cookie(headers, cookie_name(state.cfg.cookie_secure)) else {
        return false;
    };
    let Ok(mut sessions) = state.sessions.lock() else {
        return false;
    };
    let now = SystemTime::now();
    // 先清掉所有过期会话。
    sessions.retain(|_, expiry| *expiry > now);
    if let Some(expiry) = sessions.get_mut(&token) {
        // 滑动过期：只要还在使用，就把有效期再延长 7 天。
        *expiry = now + SESSION_TTL;
        true
    } else {
        false
    }
}

/// 新建会话并返回随机令牌。
pub fn create_session(state: &AppState) -> String {
    let token = uuid::Uuid::new_v4().to_string();
    if let Ok(mut sessions) = state.sessions.lock() {
        sessions.insert(token.clone(), SystemTime::now() + SESSION_TTL);
    }
    token
}

/// 注销指定的会话令牌。
pub fn drop_session(state: &AppState, token: &str) {
    if let Ok(mut sessions) = state.sessions.lock() {
        sessions.remove(token);
    }
}

/// 生成写入会话 Cookie 的 `Set-Cookie` 值。
pub fn session_cookie(token: &str, secure: bool) -> HeaderValue {
    let secure_flag = if secure { "; Secure" } else { "" };
    let name = cookie_name(secure);
    let value = format!(
        "{name}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}{secure_flag}",
        SESSION_TTL.as_secs()
    );
    HeaderValue::from_str(&value).expect("valid session cookie")
}

/// 生成清除会话 Cookie 的 `Set-Cookie` 值。
pub fn clear_cookie(secure: bool) -> HeaderValue {
    let secure_flag = if secure { "; Secure" } else { "" };
    let name = cookie_name(secure);
    let value =
        format!("{name}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{secure_flag}");
    HeaderValue::from_str(&value).expect("valid clear cookie")
}

/// 常量时间比较两个密码。
///
/// 先各自做 SHA-256，再逐字节 XOR 累加差异。这样比较耗时与密码长度无关，
/// 也不会因为提前返回而泄漏长度信息。
pub fn constant_time_eq(left: &str, right: &str) -> bool {
    let left = Sha256::digest(left.as_bytes());
    let right = Sha256::digest(right.as_bytes());
    let mut diff = 0u8;
    for (a, b) in left.iter().zip(right.iter()) {
        diff |= a ^ b;
    }
    diff == 0
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

#[cfg(test)]
mod tests {
    use super::*;

    fn peer() -> SocketAddr {
        "127.0.0.1:50000".parse().unwrap()
    }

    fn trusted_networks() -> Vec<ipnet::IpNet> {
        vec!["172.23.0.0/16".parse().unwrap()]
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
        assert_eq!(
            limiter.record_failure(ip),
            Some(Duration::from_secs(30))
        );
        assert!(matches!(limiter.check(ip), LoginCheck::Locked(_)));

        limiter.record_success(ip);
        assert!(matches!(limiter.check(ip), LoginCheck::Allowed));
    }

    #[test]
    fn lockout_escalates() {
        assert_eq!(lockout_duration(4), Duration::ZERO);
        assert_eq!(lockout_duration(5), Duration::from_secs(30));
        assert_eq!(lockout_duration(6), Duration::from_secs(60));
        assert_eq!(lockout_duration(10), Duration::from_secs(900));
        assert_eq!(lockout_duration(50), Duration::from_secs(900));
    }

    #[test]
    fn ignores_forwarded_headers_from_untrusted_peer() {
        let mut headers = HeaderMap::new();
        headers.insert("cf-connecting-ip", "198.51.100.9".parse().unwrap());
        headers.insert("x-forwarded-for", "198.51.100.9".parse().unwrap());

        assert_eq!(
            client_ip(peer(), &headers, &trusted_networks(), false),
            peer().ip()
        );
    }

    #[test]
    fn uses_cf_header_from_trusted_proxy() {
        let proxy: SocketAddr = "172.23.0.1:50000".parse().unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("cf-connecting-ip", "203.0.113.9".parse().unwrap());
        headers.insert("x-forwarded-for", "1.2.3.4".parse().unwrap());

        assert_eq!(
            client_ip(proxy, &headers, &trusted_networks(), false),
            "203.0.113.9".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn uses_rightmost_untrusted_xff() {
        let proxy: SocketAddr = "172.23.0.1:50000".parse().unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            "1.2.3.4, 198.51.100.7, 172.23.0.1".parse().unwrap(),
        );

        assert_eq!(
            client_ip(proxy, &headers, &trusted_networks(), false),
            "198.51.100.7".parse::<IpAddr>().unwrap()
        );
    }
}
