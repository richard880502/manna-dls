//! API error type. Every error leaves the server as
//! `{ "error": { "code": "...", "message": "給人看的中文訊息" } }` (docs/API.md).

use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("unauthorized: {0}")]
    Unauthorized(String),
    #[error("forbidden: {0}")]
    Forbidden(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("internal: {0:#}")]
    Internal(#[from] anyhow::Error),
}

pub type AppResult<T> = Result<T, AppError>;

impl AppError {
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self::BadRequest(msg.into())
    }
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::NotFound(msg.into())
    }

    fn status_and_code(&self) -> (StatusCode, &'static str) {
        match self {
            Self::BadRequest(_) => (StatusCode::BAD_REQUEST, "bad_request"),
            Self::Unauthorized(_) => (StatusCode::UNAUTHORIZED, "unauthorized"),
            Self::Forbidden(_) => (StatusCode::FORBIDDEN, "forbidden"),
            Self::NotFound(_) => (StatusCode::NOT_FOUND, "not_found"),
            Self::Conflict(_) => (StatusCode::CONFLICT, "conflict"),
            Self::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
        }
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        Self::Internal(anyhow::Error::new(e))
    }
}

impl From<JsonRejection> for AppError {
    fn from(r: JsonRejection) -> Self {
        Self::BadRequest(format!("請求內容格式不正確：{}", r.body_text()))
    }
}

impl From<PathRejection> for AppError {
    fn from(r: PathRejection) -> Self {
        Self::BadRequest(format!("網址參數不正確：{}", r.body_text()))
    }
}

impl From<QueryRejection> for AppError {
    fn from(r: QueryRejection) -> Self {
        Self::BadRequest(format!("查詢參數不正確：{}", r.body_text()))
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code) = self.status_and_code();
        let message = match &self {
            Self::Internal(e) => {
                tracing::error!(error = %format!("{e:#}"), "internal error");
                "伺服器發生錯誤，請稍後再試。".to_string()
            }
            Self::BadRequest(m)
            | Self::Unauthorized(m)
            | Self::Forbidden(m)
            | Self::NotFound(m)
            | Self::Conflict(m) => m.clone(),
        };
        (
            status,
            Json(json!({ "error": { "code": code, "message": message } })),
        )
            .into_response()
    }
}

/// `axum::Json` whose rejection uses the API error format.
#[derive(axum::extract::FromRequest)]
#[from_request(via(axum::Json), rejection(AppError))]
pub struct ApiJson<T>(pub T);

/// `axum::extract::Path` whose rejection uses the API error format.
#[derive(axum::extract::FromRequestParts)]
#[from_request(via(axum::extract::Path), rejection(AppError))]
pub struct ApiPath<T>(pub T);

/// `axum::extract::Query` whose rejection uses the API error format.
#[derive(axum::extract::FromRequestParts)]
#[from_request(via(axum::extract::Query), rejection(AppError))]
pub struct ApiQuery<T>(pub T);
