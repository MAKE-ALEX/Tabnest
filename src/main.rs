#![forbid(unsafe_code)]

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
    tracing_subscriber::registry()
        .with(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("tabnest=info,tower_http=warn,sqlx=warn")),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let cfg = Arc::new(Config::from_env()?);
    cfg.validate()?;
    let pool = db::connect(&cfg).await?;
    db::seed_if_empty(&pool, cfg.seed_demo).await?;

    let state = AppState {
        pool,
        cfg: cfg.clone(),
        sessions: Arc::new(Mutex::new(HashMap::new())),
        login_limiter: Arc::new(Mutex::new(LoginLimiter::default())),
    };

    let app = api::router(state)
        .merge(assets::router())
        .layer(CompressionLayer::new())
        .layer(middleware::from_fn(security::security_headers))
        .fallback(assets::not_found);

    let listener = tokio::net::TcpListener::bind(&cfg.bind)
        .await
        .with_context(|| format!("无法监听 {}", cfg.bind))?;

    tracing::info!(
        "Tabnest v{} 已启动：http://{}（数据目录：{}）",
        cfg.version,
        cfg.bind,
        cfg.data_dir.display()
    );
    if cfg.password.is_none() {
        tracing::warn!("未设置 APP_PASSWORD，任何能访问该端口的人都可以查看和修改书签");
    }
    if cfg.allow_private_network {
        tracing::warn!("ALLOW_PRIVATE_NETWORK 已启用：服务端可请求内网地址");
    }

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    Ok(())
}

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

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("正在关闭 Tabnest…");
}
