//! The storage backend trait for execution evidence, plus a test recorder.

use super::context::{AuditFilter, ExecutionContext};
use super::engine::ExecutionOutcome;
use super::error::ExecutionError;
use crate::canonical::ContentDigest;
use crate::delegation::Delegation;
use crate::evidence::Proof;
use crate::identity::PrincipalId;
use crate::operator::{GovernedEffectPolicy, PreparedHandlerOutput};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use uuid::Uuid;

/// The exact-replay behavior required by an operation handler.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum IdempotencyPolicy {
    #[default]
    None,
    RequiredUuidV7ExactReplay,
}

/// The durable scope of an execution replay claim.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionReplayKey {
    pub operation: String,
    pub version: String,
    #[serde(with = "crate::operator::strict_uuid_v7")]
    pub idempotency_key: Uuid,
}

/// A caller's attempt to claim an exact-replay key before mutation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionReplayClaim {
    pub key: ExecutionReplayKey,
    pub input_digest: ContentDigest,
    #[serde(with = "crate::operator::strict_uuid_v7")]
    pub claim_token: Uuid,
    #[serde(with = "crate::operator::strict_principal_id")]
    pub claimed_by: PrincipalId,
    #[serde(with = "crate::operator::strict_utc")]
    pub claimed_at: DateTime<Utc>,
}

/// The durable state observed while claiming an exact-replay key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionReplayClaimResult {
    Acquired,
    Completed(ExecutionOutcome),
    Conflict,
    InProgress,
    Failed,
    Unsupported,
}

pub trait ExecutionStore: Send + Sync {
    /// Loads a stored delegation by ID. Return `None` when it is unknown.
    fn load_delegation(&self, delegation_id: &Uuid) -> Result<Option<Delegation>, String> {
        let _ = delegation_id;
        Ok(None)
    }

    /// Persists a generated proof.
    fn save_proof(&self, proof: &Proof) -> Result<(), String>;

    /// Loads the most recent proof recorded for an operation/version.
    fn latest_proof_for_operation(
        &self,
        _operation: &str,
        _version: &str,
    ) -> Result<Option<Proof>, String> {
        Ok(None)
    }

    /// Persists the execution context and returns its storage identifier.
    fn save_execution_context(&self, context: &ExecutionContext) -> Result<String, String>;

    /// Loads audit contexts matching the filter.
    ///
    /// The default returns no records so storage backends can adopt audit
    /// querying independently of kernel changes.
    fn load_audit_contexts(&self, _filter: &AuditFilter) -> Result<Vec<ExecutionContext>, String> {
        Ok(Vec::new())
    }

    /// Atomically claims a scoped exact-replay key before mutation.
    fn claim_execution_replay(
        &self,
        _claim: &ExecutionReplayClaim,
    ) -> Result<ExecutionReplayClaimResult, String> {
        Ok(ExecutionReplayClaimResult::Unsupported)
    }

    /// Atomically persists a completed execution and its exact replay outcome.
    fn complete_execution_replay(
        &self,
        _claim: &ExecutionReplayClaim,
        _context: &ExecutionContext,
        _outcome: &ExecutionOutcome,
    ) -> Result<(), String> {
        Err("execution replay is not supported by this store".to_string())
    }

    /// Marks an acquired replay claim failed without making it replayable.
    fn fail_execution_replay(
        &self,
        _claim: &ExecutionReplayClaim,
        _failed_at: DateTime<Utc>,
        _failure: &str,
    ) -> Result<(), String> {
        Err("execution replay is not supported by this store".to_string())
    }
}

#[derive(Clone)]
enum RecordingReplayState {
    Claimed,
    Completed {
        context: ExecutionContext,
        outcome: ExecutionOutcome,
    },
    Failed {
        failed_at: DateTime<Utc>,
        failure: String,
    },
}

#[derive(Clone)]
struct RecordingReplay {
    claim: ExecutionReplayClaim,
    state: RecordingReplayState,
}

/// A simple in-memory execution store for testing.
#[derive(Default)]
pub struct RecordingStore {
    pub proofs: std::sync::Mutex<Vec<Proof>>,
    pub contexts: std::sync::Mutex<Vec<ExecutionContext>>,
    pub delegations: std::sync::Mutex<Vec<Delegation>>,
    execution_replays: std::sync::Mutex<HashMap<ExecutionReplayKey, RecordingReplay>>,
}

impl ExecutionStore for RecordingStore {
    fn load_delegation(&self, delegation_id: &Uuid) -> Result<Option<Delegation>, String> {
        Ok(self
            .delegations
            .lock()
            .unwrap()
            .iter()
            .find(|delegation| &delegation.id == delegation_id)
            .cloned())
    }

    fn save_proof(&self, proof: &Proof) -> Result<(), String> {
        self.proofs.lock().unwrap().push(proof.clone());
        Ok(())
    }

    fn save_execution_context(&self, context: &ExecutionContext) -> Result<String, String> {
        self.contexts.lock().unwrap().push(context.clone());
        Ok(Uuid::now_v7().to_string())
    }

    fn latest_proof_for_operation(
        &self,
        operation: &str,
        version: &str,
    ) -> Result<Option<Proof>, String> {
        let full_operation = format!("{operation}::{version}");
        Ok(self
            .proofs
            .lock()
            .unwrap()
            .iter()
            .filter(|proof| proof.body.operation == full_operation)
            .max_by_key(|proof| proof.body.timestamp)
            .cloned())
    }

    fn claim_execution_replay(
        &self,
        claim: &ExecutionReplayClaim,
    ) -> Result<ExecutionReplayClaimResult, String> {
        let mut replays = self.execution_replays.lock().unwrap();
        let Some(replay) = replays.get(&claim.key) else {
            replays.insert(
                claim.key.clone(),
                RecordingReplay {
                    claim: claim.clone(),
                    state: RecordingReplayState::Claimed,
                },
            );
            return Ok(ExecutionReplayClaimResult::Acquired);
        };

        if replay.claim.input_digest != claim.input_digest {
            return Ok(ExecutionReplayClaimResult::Conflict);
        }

        Ok(match &replay.state {
            RecordingReplayState::Claimed => ExecutionReplayClaimResult::InProgress,
            RecordingReplayState::Completed { outcome, .. } => {
                ExecutionReplayClaimResult::Completed(outcome.clone())
            }
            RecordingReplayState::Failed { .. } => ExecutionReplayClaimResult::Failed,
        })
    }

    fn complete_execution_replay(
        &self,
        claim: &ExecutionReplayClaim,
        context: &ExecutionContext,
        outcome: &ExecutionOutcome,
    ) -> Result<(), String> {
        validate_replay_completion(claim, context, outcome)?;

        let mut replays = self.execution_replays.lock().unwrap();
        let replay = replays
            .get_mut(&claim.key)
            .ok_or_else(|| "execution replay claim does not exist".to_string())?;
        if replay.claim.input_digest != claim.input_digest {
            return Err("execution replay input digest conflicts with the claim".to_string());
        }
        if replay.claim.claim_token != claim.claim_token {
            return Err("execution replay claim token does not match".to_string());
        }

        match &replay.state {
            RecordingReplayState::Claimed => {}
            RecordingReplayState::Completed {
                context: stored_context,
                outcome: stored_outcome,
            } if execution_contexts_equal(stored_context, context) && stored_outcome == outcome => {
                return Ok(())
            }
            RecordingReplayState::Completed { .. } => {
                return Err("execution replay is already completed differently".to_string())
            }
            RecordingReplayState::Failed { .. } => {
                return Err("failed execution replay cannot be completed".to_string())
            }
        }

        let mut contexts = self.contexts.lock().unwrap();
        let mut proofs = self.proofs.lock().unwrap();
        contexts.push(context.clone());
        proofs.push(outcome.proof.clone());
        replay.state = RecordingReplayState::Completed {
            context: context.clone(),
            outcome: outcome.clone(),
        };
        Ok(())
    }

    fn fail_execution_replay(
        &self,
        claim: &ExecutionReplayClaim,
        failed_at: DateTime<Utc>,
        failure: &str,
    ) -> Result<(), String> {
        if failure.is_empty() {
            return Err("execution replay failure must not be empty".to_string());
        }
        let mut replays = self.execution_replays.lock().unwrap();
        let replay = replays
            .get_mut(&claim.key)
            .ok_or_else(|| "execution replay claim does not exist".to_string())?;
        if replay.claim.input_digest != claim.input_digest {
            return Err("execution replay input digest conflicts with the claim".to_string());
        }
        if replay.claim.claim_token != claim.claim_token {
            return Err("execution replay claim token does not match".to_string());
        }

        match &replay.state {
            RecordingReplayState::Claimed => {
                replay.state = RecordingReplayState::Failed {
                    failed_at,
                    failure: failure.to_string(),
                };
                Ok(())
            }
            RecordingReplayState::Failed {
                failed_at: stored_at,
                failure: stored_failure,
            } if *stored_at == failed_at && stored_failure == failure => Ok(()),
            RecordingReplayState::Failed { .. } => {
                Err("execution replay is already failed differently".to_string())
            }
            RecordingReplayState::Completed { .. } => {
                Err("completed execution replay cannot be failed".to_string())
            }
        }
    }
}

fn execution_contexts_equal(left: &ExecutionContext, right: &ExecutionContext) -> bool {
    left.actor == right.actor
        && left.principal_kind == right.principal_kind
        && left.delegation_id == right.delegation_id
        && left.delegation_chain == right.delegation_chain
        && left.workspace_path == right.workspace_path
        && left.timestamp == right.timestamp
}

fn validate_replay_completion(
    claim: &ExecutionReplayClaim,
    context: &ExecutionContext,
    outcome: &ExecutionOutcome,
) -> Result<(), String> {
    let proof = &outcome.proof.body;
    let expected_operation = format!("{}::{}", claim.key.operation, claim.key.version);
    if proof.operation != expected_operation {
        return Err("execution replay proof operation does not match the claim".to_string());
    }
    if proof.input_digest != claim.input_digest {
        return Err("execution replay proof input digest does not match the claim".to_string());
    }
    if proof.actor != claim.claimed_by || proof.actor != context.actor {
        return Err("execution replay proof actor does not match the claim".to_string());
    }
    if proof.delegation_id != context.delegation_id || proof.timestamp != context.timestamp {
        return Err("execution replay proof context does not match".to_string());
    }
    let output = crate::canonical::canonicalize(&outcome.output)
        .map_err(|_| "execution replay output could not be canonicalized".to_string())?;
    if proof.output_digest
        != crate::canonical::digest(crate::canonical::ArtifactKind::OperationOutput, &output)
    {
        return Err("execution replay proof output digest does not match the output".to_string());
    }
    Ok(())
}

/// A handler that executes a specific operation.
pub trait OperationHandler: Send + Sync {
    /// The operation name this handler executes.
    fn operation(&self) -> &str;
    /// Declares whether this handler requires durable exact replay.
    fn idempotency_policy(&self) -> IdempotencyPolicy {
        IdempotencyPolicy::None
    }
    /// Declares the exact-replay policy for a requested operation version.
    ///
    /// Existing handlers retain their operation-wide policy through this
    /// default. Handlers that serve multiple versions can override it without
    /// changing their legacy `idempotency_policy` implementation.
    fn idempotency_policy_for(&self, _version: &str) -> IdempotencyPolicy {
        self.idempotency_policy()
    }
    /// Executes the operation with the given input and context.
    fn execute(&self, input: &Value, context: &ExecutionContext) -> Result<Value, ExecutionError>;
    /// Executes the requested operation version.
    ///
    /// Existing handlers retain their current execution behavior through this
    /// default. Multi-version handlers can override it to select an exact
    /// version-specific implementation.
    fn execute_versioned(
        &self,
        _version: &str,
        input: &Value,
        context: &ExecutionContext,
    ) -> Result<Value, ExecutionError> {
        self.execute(input, context)
    }
    /// Declares whether this version is eligible for prepared governed execution.
    fn governed_effect_policy_for(&self, _version: &str) -> GovernedEffectPolicy {
        GovernedEffectPolicy::Ineligible
    }
    /// Executes the single bounded boundary without persisting any result.
    fn execute_governed_versioned(
        &self,
        _version: &str,
        _input: &Value,
        _context: &ExecutionContext,
    ) -> Result<PreparedHandlerOutput, ExecutionError> {
        Err(ExecutionError::HandlerFailed(
            "handler is ineligible for governed execution".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{create_proof, digest, generate_keypair, ArtifactKind, PrincipalKind};
    use chrono::Duration;
    use serde_json::json;
    use std::path::PathBuf;

    struct LegacyStore;

    impl ExecutionStore for LegacyStore {
        fn save_proof(&self, _proof: &Proof) -> Result<(), String> {
            Ok(())
        }

        fn save_execution_context(&self, _context: &ExecutionContext) -> Result<String, String> {
            Ok("context".to_string())
        }
    }

    struct LegacyHandler;

    impl OperationHandler for LegacyHandler {
        fn operation(&self) -> &str {
            "test.legacy"
        }

        fn execute(
            &self,
            input: &Value,
            _context: &ExecutionContext,
        ) -> Result<Value, ExecutionError> {
            Ok(input.clone())
        }
    }

    fn fixture() -> (
        crate::Keypair,
        ExecutionContext,
        ExecutionReplayClaim,
        ExecutionOutcome,
    ) {
        let keypair = generate_keypair();
        let context = ExecutionContext {
            actor: keypair.principal_id,
            principal_kind: Some(PrincipalKind::Agent),
            delegation_id: None,
            delegation_chain: None,
            workspace_path: PathBuf::from("/tmp/replay"),
            timestamp: Utc::now(),
        };
        let input = json!({"idempotency_key": Uuid::now_v7(), "value": 1});
        let output = json!({"ok": true});
        let canonical_input = crate::canonicalize(&input).unwrap();
        let claim = ExecutionReplayClaim {
            key: ExecutionReplayKey {
                operation: "test.replay".to_string(),
                version: "v1".to_string(),
                idempotency_key: input["idempotency_key"].as_str().unwrap().parse().unwrap(),
            },
            input_digest: digest(ArtifactKind::OperationInput, &canonical_input),
            claim_token: Uuid::now_v7(),
            claimed_by: context.actor,
            claimed_at: context.timestamp,
        };
        let proof = create_proof(
            context.actor,
            None,
            "test.replay::v1",
            &input,
            &output,
            context.timestamp,
            &keypair,
        )
        .unwrap();
        (keypair, context, claim, ExecutionOutcome { output, proof })
    }

    #[test]
    fn legacy_defaults_are_source_compatible_and_fail_closed() {
        let store = LegacyStore;
        let (_, context, claim, outcome) = fixture();
        assert_eq!(
            store.claim_execution_replay(&claim).unwrap(),
            ExecutionReplayClaimResult::Unsupported
        );
        assert!(store
            .complete_execution_replay(&claim, &context, &outcome)
            .unwrap_err()
            .contains("not supported"));
        assert!(store
            .fail_execution_replay(&claim, context.timestamp, "failed")
            .unwrap_err()
            .contains("not supported"));
        assert_eq!(LegacyHandler.idempotency_policy(), IdempotencyPolicy::None);
        assert_eq!(
            LegacyHandler.idempotency_policy_for("v2"),
            IdempotencyPolicy::None
        );
        assert_eq!(
            LegacyHandler
                .execute_versioned("v2", &json!({"legacy": true}), &context)
                .unwrap(),
            json!({"legacy": true})
        );
        assert_eq!(IdempotencyPolicy::default(), IdempotencyPolicy::None);
    }

    #[test]
    fn recording_store_claims_completes_and_replays_exact_outcome() {
        let store = RecordingStore::default();
        let (_, context, claim, outcome) = fixture();
        assert_eq!(
            store.claim_execution_replay(&claim).unwrap(),
            ExecutionReplayClaimResult::Acquired
        );
        assert_eq!(
            store.claim_execution_replay(&claim).unwrap(),
            ExecutionReplayClaimResult::InProgress
        );

        store
            .complete_execution_replay(&claim, &context, &outcome)
            .unwrap();
        store
            .complete_execution_replay(&claim, &context, &outcome)
            .unwrap();
        assert_eq!(
            store.claim_execution_replay(&claim).unwrap(),
            ExecutionReplayClaimResult::Completed(outcome.clone())
        );
        assert_eq!(store.contexts.lock().unwrap().len(), 1);
        assert_eq!(store.proofs.lock().unwrap().as_slice(), &[outcome.proof]);
    }

    #[test]
    fn recording_store_reports_conflict_and_rejects_wrong_claim_token() {
        let store = RecordingStore::default();
        let (_, context, claim, outcome) = fixture();
        assert_eq!(
            store.claim_execution_replay(&claim).unwrap(),
            ExecutionReplayClaimResult::Acquired
        );

        let mut conflict = claim.clone();
        conflict.input_digest = ContentDigest::from_bytes([9; 32]);
        assert_eq!(
            store.claim_execution_replay(&conflict).unwrap(),
            ExecutionReplayClaimResult::Conflict
        );

        let mut wrong_token = claim.clone();
        wrong_token.claim_token = Uuid::now_v7();
        assert!(store
            .complete_execution_replay(&wrong_token, &context, &outcome)
            .unwrap_err()
            .contains("claim token"));
        assert_eq!(
            store.claim_execution_replay(&claim).unwrap(),
            ExecutionReplayClaimResult::InProgress
        );
    }

    #[test]
    fn recording_store_failed_claim_is_indeterminate_and_never_completed() {
        let store = RecordingStore::default();
        let (_, context, claim, outcome) = fixture();
        store.claim_execution_replay(&claim).unwrap();
        let failed_at = context.timestamp + Duration::seconds(1);
        store
            .fail_execution_replay(&claim, failed_at, "handler failed")
            .unwrap();
        store
            .fail_execution_replay(&claim, failed_at, "handler failed")
            .unwrap();
        assert_eq!(
            store.claim_execution_replay(&claim).unwrap(),
            ExecutionReplayClaimResult::Failed
        );
        assert!(store
            .complete_execution_replay(&claim, &context, &outcome)
            .unwrap_err()
            .contains("cannot be completed"));
        assert!(store.proofs.lock().unwrap().is_empty());
        assert!(store.contexts.lock().unwrap().is_empty());
    }
}
