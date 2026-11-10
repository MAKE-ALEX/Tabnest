//! 从 Worker 环境变量读取配置。

use sha2::{Digest, Sha256};
use worker::Env;

/// Worker 运行配置，对应 wrangler 里配置的变量 / secret。
pub struct Config {
    /// 访问密码；`None` 表示免密。
    pub password: Option<String>,
    /// 无状态会话 Cookie 的 HMAC 密钥。
    pub session_secret: Vec<u8>,
    /// 允许抓取元信息 / favicon 时访问私有、回环地址。
    pub allow_private_network: bool,
    /// 给会话 Cookie 加 `Secure` 属性。
    pub cookie_secure: bool,
    /// 首次初始化时是否写入示例面板。
    pub seed_demo: bool,
    /// 编译时写入的版本号。
    pub version: &'static str,
}

/// 读取字符串变量：去掉首尾空白，空串视为未设置。
fn var(env: &Env, name: &str) -> Option<String> {
    env.var(name)
        .ok()
        .map(|value| value.to_string())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// 解析布尔变量：`1/true/yes/on` 为真，未设置为假。
fn flag(env: &Env, name: &str) -> bool {
    var(env, name)
        .map(|value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

impl Config {
    pub fn from_env(env: &Env) -> Self {
        let password = var(env, "APP_PASSWORD");

        // 没显式配置 SESSION_SECRET 时，从密码派生一个，保证新部署开箱可用。
        // 代价是改密码会让所有会话失效——这本来也是期望的行为。
        let session_secret = var(env, "SESSION_SECRET")
            .map(|value| value.into_bytes())
            .unwrap_or_else(|| {
                let seed = password
                    .clone()
                    .unwrap_or_else(|| "tabnest-insecure-default".to_string());
                let mut hasher = Sha256::new();
                hasher.update(seed.as_bytes());
                // 加入固定盐，避免与其他系统的 SHA-256(password) 撞值。
                hasher.update(b"tabnest-worker-session-v1");
                hasher.finalize().to_vec()
            });

        Self {
            password,
            session_secret,
            allow_private_network: flag(env, "ALLOW_PRIVATE_NETWORK"),
            // Workers 永远跑在 HTTPS 后面，所以 Cookie 默认加 Secure；
            // 只有显式写 0/false/no/off 才关闭。
            cookie_secure: var(env, "COOKIE_SECURE")
                .map(|value| {
                    !matches!(
                        value.to_ascii_lowercase().as_str(),
                        "0" | "false" | "no" | "off"
                    )
                })
                .unwrap_or(true),
            // SEED_DEMO 默认开启，同样只有显式关闭才不写示例数据。
            seed_demo: var(env, "SEED_DEMO")
                .map(|value| {
                    !matches!(
                        value.to_ascii_lowercase().as_str(),
                        "0" | "false" | "no" | "off"
                    )
                })
                .unwrap_or(true),
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}
