//! Worker 运行时状态。

use std::sync::{Arc, Mutex, OnceLock};

use worker::d1::D1Database;
use worker::send::SendWrapper;

use crate::{auth::LoginLimiter, config::Config};

/// 每个 isolate 一个限流器：Workers 没有共享的进程内存，登录限流只能是
/// 单实例内的尽力而为（详见 README 的说明）。
static LIMITER: OnceLock<Arc<Mutex<LoginLimiter>>> = OnceLock::new();

fn shared_limiter() -> Arc<Mutex<LoginLimiter>> {
    LIMITER
        .get_or_init(|| Arc::new(Mutex::new(LoginLimiter::default())))
        .clone()
}

/// 每个请求共享的状态。
#[derive(Clone)]
pub struct AppState {
    /// `D1Database` 是 JS 句柄，本身既不 `Clone` 也不 `Send`。Worker 是单线程的，
    /// 所以用 `SendWrapper<Arc<_>>`：每个请求克隆成本很低，同时满足 axum 对
    /// `Send + Sync` 的要求。
    pub db: SendWrapper<Arc<D1Database>>,
    pub cfg: Arc<Config>,
    pub limiter: Arc<Mutex<LoginLimiter>>,
}

impl AppState {
    /// 从 Worker 环境绑定中取出 D1 数据库并读取配置。
    pub fn new(env: &worker::Env) -> worker::Result<Self> {
        Ok(Self {
            db: SendWrapper::new(Arc::new(env.d1("DB")?)),
            cfg: Arc::new(Config::from_env(env)),
            limiter: shared_limiter(),
        })
    }
}
