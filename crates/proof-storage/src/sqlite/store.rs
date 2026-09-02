//! The SqliteStore type, its filter type, and the ExecutionStore implementation.

use super::migrations::run_migrations;
use crate::StorageError;
use chrono::{DateTime, Utc};
use proof_kernel::{
    AuditFilter, Delegation, ExecutionContext, ExecutionOutcome, ExecutionReplayClaim,
    ExecutionReplayClaimResult, ExecutionStore, Proof,
};
use rusqlite::Connection;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::{Mutex, MutexGuard};
use uuid::Uuid;

pub struct SqliteStore {
    // Field order is intentional: Rust drops the connection before the path
    // guards, so SQLite has finished all close-time filesystem work first.
    pub(super) conn: Mutex<Connection>,
    _trusted_path_guards: Option<TrustedSqlitePathGuards>,
    pub(super) operator: Option<OperatorStoreContext>,
}

pub(super) struct OperatorStoreContext {
    pub environment: Arc<dyn proof_kernel::OperatorControlEnvironment>,
    pub catalog: Arc<proof_kernel::OperatorSchemaCatalog>,
}

pub(super) struct TrustedSqlitePathGuards {
    _directory: File,
    _database: File,
}

/// Filters used by proof listing and counting queries.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ProofFilter {
    pub operation: Option<String>,
    pub version: Option<String>,
    pub actor: Option<String>,
}

impl ExecutionStore for SqliteStore {
    fn load_delegation(&self, delegation_id: &Uuid) -> Result<Option<Delegation>, String> {
        SqliteStore::load_delegation(self, delegation_id).map_err(|error| error.to_string())
    }

    fn save_proof(&self, proof: &Proof) -> Result<(), String> {
        SqliteStore::save_proof(self, proof).map_err(|error| error.to_string())
    }

    fn save_execution_context(&self, context: &ExecutionContext) -> Result<String, String> {
        SqliteStore::save_execution_context(self, context)
            .map(|context_id| context_id.to_string())
            .map_err(|error| error.to_string())
    }

    fn latest_proof_for_operation(
        &self,
        operation: &str,
        version: &str,
    ) -> Result<Option<Proof>, String> {
        self.list_proofs_for_operation_with_options(operation, Some(version), true)
            .map(|mut proofs| proofs.pop())
            .map_err(|error| error.to_string())
    }

    fn load_audit_contexts(&self, filter: &AuditFilter) -> Result<Vec<ExecutionContext>, String> {
        let mut sql = "
            SELECT execution_contexts.actor,
                   execution_contexts.delegation_id,
                   execution_contexts.workspace_path,
                   execution_contexts.timestamp
            FROM execution_contexts
            WHERE 1 = 1
        "
        .to_string();

        if filter.operation.is_some() {
            sql.push_str(
                " AND EXISTS (
                    SELECT 1 FROM proofs
                    WHERE proofs.actor = execution_contexts.actor
                      AND proofs.timestamp = execution_contexts.timestamp
                      AND substr(proofs.operation, 1, length(:operation_prefix))
                          = :operation_prefix
                )",
            );
        }
        if filter.actor.is_some() {
            sql.push_str(" AND execution_contexts.actor = :actor");
        }
        if filter.since.is_some() {
            sql.push_str(" AND execution_contexts.timestamp >= :since");
        }
        sql.push_str(&format!(
            " ORDER BY execution_contexts.timestamp DESC LIMIT {} OFFSET {}",
            filter.limit.clamp(1, 100),
            filter.offset
        ));

        let connection = self.conn.lock().map_err(|_| "storage lock poisoned")?;
        let mut statement = connection.prepare_cached(&sql).map_err(|e| e.to_string())?;

        let mut params: Vec<(&str, Box<dyn rusqlite::ToSql>)> = Vec::new();
        if let Some(operation) = &filter.operation {
            params.push((":operation_prefix", Box::new(format!("{operation}::"))));
        }
        if let Some(actor) = &filter.actor {
            params.push((":actor", Box::new(actor.as_uuid().to_string())));
        }
        if let Some(since) = &filter.since {
            params.push((":since", Box::new(since.to_rfc3339())));
        }
        let params: Vec<(&str, &dyn rusqlite::ToSql)> = params
            .iter()
            .map(|(name, value)| (*name, value.as_ref() as &dyn rusqlite::ToSql))
            .collect();

        let mut query = statement
            .query(params.as_slice())
            .map_err(|e| e.to_string())?;

        let mut contexts = Vec::new();
        while let Some(row) = query.next().map_err(|e| e.to_string())? {
            let actor: String = row.get("actor").map_err(|e| e.to_string())?;
            let actor_uuid = Uuid::parse_str(&actor).map_err(|e| e.to_string())?;
            let delegation_id: Option<String> =
                row.get("delegation_id").map_err(|e| e.to_string())?;
            let workspace_path: String = row.get("workspace_path").map_err(|e| e.to_string())?;
            let timestamp: String = row.get("timestamp").map_err(|e| e.to_string())?;
            contexts.push(ExecutionContext {
                actor: proof_kernel::PrincipalId::new(actor_uuid),
                principal_kind: None,
                delegation_id: delegation_id
                    .map(|id| Uuid::parse_str(&id))
                    .transpose()
                    .map_err(|e| e.to_string())?,
                delegation_chain: None,
                workspace_path: PathBuf::from(workspace_path),
                timestamp: DateTime::parse_from_rfc3339(&timestamp)
                    .map_err(|e| e.to_string())?
                    .with_timezone(&Utc),
            });
        }
        Ok(contexts)
    }

    fn claim_execution_replay(
        &self,
        claim: &ExecutionReplayClaim,
    ) -> Result<ExecutionReplayClaimResult, String> {
        SqliteStore::claim_execution_replay(self, claim).map_err(|error| error.to_string())
    }

    fn complete_execution_replay(
        &self,
        claim: &ExecutionReplayClaim,
        context: &ExecutionContext,
        outcome: &ExecutionOutcome,
    ) -> Result<(), String> {
        SqliteStore::complete_execution_replay(self, claim, context, outcome)
            .map_err(|error| error.to_string())
    }

    fn fail_execution_replay(
        &self,
        claim: &ExecutionReplayClaim,
        failed_at: DateTime<Utc>,
        failure: &str,
    ) -> Result<(), String> {
        SqliteStore::fail_execution_replay(self, claim, failed_at, failure)
            .map_err(|error| error.to_string())
    }
}

impl SqliteStore {
    /// Opens (or creates) a SQLite database at the given path and initializes the schema.
    pub fn open(path: &Path) -> Result<Self, StorageError> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;

            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .mode(0o600)
                .open(path)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        run_migrations(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
            _trusted_path_guards: None,
            operator: None,
        })
    }

    /// Opens an in-memory database (for testing).
    pub fn in_memory() -> Result<Self, StorageError> {
        let conn = Connection::open_in_memory()?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        run_migrations(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
            _trusted_path_guards: None,
            operator: None,
        })
    }

    pub(super) fn from_trusted_existing_connection(
        conn: Connection,
        directory: File,
        database: File,
    ) -> Self {
        Self {
            conn: Mutex::new(conn),
            _trusted_path_guards: Some(TrustedSqlitePathGuards {
                _directory: directory,
                _database: database,
            }),
            operator: None,
        }
    }

    pub(super) fn from_operator_existing_connection(
        conn: Connection,
        directory: File,
        database: File,
        environment: Arc<dyn proof_kernel::OperatorControlEnvironment>,
        catalog: Arc<proof_kernel::OperatorSchemaCatalog>,
    ) -> Self {
        Self {
            conn: Mutex::new(conn),
            _trusted_path_guards: Some(TrustedSqlitePathGuards {
                _directory: directory,
                _database: database,
            }),
            operator: Some(OperatorStoreContext {
                environment,
                catalog,
            }),
        }
    }

    /// Returns the underlying connection for queries not covered by higher-level APIs.
    pub fn connection(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap()
    }
}
