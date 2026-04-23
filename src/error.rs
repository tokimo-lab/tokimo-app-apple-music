//! 统一错误响应类型。

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

#[derive(Debug)]
#[allow(dead_code)]
pub enum AppError {
    BadRequest(String),
    NotFound(String),
    Unauthorized(String),
    Internal(String),
    /// `OpenAPI` 调用返回的非 2xx，透传 status 与原始 body 文本（多半是 JSON）。
    Upstream { status: StatusCode, body: String },
}

impl AppError {
    #[allow(dead_code)]
    pub fn internal(msg: impl Into<String>) -> Self {
        Self::Internal(msg.into())
    }
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self::BadRequest(msg.into())
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadRequest(m) | Self::NotFound(m) | Self::Unauthorized(m) | Self::Internal(m) => {
                write!(f, "{m}")
            }
            Self::Upstream { status, body } => write!(f, "upstream {status}: {body}"),
        }
    }
}

impl std::error::Error for AppError {}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            Self::BadRequest(m) => (StatusCode::BAD_REQUEST, m.clone()),
            Self::NotFound(m) => (StatusCode::NOT_FOUND, m.clone()),
            Self::Unauthorized(m) => (StatusCode::UNAUTHORIZED, m.clone()),
            Self::Internal(m) => (StatusCode::INTERNAL_SERVER_ERROR, m.clone()),
            Self::Upstream { status, body } => (*status, body.clone()),
        };
        (status, Json(json!({ "error": message }))).into_response()
    }
}

impl From<reqwest::Error> for AppError {
    fn from(e: reqwest::Error) -> Self {
        Self::Internal(format!("reqwest: {e}"))
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        Self::Internal(format!("json: {e}"))
    }
}

/// 统一的成功响应包装（与主 server `crate::handlers::ApiResponse<T>` 形态一致：
/// `{ success: bool, data?: T, error?: string }`，前端代码无需改动）。
#[derive(serde::Serialize)]
pub struct ApiResponse<T: serde::Serialize> {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub fn ok<T: serde::Serialize>(data: T) -> Json<ApiResponse<T>> {
    Json(ApiResponse {
        success: true,
        data: Some(data),
        error: None,
    })
}
