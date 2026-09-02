//! Durable storage for signed approval workflows.

use proof_kernel::{
    ApprovalExecution, ApprovalOutcome, ApprovalStore, Principal, PrincipalId,
    SignedApprovalDecision, SignedApprovalRequest,
};
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};
use uuid::Uuid;

use super::{agent::reject_if_approval_is_bound_to_sealed_trace, store::SqliteStore};
use crate::StorageError;

impl SqliteStore {
    /// Saves an immutable signed approval request.
    pub fn save_approval_request(
        &self,
        request: &SignedApprovalRequest,
    ) -> Result<(), StorageError> {
        let serialized = serde_json::to_string(request)?;
        let connection = self.conn.lock().unwrap();
        let transaction = Transaction::new_unchecked(&connection, TransactionBehavior::Immediate)?;
        let existing = transaction
            .query_row(
                "SELECT request_json FROM approval_requests WHERE id = ?1",
                [request.body.id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        if let Some(existing) = existing {
            if existing != serialized {
                return Err(StorageError::Conflict(format!(
                    "approval request {} already exists with different content",
                    request.body.id
                )));
            }
            transaction.commit()?;
            return Ok(());
        }
        reject_if_operator_governed_approval(
            &transaction,
            &request.body.id,
            "insert approval request evidence",
        )?;
        reject_if_approval_is_bound_to_sealed_trace(
            &transaction,
            &request.body.id,
            "insert approval request evidence",
        )?;
        transaction.execute(
            "INSERT INTO approval_requests (
                id, requested_by, operation, version, input_digest,
                requested_at, expires_at, request_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                request.body.id.to_string(),
                request.body.requested_by.to_string(),
                request.body.operation,
                request.body.version,
                request.body.input_digest.hex(),
                request.body.requested_at.to_rfc3339(),
                request.body.expires_at.to_rfc3339(),
                serialized,
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Loads a signed approval request by request ID.
    pub fn load_approval_request(
        &self,
        request_id: &Uuid,
    ) -> Result<Option<SignedApprovalRequest>, StorageError> {
        self.conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT request_json FROM approval_requests WHERE id = ?1",
                [request_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(|serialized| serde_json::from_str(&serialized).map_err(StorageError::from))
            .transpose()
    }

    /// Lists signed approval requests in creation order.
    pub fn list_approval_requests(&self) -> Result<Vec<SignedApprovalRequest>, StorageError> {
        let connection = self.conn.lock().unwrap();
        let mut statement = connection.prepare_cached(
            "SELECT request_json FROM approval_requests ORDER BY requested_at, id",
        )?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        rows.map(|row| {
            let serialized = row?;
            serde_json::from_str(&serialized).map_err(StorageError::from)
        })
        .collect()
    }

    /// Saves one immutable signed decision for an approval request.
    pub fn save_approval_decision(
        &self,
        decision: &SignedApprovalDecision,
    ) -> Result<(), StorageError> {
        let serialized = serde_json::to_string(decision)?;
        let outcome = match decision.body.outcome {
            ApprovalOutcome::Approved => "approved",
            ApprovalOutcome::Denied => "denied",
        };
        let connection = self.conn.lock().unwrap();
        let transaction = Transaction::new_unchecked(&connection, TransactionBehavior::Immediate)?;
        let existing = transaction
            .query_row(
                "SELECT decision_json FROM approval_decisions
                 WHERE id = ?1 OR request_id = ?2
                 LIMIT 1",
                params![
                    decision.body.id.to_string(),
                    decision.body.request_id.to_string()
                ],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        if let Some(existing) = existing {
            if existing != serialized {
                return Err(StorageError::Conflict(format!(
                    "approval request {} already has a different decision",
                    decision.body.request_id
                )));
            }
            transaction.commit()?;
            return Ok(());
        }
        reject_if_operator_governed_approval(
            &transaction,
            &decision.body.request_id,
            "insert approval decision evidence",
        )?;
        reject_if_approval_is_bound_to_sealed_trace(
            &transaction,
            &decision.body.request_id,
            "insert approval decision evidence",
        )?;
        transaction.execute(
            "INSERT INTO approval_decisions (
                id, request_id, decided_by, outcome, decided_at, decision_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                decision.body.id.to_string(),
                decision.body.request_id.to_string(),
                decision.body.decided_by.to_string(),
                outcome,
                decision.body.decided_at.to_rfc3339(),
                serialized,
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Loads the signed decision for an approval request.
    pub fn load_approval_decision(
        &self,
        request_id: &Uuid,
    ) -> Result<Option<SignedApprovalDecision>, StorageError> {
        self.conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT decision_json FROM approval_decisions WHERE request_id = ?1",
                [request_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(|serialized| serde_json::from_str(&serialized).map_err(StorageError::from))
            .transpose()
    }

    /// Saves the immutable result of an approved execution for replay.
    pub fn save_approval_execution(
        &self,
        execution: &ApprovalExecution,
    ) -> Result<(), StorageError> {
        let output = serde_json::to_string(&execution.output)?;
        let proof = serde_json::to_string(&execution.proof)?;
        let connection = self.conn.lock().unwrap();
        let transaction = Transaction::new_unchecked(&connection, TransactionBehavior::Immediate)?;
        let existing = transaction
            .query_row(
                "SELECT executed_at, output_json, proof_json
                 FROM approval_executions WHERE request_id = ?1",
                [execution.request_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?;
        let supplied = (execution.executed_at.to_rfc3339(), output, proof);
        if let Some(existing) = existing {
            if existing != supplied {
                return Err(StorageError::Conflict(format!(
                    "approval request {} already has a different execution",
                    execution.request_id
                )));
            }
            transaction.commit()?;
            return Ok(());
        }
        reject_if_operator_governed_approval(
            &transaction,
            &execution.request_id,
            "insert approval execution evidence",
        )?;
        reject_if_approval_is_bound_to_sealed_trace(
            &transaction,
            &execution.request_id,
            "insert approval execution evidence",
        )?;
        transaction.execute(
            "INSERT INTO approval_executions (
                request_id, executed_at, output_json, proof_json
            ) VALUES (?1, ?2, ?3, ?4)",
            params![
                execution.request_id.to_string(),
                supplied.0,
                supplied.1,
                supplied.2,
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Loads the immutable execution result for an approval request.
    pub fn load_approval_execution(
        &self,
        request_id: &Uuid,
    ) -> Result<Option<ApprovalExecution>, StorageError> {
        self.conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT executed_at, output_json, proof_json
                 FROM approval_executions WHERE request_id = ?1",
                [request_id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?
            .map(|(executed_at, output, proof)| {
                Ok(ApprovalExecution {
                    request_id: *request_id,
                    executed_at: chrono::DateTime::parse_from_rfc3339(&executed_at)
                        .map_err(|error| {
                            StorageError::Conflict(format!(
                                "invalid approval execution timestamp: {error}"
                            ))
                        })?
                        .with_timezone(&chrono::Utc),
                    output: serde_json::from_str(&output)?,
                    proof: serde_json::from_str(&proof)?,
                })
            })
            .transpose()
    }
}

fn reject_if_operator_governed_approval(
    transaction: &Transaction<'_>,
    request_id: &Uuid,
    action: &str,
) -> Result<(), StorageError> {
    if super::migrations::schema_version(transaction)? < 14 {
        return Ok(());
    }
    let governed: i64 = transaction.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM operator_approval_bindings WHERE approval_request_id = ?1
             UNION ALL
             SELECT 1 FROM agent_run_steps s
             JOIN operator_run_control c ON c.run_id = s.run_id
             WHERE s.approval_request_id = ?1
         )",
        [request_id.to_string()],
        |row| row.get(0),
    )?;
    if governed != 0 {
        return Err(StorageError::Conflict(format!(
            "operator-governed approval {request_id} may not use the legacy storage path to {action}"
        )));
    }
    Ok(())
}

impl ApprovalStore for SqliteStore {
    fn save_approval_request(&self, request: &SignedApprovalRequest) -> Result<(), String> {
        SqliteStore::save_approval_request(self, request).map_err(|error| error.to_string())
    }

    fn load_approval_request(
        &self,
        request_id: &Uuid,
    ) -> Result<Option<SignedApprovalRequest>, String> {
        SqliteStore::load_approval_request(self, request_id).map_err(|error| error.to_string())
    }

    fn list_approval_requests(&self) -> Result<Vec<SignedApprovalRequest>, String> {
        SqliteStore::list_approval_requests(self).map_err(|error| error.to_string())
    }

    fn save_approval_decision(&self, decision: &SignedApprovalDecision) -> Result<(), String> {
        SqliteStore::save_approval_decision(self, decision).map_err(|error| error.to_string())
    }

    fn load_approval_decision(
        &self,
        request_id: &Uuid,
    ) -> Result<Option<SignedApprovalDecision>, String> {
        SqliteStore::load_approval_decision(self, request_id).map_err(|error| error.to_string())
    }

    fn save_approval_execution(&self, execution: &ApprovalExecution) -> Result<(), String> {
        SqliteStore::save_approval_execution(self, execution).map_err(|error| error.to_string())
    }

    fn load_approval_execution(
        &self,
        request_id: &Uuid,
    ) -> Result<Option<ApprovalExecution>, String> {
        SqliteStore::load_approval_execution(self, request_id).map_err(|error| error.to_string())
    }

    fn load_trusted_approver(&self, approver: &PrincipalId) -> Result<Option<Principal>, String> {
        match self.load_principal(approver) {
            Ok(principal) => Ok(Some(principal)),
            Err(StorageError::NotFound(_)) => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }
}
