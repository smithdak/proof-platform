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

    assert_eq!(schema_version(&connection).unwrap(), 13);
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
        "execution_replays",
        "live_run_start_claims",
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

    assert_eq!(schema_version(&connection).unwrap(), 13);
}

#[test]
fn open_and_in_memory_run_migrations_automatically() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("proof.db");
    let file_store = SqliteStore::open(&path).unwrap();
    let memory_store = SqliteStore::in_memory().unwrap();

    assert_eq!(schema_version(&file_store.connection()).unwrap(), 13);
    assert_eq!(schema_version(&memory_store.connection()).unwrap(), 13);
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

    rollback_to(&connection, 13).unwrap();

    assert_eq!(schema_version(&connection).unwrap(), 13);
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

    assert_eq!(schema_version(&connection).unwrap(), 13);
}

#[test]
fn migration_12_upgrades_v11_with_empty_scope_and_rolls_back_losslessly() {
    let connection = Connection::open_in_memory().unwrap();
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .unwrap();
    run_migrations(&connection).unwrap();
    rollback_to(&connection, 11).unwrap();
    let issuer = uuid::Uuid::now_v7().to_string();
    let recipient = uuid::Uuid::now_v7().to_string();
    let delegation_id = uuid::Uuid::now_v7().to_string();
    for (id, kind) in [(&issuer, "\"human\""), (&recipient, "\"agent\"")] {
        connection
            .execute(
                "INSERT INTO principals (id, kind, display_name, public_key)
                 VALUES (?1, ?2, ?2, ?3)",
                params![id, kind, vec![0_u8; 32]],
            )
            .unwrap();
    }
    connection
        .execute(
            "INSERT INTO delegations (
                 id, issuer, recipient, allowed_actions, resource_scope,
                 valid_from, valid_until, revoked
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1)",
            params![
                delegation_id,
                issuer,
                recipient,
                r#"["content:release_publish"]"#,
                r#"["workspace:preview/*"]"#,
                "2026-08-30T12:00:00+00:00",
                "2026-08-30T12:05:00+00:00",
            ],
        )
        .unwrap();

    run_migrations(&connection).unwrap();

    assert_eq!(schema_version(&connection).unwrap(), 13);
    let scope_json: String = connection
        .query_row(
            "SELECT scope_json FROM delegations WHERE id = ?1",
            [&delegation_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(scope_json, "{}");

    rollback_to(&connection, 11).unwrap();

    assert_eq!(schema_version(&connection).unwrap(), 11);
    let columns = connection
        .prepare("PRAGMA table_info(delegations)")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        columns,
        vec![
            "id",
            "issuer",
            "recipient",
            "allowed_actions",
            "resource_scope",
            "valid_from",
            "valid_until",
            "revoked",
        ]
    );
    let legacy_row: (String, String, String, String, String, String, String, i64) = connection
        .query_row(
            "SELECT id, issuer, recipient, allowed_actions, resource_scope,
                    valid_from, valid_until, revoked
             FROM delegations WHERE id = ?1",
            [&delegation_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(
        legacy_row,
        (
            delegation_id.clone(),
            issuer,
            recipient,
            r#"["content:release_publish"]"#.to_string(),
            r#"["workspace:preview/*"]"#.to_string(),
            "2026-08-30T12:00:00+00:00".to_string(),
            "2026-08-30T12:05:00+00:00".to_string(),
            1,
        )
    );

    run_migrations(&connection).unwrap();
    assert_eq!(schema_version(&connection).unwrap(), 13);
    let restored_scope: String = connection
        .query_row(
            "SELECT scope_json FROM delegations WHERE id = ?1",
            [&delegation_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(restored_scope, "{}");
}

#[test]
fn migration_11_upgrades_v10_with_an_empty_ledger_and_preserves_existing_data() {
    let connection = Connection::open_in_memory().unwrap();
    run_migrations(&connection).unwrap();
    rollback_to(&connection, 10).unwrap();
    connection
        .execute(
            "INSERT INTO schemas (id, name, version, definition, created_at)
             VALUES ('schema-1', 'Article', 1, '{}', '2026-08-29T15:00:00Z')",
            [],
        )
        .unwrap();

    run_migrations(&connection).unwrap();

    assert_eq!(schema_version(&connection).unwrap(), 13);
    let schema_name: String = connection
        .query_row(
            "SELECT name FROM schemas WHERE id = 'schema-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(schema_name, "Article");
    let replay_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM execution_replays", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(replay_count, 0);
}

#[test]
fn migration_11_is_reversible_and_can_be_reapplied_without_touching_v10_data() {
    let connection = Connection::open_in_memory().unwrap();
    run_migrations(&connection).unwrap();
    connection
        .execute(
            "INSERT INTO schemas (id, name, version, definition, created_at)
             VALUES ('schema-1', 'Article', 1, '{}', '2026-08-29T15:00:00Z')",
            [],
        )
        .unwrap();

    rollback_to(&connection, 10).unwrap();

    assert_eq!(schema_version(&connection).unwrap(), 10);
    let replay_table: Option<String> = connection
        .query_row(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'execution_replays'",
            [],
            |row| row.get(0),
        )
        .optional()
        .unwrap();
    let replay_index: Option<String> = connection
        .query_row(
            "SELECT name FROM sqlite_master
             WHERE type = 'index' AND name = 'idx_execution_replays_state_claimed_at'",
            [],
            |row| row.get(0),
        )
        .optional()
        .unwrap();
    assert_eq!(replay_table, None);
    assert_eq!(replay_index, None);
    let schema_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM schemas WHERE id = 'schema-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(schema_count, 1);

    run_migrations(&connection).unwrap();

    assert_eq!(schema_version(&connection).unwrap(), 13);
    connection
        .prepare("SELECT 1 FROM execution_replays")
        .unwrap();
    let schema_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM schemas WHERE id = 'schema-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(schema_count, 1);
}

#[test]
fn migration_11_enforces_digest_state_and_identity_constraints() {
    let connection = Connection::open_in_memory().unwrap();
    run_migrations(&connection).unwrap();
    let insert_claimed =
        |operation: &str, key: &str, digest: &str, token: &str, completed_at: Option<&str>| {
            connection.execute(
                "INSERT INTO execution_replays (
                 operation, version, idempotency_key, input_digest, state, claim_token,
                 claimed_by, claimed_at, completed_at
             ) VALUES (?1, 'v1', ?2, ?3, 'claimed', ?4, 'actor-1',
                       '2026-08-29T15:00:00Z', ?5)",
                params![operation, key, digest, token, completed_at],
            )
        };
    let digest = "a".repeat(64);

    insert_claimed("edition.create", "key-1", &digest, "token-1", None).unwrap();
    assert!(insert_claimed("edition.create", "key-1", &digest, "token-2", None).is_err());
    assert!(insert_claimed("changeset.commit", "key-2", &digest, "token-1", None).is_err());
    assert!(insert_claimed("edition.create", "key-3", "short", "token-3", None).is_err());
    assert!(insert_claimed(
        "edition.create",
        "key-4",
        &digest,
        "token-4",
        Some("2026-08-29T15:01:00Z")
    )
    .is_err());
    assert!(connection
        .execute(
            "INSERT INTO execution_replays (
                 operation, version, idempotency_key, input_digest, state, claim_token,
                 claimed_by, claimed_at, failed_at, failure
             ) VALUES ('edition.create', 'v1', 'key-5', ?1, 'failed', 'token-5',
                       'actor-1', '2026-08-29T15:00:00Z', '2026-08-29T15:01:00Z', '')",
            [&digest],
        )
        .is_err());
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
    rollback_to(&store.connection(), 11).unwrap();
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
        assert_eq!(handle.join().unwrap().unwrap(), 13);
    }

    let connection = Connection::open(&path).unwrap();
    let history_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version = 13",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(history_count, 1);
}
