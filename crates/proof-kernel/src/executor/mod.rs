//! Operation execution engine: routes operations from transports through the kernel.

mod context;
mod engine;
mod error;
mod store;

pub use context::{AuditFilter, ExecutionContext};
pub use engine::{create_proof, ExecutionEngine, ExecutionOutcome};
pub use error::{ExecutionError, IdempotencyError};
pub use store::{
    ExecutionReplayClaim, ExecutionReplayClaimResult, ExecutionReplayKey, ExecutionStore,
    IdempotencyPolicy, OperationHandler, RecordingStore,
};
