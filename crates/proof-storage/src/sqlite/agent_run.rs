//! Durable agent run control-plane storage.

use proof_kernel::{
    AgentCheckpoint, AgentEvaluationOutcome, AgentRun, AgentRunEvaluation, AgentRunEvent,
    AgentRunMode, AgentRunStatus, AgentRunStep, AgentRunStepStatus, AgentRunStore,
    LiveRunStartClaim, LiveRunStartClaimResult,
};
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};
use uuid::Uuid;

use super::{agent::reject_if_agent_trace_sealed, store::SqliteStore};
use crate::StorageError;

impl SqliteStore {
    /// Atomically claims a paid live start together with its complete initial
    /// run/checkpoint/event barrier.
    pub fn claim_live_run_start(
        &self,
        claim: &LiveRunStartClaim,
        initial_run: &AgentRun,
        initial_checkpoint: &AgentCheckpoint,
        started_event: &AgentRunEvent,
    ) -> Result<LiveRunStartClaimResult, StorageError> {
        claim
            .validate_initial_bundle(initial_run, initial_checkpoint, started_event)
            .map_err(|error| StorageError::Conflict(error.to_string()))?;

        let connection = self.conn.lock().unwrap();
        let transaction = Transaction::new_unchecked(&connection, TransactionBehavior::Immediate)?;
        let mut statement = transaction.prepare_cached(
            "SELECT readiness_binding_digest, setup_digest, schema, run_id,
                    initial_checkpoint_id, started_event_id, claimed_at,
                    claim_json, initial_run_json
             FROM live_run_start_claims
             WHERE readiness_binding_digest = ?1 OR setup_digest = ?2
             ORDER BY readiness_binding_digest",
        )?;
        let matching = statement
            .query_map(
                params![
                    claim.readiness_binding_digest.hex(),
                    claim.setup_digest.hex()
                ],
                |row| {
                    Ok(StoredLiveRunStartClaim {
                        readiness_binding_digest: row.get(0)?,
                        setup_digest: row.get(1)?,
                        schema: row.get(2)?,
                        run_id: row.get(3)?,
                        initial_checkpoint_id: row.get(4)?,
                        started_event_id: row.get(5)?,
                        claimed_at: row.get(6)?,
                        claim_json: row.get(7)?,
                        initial_run_json: row.get(8)?,
                    })
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        if !matching.is_empty() {
            if matching.len() != 1
                || matching[0].readiness_binding_digest != claim.readiness_binding_digest.hex()
                || matching[0].setup_digest != claim.setup_digest.hex()
            {
                transaction.commit()?;
                return Ok(LiveRunStartClaimResult::Conflict);
            }
            let run_id = validate_existing_live_start_claim(&transaction, claim, &matching[0])?;
            transaction.commit()?;
            return Ok(LiveRunStartClaimResult::Existing(run_id));
        }

        let run_json = serde_json::to_string(initial_run)?;
        let checkpoint_json = serde_json::to_string(initial_checkpoint)?;
        let event_json = serde_json::to_string(started_event)?;
        let claim_json = serde_json::to_string(claim)?;
        let agent_id = initial_run.agent_id.ok_or_else(|| {
            StorageError::Conflict("initial live run has no agent ID".to_string())
        })?;
        transaction.execute(
            "INSERT INTO agent_runs (
                id, actor, agent_id, mode, status, revision, created_at, updated_at, run_json
             ) VALUES (?1, ?2, ?3, 'session', 'running', ?4, ?5, ?6, ?7)",
            params![
                initial_run.id.to_string(),
                initial_run.actor.to_string(),
                agent_id.to_string(),
                revision_i64(initial_run.revision)?,
                initial_run.created_at.to_rfc3339(),
                initial_run.updated_at.to_rfc3339(),
                run_json,
            ],
        )?;
        transaction.execute(
            "INSERT INTO agent_checkpoints (
                id, run_id, sequence, state_digest, created_at, checkpoint_json
             ) VALUES (?1, ?2, 0, ?3, ?4, ?5)",
            params![
                initial_checkpoint.id.to_string(),
                initial_run.id.to_string(),
                initial_checkpoint.state_digest.hex(),
                initial_checkpoint.created_at.to_rfc3339(),
                checkpoint_json,
            ],
        )?;
        transaction.execute(
            "INSERT INTO agent_run_events (
                id, run_id, sequence, kind, data_digest, created_at, event_json
             ) VALUES (?1, ?2, 0, 'started', ?3, ?4, ?5)",
            params![
                started_event.id.to_string(),
                initial_run.id.to_string(),
                started_event.data_digest.hex(),
                started_event.created_at.to_rfc3339(),
                event_json,
            ],
        )?;
        transaction.execute(
            "INSERT INTO live_run_start_claims (
                readiness_binding_digest, setup_digest, schema, run_id,
                initial_checkpoint_id, started_event_id, claimed_at,
                claim_json, initial_run_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                claim.readiness_binding_digest.hex(),
                claim.setup_digest.hex(),
                claim.schema,
                initial_run.id.to_string(),
                initial_checkpoint.id.to_string(),
                started_event.id.to_string(),
                initial_run.created_at.to_rfc3339(),
                claim_json,
                serde_json::to_string(initial_run)?,
            ],
        )?;
        transaction.commit()?;
        Ok(LiveRunStartClaimResult::Acquired)
    }

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

struct StoredLiveRunStartClaim {
    readiness_binding_digest: String,
    setup_digest: String,
    schema: String,
    run_id: String,
    initial_checkpoint_id: String,
    started_event_id: String,
    claimed_at: String,
    claim_json: String,
    initial_run_json: String,
}

fn validate_existing_live_start_claim(
    transaction: &Transaction<'_>,
    expected_claim: &LiveRunStartClaim,
    stored: &StoredLiveRunStartClaim,
) -> Result<Uuid, StorageError> {
    let claim: LiveRunStartClaim = serde_json::from_str(&stored.claim_json).map_err(|error| {
        StorageError::Conflict(format!("stored live start claim is malformed: {error}"))
    })?;
    let initial_run: AgentRun =
        serde_json::from_str(&stored.initial_run_json).map_err(|error| {
            StorageError::Conflict(format!("stored initial live run is malformed: {error}"))
        })?;
    let run_id = Uuid::parse_str(&stored.run_id).map_err(|error| {
        StorageError::Conflict(format!("stored live start run ID is malformed: {error}"))
    })?;
    let checkpoint_id = Uuid::parse_str(&stored.initial_checkpoint_id).map_err(|error| {
        StorageError::Conflict(format!(
            "stored live start checkpoint ID is malformed: {error}"
        ))
    })?;
    let event_id = Uuid::parse_str(&stored.started_event_id).map_err(|error| {
        StorageError::Conflict(format!("stored live start event ID is malformed: {error}"))
    })?;
    if claim != *expected_claim
        || stored.schema != claim.schema
        || stored.readiness_binding_digest != claim.readiness_binding_digest.hex()
        || stored.setup_digest != claim.setup_digest.hex()
        || initial_run.id != run_id
        || stored.claimed_at != initial_run.created_at.to_rfc3339()
        || serde_json::to_string(&claim)? != stored.claim_json
        || serde_json::to_string(&initial_run)? != stored.initial_run_json
    {
        return Err(StorageError::Conflict(
            "stored live start claim columns or immutable JSON drifted".to_string(),
        ));
    }

    let (checkpoint_json, checkpoint_digest, checkpoint_created_at) = transaction
        .query_row(
            "SELECT checkpoint_json, state_digest, created_at FROM agent_checkpoints
             WHERE id = ?1 AND run_id = ?2 AND sequence = 0",
            params![checkpoint_id.to_string(), run_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| {
            StorageError::Conflict("claimed live start checkpoint is missing".to_string())
        })?;
    let (event_json, event_digest, event_created_at) = transaction
        .query_row(
            "SELECT event_json, data_digest, created_at FROM agent_run_events
             WHERE id = ?1 AND run_id = ?2 AND sequence = 0 AND kind = 'started'",
            params![event_id.to_string(), run_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| StorageError::Conflict("claimed live start event is missing".to_string()))?;
    let (
        run_actor,
        run_agent_id,
        run_mode,
        indexed_run_status,
        run_revision,
        run_created_at,
        run_updated_at,
        current_run_json,
    ) = transaction
        .query_row(
            "SELECT actor, agent_id, mode, status, revision, created_at, updated_at, run_json
             FROM agent_runs WHERE id = ?1",
            [run_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| StorageError::Conflict("claimed live run is missing".to_string()))?;
    let checkpoint: AgentCheckpoint = serde_json::from_str(&checkpoint_json).map_err(|error| {
        StorageError::Conflict(format!(
            "claimed live start checkpoint is malformed: {error}"
        ))
    })?;
    let event: AgentRunEvent = serde_json::from_str(&event_json).map_err(|error| {
        StorageError::Conflict(format!("claimed live start event is malformed: {error}"))
    })?;
    let current_run: AgentRun = serde_json::from_str(&current_run_json).map_err(|error| {
        StorageError::Conflict(format!("claimed live run is malformed: {error}"))
    })?;
    claim
        .validate_initial_bundle(&initial_run, &checkpoint, &event)
        .map_err(|error| StorageError::Conflict(error.to_string()))?;
    let expected_mode = match current_run.mode {
        AgentRunMode::OneShot => "one_shot",
        AgentRunMode::Session => "session",
    };
    if serde_json::to_string(&checkpoint)? != checkpoint_json
        || checkpoint.state_digest.hex() != checkpoint_digest
        || checkpoint.created_at.to_rfc3339() != checkpoint_created_at
        || serde_json::to_string(&event)? != event_json
        || event.data_digest.hex() != event_digest
        || event.created_at.to_rfc3339() != event_created_at
        || serde_json::to_string(&current_run)? != current_run_json
        || run_actor != current_run.actor.to_string()
        || run_agent_id != current_run.agent_id.map(|id| id.to_string())
        || run_mode != expected_mode
        || indexed_run_status != run_status(current_run.status)
        || run_revision != revision_i64(current_run.revision)?
        || run_created_at != current_run.created_at.to_rfc3339()
        || run_updated_at != current_run.updated_at.to_rfc3339()
    {
        return Err(StorageError::Conflict(
            "claimed live start indexed columns or serialized evidence drifted".to_string(),
        ));
    }
    if current_run.id != initial_run.id
        || current_run.actor != initial_run.actor
        || current_run.agent_id != initial_run.agent_id
        || current_run.mode != initial_run.mode
        || current_run.goal != initial_run.goal
        || current_run.created_at != initial_run.created_at
        || current_run.revision < initial_run.revision
        || current_run.updated_at < initial_run.updated_at
    {
        return Err(StorageError::Conflict(
            "claimed live run identity or history drifted".to_string(),
        ));
    }
    Ok(run_id)
}

impl AgentRunStore for SqliteStore {
    fn claim_live_run_start(
        &self,
        claim: &LiveRunStartClaim,
        initial_run: &AgentRun,
        initial_checkpoint: &AgentCheckpoint,
        started_event: &AgentRunEvent,
    ) -> Result<LiveRunStartClaimResult, String> {
        SqliteStore::claim_live_run_start(
            self,
            claim,
            initial_run,
            initial_checkpoint,
            started_event,
        )
        .map_err(|error| error.to_string())
    }

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
