use proof_storage::sqlite::{rollback_to, run_migrations, schema_version, SqliteStore};
use rusqlite::Connection;
use tempfile::TempDir;

#[test]
fn fresh_database_applies_all_migrations() {
    let connection = Connection::open_in_memory().unwrap();

    run_migrations(&connection).unwrap();

    assert_eq!(schema_version(&connection).unwrap(), 6);
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

    assert_eq!(schema_version(&connection).unwrap(), 6);
}

#[test]
fn open_and_in_memory_run_migrations_automatically() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("proof.db");
    let file_store = SqliteStore::open(&path).unwrap();
    let memory_store = SqliteStore::in_memory().unwrap();

    assert_eq!(schema_version(&file_store.connection()).unwrap(), 6);
    assert_eq!(schema_version(&memory_store.connection()).unwrap(), 6);
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

    rollback_to(&connection, 6).unwrap();

    assert_eq!(schema_version(&connection).unwrap(), 6);
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

    assert_eq!(schema_version(&connection).unwrap(), 6);
}
