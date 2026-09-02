//! Schema migrations for the SQLite storage adapter.

use crate::StorageError;
use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};

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
    Migration {
        version: 4,
        description: "create commerce catalog, product, order, and order_line tables",
        up: "
            CREATE TABLE IF NOT EXISTS catalog (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS catalog_product (
                id TEXT PRIMARY KEY,
                catalog_id TEXT NOT NULL REFERENCES catalog(id),
                name TEXT NOT NULL,
                description TEXT,
                price_cents INTEGER,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS \"order\" (
                id TEXT PRIMARY KEY,
                status TEXT NOT NULL DEFAULT 'pending',
                created_at TEXT NOT NULL,
                approved_at TEXT,
                fulfilled_at TEXT
            );

            CREATE TABLE IF NOT EXISTS order_line (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                order_id TEXT NOT NULL REFERENCES \"order\"(id),
                catalog_id TEXT NOT NULL REFERENCES catalog(id),
                name TEXT NOT NULL,
                quantity INTEGER NOT NULL CHECK (quantity >= 1)
            );

            CREATE INDEX IF NOT EXISTS idx_catalog_product_catalog ON catalog_product(catalog_id);
            CREATE INDEX IF NOT EXISTS idx_order_line_order ON order_line(order_id);
            CREATE INDEX IF NOT EXISTS idx_order_status ON \"order\"(status);
            ",
        down: "
            DROP INDEX IF EXISTS idx_order_status;
            DROP INDEX IF EXISTS idx_order_line_order;
            DROP INDEX IF EXISTS idx_catalog_product_catalog;
            DROP TABLE IF EXISTS order_line;
            DROP TABLE IF EXISTS \"order\";
            DROP TABLE IF EXISTS catalog_product;
            DROP TABLE IF EXISTS catalog;
            ",
    },
    Migration {
        version: 5,
        description: "create workflow definition, run, and step tables",
        up: "
            CREATE TABLE IF NOT EXISTS workflow_definition (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                steps TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS workflow_run (
                id TEXT PRIMARY KEY,
                workflow_definition_id TEXT NOT NULL REFERENCES workflow_definition(id),
                status TEXT NOT NULL DEFAULT 'pending',
                created_at TEXT NOT NULL,
                completed_at TEXT,
                approved_at TEXT
            );

            CREATE TABLE IF NOT EXISTS workflow_step (
                id TEXT PRIMARY KEY,
                run_id TEXT NOT NULL REFERENCES workflow_run(id),
                name TEXT NOT NULL,
                kind TEXT NOT NULL CHECK (kind IN ('agent', 'human')),
                description TEXT NOT NULL DEFAULT '',
                status TEXT NOT NULL DEFAULT 'pending',
                ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
                completed_at TEXT
            );

            CREATE INDEX IF NOT EXISTS idx_workflow_run_definition
                ON workflow_run(workflow_definition_id);
            CREATE INDEX IF NOT EXISTS idx_workflow_step_run
                ON workflow_step(run_id);
            CREATE INDEX IF NOT EXISTS idx_workflow_run_status
                ON workflow_run(status);
            ",
        down: "
            DROP INDEX IF EXISTS idx_workflow_run_status;
            DROP INDEX IF EXISTS idx_workflow_step_run;
            DROP INDEX IF EXISTS idx_workflow_run_definition;
            DROP TABLE IF EXISTS workflow_step;
            DROP TABLE IF EXISTS workflow_run;
            DROP TABLE IF EXISTS workflow_definition;
            ",
    },
    Migration {
        version: 6,
        description: "create analytics snapshot, query, and insight tables",
        up: "
            CREATE TABLE IF NOT EXISTS analytics_snapshot (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                digest TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS analytics_query (
                id TEXT PRIMARY KEY,
                snapshot_id TEXT NOT NULL REFERENCES analytics_snapshot(id),
                name TEXT NOT NULL,
                filter TEXT NOT NULL DEFAULT '{}',
                aggregation TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS analytics_insight (
                id TEXT PRIMARY KEY,
                query_id TEXT NOT NULL REFERENCES analytics_query(id),
                result_digest TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'approved')),
                approved_at TEXT,
                approved_by TEXT
            );

            CREATE INDEX IF NOT EXISTS idx_analytics_query_snapshot
                ON analytics_query(snapshot_id);
            CREATE INDEX IF NOT EXISTS idx_analytics_insight_query
                ON analytics_insight(query_id);
            CREATE INDEX IF NOT EXISTS idx_analytics_insight_status
                ON analytics_insight(status);
            ",
        down: "
            DROP INDEX IF EXISTS idx_analytics_insight_status;
            DROP INDEX IF EXISTS idx_analytics_insight_query;
            DROP INDEX IF EXISTS idx_analytics_query_snapshot;
            DROP TABLE IF EXISTS analytics_insight;
            DROP TABLE IF EXISTS analytics_query;
            DROP TABLE IF EXISTS analytics_snapshot;
            ",
    },
    Migration {
        version: 7,
        description: "create signed approval request, decision, and execution tables",
        up: "
            CREATE TABLE IF NOT EXISTS approval_requests (
                id TEXT PRIMARY KEY,
                requested_by TEXT NOT NULL,
                operation TEXT NOT NULL,
                version TEXT NOT NULL,
                input_digest TEXT NOT NULL,
                requested_at TEXT NOT NULL,
                expires_at TEXT NOT NULL,
                request_json TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS approval_decisions (
                id TEXT PRIMARY KEY,
                request_id TEXT NOT NULL UNIQUE REFERENCES approval_requests(id),
                decided_by TEXT NOT NULL REFERENCES principals(id),
                outcome TEXT NOT NULL CHECK (outcome IN ('approved', 'denied')),
                decided_at TEXT NOT NULL,
                decision_json TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS approval_executions (
                request_id TEXT PRIMARY KEY REFERENCES approval_requests(id),
                executed_at TEXT NOT NULL,
                output_json TEXT NOT NULL,
                proof_json TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_approval_requests_requested_at
                ON approval_requests(requested_at);
            CREATE INDEX IF NOT EXISTS idx_approval_requests_operation
                ON approval_requests(operation, version);
            CREATE INDEX IF NOT EXISTS idx_approval_decisions_decided_by
                ON approval_decisions(decided_by);
            ",
        down: "
            DROP INDEX IF EXISTS idx_approval_decisions_decided_by;
            DROP INDEX IF EXISTS idx_approval_requests_operation;
            DROP INDEX IF EXISTS idx_approval_requests_requested_at;
            DROP TABLE IF EXISTS approval_executions;
            DROP TABLE IF EXISTS approval_decisions;
            DROP TABLE IF EXISTS approval_requests;
            ",
    },
    Migration {
        version: 8,
        description: "create durable agent run control-plane tables",
        up: "
            CREATE TABLE IF NOT EXISTS agent_runs (
                id TEXT PRIMARY KEY,
                actor TEXT NOT NULL,
                mode TEXT NOT NULL CHECK (mode IN ('one_shot', 'session')),
                status TEXT NOT NULL CHECK (
                    status IN ('queued', 'running', 'waiting_for_input', 'succeeded', 'failed', 'cancelled')
                ),
                revision INTEGER NOT NULL CHECK (revision >= 0),
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                run_json TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS agent_run_steps (
                id TEXT PRIMARY KEY,
                run_id TEXT NOT NULL REFERENCES agent_runs(id),
                ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
                attempt INTEGER NOT NULL CHECK (attempt >= 1),
                status TEXT NOT NULL CHECK (
                    status IN ('pending', 'running', 'waiting_for_approval', 'succeeded', 'failed', 'cancelled')
                ),
                approval_request_id TEXT REFERENCES approval_requests(id),
                revision INTEGER NOT NULL CHECK (revision >= 0),
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                step_json TEXT NOT NULL,
                UNIQUE (run_id, ordinal, attempt)
            );

            CREATE TABLE IF NOT EXISTS agent_checkpoints (
                id TEXT PRIMARY KEY,
                run_id TEXT NOT NULL REFERENCES agent_runs(id),
                sequence INTEGER NOT NULL CHECK (sequence >= 0),
                state_digest TEXT NOT NULL,
                created_at TEXT NOT NULL,
                checkpoint_json TEXT NOT NULL,
                UNIQUE (run_id, sequence)
            );

            CREATE TABLE IF NOT EXISTS agent_run_evaluations (
                id TEXT PRIMARY KEY,
                run_id TEXT NOT NULL REFERENCES agent_runs(id),
                evaluator TEXT NOT NULL,
                outcome TEXT NOT NULL CHECK (outcome IN ('passed', 'failed')),
                score_bps INTEGER CHECK (score_bps BETWEEN 0 AND 10000),
                created_at TEXT NOT NULL,
                evaluation_json TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_agent_runs_actor_status
                ON agent_runs(actor, status, updated_at);
            CREATE INDEX IF NOT EXISTS idx_agent_run_steps_run
                ON agent_run_steps(run_id, ordinal, attempt);
            CREATE INDEX IF NOT EXISTS idx_agent_run_steps_approval
                ON agent_run_steps(approval_request_id);
            CREATE INDEX IF NOT EXISTS idx_agent_checkpoints_run
                ON agent_checkpoints(run_id, sequence);
            CREATE INDEX IF NOT EXISTS idx_agent_run_evaluations_run
                ON agent_run_evaluations(run_id, created_at);
            ",
        down: "
            DROP INDEX IF EXISTS idx_agent_run_evaluations_run;
            DROP INDEX IF EXISTS idx_agent_checkpoints_run;
            DROP INDEX IF EXISTS idx_agent_run_steps_approval;
            DROP INDEX IF EXISTS idx_agent_run_steps_run;
            DROP INDEX IF EXISTS idx_agent_runs_actor_status;
            DROP TABLE IF EXISTS agent_run_evaluations;
            DROP TABLE IF EXISTS agent_checkpoints;
            DROP TABLE IF EXISTS agent_run_steps;
            DROP TABLE IF EXISTS agent_runs;
            ",
    },
    Migration {
        version: 9,
        description: "create agent definitions and append-only runtime events",
        up: "
            ALTER TABLE agent_runs ADD COLUMN agent_id TEXT;

            CREATE TABLE IF NOT EXISTS agent_definitions (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                provider TEXT NOT NULL,
                model TEXT NOT NULL,
                created_at TEXT NOT NULL,
                definition_json TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS agent_run_events (
                id TEXT PRIMARY KEY,
                run_id TEXT NOT NULL REFERENCES agent_runs(id),
                sequence INTEGER NOT NULL CHECK (sequence >= 0),
                kind TEXT NOT NULL,
                data_digest TEXT NOT NULL,
                created_at TEXT NOT NULL,
                event_json TEXT NOT NULL,
                UNIQUE (run_id, sequence)
            );

            CREATE INDEX IF NOT EXISTS idx_agent_runs_agent
                ON agent_runs(agent_id, created_at);
            CREATE INDEX IF NOT EXISTS idx_agent_run_events_run
                ON agent_run_events(run_id, sequence);
            ",
        down: "
            DROP INDEX IF EXISTS idx_agent_run_events_run;
            DROP INDEX IF EXISTS idx_agent_runs_agent;
            DROP TABLE IF EXISTS agent_run_events;
            DROP TABLE IF EXISTS agent_definitions;
            ALTER TABLE agent_runs DROP COLUMN agent_id;
            ",
    },
    Migration {
        version: 10,
        description: "enforce single-use approval bindings for agent run steps",
        up: "
            CREATE UNIQUE INDEX idx_agent_run_steps_approval_unique
                ON agent_run_steps(approval_request_id)
                WHERE approval_request_id IS NOT NULL;
            ",
        down: "
            DROP INDEX IF EXISTS idx_agent_run_steps_approval_unique;
            ",
    },
    Migration {
        version: 11,
        description: "create exact execution replay ledger",
        up: "
            CREATE TABLE execution_replays (
                operation TEXT NOT NULL,
                version TEXT NOT NULL,
                idempotency_key TEXT NOT NULL,
                input_digest TEXT NOT NULL CHECK (length(input_digest) = 64),
                state TEXT NOT NULL CHECK (state IN ('claimed', 'completed', 'failed')),
                claim_token TEXT NOT NULL UNIQUE,
                claimed_by TEXT NOT NULL,
                claimed_at TEXT NOT NULL,
                completed_at TEXT,
                failed_at TEXT,
                failure TEXT,
                output_json TEXT,
                proof_id TEXT UNIQUE REFERENCES proofs(id),
                proof_json TEXT,
                execution_context_id TEXT UNIQUE REFERENCES execution_contexts(id),
                PRIMARY KEY (operation, version, idempotency_key),
                CHECK (
                    (state = 'claimed'
                     AND completed_at IS NULL AND failed_at IS NULL AND failure IS NULL
                     AND output_json IS NULL AND proof_id IS NULL AND proof_json IS NULL
                     AND execution_context_id IS NULL)
                    OR
                    (state = 'completed'
                     AND completed_at IS NOT NULL AND failed_at IS NULL AND failure IS NULL
                     AND output_json IS NOT NULL AND proof_id IS NOT NULL
                     AND proof_json IS NOT NULL AND execution_context_id IS NOT NULL)
                    OR
                    (state = 'failed'
                     AND completed_at IS NULL AND failed_at IS NOT NULL
                     AND failure IS NOT NULL AND length(failure) > 0
                     AND output_json IS NULL AND proof_id IS NULL AND proof_json IS NULL
                     AND execution_context_id IS NULL)
                )
            );

            CREATE INDEX idx_execution_replays_state_claimed_at
                ON execution_replays(state, claimed_at);
            ",
        down: "
            DROP INDEX IF EXISTS idx_execution_replays_state_claimed_at;
            DROP TABLE IF EXISTS execution_replays;
            ",
    },
    Migration {
        version: 12,
        description: "persist structured delegation scope",
        up: "
            ALTER TABLE delegations
                ADD COLUMN scope_json TEXT NOT NULL DEFAULT '{}';
            ",
        down: "
            ALTER TABLE delegations DROP COLUMN scope_json;
            ",
    },
    Migration {
        version: 13,
        description: "claim live agent starts atomically",
        up: "
            CREATE TABLE live_run_start_claims (
                readiness_binding_digest TEXT PRIMARY KEY
                    CHECK (length(readiness_binding_digest) = 64),
                setup_digest TEXT NOT NULL UNIQUE
                    CHECK (length(setup_digest) = 64),
                schema TEXT NOT NULL
                    CHECK (schema = 'proof-live-run-start-claim/v1'),
                run_id TEXT NOT NULL UNIQUE REFERENCES agent_runs(id),
                initial_checkpoint_id TEXT NOT NULL UNIQUE REFERENCES agent_checkpoints(id),
                started_event_id TEXT NOT NULL UNIQUE REFERENCES agent_run_events(id),
                claimed_at TEXT NOT NULL,
                claim_json TEXT NOT NULL,
                initial_run_json TEXT NOT NULL
            );
            ",
        down: "
            DROP TABLE IF EXISTS live_run_start_claims;
            ",
    },
    Migration {
        version: 14,
        description:
            "create governed operator control, projection, fence, budget, command, and audit schema",
        up: include_str!("migration_14_up.sql"),
        down: include_str!("migration_14_down.sql"),
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
    run_migrations_through(conn, 13, TransactionBehavior::Immediate)
}

/// Applies pending migrations through `target_version` in one owned transaction.
///
/// Ordinary openers deliberately call [`run_migrations`], which stops at schema
/// 13. The guarded operator upgrader is the only product path that requests 14.
pub(super) fn run_migrations_through(
    conn: &Connection,
    target_version: u32,
    behavior: TransactionBehavior,
) -> Result<(), StorageError> {
    if target_version > 14
        || !MIGRATIONS
            .iter()
            .any(|migration| migration.version == target_version)
    {
        return Err(StorageError::Conflict(format!(
            "unknown migration target version: {target_version}"
        )));
    }
    ensure_migration_table(conn)?;
    let transaction = Transaction::new_unchecked(conn, behavior)?;
    let applied = schema_version(&transaction)?;
    if applied > target_version {
        transaction.commit()?;
        return Ok(());
    }
    for migration in MIGRATIONS {
        if migration.version <= applied || migration.version > target_version {
            continue;
        }
        if migration.version == 10 {
            reject_duplicate_agent_step_approvals(&transaction)?;
        }
        transaction.execute_batch(migration.up)?;
        transaction.execute(
            "INSERT INTO schema_migrations (version, description, applied_at)
             VALUES (?1, ?2, ?3)",
            rusqlite::params![
                migration.version,
                migration.description,
                Utc::now().to_rfc3339()
            ],
        )?;
    }
    transaction.commit()?;
    Ok(())
}

fn reject_duplicate_agent_step_approvals(conn: &Connection) -> Result<(), StorageError> {
    let duplicate = conn
        .query_row(
            "SELECT approval_request_id, COUNT(*)
             FROM agent_run_steps
             WHERE approval_request_id IS NOT NULL
             GROUP BY approval_request_id
             HAVING COUNT(*) > 1
             ORDER BY approval_request_id
             LIMIT 1",
            [],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()?;
    if let Some((approval_request_id, count)) = duplicate {
        return Err(StorageError::Conflict(format!(
            "cannot apply migration 10: approval request {approval_request_id} is bound to {count} agent run steps"
        )));
    }
    Ok(())
}

/// Rolls back migrations greater than `target_version`.
pub fn rollback_to(conn: &Connection, target_version: u32) -> Result<(), StorageError> {
    ensure_migration_table(conn)?;
    let transaction = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    let current_version = schema_version(&transaction)?;
    if target_version >= current_version {
        transaction.commit()?;
        return Ok(());
    }
    if current_version >= 14 && target_version < 14 {
        return Err(StorageError::Conflict(
            "public rollback cannot cross below operator schema 14".into(),
        ));
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
        transaction.execute_batch(migration.down)?;
        transaction.execute(
            "DELETE FROM schema_migrations WHERE version = ?1",
            [migration.version],
        )?;
    }
    transaction.commit()?;
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
    use rusqlite::OptionalExtension;

    #[test]
    fn ordinary_migration_path_stops_before_operator_schema() {
        let connection = Connection::open_in_memory().unwrap();

        run_migrations(&connection).unwrap();

        assert_eq!(schema_version(&connection).unwrap(), 13);
        let operator_table: Option<String> = connection
            .query_row(
                "SELECT name FROM sqlite_master
                 WHERE type='table' AND name='operator_workspaces'",
                [],
                |row| row.get(0),
            )
            .optional()
            .unwrap();
        assert_eq!(operator_table, None);
    }

    #[test]
    fn guarded_migration_14_target_upgrades_once_and_reopens_exactly() {
        let connection = Connection::open_in_memory().unwrap();
        run_migrations(&connection).unwrap();

        run_migrations_through(&connection, 14, TransactionBehavior::Exclusive).unwrap();
        run_migrations_through(&connection, 14, TransactionBehavior::Exclusive).unwrap();

        assert_eq!(schema_version(&connection).unwrap(), 14);
        let history: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE version=14",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(history, 1);
        connection
            .prepare("SELECT 1 FROM operator_workspaces")
            .unwrap();
    }

    #[test]
    fn public_rollback_rejects_crossing_operator_schema() {
        let connection = Connection::open_in_memory().unwrap();
        run_migrations_through(&connection, 14, TransactionBehavior::Exclusive).unwrap();

        let error = rollback_to(&connection, 13).unwrap_err();

        assert!(matches!(
            error,
            StorageError::Conflict(message)
                if message == "public rollback cannot cross below operator schema 14"
        ));
        assert_eq!(schema_version(&connection).unwrap(), 14);
    }

    #[test]
    fn disposable_operator_down_sql_drops_populated_recovery_graph_child_first() {
        let connection = Connection::open_in_memory().unwrap();
        run_migrations_through(&connection, 14, TransactionBehavior::Exclusive).unwrap();
        let digest = format!("blake3-256:{}", "a".repeat(64));
        connection
            .execute_batch(
                "PRAGMA foreign_keys=OFF;
                 PRAGMA ignore_check_constraints=ON;",
            )
            .unwrap();
        for (reservation_id, idempotency_key, recovery) in [
            (
                "00000000-0000-7000-8000-000000000001",
                "00000000-0000-7000-8000-000000000002",
                false,
            ),
            (
                "00000000-0000-7000-8000-000000000003",
                "00000000-0000-7000-8000-000000000004",
                true,
            ),
        ] {
            connection
                .execute(
                    "INSERT INTO operator_budget_reservations
                     (reservation_id, budget_id, run_id, lease_id, fence_epoch,
                      idempotency_key, request_digest, schema, kind, intent_digest,
                      intent_json, recovery_directive_id, recovery_directive_digest,
                      recovery_json, state, reserved_steps, reserved_tokens,
                      reserved_duration_ms, reserved_cost_microusd,
                      reserved_tool_dispatches, created_at, reservation_json)
                     VALUES (?1, '00000000-0000-7000-8000-000000000005',
                             '00000000-0000-7000-8000-000000000006',
                             '00000000-0000-7000-8000-000000000007', 1, ?2, ?3,
                             'proof-operator-budget-reservation/v1', 'tool', ?3, '{}',
                             CASE WHEN ?4 THEN '00000000-0000-7000-8000-000000000008' END,
                             CASE WHEN ?4 THEN ?3 END,
                             CASE WHEN ?4 THEN '{}' END,
                             'released', 1, 1, 1, 1, 1,
                             '2026-09-02T12:00:00+00:00', '{}')",
                    rusqlite::params![reservation_id, idempotency_key, digest, recovery],
                )
                .unwrap();
        }
        connection
            .execute(
                "INSERT INTO operator_recovery_directives
                 (directive_id, workspace_id, run_id, source_lease_id,
                  source_reservation_id, source_budget_id, source_idempotency_key,
                  source_request_digest, schema, classification, checkpoint_id,
                  checkpoint_sequence, checkpoint_digest, source_fence_epoch,
                  source_control_revision, intent_digest, required_budget_disposition,
                  created_at, directive_json, directive_digest)
                 VALUES ('00000000-0000-7000-8000-000000000008',
                         '00000000-0000-7000-8000-000000000009',
                         '00000000-0000-7000-8000-000000000006',
                         '00000000-0000-7000-8000-000000000007',
                         '00000000-0000-7000-8000-000000000001',
                         '00000000-0000-7000-8000-000000000005',
                         '00000000-0000-7000-8000-000000000002', ?1,
                         'proof.operator.recovery-directive/v1',
                         'pre_dispatch_recoverable',
                         '00000000-0000-7000-8000-000000000010', 0, ?2, 1, 0, ?1,
                         'none', '2026-09-02T12:00:00+00:00', '{}', ?1)",
                rusqlite::params![digest, "b".repeat(64)],
            )
            .unwrap();
        connection
            .execute_batch(
                "PRAGMA ignore_check_constraints=OFF;
                 PRAGMA foreign_keys=ON;",
            )
            .unwrap();

        connection
            .execute_batch(MIGRATIONS.last().unwrap().down)
            .unwrap();

        for table in [
            "operator_recovery_directives",
            "operator_budget_reservations",
            "operator_run_control",
            "operator_workspaces",
        ] {
            let remaining: Option<String> = connection
                .query_row(
                    "SELECT name FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |row| row.get(0),
                )
                .optional()
                .unwrap();
            assert_eq!(remaining, None, "operator table {table} survived down SQL");
        }
    }
}
