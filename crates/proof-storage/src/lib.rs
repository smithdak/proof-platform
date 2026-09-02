//! SQLite and PostgreSQL storage adapters for the Proof platform.

pub mod cas;
pub mod sqlite;

pub use cas::{BlobReference, ContentAddressedStore, GarbageCollectionResult};
pub use sqlite::{
    acquire_operator_workspace_lock, initialize_operator_workspace_guarded,
    open_operator_schema14_existing, release_operator_workspace_lock,
    upgrade_operator_schema14_offline, AnalyticsInsight, AnalyticsInsightStatus, AnalyticsQuery,
    AnalyticsSnapshot, Catalog, CatalogProduct, Migration, OperatorLockMode, Order, OrderLine,
    OrderStatus, OwnedOperatorWorkspaceLock, ProofFilter, SqliteStore, WorkflowDefinition,
    WorkflowRun, WorkflowRunStatus, WorkflowStep, WorkflowStepKind, WorkflowStepStatus,
    WorkflowStepTemplate, MIGRATIONS,
};

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("filesystem error: {0}")]
    Io(#[from] std::io::Error),
}
