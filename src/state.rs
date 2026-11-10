//! 全应用共享的运行时状态。

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::SystemTime,
};

use sqlx::SqlitePool;

use crate::{auth::LoginLimiter, config::Config};

/// 所有请求处理器共享的状态。
///
/// 克隆成本很低：连接池和 `Arc` 都只是引用计数，锁也是共享的。
#[derive(Clone)]
pub struct AppState {
    /// SQLite 连接池。
    pub pool: SqlitePool,
    /// 启动时读取的环境配置。
    pub cfg: Arc<Config>,
    /// 会话令牌 -> 过期时间。会话故意只放在内存里：
    /// 重启进程只会让用户重新登录一次，不会留下持久化令牌。
    pub sessions: Arc<Mutex<HashMap<String, SystemTime>>>,
    /// 登录失败限流器，按来源 IP 计数。
    pub login_limiter: Arc<Mutex<LoginLimiter>>,
}
