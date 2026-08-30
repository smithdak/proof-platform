//! Error response mapping for the HTTP transport.

use axum::{http::StatusCode, Json};
use proof_kernel::{ExecutionError, IdempotencyError};
use serde_json::{json, Value};

pub(crate) fn execution_error_response(error: &ExecutionError) -> (StatusCode, Json<Value>) {
    let status = match error {
        ExecutionError::OperationNotFound { .. } => StatusCode::NOT_FOUND,
        ExecutionError::HumanOnly => StatusCode::FORBIDDEN,
        ExecutionError::Approval(_) => StatusCode::FORBIDDEN,
        ExecutionError::Delegation(_) => StatusCode::FORBIDDEN,
        ExecutionError::ScopeViolation => StatusCode::FORBIDDEN,
        ExecutionError::Sunset => StatusCode::GONE,
        ExecutionError::BenchmarkExpired { .. } => StatusCode::CONFLICT,
        ExecutionError::Idempotency(IdempotencyError::MissingKey)
        | ExecutionError::Idempotency(IdempotencyError::InvalidUuidV7) => StatusCode::BAD_REQUEST,
        ExecutionError::Idempotency(IdempotencyError::Conflict)
        | ExecutionError::Idempotency(IdempotencyError::InProgress)
        | ExecutionError::Idempotency(IdempotencyError::Indeterminate) => StatusCode::CONFLICT,
        ExecutionError::Idempotency(IdempotencyError::StorageRequired) => {
            StatusCode::SERVICE_UNAVAILABLE
        }
        ExecutionError::NoHandler(_)
        | ExecutionError::HandlerFailed(_)
        | ExecutionError::EvidenceFailed(_)
        | ExecutionError::StorageFailed(_) => StatusCode::INTERNAL_SERVER_ERROR,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_errors_are_forbidden() {
        let (status, _) = execution_error_response(&ExecutionError::Approval(
            proof_kernel::ApprovalError::Denied,
        ));

        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[test]
    fn delegation_errors_are_forbidden() {
        let (status, _) = execution_error_response(&ExecutionError::Delegation(
            proof_kernel::DelegationError::EmptyChain,
        ));

        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[test]
    fn expired_benchmark_evidence_is_a_conflict() {
        let (status, _) = execution_error_response(&ExecutionError::BenchmarkExpired {
            benchmark: "B1".to_string(),
            proof_id: uuid::Uuid::now_v7().to_string(),
        });

        assert_eq!(status, StatusCode::CONFLICT);
    }

    #[test]
    fn idempotency_errors_use_stable_http_classes() {
        for error in [
            IdempotencyError::MissingKey,
            IdempotencyError::InvalidUuidV7,
        ] {
            assert_eq!(
                execution_error_response(&error.into()).0,
                StatusCode::BAD_REQUEST
            );
        }
        for error in [
            IdempotencyError::Conflict,
            IdempotencyError::InProgress,
            IdempotencyError::Indeterminate,
        ] {
            assert_eq!(
                execution_error_response(&error.into()).0,
                StatusCode::CONFLICT
            );
        }
        assert_eq!(
            execution_error_response(&IdempotencyError::StorageRequired.into()).0,
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(
            execution_error_response(&ExecutionError::StorageFailed("corrupt".into())).0,
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }
}
