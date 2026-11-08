//! Cloudflare Workers entry point.
//!
//! The Worker owns `/api/*` and `/healthz`; everything else is served from
//! `static/` by Cloudflare Static Assets (see `wrangler.toml`).

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

/// 0 = not started, 1 = running, 2 = done. A plain `OnceLock` would let two
/// concurrent requests on a cold isolate run the seed twice.
static BOOTSTRAP: AtomicU8 = AtomicU8::new(0);

#[event(fetch)]
async fn fetch(
    req: worker::HttpRequest,
    env: Env,
    _ctx: Context,
) -> Result<axum::response::Response> {
    let state = state::AppState::new(&env)?;

    // `CREATE TABLE IF NOT EXISTS` plus the optional demo seed. Kept once per
    // isolate so steady-state requests pay nothing.
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
    tower::ServiceExt::oneshot(app, req)
        .await
        .map_err(|never| Error::RustError(never.to_string()))
}
