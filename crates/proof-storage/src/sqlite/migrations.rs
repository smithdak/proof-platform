//! Schema migrations for the SQLite storage adapter.

use crate::StorageError;
use chrono::Utc;
use rusqlite::Connection;

pub struct Migration {
    pub version: u32,
    pub description: &'static str,
    pub up: &'static str,
    pub down: &'static str,
}

/// All ordered schema migrations.
pub const MIGRATIONS: &[Migration] = &[
    Migration {
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
    },
    Migration {
        version: 2,
        description: "create benchmark results schema",
        up: "
            CREATE TABLE IF NOT EXISTS benchmark_results (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                benchmark TEXT NOT NULL,
                operation TEXT NOT NULL,
                version TEXT NOT NULL,
                passed INTEGER NOT NULL CHECK (passed IN (0, 1)),
                duration_ms INTEGER NOT NULL CHECK (duration_ms >= 0),
                failure TEXT,
                recorded_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_benchmark_results_operation_version
                ON benchmark_results(operation, version, recorded_at);
            ",
        down: "
            DROP INDEX IF EXISTS idx_benchmark_results_operation_version;
            DROP TABLE IF EXISTS benchmark_results;
            ",
    },
    Migration {
        version: 3,
        description: "track proof expiration",
        up: "
            ALTER TABLE proofs ADD COLUMN expires_at TEXT;
            CREATE INDEX IF NOT EXISTS idx_proofs_expiration ON proofs(expires_at);
        ",
        down: "
            DROP INDEX IF EXISTS idx_proofs_expiration;
            ALTER TABLE proofs DROP COLUMN expires_at;
        ",
    },
];

/// A SQLite-backed store for Proof data.
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
