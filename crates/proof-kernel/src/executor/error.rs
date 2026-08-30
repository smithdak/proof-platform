//! Execution error types.

use crate::approval::ApprovalError;
use crate::delegation::DelegationError;
use thiserror::Error;

#[derive(Debug, Clone, Copy, Error, PartialEq, Eq)]
pub enum IdempotencyError {
    #[error("idempotency_key is required")]
    MissingKey,
    #[error("idempotency_key must be a UUIDv7 string")]
    InvalidUuidV7,
    #[error("idempotency key is already bound to different input")]
    Conflict,
    #[error("idempotency key is currently being executed; retry later")]
    InProgress,
    #[error("idempotency key has an indeterminate prior execution and requires reconciliation")]
    Indeterminate,
    #[error("durable exact-replay storage is required for this operation")]
    StorageRequired,
}

#[derive(Debug, Clone, Error, PartialEq)]
pub enum ExecutionError {
    #[error("operation not found: {operation} {version}")]
    OperationNotFound { operation: String, version: String },
    #[error("no handler registered for: {0}")]
    NoHandler(String),
    #[error("operation is human-only, agents cannot execute")]
    HumanOnly,
    #[error("approval invalid: {0}")]
    Approval(#[from] ApprovalError),
    #[error("operation is sunset and cannot be executed")]
    Sunset,
    #[error("delegation chain invalid: {0}")]
    Delegation(#[from] DelegationError),
    #[error("operation is outside the delegation scope")]
    ScopeViolation,
    #[error("handler execution failed: {0}")]
    HandlerFailed(String),
    #[error("evidence generation failed: {0}")]
    EvidenceFailed(String),
    #[error("benchmark proof expired: benchmark={benchmark} proof={proof_id}")]
    BenchmarkExpired { benchmark: String, proof_id: String },
    #[error("idempotency failed: {0}")]
    Idempotency(#[from] IdempotencyError),
    #[error("storage failed: {0}")]
    StorageFailed(String),
}
