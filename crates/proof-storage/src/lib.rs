//! SQLite and PostgreSQL storage adapters for the Proof platform.

pub mod cas;
pub mod sqlite;

pub use cas::{BlobReference, ContentAddressedStore, GarbageCollectionResult};
pub use sqlite::{Migration, SqliteStore, MIGRATIONS};

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
