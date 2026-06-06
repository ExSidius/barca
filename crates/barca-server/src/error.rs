//! HTTP error mapping — turns `BarcaError` into JSON responses with sensible
//! status codes.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use barca_core::BarcaError;
use serde_json::json;

/// Error type returned by route handlers. Implements `IntoResponse` so handlers
/// can use `?` directly.
#[derive(Debug)]
pub enum ApiError {
    /// A core engine error (parse/dag/db/worker).
    Barca(BarcaError),
    /// A resource (asset, run) was not found.
    NotFound(String),
}

impl From<BarcaError> for ApiError {
    fn from(e: BarcaError) -> Self {
        ApiError::Barca(e)
    }
}

impl From<tokio::task::JoinError> for ApiError {
    fn from(e: tokio::task::JoinError) -> Self {
        ApiError::Barca(BarcaError::Other(format!("background task failed: {e}")))
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            ApiError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            ApiError::Barca(err) => {
                let status = match &err {
                    BarcaError::AssetNotFound(..) => StatusCode::NOT_FOUND,
                    BarcaError::Parse(_) | BarcaError::Dag(_) => StatusCode::BAD_REQUEST,
                    _ => StatusCode::INTERNAL_SERVER_ERROR,
                };
                (status, err.to_string())
            }
        };
        (status, Json(json!({ "error": message }))).into_response()
    }
}
