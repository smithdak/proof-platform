//! SQLite storage adapter for Proof.

use crate::StorageError;
use chrono::{DateTime, Utc};
use ed25519_dalek::VerifyingKey;
use proof_kernel::{ExecutionContext, ExecutionStore, Proof, RegistryEntry};
use rusqlite::Connection;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use uuid::Uuid;

/// A single schema migration and its reverse transformation.
#[derive(Debug, Clone)]
pub struct Migration {
    pub version: u32,
    pub description: &'static str,
    pub up: &'static str,
    pub down: &'static str,
}

/// All ordered schema migrations.
pub const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    description: "create initial proof storage schema",
    up: "
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
                version TEXT,
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
            CREATE INDEX IF NOT EXISTS idx_proofs_operation ON proofs(operation);
            CREATE INDEX IF NOT EXISTS idx_proofs_operation_version ON proofs(operation, version);
            CREATE INDEX IF NOT EXISTS idx_proofs_actor ON proofs(actor);
            CREATE INDEX IF NOT EXISTS idx_execution_contexts_timestamp
                ON execution_contexts(timestamp);
            ",
    down: "
            DROP INDEX IF EXISTS idx_execution_contexts_timestamp;
            DROP INDEX IF EXISTS idx_proofs_actor;
            DROP INDEX IF EXISTS idx_proofs_operation_version;
            DROP INDEX IF EXISTS idx_proofs_operation;
            DROP INDEX IF EXISTS idx_releases_env;
            DROP INDEX IF EXISTS idx_releases_edition;
            DROP INDEX IF EXISTS idx_changesets_status;
            DROP INDEX IF EXISTS idx_objects_schema;
            DROP TABLE IF EXISTS delegations;
            DROP TABLE IF EXISTS principals;
            DROP TABLE IF EXISTS execution_contexts;
            DROP TABLE IF EXISTS registry_entries;
            DROP TABLE IF EXISTS proofs;
            DROP TABLE IF EXISTS releases;
            DROP TABLE IF EXISTS editions;
            DROP TABLE IF EXISTS changeset_edits;
            DROP TABLE IF EXISTS changesets;
            DROP TABLE IF EXISTS objects;
            DROP TABLE IF EXISTS schemas;
            ",
}];

/// A SQLite-backed store for Proof data.
pub struct SqliteStore {
    conn: Mutex<Connection>,
}

impl ExecutionStore for SqliteStore {
    fn save_proof(&self, proof: &Proof) -> Result<(), String> {
        SqliteStore::save_proof(self, proof).map_err(|error| error.to_string())
    }

    fn save_execution_context(&self, context: &ExecutionContext) -> Result<String, String> {
        SqliteStore::save_execution_context(self, context)
            .map(|context_id| context_id.to_string())
            .map_err(|error| error.to_string())
    }
}

impl SqliteStore {
    /// Opens (or creates) a SQLite database at the given path and initializes the schema.
    pub fn open(path: &Path) -> Result<Self, StorageError> {
        let conn = Connection::open(path)?;
        run_migrations(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Opens an in-memory database (for testing).
    pub fn in_memory() -> Result<Self, StorageError> {
        let conn = Connection::open_in_memory()?;
        run_migrations(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Returns the underlying connection for queries not covered by higher-level APIs.
    pub fn connection(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap()
    }

    /// Returns the count of objects in the store.
    pub fn object_count(&self) -> Result<u64, StorageError> {
        let count: u64 =
            self.conn
                .lock()
                .unwrap()
                .query_row("SELECT COUNT(*) FROM objects", [], |row| row.get(0))?;
        Ok(count)
    }

    /// Returns the count of schemas in the store.
    pub fn schema_count(&self) -> Result<u64, StorageError> {
        let count: u64 =
            self.conn
                .lock()
                .unwrap()
                .query_row("SELECT COUNT(*) FROM schemas", [], |row| row.get(0))?;
        Ok(count)
    }

    /// Returns the count of proofs in the store.
    pub fn proof_count(&self) -> Result<u64, StorageError> {
        let count: u64 =
            self.conn
                .lock()
                .unwrap()
                .query_row("SELECT COUNT(*) FROM proofs", [], |row| row.get(0))?;
        Ok(count)
    }

    /// Returns the count of audit contexts in the store.
    pub fn context_count(&self) -> Result<u64, StorageError> {
        let count: u64 = self.conn.lock().unwrap().query_row(
            "SELECT COUNT(*) FROM execution_contexts",
            [],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    /// Persists a principal so signed proofs can later be verified.
    pub fn save_principal(&self, principal: &proof_kernel::Principal) -> Result<(), StorageError> {
        self.conn.lock().unwrap().execute(
            "
            INSERT INTO principals (id, kind, display_name, public_key)
            VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT(id) DO UPDATE SET
                kind = excluded.kind,
                display_name = excluded.display_name,
                public_key = excluded.public_key
            ",
            rusqlite::params![
                principal.id.as_uuid().to_string(),
                serde_json::to_string(&principal.kind)?,
                serde_json::to_string(&principal.kind)?,
                principal.public_key.as_bytes().to_vec(),
            ],
        )?;
        Ok(())
    }

    /// Loads a principal by ID.
    pub fn load_principal(
        &self,
        principal_id: &proof_kernel::PrincipalId,
    ) -> Result<proof_kernel::Principal, StorageError> {
        let (id, kind, public_key) = self
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT id, kind, public_key FROM principals WHERE id = ?1",
                [principal_id.as_uuid().to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Vec<u8>>(2)?,
                    ))
                },
            )
            .map_err(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => {
                    StorageError::NotFound(principal_id.as_uuid().to_string())
                }
                error => error.into(),
            })?;
        let kind: proof_kernel::PrincipalKind = serde_json::from_str(&kind)?;
        let public_key_bytes: [u8; 32] = public_key
            .as_slice()
            .try_into()
            .map_err(|_| StorageError::Conflict("invalid principal public key".to_string()))?;
        let public_key = VerifyingKey::from_bytes(&public_key_bytes)
            .map_err(|_| StorageError::Conflict("invalid principal public key".to_string()))?;
        Ok(proof_kernel::Principal {
            id: proof_kernel::PrincipalId::new(Uuid::parse_str(&id).map_err(|error| {
                StorageError::Conflict(format!("invalid principal ID: {error}"))
            })?),
            kind,
            public_key,
            created_at: Utc::now(),
        })
    }

    /// Persists a serialized proof, replacing any prior proof with the same ID.
    pub fn save_proof(&self, proof: &Proof) -> Result<(), StorageError> {
        let serialized = serde_json::to_string(proof)?;
        let version = proof.body.operation.rsplit("::").next().map(str::to_string);
        let operation = proof.body.operation.clone();
        let actor = proof.body.actor.as_uuid().to_string();
        let id = proof.body.id.to_string();
        let delegation_id = proof
            .body
            .delegation_id
            .map(|delegation_id| delegation_id.to_string());
        let input_digest = proof.body.input_digest.hex();
        let output_digest = proof.body.output_digest.hex();
        let timestamp = proof.body.timestamp.to_rfc3339();
        self.conn.lock().unwrap().execute(
            "
                INSERT INTO proofs (
                    id, actor, version, delegation_id, operation, input_digest, output_digest,
                    timestamp, signature
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                ON CONFLICT(id) DO UPDATE SET
                    actor = excluded.actor,
                    version = excluded.version,
                    delegation_id = excluded.delegation_id,
                operation = excluded.operation,
                input_digest = excluded.input_digest,
                output_digest = excluded.output_digest,
                timestamp = excluded.timestamp,
                signature = excluded.signature
            ",
            rusqlite::params![
                id,
                actor,
                version,
                delegation_id,
                operation,
                input_digest,
                output_digest,
                timestamp,
                serialized,
            ],
        )?;
        Ok(())
    }

    /// Loads a proof by ID.
    pub fn load_proof(&self, proof_id: &Uuid) -> Result<Proof, StorageError> {
        let serialized: String = self
            .conn
            .lock()
            .unwrap()
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

    /// Loads all proofs for an operation in ascending proof timestamp order.
    pub fn list_proofs_for_operation(
        &self,
        operation: &str,
        version: Option<&str>,
    ) -> Result<Vec<Proof>, StorageError> {
        let connection = self.conn.lock().unwrap();
        let serialized_proofs;
        if let Some(version) = version {
            let mut statement = connection.prepare_cached(
                "SELECT signature FROM proofs
                 WHERE operation = ?1 AND version = ?2
                 ORDER BY timestamp, id",
            )?;
            serialized_proofs = statement
                .query_map(
                    rusqlite::params![format!("{operation}::{version}"), version],
                    |row| row.get::<_, String>(0),
                )?
                .collect::<Result<Vec<_>, _>>()?;
        } else {
            let mut statement = connection.prepare_cached(
                "SELECT signature FROM proofs WHERE operation LIKE ?1 || '::%' ORDER BY timestamp, id",
            )?;
            serialized_proofs = statement
                .query_map([operation], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
        }
        Ok(serialized_proofs
            .iter()
            .map(|serialized| serde_json::from_str(serialized))
            .collect::<Result<Vec<_>, _>>()?)
    }

    /// Loads all proofs signed by an actor in ascending proof timestamp order.
    pub fn list_proofs_for_actor(
        &self,
        actor_id: &proof_kernel::PrincipalId,
    ) -> Result<Vec<Proof>, StorageError> {
        let connection = self.conn.lock().unwrap();
        let mut statement = connection.prepare_cached(
            "SELECT signature FROM proofs WHERE actor = ?1 ORDER BY timestamp, id",
        )?;
        let serialized_proofs = statement
            .query_map([actor_id.as_uuid().to_string()], |row| {
                row.get::<_, String>(0)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        serialized_proofs
            .iter()
            .map(|serialized| Ok(serde_json::from_str(serialized)?))
            .collect()
    }

    /// Verifies signatures and digest continuity for the supplied proof chain.
    pub fn verify_proof_chain(&self, proof_ids: &[Uuid]) -> Result<(), StorageError> {
        let proofs = proof_ids
            .iter()
            .map(|proof_id| self.load_proof(proof_id))
            .collect::<Result<Vec<_>, _>>()?;
        for proof in &proofs {
            let principal =
                self.load_principal(&proof.body.actor)
                    .map_err(|error| match error {
                        StorageError::NotFound(_) => StorageError::Conflict(format!(
                            "missing principal for proof {}: {}",
                            proof.body.id, proof.body.actor
                        )),
                        error => error,
                    })?;
            proof.verify(&principal.public_key).map_err(|_| {
                StorageError::Conflict(format!("invalid signature for proof {}", proof.body.id))
            })?;
        }
        for pair in proofs.windows(2) {
            if pair[0].body.output_digest != pair[1].body.input_digest {
                return Err(StorageError::Conflict(format!(
                    "proof chain discontinuity between {} and {}",
                    pair[0].body.id, pair[1].body.id
                )));
            }
        }
        Ok(())
    }

    /// Deletes audit contexts strictly older than the supplied timestamp.
    pub fn delete_expired_contexts(&self, before: DateTime<Utc>) -> Result<u64, StorageError> {
        let deleted = self.conn.lock().unwrap().execute(
            "DELETE FROM execution_contexts WHERE timestamp < ?1",
            [before.to_rfc3339()],
        )?;
        Ok(deleted as u64)
    }

    /// Persists registry entries, replacing the stored collection.
    pub fn save_registry(&self, entries: &[RegistryEntry]) -> Result<(), StorageError> {
        let connection = self.conn.lock().unwrap();
        let transaction = connection.unchecked_transaction()?;
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
        let connection = self.conn.lock().unwrap();
        let mut statement =
            connection.prepare("SELECT data FROM registry_entries ORDER BY operation, version")?;
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
        self.conn.lock().unwrap().execute(
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

/// Returns the schema version recorded by the migration history.
pub fn schema_version(conn: &Connection) -> Result<u32, StorageError> {
    ensure_migration_table(conn)?;
    conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |row| row.get(0),
    )
    .map_err(StorageError::from)
}

/// Applies every pending migration in ascending version order.
pub fn run_migrations(conn: &Connection) -> Result<(), StorageError> {
    ensure_migration_table(conn)?;
    let applied = schema_version(conn)?;
    for migration in MIGRATIONS {
        if migration.version <= applied {
            continue;
        }
        conn.execute_batch(migration.up)?;
        conn.execute(
            "INSERT INTO schema_migrations (version, description, applied_at)
             VALUES (?1, ?2, ?3)",
            rusqlite::params![
                migration.version,
                migration.description,
                Utc::now().to_rfc3339()
            ],
        )?;
    }
    Ok(())
}

/// Rolls back migrations greater than `target_version`.
pub fn rollback_to(conn: &Connection, target_version: u32) -> Result<(), StorageError> {
    ensure_migration_table(conn)?;
    let current_version = schema_version(conn)?;
    if target_version >= current_version {
        return Ok(());
    }
    if target_version != 0
        && !MIGRATIONS
            .iter()
            .any(|migration| migration.version == target_version)
    {
        return Err(StorageError::Conflict(format!(
            "unknown migration target version: {target_version}"
        )));
    }
    for migration in MIGRATIONS.iter().rev() {
        if migration.version <= target_version || migration.version > current_version {
            continue;
        }
        conn.execute_batch(migration.down)?;
        conn.execute(
            "DELETE FROM schema_migrations WHERE version = ?1",
            [migration.version],
        )?;
    }
    Ok(())
}

fn ensure_migration_table(conn: &Connection) -> Result<(), StorageError> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            description TEXT NOT NULL,
            applied_at TEXT NOT NULL
        );
        ",
    )?;
    Ok(())
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

    fn signed_proof(
        keypair: &proof_kernel::Keypair,
        operation: &str,
        input_digest: proof_kernel::ContentDigest,
        output_digest: proof_kernel::ContentDigest,
        timestamp: chrono::DateTime<Utc>,
    ) -> Proof {
        Proof::new(
            Uuid::now_v7(),
            keypair.principal_id,
            None,
            operation,
            input_digest,
            output_digest,
            timestamp,
        )
        .sign(keypair)
        .unwrap()
    }

    fn json_digest(
        kind: proof_kernel::ArtifactKind,
        value: serde_json::Value,
    ) -> proof_kernel::ContentDigest {
        let canonical = proof_kernel::canonicalize(&value).unwrap();
        proof_kernel::digest(kind, &canonical)
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
            status: proof_kernel::VersionStatus::Active,
            deprecated_since: None,
            replacement_operation: None,
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
            delegation_chain: None,
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

    #[test]
    fn principals_round_trip() {
        let store = SqliteStore::in_memory().unwrap();
        let keypair = generate_keypair_for(proof_kernel::PrincipalKind::Agent);
        let principal = proof_kernel::principal_from_keypair(&keypair);

        store.save_principal(&principal).unwrap();
        let loaded = store.load_principal(&principal.id).unwrap();

        assert_eq!(loaded.id, principal.id);
        assert_eq!(loaded.kind, principal.kind);
        assert_eq!(
            loaded.public_key.as_bytes(),
            principal.public_key.as_bytes()
        );
        assert!(matches!(
            store.load_principal(&proof_kernel::PrincipalId::now()),
            Err(StorageError::NotFound(_))
        ));
    }

    #[test]
    fn lists_proofs_by_operation_and_version() {
        let store = SqliteStore::in_memory().unwrap();
        let keypair = generate_keypair_for(proof_kernel::PrincipalKind::Agent);
        let principal = proof_kernel::principal_from_keypair(&keypair);
        store.save_principal(&principal).unwrap();

        let first = signed_proof(
            &keypair,
            "chain.operation::v1",
            json_digest(
                proof_kernel::ArtifactKind::OperationInput,
                json!({"step": 0}),
            ),
            json_digest(
                proof_kernel::ArtifactKind::OperationOutput,
                json!({"step": 1}),
            ),
            Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 1).unwrap(),
        );
        let second = signed_proof(
            &keypair,
            "chain.operation::v2",
            json_digest(
                proof_kernel::ArtifactKind::OperationInput,
                json!({"step": 2}),
            ),
            json_digest(
                proof_kernel::ArtifactKind::OperationOutput,
                json!({"step": 3}),
            ),
            Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 2).unwrap(),
        );
        store.save_proof(&first).unwrap();
        store.save_proof(&second).unwrap();

        let all = store
            .list_proofs_for_operation("chain.operation", None)
            .unwrap();
        let v1 = store
            .list_proofs_for_operation("chain.operation", Some("v1"))
            .unwrap();
        let v2 = store
            .list_proofs_for_operation("chain.operation", Some("v2"))
            .unwrap();

        assert_eq!(all, vec![first.clone(), second.clone()]);
        assert_eq!(v1, vec![first]);
        assert_eq!(v2, vec![second]);
    }

    #[test]
    fn lists_proofs_by_actor() {
        let store = SqliteStore::in_memory().unwrap();
        let first_keypair = generate_keypair_for(proof_kernel::PrincipalKind::Agent);
        let second_keypair = generate_keypair_for(proof_kernel::PrincipalKind::Agent);
        for keypair in [&first_keypair, &second_keypair] {
            store
                .save_principal(&proof_kernel::principal_from_keypair(keypair))
                .unwrap();
        }

        let first = signed_proof(
            &first_keypair,
            "actor.operation::v1",
            json_digest(proof_kernel::ArtifactKind::OperationInput, json!({"a": 1})),
            json_digest(proof_kernel::ArtifactKind::OperationOutput, json!({"a": 2})),
            Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 1).unwrap(),
        );
        let second = signed_proof(
            &first_keypair,
            "actor.operation::v1",
            json_digest(proof_kernel::ArtifactKind::OperationInput, json!({"a": 2})),
            json_digest(proof_kernel::ArtifactKind::OperationOutput, json!({"a": 3})),
            Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 2).unwrap(),
        );
        let other = signed_proof(
            &second_keypair,
            "actor.operation::v1",
            json_digest(proof_kernel::ArtifactKind::OperationInput, json!({"b": 1})),
            json_digest(proof_kernel::ArtifactKind::OperationOutput, json!({"b": 2})),
            Utc::now(),
        );
        for proof in [&first, &second, &other] {
            store.save_proof(proof).unwrap();
        }

        let actor_proofs = store
            .list_proofs_for_actor(&first_keypair.principal_id)
            .unwrap();

        assert_eq!(actor_proofs, vec![first, second]);
    }

    #[test]
    fn verifies_a_valid_proof_chain() {
        let store = SqliteStore::in_memory().unwrap();
        let keypair = generate_keypair_for(proof_kernel::PrincipalKind::Agent);
        store
            .save_principal(&proof_kernel::principal_from_keypair(&keypair))
            .unwrap();

        let first_input = json_digest(
            proof_kernel::ArtifactKind::OperationInput,
            json!({"state": 0}),
        );
        let first_output = json_digest(
            proof_kernel::ArtifactKind::OperationOutput,
            json!({"state": 1}),
        );
        let second_input = first_output;
        let second_output = json_digest(
            proof_kernel::ArtifactKind::OperationOutput,
            json!({"state": 2}),
        );
        let first = signed_proof(
            &keypair,
            "chain.operation::v1",
            first_input,
            first_output,
            Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 1).unwrap(),
        );
        let second = signed_proof(
            &keypair,
            "chain.operation::v1",
            second_input,
            second_output,
            Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 2).unwrap(),
        );
        store.save_proof(&first).unwrap();
        store.save_proof(&second).unwrap();

        store
            .verify_proof_chain(&[first.body.id, second.body.id])
            .unwrap();
    }

    #[test]
    fn rejects_signature_without_principal() {
        let store = SqliteStore::in_memory().unwrap();
        let proof = test_proof();
        store.save_proof(&proof).unwrap();

        let result = store.verify_proof_chain(&[proof.body.id]);

        assert!(matches!(result, Err(StorageError::Conflict(_))));
    }

    #[test]
    fn rejects_disconnected_proof_chain() {
        let store = SqliteStore::in_memory().unwrap();
        let keypair = generate_keypair_for(proof_kernel::PrincipalKind::Agent);
        store
            .save_principal(&proof_kernel::principal_from_keypair(&keypair))
            .unwrap();

        let first = signed_proof(
            &keypair,
            "chain.operation::v1",
            json_digest(
                proof_kernel::ArtifactKind::OperationInput,
                json!({"state": 0}),
            ),
            json_digest(
                proof_kernel::ArtifactKind::OperationOutput,
                json!({"state": 1}),
            ),
            Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 1).unwrap(),
        );
        let second = signed_proof(
            &keypair,
            "chain.operation::v1",
            json_digest(
                proof_kernel::ArtifactKind::OperationInput,
                json!({"unrelated": 1}),
            ),
            json_digest(
                proof_kernel::ArtifactKind::OperationOutput,
                json!({"unrelated": 2}),
            ),
            Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 2).unwrap(),
        );
        store.save_proof(&first).unwrap();
        store.save_proof(&second).unwrap();

        let result = store.verify_proof_chain(&[first.body.id, second.body.id]);

        assert!(matches!(result, Err(StorageError::Conflict(_))));
    }

    #[test]
    fn deletes_expired_contexts() {
        let store = SqliteStore::in_memory().unwrap();
        let keypair = generate_keypair_for(proof_kernel::PrincipalKind::Human);
        let make_context = |timestamp| ExecutionContext {
            actor: keypair.principal_id,
            delegation_id: None,
            delegation_chain: None,
            workspace_path: PathBuf::from("/tmp/workspace"),
            timestamp,
        };
        let expired_id = store
            .save_execution_context(&make_context(
                Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            ))
            .unwrap();
        let keep_id = store
            .save_execution_context(&make_context(
                Utc.with_ymd_and_hms(2026, 1, 2, 0, 0, 0).unwrap(),
            ))
            .unwrap();

        let deleted = store
            .delete_expired_contexts(Utc.with_ymd_and_hms(2026, 1, 2, 0, 0, 0).unwrap())
            .unwrap();
        let count = store.context_count().unwrap();

        assert_eq!(deleted, 1);
        assert_eq!(count, 1);
        assert_eq!(store.proof_count().unwrap(), 0);
        let remaining: Vec<String> = store
            .connection()
            .prepare("SELECT id FROM execution_contexts")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(remaining, vec![keep_id.to_string()]);
        assert_ne!(expired_id, keep_id);
    }
}
