//! SQLite storage adapter for Proof.

use crate::StorageError;
use proof_kernel::{ExecutionContext, Proof, RegistryEntry};
use rusqlite::Connection;
use std::path::Path;
use uuid::Uuid;

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

            CREATE TABLE IF NOT EXISTS registry_entries (
                operation TEXT NOT NULL,
                version TEXT NOT NULL,
                data TEXT NOT NULL,
                PRIMARY KEY (operation, version)
            );

            CREATE TABLE IF NOT EXISTS execution_contexts (
                id TEXT PRIMARY KEY,
                actor TEXT NOT NULL,
                delegation_id TEXT,
                workspace_path TEXT NOT NULL,
                timestamp TEXT NOT NULL
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

    /// Persists a serialized proof, replacing any prior proof with the same ID.
    pub fn save_proof(&self, proof: &Proof) -> Result<(), StorageError> {
        let serialized = serde_json::to_string(proof)?;
        self.conn.execute(
            "
            INSERT INTO proofs (
                id, actor, delegation_id, operation, input_digest, output_digest,
                timestamp, signature
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            ON CONFLICT(id) DO UPDATE SET
                actor = excluded.actor,
                delegation_id = excluded.delegation_id,
                operation = excluded.operation,
                input_digest = excluded.input_digest,
                output_digest = excluded.output_digest,
                timestamp = excluded.timestamp,
                signature = excluded.signature
            ",
            rusqlite::params![
                proof.body.id.to_string(),
                proof.body.actor.as_uuid().to_string(),
                proof
                    .body
                    .delegation_id
                    .map(|delegation_id| delegation_id.to_string()),
                proof.body.operation,
                proof.body.input_digest.hex(),
                proof.body.output_digest.hex(),
                proof.body.timestamp.to_rfc3339(),
                serialized,
            ],
        )?;
        Ok(())
    }

    /// Loads a proof by ID.
    pub fn load_proof(&self, proof_id: &Uuid) -> Result<Proof, StorageError> {
        let serialized: String = self
            .conn
            .query_row(
                "SELECT signature FROM proofs WHERE id = ?1",
                [proof_id.to_string()],
                |row| row.get(0),
            )
            .map_err(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => {
                    StorageError::NotFound(proof_id.to_string())
                }
                error => error.into(),
            })?;
        Ok(serde_json::from_str(&serialized)?)
    }

    /// Persists registry entries, replacing the stored collection.
    pub fn save_registry(&self, entries: &[RegistryEntry]) -> Result<(), StorageError> {
        let transaction = self.conn.unchecked_transaction()?;
        transaction.execute("DELETE FROM registry_entries", [])?;
        for entry in entries {
            transaction.execute(
                "
                INSERT INTO registry_entries (operation, version, data)
                VALUES (?1, ?2, ?3)
                ",
                rusqlite::params![
                    entry.operation,
                    entry.version,
                    serde_json::to_string(entry)?,
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Loads all persisted registry entries in operation/version order.
    pub fn load_registry(&self) -> Result<Vec<RegistryEntry>, StorageError> {
        let mut statement = self
            .conn
            .prepare("SELECT data FROM registry_entries ORDER BY operation, version")?;
        let serialized_entries = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let entries = serialized_entries
            .iter()
            .map(|serialized| serde_json::from_str(serialized))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(entries)
    }

    /// Persists an execution context for the audit trail.
    pub fn save_execution_context(&self, context: &ExecutionContext) -> Result<Uuid, StorageError> {
        let context_id = Uuid::now_v7();
        self.conn.execute(
            "
            INSERT INTO execution_contexts (
                id, actor, delegation_id, workspace_path, timestamp
            ) VALUES (?1, ?2, ?3, ?4, ?5)
            ",
            rusqlite::params![
                context_id.to_string(),
                context.actor.as_uuid().to_string(),
                context
                    .delegation_id
                    .map(|delegation_id| delegation_id.to_string()),
                context.workspace_path.display().to_string(),
                context.timestamp.to_rfc3339(),
            ],
        )?;
        Ok(context_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use proof_kernel::{canonicalize, digest, generate_keypair_for, ArtifactKind, Governance};
    use serde_json::json;
    use std::path::PathBuf;

    fn test_proof() -> Proof {
        let keypair = generate_keypair_for(proof_kernel::PrincipalKind::Agent);
        let input_json = canonicalize(&json!({"input": "test"})).unwrap();
        let output_json = canonicalize(&json!({"output": "ok"})).unwrap();
        let input = digest(ArtifactKind::OperationInput, &input_json);
        let output = digest(ArtifactKind::OperationOutput, &output_json);
        Proof::new(
            Uuid::now_v7(),
            keypair.principal_id,
            None,
            "test.operation",
            input,
            output,
            Utc.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap(),
        )
        .sign(&keypair)
        .unwrap()
    }

    fn test_registry_entry(operation: &str) -> RegistryEntry {
        RegistryEntry {
            operation: operation.to_string(),
            domain: "test".to_string(),
            version: "v1".to_string(),
            action: "test:action".to_string(),
            description: "test entry".to_string(),
            input_schema: "input.json".to_string(),
            output_schema: "output.json".to_string(),
            required_authority: "delegation-grant".to_string(),
            governance: Governance::AgentExecutable,
            idempotency: "required".to_string(),
            consequence: "test-consequence".to_string(),
            evidence_contract: "test-contract".to_string(),
            benchmark: None,
        }
    }

    #[test]
    fn proof_round_trips() {
        let store = SqliteStore::in_memory().unwrap();
        let proof = test_proof();
        store.save_proof(&proof).unwrap();
        let loaded = store.load_proof(&proof.body.id).unwrap();
        assert_eq!(loaded, proof);
    }

    #[test]
    fn registry_entries_round_trip() {
        let store = SqliteStore::in_memory().unwrap();
        let entries = vec![
            test_registry_entry("alpha.operation"),
            test_registry_entry("zulu.operation"),
        ];
        store.save_registry(&entries).unwrap();
        let loaded = store.load_registry().unwrap();
        assert_eq!(loaded, entries);
    }

    #[test]
    fn execution_context_is_persisted() {
        let store = SqliteStore::in_memory().unwrap();
        let keypair = generate_keypair_for(proof_kernel::PrincipalKind::Human);
        let context = ExecutionContext {
            actor: keypair.principal_id,
            delegation_id: None,
            workspace_path: PathBuf::from("/tmp/workspace"),
            timestamp: Utc::now(),
        };
        let context_id = store.save_execution_context(&context).unwrap();
        let count: i64 = store
            .connection()
            .query_row("SELECT COUNT(*) FROM execution_contexts", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);
        assert!(Uuid::parse_str(&context_id.to_string()).is_ok());
    }
}
