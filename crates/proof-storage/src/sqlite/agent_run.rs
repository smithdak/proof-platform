//! Durable agent run control-plane storage.

use proof_kernel::{
    AgentCheckpoint, AgentEvaluationOutcome, AgentRun, AgentRunEvaluation, AgentRunMode,
    AgentRunStatus, AgentRunStep, AgentRunStepStatus, AgentRunStore,
};
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};
use uuid::Uuid;

use super::{agent::reject_if_agent_trace_sealed, store::SqliteStore};
use crate::StorageError;

impl SqliteStore {
    /// Saves a new run or its next optimistic revision.
    pub fn save_agent_run(&self, run: &AgentRun) -> Result<(), StorageError> {
        let serialized = serde_json::to_string(run)?;
        let connection = self.conn.lock().unwrap();
        let transaction = Transaction::new_unchecked(&connection, TransactionBehavior::Immediate)?;
        let existing = transaction
            .query_row(
                "SELECT revision, run_json FROM agent_runs WHERE id = ?1",
                [run.id.to_string()],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;
        if existing
            .as_ref()
            .is_some_and(|(_, existing)| existing == &serialized)
        {
            transaction.commit()?;
            return Ok(());
        }
        reject_if_agent_trace_sealed(&transaction, &run.id, "modify the run")?;
        match existing {
            Some((revision, _)) if revision.checked_add(1) != Some(revision_i64(run.revision)?) => {
                return Err(StorageError::Conflict(format!(
                    "stale agent run revision: stored {revision}, supplied {}",
                    run.revision
                )));
            }
            None if run.revision != 0 => {
                return Err(StorageError::Conflict(format!(
                    "new agent run {} must start at revision 0",
                    run.id
                )));
            }
            _ => {}
        }
        let mode = match run.mode {
            AgentRunMode::OneShot => "one_shot",
            AgentRunMode::Session => "session",
        };
        transaction.execute(
            "INSERT INTO agent_runs (
                id, actor, agent_id, mode, status, revision, created_at, updated_at, run_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            ON CONFLICT(id) DO UPDATE SET
                actor = excluded.actor,
                agent_id = excluded.agent_id,
                mode = excluded.mode,
                status = excluded.status,
                revision = excluded.revision,
                updated_at = excluded.updated_at,
                run_json = excluded.run_json",
            params![
                run.id.to_string(),
                run.actor.to_string(),
                run.agent_id.map(|id| id.to_string()),
                mode,
                run_status(run.status),
                revision_i64(run.revision)?,
                run.created_at.to_rfc3339(),
                run.updated_at.to_rfc3339(),
                serialized,
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Loads an agent run by ID.
    pub fn load_agent_run(&self, run_id: &Uuid) -> Result<Option<AgentRun>, StorageError> {
        load_json(
            &self.conn.lock().unwrap(),
            "SELECT run_json FROM agent_runs WHERE id = ?1",
            run_id,
        )
    }

    /// Lists agent runs in creation order.
    pub fn list_agent_runs(&self) -> Result<Vec<AgentRun>, StorageError> {
        list_json(
            &self.conn.lock().unwrap(),
            "SELECT run_json FROM agent_runs ORDER BY created_at, id",
            [],
        )
    }

    /// Saves a new step attempt or its next optimistic revision.
    pub fn save_agent_run_step(&self, step: &AgentRunStep) -> Result<(), StorageError> {
        let serialized = serde_json::to_string(step)?;
        let connection = self.conn.lock().unwrap();
        let transaction = Transaction::new_unchecked(&connection, TransactionBehavior::Immediate)?;
        let existing = transaction
            .query_row(
                "SELECT revision, run_id, approval_request_id, step_json
                 FROM agent_run_steps WHERE id = ?1",
                [step.id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()?;
        let supplied_run_id = step.run_id.to_string();
        let supplied_approval_request_id = step.approval_request_id.map(|id| id.to_string());
        if let Some((_, stored_run_id, stored_approval_request_id, existing_json)) = &existing {
            if existing_json == &serialized {
                if stored_run_id != &supplied_run_id {
                    return Err(StorageError::Conflict(format!(
                        "agent run step {} run binding column does not match its serialized record",
                        step.id
                    )));
                }
                if stored_approval_request_id != &supplied_approval_request_id {
                    return Err(StorageError::Conflict(format!(
                        "agent run step {} approval binding column does not match its serialized record",
                        step.id
                    )));
                }
                transaction.commit()?;
                return Ok(());
            }
            if stored_run_id != &supplied_run_id {
                return Err(StorageError::Conflict(format!(
                    "agent run step {} run binding is immutable",
                    step.id
                )));
            }
            if stored_approval_request_id.is_some()
                && stored_approval_request_id != &supplied_approval_request_id
            {
                return Err(StorageError::Conflict(format!(
                    "agent run step {} approval binding is immutable once assigned",
                    step.id
                )));
            }
        }
        reject_if_agent_trace_sealed(&transaction, &step.run_id, "modify a run step")?;
        if let Some(approval_request_id) = step.approval_request_id {
            let conflicting_step_id = transaction
                .query_row(
                    "SELECT id FROM agent_run_steps
                     WHERE approval_request_id = ?1 AND id != ?2
                     LIMIT 1",
                    params![approval_request_id.to_string(), step.id.to_string()],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            if let Some(conflicting_step_id) = conflicting_step_id {
                return Err(StorageError::Conflict(format!(
                    "approval request {approval_request_id} is already bound to agent run step {conflicting_step_id}"
                )));
            }
        }
        match existing {
            Some((revision, _, _, _))
                if revision.checked_add(1) != Some(revision_i64(step.revision)?) =>
            {
                return Err(StorageError::Conflict(format!(
                    "stale agent run step revision: stored {revision}, supplied {}",
                    step.revision
                )));
            }
            None if step.revision != 0 => {
                return Err(StorageError::Conflict(format!(
                    "new agent run step {} must start at revision 0",
                    step.id
                )));
            }
            _ => {}
        }
        transaction.execute(
            "INSERT INTO agent_run_steps (
                id, run_id, ordinal, attempt, status, approval_request_id,
                revision, created_at, updated_at, step_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ON CONFLICT(id) DO UPDATE SET
                status = excluded.status,
                approval_request_id = excluded.approval_request_id,
                revision = excluded.revision,
                updated_at = excluded.updated_at,
                step_json = excluded.step_json",
            params![
                step.id.to_string(),
                step.run_id.to_string(),
                i64::from(step.ordinal),
                i64::from(step.attempt),
                step_status(step.status),
                step.approval_request_id.map(|id| id.to_string()),
                revision_i64(step.revision)?,
                step.created_at.to_rfc3339(),
                step.updated_at.to_rfc3339(),
                serialized,
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Loads an agent run step by ID.
    pub fn load_agent_run_step(
        &self,
        step_id: &Uuid,
    ) -> Result<Option<AgentRunStep>, StorageError> {
        load_json(
            &self.conn.lock().unwrap(),
            "SELECT step_json FROM agent_run_steps WHERE id = ?1",
            step_id,
        )
    }

    /// Lists every attempt for an agent run in logical order.
    pub fn list_agent_run_steps(&self, run_id: &Uuid) -> Result<Vec<AgentRunStep>, StorageError> {
        list_json(
            &self.conn.lock().unwrap(),
            "SELECT step_json FROM agent_run_steps
             WHERE run_id = ?1 ORDER BY ordinal, attempt",
            [run_id.to_string()],
        )
    }

    /// Finds the step suspended on a signed approval request.
    pub fn find_agent_run_step_by_approval(
        &self,
        approval_request_id: &Uuid,
    ) -> Result<Option<AgentRunStep>, StorageError> {
        let connection = self.conn.lock().unwrap();
        let mut statement = connection.prepare_cached(
            "SELECT step_json FROM agent_run_steps
             WHERE approval_request_id = ?1
             ORDER BY id
             LIMIT 2",
        )?;
        let serialized = statement
            .query_map([approval_request_id.to_string()], |row| {
                row.get::<_, String>(0)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        if serialized.len() > 1 {
            return Err(StorageError::Conflict(format!(
                "approval request {approval_request_id} is bound to multiple agent run steps"
            )));
        }
        let Some(serialized) = serialized.into_iter().next() else {
            return Ok(None);
        };
        let step: AgentRunStep = serde_json::from_str(&serialized).map_err(|error| {
            StorageError::Conflict(format!(
                "agent run step for approval request {approval_request_id} contains invalid serialized data: {error}"
            ))
        })?;
        if step.approval_request_id != Some(*approval_request_id) {
            return Err(StorageError::Conflict(format!(
                "agent run step {} approval binding does not match indexed request {approval_request_id}",
                step.id
            )));
        }
        Ok(Some(step))
    }

    /// Appends an immutable checkpoint to an agent run.
    pub fn save_agent_checkpoint(&self, checkpoint: &AgentCheckpoint) -> Result<(), StorageError> {
        let serialized = serde_json::to_string(checkpoint)?;
        let connection = self.conn.lock().unwrap();
        let transaction = Transaction::new_unchecked(&connection, TransactionBehavior::Immediate)?;
        let existing = transaction
            .query_row(
                "SELECT checkpoint_json FROM agent_checkpoints
                 WHERE id = ?1 OR (run_id = ?2 AND sequence = ?3)
                 LIMIT 1",
                params![
                    checkpoint.id.to_string(),
                    checkpoint.run_id.to_string(),
                    i64::from(checkpoint.sequence),
                ],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        if let Some(existing) = existing {
            if existing != serialized {
                return Err(StorageError::Conflict(format!(
                    "conflicting checkpoint {} for agent run {}",
                    checkpoint.sequence, checkpoint.run_id
                )));
            }
            transaction.commit()?;
            return Ok(());
        }
        reject_if_agent_trace_sealed(&transaction, &checkpoint.run_id, "append a checkpoint")?;
        transaction.execute(
            "INSERT INTO agent_checkpoints (
                id, run_id, sequence, state_digest, created_at, checkpoint_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                checkpoint.id.to_string(),
                checkpoint.run_id.to_string(),
                i64::from(checkpoint.sequence),
                checkpoint.state_digest.hex(),
                checkpoint.created_at.to_rfc3339(),
                serialized,
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Lists immutable checkpoints in sequence order.
    pub fn list_agent_checkpoints(
        &self,
        run_id: &Uuid,
    ) -> Result<Vec<AgentCheckpoint>, StorageError> {
        list_json(
            &self.conn.lock().unwrap(),
            "SELECT checkpoint_json FROM agent_checkpoints
             WHERE run_id = ?1 ORDER BY sequence",
            [run_id.to_string()],
        )
    }

    /// Appends an immutable evaluation for a terminal agent run.
    pub fn save_agent_run_evaluation(
        &self,
        evaluation: &AgentRunEvaluation,
    ) -> Result<(), StorageError> {
        let serialized = serde_json::to_string(evaluation)?;
        let outcome = match evaluation.outcome {
            AgentEvaluationOutcome::Passed => "passed",
            AgentEvaluationOutcome::Failed => "failed",
        };
        let connection = self.conn.lock().unwrap();
        connection.execute(
            "INSERT OR IGNORE INTO agent_run_evaluations (
                id, run_id, evaluator, outcome, score_bps, created_at, evaluation_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                evaluation.id.to_string(),
                evaluation.run_id.to_string(),
                evaluation.evaluator,
                outcome,
                evaluation.score_bps.map(i64::from),
                evaluation.created_at.to_rfc3339(),
                serialized,
            ],
        )?;
        let existing: String = connection.query_row(
            "SELECT evaluation_json FROM agent_run_evaluations WHERE id = ?1",
            [evaluation.id.to_string()],
            |row| row.get(0),
        )?;
        if existing != serialized {
            return Err(StorageError::Conflict(format!(
                "conflicting agent run evaluation: {}",
                evaluation.id
            )));
        }
        Ok(())
    }

    /// Lists evaluations in creation order.
    pub fn list_agent_run_evaluations(
        &self,
        run_id: &Uuid,
    ) -> Result<Vec<AgentRunEvaluation>, StorageError> {
        list_json(
            &self.conn.lock().unwrap(),
            "SELECT evaluation_json FROM agent_run_evaluations
             WHERE run_id = ?1 ORDER BY created_at, id",
            [run_id.to_string()],
        )
    }
}

impl AgentRunStore for SqliteStore {
    fn save_agent_run(&self, run: &AgentRun) -> Result<(), String> {
        SqliteStore::save_agent_run(self, run).map_err(|error| error.to_string())
    }

    fn load_agent_run(&self, run_id: &Uuid) -> Result<Option<AgentRun>, String> {
        SqliteStore::load_agent_run(self, run_id).map_err(|error| error.to_string())
    }

    fn list_agent_runs(&self) -> Result<Vec<AgentRun>, String> {
        SqliteStore::list_agent_runs(self).map_err(|error| error.to_string())
    }

    fn save_agent_run_step(&self, step: &AgentRunStep) -> Result<(), String> {
        SqliteStore::save_agent_run_step(self, step).map_err(|error| error.to_string())
    }

    fn load_agent_run_step(&self, step_id: &Uuid) -> Result<Option<AgentRunStep>, String> {
        SqliteStore::load_agent_run_step(self, step_id).map_err(|error| error.to_string())
    }

    fn list_agent_run_steps(&self, run_id: &Uuid) -> Result<Vec<AgentRunStep>, String> {
        SqliteStore::list_agent_run_steps(self, run_id).map_err(|error| error.to_string())
    }

    fn find_agent_run_step_by_approval(
        &self,
        approval_request_id: &Uuid,
    ) -> Result<Option<AgentRunStep>, String> {
        SqliteStore::find_agent_run_step_by_approval(self, approval_request_id)
            .map_err(|error| error.to_string())
    }

    fn save_agent_checkpoint(&self, checkpoint: &AgentCheckpoint) -> Result<(), String> {
        SqliteStore::save_agent_checkpoint(self, checkpoint).map_err(|error| error.to_string())
    }

    fn list_agent_checkpoints(&self, run_id: &Uuid) -> Result<Vec<AgentCheckpoint>, String> {
        SqliteStore::list_agent_checkpoints(self, run_id).map_err(|error| error.to_string())
    }

    fn save_agent_run_evaluation(&self, evaluation: &AgentRunEvaluation) -> Result<(), String> {
        SqliteStore::save_agent_run_evaluation(self, evaluation).map_err(|error| error.to_string())
    }

    fn list_agent_run_evaluations(&self, run_id: &Uuid) -> Result<Vec<AgentRunEvaluation>, String> {
        SqliteStore::list_agent_run_evaluations(self, run_id).map_err(|error| error.to_string())
    }
}

fn revision_i64(revision: u64) -> Result<i64, StorageError> {
    revision
        .try_into()
        .map_err(|_| StorageError::Conflict("agent run revision exceeds SQLite range".to_string()))
}

fn run_status(status: AgentRunStatus) -> &'static str {
    match status {
        AgentRunStatus::Queued => "queued",
        AgentRunStatus::Running => "running",
        AgentRunStatus::WaitingForInput => "waiting_for_input",
        AgentRunStatus::Succeeded => "succeeded",
        AgentRunStatus::Failed => "failed",
        AgentRunStatus::Cancelled => "cancelled",
    }
}

fn step_status(status: AgentRunStepStatus) -> &'static str {
    match status {
        AgentRunStepStatus::Pending => "pending",
        AgentRunStepStatus::Running => "running",
        AgentRunStepStatus::WaitingForApproval => "waiting_for_approval",
        AgentRunStepStatus::Succeeded => "succeeded",
        AgentRunStepStatus::Failed => "failed",
        AgentRunStepStatus::Cancelled => "cancelled",
    }
}

fn load_json<T: serde::de::DeserializeOwned>(
    connection: &rusqlite::Connection,
    query: &str,
    id: &Uuid,
) -> Result<Option<T>, StorageError> {
    connection
        .query_row(query, [id.to_string()], |row| row.get::<_, String>(0))
        .optional()?
        .map(|serialized| serde_json::from_str(&serialized).map_err(StorageError::from))
        .transpose()
}

fn list_json<T, P>(
    connection: &rusqlite::Connection,
    query: &str,
    parameters: P,
) -> Result<Vec<T>, StorageError>
where
    T: serde::de::DeserializeOwned,
    P: rusqlite::Params,
{
    let mut statement = connection.prepare_cached(query)?;
    let rows = statement.query_map(parameters, |row| row.get::<_, String>(0))?;
    rows.map(|row| {
        let serialized = row?;
        serde_json::from_str(&serialized).map_err(StorageError::from)
    })
    .collect()
}
