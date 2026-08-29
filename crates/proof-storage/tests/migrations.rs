use proof_storage::sqlite::{rollback_to, run_migrations, schema_version, SqliteStore, MIGRATIONS};
use proof_storage::StorageError;
use rusqlite::{params, Connection, OptionalExtension};
use std::sync::{Arc, Barrier};
use std::thread;
use tempfile::TempDir;

#[test]
fn fresh_database_applies_all_migrations() {
    let connection = Connection::open_in_memory().unwrap();

    run_migrations(&connection).unwrap();

    assert_eq!(schema_version(&connection).unwrap(), 10);
    let history_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(history_count, 1);
    for table in [
        "schemas",
        "objects",
        "changesets",
        "changeset_edits",
        "editions",
        "releases",
        "proofs",
        "registry_entries",
        "execution_contexts",
        "principals",
        "delegations",
        "benchmark_results",
        "workflow_definition",
        "workflow_run",
        "workflow_step",
        "analytics_snapshot",
        "analytics_query",
        "analytics_insight",
        "approval_requests",
        "approval_decisions",
        "approval_executions",
        "agent_runs",
        "agent_run_steps",
        "agent_checkpoints",
        "agent_run_evaluations",
        "agent_definitions",
        "agent_run_events",
    ] {
        connection
            .prepare(&format!("SELECT 1 FROM {table}"))
            .unwrap_or_else(|error| panic!("missing table {table}: {error}"));
    }
}

#[test]
fn migrations_are_idempotent() {
    let connection = Connection::open_in_memory().unwrap();

    run_migrations(&connection).unwrap();
    run_migrations(&connection).unwrap();

    assert_eq!(schema_version(&connection).unwrap(), 10);
}

#[test]
fn open_and_in_memory_run_migrations_automatically() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("proof.db");
    let file_store = SqliteStore::open(&path).unwrap();
    let memory_store = SqliteStore::in_memory().unwrap();

    assert_eq!(schema_version(&file_store.connection()).unwrap(), 10);
    assert_eq!(schema_version(&memory_store.connection()).unwrap(), 10);
}

#[test]
fn rollback_reverts_to_target_version() {
    let connection = Connection::open_in_memory().unwrap();
    run_migrations(&connection).unwrap();

    rollback_to(&connection, 0).unwrap();

    assert_eq!(schema_version(&connection).unwrap(), 0);
    let table_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'table'
               AND name NOT LIKE 'sqlite_%'
               AND name != 'schema_migrations'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(table_count, 0);
}

#[test]
fn rollback_to_current_version_is_a_no_op() {
    let connection = Connection::open_in_memory().unwrap();
    run_migrations(&connection).unwrap();

    rollback_to(&connection, 10).unwrap();

    assert_eq!(schema_version(&connection).unwrap(), 10);
}

#[test]
fn rollback_rejects_unknown_target() {
    let connection = Connection::open_in_memory().unwrap();
    run_migrations(&connection).unwrap();

    let result = rollback_to(&connection, 6);

    assert!(result.is_ok());
    assert_eq!(schema_version(&connection).unwrap(), 6);
}

#[test]
fn rolled_back_database_can_be_migrated_again() {
    let connection = Connection::open_in_memory().unwrap();
    run_migrations(&connection).unwrap();

    rollback_to(&connection, 0).unwrap();
    run_migrations(&connection).unwrap();

    assert_eq!(schema_version(&connection).unwrap(), 10);
}

#[test]
fn migration_10_adds_a_reversible_partial_unique_index() {
    let connection = Connection::open_in_memory().unwrap();
    run_migrations(&connection).unwrap();

    let index_sql: String = connection
        .query_row(
            "SELECT sql FROM sqlite_master
             WHERE type = 'index' AND name = 'idx_agent_run_steps_approval_unique'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(index_sql.contains("CREATE UNIQUE INDEX"));
    assert!(index_sql.contains("WHERE approval_request_id IS NOT NULL"));

    rollback_to(&connection, 9).unwrap();

    assert_eq!(schema_version(&connection).unwrap(), 9);
    let unique_index: Option<String> = connection
        .query_row(
            "SELECT name FROM sqlite_master
             WHERE type = 'index' AND name = 'idx_agent_run_steps_approval_unique'",
            [],
            |row| row.get(0),
        )
        .optional()
        .unwrap();
    assert_eq!(unique_index, None);
    let original_index: String = connection
        .query_row(
            "SELECT name FROM sqlite_master
             WHERE type = 'index' AND name = 'idx_agent_run_steps_approval'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(original_index, "idx_agent_run_steps_approval");
}

#[test]
fn migration_10_rejects_preexisting_duplicate_approval_bindings() {
    let connection = Connection::open_in_memory().unwrap();
    connection
        .execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE schema_migrations (
                 version INTEGER PRIMARY KEY,
                 description TEXT NOT NULL,
                 applied_at TEXT NOT NULL
             );",
        )
        .unwrap();
    for migration in MIGRATIONS.iter().filter(|migration| migration.version <= 9) {
        connection.execute_batch(migration.up).unwrap();
        connection
            .execute(
                "INSERT INTO schema_migrations (version, description, applied_at)
                 VALUES (?1, ?2, ?3)",
                params![
                    migration.version,
                    migration.description,
                    "2026-08-29T15:00:00Z"
                ],
            )
            .unwrap();
    }
    connection
        .execute(
            "INSERT INTO approval_requests (
                 id, requested_by, operation, version, input_digest,
                 requested_at, expires_at, request_json
             ) VALUES ('approval-1', 'agent-1', 'release.publish', 'v1', 'digest',
                       '2026-08-29T15:00:00Z', '2026-08-29T15:15:00Z', '{}')",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO agent_runs (
                 id, actor, agent_id, mode, status, revision, created_at, updated_at, run_json
             ) VALUES ('run-1', 'agent-1', NULL, 'one_shot', 'running', 0,
                       '2026-08-29T15:00:00Z', '2026-08-29T15:00:00Z', '{}')",
            [],
        )
        .unwrap();
    for (id, ordinal) in [("step-1", 0), ("step-2", 1)] {
        connection
            .execute(
                "INSERT INTO agent_run_steps (
                     id, run_id, ordinal, attempt, status, approval_request_id,
                     revision, created_at, updated_at, step_json
                 ) VALUES (?1, 'run-1', ?2, 1, 'waiting_for_approval', 'approval-1',
                           0, '2026-08-29T15:00:00Z', '2026-08-29T15:00:00Z', '{}')",
                params![id, ordinal],
            )
            .unwrap();
    }

    let error = run_migrations(&connection).unwrap_err();

    assert!(matches!(
        error,
        StorageError::Conflict(message)
            if message.contains("approval request approval-1 is bound to 2 agent run steps")
    ));
    assert_eq!(schema_version(&connection).unwrap(), 9);
}

#[test]
fn concurrent_openers_apply_pending_migration_once() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("proof.db");
    let store = SqliteStore::open(&path).unwrap();
    rollback_to(&store.connection(), 9).unwrap();
    drop(store);

    let worker_count = 4;
    let barrier = Arc::new(Barrier::new(worker_count));
    let handles = (0..worker_count)
        .map(|_| {
            let barrier = barrier.clone();
            let path = path.clone();
            thread::spawn(move || {
                barrier.wait();
                let store = SqliteStore::open(&path)?;
                let version = schema_version(&store.connection());
                version
            })
        })
        .collect::<Vec<_>>();

    for handle in handles {
        assert_eq!(handle.join().unwrap().unwrap(), 10);
    }
}
