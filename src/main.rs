//! Tabnest 原生服务端入口：初始化日志、配置、数据库，装配路由后监听端口。

mod api;
mod assets;
mod auth;
mod config;
mod db;
mod error;
mod fetch;
mod import;
mod security;
mod state;

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use anyhow::Context;
use axum::middleware;
use tower_http::compression::CompressionLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::{auth::LoginLimiter, config::Config, state::AppState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 日志级别可用 RUST_LOG 覆盖；默认只保留本应用 info，依赖库尽量安静。
    tracing_subscriber::registry()
        .with(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("tabnest=info,tower_http=warn,sqlx=warn")),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // 读取环境变量配置，并做启动前的安全检查（例如禁止无密码对公网监听）。
    let cfg = Arc::new(Config::from_env()?);
    cfg.validate()?;
    // 打开 SQLite、执行迁移，首次启动时写入示例数据。
    let pool = db::connect(&cfg).await?;
    db::seed_if_empty(&pool, cfg.seed_demo).await?;

    let state = AppState {
        pool,
        cfg: cfg.clone(),
        sessions: Arc::new(Mutex::new(HashMap::new())),
        login_limiter: Arc::new(Mutex::new(LoginLimiter::default())),
    };

    // 业务 API + 静态资源；未匹配的路径交给统一的 404 处理。
    // 最后叠加 gzip 压缩和全局安全响应头。
    let app = api::router(state)
        .merge(assets::router())
        .fallback(assets::not_found)
        .layer(CompressionLayer::new())
        .layer(middleware::from_fn(security::security_headers));

    let listener = tokio::net::TcpListener::bind(&cfg.bind)
        .await
        .with_context(|| format!("无法监听 {}", cfg.bind))?;

    tracing::info!(
        "Tabnest v{} 已启动：http://{}（数据目录：{}）",
        cfg.version,
        cfg.bind,
        cfg.data_dir.display()
    );
    // 对几个容易踩坑的配置给出显式警告。
    if cfg.password.is_none() {
        tracing::warn!("未设置 APP_PASSWORD，任何能访问该端口的人都可以查看和修改书签");
    }
    if cfg.allow_private_network {
        tracing::warn!("ALLOW_PRIVATE_NETWORK 已启用：服务端可请求内网地址");
    }
    if cfg.trust_proxy {
        tracing::warn!(
            "TRUST_PROXY 已启用：会信任任意对端的转发头，建议改用 TRUSTED_PROXIES 限定代理网段"
        );
    }

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    // 收到 Ctrl+C 或 SIGTERM 时优雅退出。
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    Ok(())
}

/// 等待退出信号：Unix 下同时处理 SIGINT 和 SIGTERM，其他平台只处理 Ctrl+C。
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            signal.recv().await;
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    // 任意一个信号先到就开始关闭。
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("正在关闭 Tabnest…");
}
