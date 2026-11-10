//! 从环境变量读取并校验运行配置。

use std::{env, net::SocketAddr, path::PathBuf};

use anyhow::Context;

/// 服务端运行配置，全部来自环境变量。
pub struct Config {
    /// 监听地址，如 `0.0.0.0:8420`。
    pub bind: String,
    /// 数据目录，存放数据库与图标缓存。
    pub data_dir: PathBuf,
    /// SQLite 文件路径。
    pub db_path: PathBuf,
    /// 访问密码；`None` 表示免密（仅允许回环监听或显式开启不安全模式）。
    pub password: Option<String>,
    /// 首次启动且数据库为空时是否写入示例面板。
    pub seed_demo: bool,
    /// 允许抓取元信息 / favicon 时访问私有、回环、链路本地地址。
    /// 默认关闭，避免服务端被当成访问内网的 SSRF 代理。
    pub allow_private_network: bool,
    /// 给会话 Cookie 加 `Secure` 属性（部署在 HTTPS 后面时应开启）。
    pub cookie_secure: bool,
    /// 信任 `X-Forwarded-For` 等代理头（用于客户端 IP 和 HSTS）。
    pub trust_proxy: bool,
    /// 允许信任其 `CF-Connecting-IP` / `X-Forwarded-For` 的代理网段。
    pub trusted_proxies: Vec<ipnet::IpNet>,
    /// 显式允许在非回环地址上无密码运行。
    pub allow_insecure: bool,
    /// 编译时写入的版本号。
    pub version: &'static str,
}

/// 解析布尔型环境变量：`1/true/yes/on` 为真，其余为假，未设置也为假。
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
    /// 从环境变量构造配置，并创建必要的数据目录。
    pub fn from_env() -> anyhow::Result<Self> {
        let data_dir = PathBuf::from(env::var("DATA_DIR").unwrap_or_else(|_| "./data".into()));
        std::fs::create_dir_all(&data_dir)
            .with_context(|| format!("无法创建数据目录 {}", data_dir.display()))?;
        std::fs::create_dir_all(data_dir.join("favicons"))
            .context("无法创建图标缓存目录")?;

        let bind = env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".into());
        // 空密码视为未设置，避免 `APP_PASSWORD=` 造成“有密码”的假象。
        let password = env::var("APP_PASSWORD")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        // SEED_DEMO 默认开启；只有显式写 0/false/no/off 才关闭。
        let seed_demo = env::var("SEED_DEMO")
            .map(|value| {
                !matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "0" | "false" | "no" | "off"
                )
            })
            .unwrap_or(true);

        // 未指定 DB_PATH 时，数据库放在数据目录下。
        let db_path = env::var("DB_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| data_dir.join("tabnest.db"));

        // TRUSTED_PROXIES 是逗号分隔的 CIDR 列表，逐个解析并报错定位。
        let mut trusted_proxies = Vec::new();
        if let Ok(value) = env::var("TRUSTED_PROXIES") {
            for part in value.split(',') {
                let part = part.trim();
                if part.is_empty() {
                    continue;
                }
                trusted_proxies.push(
                    part.parse()
                        .with_context(|| format!("TRUSTED_PROXIES 中的 CIDR 无效：{part}"))?,
                );
            }
        }

        Ok(Self {
            bind,
            data_dir,
            db_path,
            password,
            seed_demo,
            allow_private_network: env_flag("ALLOW_PRIVATE_NETWORK"),
            cookie_secure: env_flag("COOKIE_SECURE"),
            trust_proxy: env_flag("TRUST_PROXY"),
            trusted_proxies,
            allow_insecure: env_flag("ALLOW_INSECURE"),
            version: env!("CARGO_PKG_VERSION"),
        })
    }

    /// 启动前的安全检查：不允许把无密码实例暴露在公网接口上，
    /// 除非运维显式开启 `ALLOW_INSECURE`。
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
