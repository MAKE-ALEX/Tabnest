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

use std::sync::OnceLock;

use worker::*;

static BOOTSTRAPPED: OnceLock<()> = OnceLock::new();

#[event(fetch)]
async fn fetch(
    req: worker::HttpRequest,
    env: Env,
    _ctx: Context,
) -> Result<axum::response::Response> {
    let state = state::AppState::new(&env)?;

    // `CREATE TABLE IF NOT EXISTS` plus the optional demo seed. Kept once per
    // isolate so steady-state requests pay nothing.
    if BOOTSTRAPPED.get().is_none() {
        match db::bootstrap(&state.db, state.cfg.seed_demo).await {
            Ok(()) => {
                let _ = BOOTSTRAPPED.set(());
            }
            Err(error) => {
                console_error!("database bootstrap failed: {}", error.message);
            }
        }
    }

    let app = api::router(state);
    tower::ServiceExt::oneshot(app, req)
        .await
        .map_err(|never| Error::RustError(never.to_string()))
}
