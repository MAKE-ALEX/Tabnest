use std::sync::{Arc, Mutex, OnceLock};

use worker::d1::D1Database;
use worker::send::SendWrapper;

use crate::{auth::LoginLimiter, config::Config};

/// One limiter per isolate: Workers have no shared process memory, so login
/// throttling is per-instance and best effort. See README for the tradeoff.
static LIMITER: OnceLock<Arc<Mutex<LoginLimiter>>> = OnceLock::new();

fn shared_limiter() -> Arc<Mutex<LoginLimiter>> {
    LIMITER
        .get_or_init(|| Arc::new(Mutex::new(LoginLimiter::default())))
        .clone()
}

#[derive(Clone)]
pub struct AppState {
    /// `D1Database` is a JS handle and therefore neither `Clone` nor `Send`. The
    /// Worker is single threaded, so `SendWrapper<Arc<_>>` is the supported
    /// combination: cheap to clone per request and `Send + Sync` for axum.
    pub db: SendWrapper<Arc<D1Database>>,
    pub cfg: Arc<Config>,
    pub limiter: Arc<Mutex<LoginLimiter>>,
}

impl AppState {
    pub fn new(env: &worker::Env) -> worker::Result<Self> {
        Ok(Self {
            db: SendWrapper::new(Arc::new(env.d1("DB")?)),
            cfg: Arc::new(Config::from_env(env)),
            limiter: shared_limiter(),
        })
    }
}
