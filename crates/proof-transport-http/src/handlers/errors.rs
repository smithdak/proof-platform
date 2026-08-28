//! Error response mapping for the HTTP transport.

use axum::{http::StatusCode, Json};
use proof_kernel::ExecutionError;
use serde_json::{json, Value};

pub(crate) fn execution_error_response(error: &ExecutionError) -> (StatusCode, Json<Value>) {
    let status = match error {
        ExecutionError::OperationNotFound { .. } => StatusCode::NOT_FOUND,
        ExecutionError::HumanOnly => StatusCode::FORBIDDEN,
        ExecutionError::ScopeViolation => StatusCode::FORBIDDEN,
        ExecutionError::Sunset => StatusCode::GONE,
        ExecutionError::NoHandler(_)
        | ExecutionError::HandlerFailed(_)
        | ExecutionError::EvidenceFailed(_)
        | ExecutionError::Delegation(_)
        | ExecutionError::StorageFailed(_)
        | ExecutionError::BenchmarkExpired { .. } => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, Json(json!({"error": error.to_string()})))
}

pub(crate) fn internal_error(error: String) -> (StatusCode, Json<Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": error })),
    )
}

pub(crate) fn bad_request(error: &str) -> (StatusCode, Json<Value>) {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": error })))
}
