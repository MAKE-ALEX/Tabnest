//! 统一的 HTTP 错误类型：携带状态码和给用户看的提示语。

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

/// 业务错误。`message` 会原样返回给前端，因此不要放内部细节。
#[derive(Debug)]
pub struct AppError {
    pub status: StatusCode,
    pub message: String,
}

impl AppError {
    /// 400：请求参数不合法。
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    /// 404：资源不存在。
    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }

    /// 401：未登录或会话失效。
    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: message.into(),
        }
    }

    /// 403：已登录但没有权限 / 请求被安全策略拒绝。
    pub fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            message: message.into(),
        }
    }

    /// 500：服务器内部错误。
    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }
}

impl IntoResponse for AppError {
    /// 以 `{ "error": "..." }` 的 JSON 形式返回错误。
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}

impl From<sqlx::Error> for AppError {
    /// 数据库错误只记录日志，对外统一提示「数据库操作失败」。
    fn from(error: sqlx::Error) -> Self {
        tracing::error!(%error, "database error");
        Self::internal("数据库操作失败")
    }
}

impl From<anyhow::Error> for AppError {
    /// 其他内部错误同理：细节进日志，响应里只给泛化提示。
    fn from(error: anyhow::Error) -> Self {
        tracing::error!(%error, "internal error");
        Self::internal("服务器内部错误")
    }
}

impl From<serde_json::Error> for AppError {
    fn from(error: serde_json::Error) -> Self {
        tracing::error!(%error, "json error");
        Self::internal("数据序列化失败")
    }
}

/// 处理器常用的结果类型别名。
pub type AppResult<T> = Result<T, AppError>;
