use std::{env, net::SocketAddr, path::PathBuf};

use anyhow::Context;

pub struct Config {
    pub bind: String,
    pub data_dir: PathBuf,
    pub db_path: PathBuf,
    pub password: Option<String>,
    pub seed_demo: bool,
    /// Allow metadata / favicon fetching from private, loopback and link-local
    /// addresses. Disabled by default to prevent the server from being used as
    /// an SSRF proxy into the LAN.
    pub allow_private_network: bool,
    /// Add the `Secure` attribute to the session cookie (enable behind HTTPS).
    pub cookie_secure: bool,
    /// Trust `X-Forwarded-For` / proxy headers for client IP and HSTS.
    pub trust_proxy: bool,
    /// Explicitly allow running without a password on a non-loopback bind.
    pub allow_insecure: bool,
    pub version: &'static str,
}

fn env_flag(name: &str) -> bool {
    env::var(name)
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let data_dir = PathBuf::from(env::var("DATA_DIR").unwrap_or_else(|_| "./data".into()));
        std::fs::create_dir_all(&data_dir)
            .with_context(|| format!("无法创建数据目录 {}", data_dir.display()))?;
        std::fs::create_dir_all(data_dir.join("favicons"))
            .context("无法创建图标缓存目录")?;

        let bind = env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".into());
        let password = env::var("APP_PASSWORD")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let seed_demo = env::var("SEED_DEMO")
            .map(|value| {
                !matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "0" | "false" | "no" | "off"
                )
            })
            .unwrap_or(true);

        let db_path = env::var("DB_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| data_dir.join("tabnest.db"));

        Ok(Self {
            bind,
            data_dir,
            db_path,
            password,
            seed_demo,
            allow_private_network: env_flag("ALLOW_PRIVATE_NETWORK"),
            cookie_secure: env_flag("COOKIE_SECURE"),
            trust_proxy: env_flag("TRUST_PROXY"),
            allow_insecure: env_flag("ALLOW_INSECURE"),
            version: env!("CARGO_PKG_VERSION"),
        })
    }

    /// Refuse to expose an unauthenticated instance on a public interface unless
    /// the operator explicitly opts in.
    pub fn validate(&self) -> anyhow::Result<()> {
        let addr: SocketAddr = self
            .bind
            .parse()
            .with_context(|| format!("BIND_ADDR 不是合法的 IP:端口：{}", self.bind))?;
        if self.password.is_none() && !addr.ip().is_loopback() && !self.allow_insecure {
            anyhow::bail!(
                "未设置 APP_PASSWORD，且监听地址 {} 不是回环地址。\
                 请设置访问密码，或显式设置 ALLOW_INSECURE=true 允许无鉴权访问。",
                self.bind
            );
        }
        Ok(())
    }
}
