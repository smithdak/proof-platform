//! SQLite storage adapter (modular: see sqlite/ directory).

mod methods;
mod migrations;
mod store;
pub mod tests;

pub use migrations::{rollback_to, run_migrations, schema_version, Migration, MIGRATIONS};
pub use store::{ProofFilter, SqliteStore};
