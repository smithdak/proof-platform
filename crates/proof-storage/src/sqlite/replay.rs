//! Durable exact execution replay persistence.

use super::store::SqliteStore;
use crate::StorageError;
use chrono::{DateTime, Utc};
use proof_kernel::{
    canonicalize, digest, ArtifactKind, ExecutionContext, ExecutionOutcome, ExecutionReplayClaim,
    ExecutionReplayClaimResult, Proof,
};
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior};
use serde_json::Value;
use uuid::Uuid;

struct ReplayRow {
    input_digest: String,
    state: String,
    claim_token: String,
    claimed_by: String,
    claimed_at: String,
    completed_at: Option<String>,
    failed_at: Option<String>,
    failure: Option<String>,
    output_json: Option<String>,
    proof_id: Option<String>,
    proof_json: Option<String>,
    execution_context_id: Option<String>,
}

impl SqliteStore {
    /// Atomically claims an exact-replay tuple before the governed mutation begins.
    pub fn claim_execution_replay(
        &self,
        claim: &ExecutionReplayClaim,
    ) -> Result<ExecutionReplayClaimResult, StorageError> {
        validate_claim_shape(claim)?;
        let connection = self.conn.lock().unwrap();
        let transaction = Transaction::new_unchecked(&connection, TransactionBehavior::Immediate)?;
        let existing = load_replay(&transaction, claim)?;

        let result = if let Some(row) = existing {
            if row.input_digest != claim.input_digest.hex() {
                ExecutionReplayClaimResult::Conflict
            } else {
                match row.state.as_str() {
                    "claimed" => ExecutionReplayClaimResult::InProgress,
                    "failed" => ExecutionReplayClaimResult::Failed,
                    "completed" => load_completed_outcome(&transaction, &row)?,
                    state => {
                        return Err(StorageError::Conflict(format!(
                            "invalid execution replay state: {state}"
                        )))
                    }
                }
            }
        } else {
            transaction.execute(
                "INSERT INTO execution_replays (
                     operation, version, idempotency_key, input_digest, state, claim_token,
                     claimed_by, claimed_at
                 ) VALUES (?1, ?2, ?3, ?4, 'claimed', ?5, ?6, ?7)",
                rusqlite::params![
                    claim.key.operation,
                    claim.key.version,
                    claim.key.idempotency_key.to_string(),
                    claim.input_digest.hex(),
                    claim.claim_token.to_string(),
                    claim.claimed_by.as_uuid().to_string(),
                    claim.claimed_at.to_rfc3339(),
                ],
            )?;
            ExecutionReplayClaimResult::Acquired
        };

        transaction.commit()?;
        Ok(result)
    }

    /// Atomically persists the context, immutable proof, canonical output, and completion state.
    pub fn complete_execution_replay(
        &self,
        claim: &ExecutionReplayClaim,
        context: &ExecutionContext,
        outcome: &ExecutionOutcome,
    ) -> Result<(), StorageError> {
        validate_claim_shape(claim)?;
        let output_json = validate_completion(claim, context, outcome)?;
        let proof_json = serde_json::to_string(&outcome.proof)?;
        let connection = self.conn.lock().unwrap();
        let transaction = Transaction::new_unchecked(&connection, TransactionBehavior::Immediate)?;
        let row = load_replay(&transaction, claim)?.ok_or_else(|| {
            StorageError::Conflict("execution replay claim does not exist".to_string())
        })?;
        validate_stored_claim(&row, claim)?;

        match row.state.as_str() {
            "completed" => {
                validate_idempotent_completion(
                    &transaction,
                    &row,
                    context,
                    outcome,
                    &output_json,
                    &proof_json,
                )?;
                transaction.commit()?;
                return Ok(());
            }
            "failed" => {
                return Err(StorageError::Conflict(
                    "failed execution replay cannot be completed".to_string(),
                ))
            }
            "claimed" => {}
            state => {
                return Err(StorageError::Conflict(format!(
                    "invalid execution replay state: {state}"
                )))
            }
        }

        let context_id = Uuid::now_v7();
        insert_execution_context(&transaction, &context_id, context)?;
        insert_immutable_proof(&transaction, &outcome.proof, &proof_json)?;
        let updated = transaction.execute(
            "UPDATE execution_replays
             SET state = 'completed', completed_at = ?1, output_json = ?2,
                 proof_id = ?3, proof_json = ?4, execution_context_id = ?5
             WHERE operation = ?6 AND version = ?7 AND idempotency_key = ?8
               AND state = 'claimed' AND claim_token = ?9 AND input_digest = ?10",
            rusqlite::params![
                outcome.proof.body.timestamp.to_rfc3339(),
                output_json,
                outcome.proof.body.id.to_string(),
                proof_json,
                context_id.to_string(),
                claim.key.operation,
                claim.key.version,
                claim.key.idempotency_key.to_string(),
                claim.claim_token.to_string(),
                claim.input_digest.hex(),
            ],
        )?;
        if updated != 1 {
            return Err(StorageError::Conflict(
                "execution replay claim changed before completion".to_string(),
            ));
        }
        transaction.commit()?;
        Ok(())
    }

    /// Permanently marks an acquired claim failed; failed claims are never reclaimed.
    pub fn fail_execution_replay(
        &self,
        claim: &ExecutionReplayClaim,
        failed_at: DateTime<Utc>,
        failure: &str,
    ) -> Result<(), StorageError> {
        validate_claim_shape(claim)?;
        if failure.is_empty() {
            return Err(StorageError::Conflict(
                "execution replay failure must not be empty".to_string(),
            ));
        }
        let connection = self.conn.lock().unwrap();
        let transaction = Transaction::new_unchecked(&connection, TransactionBehavior::Immediate)?;
        let row = load_replay(&transaction, claim)?.ok_or_else(|| {
            StorageError::Conflict("execution replay claim does not exist".to_string())
        })?;
        validate_stored_claim(&row, claim)?;
        let failed_at = failed_at.to_rfc3339();

        match row.state.as_str() {
            "claimed" => {
                let updated = transaction.execute(
                    "UPDATE execution_replays
                     SET state = 'failed', failed_at = ?1, failure = ?2
                     WHERE operation = ?3 AND version = ?4 AND idempotency_key = ?5
                       AND state = 'claimed' AND claim_token = ?6 AND input_digest = ?7",
                    rusqlite::params![
                        failed_at,
                        failure,
                        claim.key.operation,
                        claim.key.version,
                        claim.key.idempotency_key.to_string(),
                        claim.claim_token.to_string(),
                        claim.input_digest.hex(),
                    ],
                )?;
                if updated != 1 {
                    return Err(StorageError::Conflict(
                        "execution replay claim changed before failure was recorded".to_string(),
                    ));
                }
            }
            "failed"
                if row.failed_at.as_deref() == Some(&failed_at)
                    && row.failure.as_deref() == Some(failure) => {}
            "failed" => {
                return Err(StorageError::Conflict(
                    "execution replay is already failed differently".to_string(),
                ))
            }
            "completed" => {
                return Err(StorageError::Conflict(
                    "completed execution replay cannot be failed".to_string(),
                ))
            }
            state => {
                return Err(StorageError::Conflict(format!(
                    "invalid execution replay state: {state}"
                )))
            }
        }

        transaction.commit()?;
        Ok(())
    }
}

fn validate_claim_shape(claim: &ExecutionReplayClaim) -> Result<(), StorageError> {
    if claim.key.idempotency_key.get_version_num() != 7 {
        return Err(StorageError::Conflict(
            "execution replay idempotency key must be UUIDv7".to_string(),
        ));
    }
    Ok(())
}

fn load_replay(
    transaction: &Transaction<'_>,
    claim: &ExecutionReplayClaim,
) -> Result<Option<ReplayRow>, StorageError> {
    transaction
        .query_row(
            "SELECT input_digest, state, claim_token, claimed_by, claimed_at,
                    completed_at, failed_at, failure, output_json, proof_id, proof_json,
                    execution_context_id
             FROM execution_replays
             WHERE operation = ?1 AND version = ?2 AND idempotency_key = ?3",
            rusqlite::params![
                claim.key.operation,
                claim.key.version,
                claim.key.idempotency_key.to_string(),
            ],
            |row| {
                Ok(ReplayRow {
                    input_digest: row.get(0)?,
                    state: row.get(1)?,
                    claim_token: row.get(2)?,
                    claimed_by: row.get(3)?,
                    claimed_at: row.get(4)?,
                    completed_at: row.get(5)?,
                    failed_at: row.get(6)?,
                    failure: row.get(7)?,
                    output_json: row.get(8)?,
                    proof_id: row.get(9)?,
                    proof_json: row.get(10)?,
                    execution_context_id: row.get(11)?,
                })
            },
        )
        .optional()
        .map_err(StorageError::from)
}

fn validate_stored_claim(
    row: &ReplayRow,
    claim: &ExecutionReplayClaim,
) -> Result<(), StorageError> {
    if row.input_digest != claim.input_digest.hex() {
        return Err(StorageError::Conflict(
            "execution replay input digest conflicts with the claim".to_string(),
        ));
    }
    if row.claim_token != claim.claim_token.to_string() {
        return Err(StorageError::Conflict(
            "execution replay claim token does not match".to_string(),
        ));
    }
    if row.claimed_by != claim.claimed_by.as_uuid().to_string()
        || row.claimed_at != claim.claimed_at.to_rfc3339()
    {
        return Err(StorageError::Conflict(
            "execution replay claimant does not match".to_string(),
        ));
    }
    Ok(())
}

fn load_completed_outcome(
    transaction: &Transaction<'_>,
    row: &ReplayRow,
) -> Result<ExecutionReplayClaimResult, StorageError> {
    let output_json = row.output_json.as_deref().ok_or_else(|| {
        StorageError::Conflict("completed execution replay has no output".to_string())
    })?;
    let proof_id = row.proof_id.as_deref().ok_or_else(|| {
        StorageError::Conflict("completed execution replay has no proof ID".to_string())
    })?;
    let proof_json = row.proof_json.as_deref().ok_or_else(|| {
        StorageError::Conflict("completed execution replay has no proof".to_string())
    })?;
    let output: Value = serde_json::from_str(output_json)?;
    let canonical_output = canonicalize(&output).map_err(|_| {
        StorageError::Conflict(
            "completed execution replay output could not be canonicalized".to_string(),
        )
    })?;
    if canonical_output.as_str() != output_json {
        return Err(StorageError::Conflict(
            "completed execution replay output is not canonical".to_string(),
        ));
    }
    let proof: Proof = serde_json::from_str(proof_json)?;
    if proof.body.id.to_string() != proof_id {
        return Err(StorageError::Conflict(
            "completed execution replay proof ID does not match its proof".to_string(),
        ));
    }
    let stored_proof: Option<String> = transaction
        .query_row(
            "SELECT signature FROM proofs WHERE id = ?1",
            [proof_id],
            |row| row.get(0),
        )
        .optional()?;
    if stored_proof.as_deref() != Some(proof_json) {
        return Err(StorageError::Conflict(
            "completed execution replay proof does not match immutable proof storage".to_string(),
        ));
    }
    let context_id = row.execution_context_id.as_deref().ok_or_else(|| {
        StorageError::Conflict("completed execution replay has no execution context".to_string())
    })?;
    let context_exists: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM execution_contexts WHERE id = ?1)",
        [context_id],
        |row| row.get(0),
    )?;
    if !context_exists {
        return Err(StorageError::Conflict(
            "completed execution replay execution context is missing".to_string(),
        ));
    }
    Ok(ExecutionReplayClaimResult::Completed(ExecutionOutcome {
        output,
        proof,
    }))
}

fn validate_completion(
    claim: &ExecutionReplayClaim,
    context: &ExecutionContext,
    outcome: &ExecutionOutcome,
) -> Result<String, StorageError> {
    let proof = &outcome.proof.body;
    let expected_operation = format!("{}::{}", claim.key.operation, claim.key.version);
    if proof.operation != expected_operation {
        return Err(StorageError::Conflict(
            "execution replay proof operation does not match the claim".to_string(),
        ));
    }
    if proof.input_digest != claim.input_digest {
        return Err(StorageError::Conflict(
            "execution replay proof input digest does not match the claim".to_string(),
        ));
    }
    if proof.actor != claim.claimed_by || proof.actor != context.actor {
        return Err(StorageError::Conflict(
            "execution replay proof actor does not match the claim".to_string(),
        ));
    }
    if proof.delegation_id != context.delegation_id || proof.timestamp != context.timestamp {
        return Err(StorageError::Conflict(
            "execution replay proof context does not match".to_string(),
        ));
    }
    let output = canonicalize(&outcome.output).map_err(|_| {
        StorageError::Conflict("execution replay output could not be canonicalized".to_string())
    })?;
    if proof.output_digest != digest(ArtifactKind::OperationOutput, &output) {
        return Err(StorageError::Conflict(
            "execution replay proof output digest does not match the output".to_string(),
        ));
    }
    Ok(output.as_str().to_string())
}

fn insert_execution_context(
    transaction: &Transaction<'_>,
    context_id: &Uuid,
    context: &ExecutionContext,
) -> Result<(), StorageError> {
    transaction.execute(
        "INSERT INTO execution_contexts (
             id, actor, delegation_id, workspace_path, timestamp
         ) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![
            context_id.to_string(),
            context.actor.as_uuid().to_string(),
            context.delegation_id.map(|id| id.to_string()),
            context.workspace_path.display().to_string(),
            context.timestamp.to_rfc3339(),
        ],
    )?;
    Ok(())
}

fn insert_immutable_proof(
    transaction: &Transaction<'_>,
    proof: &Proof,
    proof_json: &str,
) -> Result<(), StorageError> {
    transaction.execute(
        "INSERT INTO proofs (
             id, actor, version, delegation_id, operation, input_digest, output_digest,
             timestamp, expires_at, signature
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        rusqlite::params![
            proof.body.id.to_string(),
            proof.body.actor.as_uuid().to_string(),
            proof.body.operation.rsplit("::").next(),
            proof.body.delegation_id.map(|id| id.to_string()),
            proof.body.operation,
            proof.body.input_digest.hex(),
            proof.body.output_digest.hex(),
            proof.body.timestamp.to_rfc3339(),
            proof.body.expires_at.map(|time| time.to_rfc3339()),
            proof_json,
        ],
    )?;
    Ok(())
}

fn validate_idempotent_completion(
    transaction: &Transaction<'_>,
    row: &ReplayRow,
    context: &ExecutionContext,
    outcome: &ExecutionOutcome,
    output_json: &str,
    proof_json: &str,
) -> Result<(), StorageError> {
    let completed_at = outcome.proof.body.timestamp.to_rfc3339();
    let proof_id = outcome.proof.body.id.to_string();
    let exact_envelope = row.completed_at.as_deref() == Some(completed_at.as_str())
        && row.output_json.as_deref() == Some(output_json)
        && row.proof_id.as_deref() == Some(proof_id.as_str())
        && row.proof_json.as_deref() == Some(proof_json);
    if !exact_envelope {
        return Err(StorageError::Conflict(
            "execution replay is already completed differently".to_string(),
        ));
    }
    let context_id = row.execution_context_id.as_deref().ok_or_else(|| {
        StorageError::Conflict("completed execution replay has no execution context".to_string())
    })?;
    let stored_context: Option<(String, Option<String>, String, String)> = transaction
        .query_row(
            "SELECT actor, delegation_id, workspace_path, timestamp
             FROM execution_contexts WHERE id = ?1",
            [context_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;
    let expected_context = (
        context.actor.as_uuid().to_string(),
        context.delegation_id.map(|id| id.to_string()),
        context.workspace_path.display().to_string(),
        context.timestamp.to_rfc3339(),
    );
    if stored_context.as_ref() != Some(&expected_context) {
        return Err(StorageError::Conflict(
            "execution replay is already completed with a different context".to_string(),
        ));
    }
    let stored_proof: Option<String> = transaction
        .query_row(
            "SELECT signature FROM proofs WHERE id = ?1",
            [outcome.proof.body.id.to_string()],
            |row| row.get(0),
        )
        .optional()?;
    if stored_proof.as_deref() != Some(proof_json) {
        return Err(StorageError::Conflict(
            "execution replay is already completed with a different proof".to_string(),
        ));
    }
    Ok(())
}
