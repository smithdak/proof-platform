//! Durable agent run, step, checkpoint, retry, and evaluation contracts.

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use uuid::Uuid;

use crate::canonical::{canonicalize, digest, ArtifactKind, ContentDigest};
use crate::evidence::Proof;
use crate::identity::PrincipalId;
use crate::{AgentDefinition, AgentRunEvent, AgentRunEventKind, AgentStore};

pub const LIVE_RUN_START_CLAIM_SCHEMA: &str = "proof-live-run-start-claim/v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunMode {
    OneShot,
    Session,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunStatus {
    Queued,
    Running,
    WaitingForInput,
    Succeeded,
    Failed,
    Cancelled,
}

impl AgentRunStatus {
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunStepStatus {
    Pending,
    Running,
    WaitingForApproval,
    Succeeded,
    Failed,
    Cancelled,
}

impl AgentRunStepStatus {
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRun {
    pub id: Uuid,
    pub actor: PrincipalId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<Uuid>,
    pub mode: AgentRunMode,
    pub goal: String,
    pub status: AgentRunStatus,
    pub retry_count: u32,
    pub revision: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRunStep {
    pub id: Uuid,
    pub run_id: Uuid,
    pub ordinal: u32,
    pub attempt: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_of: Option<Uuid>,
    pub operation: String,
    pub version: String,
    pub input_digest: ContentDigest,
    pub status: AgentRunStepStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_request_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proof: Option<Proof>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub revision: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentCheckpoint {
    pub id: Uuid,
    pub run_id: Uuid,
    pub sequence: u32,
    pub state: Value,
    pub state_digest: ContentDigest,
    pub created_at: DateTime<Utc>,
}

/// Compact identity of one run's durable checkpoint tail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentCheckpointTail {
    pub checkpoint_id: Uuid,
    pub sequence: u32,
    pub state_digest: ContentDigest,
}

impl From<&AgentCheckpoint> for AgentCheckpointTail {
    fn from(checkpoint: &AgentCheckpoint) -> Self {
        Self {
            checkpoint_id: checkpoint.id,
            sequence: checkpoint.sequence,
            state_digest: checkpoint.state_digest,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentCheckpointAppendResult {
    Appended,
    Stale,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentEvaluationOutcome {
    Passed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRunEvaluation {
    pub id: Uuid,
    pub run_id: Uuid,
    pub evaluator: String,
    pub outcome: AgentEvaluationOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score_bps: Option<u16>,
    #[serde(default)]
    pub metrics: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Stable identity for one paid live-start authorization.
///
/// The proposed run ID is deliberately carried by the initial bundle rather
/// than this claim. An exact replay may propose a fresh UUID while the store
/// returns the run ID that first acquired this immutable claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveRunStartClaim {
    pub schema: String,
    pub readiness_binding_digest: ContentDigest,
    pub setup_digest: ContentDigest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveRunStartClaimResult {
    Acquired,
    Existing(Uuid),
    Conflict,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AgentRunError {
    #[error("agent run goal must not be empty")]
    EmptyGoal,
    #[error("agent run operation must not be empty")]
    EmptyOperation,
    #[error("agent run version must not be empty")]
    EmptyVersion,
    #[error("invalid agent run transition from {from:?} to {to:?}")]
    InvalidRunTransition {
        from: AgentRunStatus,
        to: AgentRunStatus,
    },
    #[error("invalid agent run step transition from {from:?} to {to:?}")]
    InvalidStepTransition {
        from: AgentRunStepStatus,
        to: AgentRunStepStatus,
    },
    #[error("agent run retry count overflow")]
    RetryOverflow,
    #[error("agent run step proof does not match its operation, input, or output")]
    ProofMismatch,
    #[error("agent run step approval request binding is immutable once assigned")]
    ApprovalBindingImmutable,
    #[error("agent run step input could not be canonicalized")]
    InvalidInput,
    #[error("agent checkpoint state could not be canonicalized")]
    InvalidCheckpoint,
    #[error("agent evaluation requires a terminal run")]
    RunNotTerminal,
    #[error("agent evaluation evaluator must not be empty")]
    EmptyEvaluator,
    #[error("agent evaluation score must be between 0 and 10000 basis points")]
    InvalidScore,
    #[error("agent evaluation metrics could not be canonicalized")]
    InvalidMetrics,
    #[error("live run start claim or initial evidence bundle is invalid")]
    InvalidLiveStartClaim,
}

impl LiveRunStartClaim {
    pub fn create(
        readiness_binding_digest: ContentDigest,
        setup_digest: ContentDigest,
    ) -> Result<Self, AgentRunError> {
        if readiness_binding_digest == ContentDigest::from_bytes([0; 32])
            || setup_digest == ContentDigest::from_bytes([0; 32])
        {
            return Err(AgentRunError::InvalidLiveStartClaim);
        }
        Ok(Self {
            schema: LIVE_RUN_START_CLAIM_SCHEMA.to_string(),
            readiness_binding_digest,
            setup_digest,
        })
    }

    /// Validates the exact initial live-v2 barrier represented by this claim.
    pub fn validate_initial_bundle(
        &self,
        run: &AgentRun,
        checkpoint: &AgentCheckpoint,
        started_event: &AgentRunEvent,
    ) -> Result<(), AgentRunError> {
        if self.schema != LIVE_RUN_START_CLAIM_SCHEMA
            || self.readiness_binding_digest == ContentDigest::from_bytes([0; 32])
            || self.setup_digest == ContentDigest::from_bytes([0; 32])
            || !crate::operator::uuid_is_v7(run.id)
            || run
                .agent_id
                .is_none_or(|id| !crate::operator::uuid_is_v7(id))
            || run.mode != AgentRunMode::Session
            || run.status != AgentRunStatus::Running
            || run.retry_count != 0
            || run.revision != 1
            || run.created_at != run.updated_at
            || run.completed_at.is_some()
            || !crate::operator::uuid_is_v7(checkpoint.id)
            || checkpoint.run_id != run.id
            || checkpoint.sequence != 0
            || checkpoint.created_at < run.created_at
            || !crate::operator::uuid_is_v7(started_event.id)
            || started_event.run_id != run.id
            || started_event.sequence != 0
            || started_event.kind != AgentRunEventKind::Started
            || started_event.created_at < checkpoint.created_at
        {
            return Err(AgentRunError::InvalidLiveStartClaim);
        }

        let envelope = checkpoint
            .state
            .as_object()
            .ok_or(AgentRunError::InvalidLiveStartClaim)?;
        if envelope.len() != 2
            || envelope.get("kind").and_then(Value::as_str) != Some("agent_runtime_v2")
        {
            return Err(AgentRunError::InvalidLiveStartClaim);
        }
        let runtime = envelope
            .get("runtime")
            .and_then(Value::as_object)
            .ok_or(AgentRunError::InvalidLiveStartClaim)?;
        let runtime_run_id = runtime
            .get("run_id")
            .cloned()
            .and_then(|value| serde_json::from_value::<Uuid>(value).ok());
        let runtime_agent_id = runtime
            .get("agent_id")
            .cloned()
            .and_then(|value| serde_json::from_value::<Uuid>(value).ok());
        let runtime_started_at = runtime
            .get("started_at")
            .cloned()
            .and_then(|value| serde_json::from_value::<DateTime<Utc>>(value).ok());
        let process_epoch_id = runtime
            .get("process_epoch_id")
            .cloned()
            .and_then(|value| serde_json::from_value::<Uuid>(value).ok());
        if runtime.get("schema").and_then(Value::as_str) != Some("proof-agent-runtime-state/v2")
            || runtime_run_id != Some(run.id)
            || runtime_agent_id != run.agent_id
            || runtime_started_at != Some(run.created_at)
            || process_epoch_id.is_none_or(|id| !crate::operator::uuid_is_v7(id))
        {
            return Err(AgentRunError::InvalidLiveStartClaim);
        }

        let checkpoint_canonical =
            canonicalize(&checkpoint.state).map_err(|_| AgentRunError::InvalidLiveStartClaim)?;
        if checkpoint.state_digest != digest(ArtifactKind::AgentCheckpoint, &checkpoint_canonical) {
            return Err(AgentRunError::InvalidLiveStartClaim);
        }
        let expected_started_data = serde_json::json!({
            "live": true,
            "schema": runtime.get("schema").cloned().unwrap_or(Value::Null),
            "process_epoch_id": runtime.get("process_epoch_id").cloned().unwrap_or(Value::Null),
            "policy_binding": runtime.get("policy_binding").cloned().unwrap_or(Value::Null),
            "authority": runtime.get("authority").cloned().unwrap_or(Value::Null),
        });
        let event_canonical =
            canonicalize(&started_event.data).map_err(|_| AgentRunError::InvalidLiveStartClaim)?;
        if started_event.data != expected_started_data
            || started_event.data_digest != digest(ArtifactKind::AgentEvent, &event_canonical)
        {
            return Err(AgentRunError::InvalidLiveStartClaim);
        }
        Ok(())
    }
}

impl AgentRun {
    pub fn new(
        actor: PrincipalId,
        mode: AgentRunMode,
        goal: impl Into<String>,
        created_at: DateTime<Utc>,
    ) -> Result<Self, AgentRunError> {
        Self::create(actor, None, mode, goal, created_at)
    }

    pub fn new_for_agent(
        actor: PrincipalId,
        agent_id: Uuid,
        mode: AgentRunMode,
        goal: impl Into<String>,
        created_at: DateTime<Utc>,
    ) -> Result<Self, AgentRunError> {
        Self::create(actor, Some(agent_id), mode, goal, created_at)
    }

    fn create(
        actor: PrincipalId,
        agent_id: Option<Uuid>,
        mode: AgentRunMode,
        goal: impl Into<String>,
        created_at: DateTime<Utc>,
    ) -> Result<Self, AgentRunError> {
        let goal = goal.into();
        if goal.trim().is_empty() {
            return Err(AgentRunError::EmptyGoal);
        }
        Ok(Self {
            id: Uuid::now_v7(),
            actor,
            agent_id,
            mode,
            goal,
            status: AgentRunStatus::Queued,
            retry_count: 0,
            revision: 0,
            created_at,
            updated_at: created_at,
            completed_at: None,
        })
    }

    pub fn start(&mut self, now: DateTime<Utc>) -> Result<(), AgentRunError> {
        self.transition(AgentRunStatus::Running, now)
    }

    pub fn wait_for_input(&mut self, now: DateTime<Utc>) -> Result<(), AgentRunError> {
        self.transition(AgentRunStatus::WaitingForInput, now)
    }

    pub fn resume(&mut self, now: DateTime<Utc>) -> Result<(), AgentRunError> {
        if self.status == AgentRunStatus::Failed {
            self.retry_count = self
                .retry_count
                .checked_add(1)
                .ok_or(AgentRunError::RetryOverflow)?;
        }
        self.transition(AgentRunStatus::Running, now)
    }

    pub fn succeed(&mut self, now: DateTime<Utc>) -> Result<(), AgentRunError> {
        self.transition(AgentRunStatus::Succeeded, now)
    }

    pub fn fail(&mut self, now: DateTime<Utc>) -> Result<(), AgentRunError> {
        self.transition(AgentRunStatus::Failed, now)
    }

    pub fn cancel(&mut self, now: DateTime<Utc>) -> Result<(), AgentRunError> {
        self.transition(AgentRunStatus::Cancelled, now)
    }

    fn transition(
        &mut self,
        next: AgentRunStatus,
        now: DateTime<Utc>,
    ) -> Result<(), AgentRunError> {
        let valid = matches!(
            (self.status, next),
            (AgentRunStatus::Queued, AgentRunStatus::Running)
                | (AgentRunStatus::Queued, AgentRunStatus::Cancelled)
                | (AgentRunStatus::Running, AgentRunStatus::WaitingForInput)
                | (AgentRunStatus::Running, AgentRunStatus::Succeeded)
                | (AgentRunStatus::Running, AgentRunStatus::Failed)
                | (AgentRunStatus::Running, AgentRunStatus::Cancelled)
                | (AgentRunStatus::WaitingForInput, AgentRunStatus::Running)
                | (AgentRunStatus::WaitingForInput, AgentRunStatus::Failed)
                | (AgentRunStatus::WaitingForInput, AgentRunStatus::Cancelled)
                | (AgentRunStatus::Failed, AgentRunStatus::Running)
                | (AgentRunStatus::Failed, AgentRunStatus::Cancelled)
        );
        if !valid {
            return Err(AgentRunError::InvalidRunTransition {
                from: self.status,
                to: next,
            });
        }
        self.status = next;
        self.updated_at = now;
        self.revision += 1;
        self.completed_at = next.is_terminal().then_some(now);
        Ok(())
    }
}

impl AgentRunStep {
    pub fn new(
        run_id: Uuid,
        ordinal: u32,
        operation: impl Into<String>,
        version: impl Into<String>,
        input: &Value,
        created_at: DateTime<Utc>,
    ) -> Result<Self, AgentRunError> {
        let operation = operation.into();
        if operation.trim().is_empty() {
            return Err(AgentRunError::EmptyOperation);
        }
        let version = version.into();
        if version.trim().is_empty() {
            return Err(AgentRunError::EmptyVersion);
        }
        let input = canonicalize(input).map_err(|_| AgentRunError::InvalidInput)?;
        Ok(Self {
            id: Uuid::now_v7(),
            run_id,
            ordinal,
            attempt: 1,
            retry_of: None,
            operation,
            version,
            input_digest: digest(ArtifactKind::OperationInput, &input),
            status: AgentRunStepStatus::Pending,
            approval_request_id: None,
            output: None,
            proof: None,
            error: None,
            revision: 0,
            created_at,
            updated_at: created_at,
            started_at: None,
            completed_at: None,
        })
    }

    pub fn start(&mut self, now: DateTime<Utc>) -> Result<(), AgentRunError> {
        self.transition(AgentRunStepStatus::Running, now)?;
        self.started_at.get_or_insert(now);
        Ok(())
    }

    pub fn wait_for_approval(
        &mut self,
        approval_request_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<(), AgentRunError> {
        if self
            .approval_request_id
            .is_some_and(|existing| existing != approval_request_id)
        {
            return Err(AgentRunError::ApprovalBindingImmutable);
        }
        self.transition(AgentRunStepStatus::WaitingForApproval, now)?;
        self.approval_request_id = Some(approval_request_id);
        Ok(())
    }

    pub fn resume_from_approval(&mut self, now: DateTime<Utc>) -> Result<(), AgentRunError> {
        self.transition(AgentRunStepStatus::Running, now)
    }

    pub fn succeed(
        &mut self,
        output: Value,
        proof: Proof,
        now: DateTime<Utc>,
    ) -> Result<(), AgentRunError> {
        let output_canonical = canonicalize(&output).map_err(|_| AgentRunError::ProofMismatch)?;
        let proof_operation = format!("{}::{}", self.operation, self.version);
        if proof.body.operation != proof_operation
            || proof.body.input_digest != self.input_digest
            || proof.body.output_digest != digest(ArtifactKind::OperationOutput, &output_canonical)
        {
            return Err(AgentRunError::ProofMismatch);
        }
        self.transition(AgentRunStepStatus::Succeeded, now)?;
        self.output = Some(output);
        self.proof = Some(proof);
        self.error = None;
        Ok(())
    }

    pub fn fail(
        &mut self,
        error: impl Into<String>,
        now: DateTime<Utc>,
    ) -> Result<(), AgentRunError> {
        self.transition(AgentRunStepStatus::Failed, now)?;
        self.error = Some(error.into());
        Ok(())
    }

    pub fn cancel(&mut self, now: DateTime<Utc>) -> Result<(), AgentRunError> {
        self.transition(AgentRunStepStatus::Cancelled, now)
    }

    pub fn retry(&self, created_at: DateTime<Utc>) -> Result<Self, AgentRunError> {
        if !matches!(
            self.status,
            AgentRunStepStatus::Failed | AgentRunStepStatus::Cancelled
        ) {
            return Err(AgentRunError::InvalidStepTransition {
                from: self.status,
                to: AgentRunStepStatus::Pending,
            });
        }
        Ok(Self {
            id: Uuid::now_v7(),
            run_id: self.run_id,
            ordinal: self.ordinal,
            attempt: self
                .attempt
                .checked_add(1)
                .ok_or(AgentRunError::RetryOverflow)?,
            retry_of: Some(self.id),
            operation: self.operation.clone(),
            version: self.version.clone(),
            input_digest: self.input_digest,
            status: AgentRunStepStatus::Pending,
            approval_request_id: None,
            output: None,
            proof: None,
            error: None,
            revision: 0,
            created_at,
            updated_at: created_at,
            started_at: None,
            completed_at: None,
        })
    }

    fn transition(
        &mut self,
        next: AgentRunStepStatus,
        now: DateTime<Utc>,
    ) -> Result<(), AgentRunError> {
        let valid = matches!(
            (self.status, next),
            (AgentRunStepStatus::Pending, AgentRunStepStatus::Running)
                | (AgentRunStepStatus::Pending, AgentRunStepStatus::Cancelled)
                | (
                    AgentRunStepStatus::Running,
                    AgentRunStepStatus::WaitingForApproval
                )
                | (AgentRunStepStatus::Running, AgentRunStepStatus::Succeeded)
                | (AgentRunStepStatus::Running, AgentRunStepStatus::Failed)
                | (AgentRunStepStatus::Running, AgentRunStepStatus::Cancelled)
                | (
                    AgentRunStepStatus::WaitingForApproval,
                    AgentRunStepStatus::Running
                )
                | (
                    AgentRunStepStatus::WaitingForApproval,
                    AgentRunStepStatus::Failed
                )
                | (
                    AgentRunStepStatus::WaitingForApproval,
                    AgentRunStepStatus::Cancelled
                )
        );
        if !valid {
            return Err(AgentRunError::InvalidStepTransition {
                from: self.status,
                to: next,
            });
        }
        self.status = next;
        self.updated_at = now;
        self.revision += 1;
        self.completed_at = next.is_terminal().then_some(now);
        Ok(())
    }
}

impl AgentCheckpoint {
    pub fn create(
        run_id: Uuid,
        sequence: u32,
        state: Value,
        created_at: DateTime<Utc>,
    ) -> Result<Self, AgentRunError> {
        let canonical = canonicalize(&state).map_err(|_| AgentRunError::InvalidCheckpoint)?;
        Ok(Self {
            id: Uuid::now_v7(),
            run_id,
            sequence,
            state,
            state_digest: digest(ArtifactKind::AgentCheckpoint, &canonical),
            created_at,
        })
    }
}

impl AgentRunEvaluation {
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        run: &AgentRun,
        evaluator: impl Into<String>,
        outcome: AgentEvaluationOutcome,
        score_bps: Option<u16>,
        metrics: Value,
        summary: Option<String>,
        created_at: DateTime<Utc>,
    ) -> Result<Self, AgentRunError> {
        if !run.status.is_terminal() {
            return Err(AgentRunError::RunNotTerminal);
        }
        let evaluator = evaluator.into();
        if evaluator.trim().is_empty() {
            return Err(AgentRunError::EmptyEvaluator);
        }
        if score_bps.is_some_and(|score| score > 10_000) {
            return Err(AgentRunError::InvalidScore);
        }
        canonicalize(&metrics).map_err(|_| AgentRunError::InvalidMetrics)?;
        Ok(Self {
            id: Uuid::now_v7(),
            run_id: run.id,
            evaluator,
            outcome,
            score_bps,
            metrics,
            summary: summary.filter(|summary| !summary.trim().is_empty()),
            created_at,
        })
    }
}

pub trait AgentRunStore: Send + Sync {
    /// Atomically claims one immutable live start and its initial durable
    /// run/checkpoint/event barrier. Third-party stores remain compatible and
    /// fail closed until they implement the capability.
    fn claim_live_run_start(
        &self,
        _claim: &LiveRunStartClaim,
        _initial_run: &AgentRun,
        _initial_checkpoint: &AgentCheckpoint,
        _started_event: &AgentRunEvent,
    ) -> Result<LiveRunStartClaimResult, String> {
        Ok(LiveRunStartClaimResult::Unsupported)
    }
    fn save_agent_run(&self, run: &AgentRun) -> Result<(), String>;
    fn load_agent_run(&self, run_id: &Uuid) -> Result<Option<AgentRun>, String>;
    fn list_agent_runs(&self) -> Result<Vec<AgentRun>, String>;
    /// Saves a step revision without ever replacing a non-null approval request binding.
    fn save_agent_run_step(&self, step: &AgentRunStep) -> Result<(), String>;
    fn load_agent_run_step(&self, step_id: &Uuid) -> Result<Option<AgentRunStep>, String>;
    fn list_agent_run_steps(&self, run_id: &Uuid) -> Result<Vec<AgentRunStep>, String>;
    fn find_agent_run_step_by_approval(
        &self,
        approval_request_id: &Uuid,
    ) -> Result<Option<AgentRunStep>, String>;
    /// Atomically appends a checkpoint only when the current run tail matches
    /// `expected_tail`. Exact retries are idempotently reported as appended;
    /// stale observations do not write, and malformed or conflicting
    /// checkpoint candidates return an error.
    fn append_agent_checkpoint(
        &self,
        _expected_tail: Option<&AgentCheckpointTail>,
        _checkpoint: &AgentCheckpoint,
    ) -> Result<AgentCheckpointAppendResult, String> {
        Ok(AgentCheckpointAppendResult::Unsupported)
    }
    fn save_agent_checkpoint(&self, checkpoint: &AgentCheckpoint) -> Result<(), String>;
    fn list_agent_checkpoints(&self, run_id: &Uuid) -> Result<Vec<AgentCheckpoint>, String>;
    fn save_agent_run_evaluation(&self, evaluation: &AgentRunEvaluation) -> Result<(), String>;
    fn list_agent_run_evaluations(&self, run_id: &Uuid) -> Result<Vec<AgentRunEvaluation>, String>;
}

#[derive(Default)]
pub struct RecordingAgentRunStore {
    runs: Mutex<BTreeMap<Uuid, AgentRun>>,
    steps: Mutex<BTreeMap<Uuid, AgentRunStep>>,
    checkpoints: Mutex<BTreeMap<Uuid, AgentCheckpoint>>,
    evaluations: Mutex<BTreeMap<Uuid, AgentRunEvaluation>>,
    definitions: Mutex<BTreeMap<Uuid, AgentDefinition>>,
    events: Mutex<BTreeMap<Uuid, AgentRunEvent>>,
    live_start_claims: Mutex<HashMap<ContentDigest, RecordingLiveRunStartClaim>>,
}

#[derive(Clone)]
struct RecordingLiveRunStartClaim {
    claim: LiveRunStartClaim,
    initial_run: AgentRun,
    checkpoint_id: Uuid,
    event_id: Uuid,
}

impl AgentRunStore for RecordingAgentRunStore {
    fn claim_live_run_start(
        &self,
        claim: &LiveRunStartClaim,
        initial_run: &AgentRun,
        initial_checkpoint: &AgentCheckpoint,
        started_event: &AgentRunEvent,
    ) -> Result<LiveRunStartClaimResult, String> {
        claim
            .validate_initial_bundle(initial_run, initial_checkpoint, started_event)
            .map_err(|error| error.to_string())?;
        let mut claims = self
            .live_start_claims
            .lock()
            .map_err(|_| "live start claim lock poisoned".to_string())?;
        let existing = claims.values().find(|existing| {
            existing.claim.readiness_binding_digest == claim.readiness_binding_digest
                || existing.claim.setup_digest == claim.setup_digest
        });
        if let Some(existing) = existing {
            if existing.claim != *claim {
                return Ok(LiveRunStartClaimResult::Conflict);
            }
            let runs = self
                .runs
                .lock()
                .map_err(|_| "agent run lock poisoned".to_string())?;
            let checkpoints = self
                .checkpoints
                .lock()
                .map_err(|_| "agent checkpoint lock poisoned".to_string())?;
            let events = self
                .events
                .lock()
                .map_err(|_| "agent event lock poisoned".to_string())?;
            let current_run = runs
                .get(&existing.initial_run.id)
                .ok_or_else(|| "claimed live run is missing".to_string())?;
            let checkpoint = checkpoints
                .get(&existing.checkpoint_id)
                .ok_or_else(|| "claimed live start checkpoint is missing".to_string())?;
            let event = events
                .get(&existing.event_id)
                .ok_or_else(|| "claimed live start event is missing".to_string())?;
            existing
                .claim
                .validate_initial_bundle(&existing.initial_run, checkpoint, event)
                .map_err(|error| error.to_string())?;
            validate_progressed_claim_run(&existing.initial_run, current_run)?;
            return Ok(LiveRunStartClaimResult::Existing(existing.initial_run.id));
        }

        let mut runs = self
            .runs
            .lock()
            .map_err(|_| "agent run lock poisoned".to_string())?;
        let mut checkpoints = self
            .checkpoints
            .lock()
            .map_err(|_| "agent checkpoint lock poisoned".to_string())?;
        let mut events = self
            .events
            .lock()
            .map_err(|_| "agent event lock poisoned".to_string())?;
        if runs.contains_key(&initial_run.id)
            || checkpoints.contains_key(&initial_checkpoint.id)
            || checkpoints
                .values()
                .any(|checkpoint| checkpoint.run_id == initial_run.id && checkpoint.sequence == 0)
            || events.contains_key(&started_event.id)
            || events
                .values()
                .any(|event| event.run_id == initial_run.id && event.sequence == 0)
        {
            return Err("live start initial evidence conflicts with existing rows".to_string());
        }
        runs.insert(initial_run.id, initial_run.clone());
        checkpoints.insert(initial_checkpoint.id, initial_checkpoint.clone());
        events.insert(started_event.id, started_event.clone());
        claims.insert(
            claim.readiness_binding_digest,
            RecordingLiveRunStartClaim {
                claim: claim.clone(),
                initial_run: initial_run.clone(),
                checkpoint_id: initial_checkpoint.id,
                event_id: started_event.id,
            },
        );
        Ok(LiveRunStartClaimResult::Acquired)
    }

    fn save_agent_run(&self, run: &AgentRun) -> Result<(), String> {
        save_versioned(&self.runs, run.id, run.revision, run, |run| run.revision)
    }

    fn load_agent_run(&self, run_id: &Uuid) -> Result<Option<AgentRun>, String> {
        load_record(&self.runs, run_id, "agent run")
    }

    fn list_agent_runs(&self) -> Result<Vec<AgentRun>, String> {
        let mut runs = self
            .runs
            .lock()
            .map_err(|_| "agent run lock poisoned".to_string())?
            .values()
            .cloned()
            .collect::<Vec<_>>();
        runs.sort_by_key(|run| (run.created_at, run.id));
        Ok(runs)
    }

    fn save_agent_run_step(&self, step: &AgentRunStep) -> Result<(), String> {
        let mut steps = self
            .steps
            .lock()
            .map_err(|_| "agent run step lock poisoned".to_string())?;
        if let Some(existing) = steps.get(&step.id) {
            if existing.approval_request_id.is_some()
                && existing.approval_request_id != step.approval_request_id
            {
                return Err(format!(
                    "agent run step {} approval binding is immutable once assigned",
                    step.id
                ));
            }
        }
        if let Some(approval_request_id) = step.approval_request_id {
            if let Some(existing) = steps.values().find(|existing| {
                existing.id != step.id && existing.approval_request_id == Some(approval_request_id)
            }) {
                return Err(format!(
                    "approval request {approval_request_id} is already bound to agent run step {}",
                    existing.id
                ));
            }
        }
        save_versioned_locked(&mut steps, step.id, step.revision, step, |step| {
            step.revision
        })
    }

    fn load_agent_run_step(&self, step_id: &Uuid) -> Result<Option<AgentRunStep>, String> {
        load_record(&self.steps, step_id, "agent run step")
    }

    fn list_agent_run_steps(&self, run_id: &Uuid) -> Result<Vec<AgentRunStep>, String> {
        let mut steps = self
            .steps
            .lock()
            .map_err(|_| "agent run step lock poisoned".to_string())?
            .values()
            .filter(|step| step.run_id == *run_id)
            .cloned()
            .collect::<Vec<_>>();
        steps.sort_by_key(|step| (step.ordinal, step.attempt));
        Ok(steps)
    }

    fn find_agent_run_step_by_approval(
        &self,
        approval_request_id: &Uuid,
    ) -> Result<Option<AgentRunStep>, String> {
        let steps = self
            .steps
            .lock()
            .map_err(|_| "agent run step lock poisoned".to_string())?;
        let mut matches = steps
            .values()
            .filter(|step| step.approval_request_id == Some(*approval_request_id));
        let matched = matches.next().cloned();
        if matches.next().is_some() {
            return Err(format!(
                "approval request {approval_request_id} is bound to multiple agent run steps"
            ));
        }
        Ok(matched)
    }

    fn append_agent_checkpoint(
        &self,
        expected_tail: Option<&AgentCheckpointTail>,
        checkpoint: &AgentCheckpoint,
    ) -> Result<AgentCheckpointAppendResult, String> {
        validate_agent_checkpoint_integrity(checkpoint, "new agent checkpoint")?;
        let expected_sequence = expected_tail
            .map(|tail| {
                tail.sequence.checked_add(1).ok_or_else(|| {
                    "new agent checkpoint sequence cannot follow the maximum expected tail sequence"
                        .to_string()
                })
            })
            .transpose()?
            .unwrap_or(0);
        if checkpoint.sequence != expected_sequence {
            return Err(format!(
                "new agent checkpoint sequence {} does not follow expected sequence {}",
                checkpoint.sequence, expected_sequence
            ));
        }

        let mut checkpoints = self
            .checkpoints
            .lock()
            .map_err(|_| "agent checkpoint lock poisoned".to_string())?;

        if let Some(existing) = checkpoints.get(&checkpoint.id) {
            if existing != checkpoint {
                return Err(format!(
                    "agent checkpoint {} conflicts with an existing immutable checkpoint",
                    checkpoint.id
                ));
            }
        }
        if let Some(expected_tail) = expected_tail {
            if let Some(predecessor) = checkpoints.get(&expected_tail.checkpoint_id) {
                if predecessor.run_id != checkpoint.run_id {
                    return Err(format!(
                        "expected agent checkpoint tail {} belongs to run {}, not {}",
                        expected_tail.checkpoint_id, predecessor.run_id, checkpoint.run_id
                    ));
                }
            }
        }

        let current = checkpoints
            .values()
            .filter(|existing| existing.run_id == checkpoint.run_id)
            .max_by_key(|existing| existing.sequence)
            .cloned();
        if let Some(current) = current.as_ref() {
            validate_agent_checkpoint_integrity(current, "stored current agent checkpoint")?;
        }
        let current_tail = current.as_ref().map(AgentCheckpointTail::from);
        let candidate_tail = AgentCheckpointTail::from(checkpoint);

        if current_tail == Some(candidate_tail) {
            let predecessor = checkpoint.sequence.checked_sub(1).and_then(|sequence| {
                checkpoints
                    .values()
                    .find(|existing| {
                        existing.run_id == checkpoint.run_id && existing.sequence == sequence
                    })
                    .cloned()
            });
            if let Some(predecessor) = predecessor.as_ref() {
                validate_agent_checkpoint_integrity(
                    predecessor,
                    "stored predecessor agent checkpoint",
                )?;
            }
            let predecessor_tail = predecessor.as_ref().map(AgentCheckpointTail::from);
            if current.as_ref() != Some(checkpoint) {
                return Err(format!(
                    "agent checkpoint {} has a conflicting immutable envelope",
                    checkpoint.id
                ));
            }
            return Ok(if predecessor_tail.as_ref() == expected_tail {
                AgentCheckpointAppendResult::Appended
            } else {
                AgentCheckpointAppendResult::Stale
            });
        }

        if current_tail.as_ref() != expected_tail {
            return Ok(AgentCheckpointAppendResult::Stale);
        }
        if checkpoints.values().any(|existing| {
            existing.run_id == checkpoint.run_id && existing.sequence == checkpoint.sequence
        }) {
            return Err(format!(
                "agent checkpoint sequence {} conflicts within run {}",
                checkpoint.sequence, checkpoint.run_id
            ));
        }

        checkpoints.insert(checkpoint.id, checkpoint.clone());
        Ok(AgentCheckpointAppendResult::Appended)
    }

    fn save_agent_checkpoint(&self, checkpoint: &AgentCheckpoint) -> Result<(), String> {
        let mut checkpoints = self
            .checkpoints
            .lock()
            .map_err(|_| "agent checkpoint lock poisoned".to_string())?;
        if checkpoints.values().any(|existing| {
            existing.run_id == checkpoint.run_id
                && existing.sequence == checkpoint.sequence
                && existing.id != checkpoint.id
        }) {
            return Err(format!(
                "agent checkpoint sequence {} already exists for run {}",
                checkpoint.sequence, checkpoint.run_id
            ));
        }
        save_once_locked(
            &mut checkpoints,
            checkpoint.id,
            checkpoint,
            "agent checkpoint",
        )
    }

    fn list_agent_checkpoints(&self, run_id: &Uuid) -> Result<Vec<AgentCheckpoint>, String> {
        let mut checkpoints = self
            .checkpoints
            .lock()
            .map_err(|_| "agent checkpoint lock poisoned".to_string())?
            .values()
            .filter(|checkpoint| checkpoint.run_id == *run_id)
            .cloned()
            .collect::<Vec<_>>();
        checkpoints.sort_by_key(|checkpoint| checkpoint.sequence);
        Ok(checkpoints)
    }

    fn save_agent_run_evaluation(&self, evaluation: &AgentRunEvaluation) -> Result<(), String> {
        let mut evaluations = self
            .evaluations
            .lock()
            .map_err(|_| "agent run evaluation lock poisoned".to_string())?;
        save_once_locked(
            &mut evaluations,
            evaluation.id,
            evaluation,
            "agent run evaluation",
        )
    }

    fn list_agent_run_evaluations(&self, run_id: &Uuid) -> Result<Vec<AgentRunEvaluation>, String> {
        let mut evaluations = self
            .evaluations
            .lock()
            .map_err(|_| "agent run evaluation lock poisoned".to_string())?
            .values()
            .filter(|evaluation| evaluation.run_id == *run_id)
            .cloned()
            .collect::<Vec<_>>();
        evaluations.sort_by_key(|evaluation| (evaluation.created_at, evaluation.id));
        Ok(evaluations)
    }
}

impl AgentStore for RecordingAgentRunStore {
    fn save_agent_definition(&self, agent: &AgentDefinition) -> Result<(), String> {
        let mut definitions = self
            .definitions
            .lock()
            .map_err(|_| "agent definition lock poisoned".to_string())?;
        if definitions
            .values()
            .any(|existing| existing.name == agent.name && existing.id != agent.id)
        {
            return Err(format!("agent name already exists: {}", agent.name));
        }
        save_once_locked(&mut definitions, agent.id, agent, "agent definition")
    }

    fn load_agent_definition(&self, agent_id: &Uuid) -> Result<Option<AgentDefinition>, String> {
        load_record(&self.definitions, agent_id, "agent definition")
    }

    fn list_agent_definitions(&self) -> Result<Vec<AgentDefinition>, String> {
        let mut definitions = self
            .definitions
            .lock()
            .map_err(|_| "agent definition lock poisoned".to_string())?
            .values()
            .cloned()
            .collect::<Vec<_>>();
        definitions.sort_by_key(|agent| (agent.created_at, agent.id));
        Ok(definitions)
    }

    fn save_agent_run_event(&self, event: &AgentRunEvent) -> Result<(), String> {
        let mut events = self
            .events
            .lock()
            .map_err(|_| "agent run event lock poisoned".to_string())?;
        if events.values().any(|existing| {
            existing.run_id == event.run_id
                && existing.sequence == event.sequence
                && existing.id != event.id
        }) {
            return Err(format!(
                "agent run event sequence {} already exists for run {}",
                event.sequence, event.run_id
            ));
        }
        save_once_locked(&mut events, event.id, event, "agent run event")
    }

    fn list_agent_run_events(&self, run_id: &Uuid) -> Result<Vec<AgentRunEvent>, String> {
        let mut events = self
            .events
            .lock()
            .map_err(|_| "agent run event lock poisoned".to_string())?
            .values()
            .filter(|event| event.run_id == *run_id)
            .cloned()
            .collect::<Vec<_>>();
        events.sort_by_key(|event| event.sequence);
        Ok(events)
    }
}

fn validate_agent_checkpoint_integrity(
    checkpoint: &AgentCheckpoint,
    description: &str,
) -> Result<(), String> {
    let canonical = canonicalize(&checkpoint.state)
        .map_err(|error| format!("{description} state is not canonicalizable: {error}"))?;
    if checkpoint.state_digest != digest(ArtifactKind::AgentCheckpoint, &canonical) {
        return Err(format!(
            "{description} state digest does not match its state"
        ));
    }
    Ok(())
}

fn validate_progressed_claim_run(initial: &AgentRun, current: &AgentRun) -> Result<(), String> {
    if current.id != initial.id
        || current.actor != initial.actor
        || current.agent_id != initial.agent_id
        || current.mode != initial.mode
        || current.goal != initial.goal
        || current.created_at != initial.created_at
        || current.revision < initial.revision
        || current.updated_at < initial.updated_at
    {
        return Err("claimed live run identity or history drifted".to_string());
    }
    Ok(())
}

fn save_versioned<T: Clone + PartialEq>(
    records: &Mutex<BTreeMap<Uuid, T>>,
    id: Uuid,
    revision: u64,
    value: &T,
    stored_revision: impl Fn(&T) -> u64,
) -> Result<(), String> {
    let mut records = records
        .lock()
        .map_err(|_| "agent run record lock poisoned".to_string())?;
    save_versioned_locked(&mut records, id, revision, value, stored_revision)
}

fn save_versioned_locked<T: Clone + PartialEq>(
    records: &mut BTreeMap<Uuid, T>,
    id: Uuid,
    revision: u64,
    value: &T,
    stored_revision: impl Fn(&T) -> u64,
) -> Result<(), String> {
    match records.get(&id) {
        Some(existing) if existing == value => Ok(()),
        Some(existing) if stored_revision(existing).checked_add(1) == Some(revision) => {
            records.insert(id, value.clone());
            Ok(())
        }
        Some(existing) => Err(format!(
            "stale agent run revision: stored {}, supplied {revision}",
            stored_revision(existing)
        )),
        None if revision == 0 => {
            records.insert(id, value.clone());
            Ok(())
        }
        None => Err(format!(
            "new agent run record {id} must start at revision 0"
        )),
    }
}

fn load_record<T: Clone>(
    records: &Mutex<BTreeMap<Uuid, T>>,
    id: &Uuid,
    name: &str,
) -> Result<Option<T>, String> {
    Ok(records
        .lock()
        .map_err(|_| format!("{name} lock poisoned"))?
        .get(id)
        .cloned())
}

fn save_once_locked<T: Clone + PartialEq>(
    records: &mut BTreeMap<Uuid, T>,
    id: Uuid,
    value: &T,
    name: &str,
) -> Result<(), String> {
    match records.get(&id) {
        Some(existing) if existing == value => Ok(()),
        Some(_) => Err(format!("conflicting {name}: {id}")),
        None => {
            records.insert(id, value.clone());
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{canonicalize, create_proof, generate_keypair, ArtifactKind};
    use chrono::Duration;
    use serde_json::json;

    fn live_start_bundle(
        claim: LiveRunStartClaim,
    ) -> (LiveRunStartClaim, AgentRun, AgentCheckpoint, AgentRunEvent) {
        let now = Utc::now();
        let agent_id = Uuid::now_v7();
        let mut run = AgentRun::new_for_agent(
            PrincipalId::now(),
            agent_id,
            AgentRunMode::Session,
            "Publish one preview",
            now,
        )
        .unwrap();
        run.start(now).unwrap();
        let process_epoch_id = Uuid::now_v7();
        let state = json!({
            "kind": "agent_runtime_v2",
            "runtime": {
                "schema": "proof-agent-runtime-state/v2",
                "agent_id": agent_id,
                "run_id": run.id,
                "started_at": now,
                "process_epoch_id": process_epoch_id,
                "policy_binding": {"resolved_policy_digest": claim.setup_digest},
                "authority": {"delegation_id": Uuid::now_v7()},
            }
        });
        let checkpoint = AgentCheckpoint::create(run.id, 0, state, now).unwrap();
        let event = AgentRunEvent::create(
            run.id,
            0,
            AgentRunEventKind::Started,
            json!({
                "live": true,
                "schema": "proof-agent-runtime-state/v2",
                "process_epoch_id": process_epoch_id,
                "policy_binding": {"resolved_policy_digest": claim.setup_digest},
                "authority": checkpoint.state["runtime"]["authority"].clone(),
            }),
            now,
        )
        .unwrap();
        (claim, run, checkpoint, event)
    }

    fn live_claim(seed: &str) -> LiveRunStartClaim {
        let readiness = digest(
            ArtifactKind::Generic,
            &canonicalize(&json!({"readiness": seed})).unwrap(),
        );
        let setup = digest(
            ArtifactKind::Generic,
            &canonicalize(&json!({"setup": seed})).unwrap(),
        );
        LiveRunStartClaim::create(readiness, setup).unwrap()
    }

    #[test]
    fn live_start_claim_is_strict_and_recording_store_replays_exact_bundle() {
        assert!(LiveRunStartClaim::create(
            ContentDigest::from_bytes([0; 32]),
            live_claim("valid").setup_digest,
        )
        .is_err());
        assert!(serde_json::from_value::<LiveRunStartClaim>(json!({
            "schema": LIVE_RUN_START_CLAIM_SCHEMA,
            "readiness_binding_digest": live_claim("strict").readiness_binding_digest,
            "setup_digest": live_claim("strict").setup_digest,
            "unknown": true,
        }))
        .is_err());

        let store = RecordingAgentRunStore::default();
        let claim = live_claim("one");
        let (_, run, checkpoint, event) = live_start_bundle(claim.clone());
        assert_eq!(
            store
                .claim_live_run_start(&claim, &run, &checkpoint, &event)
                .unwrap(),
            LiveRunStartClaimResult::Acquired
        );
        assert_eq!(store.load_agent_run(&run.id).unwrap(), Some(run.clone()));
        assert_eq!(
            store.list_agent_checkpoints(&run.id).unwrap(),
            vec![checkpoint.clone()]
        );
        assert_eq!(
            AgentStore::list_agent_run_events(&store, &run.id).unwrap(),
            vec![event]
        );

        let (_, proposed_run, proposed_checkpoint, proposed_event) =
            live_start_bundle(claim.clone());
        assert_eq!(
            store
                .claim_live_run_start(&claim, &proposed_run, &proposed_checkpoint, &proposed_event,)
                .unwrap(),
            LiveRunStartClaimResult::Existing(run.id)
        );
        assert_eq!(store.list_agent_runs().unwrap(), vec![run]);
    }

    #[test]
    fn recording_live_start_claim_rejects_cross_binding_and_tampered_history() {
        let store = RecordingAgentRunStore::default();
        let claim = live_claim("original");
        let (_, run, checkpoint, event) = live_start_bundle(claim.clone());
        assert_eq!(
            store
                .claim_live_run_start(&claim, &run, &checkpoint, &event)
                .unwrap(),
            LiveRunStartClaimResult::Acquired
        );

        let mut changed_setup = live_claim("changed-setup");
        changed_setup.readiness_binding_digest = claim.readiness_binding_digest;
        let (_, changed_run, changed_checkpoint, changed_event) =
            live_start_bundle(changed_setup.clone());
        assert_eq!(
            store
                .claim_live_run_start(
                    &changed_setup,
                    &changed_run,
                    &changed_checkpoint,
                    &changed_event,
                )
                .unwrap(),
            LiveRunStartClaimResult::Conflict
        );

        let mut changed_binding = live_claim("changed-binding");
        changed_binding.setup_digest = claim.setup_digest;
        let (_, changed_run, changed_checkpoint, changed_event) =
            live_start_bundle(changed_binding.clone());
        assert_eq!(
            store
                .claim_live_run_start(
                    &changed_binding,
                    &changed_run,
                    &changed_checkpoint,
                    &changed_event,
                )
                .unwrap(),
            LiveRunStartClaimResult::Conflict
        );

        store.events.lock().unwrap().remove(&event.id);
        let (_, proposed_run, proposed_checkpoint, proposed_event) =
            live_start_bundle(claim.clone());
        assert!(store
            .claim_live_run_start(&claim, &proposed_run, &proposed_checkpoint, &proposed_event,)
            .is_err());
    }

    #[test]
    fn run_lifecycle_waits_resumes_and_retries() {
        let now = Utc::now();
        let mut run = AgentRun::new(
            PrincipalId::now(),
            AgentRunMode::Session,
            "Publish a release",
            now,
        )
        .unwrap();
        run.start(now + Duration::seconds(1)).unwrap();
        run.wait_for_input(now + Duration::seconds(2)).unwrap();
        run.resume(now + Duration::seconds(3)).unwrap();
        run.fail(now + Duration::seconds(4)).unwrap();
        run.resume(now + Duration::seconds(5)).unwrap();
        assert_eq!(run.retry_count, 1);
        run.succeed(now + Duration::seconds(6)).unwrap();
        assert_eq!(run.status, AgentRunStatus::Succeeded);
        assert_eq!(run.revision, 6);
        assert!(run.completed_at.is_some());
        assert!(run.start(now).is_err());
    }

    #[test]
    fn run_can_bind_to_an_agent_definition() {
        let agent_id = Uuid::now_v7();
        let run = AgentRun::new_for_agent(
            PrincipalId::now(),
            agent_id,
            AgentRunMode::Session,
            "Execute a goal",
            Utc::now(),
        )
        .unwrap();

        assert_eq!(run.agent_id, Some(agent_id));
        assert_eq!(run.revision, 0);
    }

    #[test]
    fn step_retry_preserves_exact_input_contract() {
        let now = Utc::now();
        let input = json!({"release_id": "release-1"});
        let mut step =
            AgentRunStep::new(Uuid::now_v7(), 0, "release.publish", "v1", &input, now).unwrap();
        step.start(now).unwrap();
        step.fail("timeout", now).unwrap();
        let retry = step.retry(now + Duration::seconds(1)).unwrap();
        assert_eq!(retry.retry_of, Some(step.id));
        assert_eq!(retry.attempt, 2);
        assert_eq!(retry.input_digest, step.input_digest);
        assert_eq!(retry.status, AgentRunStepStatus::Pending);
    }

    #[test]
    fn successful_step_requires_matching_proof() {
        let now = Utc::now();
        let keypair = generate_keypair();
        let input = json!({"release_id": "release-1"});
        let output = json!({"published": true});
        let mut step =
            AgentRunStep::new(Uuid::now_v7(), 0, "release.publish", "v1", &input, now).unwrap();
        step.start(now).unwrap();
        let proof = create_proof(
            keypair.principal_id,
            None,
            "release.publish::v1",
            &input,
            &output,
            now,
            &keypair,
        )
        .unwrap();
        step.succeed(output.clone(), proof, now).unwrap();
        assert_eq!(step.output, Some(output));

        let mut mismatched =
            AgentRunStep::new(Uuid::now_v7(), 0, "release.publish", "v1", &input, now).unwrap();
        mismatched.start(now).unwrap();
        let wrong_input = json!({"release_id": "different"});
        let proof = create_proof(
            keypair.principal_id,
            None,
            "release.publish::v1",
            &wrong_input,
            &json!({"published": true}),
            now,
            &keypair,
        )
        .unwrap();
        assert_eq!(
            mismatched.succeed(json!({"published": true}), proof, now),
            Err(AgentRunError::ProofMismatch)
        );

        let mut wrong_operation =
            AgentRunStep::new(Uuid::now_v7(), 0, "release.publish", "v1", &input, now).unwrap();
        wrong_operation.start(now).unwrap();
        let proof = create_proof(
            keypair.principal_id,
            None,
            "release.preview::v1",
            &input,
            &json!({"published": true}),
            now,
            &keypair,
        )
        .unwrap();
        assert_eq!(
            wrong_operation.succeed(json!({"published": true}), proof, now),
            Err(AgentRunError::ProofMismatch)
        );

        let mut wrong_version =
            AgentRunStep::new(Uuid::now_v7(), 0, "release.publish", "v1", &input, now).unwrap();
        wrong_version.start(now).unwrap();
        let proof = create_proof(
            keypair.principal_id,
            None,
            "release.publish::v2",
            &input,
            &json!({"published": true}),
            now,
            &keypair,
        )
        .unwrap();
        assert_eq!(
            wrong_version.succeed(json!({"published": true}), proof, now),
            Err(AgentRunError::ProofMismatch)
        );
    }

    #[test]
    fn checkpoints_and_terminal_evaluations_validate() {
        let now = Utc::now();
        let mut run =
            AgentRun::new(PrincipalId::now(), AgentRunMode::OneShot, "Check", now).unwrap();
        let checkpoint = AgentCheckpoint::create(run.id, 0, json!({"cursor": 3}), now).unwrap();
        let canonical = canonicalize(&checkpoint.state).unwrap();
        assert_eq!(
            checkpoint.state_digest,
            digest(ArtifactKind::AgentCheckpoint, &canonical)
        );
        assert_eq!(
            AgentRunEvaluation::create(
                &run,
                "policy-v1",
                AgentEvaluationOutcome::Passed,
                Some(9_500),
                json!({}),
                None,
                now,
            ),
            Err(AgentRunError::RunNotTerminal)
        );
        run.start(now).unwrap();
        run.succeed(now).unwrap();
        let evaluation = AgentRunEvaluation::create(
            &run,
            "policy-v1",
            AgentEvaluationOutcome::Passed,
            Some(9_500),
            json!({"proof_valid": true}),
            Some("meets policy".to_string()),
            now,
        )
        .unwrap();
        assert_eq!(evaluation.run_id, run.id);
    }

    #[test]
    fn recording_store_enforces_revisions_and_round_trips() {
        let now = Utc::now();
        let store = RecordingAgentRunStore::default();
        let mut run = AgentRun::new(PrincipalId::now(), AgentRunMode::Session, "Run", now).unwrap();
        store.save_agent_run(&run).unwrap();
        let stale = run.clone();
        run.start(now).unwrap();
        store.save_agent_run(&run).unwrap();
        assert!(store.save_agent_run(&stale).is_err());

        let input = json!({"a": 1});
        let mut step = AgentRunStep::new(run.id, 0, "test.echo", "v1", &input, now).unwrap();
        store.save_agent_run_step(&step).unwrap();
        step.start(now).unwrap();
        store.save_agent_run_step(&step).unwrap();
        let request_id = Uuid::now_v7();
        step.wait_for_approval(request_id, now).unwrap();
        store.save_agent_run_step(&step).unwrap();
        assert_eq!(
            store.find_agent_run_step_by_approval(&request_id).unwrap(),
            Some(step.clone())
        );

        let checkpoint = AgentCheckpoint::create(run.id, 0, json!({"a": 1}), now).unwrap();
        store.save_agent_checkpoint(&checkpoint).unwrap();
        assert_eq!(
            store.list_agent_checkpoints(&run.id).unwrap(),
            vec![checkpoint]
        );
        assert_eq!(store.list_agent_run_steps(&run.id).unwrap(), vec![step]);
        assert_eq!(store.list_agent_runs().unwrap(), vec![run]);
    }

    #[test]
    fn step_cannot_replace_an_assigned_approval_request() {
        let now = Utc::now();
        let mut step = AgentRunStep::new(
            Uuid::now_v7(),
            0,
            "release.publish",
            "v1",
            &json!({"environment": "preview"}),
            now,
        )
        .unwrap();
        step.start(now).unwrap();
        let original = Uuid::now_v7();
        step.wait_for_approval(original, now).unwrap();
        step.resume_from_approval(now).unwrap();

        assert_eq!(
            step.wait_for_approval(Uuid::now_v7(), now),
            Err(AgentRunError::ApprovalBindingImmutable)
        );
        assert_eq!(step.approval_request_id, Some(original));
        assert_eq!(step.status, AgentRunStepStatus::Running);
    }

    #[test]
    fn recording_store_rejects_duplicate_and_ambiguous_approval_bindings() {
        let now = Utc::now();
        let store = RecordingAgentRunStore::default();
        let run_id = Uuid::now_v7();
        let request_id = Uuid::now_v7();
        let input = json!({"release": "preview"});

        let mut first = AgentRunStep::new(run_id, 0, "release.publish", "v1", &input, now).unwrap();
        store.save_agent_run_step(&first).unwrap();
        first.start(now).unwrap();
        store.save_agent_run_step(&first).unwrap();
        first.wait_for_approval(request_id, now).unwrap();
        store.save_agent_run_step(&first).unwrap();

        let mut second =
            AgentRunStep::new(run_id, 1, "release.publish", "v1", &input, now).unwrap();
        store.save_agent_run_step(&second).unwrap();
        second.start(now).unwrap();
        store.save_agent_run_step(&second).unwrap();
        second.wait_for_approval(request_id, now).unwrap();
        let error = store.save_agent_run_step(&second).unwrap_err();
        assert!(error.contains("already bound"));

        store
            .steps
            .lock()
            .unwrap()
            .insert(second.id, second.clone());
        let error = store
            .find_agent_run_step_by_approval(&request_id)
            .unwrap_err();
        assert!(error.contains("multiple agent run steps"));
    }
}
