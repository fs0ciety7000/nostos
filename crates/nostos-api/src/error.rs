use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

/// Errors returned to API clients as `{"error": "...", "message": "..."}`.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("{0}")]
    BadRequest(String),
    #[error("missing or invalid owner token")]
    Unauthorized,
    #[error("{0} not found")]
    NotFound(&'static str),
    #[error("{0}")]
    Conflict(String),
    #[error("database error")]
    Db(#[from] sqlx::Error),
    #[error("storage error")]
    Storage(#[from] object_store::Error),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            Self::BadRequest(_) => (StatusCode::BAD_REQUEST, "bad_request"),
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            Self::NotFound(_) | Self::Db(sqlx::Error::RowNotFound) => {
                (StatusCode::NOT_FOUND, "not_found")
            }
            Self::Conflict(_) => (StatusCode::CONFLICT, "conflict"),
            Self::Db(e) => {
                tracing::error!(error = %e, "database error");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal")
            }
            Self::Storage(e) => {
                tracing::error!(error = %e, "storage error");
                (StatusCode::BAD_GATEWAY, "storage")
            }
        };
        (
            status,
            Json(json!({ "error": code, "message": self.to_string() })),
        )
            .into_response()
    }
}

pub type ApiResult<T> = Result<T, ApiError>;
