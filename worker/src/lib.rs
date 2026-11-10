//! Cloudflare Workers 入口。
//!
//! Worker 负责 `/api/*` 和 `/healthz`；其余路径由 Cloudflare Static Assets
//! 直接返回 `static/` 下的文件（见 `wrangler.toml`）。

mod api;
mod auth;
mod config;
mod db;
mod error;
mod favicon;
mod fetch;
mod security;
mod state;

use std::sync::atomic::{AtomicU8, Ordering};

use worker::*;

/// 初始化状态：0 = 未开始，1 = 进行中，2 = 已完成。
///
/// 不能用普通的 `OnceLock`：冷启动时两个并发请求可能同时进入初始化，
/// 导致建表 / 示例数据被执行两次。这里用一次 CAS 抢占执行权。
static BOOTSTRAP: AtomicU8 = AtomicU8::new(0);

#[event(fetch)]
async fn fetch(
    req: worker::HttpRequest,
    env: Env,
    _ctx: Context,
) -> Result<axum::response::Response> {
    let state = state::AppState::new(&env)?;

    // 每个 isolate 只执行一次 `CREATE TABLE IF NOT EXISTS` 和可选的示例数据，
    // 之后的热请求不再付出这部分开销。失败时把状态复位，下次请求重试。
    if BOOTSTRAP.load(Ordering::Relaxed) == 0
        && BOOTSTRAP
            .compare_exchange(0, 1, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
    {
        match db::bootstrap(&state.db, state.cfg.seed_demo).await {
            Ok(()) => BOOTSTRAP.store(2, Ordering::Relaxed),
            Err(error) => {
                console_error!("database bootstrap failed: {}", error.message);
                BOOTSTRAP.store(0, Ordering::Relaxed);
            }
        }
    }

    let app = api::router(state);
    // 把 Workers 的请求交给 axum 处理；错误类型不同，这里做一次转换。
    tower::ServiceExt::oneshot(app, req)
        .await
        .map_err(|never| Error::RustError(never.to_string()))
}
