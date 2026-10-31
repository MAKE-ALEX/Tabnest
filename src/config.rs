use std::{env, path::PathBuf};

use anyhow::Context;

pub struct Config {
    pub bind: String,
    pub data_dir: PathBuf,
    pub db_path: PathBuf,
    pub password: Option<String>,
    pub seed_demo: bool,
    pub version: &'static str,
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
            version: env!("CARGO_PKG_VERSION"),
        })
    }
}
