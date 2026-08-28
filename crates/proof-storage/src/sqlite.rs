//! SQLite storage adapter for Proof.

use crate::StorageError;
use rusqlite::Connection;
use std::path::Path;

/// A SQLite-backed store for Proof data.
pub struct SqliteStore {
    conn: Connection,
}

impl SqliteStore {
    /// Opens (or creates) a SQLite database at the given path and initializes the schema.
    pub fn open(path: &Path) -> Result<Self, StorageError> {
        let conn = Connection::open(path)?;
        Self::initialize(&conn)?;
        Ok(Self { conn })
    }

    /// Opens an in-memory database (for testing).
    pub fn in_memory() -> Result<Self, StorageError> {
        let conn = Connection::open_in_memory()?;
        Self::initialize(&conn)?;
        Ok(Self { conn })
    }

    fn initialize(conn: &Connection) -> Result<(), StorageError> {
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS schemas (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                version INTEGER NOT NULL,
                definition TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS objects (
                id TEXT PRIMARY KEY,
                schema_id TEXT NOT NULL REFERENCES schemas(id),
                schema_version INTEGER NOT NULL,
                locale TEXT NOT NULL,
                content TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'draft',
                revision INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS changesets (
                id TEXT PRIMARY KEY,
                intent TEXT NOT NULL,
                base_state_digest TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'draft',
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS changeset_edits (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                changeset_id TEXT NOT NULL REFERENCES changesets(id),
                edit_type TEXT NOT NULL,
                edit_data TEXT NOT NULL,
                ordinal INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS editions (
                id TEXT PRIMARY KEY,
                changeset_id TEXT NOT NULL REFERENCES changesets(id),
                objects TEXT NOT NULL,
                content_digest TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS releases (
                id TEXT PRIMARY KEY,
                edition_id TEXT NOT NULL REFERENCES editions(id),
                environment TEXT NOT NULL,
                published_at TEXT NOT NULL,
                published_by TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS proofs (
                id TEXT PRIMARY KEY,
                actor TEXT NOT NULL,
                delegation_id TEXT,
                operation TEXT NOT NULL,
                input_digest TEXT NOT NULL,
                output_digest TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                signature TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS principals (
                id TEXT PRIMARY KEY,
                kind TEXT NOT NULL,
                display_name TEXT NOT NULL,
                public_key TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS delegations (
                id TEXT PRIMARY KEY,
                issuer TEXT NOT NULL REFERENCES principals(id),
                recipient TEXT NOT NULL REFERENCES principals(id),
                allowed_actions TEXT NOT NULL,
                resource_scope TEXT NOT NULL,
                valid_from TEXT NOT NULL,
                valid_until TEXT NOT NULL,
                revoked INTEGER NOT NULL DEFAULT 0
            );

            CREATE INDEX IF NOT EXISTS idx_objects_schema ON objects(schema_id);
            CREATE INDEX IF NOT EXISTS idx_changesets_status ON changesets(status);
            CREATE INDEX IF NOT EXISTS idx_releases_edition ON releases(edition_id);
            CREATE INDEX IF NOT EXISTS idx_releases_env ON releases(environment);
            ",
        )?;
        Ok(())
    }

    /// Returns the underlying connection (for queries not covered by higher-level APIs).
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Returns the count of objects in the store.
    pub fn object_count(&self) -> Result<u64, StorageError> {
        let count: u64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM objects", [], |row| row.get(0))?;
        Ok(count)
    }

    /// Returns the count of schemas in the store.
    pub fn schema_count(&self) -> Result<u64, StorageError> {
        let count: u64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM schemas", [], |row| row.get(0))?;
        Ok(count)
    }

    /// Returns the count of proofs in the store.
    pub fn proof_count(&self) -> Result<u64, StorageError> {
        let count: u64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM proofs", [], |row| row.get(0))?;
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_in_memory() {
        let store = SqliteStore::in_memory().unwrap();
        assert_eq!(store.object_count().unwrap(), 0);
        assert_eq!(store.schema_count().unwrap(), 0);
        assert_eq!(store.proof_count().unwrap(), 0);
    }

    #[test]
    fn opens_file_database() {
        let dir = std::env::temp_dir().join("proof-storage-test");
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("test.db");
        let _ = std::fs::remove_file(&db_path);
        let store = SqliteStore::open(&db_path).unwrap();
        assert_eq!(store.object_count().unwrap(), 0);
    }
}
