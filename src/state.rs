use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::SystemTime,
};

use sqlx::SqlitePool;

use crate::{auth::LoginLimiter, config::Config};

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub cfg: Arc<Config>,
    /// Session token -> expiry. Sessions are intentionally in memory: restarting
    /// the process only asks the user to log in again.
    pub sessions: Arc<Mutex<HashMap<String, SystemTime>>>,
    pub login_limiter: Arc<Mutex<LoginLimiter>>,
}
