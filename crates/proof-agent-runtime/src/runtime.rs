//! Durable planner/tool loop backed by Proof run and approval stores.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use proof_kernel::{
    canonicalize, digest, principal_from_keypair, AgentCheckpoint, AgentDefinition,
    AgentEvaluationOutcome, AgentRun, AgentRunError, AgentRunEvaluation, AgentRunEvent,
    AgentRunEventKind, AgentRunMode, AgentRunStatus, AgentRunStep, AgentRunStepStatus,
    AgentRunStore, AgentStore, ApprovalError, ApprovalExecution, ApprovalGrant, ApprovalOutcome,
    ApprovalRequest, ApprovalStore, ArtifactKind, ContentDigest, Delegation, DelegationChain,
    ExecutionContext, ExecutionEngine, ExecutionOutcome, Governance, Keypair, PrincipalId,
    PrincipalKind, Proof, Registry, RegistryEntry, SignedApprovalRequest,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;
use uuid::Uuid;

use crate::model::{
    AgentFunctionTool, ModelDecision, ModelGateway, ModelInput, ModelTurnRequest, ModelUsage,
};
use crate::trace_eval::{durable_principal_binding, TraceEvaluationPolicy};

const RUNTIME_CHECKPOINT_KIND: &str = "agent_runtime_v1";
const LIVE_RUNTIME_CHECKPOINT_KIND: &str = "agent_runtime_v2";
const RUNTIME_EVALUATOR: &str = "proof-agent-runtime/v1";
const LIVE_EVALUATOR: &str = "proof-release-manager-live/v1";
const LIVE_PROVIDER: &str = "openai";
const LIVE_ENDPOINT: &str = "https://api.openai.com/v1/responses";
const LIVE_MODEL: &str = "gpt-5.6-sol";
const LIVE_SERVICE_TIER: &str = "default";
const LIVE_TOOL_NAME: &str = "proof_content_v2_release_publish";
const LIVE_POLICY_SOURCE: &str = include_str!("../../../evals/release-manager-live-v1.json");
const PREVIEW_POLICY_SOURCE: &str = include_str!("../../../evals/release-manager-preview-v1.json");

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingToolCall {
    pub call_id: String,
    pub tool_name: String,
    pub operation: String,
    pub version: String,
    pub arguments: Value,
    pub step_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_request_id: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentRuntimeState {
    pub agent_id: Uuid,
    pub started_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_response_id: Option<String>,
    pub next_input: ModelInput,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_tool: Option<PendingToolCall>,
    pub model_calls: u32,
    pub tool_attempts: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_microusd: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_output: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_error: Option<String>,
}

/// Version-neutral, validated runtime evidence used by human approval review
/// surfaces. The projection intentionally exposes only the fields needed to
/// bind displayed arguments to the durable run, step, and signed request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeApprovalContext {
    pub checkpoint_kind: String,
    pub run_id: Uuid,
    pub agent_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_approver_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_tool: Option<PendingToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sealed_approval_request: Option<SignedApprovalRequest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sealed_step: Option<AgentRunStep>,
}

/// Event-independent, typed projection of the newest checkpoint in one
/// complete native runtime history. This is safe for diagnostic surfaces in
/// crash windows where a durable checkpoint intentionally precedes its causal
/// event, but it does not make a pending approval actionable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeStateView {
    pub checkpoint_kind: String,
    pub state: Value,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GenericModelRequestedEvent {
    model: String,
    model_call: u32,
    previous_response_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GenericModelRespondedEvent {
    response_id: String,
    decision: ModelDecision,
    usage: ModelUsage,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct GenericToolRequestedEvent {
    step_id: Uuid,
    call_id: String,
    tool: String,
    operation: String,
    version: String,
    arguments: Value,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GenericToolSucceededEvent {
    step_id: Uuid,
    call_id: String,
    operation: String,
    version: String,
    proof_id: Uuid,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GenericToolFailedEvent {
    step_id: Uuid,
    call_id: String,
    error: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GenericApprovalRequiredEvent {
    step_id: Uuid,
    request_id: Uuid,
    operation: String,
    version: String,
    expires_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GenericApprovalResumedEvent {
    step_id: Uuid,
    request_id: Uuid,
    decided_by: PrincipalId,
    outcome: ApprovalOutcome,
}

#[derive(Debug, Clone)]
struct GenericToolRequestEvidence {
    sequence: u32,
    data: GenericToolRequestedEvent,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum AgentRuntimeOutcome {
    Completed {
        run: AgentRun,
        output: String,
        evaluation: AgentRunEvaluation,
    },
    WaitingForApproval {
        run: AgentRun,
        step: AgentRunStep,
        request: SignedApprovalRequest,
    },
    Failed {
        run: AgentRun,
        error: String,
        evaluation: AgentRunEvaluation,
    },
}

#[derive(Debug, Error)]
pub enum AgentRuntimeError {
    #[error("agent runtime identity must be an agent principal")]
    IdentityMustBeAgent,
    #[error("agent definition not found: {0}")]
    AgentNotFound(Uuid),
    #[error("agent run not found: {0}")]
    RunNotFound(Uuid),
    #[error("agent run {0} is not bound to an agent definition")]
    RunMissingAgent(Uuid),
    #[error("agent run {run_id} belongs to actor {actual}, not runtime actor {expected}")]
    ActorMismatch {
        run_id: Uuid,
        actual: String,
        expected: String,
    },
    #[error("agent provider {configured} does not match runtime provider {runtime}")]
    ProviderMismatch { configured: String, runtime: String },
    #[error("agent tool is not registered: {operation}::{version}")]
    ToolNotRegistered { operation: String, version: String },
    #[error("agent tools resolve to a duplicate model function name: {0}")]
    DuplicateToolName(String),
    #[error("agent run {0} has no runtime checkpoint")]
    MissingCheckpoint(Uuid),
    #[error("agent run {0} has an invalid runtime checkpoint")]
    InvalidCheckpoint(Uuid),
    #[error("agent run {run_id} cannot resume from status {status:?}")]
    RunNotResumable {
        run_id: Uuid,
        status: AgentRunStatus,
    },
    #[error("agent run state is inconsistent: {0}")]
    InconsistentState(String),
    #[error("agent store failed: {0}")]
    Store(String),
    #[error("agent run contract failed: {0}")]
    Run(#[from] AgentRunError),
    #[error("approval contract failed: {0}")]
    Approval(#[from] ApprovalError),
    #[error("registry schema failed: {0}")]
    Schema(String),
    #[error("live run setup rejected: {0}")]
    LiveSetup(String),
    #[error("live provider gateway factory failed: {0}")]
    GatewayFactory(String),
}

/// Credential-free intent supplied by the CLI after its deterministic local
/// preflight. The runtime alone allocates the start run ID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum LiveRunIntent {
    Start { agent_id: Uuid, goal: String },
    Resume { run_id: Uuid },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveBindingInputs {
    pub preflight_evidence_digest: ContentDigest,
    pub agent_principal_id: PrincipalId,
    pub approver_principal_id: PrincipalId,
    pub delegation_id: Uuid,
    pub delegation_digest: ContentDigest,
    pub edition_id: Uuid,
    pub manifest_digest: String,
    pub idempotency_key: Uuid,
    pub version_label: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LiveAuthoritySetup {
    pub delegation: Delegation,
    pub delegation_digest: ContentDigest,
    pub delegation_chain: DelegationChain,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LivePolicyMaterial {
    pub template: Value,
    pub template_policy_digest: ContentDigest,
    pub binding_inputs: LiveBindingInputs,
    pub check_set_digest: ContentDigest,
    pub tamper_vector_set_digest: ContentDigest,
    pub pricing_schedule_digest: ContentDigest,
    pub instructions_digest: ContentDigest,
    pub initial_input_digest: ContentDigest,
    pub parameters_schema_digest: ContentDigest,
    pub tool_declaration_digest: ContentDigest,
    pub tool_set_digest: ContentDigest,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveRunSetup {
    pub intent: LiveRunIntent,
    pub process_epoch_id: Uuid,
    pub preflight_evidence: Value,
    pub preflight_evidence_digest: ContentDigest,
    pub authority: LiveAuthoritySetup,
    pub policy: LivePolicyMaterial,
}

/// The immutable deterministic assertion is deliberately parsed separately
/// from the live template.  In particular its `policy_digest` is the digest of
/// the preview policy, never of the live template.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PreflightEvidence {
    schema: String,
    policy_path: String,
    policy_digest: ContentDigest,
    trace_digest: ContentDigest,
    evaluator: String,
    run_id: Uuid,
    evaluation_id: Uuid,
    evaluation_created_at: DateTime<Utc>,
    outcome: String,
    score_bps: u16,
    passed_checks: u16,
    total_checks: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StrictDelegationScope {
    allowed_operations: Option<Vec<String>>,
    allowed_domains: Option<Vec<String>>,
    resource_scope: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StrictDelegation {
    id: Uuid,
    issuer: PrincipalId,
    recipient: PrincipalId,
    allowed_actions: Vec<String>,
    resource_scope: Vec<String>,
    scope: StrictDelegationScope,
    valid_from: DateTime<Utc>,
    valid_until: DateTime<Utc>,
    revoked: bool,
}

impl From<&Delegation> for StrictDelegation {
    fn from(value: &Delegation) -> Self {
        Self {
            id: value.id,
            issuer: value.issuer,
            recipient: value.recipient,
            allowed_actions: value.allowed_actions.clone(),
            resource_scope: value.resource_scope.clone(),
            scope: StrictDelegationScope {
                allowed_operations: value.scope.allowed_operations.clone(),
                allowed_domains: value.scope.allowed_domains.clone(),
                resource_scope: value.scope.resource_scope.clone(),
            },
            valid_from: value.valid_from,
            valid_until: value.valid_until,
            revoked: value.revoked,
        }
    }
}

impl From<StrictDelegation> for Delegation {
    fn from(value: StrictDelegation) -> Self {
        Self {
            id: value.id,
            issuer: value.issuer,
            recipient: value.recipient,
            allowed_actions: value.allowed_actions,
            resource_scope: value.resource_scope,
            scope: proof_kernel::delegation::DelegationScope {
                allowed_operations: value.scope.allowed_operations,
                allowed_domains: value.scope.allowed_domains,
                resource_scope: value.scope.resource_scope,
            },
            valid_from: value.valid_from,
            valid_until: value.valid_until,
            revoked: value.revoked,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DelegationChainWire {
    root: PrincipalId,
    grants: Vec<StrictDelegation>,
}

impl Serialize for LiveAuthoritySetup {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        #[derive(Serialize)]
        struct Wire {
            delegation: StrictDelegation,
            delegation_digest: ContentDigest,
            delegation_chain: DelegationChainWire,
        }
        Wire {
            delegation: StrictDelegation::from(&self.delegation),
            delegation_digest: self.delegation_digest,
            delegation_chain: DelegationChainWire {
                root: self.delegation_chain.root,
                grants: self
                    .delegation_chain
                    .grants
                    .iter()
                    .map(StrictDelegation::from)
                    .collect(),
            },
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for LiveAuthoritySetup {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            delegation: StrictDelegation,
            delegation_digest: ContentDigest,
            delegation_chain: DelegationChainWire,
        }
        let wire = Wire::deserialize(deserializer)?;
        Ok(Self {
            delegation: Delegation::from(wire.delegation),
            delegation_digest: wire.delegation_digest,
            delegation_chain: DelegationChain {
                root: wire.delegation_chain.root,
                grants: wire
                    .delegation_chain
                    .grants
                    .into_iter()
                    .map(Delegation::from)
                    .collect(),
            },
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelGatewayFactoryContext {
    pub run_id: Uuid,
    pub attempt_id: Uuid,
    pub process_epoch_id: Uuid,
    pub provider: String,
    pub endpoint: String,
    pub requested_model: String,
    pub service_tier: String,
    pub request_body_digest: ContentDigest,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ModelGatewayFactoryError {
    #[error("gateway configuration failed: {0}")]
    Configuration(String),
    #[error("gateway construction failed: {0}")]
    Construction(String),
}

pub trait ModelGatewayFactory: Send + Sync {
    fn create(
        &self,
        context: &ModelGatewayFactoryContext,
    ) -> Result<Arc<dyn ModelGateway>, ModelGatewayFactoryError>;
}

struct FixedModelGatewayFactory {
    gateway: Arc<dyn ModelGateway>,
}

impl ModelGatewayFactory for FixedModelGatewayFactory {
    fn create(
        &self,
        _context: &ModelGatewayFactoryContext,
    ) -> Result<Arc<dyn ModelGateway>, ModelGatewayFactoryError> {
        Ok(self.gateway.clone())
    }
}

struct UnavailableModelGateway;

impl ModelGateway for UnavailableModelGateway {
    fn provider(&self) -> &str {
        "unconfigured"
    }

    fn complete(
        &self,
        _request: &ModelTurnRequest,
    ) -> Result<crate::model::ModelTurn, crate::model::ModelGatewayError> {
        Err(crate::model::ModelGatewayError::Request(
            "no fixed model gateway is configured; use run_live".to_string(),
        ))
    }
}

pub struct AgentRuntime {
    registry: Registry,
    engine: ExecutionEngine,
    identity: Keypair,
    workspace_path: PathBuf,
    agent_store: Arc<dyn AgentStore>,
    run_store: Arc<dyn AgentRunStore>,
    approval_store: Arc<dyn ApprovalStore>,
    model: Arc<dyn ModelGateway>,
    gateway_factory: Arc<dyn ModelGatewayFactory>,
    approval_ttl: Duration,
}

impl AgentRuntime {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        registry: Registry,
        engine: ExecutionEngine,
        identity: Keypair,
        workspace_path: impl Into<PathBuf>,
        agent_store: Arc<dyn AgentStore>,
        run_store: Arc<dyn AgentRunStore>,
        approval_store: Arc<dyn ApprovalStore>,
        model: Arc<dyn ModelGateway>,
    ) -> Result<Self, AgentRuntimeError> {
        let factory: Arc<dyn ModelGatewayFactory> = Arc::new(FixedModelGatewayFactory {
            gateway: model.clone(),
        });
        let mut runtime = Self::new_with_gateway_factory(
            registry,
            engine,
            identity,
            workspace_path,
            agent_store,
            run_store,
            approval_store,
            factory,
        )?;
        runtime.model = model;
        Ok(runtime)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_with_gateway_factory(
        registry: Registry,
        engine: ExecutionEngine,
        identity: Keypair,
        workspace_path: impl Into<PathBuf>,
        agent_store: Arc<dyn AgentStore>,
        run_store: Arc<dyn AgentRunStore>,
        approval_store: Arc<dyn ApprovalStore>,
        gateway_factory: Arc<dyn ModelGatewayFactory>,
    ) -> Result<Self, AgentRuntimeError> {
        if identity.kind != PrincipalKind::Agent {
            return Err(AgentRuntimeError::IdentityMustBeAgent);
        }
        // Legacy callers retain the fixed-gateway constructor. The factory is
        // intentionally not invoked here, so live registration is secret-free.
        let model: Arc<dyn ModelGateway> = Arc::new(UnavailableModelGateway);
        Ok(Self {
            registry,
            engine,
            identity,
            workspace_path: workspace_path.into(),
            agent_store,
            run_store,
            approval_store,
            model,
            gateway_factory,
            approval_ttl: Duration::minutes(15),
        })
    }

    pub fn start(
        &self,
        agent_id: Uuid,
        goal: impl Into<String>,
    ) -> Result<AgentRuntimeOutcome, AgentRuntimeError> {
        let agent = self.load_agent(agent_id)?;
        let tools = self.resolve_tools(&agent)?;
        let now = Utc::now();
        let run = AgentRun::new_for_agent(
            self.identity.principal_id,
            agent.id,
            AgentRunMode::Session,
            goal,
            now,
        )?;
        self.save_run(&run)?;
        let (run, state) = self.reconcile_generic_bootstrap(run, &agent)?;
        self.drive(run, agent, tools, state)
    }

    pub fn resume(&self, run_id: Uuid) -> Result<AgentRuntimeOutcome, AgentRuntimeError> {
        let run = self
            .run_store
            .load_agent_run(&run_id)
            .map_err(AgentRuntimeError::Store)?
            .ok_or(AgentRuntimeError::RunNotFound(run_id))?;
        if run.actor != self.identity.principal_id {
            return Err(AgentRuntimeError::ActorMismatch {
                run_id,
                actual: run.actor.to_string(),
                expected: self.identity.principal_id.to_string(),
            });
        }
        let agent_id = run
            .agent_id
            .ok_or(AgentRuntimeError::RunMissingAgent(run_id))?;
        let agent = self.load_agent(agent_id)?;
        // Resolve every immutable tool contract before bootstrap recovery may
        // write a run revision, checkpoint, or event. An unstartable agent
        // must remain an untouched partial run.
        let tools = self.resolve_tools(&agent)?;
        match run.status {
            AgentRunStatus::Queued | AgentRunStatus::Running => {
                let (run, state) = self.reconcile_generic_bootstrap(run, &agent)?;
                self.drive(run, agent, tools, state)
            }
            AgentRunStatus::Succeeded
            | AgentRunStatus::Failed
            | AgentRunStatus::WaitingForInput => {
                let state = self.state(run_id)?;
                if state.agent_id != agent_id {
                    return Err(AgentRuntimeError::InvalidCheckpoint(run_id));
                }
                match run.status {
                    AgentRunStatus::Succeeded | AgentRunStatus::Failed => {
                        self.terminal_outcome(run, state)
                    }
                    AgentRunStatus::WaitingForInput => {
                        let checkpoints = self
                            .run_store
                            .list_agent_checkpoints(&run.id)
                            .map_err(AgentRuntimeError::Store)?;
                        let events = self
                            .agent_store
                            .list_agent_run_events(&run.id)
                            .map_err(AgentRuntimeError::Store)?;
                        let expected_state = generic_initial_state(&run, &agent);
                        let expected_started = generic_started_event(&agent, &run);
                        self.validate_complete_generic_bootstrap(
                            &run,
                            &expected_state,
                            &expected_started,
                            &checkpoints,
                            &events,
                        )?;
                        self.validate_generic_resume_evidence(
                            &run,
                            &agent,
                            &state,
                            &checkpoints,
                            &events,
                        )?;
                        self.drive(run, agent, tools, state)
                    }
                    _ => unreachable!("status group is exhaustive above"),
                }
            }
            status => Err(AgentRuntimeError::RunNotResumable { run_id, status }),
        }
    }

    /// Reconciles only the exact generic bootstrap emitted by `start`.
    ///
    /// The Queued/Running run row, checkpoint zero, and Started event zero are
    /// separate durable barriers.  A process may disappear between them, but
    /// recovery never allocates a replacement run or infers progress from
    /// later evidence.  Once both barriers are exact, ordinary runtime
    /// recovery may continue from the latest v1 checkpoint.
    fn reconcile_generic_bootstrap(
        &self,
        mut run: AgentRun,
        agent: &AgentDefinition,
    ) -> Result<(AgentRun, AgentRuntimeState), AgentRuntimeError> {
        self.validate_generic_run_identity(&run, agent)?;
        self.reread_exact_run(&run)?;

        let expected_state = generic_initial_state(&run, agent);
        let expected_started = generic_started_event(agent, &run);
        let mut checkpoints = self
            .run_store
            .list_agent_checkpoints(&run.id)
            .map_err(AgentRuntimeError::Store)?;
        let mut events = self
            .agent_store
            .list_agent_run_events(&run.id)
            .map_err(AgentRuntimeError::Store)?;

        let checkpoint_present = checkpoints.first().is_some_and(|checkpoint| {
            validate_generic_initial_checkpoint(&run, &expected_state, checkpoint).is_ok()
        });
        let started_present = events.first().is_some_and(|event| {
            validate_generic_started_event(run.id, &expected_started, checkpoints.first(), event)
                .is_ok()
        });

        if checkpoint_present && started_present {
            self.validate_complete_generic_bootstrap(
                &run,
                &expected_state,
                &expected_started,
                &checkpoints,
                &events,
            )?;
            let state = self.state(run.id)?;
            if state.agent_id != agent.id {
                return Err(AgentRuntimeError::InvalidCheckpoint(run.id));
            }
            self.validate_generic_resume_evidence(&run, agent, &state, &checkpoints, &events)?;
            return Ok((run, state));
        }

        self.validate_pristine_generic_bootstrap(
            &run,
            &expected_state,
            &expected_started,
            &checkpoints,
            &events,
        )?;

        if run.status == AgentRunStatus::Queued {
            run.start(run.created_at)?;
            self.save_run(&run)?;
            self.reread_exact_run(&run)?;
        }

        if checkpoints.is_empty() {
            self.save_state(run.id, &expected_state)?;
            checkpoints = self
                .run_store
                .list_agent_checkpoints(&run.id)
                .map_err(AgentRuntimeError::Store)?;
            if checkpoints.len() != 1 {
                return Err(AgentRuntimeError::InconsistentState(format!(
                    "agent run {} bootstrap checkpoint is not exact-one",
                    run.id
                )));
            }
            validate_generic_initial_checkpoint(&run, &expected_state, &checkpoints[0])?;
        }

        if events.is_empty() {
            self.append_event(run.id, AgentRunEventKind::Started, expected_started.clone())?;
        }

        // Re-read both immutable barriers together immediately before drive.
        // This is deliberately more than trusting the successful save calls:
        // a split store or interrupted process must expose the same exact
        // checkpoint/event pair through its public read APIs.
        checkpoints = self
            .run_store
            .list_agent_checkpoints(&run.id)
            .map_err(AgentRuntimeError::Store)?;
        events = self
            .agent_store
            .list_agent_run_events(&run.id)
            .map_err(AgentRuntimeError::Store)?;
        self.validate_complete_generic_bootstrap(
            &run,
            &expected_state,
            &expected_started,
            &checkpoints,
            &events,
        )?;
        let reread_state = self.state(run.id)?;
        if reread_state != expected_state {
            return Err(AgentRuntimeError::InvalidCheckpoint(run.id));
        }
        self.validate_generic_resume_evidence(&run, agent, &reread_state, &checkpoints, &events)?;
        Ok((run, reread_state))
    }

    fn validate_generic_run_identity(
        &self,
        run: &AgentRun,
        agent: &AgentDefinition,
    ) -> Result<(), AgentRuntimeError> {
        let observed_at = Utc::now();
        if run.actor != self.identity.principal_id
            || run.agent_id != Some(agent.id)
            || run.mode != AgentRunMode::Session
            || run.goal.trim().is_empty()
            || run.id.get_version_num() != 7
            || agent.id.get_version_num() != 7
            || agent.created_at > run.created_at
            || run.updated_at < run.created_at
            || run.created_at > observed_at
            || run.updated_at > observed_at
        {
            return Err(AgentRuntimeError::InconsistentState(format!(
                "agent run {} does not match a generic runtime start",
                run.id
            )));
        }
        Ok(())
    }

    fn validate_pristine_generic_bootstrap(
        &self,
        run: &AgentRun,
        expected_state: &AgentRuntimeState,
        expected_started: &Value,
        checkpoints: &[AgentCheckpoint],
        events: &[AgentRunEvent],
    ) -> Result<(), AgentRuntimeError> {
        let exact_run_chronology = match run.status {
            AgentRunStatus::Queued => {
                run.revision == 0
                    && run.retry_count == 0
                    && run.updated_at == run.created_at
                    && run.completed_at.is_none()
            }
            AgentRunStatus::Running => {
                run.revision == 1
                    && run.retry_count == 0
                    && run.updated_at == run.created_at
                    && run.completed_at.is_none()
            }
            _ => false,
        };
        if !exact_run_chronology {
            return Err(AgentRuntimeError::InconsistentState(format!(
                "agent run {} has impossible bootstrap run chronology",
                run.id
            )));
        }

        let steps = self
            .run_store
            .list_agent_run_steps(&run.id)
            .map_err(AgentRuntimeError::Store)?;
        let evaluations = self
            .run_store
            .list_agent_run_evaluations(&run.id)
            .map_err(AgentRuntimeError::Store)?;
        self.reject_bound_bootstrap_approval_evidence(run.id)?;
        if !steps.is_empty() || !evaluations.is_empty() {
            return Err(AgentRuntimeError::InconsistentState(format!(
                "agent run {} has later step or evaluation evidence during bootstrap",
                run.id
            )));
        }

        match checkpoints {
            [] => {
                if !events.is_empty() {
                    return Err(AgentRuntimeError::InconsistentState(format!(
                        "agent run {} has event evidence before its initial checkpoint",
                        run.id
                    )));
                }
            }
            [checkpoint] => {
                validate_generic_initial_checkpoint(run, expected_state, checkpoint)?;
                if run.status != AgentRunStatus::Running {
                    return Err(AgentRuntimeError::InconsistentState(format!(
                        "queued agent run {} already has a bootstrap checkpoint",
                        run.id
                    )));
                }
                if !events.is_empty() {
                    let exact_started = events.len() == 1
                        && validate_generic_started_event(
                            run.id,
                            expected_started,
                            Some(checkpoint),
                            &events[0],
                        )
                        .is_ok();
                    if exact_started {
                        return Err(AgentRuntimeError::InconsistentState(format!(
                            "agent run {} complete bootstrap was classified as partial",
                            run.id
                        )));
                    }
                    return Err(AgentRuntimeError::InconsistentState(format!(
                        "agent run {} has conflicting bootstrap event evidence",
                        run.id
                    )));
                }
            }
            _ => {
                return Err(AgentRuntimeError::InconsistentState(format!(
                    "agent run {} has duplicate or later bootstrap checkpoints",
                    run.id
                )));
            }
        }
        Ok(())
    }

    fn validate_complete_generic_bootstrap(
        &self,
        run: &AgentRun,
        expected_state: &AgentRuntimeState,
        expected_started: &Value,
        checkpoints: &[AgentCheckpoint],
        events: &[AgentRunEvent],
    ) -> Result<(), AgentRuntimeError> {
        validate_generic_evidence_envelopes(run.id, checkpoints, events)?;
        let checkpoint = checkpoints.first().ok_or_else(|| {
            AgentRuntimeError::InconsistentState(format!(
                "agent run {} is missing bootstrap checkpoint zero",
                run.id
            ))
        })?;
        validate_generic_initial_checkpoint(run, expected_state, checkpoint)?;
        if checkpoints.iter().skip(1).any(|candidate| {
            candidate.state == checkpoint.state
                || validate_generic_initial_checkpoint(run, expected_state, candidate).is_ok()
        }) {
            return Err(AgentRuntimeError::InconsistentState(format!(
                "agent run {} has duplicate initial checkpoints",
                run.id
            )));
        }
        let started = events.first().ok_or_else(|| {
            AgentRuntimeError::InconsistentState(format!(
                "agent run {} is missing Started event zero",
                run.id
            ))
        })?;
        validate_generic_started_event(run.id, expected_started, Some(checkpoint), started)?;
        if events
            .iter()
            .skip(1)
            .any(|event| event.kind == AgentRunEventKind::Started)
        {
            return Err(AgentRuntimeError::InconsistentState(format!(
                "agent run {} has duplicate Started events",
                run.id
            )));
        }
        let evaluations = self
            .run_store
            .list_agent_run_evaluations(&run.id)
            .map_err(AgentRuntimeError::Store)?;
        if !evaluations.is_empty() {
            return Err(AgentRuntimeError::InconsistentState(format!(
                "running agent run {} has bootstrap-time evaluation evidence",
                run.id
            )));
        }
        let steps = self
            .run_store
            .list_agent_run_steps(&run.id)
            .map_err(AgentRuntimeError::Store)?;
        if !matches!(
            run.status,
            AgentRunStatus::Running | AgentRunStatus::WaitingForInput
        ) || run.retry_count != 0
            || run.completed_at.is_some()
        {
            return Err(AgentRuntimeError::InconsistentState(format!(
                "agent run {} has impossible completed-bootstrap chronology",
                run.id
            )));
        }
        if checkpoints.len() == 1 {
            self.reject_bound_bootstrap_approval_evidence(run.id)?;
            if events.len() != 1 || !steps.is_empty() {
                return Err(AgentRuntimeError::InconsistentState(format!(
                    "agent run {} has later evidence without a post-bootstrap checkpoint",
                    run.id
                )));
            }
        }
        Ok(())
    }

    fn validate_generic_resume_evidence(
        &self,
        run: &AgentRun,
        agent: &AgentDefinition,
        state: &AgentRuntimeState,
        checkpoints: &[AgentCheckpoint],
        events: &[AgentRunEvent],
    ) -> Result<(), AgentRuntimeError> {
        let invalid = |detail: &str| {
            AgentRuntimeError::InconsistentState(format!(
                "agent run {} has unproven later generic state: {detail}",
                run.id
            ))
        };
        let steps = self
            .run_store
            .list_agent_run_steps(&run.id)
            .map_err(AgentRuntimeError::Store)?;
        let tool_requests = self.collect_generic_tool_requests(run, agent, events, &steps)?;
        self.validate_generic_approval_chronology(run, state, events, &steps, &tool_requests)?;
        if checkpoints.len() == 1 && events.len() == 1 && steps.is_empty() {
            if *state != generic_initial_state(run, agent) {
                return Err(invalid("exact bootstrap state drifted"));
            }
            return Ok(());
        }

        // A checkpoint without any post-Started event is not proof that the
        // model state machine advanced. The sole recoverable exception is a
        // persisted pre-model terminal error, which cannot dispatch and which
        // carries the terminal-event intent in the checkpoint itself.
        if events.len() == 1 {
            let mut expected = generic_initial_state(run, agent);
            expected.terminal_error = state.terminal_error.clone();
            let terminal_marker = checkpoints
                .last()
                .and_then(|checkpoint| checkpoint.state.get("terminal_event_kind"));
            if checkpoints.len() < 2
                || steps.len() != 0
                || state.terminal_error.is_none()
                || state != &expected
                || terminal_marker.is_none()
            {
                return Err(invalid(
                    "checkpoint advance has no matching immutable event transition",
                ));
            }
            return Ok(());
        }

        let mut events = events.to_vec();
        self.reconcile_missing_generic_tool_result(
            run,
            state,
            checkpoints,
            &steps,
            &tool_requests,
            &mut events,
        )?;

        let mut requested = 0_u32;
        let mut responded = 0_u32;
        let mut awaiting_response = false;
        let mut prior_response_id: Option<String> = None;
        let mut input_tokens = 0_u64;
        let mut output_tokens = 0_u64;
        let mut total_tokens = 0_u64;
        let mut cost_microusd = Some(0_u64);
        let mut pending_decision: Option<ModelDecision> = None;
        let mut tool_requested = 0_u32;
        let mut active_tool: Option<usize> = None;
        let mut latest_next_input = ModelInput::Goal {
            text: run.goal.clone(),
        };

        for event in events.iter().skip(1) {
            match event.kind {
                AgentRunEventKind::ModelRequested => {
                    if awaiting_response || pending_decision.is_some() || active_tool.is_some() {
                        return Err(invalid("model request order is ambiguous"));
                    }
                    let parsed: GenericModelRequestedEvent =
                        serde_json::from_value(event.data.clone())
                            .map_err(|_| invalid("model request event is not exact"))?;
                    requested = requested
                        .checked_add(1)
                        .ok_or_else(|| invalid("model request count overflow"))?;
                    if parsed.model != agent.model
                        || parsed.model_call != requested
                        || parsed.previous_response_id != prior_response_id
                    {
                        return Err(invalid("model request event does not match its lineage"));
                    }
                    awaiting_response = true;
                }
                AgentRunEventKind::ModelResponded => {
                    if !awaiting_response || pending_decision.is_some() {
                        return Err(invalid("model response has no unique request"));
                    }
                    let parsed: GenericModelRespondedEvent =
                        serde_json::from_value(event.data.clone())
                            .map_err(|_| invalid("model response event is not exact"))?;
                    if parsed.response_id.trim().is_empty()
                        || parsed.usage.total_tokens
                            != parsed
                                .usage
                                .input_tokens
                                .checked_add(parsed.usage.output_tokens)
                                .ok_or_else(|| invalid("response token total overflow"))?
                    {
                        return Err(invalid("model response usage is inconsistent"));
                    }
                    responded = responded
                        .checked_add(1)
                        .ok_or_else(|| invalid("model response count overflow"))?;
                    input_tokens = input_tokens
                        .checked_add(parsed.usage.input_tokens)
                        .ok_or_else(|| invalid("input token total overflow"))?;
                    output_tokens = output_tokens
                        .checked_add(parsed.usage.output_tokens)
                        .ok_or_else(|| invalid("output token total overflow"))?;
                    total_tokens = total_tokens
                        .checked_add(parsed.usage.total_tokens)
                        .ok_or_else(|| invalid("token total overflow"))?;
                    cost_microusd = match (cost_microusd, parsed.usage.cost_microusd) {
                        (Some(total), Some(cost)) => Some(
                            total
                                .checked_add(cost)
                                .ok_or_else(|| invalid("cost total overflow"))?,
                        ),
                        _ => None,
                    };
                    prior_response_id = Some(parsed.response_id);
                    pending_decision = Some(parsed.decision);
                    awaiting_response = false;
                }
                AgentRunEventKind::ToolRequested => {
                    if awaiting_response || active_tool.is_some() {
                        return Err(invalid("tool request precedes a committed response"));
                    }
                    let parsed: GenericToolRequestedEvent =
                        serde_json::from_value(event.data.clone())
                            .map_err(|_| invalid("tool request event is not exact"))?;
                    let Some(ModelDecision::ToolCall {
                        call_id,
                        name,
                        arguments,
                    }) = pending_decision.take()
                    else {
                        return Err(invalid("tool request has no model decision"));
                    };
                    if parsed.call_id != call_id
                        || parsed.tool != name
                        || parsed.arguments != arguments
                    {
                        return Err(invalid("tool request differs from the model decision"));
                    }
                    let request_index = usize::try_from(tool_requested)
                        .map_err(|_| invalid("tool request index overflow"))?;
                    let evidence = tool_requests
                        .get(request_index)
                        .ok_or_else(|| invalid("tool request evidence is missing"))?;
                    if evidence.sequence != event.sequence
                        || evidence.data.step_id != parsed.step_id
                        || evidence.data.call_id != parsed.call_id
                        || evidence.data.tool != parsed.tool
                        || evidence.data.operation != parsed.operation
                        || evidence.data.version != parsed.version
                        || evidence.data.arguments != parsed.arguments
                    {
                        return Err(invalid("tool request evidence changed during validation"));
                    }
                    active_tool = Some(request_index);
                    tool_requested = tool_requested
                        .checked_add(1)
                        .ok_or_else(|| invalid("tool request count overflow"))?;
                }
                AgentRunEventKind::ToolSucceeded => {
                    if awaiting_response || pending_decision.is_some() {
                        return Err(invalid("tool success bypasses a model transition"));
                    }
                    let request_index = active_tool
                        .take()
                        .ok_or_else(|| invalid("tool success has no unique request"))?;
                    let request = &tool_requests[request_index];
                    let step = steps
                        .iter()
                        .find(|step| step.id == request.data.step_id)
                        .ok_or_else(|| invalid("tool success step is missing"))?;
                    let Some((kind, expected_data, next_input)) =
                        self.expected_generic_tool_result(run, request, step)?
                    else {
                        return Err(invalid("tool success step is not terminal"));
                    };
                    let parsed: GenericToolSucceededEvent =
                        serde_json::from_value(event.data.clone())
                            .map_err(|_| invalid("tool success event is not exact"))?;
                    if kind != AgentRunEventKind::ToolSucceeded
                        || parsed.step_id != request.data.step_id
                        || parsed.call_id != request.data.call_id
                        || parsed.operation != request.data.operation
                        || parsed.version != request.data.version
                        || parsed.proof_id
                            != step
                                .proof
                                .as_ref()
                                .map(|proof| proof.body.id)
                                .ok_or_else(|| invalid("tool success proof disappeared"))?
                        || event.created_at < step.updated_at
                        || event.data != expected_data
                    {
                        return Err(invalid(
                            "tool success does not match its call, step, and proof",
                        ));
                    }
                    latest_next_input = next_input;
                }
                AgentRunEventKind::ToolFailed => {
                    if awaiting_response || pending_decision.is_some() {
                        return Err(invalid("tool failure bypasses a model transition"));
                    }
                    let request_index = active_tool
                        .take()
                        .ok_or_else(|| invalid("tool failure has no unique request"))?;
                    let request = &tool_requests[request_index];
                    let step = steps
                        .iter()
                        .find(|step| step.id == request.data.step_id)
                        .ok_or_else(|| invalid("tool failure step is missing"))?;
                    let Some((kind, expected_data, next_input)) =
                        self.expected_generic_tool_result(run, request, step)?
                    else {
                        return Err(invalid("tool failure step is not terminal"));
                    };
                    let parsed: GenericToolFailedEvent = serde_json::from_value(event.data.clone())
                        .map_err(|_| invalid("tool failure event is not exact"))?;
                    if kind != AgentRunEventKind::ToolFailed
                        || parsed.step_id != request.data.step_id
                        || parsed.call_id != request.data.call_id
                        || parsed.error.trim().is_empty()
                        || event.created_at < step.updated_at
                        || event.data != expected_data
                    {
                        return Err(invalid(
                            "tool failure does not match its call, step, and error",
                        ));
                    }
                    latest_next_input = next_input;
                }
                AgentRunEventKind::ApprovalRequired | AgentRunEventKind::ApprovalResumed => {
                    if awaiting_response || pending_decision.is_some() || active_tool.is_none() {
                        return Err(invalid("approval event has no active tool transition"));
                    }
                }
                _ => {
                    if awaiting_response || pending_decision.is_some() || active_tool.is_some() {
                        return Err(invalid(
                            "later event bypasses an unfinished model transition",
                        ));
                    }
                }
            }
        }

        if awaiting_response || requested != responded {
            return Err(invalid(
                "model request may have been dispatched without a durable response",
            ));
        }
        if state.model_calls != requested
            || state.input_tokens != input_tokens
            || state.output_tokens != output_tokens
            || state.total_tokens != total_tokens
            || state.cost_microusd != cost_microusd
            || state.previous_response_id != prior_response_id
            || state.tool_attempts != tool_requested
            || steps.len() != tool_requested as usize
            || state.next_input != latest_next_input
        {
            return Err(invalid("checkpoint counters do not match immutable events"));
        }

        match pending_decision {
            Some(ModelDecision::Finish { output })
                if state.terminal_error.is_some()
                    || state.final_output.as_ref() == Some(&output) => {}
            Some(_) if state.terminal_error.is_some() => {}
            Some(_) => return Err(invalid("model decision was not durably materialized")),
            None if state.final_output.is_some() => {
                return Err(invalid("terminal output has no matching model decision"));
            }
            None => {}
        }

        if let Some(pending) = state.pending_tool.as_ref() {
            let request_index = active_tool
                .ok_or_else(|| invalid("pending tool has no unmatched request event"))?;
            let request = &tool_requests[request_index].data;
            let step = steps
                .iter()
                .find(|step| step.id == pending.step_id)
                .ok_or_else(|| invalid("pending tool step is missing"))?;
            let canonical = canonicalize(&pending.arguments)
                .map_err(|_| invalid("pending tool arguments are not canonical"))?;
            if step.run_id != run.id
                || step.operation != pending.operation
                || step.version != pending.version
                || step.input_digest != digest(ArtifactKind::OperationInput, &canonical)
                || pending.step_id != request.step_id
                || pending.call_id != request.call_id
                || pending.tool_name != request.tool
                || pending.operation != request.operation
                || pending.version != request.version
                || pending.arguments != request.arguments
                || pending.approval_request_id != step.approval_request_id
                || !matches!(
                    step.status,
                    AgentRunStepStatus::Running
                        | AgentRunStepStatus::WaitingForApproval
                        | AgentRunStepStatus::Succeeded
                        | AgentRunStepStatus::Failed
                )
            {
                return Err(invalid("pending tool differs from its durable step"));
            }
        } else {
            if active_tool.is_some() {
                return Err(invalid(
                    "unresolved tool transition has no pending checkpoint",
                ));
            }
            if steps.iter().any(|step| !step.status.is_terminal()) {
                return Err(invalid("nonterminal step has no pending tool state"));
            }
        }
        Ok(())
    }

    fn collect_generic_tool_requests(
        &self,
        run: &AgentRun,
        agent: &AgentDefinition,
        events: &[AgentRunEvent],
        steps: &[AgentRunStep],
    ) -> Result<Vec<GenericToolRequestEvidence>, AgentRuntimeError> {
        let invalid = |detail: &str| {
            AgentRuntimeError::InconsistentState(format!(
                "agent run {} has unproven tool request evidence: {detail}",
                run.id
            ))
        };
        let observed_at = Utc::now();
        let mut step_ids = BTreeSet::new();
        let mut call_ids = BTreeSet::new();
        let mut requests = Vec::new();

        for event in events
            .iter()
            .filter(|event| event.kind == AgentRunEventKind::ToolRequested)
        {
            let parsed: GenericToolRequestedEvent = serde_json::from_value(event.data.clone())
                .map_err(|_| invalid("ToolRequested data is not exact"))?;
            if parsed.call_id.trim().is_empty()
                || parsed.tool.trim().is_empty()
                || !step_ids.insert(parsed.step_id)
                || !call_ids.insert(parsed.call_id.clone())
            {
                return Err(invalid("tool step or call binding is empty or duplicated"));
            }
            let ordinal = u32::try_from(requests.len())
                .map_err(|_| invalid("tool request ordinal exceeds u32"))?;
            let step = steps
                .iter()
                .find(|step| step.id == parsed.step_id)
                .ok_or_else(|| invalid("tool request step is missing"))?;
            let canonical = canonicalize(&parsed.arguments)
                .map_err(|_| invalid("tool request arguments are not canonical"))?;
            let started_at = step
                .started_at
                .ok_or_else(|| invalid("tool request step was never started"))?;
            let terminal_chronology = if step.status.is_terminal() {
                step.completed_at.is_some_and(|completed_at| {
                    completed_at == step.updated_at
                        && completed_at >= started_at
                        && completed_at <= observed_at
                })
            } else {
                step.completed_at.is_none()
            };
            if step.id.get_version_num() != 7
                || step.run_id != run.id
                || step.ordinal != ordinal
                || step.attempt != 1
                || step.retry_of.is_some()
                || step.operation != parsed.operation
                || step.version != parsed.version
                || step.input_digest != digest(ArtifactKind::OperationInput, &canonical)
                || step.created_at < run.created_at
                || step.created_at > started_at
                || started_at > step.updated_at
                || step.updated_at > observed_at
                || event.created_at < started_at
                || !terminal_chronology
            {
                return Err(invalid(
                    "tool request does not match its durable step chronology",
                ));
            }
            match step.status {
                AgentRunStepStatus::Running | AgentRunStepStatus::WaitingForApproval
                    if step.output.is_none() && step.proof.is_none() && step.error.is_none() => {}
                AgentRunStepStatus::Succeeded
                    if step.output.is_some() && step.proof.is_some() && step.error.is_none() => {}
                AgentRunStepStatus::Failed
                    if step.output.is_none()
                        && step.proof.is_none()
                        && step
                            .error
                            .as_ref()
                            .is_some_and(|error| !error.trim().is_empty()) => {}
                _ => return Err(invalid("tool step payload does not match its status")),
            }
            let entry = self
                .registry
                .find(&parsed.operation, &parsed.version)
                .ok_or_else(|| invalid("tool request registry entry is missing"))?;
            if !agent.tools.iter().any(|allowed| {
                allowed.operation == parsed.operation && allowed.version == parsed.version
            }) || parsed.tool != tool_name(entry)
            {
                return Err(invalid("tool request is not an exact allowed model tool"));
            }
            requests.push(GenericToolRequestEvidence {
                sequence: event.sequence,
                data: parsed,
            });
        }

        if requests.len() != steps.len() {
            return Err(invalid(
                "durable steps and ToolRequested events are not one-to-one",
            ));
        }
        Ok(requests)
    }

    fn validate_generic_approval_chronology(
        &self,
        run: &AgentRun,
        state: &AgentRuntimeState,
        events: &[AgentRunEvent],
        steps: &[AgentRunStep],
        tool_requests: &[GenericToolRequestEvidence],
    ) -> Result<(), AgentRuntimeError> {
        let invalid = |detail: &str| {
            AgentRuntimeError::InconsistentState(format!(
                "agent run {} has unproven approval evidence: {detail}",
                run.id
            ))
        };
        let requester = principal_from_keypair(&self.identity);
        let mut required = Vec::<(Uuid, Uuid, SignedApprovalRequest, u32, DateTime<Utc>)>::new();
        let mut resumed = Vec::<(Uuid, Uuid, ApprovalOutcome, u32, DateTime<Utc>)>::new();
        let mut outstanding: Option<usize> = None;

        for event in events.iter().skip(1) {
            match event.kind {
                AgentRunEventKind::ApprovalRequired => {
                    if outstanding.is_some() {
                        return Err(invalid("approval requests overlap"));
                    }
                    let parsed: GenericApprovalRequiredEvent =
                        serde_json::from_value(event.data.clone())
                            .map_err(|_| invalid("ApprovalRequired data is not exact"))?;
                    if required.iter().any(|(step_id, request_id, ..)| {
                        *step_id == parsed.step_id || *request_id == parsed.request_id
                    }) {
                        return Err(invalid("approval request event is duplicated"));
                    }
                    let tool = tool_requests
                        .iter()
                        .find(|request| request.data.step_id == parsed.step_id)
                        .ok_or_else(|| invalid("approval request has no tool request"))?;
                    let step = steps
                        .iter()
                        .find(|step| step.id == parsed.step_id)
                        .ok_or_else(|| invalid("approval request step is missing"))?;
                    if event.sequence <= tool.sequence
                        || parsed.operation != tool.data.operation
                        || parsed.version != tool.data.version
                        || step.approval_request_id != Some(parsed.request_id)
                    {
                        return Err(invalid(
                            "approval request event is not bound to its tool step",
                        ));
                    }
                    if run.status == AgentRunStatus::Running {
                        let rebound = self
                            .run_store
                            .find_agent_run_step_by_approval(&parsed.request_id)
                            .map_err(AgentRuntimeError::Store)?
                            .ok_or_else(|| {
                                invalid("approval request has no durable step binding")
                            })?;
                        if rebound != *step {
                            return Err(invalid("approval request resolves to a substituted step"));
                        }
                    }
                    let request = self
                        .approval_store
                        .load_approval_request(&parsed.request_id)
                        .map_err(AgentRuntimeError::Store)?
                        .ok_or_else(|| invalid("signed approval request is missing"))?;
                    if request.body.id != parsed.request_id
                        || request.body.id.get_version_num() != 7
                        || request.body.operation != parsed.operation
                        || request.body.version != parsed.version
                        || request.body.expires_at != parsed.expires_at
                        || request.body.requested_by != run.actor
                        || step.started_at.is_none_or(|started_at| {
                            request.body.requested_at < started_at
                                || request.body.requested_at > step.updated_at
                        })
                        || request.body.requested_at
                            < tool_requests
                                .iter()
                                .find(|candidate| candidate.data.step_id == parsed.step_id)
                                .and_then(|candidate| {
                                    events
                                        .iter()
                                        .find(|candidate_event| {
                                            candidate_event.sequence == candidate.sequence
                                        })
                                        .map(|candidate_event| candidate_event.created_at)
                                })
                                .ok_or_else(|| {
                                    invalid("tool request event chronology is missing")
                                })?
                        || request.body.requested_at > event.created_at
                    {
                        return Err(invalid(
                            "signed approval request data or chronology differs",
                        ));
                    }
                    request
                        .verify_for_call(
                            &requester,
                            &tool.data.operation,
                            &tool.data.version,
                            &tool.data.arguments,
                            run.actor,
                            event.created_at,
                        )
                        .map_err(|_| {
                            invalid("signed approval request does not authorize the call")
                        })?;
                    required.push((
                        parsed.step_id,
                        parsed.request_id,
                        request,
                        event.sequence,
                        event.created_at,
                    ));
                    outstanding = Some(required.len() - 1);
                }
                AgentRunEventKind::ApprovalResumed => {
                    let parsed: GenericApprovalResumedEvent =
                        serde_json::from_value(event.data.clone())
                            .map_err(|_| invalid("ApprovalResumed data is not exact"))?;
                    let required_index = outstanding
                        .take()
                        .ok_or_else(|| invalid("approval resume has no unique request event"))?;
                    let (step_id, request_id, request, required_sequence, required_at) =
                        &required[required_index];
                    if parsed.step_id != *step_id
                        || parsed.request_id != *request_id
                        || event.sequence <= *required_sequence
                        || event.created_at < *required_at
                    {
                        return Err(invalid("approval resume is reordered or substituted"));
                    }
                    if resumed.iter().any(|(prior_step, prior_request, ..)| {
                        *prior_step == parsed.step_id || *prior_request == parsed.request_id
                    }) {
                        return Err(invalid("approval resume event is duplicated"));
                    }
                    let decision = self
                        .approval_store
                        .load_approval_decision(&parsed.request_id)
                        .map_err(AgentRuntimeError::Store)?
                        .ok_or_else(|| invalid("signed approval decision is missing"))?;
                    if decision.body.id.get_version_num() != 7
                        || decision.body.request_id != parsed.request_id
                        || decision.body.decided_by != parsed.decided_by
                        || decision.body.outcome != parsed.outcome
                        || decision.body.decided_at > event.created_at
                    {
                        return Err(invalid("approval resume differs from its signed decision"));
                    }
                    let approver = self
                        .approval_store
                        .load_trusted_approver(&parsed.decided_by)
                        .map_err(AgentRuntimeError::Store)?
                        .ok_or_else(|| invalid("approval decision signer is not trusted"))?;
                    ApprovalGrant {
                        request: request.clone(),
                        decision,
                        approver: approver.clone(),
                    }
                    .verify_decision(&requester, &approver)
                    .map_err(|_| invalid("approval signatures or request binding are invalid"))?;
                    resumed.push((
                        parsed.step_id,
                        parsed.request_id,
                        parsed.outcome,
                        event.sequence,
                        event.created_at,
                    ));
                }
                _ => {}
            }
        }

        for step in steps {
            let Some(request_id) = step.approval_request_id else {
                let expected_revision = if step.status.is_terminal() { 2 } else { 1 };
                if step.status == AgentRunStepStatus::WaitingForApproval
                    || step.revision != expected_revision
                {
                    return Err(invalid("nonapproval step has an impossible revision"));
                }
                continue;
            };
            let (step_id, _, request, _, _) = required
                .iter()
                .find(|(required_step, required_request, ..)| {
                    *required_step == step.id && *required_request == request_id
                })
                .ok_or_else(|| invalid("step approval binding has no Required event"))?;
            if *step_id != step.id {
                return Err(invalid("step approval binding changed"));
            }
            let resumed_outcome = resumed
                .iter()
                .find(|(resumed_step, resumed_request, ..)| {
                    *resumed_step == step.id && *resumed_request == request_id
                })
                .map(|(_, _, outcome, _, _)| *outcome);
            let revision_is_exact = match (resumed_outcome, step.status) {
                (None, AgentRunStepStatus::WaitingForApproval) => step.revision == 2,
                (Some(ApprovalOutcome::Approved), AgentRunStepStatus::Running) => {
                    step.revision == 3
                }
                (Some(ApprovalOutcome::Approved), status) if status.is_terminal() => {
                    step.revision == 4
                }
                (Some(ApprovalOutcome::Denied), AgentRunStepStatus::Failed) => step.revision == 3,
                _ => false,
            };
            if !revision_is_exact {
                return Err(invalid("approval step status or revision is impossible"));
            }

            if let Some(execution) = self
                .approval_store
                .load_approval_execution(&request_id)
                .map_err(AgentRuntimeError::Store)?
            {
                let decision = self
                    .approval_store
                    .load_approval_decision(&request_id)
                    .map_err(AgentRuntimeError::Store)?
                    .ok_or_else(|| invalid("approval execution has no signed decision"))?;
                let approver = self
                    .approval_store
                    .load_trusted_approver(&decision.body.decided_by)
                    .map_err(AgentRuntimeError::Store)?
                    .ok_or_else(|| invalid("approval execution signer is not trusted"))?;
                let grant = ApprovalGrant {
                    request: request.clone(),
                    decision,
                    approver: approver.clone(),
                };
                grant
                    .verify_for_execution(
                        &self.identity,
                        &approver,
                        &step.operation,
                        &step.version,
                        &tool_requests
                            .iter()
                            .find(|tool| tool.data.step_id == step.id)
                            .ok_or_else(|| invalid("approval execution tool is missing"))?
                            .data
                            .arguments,
                        run.actor,
                        execution.executed_at,
                    )
                    .map_err(|_| invalid("approval execution is not authorized"))?;
                self.validate_generic_proof(
                    run,
                    step,
                    &execution.output,
                    &execution.proof,
                    execution.executed_at,
                    execution.executed_at,
                )?;
                if execution.request_id != request_id
                    || execution.proof.body.timestamp != execution.executed_at
                    || (step.status == AgentRunStepStatus::Succeeded
                        && (step.output.as_ref() != Some(&execution.output)
                            || step.proof.as_ref() != Some(&execution.proof)))
                {
                    return Err(invalid("approval execution differs from its step or proof"));
                }
            }
        }

        let resumed_count = u64::try_from(resumed.len())
            .map_err(|_| invalid("approval resume count exceeds u64"))?;
        let base_revision = resumed_count
            .checked_mul(2)
            .and_then(|count| count.checked_add(1))
            .ok_or_else(|| invalid("approval run revision overflow"))?;
        let run_chronology_is_exact = match run.status {
            AgentRunStatus::Running => {
                outstanding.is_none()
                    && required.len() == resumed.len()
                    && run.revision == base_revision
                    && if let Some((_, _, _, _, resumed_at)) = resumed.last() {
                        run.updated_at >= *resumed_at
                    } else {
                        run.updated_at == run.created_at
                    }
            }
            AgentRunStatus::WaitingForInput => {
                let Some(required_index) = outstanding else {
                    return Err(invalid("waiting run has no outstanding signed approval"));
                };
                let (_, request_id, request, _, required_at) = &required[required_index];
                required.len() == resumed.len() + 1
                    && run.revision == base_revision + 1
                    && run.updated_at == request.body.requested_at
                    && run.updated_at <= *required_at
                    && state
                        .pending_tool
                        .as_ref()
                        .is_some_and(|pending| pending.approval_request_id == Some(*request_id))
            }
            _ => false,
        };
        if run.retry_count != 0 || run.completed_at.is_some() || !run_chronology_is_exact {
            return Err(invalid(
                "run revision is not backed by exact signed approval transitions",
            ));
        }
        Ok(())
    }

    fn reconcile_missing_generic_tool_result(
        &self,
        run: &AgentRun,
        state: &AgentRuntimeState,
        checkpoints: &[AgentCheckpoint],
        steps: &[AgentRunStep],
        tool_requests: &[GenericToolRequestEvidence],
        events: &mut Vec<AgentRunEvent>,
    ) -> Result<(), AgentRuntimeError> {
        let invalid = |detail: &str| {
            AgentRuntimeError::InconsistentState(format!(
                "agent run {} cannot reconcile tool result evidence: {detail}",
                run.id
            ))
        };
        let mut active: Option<&GenericToolRequestEvidence> = None;
        for event in events.iter().skip(1) {
            match event.kind {
                AgentRunEventKind::ToolRequested => {
                    if active.is_some() {
                        return Ok(());
                    }
                    active = tool_requests
                        .iter()
                        .find(|request| request.sequence == event.sequence);
                }
                AgentRunEventKind::ToolSucceeded | AgentRunEventKind::ToolFailed => {
                    active = None;
                }
                AgentRunEventKind::ModelRequested if active.is_some() => return Ok(()),
                _ => {}
            }
        }
        let Some(request) = active else {
            return Ok(());
        };
        if state.pending_tool.is_some() {
            return Ok(());
        }
        if events
            .iter()
            .filter(|event| event.sequence > request.sequence)
            .any(|event| {
                !matches!(
                    event.kind,
                    AgentRunEventKind::ApprovalRequired | AgentRunEventKind::ApprovalResumed
                )
            })
        {
            return Ok(());
        }
        let step = steps
            .iter()
            .find(|step| step.id == request.data.step_id)
            .ok_or_else(|| invalid("missing result step is absent"))?;
        let Some((kind, data, expected_next_input)) =
            self.expected_generic_tool_result(run, request, step)?
        else {
            return Ok(());
        };
        let latest_checkpoint = checkpoints
            .last()
            .ok_or_else(|| invalid("result checkpoint is missing"))?;
        if state.next_input != expected_next_input
            || latest_checkpoint.created_at < step.updated_at
            || latest_checkpoint.state.get("terminal_event_kind").is_some()
        {
            return Err(invalid(
                "missing event is not backed by the exact post-tool checkpoint",
            ));
        }

        let prior = events.clone();
        self.append_event(run.id, kind, data.clone())?;
        let reread = self
            .agent_store
            .list_agent_run_events(&run.id)
            .map_err(AgentRuntimeError::Store)?;
        validate_generic_evidence_envelopes(run.id, checkpoints, &reread)?;
        let exact_append = reread.len() == prior.len() + 1
            && reread[..prior.len()] == prior
            && reread.last().is_some_and(|event| {
                validate_exact_event_record(run.id, kind, &data, event).is_ok()
            });
        if !exact_append {
            return Err(invalid("repaired result event failed exact durable reread"));
        }
        *events = reread;
        Ok(())
    }

    fn expected_generic_tool_result(
        &self,
        run: &AgentRun,
        request: &GenericToolRequestEvidence,
        step: &AgentRunStep,
    ) -> Result<Option<(AgentRunEventKind, Value, ModelInput)>, AgentRuntimeError> {
        let invalid = |detail: &str| {
            AgentRuntimeError::InconsistentState(format!(
                "agent run {} has invalid tool result evidence: {detail}",
                run.id
            ))
        };
        match step.status {
            AgentRunStepStatus::Succeeded => {
                let output = step
                    .output
                    .as_ref()
                    .ok_or_else(|| invalid("succeeded step output is missing"))?;
                let proof = step
                    .proof
                    .as_ref()
                    .ok_or_else(|| invalid("succeeded step proof is missing"))?;
                let started_at = step
                    .started_at
                    .ok_or_else(|| invalid("succeeded step start time is missing"))?;
                let completed_at = step
                    .completed_at
                    .ok_or_else(|| invalid("succeeded step completion time is missing"))?;
                self.validate_generic_proof(run, step, output, proof, started_at, completed_at)?;
                Ok(Some((
                    AgentRunEventKind::ToolSucceeded,
                    json!({
                        "step_id": step.id,
                        "call_id": request.data.call_id,
                        "operation": request.data.operation,
                        "version": request.data.version,
                        "proof_id": proof.body.id,
                    }),
                    ModelInput::ToolOutput {
                        call_id: request.data.call_id.clone(),
                        output: json!({
                            "ok": true,
                            "result": output,
                            "proof_id": proof.body.id,
                        }),
                    },
                )))
            }
            AgentRunStepStatus::Failed => {
                let error = step
                    .error
                    .as_ref()
                    .filter(|error| !error.trim().is_empty())
                    .ok_or_else(|| invalid("failed step error is missing"))?;
                Ok(Some((
                    AgentRunEventKind::ToolFailed,
                    json!({
                        "step_id": step.id,
                        "call_id": request.data.call_id,
                        "error": error,
                    }),
                    ModelInput::ToolOutput {
                        call_id: request.data.call_id.clone(),
                        output: json!({
                            "ok": false,
                            "error": error,
                            "operation": request.data.operation,
                            "version": request.data.version,
                        }),
                    },
                )))
            }
            AgentRunStepStatus::Running | AgentRunStepStatus::WaitingForApproval => Ok(None),
            _ => Err(invalid("tool result step has an unsupported status")),
        }
    }

    fn validate_generic_proof(
        &self,
        run: &AgentRun,
        step: &AgentRunStep,
        output: &Value,
        proof: &Proof,
        earliest: DateTime<Utc>,
        latest: DateTime<Utc>,
    ) -> Result<(), AgentRuntimeError> {
        let canonical = canonicalize(output).map_err(|_| {
            AgentRuntimeError::InconsistentState(format!(
                "agent run {} tool output is not canonical",
                run.id
            ))
        })?;
        let principal = principal_from_keypair(&self.identity);
        if proof.body.id.get_version_num() != 7
            || proof.body.actor != run.actor
            || proof.body.delegation_id.is_some()
            || proof.body.operation != format!("{}::{}", step.operation, step.version)
            || proof.body.input_digest != step.input_digest
            || proof.body.output_digest != digest(ArtifactKind::OperationOutput, &canonical)
            || proof.body.timestamp < earliest
            || proof.body.timestamp > latest
            || proof.body.expires_at.is_some()
            || proof.verify(&principal.public_key).is_err()
        {
            return Err(AgentRuntimeError::InconsistentState(format!(
                "agent run {} tool proof is substituted or invalid",
                run.id
            )));
        }
        Ok(())
    }

    fn reject_bound_bootstrap_approval_evidence(
        &self,
        run_id: Uuid,
    ) -> Result<(), AgentRuntimeError> {
        for request in self
            .approval_store
            .list_approval_requests()
            .map_err(AgentRuntimeError::Store)?
        {
            let Some(step) = self
                .run_store
                .find_agent_run_step_by_approval(&request.body.id)
                .map_err(AgentRuntimeError::Store)?
            else {
                continue;
            };
            if step.run_id == run_id {
                // Load any execution before rejecting so an adversarial store
                // cannot hide consequential evidence behind a split lookup.
                let _ = self
                    .approval_store
                    .load_approval_execution(&request.body.id)
                    .map_err(AgentRuntimeError::Store)?;
                return Err(AgentRuntimeError::InconsistentState(format!(
                    "agent run {run_id} has approval evidence during bootstrap"
                )));
            }
        }
        Ok(())
    }

    fn reread_exact_run(&self, expected: &AgentRun) -> Result<(), AgentRuntimeError> {
        let reread = self
            .run_store
            .load_agent_run(&expected.id)
            .map_err(AgentRuntimeError::Store)?
            .ok_or(AgentRuntimeError::RunNotFound(expected.id))?;
        if reread != *expected {
            return Err(AgentRuntimeError::InconsistentState(format!(
                "agent run {} failed exact durable reread",
                expected.id
            )));
        }
        Ok(())
    }

    pub fn state(&self, run_id: Uuid) -> Result<AgentRuntimeState, AgentRuntimeError> {
        let checkpoints = self
            .run_store
            .list_agent_checkpoints(&run_id)
            .map_err(AgentRuntimeError::Store)?;
        let checkpoint = checkpoints
            .iter()
            .rev()
            .find(|checkpoint| {
                checkpoint.state.get("kind").and_then(Value::as_str)
                    == Some(RUNTIME_CHECKPOINT_KIND)
            })
            .ok_or(AgentRuntimeError::MissingCheckpoint(run_id))?;
        serde_json::from_value(
            checkpoint
                .state
                .get("runtime")
                .cloned()
                .ok_or(AgentRuntimeError::InvalidCheckpoint(run_id))?,
        )
        .map_err(|_| AgentRuntimeError::InvalidCheckpoint(run_id))
    }

    /// Starts or resumes the sealed E0001 live journey. This path is separate
    /// from `start`/`resume`: it never constructs a provider until the complete
    /// local setup, prepared attempt, and durable dispatch barrier are present.
    pub fn run_live(&self, setup: LiveRunSetup) -> Result<AgentRuntimeOutcome, AgentRuntimeError> {
        match setup.intent.clone() {
            LiveRunIntent::Start { agent_id, goal } => {
                self.check_live_start_setup(&setup)?;
                self.start_live(setup, agent_id, goal)
            }
            LiveRunIntent::Resume { run_id } => self.resume_live(setup, run_id),
        }
    }

    /// Validates a prospective sealed live start without creating a run or
    /// invoking the configured gateway factory.
    ///
    /// This is the authoritative credential-free check-only path used by
    /// `run_live`. `start_live` repeats these validations after selecting the
    /// actual start timestamp so authority remains valid through the exact
    /// 300-second run deadline immediately before the first run write.
    pub fn check_live_start_setup(&self, setup: &LiveRunSetup) -> Result<(), AgentRuntimeError> {
        let (agent_id, goal) = match &setup.intent {
            LiveRunIntent::Start { agent_id, goal } => (*agent_id, goal.as_str()),
            LiveRunIntent::Resume { .. } => {
                return Err(AgentRuntimeError::LiveSetup(
                    "check_live_start_setup accepts only LiveRunIntent::Start".to_string(),
                ));
            }
        };
        self.validate_live_setup(setup, goal, run_deadline(Utc::now(), 300))?;
        let agent = self.load_live_agent(agent_id)?;
        self.validate_live_agent(&agent, setup, goal)
    }

    fn start_live(
        &self,
        setup: LiveRunSetup,
        agent_id: Uuid,
        goal: String,
    ) -> Result<AgentRuntimeOutcome, AgentRuntimeError> {
        // Select the actual immutable start timestamp first, then close the
        // authority-validity TOCTOU window before any AgentRun write.
        let now = Utc::now();
        self.validate_live_setup(&setup, &goal, run_deadline(now, 300))?;
        let agent = self.load_live_agent(agent_id)?;
        self.validate_live_agent(&agent, &setup, &goal)?;
        let mut run = AgentRun::new_for_agent(
            self.identity.principal_id,
            agent.id,
            AgentRunMode::Session,
            goal,
            now,
        )?;
        self.save_run(&run)?;
        run.start(now)?;
        self.save_run(&run)?;
        let binding = resolved_live_bindings(
            run.id,
            agent.id,
            setup.process_epoch_id,
            &setup.policy.binding_inputs,
        );
        let resolved_policy = resolve_live_policy(&setup.policy.template, &binding)?;
        let binding_value = serde_json::to_value(&binding)
            .map_err(|error| AgentRuntimeError::LiveSetup(error.to_string()))?;
        let bindings_digest = wrapped_digest(
            "proof-release-manager-live-bindings-digest/v1",
            "bindings",
            &binding_value,
        )?;
        let resolved_policy_digest = value_digest(&resolved_policy)?;
        let request = self.live_request(&agent, &setup.policy, &binding, None)?;
        let mut state = LiveRuntimeState::new(
            run.id,
            agent.id,
            now,
            &setup,
            binding,
            bindings_digest,
            resolved_policy,
            resolved_policy_digest,
            request,
        );
        self.save_live_state(run.id, &state)?;
        self.append_event(
            run.id,
            AgentRunEventKind::Started,
            live_started_event(&state),
        )?;
        self.reread_live_state(run.id, &state)?;
        self.reread_live_event(
            run.id,
            AgentRunEventKind::Started,
            &live_started_event(&state),
        )?;
        self.prepare_and_dispatch_live(run, agent, &mut state)
    }

    fn resume_live(
        &self,
        setup: LiveRunSetup,
        run_id: Uuid,
    ) -> Result<AgentRuntimeOutcome, AgentRuntimeError> {
        let run = self
            .run_store
            .load_agent_run(&run_id)
            .map_err(AgentRuntimeError::Store)?
            .ok_or(AgentRuntimeError::RunNotFound(run_id))?;
        if run.actor != self.identity.principal_id {
            return Err(AgentRuntimeError::ActorMismatch {
                run_id,
                actual: run.actor.to_string(),
                expected: self.identity.principal_id.to_string(),
            });
        }
        let agent_id = run
            .agent_id
            .ok_or(AgentRuntimeError::RunMissingAgent(run_id))?;
        let agent = self.load_live_agent(agent_id)?;
        let mut state = self.live_state(run_id)?;
        self.validate_live_setup(&setup, &run.goal, run_deadline(state.started_at, 300))?;
        self.validate_live_agent(&agent, &setup, &run.goal)?;
        self.validate_live_state_material(&state, &setup)?;
        if self.live_epoch_seen(run_id, setup.process_epoch_id)? {
            return Err(AgentRuntimeError::LiveSetup(
                "live resume requires an epoch absent from all immutable history".to_string(),
            ));
        }
        // A matching terminal event seals SQLite's trace.  A replay still has
        // to present a valid globally new epoch, but persisting that epoch
        // would mutate the already evaluated trace.  Missing terminal events
        // remain unsealed crash states and cross the durable epoch barrier
        // below before recovery.
        if self.sealed_live_terminal_trace(&run, &state)? {
            return self.live_terminal_outcome(run, state, true);
        }
        state.process_epoch_id = setup.process_epoch_id;
        self.save_live_state(run_id, &state)?;
        self.reread_live_state(run_id, &state)?;
        if state.terminal_error.is_some() || run.status.is_terminal() {
            return self.live_terminal_outcome(run, state, false);
        }
        // A process death after the durable dispatch barrier is ambiguous even
        // when the process died before the TCP write. Never create a gateway.
        if state.attempts.iter().any(|attempt| {
            matches!(
                attempt.state,
                ProviderAttemptState::Dispatching | ProviderAttemptState::ResponseReceived
            )
        }) {
            state.terminal_error =
                Some("live provider attempt is ambiguous after restart".to_string());
            self.save_live_state(run_id, &state)?;
            return self.fail_live_run(run, state, AgentRunEventKind::Failed);
        }
        // `Committed` is only durable when its matching immutable
        // `model_responded` event is present.  A checkpoint alone might have
        // been written immediately before a crash; it must never authorize a
        // continuation, tool execution, or another provider dispatch.
        let events = self
            .agent_store
            .list_agent_run_events(&run_id)
            .map_err(AgentRuntimeError::Store)?;
        let requested_count = events
            .iter()
            .filter(|event| event.kind == AgentRunEventKind::ModelRequested)
            .count();
        if requested_count != state.counters.provider_dispatches as usize
            || state.attempts.iter().any(|attempt| {
                attempt.dispatched_at.is_some()
                    && exact_model_requested_event(run_id, attempt, &events).is_err()
            })
        {
            state.terminal_error =
                Some("provider dispatch event evidence is not exact".to_string());
            self.save_live_state(run_id, &state)?;
            return self.fail_live_run(run, state, AgentRunEventKind::Failed);
        }
        let committed_count = state
            .attempts
            .iter()
            .filter(|attempt| attempt.state == ProviderAttemptState::Committed)
            .count();
        let responded_count = events
            .iter()
            .filter(|event| event.kind == AgentRunEventKind::ModelResponded)
            .count();
        if responded_count != committed_count {
            state.terminal_error =
                Some("provider response event ledger has an unknown or extra attempt".to_string());
            self.save_live_state(run_id, &state)?;
            return self.fail_live_run(run, state, AgentRunEventKind::Failed);
        }
        let committed_decisions = state
            .attempts
            .iter()
            .filter(|attempt| attempt.state == ProviderAttemptState::Committed)
            .map(|attempt| exact_committed_event(run_id, attempt, &events))
            .collect::<Result<Vec<_>, _>>();
        let Ok(committed_decisions) = committed_decisions else {
            state.terminal_error = Some(
                "committed provider checkpoint has no matching model_responded event".to_string(),
            );
            self.save_live_state(run_id, &state)?;
            return self.fail_live_run(run, state, AgentRunEventKind::Failed);
        };
        if !live_pending_matches_committed_decision(&state, &committed_decisions) {
            state.terminal_error =
                Some("pending live approval differs from committed tool decision".to_string());
            self.save_live_state(run_id, &state)?;
            return self.fail_live_run(run, state, AgentRunEventKind::Failed);
        }
        let approval_resume_epoch = if let Some(pending) = state.pending_tool.as_ref() {
            let has_resumed = events
                .iter()
                .any(|event| event.kind == AgentRunEventKind::ApprovalResumed);
            if has_resumed {
                let checkpoints = self
                    .run_store
                    .list_agent_checkpoints(&run_id)
                    .map_err(AgentRuntimeError::Store)?;
                let resumed = exact_approval_resumed_event(
                    run_id,
                    pending.step_id,
                    pending.approval_request_id,
                    &events,
                    &checkpoints,
                );
                match resumed {
                    Ok(resumed) => resumed.process_epoch_id,
                    Err(_) => {
                        state.terminal_error =
                            Some("approval resume event evidence is not exact".to_string());
                        self.save_live_state(run_id, &state)?;
                        return self.fail_live_run(run, state, AgentRunEventKind::Failed);
                    }
                }
            } else {
                setup.process_epoch_id
            }
        } else {
            setup.process_epoch_id
        };
        if state
            .attempts
            .last()
            .is_some_and(|attempt| attempt.state == ProviderAttemptState::Prepared)
        {
            return self.prepare_and_dispatch_live(run, agent, &mut state);
        }
        if state.attempts.last().is_some_and(|attempt| {
            matches!(
                attempt.state,
                ProviderAttemptState::FailedRetryable | ProviderAttemptState::RejectedRetryable
            )
        }) {
            return self.prepare_and_dispatch_live(run, agent, &mut state);
        }
        if state.pending_tool.is_some() {
            return self.resume_live_approval(run, agent, setup, approval_resume_epoch, &mut state);
        }
        if state
            .attempts
            .last()
            .is_some_and(|attempt| attempt.state == ProviderAttemptState::Committed)
        {
            match committed_decisions.last() {
                Some(LiveCommittedDecision::ToolCall {
                    call_id,
                    name,
                    arguments,
                }) if state.counters.successful_publication_mutations == 0
                    && matches!(state.next_input, LiveModelInput::Goal { .. }) =>
                {
                    return self.wait_live_approval(
                        run,
                        &mut state,
                        call_id.clone(),
                        name.clone(),
                        arguments.as_value()?,
                    );
                }
                Some(LiveCommittedDecision::Finish { output })
                    if state.counters.successful_publication_mutations == 1
                        && matches!(state.next_input, LiveModelInput::ToolOutput { .. }) =>
                {
                    state.final_output = Some(output.clone());
                    self.save_live_state(run_id, &state)?;
                    return self.live_terminal_outcome(run, state, false);
                }
                _ => {}
            }
        }
        if !matches!(state.next_input, LiveModelInput::ToolOutput { .. })
            || state.counters.successful_publication_mutations != 1
        {
            state.terminal_error = Some(
                "committed provider evidence has no contract-defined pending decision".to_string(),
            );
            self.save_live_state(run_id, &state)?;
            return self.fail_live_run(run, state, AgentRunEventKind::Failed);
        }
        let steps = self
            .run_store
            .list_agent_run_steps(&run_id)
            .map_err(AgentRuntimeError::Store)?;
        if steps.len() != 1
            || steps[0].status != AgentRunStepStatus::Succeeded
            || steps[0].proof.is_none()
        {
            state.terminal_error =
                Some("committed continuation has no exact succeeded tool step".to_string());
            self.save_live_state(run_id, &state)?;
            return self.fail_live_run(run, state, AgentRunEventKind::Failed);
        }
        let tool_succeeded = json!({
            "step_id": steps[0].id,
            "proof_id": steps[0].proof.as_ref().expect("checked proof").body.id,
            "live": true,
        });
        let succeeded_events = self
            .agent_store
            .list_agent_run_events(&run_id)
            .map_err(AgentRuntimeError::Store)?
            .into_iter()
            .filter(|event| event.kind == AgentRunEventKind::ToolSucceeded)
            .collect::<Vec<_>>();
        if succeeded_events.is_empty() {
            self.append_event(run_id, AgentRunEventKind::ToolSucceeded, tool_succeeded)?;
        } else if succeeded_events.len() != 1 || succeeded_events[0].data != tool_succeeded {
            state.terminal_error = Some("tool success event evidence is not exact".to_string());
            self.save_live_state(run_id, &state)?;
            return self.fail_live_run(run, state, AgentRunEventKind::Failed);
        }
        self.reread_live_state(run_id, &state)?;
        self.prepare_and_dispatch_live(run, agent, &mut state)
    }

    fn prepare_and_dispatch_live(
        &self,
        run: AgentRun,
        agent: AgentDefinition,
        state: &mut LiveRuntimeState,
    ) -> Result<AgentRuntimeOutcome, AgentRuntimeError> {
        if Utc::now() >= run_deadline(state.started_at, 300) {
            state.terminal_error =
                Some("live wall-clock deadline exceeded before dispatch".to_string());
            self.save_live_state(run.id, state)?;
            return self.fail_live_run(run, state.clone(), AgentRunEventKind::BudgetExceeded);
        }
        if state.counters.provider_dispatches >= 4 || state.counters.logical_model_turns >= 3 {
            state.terminal_error =
                Some("live provider/model-turn budget exceeded before dispatch".to_string());
            self.save_live_state(run.id, state)?;
            return self.fail_live_run(run, state.clone(), AgentRunEventKind::BudgetExceeded);
        }
        let retry_of = state
            .attempts
            .last()
            .and_then(|attempt| match attempt.state {
                ProviderAttemptState::Prepared => attempt.retry_of,
                ProviderAttemptState::FailedRetryable | ProviderAttemptState::RejectedRetryable => {
                    Some(attempt.attempt_id)
                }
                _ => None,
            });
        if retry_of.is_some() && state.counters.retries > 1 {
            state.terminal_error = Some("live automatic retry limit exceeded".to_string());
            self.save_live_state(run.id, state)?;
            return self.fail_live_run(run, state.clone(), AgentRunEventKind::BudgetExceeded);
        }
        let request = self.request_from_live_state(state)?;
        let attempt_id = match state.attempts.last() {
            Some(attempt) if attempt.state == ProviderAttemptState::Prepared => {
                // A persisted prepared attempt has certified zero provider
                // bytes.  Reuse that exact sealed request rather than append a
                // second logical attempt after a process crash.
                if attempt.request != request || attempt.retry_of != retry_of {
                    state.terminal_error =
                        Some("prepared attempt request/lineage drift".to_string());
                    self.save_live_state(run.id, state)?;
                    return self.fail_live_run(run, state.clone(), AgentRunEventKind::Failed);
                }
                attempt.attempt_id
            }
            _ => {
                let attempt_id = Uuid::now_v7();
                state.attempts.push(ProviderAttempt::prepared(
                    attempt_id,
                    state.counters.logical_model_turns + 1,
                    state.counters.provider_dispatches + 1,
                    retry_of,
                    state.process_epoch_id,
                    request.clone(),
                ));
                self.save_live_state(run.id, state)?;
                self.reread_live_state(run.id, state)?;
                attempt_id
            }
        };
        let factory_context = ModelGatewayFactoryContext {
            run_id: run.id,
            attempt_id,
            process_epoch_id: state
                .attempts
                .last()
                .expect("prepared attempt exists")
                .process_epoch_id,
            provider: LIVE_PROVIDER.to_string(),
            endpoint: LIVE_ENDPOINT.to_string(),
            requested_model: LIVE_MODEL.to_string(),
            service_tier: LIVE_SERVICE_TIER.to_string(),
            request_body_digest: request.request_body_digest,
        };
        let gateway = match self.gateway_factory.create(&factory_context) {
            Ok(gateway) => gateway,
            Err(error) => {
                let attempt = state.attempts.last_mut().expect("prepared attempt exists");
                attempt.state = ProviderAttemptState::FailedTerminal;
                attempt.failure = Some(ProviderFailure::terminal("gateway_factory_failed"));
                attempt.finished_at = Some(Utc::now());
                state.terminal_error = Some("gateway factory failed".to_string());
                self.save_live_state(run.id, state)?;
                return self
                    .fail_live_run(run, state.clone(), AgentRunEventKind::Failed)
                    .map_err(|_| AgentRuntimeError::GatewayFactory(error.to_string()));
            }
        };
        if gateway.provider() != LIVE_PROVIDER {
            let attempt = state.attempts.last_mut().expect("prepared attempt exists");
            attempt.state = ProviderAttemptState::FailedTerminal;
            attempt.failure = Some(ProviderFailure::terminal("gateway_provider_mismatch"));
            attempt.finished_at = Some(Utc::now());
            state.terminal_error = Some("gateway provider is not openai".to_string());
            self.save_live_state(run.id, state)?;
            return self.fail_live_run(run, state.clone(), AgentRunEventKind::Failed);
        }
        // This is the pre-I/O barrier: durable checkpoint, immutable event,
        // and successful reread all precede `complete`.
        state.counters.provider_dispatches += 1;
        let barrier;
        {
            let attempt = state.attempts.last_mut().expect("prepared attempt exists");
            attempt.state = ProviderAttemptState::Dispatching;
            attempt.dispatched_at = Some(Utc::now());
            barrier = live_model_requested_event(attempt);
        }
        self.save_live_state(run.id, state)?;
        self.append_event(run.id, AgentRunEventKind::ModelRequested, barrier.clone())?;
        self.reread_live_state(run.id, state)?;
        let requested_events = self
            .agent_store
            .list_agent_run_events(&run.id)
            .map_err(AgentRuntimeError::Store)?;
        if requested_events
            .iter()
            .filter(|event| event.kind == AgentRunEventKind::ModelRequested)
            .count()
            != state.counters.provider_dispatches as usize
        {
            return Err(AgentRuntimeError::InconsistentState(
                "model-requested dispatch barrier is not exact-one per dispatch".to_string(),
            ));
        }
        exact_model_requested_event(
            run.id,
            state.attempts.last().expect("dispatching attempt exists"),
            &requested_events,
        )?;
        let model_request = request.as_model_request()?;
        let turn = match gateway.complete(&model_request) {
            Ok(turn) => turn,
            Err(error) => return self.record_live_gateway_failure(run, state, error),
        };
        self.commit_live_response(run, agent, state, turn)
    }

    fn record_live_gateway_failure(
        &self,
        run: AgentRun,
        state: &mut LiveRuntimeState,
        error: crate::model::ModelGatewayError,
    ) -> Result<AgentRuntimeOutcome, AgentRuntimeError> {
        match error {
            crate::model::ModelGatewayError::CertifiedNoBytes(_) => {
                {
                    let attempt = state
                        .attempts
                        .last_mut()
                        .expect("dispatching attempt exists");
                    attempt.state = ProviderAttemptState::FailedRetryable;
                    attempt.failure =
                        Some(ProviderFailure::certified_no_bytes("certified_no_bytes"));
                    attempt.finished_at = Some(Utc::now());
                }
                if state.counters.retries >= 1 {
                    let attempt = state.attempts.last_mut().expect("attempt exists");
                    attempt.state = ProviderAttemptState::FailedTerminal;
                    attempt.failure = Some(ProviderFailure::terminal("retry_limit_exhausted"));
                    state.terminal_error = Some("live automatic retry limit exceeded".to_string());
                    self.save_live_state(run.id, state)?;
                    return self.fail_live_run(run, state.clone(), AgentRunEventKind::Failed);
                }
                state.counters.retries = 1;
                self.save_live_state(run.id, state)?;
                self.prepare_and_dispatch_live(run, self.load_live_agent(state.agent_id)?, state)
            }
            crate::model::ModelGatewayError::Explicit429(_) => {
                {
                    let attempt = state
                        .attempts
                        .last_mut()
                        .expect("dispatching attempt exists");
                    attempt.state = ProviderAttemptState::RejectedRetryable;
                    attempt.failure = Some(ProviderFailure::explicit_429("http_429"));
                    attempt.finished_at = Some(Utc::now());
                }
                if state.counters.retries >= 1 {
                    let attempt = state.attempts.last_mut().expect("attempt exists");
                    attempt.state = ProviderAttemptState::FailedTerminal;
                    attempt.failure = Some(ProviderFailure::terminal("retry_limit_exhausted"));
                    state.terminal_error = Some("live automatic retry limit exceeded".to_string());
                    self.save_live_state(run.id, state)?;
                    return self.fail_live_run(run, state.clone(), AgentRunEventKind::Failed);
                }
                state.counters.retries = 1;
                self.save_live_state(run.id, state)?;
                self.prepare_and_dispatch_live(run, self.load_live_agent(state.agent_id)?, state)
            }
            crate::model::ModelGatewayError::Terminal(_) => {
                {
                    let attempt = state
                        .attempts
                        .last_mut()
                        .expect("dispatching attempt exists");
                    attempt.state = ProviderAttemptState::FailedTerminal;
                    attempt.failure =
                        Some(ProviderFailure::terminal("provider_terminal_rejection"));
                    attempt.finished_at = Some(Utc::now());
                }
                state.terminal_error = Some("provider rejected the request terminally".to_string());
                self.save_live_state(run.id, state)?;
                self.fail_live_run(run, state.clone(), AgentRunEventKind::Failed)
            }
            crate::model::ModelGatewayError::Request(_)
            | crate::model::ModelGatewayError::InvalidResponse(_)
            | crate::model::ModelGatewayError::Ambiguous(_) => {
                {
                    let attempt = state
                        .attempts
                        .last_mut()
                        .expect("dispatching attempt exists");
                    attempt.state = ProviderAttemptState::Ambiguous;
                    attempt.failure = Some(ProviderFailure::ambiguous("provider_outcome_unknown"));
                    attempt.finished_at = Some(Utc::now());
                }
                state.terminal_error = Some("provider outcome is ambiguous".to_string());
                self.save_live_state(run.id, state)?;
                self.fail_live_run(run, state.clone(), AgentRunEventKind::Failed)
            }
        }
    }

    fn drive(
        &self,
        mut run: AgentRun,
        agent: AgentDefinition,
        tools: Vec<AgentFunctionTool>,
        mut state: AgentRuntimeState,
    ) -> Result<AgentRuntimeOutcome, AgentRuntimeError> {
        if state.final_output.is_some() || state.terminal_error.is_some() {
            return self.finish_terminal_state(run, state);
        }
        if run.status == AgentRunStatus::WaitingForInput {
            if let Some(reason) = duration_budget_error(&agent, &state, Utc::now()) {
                return self.fail_approval_for_budget(run, state, reason);
            }
            match self.resume_approval(&mut run, &agent, &mut state)? {
                ResumeApproval::Waiting(outcome) => return Ok(outcome),
                ResumeApproval::Continue => {}
                ResumeApproval::Failed(error) => {
                    return self.fail_run(run, state, error, AgentRunEventKind::Failed)
                }
                ResumeApproval::BudgetExceeded(reason) => {
                    return self.fail_approval_for_budget(run, state, reason)
                }
            }
        } else if state.pending_tool.is_some() {
            if let Some(error) = self.reconcile_pending_tool(&mut state)? {
                return self.fail_run(run, state, error, AgentRunEventKind::Failed);
            }
        }

        loop {
            if let Some(reason) = self.pre_model_budget_error(&agent, &state, Utc::now()) {
                return self.fail_run(run, state, reason, AgentRunEventKind::BudgetExceeded);
            }
            state.model_calls = state.model_calls.checked_add(1).ok_or_else(|| {
                AgentRuntimeError::InconsistentState("model call counter overflow".to_string())
            })?;
            self.save_state(run.id, &state)?;
            self.append_event(
                run.id,
                AgentRunEventKind::ModelRequested,
                json!({
                    "model": agent.model,
                    "model_call": state.model_calls,
                    "previous_response_id": state.previous_response_id,
                }),
            )?;
            let request = ModelTurnRequest {
                model: agent.model.clone(),
                instructions: runtime_instructions(&agent.instructions),
                input: state.next_input.clone(),
                previous_response_id: state.previous_response_id.clone(),
                tools: tools.clone(),
                max_output_tokens: agent.limits.max_output_tokens_per_call,
            };
            let turn = match self.model.complete(&request) {
                Ok(turn) => turn,
                Err(error) => {
                    return self.fail_run(run, state, error.to_string(), AgentRunEventKind::Failed)
                }
            };
            if let Err(reason) = apply_usage(&mut state, turn.usage) {
                return self.fail_run(run, state, reason, AgentRunEventKind::BudgetExceeded);
            }
            state.previous_response_id = Some(turn.response_id.clone());
            self.append_event(
                run.id,
                AgentRunEventKind::ModelResponded,
                json!({
                    "response_id": turn.response_id,
                    "decision": turn.decision,
                    "usage": turn.usage,
                }),
            )?;
            if let Some(reason) = self.post_model_budget_error(&agent, &state, Utc::now()) {
                return self.fail_run(run, state, reason, AgentRunEventKind::BudgetExceeded);
            }

            match turn.decision {
                ModelDecision::Finish { output } => {
                    state.final_output = Some(output);
                    self.save_state(run.id, &state)?;
                    return self.finish_terminal_state(run, state);
                }
                ModelDecision::ToolCall {
                    call_id,
                    name,
                    arguments,
                } => {
                    if state.tool_attempts >= agent.limits.max_steps {
                        return self.fail_run(
                            run,
                            state,
                            format!("step budget exceeded: maximum {}", agent.limits.max_steps),
                            AgentRunEventKind::BudgetExceeded,
                        );
                    }
                    let Some(tool) = tools.iter().find(|tool| tool.name == name) else {
                        return self.fail_run(
                            run,
                            state,
                            format!("model requested unapproved tool: {name}"),
                            AgentRunEventKind::Failed,
                        );
                    };
                    let entry = self
                        .registry
                        .find(&tool.operation, &tool.version)
                        .cloned()
                        .ok_or_else(|| AgentRuntimeError::ToolNotRegistered {
                            operation: tool.operation.clone(),
                            version: tool.version.clone(),
                        })?;
                    let now = Utc::now();
                    let mut step = AgentRunStep::new(
                        run.id,
                        state.tool_attempts,
                        &entry.operation,
                        &entry.version,
                        &arguments,
                        now,
                    )?;
                    self.save_step(&step)?;
                    step.start(now)?;
                    self.save_step(&step)?;
                    state.tool_attempts = state.tool_attempts.checked_add(1).ok_or_else(|| {
                        AgentRuntimeError::InconsistentState(
                            "tool attempt counter overflow".to_string(),
                        )
                    })?;
                    state.pending_tool = Some(PendingToolCall {
                        call_id,
                        tool_name: name,
                        operation: entry.operation.clone(),
                        version: entry.version.clone(),
                        arguments,
                        step_id: step.id,
                        approval_request_id: None,
                    });
                    self.save_state(run.id, &state)?;
                    let pending = state.pending_tool.as_ref().expect("pending tool set above");
                    self.append_event(
                        run.id,
                        AgentRunEventKind::ToolRequested,
                        json!({
                            "step_id": step.id,
                            "call_id": pending.call_id,
                            "tool": pending.tool_name,
                            "operation": pending.operation,
                            "version": pending.version,
                            "arguments": pending.arguments,
                        }),
                    )?;

                    if let Err(error) =
                        self.validate_schema(&entry, &entry.input_schema, &pending.arguments)
                    {
                        self.record_tool_failure(&mut step, &mut state, error.to_string())?;
                        continue;
                    }
                    if entry.governance == Governance::HumanOnly {
                        return self.wait_for_approval(run, step, state, &agent);
                    }
                    if let Some(error) = self.execute_agent_tool(&entry, &mut step, &mut state)? {
                        return self.fail_run(run, state, error, AgentRunEventKind::Failed);
                    }
                }
            }
        }
    }

    fn load_agent(&self, agent_id: Uuid) -> Result<AgentDefinition, AgentRuntimeError> {
        let agent = self
            .agent_store
            .load_agent_definition(&agent_id)
            .map_err(AgentRuntimeError::Store)?
            .ok_or(AgentRuntimeError::AgentNotFound(agent_id))?;
        if !agent.provider.eq_ignore_ascii_case(self.model.provider()) {
            return Err(AgentRuntimeError::ProviderMismatch {
                configured: agent.provider,
                runtime: self.model.provider().to_string(),
            });
        }
        Ok(agent)
    }

    fn resolve_tools(
        &self,
        agent: &AgentDefinition,
    ) -> Result<Vec<AgentFunctionTool>, AgentRuntimeError> {
        let mut names = BTreeSet::new();
        let mut tools = Vec::with_capacity(agent.tools.len());
        for allowed in &agent.tools {
            let entry = self
                .registry
                .find(&allowed.operation, &allowed.version)
                .ok_or_else(|| AgentRuntimeError::ToolNotRegistered {
                    operation: allowed.operation.clone(),
                    version: allowed.version.clone(),
                })?;
            let name = tool_name(entry);
            if !names.insert(name.clone()) {
                return Err(AgentRuntimeError::DuplicateToolName(name));
            }
            tools.push(AgentFunctionTool {
                name,
                description: entry.description.clone(),
                parameters: self.schema_value(entry, &entry.input_schema)?,
                operation: entry.operation.clone(),
                version: entry.version.clone(),
            });
        }
        Ok(tools)
    }

    fn wait_for_approval(
        &self,
        mut run: AgentRun,
        mut step: AgentRunStep,
        mut state: AgentRuntimeState,
        agent: &AgentDefinition,
    ) -> Result<AgentRuntimeOutcome, AgentRuntimeError> {
        let requested_at = Utc::now();
        if let Some(reason) = duration_budget_error(agent, &state, requested_at) {
            return self.fail_approval_for_budget(run, state, reason);
        }
        let expires_at = std::cmp::min(
            requested_at + self.approval_ttl,
            run_deadline(state.started_at, agent.limits.max_duration_seconds),
        );
        let pending = state.pending_tool.as_mut().ok_or_else(|| {
            AgentRuntimeError::InconsistentState("approval has no pending tool".to_string())
        })?;
        let request = SignedApprovalRequest::create(
            &pending.operation,
            &pending.version,
            &pending.arguments,
            requested_at,
            expires_at,
            &self.identity,
        )?;
        self.approval_store
            .save_approval_request(&request)
            .map_err(AgentRuntimeError::Store)?;
        step.wait_for_approval(request.body.id, requested_at)?;
        self.save_step(&step)?;
        run.wait_for_input(requested_at)?;
        self.save_run(&run)?;
        pending.approval_request_id = Some(request.body.id);
        self.save_state(run.id, &state)?;
        self.append_event(
            run.id,
            AgentRunEventKind::ApprovalRequired,
            json!({
                "step_id": step.id,
                "request_id": request.body.id,
                "operation": request.body.operation,
                "version": request.body.version,
                "expires_at": request.body.expires_at,
            }),
        )?;
        Ok(AgentRuntimeOutcome::WaitingForApproval { run, step, request })
    }

    fn resume_approval(
        &self,
        run: &mut AgentRun,
        agent: &AgentDefinition,
        state: &mut AgentRuntimeState,
    ) -> Result<ResumeApproval, AgentRuntimeError> {
        if let Some(reason) = duration_budget_error(agent, state, Utc::now()) {
            return Ok(ResumeApproval::BudgetExceeded(reason));
        }
        let pending = state.pending_tool.clone().ok_or_else(|| {
            AgentRuntimeError::InconsistentState("waiting run has no pending tool call".to_string())
        })?;
        let request_id = pending.approval_request_id.ok_or_else(|| {
            AgentRuntimeError::InconsistentState("waiting run has no approval request".to_string())
        })?;
        let Some(mut step) = self
            .run_store
            .find_agent_run_step_by_approval(&request_id)
            .map_err(AgentRuntimeError::Store)?
        else {
            return Ok(ResumeApproval::Failed(format!(
                "approval request {request_id} is not bound to the pending agent run step"
            )));
        };
        if step.id != pending.step_id
            || step.run_id != run.id
            || step.approval_request_id != Some(request_id)
        {
            return Ok(ResumeApproval::Failed(format!(
                "approval request {request_id} resolved to step {} in run {}, expected step {} in run {}",
                step.id, step.run_id, pending.step_id, run.id
            )));
        }
        let pending_input = canonicalize(&pending.arguments).map_err(|_| {
            AgentRuntimeError::InconsistentState(format!(
                "pending tool {} has non-canonical arguments",
                pending.step_id
            ))
        })?;
        let pending_input_digest = digest(ArtifactKind::OperationInput, &pending_input);
        if step.operation != pending.operation
            || step.version != pending.version
            || step.input_digest != pending_input_digest
        {
            return Ok(ResumeApproval::Failed(format!(
                "approval request {request_id} resolved to a step that does not match the pending operation"
            )));
        }
        let request = self
            .approval_store
            .load_approval_request(&request_id)
            .map_err(AgentRuntimeError::Store)?
            .ok_or_else(|| {
                AgentRuntimeError::InconsistentState(format!(
                    "approval request not found: {request_id}"
                ))
            })?;
        if request.body.id != request_id {
            return Ok(ResumeApproval::Failed(format!(
                "approval lookup {request_id} returned request {}",
                request.body.id
            )));
        }
        if request.body.requested_by != run.actor
            || request.body.requested_by != self.identity.principal_id
            || request.body.operation != pending.operation
            || request.body.operation != step.operation
            || request.body.version != pending.version
            || request.body.version != step.version
            || request.body.input_digest != pending_input_digest
            || request.body.input_digest != step.input_digest
        {
            return Ok(ResumeApproval::Failed(format!(
                "approval request {request_id} does not match the pending run actor and tool call"
            )));
        }
        let Some(decision) = self
            .approval_store
            .load_approval_decision(&request_id)
            .map_err(AgentRuntimeError::Store)?
        else {
            return Ok(ResumeApproval::Waiting(
                AgentRuntimeOutcome::WaitingForApproval {
                    run: run.clone(),
                    step,
                    request,
                },
            ));
        };
        let Some(approver) = self
            .approval_store
            .load_trusted_approver(&decision.body.decided_by)
            .map_err(AgentRuntimeError::Store)?
        else {
            return Ok(ResumeApproval::Failed(format!(
                "approval signer is not trusted: {}",
                decision.body.decided_by
            )));
        };
        let grant = ApprovalGrant {
            request,
            decision,
            approver: approver.clone(),
        };
        if let Err(error) =
            grant.verify_decision(&principal_from_keypair(&self.identity), &approver)
        {
            return Ok(ResumeApproval::Failed(error.to_string()));
        }
        if let Some(reason) = duration_budget_error(agent, state, Utc::now()) {
            return Ok(ResumeApproval::BudgetExceeded(reason));
        }
        if grant.decision.body.outcome == ApprovalOutcome::Denied {
            let reason = grant
                .decision
                .body
                .reason
                .clone()
                .unwrap_or_else(|| "approval request was denied".to_string());
            if step.status != AgentRunStepStatus::WaitingForApproval {
                return Ok(ResumeApproval::Failed(format!(
                    "denied tool {} was already claimed; refusing duplicate resume",
                    step.id
                )));
            }
            step.fail(&reason, Utc::now())?;
            self.save_step(&step)?;
            self.append_event(
                run.id,
                AgentRunEventKind::ApprovalResumed,
                json!({
                    "step_id": step.id,
                    "request_id": request_id,
                    "decided_by": grant.decision.body.decided_by,
                    "outcome": grant.decision.body.outcome,
                }),
            )?;
            return Ok(ResumeApproval::Failed(reason));
        }

        let stored_execution = self
            .approval_store
            .load_approval_execution(&request_id)
            .map_err(AgentRuntimeError::Store)?;
        let claimed_now = if step.status == AgentRunStepStatus::WaitingForApproval {
            step.resume_from_approval(Utc::now())?;
            self.save_step(&step)?;
            true
        } else if matches!(
            step.status,
            AgentRunStepStatus::Running | AgentRunStepStatus::Succeeded
        ) && stored_execution.is_some()
        {
            false
        } else if step.status == AgentRunStepStatus::Running {
            return Ok(ResumeApproval::Failed(format!(
                "approved tool {} was already in flight; refusing unsafe replay",
                step.id
            )));
        } else {
            return Ok(ResumeApproval::Failed(format!(
                "approval step {} cannot resume from {:?}",
                step.id, step.status
            )));
        };
        if claimed_now {
            self.append_event(
                run.id,
                AgentRunEventKind::ApprovalResumed,
                json!({
                    "step_id": step.id,
                    "request_id": request_id,
                    "decided_by": grant.decision.body.decided_by,
                    "outcome": grant.decision.body.outcome,
                }),
            )?;
        }
        if run.status == AgentRunStatus::WaitingForInput {
            run.resume(Utc::now())?;
            self.save_run(run)?;
        } else if run.status != AgentRunStatus::Running {
            return Ok(ResumeApproval::Failed(format!(
                "approval run {} cannot resume from {:?}",
                run.id, run.status
            )));
        }
        if let Some(reason) = duration_budget_error(agent, state, Utc::now()) {
            return Ok(ResumeApproval::BudgetExceeded(reason));
        }
        if let Some(execution) = stored_execution {
            if let Err(error) = grant.verify_for_execution(
                &self.identity,
                &approver,
                &pending.operation,
                &pending.version,
                &pending.arguments,
                self.identity.principal_id,
                execution.executed_at,
            ) {
                return Ok(ResumeApproval::Failed(error.to_string()));
            }
            if execution.proof.body.timestamp != execution.executed_at {
                return Ok(ResumeApproval::Failed(
                    "stored approval execution timestamp does not match its proof".to_string(),
                ));
            }
            if let Some(reason) = duration_budget_error(agent, state, Utc::now()) {
                return Ok(ResumeApproval::BudgetExceeded(reason));
            }
            if step.status == AgentRunStepStatus::Running {
                step.succeed(
                    execution.output.clone(),
                    execution.proof.clone(),
                    Utc::now(),
                )?;
                self.save_step(&step)?;
            }
            if step.status != AgentRunStepStatus::Succeeded {
                return Ok(ResumeApproval::Failed(format!(
                    "approval step {} cannot reconcile from {:?}",
                    step.id, step.status
                )));
            }
            if let Err(error) = self.validate_output(&pending, &execution.output) {
                return Ok(ResumeApproval::Failed(error));
            }
            self.record_tool_success(state, &step, execution.output)?;
            return Ok(ResumeApproval::Continue);
        }
        let timestamp = Utc::now();
        if let Some(reason) = duration_budget_error(agent, state, timestamp) {
            return Ok(ResumeApproval::BudgetExceeded(reason));
        }
        let context = self.execution_context(timestamp);
        let outcome = match self.engine.execute_with_approval_evidenced(
            &pending.operation,
            &pending.version,
            &pending.arguments,
            &context,
            &grant,
            &approver,
        ) {
            Ok(outcome) => outcome,
            Err(error) => {
                step.fail(error.to_string(), Utc::now())?;
                self.save_step(&step)?;
                return Ok(ResumeApproval::Failed(error.to_string()));
            }
        };
        let execution = ApprovalExecution {
            request_id,
            executed_at: timestamp,
            output: outcome.output.clone(),
            proof: outcome.proof.clone(),
        };
        self.approval_store
            .save_approval_execution(&execution)
            .map_err(AgentRuntimeError::Store)?;
        step.succeed(outcome.output.clone(), outcome.proof, Utc::now())?;
        self.save_step(&step)?;
        if let Err(error) = self.validate_output(&pending, &outcome.output) {
            return Ok(ResumeApproval::Failed(error));
        }
        self.record_tool_success(state, &step, outcome.output)?;
        Ok(ResumeApproval::Continue)
    }

    fn reconcile_pending_tool(
        &self,
        state: &mut AgentRuntimeState,
    ) -> Result<Option<String>, AgentRuntimeError> {
        let pending = state.pending_tool.clone().ok_or_else(|| {
            AgentRuntimeError::InconsistentState("pending tool disappeared".to_string())
        })?;
        let step = self.load_step(pending.step_id)?;
        match step.status {
            AgentRunStepStatus::Succeeded => {
                let output = step.output.clone().ok_or_else(|| {
                    AgentRuntimeError::InconsistentState(format!(
                        "succeeded step {} has no output",
                        step.id
                    ))
                })?;
                if let Err(error) = self.validate_output(&pending, &output) {
                    return Ok(Some(error));
                }
                self.record_tool_success(state, &step, output)?;
                Ok(None)
            }
            AgentRunStepStatus::Failed => {
                let error = step
                    .error
                    .clone()
                    .unwrap_or_else(|| "tool execution failed".to_string());
                self.record_tool_failure_state(state, &step, error)?;
                Ok(None)
            }
            AgentRunStepStatus::Running => {
                let error = format!(
                    "tool step {} was interrupted in flight; refusing unsafe replay",
                    step.id
                );
                let mut step = step;
                step.fail(&error, Utc::now())?;
                self.save_step(&step)?;
                Ok(Some(error))
            }
            status => Ok(Some(format!(
                "tool step {} cannot recover from {:?}",
                step.id, status
            ))),
        }
    }

    fn execute_agent_tool(
        &self,
        entry: &RegistryEntry,
        step: &mut AgentRunStep,
        state: &mut AgentRuntimeState,
    ) -> Result<Option<String>, AgentRuntimeError> {
        let pending = state.pending_tool.clone().ok_or_else(|| {
            AgentRuntimeError::InconsistentState("execution has no pending tool".to_string())
        })?;
        let context = self.execution_context(Utc::now());
        match self.engine.execute_evidenced(
            &entry.operation,
            &entry.version,
            &pending.arguments,
            &context,
        ) {
            Ok(outcome) => {
                step.succeed(outcome.output.clone(), outcome.proof, Utc::now())?;
                self.save_step(step)?;
                if let Err(error) =
                    self.validate_schema(entry, &entry.output_schema, &outcome.output)
                {
                    self.append_event(
                        step.run_id,
                        AgentRunEventKind::ToolFailed,
                        json!({"step_id": step.id, "error": error.to_string()}),
                    )?;
                    return Ok(Some(error.to_string()));
                }
                self.record_tool_success(state, step, outcome.output)?;
                Ok(None)
            }
            Err(error) => {
                self.record_tool_failure(step, state, error.to_string())?;
                Ok(None)
            }
        }
    }

    fn record_tool_success(
        &self,
        state: &mut AgentRuntimeState,
        step: &AgentRunStep,
        output: Value,
    ) -> Result<(), AgentRuntimeError> {
        let pending = state.pending_tool.take().ok_or_else(|| {
            AgentRuntimeError::InconsistentState("tool success has no pending call".to_string())
        })?;
        let proof_id = step.proof.as_ref().map(|proof| proof.body.id);
        state.next_input = ModelInput::ToolOutput {
            call_id: pending.call_id.clone(),
            output: json!({
                "ok": true,
                "result": output,
                "proof_id": proof_id,
            }),
        };
        self.save_state(step.run_id, state)?;
        self.append_event(
            step.run_id,
            AgentRunEventKind::ToolSucceeded,
            json!({
                "step_id": step.id,
                "call_id": pending.call_id,
                "operation": pending.operation,
                "version": pending.version,
                "proof_id": proof_id,
            }),
        )
    }

    fn record_tool_failure(
        &self,
        step: &mut AgentRunStep,
        state: &mut AgentRuntimeState,
        error: String,
    ) -> Result<(), AgentRuntimeError> {
        step.fail(&error, Utc::now())?;
        self.save_step(step)?;
        self.record_tool_failure_state(state, step, error)
    }

    fn record_tool_failure_state(
        &self,
        state: &mut AgentRuntimeState,
        step: &AgentRunStep,
        error: String,
    ) -> Result<(), AgentRuntimeError> {
        let pending = state.pending_tool.take().ok_or_else(|| {
            AgentRuntimeError::InconsistentState("tool failure has no pending call".to_string())
        })?;
        state.next_input = ModelInput::ToolOutput {
            call_id: pending.call_id.clone(),
            output: json!({
                "ok": false,
                "error": error,
                "operation": pending.operation,
                "version": pending.version,
            }),
        };
        self.save_state(step.run_id, state)?;
        self.append_event(
            step.run_id,
            AgentRunEventKind::ToolFailed,
            json!({
                "step_id": step.id,
                "call_id": pending.call_id,
                "error": error,
            }),
        )
    }

    fn validate_output(&self, pending: &PendingToolCall, output: &Value) -> Result<(), String> {
        let entry = self
            .registry
            .find(&pending.operation, &pending.version)
            .ok_or_else(|| {
                format!(
                    "registered tool disappeared: {}::{}",
                    pending.operation, pending.version
                )
            })?;
        self.validate_schema(entry, &entry.output_schema, output)
            .map_err(|error| error.to_string())
    }

    fn schema_value(
        &self,
        entry: &RegistryEntry,
        schema: &str,
    ) -> Result<Value, AgentRuntimeError> {
        if schema.trim_start().starts_with('{') {
            return serde_json::from_str(schema).map_err(|error| {
                AgentRuntimeError::Schema(format!(
                    "invalid inline schema for {}::{}: {error}",
                    entry.operation, entry.version
                ))
            });
        }
        let candidates = [
            self.workspace_path.join(".proof/registry").join(schema),
            self.workspace_path.join("registry").join(schema),
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("..")
                .join("registry")
                .join(schema),
        ];
        let path = candidates
            .iter()
            .find(|path| path.is_file())
            .ok_or_else(|| {
                AgentRuntimeError::Schema(format!(
                    "schema file not found for {}::{}: {schema}",
                    entry.operation, entry.version
                ))
            })?;
        let contents = std::fs::read_to_string(path).map_err(|error| {
            AgentRuntimeError::Schema(format!("could not read {}: {error}", path.display()))
        })?;
        serde_json::from_str(&contents).map_err(|error| {
            AgentRuntimeError::Schema(format!("invalid schema {}: {error}", path.display()))
        })
    }

    fn validate_schema(
        &self,
        entry: &RegistryEntry,
        schema: &str,
        value: &Value,
    ) -> Result<(), AgentRuntimeError> {
        let schema = self.schema_value(entry, schema)?;
        let validator = jsonschema::Validator::new(&schema).map_err(|error| {
            AgentRuntimeError::Schema(format!(
                "invalid schema for {}::{}: {error}",
                entry.operation, entry.version
            ))
        })?;
        let errors = validator
            .iter_errors(value)
            .map(|error| error.to_string())
            .collect::<Vec<_>>();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(AgentRuntimeError::Schema(format!(
                "{}::{} validation failed: {}",
                entry.operation,
                entry.version,
                errors.join("; ")
            )))
        }
    }

    fn execution_context(&self, timestamp: DateTime<Utc>) -> ExecutionContext {
        ExecutionContext {
            actor: self.identity.principal_id,
            principal_kind: Some(PrincipalKind::Agent),
            delegation_id: None,
            delegation_chain: None,
            workspace_path: self.workspace_path.clone(),
            timestamp,
        }
    }

    fn pre_model_budget_error(
        &self,
        agent: &AgentDefinition,
        state: &AgentRuntimeState,
        now: DateTime<Utc>,
    ) -> Option<String> {
        if state.model_calls >= agent.limits.max_model_calls {
            return Some(format!(
                "model call budget exceeded: maximum {}",
                agent.limits.max_model_calls
            ));
        }
        if elapsed_seconds(state.started_at, now) >= agent.limits.max_duration_seconds {
            return Some(format!(
                "duration budget exceeded: maximum {} seconds",
                agent.limits.max_duration_seconds
            ));
        }
        if state.total_tokens >= agent.limits.max_total_tokens {
            return Some(format!(
                "token budget exceeded: maximum {}",
                agent.limits.max_total_tokens
            ));
        }
        None
    }

    fn post_model_budget_error(
        &self,
        agent: &AgentDefinition,
        state: &AgentRuntimeState,
        now: DateTime<Utc>,
    ) -> Option<String> {
        if state.total_tokens > agent.limits.max_total_tokens {
            return Some(format!(
                "token budget exceeded: used {}, maximum {}",
                state.total_tokens, agent.limits.max_total_tokens
            ));
        }
        if elapsed_seconds(state.started_at, now) > agent.limits.max_duration_seconds {
            return Some(format!(
                "duration budget exceeded: maximum {} seconds",
                agent.limits.max_duration_seconds
            ));
        }
        if let Some(maximum) = agent.limits.max_cost_microusd {
            let Some(actual) = state.cost_microusd else {
                return Some(
                    "cost budget configured but provider did not report cost usage".to_string(),
                );
            };
            if actual > maximum {
                return Some(format!(
                    "cost budget exceeded: used {actual} microusd, maximum {maximum}"
                ));
            }
        }
        None
    }

    fn finish_terminal_state(
        &self,
        mut run: AgentRun,
        state: AgentRuntimeState,
    ) -> Result<AgentRuntimeOutcome, AgentRuntimeError> {
        if let Some(error) = state.terminal_error.clone() {
            let event_kind = self
                .failure_event_kind(run.id, &state)?
                .unwrap_or(AgentRunEventKind::Failed);
            return self.seal_failure(run, state, error, event_kind);
        }
        let output = state.final_output.clone().ok_or_else(|| {
            AgentRuntimeError::InconsistentState("terminal state has no result".to_string())
        })?;
        if !run.status.is_terminal() {
            run.succeed(Utc::now())?;
            self.save_run(&run)?;
        }
        let evaluation = self.ensure_evaluation(
            &run,
            AgentEvaluationOutcome::Passed,
            Some(10_000),
            &state,
            Some("agent completed within configured budgets".to_string()),
        )?;
        self.ensure_terminal_event(
            run.id,
            AgentRunEventKind::Completed,
            json!({"output": output, "evaluation_id": evaluation.id}),
        )?;
        Ok(AgentRuntimeOutcome::Completed {
            run,
            output,
            evaluation,
        })
    }

    fn fail_run(
        &self,
        run: AgentRun,
        mut state: AgentRuntimeState,
        error: String,
        event_kind: AgentRunEventKind,
    ) -> Result<AgentRuntimeOutcome, AgentRuntimeError> {
        state.terminal_error = Some(error.clone());
        self.save_terminal_state(run.id, &state, event_kind)?;
        self.seal_failure(run, state, error, event_kind)
    }

    fn fail_approval_for_budget(
        &self,
        run: AgentRun,
        state: AgentRuntimeState,
        reason: String,
    ) -> Result<AgentRuntimeOutcome, AgentRuntimeError> {
        if let Some(pending) = &state.pending_tool {
            let mut step = self.load_step(pending.step_id)?;
            if matches!(
                step.status,
                AgentRunStepStatus::Running | AgentRunStepStatus::WaitingForApproval
            ) {
                step.fail(&reason, Utc::now())?;
                self.save_step(&step)?;
                self.append_event(
                    run.id,
                    AgentRunEventKind::ToolFailed,
                    json!({
                        "step_id": step.id,
                        "call_id": pending.call_id,
                        "error": reason,
                    }),
                )?;
            }
        }
        self.fail_run(run, state, reason, AgentRunEventKind::BudgetExceeded)
    }

    fn seal_failure(
        &self,
        mut run: AgentRun,
        state: AgentRuntimeState,
        error: String,
        event_kind: AgentRunEventKind,
    ) -> Result<AgentRuntimeOutcome, AgentRuntimeError> {
        if !matches!(
            event_kind,
            AgentRunEventKind::Failed | AgentRunEventKind::BudgetExceeded
        ) {
            return Err(AgentRuntimeError::InconsistentState(format!(
                "invalid terminal failure event kind: {event_kind:?}"
            )));
        }
        if !run.status.is_terminal() {
            run.fail(Utc::now())?;
            self.save_run(&run)?;
        }
        let evaluation = self.ensure_evaluation(
            &run,
            AgentEvaluationOutcome::Failed,
            Some(0),
            &state,
            Some(error.clone()),
        )?;
        self.ensure_terminal_event(
            run.id,
            event_kind,
            json!({"error": error, "evaluation_id": evaluation.id}),
        )?;
        Ok(AgentRuntimeOutcome::Failed {
            run,
            error,
            evaluation,
        })
    }

    fn terminal_outcome(
        &self,
        run: AgentRun,
        state: AgentRuntimeState,
    ) -> Result<AgentRuntimeOutcome, AgentRuntimeError> {
        match run.status {
            AgentRunStatus::Succeeded => self.finish_terminal_state(run, state),
            AgentRunStatus::Failed => self.replay_or_seal_terminal_failure(run, state),
            status => Err(AgentRuntimeError::RunNotResumable {
                run_id: run.id,
                status,
            }),
        }
    }

    fn replay_or_seal_terminal_failure(
        &self,
        run: AgentRun,
        state: AgentRuntimeState,
    ) -> Result<AgentRuntimeOutcome, AgentRuntimeError> {
        let terminal_events = self
            .agent_store
            .list_agent_run_events(&run.id)
            .map_err(AgentRuntimeError::Store)?
            .into_iter()
            .filter(|event| {
                matches!(
                    event.kind,
                    AgentRunEventKind::Failed | AgentRunEventKind::BudgetExceeded
                )
            })
            .collect::<Vec<_>>();
        if terminal_events.len() > 1 {
            return Err(AgentRuntimeError::InconsistentState(format!(
                "agent run {} has multiple terminal failure events",
                run.id
            )));
        }
        if let Some(event) = terminal_events.into_iter().next() {
            let mut evaluations = self
                .run_store
                .list_agent_run_evaluations(&run.id)
                .map_err(AgentRuntimeError::Store)?
                .into_iter()
                .filter(|evaluation| evaluation.evaluator == RUNTIME_EVALUATOR);
            let evaluation = evaluations.next().ok_or_else(|| {
                AgentRuntimeError::InconsistentState(format!(
                    "sealed agent run {} has no runtime evaluation",
                    run.id
                ))
            })?;
            if evaluations.next().is_some() {
                return Err(AgentRuntimeError::InconsistentState(format!(
                    "sealed agent run {} has multiple runtime evaluations",
                    run.id
                )));
            }
            if evaluation.outcome != AgentEvaluationOutcome::Failed {
                return Err(AgentRuntimeError::InconsistentState(format!(
                    "sealed failed agent run {} has a passing runtime evaluation",
                    run.id
                )));
            }
            let evaluation_id = event
                .data
                .get("evaluation_id")
                .and_then(Value::as_str)
                .and_then(|value| Uuid::parse_str(value).ok())
                .ok_or_else(|| {
                    AgentRuntimeError::InconsistentState(format!(
                        "terminal event {} has no valid evaluation binding",
                        event.id
                    ))
                })?;
            if evaluation_id != evaluation.id {
                return Err(AgentRuntimeError::InconsistentState(format!(
                    "terminal event {} evaluation binding does not match {}",
                    event.id, evaluation.id
                )));
            }
            let error = event
                .data
                .get("error")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    AgentRuntimeError::InconsistentState(format!(
                        "terminal event {} has no failure reason",
                        event.id
                    ))
                })?
                .to_string();
            if state
                .terminal_error
                .as_ref()
                .is_some_and(|stored| stored != &error)
            {
                return Err(AgentRuntimeError::InconsistentState(format!(
                    "terminal event {} failure reason does not match the runtime checkpoint",
                    event.id
                )));
            }
            return Ok(AgentRuntimeOutcome::Failed {
                run,
                error,
                evaluation,
            });
        }

        let error = state
            .terminal_error
            .clone()
            .unwrap_or_else(|| "agent run failed before recording a terminal reason".to_string());
        let event_kind = self
            .failure_event_kind(run.id, &state)?
            .unwrap_or(AgentRunEventKind::Failed);
        self.seal_failure(run, state, error, event_kind)
    }

    fn failure_event_kind(
        &self,
        run_id: Uuid,
        state: &AgentRuntimeState,
    ) -> Result<Option<AgentRunEventKind>, AgentRuntimeError> {
        let mut persisted = self
            .agent_store
            .list_agent_run_events(&run_id)
            .map_err(AgentRuntimeError::Store)?
            .into_iter()
            .filter_map(|event| match event.kind {
                AgentRunEventKind::Failed | AgentRunEventKind::BudgetExceeded => Some(event.kind),
                _ => None,
            });
        let event_kind = persisted.next();
        if persisted.next().is_some() {
            return Err(AgentRuntimeError::InconsistentState(format!(
                "agent run {run_id} has multiple terminal failure events"
            )));
        }
        if event_kind.is_some() {
            return Ok(event_kind);
        }

        let checkpoints = self
            .run_store
            .list_agent_checkpoints(&run_id)
            .map_err(AgentRuntimeError::Store)?;
        if let Some(value) = checkpoints
            .iter()
            .rev()
            .find(|checkpoint| {
                checkpoint.state.get("kind").and_then(Value::as_str)
                    == Some(RUNTIME_CHECKPOINT_KIND)
            })
            .and_then(|checkpoint| checkpoint.state.get("terminal_event_kind"))
        {
            let kind: AgentRunEventKind = serde_json::from_value(value.clone())
                .map_err(|_| AgentRuntimeError::InvalidCheckpoint(run_id))?;
            if !matches!(
                kind,
                AgentRunEventKind::Failed | AgentRunEventKind::BudgetExceeded
            ) {
                return Err(AgentRuntimeError::InvalidCheckpoint(run_id));
            }
            return Ok(Some(kind));
        }
        Ok(state.terminal_error.as_deref().map(|error| {
            if legacy_budget_failure(error) {
                AgentRunEventKind::BudgetExceeded
            } else {
                AgentRunEventKind::Failed
            }
        }))
    }

    fn ensure_evaluation(
        &self,
        run: &AgentRun,
        outcome: AgentEvaluationOutcome,
        score_bps: Option<u16>,
        state: &AgentRuntimeState,
        summary: Option<String>,
    ) -> Result<AgentRunEvaluation, AgentRuntimeError> {
        if let Some(existing) = self
            .run_store
            .list_agent_run_evaluations(&run.id)
            .map_err(AgentRuntimeError::Store)?
            .into_iter()
            .find(|evaluation| evaluation.evaluator == RUNTIME_EVALUATOR)
        {
            return Ok(existing);
        }
        let evaluation = AgentRunEvaluation::create(
            run,
            RUNTIME_EVALUATOR,
            outcome,
            score_bps,
            metrics(state),
            summary,
            Utc::now(),
        )?;
        self.run_store
            .save_agent_run_evaluation(&evaluation)
            .map_err(AgentRuntimeError::Store)?;
        Ok(evaluation)
    }

    fn ensure_terminal_event(
        &self,
        run_id: Uuid,
        kind: AgentRunEventKind,
        data: Value,
    ) -> Result<(), AgentRuntimeError> {
        let events = self
            .agent_store
            .list_agent_run_events(&run_id)
            .map_err(AgentRuntimeError::Store)?;
        let matching = events
            .iter()
            .filter(|event| event.kind == kind)
            .collect::<Vec<_>>();
        if matching.is_empty() {
            self.append_event(run_id, kind, data.clone())?;
            let reread = self
                .agent_store
                .list_agent_run_events(&run_id)
                .map_err(AgentRuntimeError::Store)?;
            let matching = reread
                .iter()
                .filter(|event| event.kind == kind)
                .collect::<Vec<_>>();
            if matching.len() != 1 {
                return Err(AgentRuntimeError::InconsistentState(format!(
                    "terminal event {kind:?} is not exact-one"
                )));
            }
            return validate_exact_event_record(run_id, kind, &data, matching[0]).map_err(|_| {
                AgentRuntimeError::InconsistentState(format!(
                    "terminal event {kind:?} failed exact reread"
                ))
            });
        }
        if matching.len() != 1 {
            return Err(AgentRuntimeError::InconsistentState(format!(
                "terminal event {kind:?} is not exact-one"
            )));
        }
        validate_exact_event_record(run_id, kind, &data, matching[0]).map_err(|_| {
            AgentRuntimeError::InconsistentState(format!(
                "terminal event {kind:?} data or digest is inconsistent"
            ))
        })
    }

    fn save_state(&self, run_id: Uuid, state: &AgentRuntimeState) -> Result<(), AgentRuntimeError> {
        self.save_state_checkpoint(run_id, state, None)
    }

    fn save_terminal_state(
        &self,
        run_id: Uuid,
        state: &AgentRuntimeState,
        event_kind: AgentRunEventKind,
    ) -> Result<(), AgentRuntimeError> {
        self.save_state_checkpoint(run_id, state, Some(event_kind))
    }

    fn save_state_checkpoint(
        &self,
        run_id: Uuid,
        state: &AgentRuntimeState,
        terminal_event_kind: Option<AgentRunEventKind>,
    ) -> Result<(), AgentRuntimeError> {
        let checkpoints = self
            .run_store
            .list_agent_checkpoints(&run_id)
            .map_err(AgentRuntimeError::Store)?;
        let sequence = next_sequence(checkpoints.last().map(|checkpoint| checkpoint.sequence))?;
        let mut checkpoint_state = json!({"kind": RUNTIME_CHECKPOINT_KIND, "runtime": state});
        if let Some(kind) = terminal_event_kind {
            checkpoint_state["terminal_event_kind"] = json!(kind);
        }
        let checkpoint = AgentCheckpoint::create(run_id, sequence, checkpoint_state, Utc::now())?;
        self.run_store
            .save_agent_checkpoint(&checkpoint)
            .map_err(AgentRuntimeError::Store)
    }

    fn append_event(
        &self,
        run_id: Uuid,
        kind: AgentRunEventKind,
        data: Value,
    ) -> Result<(), AgentRuntimeError> {
        let events = self
            .agent_store
            .list_agent_run_events(&run_id)
            .map_err(AgentRuntimeError::Store)?;
        let sequence = next_sequence(events.last().map(|event| event.sequence))?;
        let event = AgentRunEvent::create(run_id, sequence, kind, data, Utc::now())
            .map_err(|error| AgentRuntimeError::InconsistentState(error.to_string()))?;
        self.agent_store
            .save_agent_run_event(&event)
            .map_err(AgentRuntimeError::Store)
    }

    fn ensure_exact_live_event(
        &self,
        run_id: Uuid,
        kind: AgentRunEventKind,
        data: Value,
    ) -> Result<(), AgentRuntimeError> {
        let matching = self
            .agent_store
            .list_agent_run_events(&run_id)
            .map_err(AgentRuntimeError::Store)?
            .into_iter()
            .filter(|event| event.kind == kind)
            .collect::<Vec<_>>();
        if matching.is_empty() {
            self.append_event(run_id, kind, data.clone())?;
        } else if matching.len() != 1
            || validate_exact_event_record(run_id, kind, &data, &matching[0]).is_err()
        {
            return Err(AgentRuntimeError::InconsistentState(format!(
                "live event {kind:?} evidence is not exact-one"
            )));
        }
        let reread = self
            .agent_store
            .list_agent_run_events(&run_id)
            .map_err(AgentRuntimeError::Store)?
            .into_iter()
            .filter(|event| event.kind == kind)
            .collect::<Vec<_>>();
        if reread.len() != 1
            || validate_exact_event_record(run_id, kind, &data, &reread[0]).is_err()
        {
            return Err(AgentRuntimeError::InconsistentState(format!(
                "live event {kind:?} failed exact reread"
            )));
        }
        Ok(())
    }

    fn save_run(&self, run: &AgentRun) -> Result<(), AgentRuntimeError> {
        self.run_store
            .save_agent_run(run)
            .map_err(AgentRuntimeError::Store)
    }

    fn save_step(&self, step: &AgentRunStep) -> Result<(), AgentRuntimeError> {
        self.run_store
            .save_agent_run_step(step)
            .map_err(AgentRuntimeError::Store)
    }

    fn persist_exact_live_approval_request(
        &self,
        expected: &SignedApprovalRequest,
    ) -> Result<SignedApprovalRequest, AgentRuntimeError> {
        let requester = principal_from_keypair(&self.identity);
        expected.verify(&requester).map_err(|_| {
            AgentRuntimeError::InconsistentState(
                "checkpointed live approval request signature is invalid".to_string(),
            )
        })?;
        match self
            .approval_store
            .load_approval_request(&expected.body.id)
            .map_err(AgentRuntimeError::Store)?
        {
            Some(existing) if existing != *expected => {
                return Err(AgentRuntimeError::InconsistentState(
                    "persisted live approval request differs from checkpoint intent".to_string(),
                ));
            }
            Some(_) => {}
            None => self
                .approval_store
                .save_approval_request(expected)
                .map_err(AgentRuntimeError::Store)?,
        }
        let reread = self
            .approval_store
            .load_approval_request(&expected.body.id)
            .map_err(AgentRuntimeError::Store)?
            .ok_or_else(|| {
                AgentRuntimeError::InconsistentState(
                    "live approval request was not durable after save".to_string(),
                )
            })?;
        if reread != *expected || reread.verify(&requester).is_err() {
            return Err(AgentRuntimeError::InconsistentState(
                "live approval request failed exact signed reread".to_string(),
            ));
        }
        Ok(reread)
    }

    fn materialize_live_step_intent(
        &self,
        intent: &LiveStepIntent,
    ) -> Result<AgentRunStep, AgentRuntimeError> {
        let waiting = intent.as_step();
        let mut pending = waiting.clone();
        pending.status = AgentRunStepStatus::Pending;
        pending.approval_request_id = None;
        pending.revision = 0;
        pending.updated_at = pending.created_at;
        pending.started_at = None;
        let mut running = pending.clone();
        running.status = AgentRunStepStatus::Running;
        running.revision = 1;
        running.updated_at = intent.started_at;
        running.started_at = Some(intent.started_at);

        let mut current = self
            .run_store
            .load_agent_run_step(&intent.id)
            .map_err(AgentRuntimeError::Store)?;
        if current.is_none() {
            self.save_step(&pending)?;
            current = Some(pending.clone());
        }
        if current.as_ref() == Some(&pending) {
            self.save_step(&running)?;
            current = Some(running.clone());
        }
        if current.as_ref() == Some(&running) {
            self.save_step(&waiting)?;
            current = Some(waiting.clone());
        }
        current.ok_or_else(|| {
            AgentRuntimeError::InconsistentState("live step intent did not materialize".to_string())
        })
    }

    fn load_step(&self, step_id: Uuid) -> Result<AgentRunStep, AgentRuntimeError> {
        self.run_store
            .load_agent_run_step(&step_id)
            .map_err(AgentRuntimeError::Store)?
            .ok_or_else(|| {
                AgentRuntimeError::InconsistentState(format!("agent run step not found: {step_id}"))
            })
    }
}

impl AgentRuntime {
    fn commit_live_response(
        &self,
        run: AgentRun,
        _agent: AgentDefinition,
        state: &mut LiveRuntimeState,
        turn: crate::model::ModelTurn,
    ) -> Result<AgentRuntimeOutcome, AgentRuntimeError> {
        if Utc::now() >= run_deadline(state.started_at, 300) {
            let attempt = state
                .attempts
                .last_mut()
                .expect("dispatching attempt exists");
            attempt.state = ProviderAttemptState::FailedTerminal;
            attempt.failure = Some(ProviderFailure::terminal("deadline_exceeded"));
            attempt.finished_at = Some(Utc::now());
            state.terminal_error = Some("live wall-clock deadline exceeded".to_string());
            self.save_live_state(run.id, state)?;
            return self.fail_live_run(run, state.clone(), AgentRunEventKind::BudgetExceeded);
        }
        let Some(response_body_digest) = turn.response_body_digest else {
            let attempt = state
                .attempts
                .last_mut()
                .expect("dispatching attempt exists");
            attempt.state = ProviderAttemptState::Ambiguous;
            attempt.failure = Some(ProviderFailure::ambiguous("missing_response_body_digest"));
            attempt.finished_at = Some(Utc::now());
            state.terminal_error =
                Some("live response is missing response body evidence".to_string());
            self.save_live_state(run.id, state)?;
            return self.fail_live_run(run, state.clone(), AgentRunEventKind::Failed);
        };
        let Some(returned_model) = turn.returned_model.as_deref() else {
            let attempt = state
                .attempts
                .last_mut()
                .expect("dispatching attempt exists");
            attempt.state = ProviderAttemptState::Ambiguous;
            attempt.failure = Some(ProviderFailure::ambiguous("missing_returned_model"));
            attempt.finished_at = Some(Utc::now());
            state.terminal_error = Some("provider response is missing returned model".to_string());
            self.save_live_state(run.id, state)?;
            return self.fail_live_run(run, state.clone(), AgentRunEventKind::Failed);
        };
        if returned_model != LIVE_MODEL {
            let attempt = state
                .attempts
                .last_mut()
                .expect("dispatching attempt exists");
            attempt.state = ProviderAttemptState::FailedTerminal;
            attempt.failure = Some(ProviderFailure::terminal("returned_model_mismatch"));
            attempt.finished_at = Some(Utc::now());
            state.terminal_error =
                Some("provider returned model does not match gpt-5.6-sol".to_string());
            self.save_live_state(run.id, state)?;
            return self.fail_live_run(run, state.clone(), AgentRunEventKind::Failed);
        }
        if turn.usage.input_tokens == 0
            || turn.usage.output_tokens == 0
            || turn.usage.total_tokens == 0
            || turn.usage.total_tokens
                != turn
                    .usage
                    .input_tokens
                    .checked_add(turn.usage.output_tokens)
                    .ok_or_else(|| {
                        AgentRuntimeError::LiveSetup("response usage overflow".to_string())
                    })?
        {
            let attempt = state
                .attempts
                .last_mut()
                .expect("dispatching attempt exists");
            attempt.state = ProviderAttemptState::Ambiguous;
            attempt.failure = Some(ProviderFailure::ambiguous("invalid_response_usage"));
            attempt.finished_at = Some(Utc::now());
            state.terminal_error = Some("live response has missing or malformed usage".to_string());
            self.save_live_state(run.id, state)?;
            return self.fail_live_run(run, state.clone(), AgentRunEventKind::Failed);
        }
        if turn.usage.output_tokens > 1024 {
            let attempt = state
                .attempts
                .last_mut()
                .expect("dispatching attempt exists");
            attempt.state = ProviderAttemptState::FailedTerminal;
            attempt.failure = Some(ProviderFailure::terminal("output_token_limit_exceeded"));
            attempt.finished_at = Some(Utc::now());
            state.terminal_error = Some("live per-call output token limit exceeded".to_string());
            self.save_live_state(run.id, state)?;
            return self.fail_live_run(run, state.clone(), AgentRunEventKind::BudgetExceeded);
        }
        let input_cost = turn
            .usage
            .input_tokens
            .checked_mul(5)
            .ok_or_else(|| AgentRuntimeError::LiveSetup("live cost overflow".to_string()))?;
        let output_cost = turn
            .usage
            .output_tokens
            .checked_mul(20)
            .ok_or_else(|| AgentRuntimeError::LiveSetup("live cost overflow".to_string()))?;
        let cost = input_cost
            .checked_add(output_cost)
            .ok_or_else(|| AgentRuntimeError::LiveSetup("live cost overflow".to_string()))?;
        let new_input = state
            .cumulative_usage
            .input_tokens
            .checked_add(turn.usage.input_tokens)
            .ok_or_else(|| AgentRuntimeError::LiveSetup("live input-token overflow".to_string()))?;
        let new_output = state
            .cumulative_usage
            .output_tokens
            .checked_add(turn.usage.output_tokens)
            .ok_or_else(|| {
                AgentRuntimeError::LiveSetup("live output-token overflow".to_string())
            })?;
        let new_total = state
            .cumulative_usage
            .total_tokens
            .checked_add(turn.usage.total_tokens)
            .ok_or_else(|| AgentRuntimeError::LiveSetup("live token overflow".to_string()))?;
        let prior_cost = state.cumulative_cost.calculated_cost_microusd;
        let cumulative_cost = prior_cost.checked_add(cost).ok_or_else(|| {
            AgentRuntimeError::LiveSetup("live calculated-cost overflow".to_string())
        })?;
        if new_total > 10_000 || cumulative_cost > 120_000 {
            let attempt = state
                .attempts
                .last_mut()
                .expect("dispatching attempt exists");
            attempt.state = ProviderAttemptState::FailedTerminal;
            attempt.failure = Some(ProviderFailure::terminal("token_or_cost_limit_exceeded"));
            attempt.finished_at = Some(Utc::now());
            state.terminal_error = Some("live token/cost limit exceeded".to_string());
            self.save_live_state(run.id, state)?;
            return self.fail_live_run(run, state.clone(), AgentRunEventKind::BudgetExceeded);
        }
        let provider_cost_status = if turn.usage.cost_microusd.is_some() {
            ProviderCostStatus::Reported
        } else {
            ProviderCostStatus::Unavailable
        };
        let committed_decision = match LiveCommittedDecision::from_model(&turn.decision) {
            Ok(decision) => decision,
            Err(_) => {
                let attempt = state
                    .attempts
                    .last_mut()
                    .expect("dispatching attempt exists");
                attempt.state = ProviderAttemptState::Ambiguous;
                attempt.failure = Some(ProviderFailure::ambiguous("invalid_decision_shape"));
                attempt.finished_at = Some(Utc::now());
                state.terminal_error =
                    Some("live response decision is not the strict synthetic shape".to_string());
                self.save_live_state(run.id, state)?;
                return self.fail_live_run(run, state.clone(), AgentRunEventKind::Failed);
            }
        };
        let prior_provider_cost = state.cumulative_cost.provider_cost_microusd;
        let cumulative_provider_cost = if state.counters.logical_model_turns == 0 {
            turn.usage.cost_microusd
        } else {
            match (prior_provider_cost, turn.usage.cost_microusd) {
                (Some(prior), Some(current)) => {
                    Some(prior.checked_add(current).ok_or_else(|| {
                        AgentRuntimeError::LiveSetup("provider cost overflow".to_string())
                    })?)
                }
                // A missing report is unavailable, never a synthetic zero.
                _ => None,
            }
        };
        let pricing_schedule_digest = state.cumulative_cost.pricing_schedule_digest;
        let response = ProviderResponse {
            response_id: turn.response_id.clone(),
            returned_model: LIVE_MODEL.to_string(),
            response_body_digest,
            decision_digest: committed_decision.digest()?,
            usage: LiveUsage {
                input_tokens: turn.usage.input_tokens,
                output_tokens: turn.usage.output_tokens,
                total_tokens: turn.usage.total_tokens,
            },
            provider_cost_microusd: turn.usage.cost_microusd,
            provider_cost_status,
            calculated_cost_microusd: cost,
            cumulative_input_tokens: new_input,
            cumulative_output_tokens: new_output,
            cumulative_total_tokens: new_total,
            cumulative_provider_cost_microusd: cumulative_provider_cost,
            cumulative_provider_cost_status: if cumulative_provider_cost.is_some() {
                ProviderCostStatus::Reported
            } else {
                ProviderCostStatus::Unavailable
            },
            cumulative_calculated_cost_microusd: cumulative_cost,
            pricing_schedule_id: "proof-openai-gpt-5.6-sol-pricing/2026-08-30".to_string(),
            pricing_schedule_digest,
        };
        {
            let attempt = state.attempts.last_mut().expect("response attempt exists");
            attempt.state = ProviderAttemptState::ResponseReceived;
            attempt.response = Some(response);
        }
        self.save_live_state(run.id, state)?;
        self.reread_live_state(run.id, state)?;
        state.cumulative_usage.input_tokens = new_input;
        state.cumulative_usage.output_tokens = new_output;
        state.cumulative_usage.total_tokens = new_total;
        state.counters.logical_model_turns = state
            .counters
            .logical_model_turns
            .checked_add(1)
            .ok_or_else(|| AgentRuntimeError::LiveSetup("logical turn overflow".to_string()))?;
        state.previous_response_id = Some(turn.response_id.clone());
        state.cumulative_cost.calculated_cost_microusd = cumulative_cost;
        state.cumulative_cost.provider_cost_microusd = cumulative_provider_cost;
        state.cumulative_cost.provider_cost_status = if cumulative_provider_cost.is_some() {
            ProviderCostStatus::Reported
        } else {
            ProviderCostStatus::Unavailable
        };
        {
            let attempt = state.attempts.last_mut().expect("response attempt exists");
            attempt.state = ProviderAttemptState::Committed;
            attempt.finished_at = Some(Utc::now());
        }
        self.save_live_state(run.id, state)?;
        let persisted_attempt = state
            .attempts
            .last()
            .ok_or_else(|| AgentRuntimeError::InvalidCheckpoint(run.id))?;
        let committed = serde_json::to_value(LiveModelRespondedEvent::from_attempt(
            persisted_attempt,
            committed_decision,
        )?)
        .map_err(|error| AgentRuntimeError::LiveSetup(error.to_string()))?;
        self.append_event(run.id, AgentRunEventKind::ModelResponded, committed.clone())?;
        self.reread_live_state(run.id, state)?;
        self.reread_live_event(run.id, AgentRunEventKind::ModelResponded, &committed)?;
        match turn.decision {
            ModelDecision::Finish { output } => {
                if state.counters.successful_publication_mutations != 1 {
                    state.terminal_error = Some(
                        "model finished before exactly one governed publication succeeded"
                            .to_string(),
                    );
                    self.save_live_state(run.id, state)?;
                    return self.fail_live_run(run, state.clone(), AgentRunEventKind::Failed);
                }
                state.final_output = Some(output);
                self.save_live_state(run.id, state)?;
                self.live_terminal_outcome(run, state.clone(), false)
            }
            ModelDecision::ToolCall {
                call_id,
                name,
                arguments,
            } => self.wait_live_approval(run, state, call_id, name, arguments),
        }
    }

    fn wait_live_approval(
        &self,
        mut run: AgentRun,
        state: &mut LiveRuntimeState,
        call_id: String,
        name: String,
        arguments: Value,
    ) -> Result<AgentRuntimeOutcome, AgentRuntimeError> {
        if Utc::now() >= run_deadline(state.started_at, 300) {
            state.terminal_error =
                Some("live wall-clock deadline exceeded before approval request".to_string());
            self.save_live_state(run.id, state)?;
            return self.fail_live_run(run, state.clone(), AgentRunEventKind::BudgetExceeded);
        }
        if state.counters.tool_attempts >= 1 || state.pending_tool.is_some() {
            state.terminal_error =
                Some("live policy permits exactly one requested publication tool call".to_string());
            self.save_live_state(run.id, state)?;
            return self.fail_live_run(run, state.clone(), AgentRunEventKind::Failed);
        }
        if name != LIVE_TOOL_NAME
            || arguments != expected_live_arguments(&state.policy_evidence.resolved_bindings)?
        {
            state.terminal_error = Some(
                "model tool call does not match frozen release.publish::v2 arguments".to_string(),
            );
            self.save_live_state(run.id, state)?;
            return self.fail_live_run(run, state.clone(), AgentRunEventKind::Failed);
        }
        let arguments_record: ReleasePublishArguments = serde_json::from_value(arguments.clone())
            .map_err(|error| {
            AgentRuntimeError::LiveSetup(format!("live tool arguments are invalid: {error}"))
        })?;
        let entry = self.registry.find("release.publish", "v2").ok_or_else(|| {
            AgentRuntimeError::ToolNotRegistered {
                operation: "release.publish".to_string(),
                version: "v2".to_string(),
            }
        })?;
        self.validate_schema(entry, &entry.input_schema, &arguments)?;
        let now = Utc::now();
        let mut step = AgentRunStep::new(
            run.id,
            state.counters.tool_attempts,
            "release.publish",
            "v2",
            &arguments,
            now,
        )?;
        step.start(now)?;
        let deadline = run_deadline(state.started_at, 300);
        let request = SignedApprovalRequest::create(
            "release.publish",
            "v2",
            &arguments,
            now,
            std::cmp::min(now + self.approval_ttl, deadline),
            &self.identity,
        )?;
        step.wait_for_approval(request.body.id, now)?;
        run.wait_for_input(now)?;
        state.counters.tool_attempts += 1;
        state.pending_tool = Some(LivePendingToolCall {
            call_id,
            tool_name: LIVE_TOOL_NAME.to_string(),
            operation: "release.publish".to_string(),
            version: "v2".to_string(),
            arguments: arguments_record,
            step_id: step.id,
            approval_request_id: request.body.id,
            request_process_epoch_id: state.process_epoch_id,
            step_intent: LiveStepIntent::from_waiting(&step)?,
            approval_request: LiveSignedApprovalRequest::from(&request),
        });
        self.save_live_state(run.id, state)?;
        self.reread_live_state(run.id, state)?;
        let request = self.persist_exact_live_approval_request(&request)?;
        let materialized_step = self.materialize_live_step_intent(
            &state
                .pending_tool
                .as_ref()
                .expect("saved intent")
                .step_intent,
        )?;
        if materialized_step != step {
            return Err(AgentRuntimeError::InconsistentState(
                "live approval step failed exact materialization".to_string(),
            ));
        }
        self.save_run(&run)?;
        self.ensure_exact_live_event(
            run.id,
            AgentRunEventKind::ToolRequested,
            json!({
                "step_id": step.id,
                "call_id": state.pending_tool.as_ref().expect("pending tool saved").call_id,
                "operation": "release.publish",
                "version": "v2",
                "input_digest": step.input_digest,
                "live": true,
            }),
        )?;
        self.ensure_exact_live_event(run.id, AgentRunEventKind::ApprovalRequired, json!({"step_id": step.id, "request_id": request.body.id, "process_epoch_id": state.process_epoch_id, "live": true}))?;
        Ok(AgentRuntimeOutcome::WaitingForApproval { run, step, request })
    }

    fn resume_live_approval(
        &self,
        mut run: AgentRun,
        agent: AgentDefinition,
        setup: LiveRunSetup,
        approval_resume_epoch: Uuid,
        state: &mut LiveRuntimeState,
    ) -> Result<AgentRuntimeOutcome, AgentRuntimeError> {
        let pending = state.pending_tool.clone().ok_or_else(|| {
            AgentRuntimeError::InconsistentState("live approval has no pending tool".to_string())
        })?;
        let request_id = pending.approval_request_id;
        let pending_arguments = pending.arguments.as_value()?;
        let expected_request: SignedApprovalRequest = pending.approval_request.clone().into();
        let request = match self.persist_exact_live_approval_request(&expected_request) {
            Ok(request) => request,
            Err(AgentRuntimeError::Store(error)) => {
                return Err(AgentRuntimeError::Store(error));
            }
            Err(_) => {
                state.terminal_error =
                    Some("approval intent request evidence is invalid".to_string());
                self.save_live_state(run.id, state)?;
                return self.fail_live_run(run, state.clone(), AgentRunEventKind::Failed);
            }
        };
        let expected_step = pending.step_intent.as_step();
        let mut step = self.materialize_live_step_intent(&pending.step_intent)?;
        if step.id != expected_step.id
            || step.run_id != expected_step.run_id
            || step.ordinal != expected_step.ordinal
            || step.attempt != expected_step.attempt
            || step.retry_of != expected_step.retry_of
            || step.operation != expected_step.operation
            || step.version != expected_step.version
            || step.input_digest != expected_step.input_digest
            || step.approval_request_id != expected_step.approval_request_id
        {
            state.terminal_error = Some("approval intent step evidence drifted".to_string());
            self.save_live_state(run.id, state)?;
            return self.fail_live_run(run, state.clone(), AgentRunEventKind::Failed);
        }
        if run.status == AgentRunStatus::Running
            && step.status == AgentRunStepStatus::WaitingForApproval
        {
            run.wait_for_input(pending.step_intent.updated_at)?;
            self.save_run(&run)?;
        }
        let tool_requested = json!({
            "step_id": pending.step_id,
            "call_id": pending.call_id,
            "operation": "release.publish",
            "version": "v2",
            "input_digest": pending.step_intent.input_digest,
            "live": true,
        });
        let approval_required = json!({
            "step_id": pending.step_id,
            "request_id": request_id,
            "process_epoch_id": pending.request_process_epoch_id,
            "live": true,
        });
        for (kind, data) in [
            (AgentRunEventKind::ToolRequested, tool_requested),
            (AgentRunEventKind::ApprovalRequired, approval_required),
        ] {
            match self.ensure_exact_live_event(run.id, kind, data) {
                Ok(()) => {}
                Err(AgentRuntimeError::Store(error)) => {
                    return Err(AgentRuntimeError::Store(error));
                }
                Err(_) => {
                    state.terminal_error =
                        Some("approval intent event evidence drifted".to_string());
                    self.save_live_state(run.id, state)?;
                    return self.fail_live_run(run, state.clone(), AgentRunEventKind::Failed);
                }
            }
        }
        if Utc::now() >= run_deadline(state.started_at, 300) {
            state.terminal_error =
                Some("live wall-clock deadline exceeded before approval execution".to_string());
            self.save_live_state(run.id, state)?;
            return self.fail_live_run(run, state.clone(), AgentRunEventKind::BudgetExceeded);
        }
        let Some(decision) = self
            .approval_store
            .load_approval_decision(&request_id)
            .map_err(AgentRuntimeError::Store)?
        else {
            return Ok(AgentRuntimeOutcome::WaitingForApproval { run, step, request });
        };
        let approver = match self
            .approval_store
            .load_trusted_approver(&decision.body.decided_by)
            .map_err(AgentRuntimeError::Store)?
        {
            Some(approver) => approver,
            None => {
                state.terminal_error = Some("live approver is not trusted".to_string());
                self.save_live_state(run.id, state)?;
                return self.fail_live_run(run, state.clone(), AgentRunEventKind::Failed);
            }
        };
        if approver.id != setup.policy.binding_inputs.approver_principal_id {
            state.terminal_error =
                Some("approval signer does not match sealed approver".to_string());
            self.save_live_state(run.id, state)?;
            return self.fail_live_run(run, state.clone(), AgentRunEventKind::Failed);
        }
        let grant = ApprovalGrant {
            request: request.clone(),
            decision: decision.clone(),
            approver: approver.clone(),
        };
        if grant
            .verify_decision(&principal_from_keypair(&self.identity), &approver)
            .is_err()
        {
            state.terminal_error = Some("live approval evidence is invalid".to_string());
            self.save_live_state(run.id, state)?;
            return self.fail_live_run(run, state.clone(), AgentRunEventKind::Failed);
        }
        if decision.body.outcome == ApprovalOutcome::Denied {
            state.terminal_error = Some("live approval was denied".to_string());
            self.save_live_state(run.id, state)?;
            return self.fail_live_run(run, state.clone(), AgentRunEventKind::Failed);
        }
        let existing_resume_events = self
            .agent_store
            .list_agent_run_events(&run.id)
            .map_err(AgentRuntimeError::Store)?;
        if existing_resume_events
            .iter()
            .any(|event| event.kind == AgentRunEventKind::ApprovalResumed)
        {
            let checkpoints = self
                .run_store
                .list_agent_checkpoints(&run.id)
                .map_err(AgentRuntimeError::Store)?;
            if exact_approval_resumed_chronology(
                run.id,
                step.id,
                request_id,
                decision.body.decided_at,
                Utc::now(),
                &existing_resume_events,
                &checkpoints,
            )
            .is_err()
            {
                state.terminal_error =
                    Some("existing approval resume chronology is invalid".to_string());
                self.save_live_state(run.id, state)?;
                return self.fail_live_run(run, state.clone(), AgentRunEventKind::Failed);
            }
        }
        if step.status == AgentRunStepStatus::WaitingForApproval {
            step.resume_from_approval(Utc::now())?;
            self.save_step(&step)?;
        } else if !matches!(
            step.status,
            AgentRunStepStatus::Running | AgentRunStepStatus::Succeeded
        ) {
            state.terminal_error = Some("live approval step is not resumable".to_string());
            self.save_live_state(run.id, state)?;
            return self.fail_live_run(run, state.clone(), AgentRunEventKind::Failed);
        }
        if run.status == AgentRunStatus::WaitingForInput {
            run.resume(Utc::now())?;
            self.save_run(&run)?;
        } else if run.status != AgentRunStatus::Running {
            state.terminal_error = Some("live approval run is not resumable".to_string());
            self.save_live_state(run.id, state)?;
            return self.fail_live_run(run, state.clone(), AgentRunEventKind::Failed);
        }
        let resumed_events = self
            .agent_store
            .list_agent_run_events(&run.id)
            .map_err(AgentRuntimeError::Store)?;
        if resumed_events
            .iter()
            .all(|event| event.kind != AgentRunEventKind::ApprovalResumed)
        {
            self.append_event(
                run.id,
                AgentRunEventKind::ApprovalResumed,
                serde_json::to_value(LiveApprovalResumedEvent::expected(
                    step.id,
                    request_id,
                    approval_resume_epoch,
                ))
                .map_err(|error| AgentRuntimeError::LiveSetup(error.to_string()))?,
            )?;
        }
        let resumed_events = self
            .agent_store
            .list_agent_run_events(&run.id)
            .map_err(AgentRuntimeError::Store)?;
        let checkpoints = self
            .run_store
            .list_agent_checkpoints(&run.id)
            .map_err(AgentRuntimeError::Store)?;
        if exact_approval_resumed_chronology(
            run.id,
            step.id,
            request_id,
            decision.body.decided_at,
            Utc::now(),
            &resumed_events,
            &checkpoints,
        )
        .is_err()
        {
            state.terminal_error =
                Some("duplicate or substituted live approval resume evidence".to_string());
            self.save_live_state(run.id, state)?;
            return self.fail_live_run(run, state.clone(), AgentRunEventKind::Failed);
        }
        let existing_execution = self
            .approval_store
            .load_approval_execution(&request_id)
            .map_err(AgentRuntimeError::Store)?;
        let (outcome, execution) = if let Some(execution) = existing_execution {
            let outcome = ExecutionOutcome {
                output: execution.output.clone(),
                proof: execution.proof.clone(),
            };
            if proof_content::verify_preview_approval_execution(
                &pending_arguments,
                &execution,
                &outcome,
                &grant,
                &self.identity,
                &approver,
            )
            .is_err()
            {
                state.terminal_error =
                    Some("persisted approval execution evidence is invalid".to_string());
                self.save_live_state(run.id, state)?;
                return self.fail_live_run(run, state.clone(), AgentRunEventKind::Failed);
            }
            (outcome, execution)
        } else {
            let context = ExecutionContext {
                actor: self.identity.principal_id,
                principal_kind: Some(PrincipalKind::Agent),
                delegation_id: Some(setup.authority.delegation.id),
                delegation_chain: Some(setup.authority.delegation_chain.clone()),
                workspace_path: self.workspace_path.clone(),
                timestamp: Utc::now(),
            };
            let outcome = match self.engine.execute_with_approval_evidenced(
                "release.publish",
                "v2",
                &pending_arguments,
                &context,
                &grant,
                &approver,
            ) {
                Ok(outcome) => outcome,
                Err(error) => {
                    state.terminal_error = Some(format!("governed publication failed: {error}"));
                    self.save_live_state(run.id, state)?;
                    return self.fail_live_run(run, state.clone(), AgentRunEventKind::Failed);
                }
            };
            let execution = ApprovalExecution {
                request_id,
                executed_at: outcome.proof.body.timestamp,
                output: outcome.output.clone(),
                proof: outcome.proof.clone(),
            };
            self.approval_store
                .save_approval_execution(&execution)
                .map_err(AgentRuntimeError::Store)?;
            (outcome, execution)
        };
        if step.status == AgentRunStepStatus::Running {
            step.succeed(outcome.output.clone(), outcome.proof.clone(), Utc::now())?;
            self.save_step(&step)?;
        } else if step.output.as_ref() != Some(&outcome.output)
            || step.proof.as_ref() != Some(&outcome.proof)
        {
            state.terminal_error = Some("succeeded live step evidence drifted".to_string());
            self.save_live_state(run.id, state)?;
            return self.fail_live_run(run, state.clone(), AgentRunEventKind::Failed);
        }
        state.pending_tool = None;
        if state.counters.successful_publication_mutations == 0 {
            state.counters.successful_publication_mutations = 1;
        } else if state.counters.successful_publication_mutations != 1 {
            return Err(AgentRuntimeError::InvalidCheckpoint(run.id));
        }
        state.previous_response_id = state
            .attempts
            .last()
            .and_then(|attempt| attempt.response.as_ref())
            .map(|response| response.response_id.clone());
        let mut prior_request = self.request_from_live_state(state)?;
        prior_request.previous_response_id = state.previous_response_id.clone();
        if Utc::now() >= run_deadline(state.started_at, 300) {
            state.terminal_error =
                Some("live wall-clock deadline exceeded before continuation".to_string());
            self.save_live_state(run.id, state)?;
            return self.fail_live_run(run, state.clone(), AgentRunEventKind::BudgetExceeded);
        }
        let continuation = continuation_live_request(
            &prior_request,
            &pending.call_id,
            &outcome.output,
            outcome.proof.body.id,
        )?;
        state.next_input = continuation.input.clone();
        self.save_live_state(run.id, state)?;
        let succeeded_data =
            json!({"step_id": step.id, "proof_id": execution.proof.body.id, "live": true});
        let succeeded_events = self
            .agent_store
            .list_agent_run_events(&run.id)
            .map_err(AgentRuntimeError::Store)?
            .into_iter()
            .filter(|event| event.kind == AgentRunEventKind::ToolSucceeded)
            .collect::<Vec<_>>();
        if succeeded_events.is_empty() {
            self.append_event(run.id, AgentRunEventKind::ToolSucceeded, succeeded_data)?;
        } else if succeeded_events.len() != 1 || succeeded_events[0].data != succeeded_data {
            state.terminal_error =
                Some("duplicate or substituted tool success evidence".to_string());
            self.save_live_state(run.id, state)?;
            return self.fail_live_run(run, state.clone(), AgentRunEventKind::Failed);
        }
        self.prepare_and_dispatch_live(run, agent, state)
    }

    fn fail_live_run(
        &self,
        mut run: AgentRun,
        state: LiveRuntimeState,
        kind: AgentRunEventKind,
    ) -> Result<AgentRuntimeOutcome, AgentRuntimeError> {
        if !run.status.is_terminal() {
            run.fail(Utc::now())?;
            self.save_run(&run)?;
        }
        // Seal chronology before deriving the trace digest/evaluation.  A
        // terminal event cannot contain a future evaluation ID without making
        // the trace circular.
        self.ensure_live_failure_terminal_event(
            run.id,
            kind,
            json!({"error": state.terminal_error, "live": true}),
        )?;
        let candidate_evaluation = self.live_evaluation(
            &run,
            &state,
            None,
            self.complete_live_trace_digest(&run, &state, None)?,
            false,
        )?;
        let evaluation = self
            .ensure_exact_live_evaluation(candidate_evaluation, AgentEvaluationOutcome::Failed)?;
        Ok(AgentRuntimeOutcome::Failed {
            run,
            error: state
                .terminal_error
                .unwrap_or_else(|| "live run failed".to_string()),
            evaluation,
        })
    }

    fn ensure_live_failure_terminal_event(
        &self,
        run_id: Uuid,
        requested_kind: AgentRunEventKind,
        data: Value,
    ) -> Result<AgentRunEventKind, AgentRuntimeError> {
        if !matches!(
            requested_kind,
            AgentRunEventKind::Failed | AgentRunEventKind::BudgetExceeded
        ) {
            return Err(AgentRuntimeError::InconsistentState(
                "live failure terminal kind is invalid".to_string(),
            ));
        }
        let mut events = self
            .agent_store
            .list_agent_run_events(&run_id)
            .map_err(AgentRuntimeError::Store)?;
        let mut terminal = events
            .iter()
            .filter(|event| {
                matches!(
                    event.kind,
                    AgentRunEventKind::Failed | AgentRunEventKind::BudgetExceeded
                )
            })
            .collect::<Vec<_>>();
        if terminal.is_empty() {
            self.append_event(run_id, requested_kind, data.clone())?;
            events = self
                .agent_store
                .list_agent_run_events(&run_id)
                .map_err(AgentRuntimeError::Store)?;
            terminal = events
                .iter()
                .filter(|event| {
                    matches!(
                        event.kind,
                        AgentRunEventKind::Failed | AgentRunEventKind::BudgetExceeded
                    )
                })
                .collect::<Vec<_>>();
        }
        if terminal.len() != 1 {
            return Err(AgentRuntimeError::InconsistentState(
                "live failure terminal event group is not exact-one".to_string(),
            ));
        }
        let preserved_kind = terminal[0].kind;
        validate_exact_event_record(run_id, preserved_kind, &data, terminal[0]).map_err(|_| {
            AgentRuntimeError::InconsistentState(
                "live failure terminal event data or digest is inconsistent".to_string(),
            )
        })?;
        Ok(preserved_kind)
    }

    fn sealed_live_terminal_trace(
        &self,
        run: &AgentRun,
        state: &LiveRuntimeState,
    ) -> Result<bool, AgentRuntimeError> {
        let events = self
            .agent_store
            .list_agent_run_events(&run.id)
            .map_err(AgentRuntimeError::Store)?;
        let completed = events
            .iter()
            .filter(|event| event.kind == AgentRunEventKind::Completed)
            .collect::<Vec<_>>();
        let failed = events
            .iter()
            .filter(|event| {
                matches!(
                    event.kind,
                    AgentRunEventKind::Failed | AgentRunEventKind::BudgetExceeded
                )
            })
            .collect::<Vec<_>>();
        match run.status {
            AgentRunStatus::Succeeded if !completed.is_empty() => {
                let output = state.final_output.as_ref().ok_or_else(|| {
                    AgentRuntimeError::InconsistentState(
                        "sealed live success is missing its terminal output".to_string(),
                    )
                })?;
                if completed.len() != 1 || !failed.is_empty() {
                    return Err(AgentRuntimeError::InconsistentState(
                        "sealed live success terminal event group is not exact-one".to_string(),
                    ));
                }
                validate_exact_event_record(
                    run.id,
                    AgentRunEventKind::Completed,
                    &json!({"output": output, "live": true}),
                    completed[0],
                )
                .map_err(|_| {
                    AgentRuntimeError::InconsistentState(
                        "sealed live success terminal event is inconsistent".to_string(),
                    )
                })?;
                Ok(true)
            }
            AgentRunStatus::Failed if !failed.is_empty() => {
                let error = state.terminal_error.as_ref().ok_or_else(|| {
                    AgentRuntimeError::InconsistentState(
                        "sealed live failure is missing its terminal error".to_string(),
                    )
                })?;
                if failed.len() != 1 || !completed.is_empty() {
                    return Err(AgentRuntimeError::InconsistentState(
                        "sealed live failure terminal event group is not exact-one".to_string(),
                    ));
                }
                validate_exact_event_record(
                    run.id,
                    failed[0].kind,
                    &json!({"error": error, "live": true}),
                    failed[0],
                )
                .map_err(|_| {
                    AgentRuntimeError::InconsistentState(
                        "sealed live failure terminal event is inconsistent".to_string(),
                    )
                })?;
                Ok(true)
            }
            AgentRunStatus::Cancelled => Err(AgentRuntimeError::InconsistentState(
                "cancelled live runs have no replayable terminal outcome".to_string(),
            )),
            _ if completed.is_empty() && failed.is_empty() => Ok(false),
            _ => Err(AgentRuntimeError::InconsistentState(
                "live terminal event does not match the stored run status".to_string(),
            )),
        }
    }

    fn live_terminal_outcome(
        &self,
        mut run: AgentRun,
        mut state: LiveRuntimeState,
        sealed_trace: bool,
    ) -> Result<AgentRuntimeOutcome, AgentRuntimeError> {
        if state.terminal_error.is_some() {
            return self.fail_live_run(run, state, AgentRunEventKind::Failed);
        }
        if run.status == AgentRunStatus::Running
            && Utc::now() >= run_deadline(state.started_at, 300)
        {
            state.terminal_error =
                Some("live wall-clock deadline exceeded before terminal seal".to_string());
            self.save_live_state(run.id, &state)?;
            return self.fail_live_run(run, state, AgentRunEventKind::BudgetExceeded);
        }
        let output = state.final_output.clone().ok_or_else(|| {
            AgentRuntimeError::InconsistentState("live terminal result missing output".to_string())
        })?;
        let Some(terminal_verification) = self.valid_live_terminal(&run, &state, &output)? else {
            if sealed_trace {
                return Err(AgentRuntimeError::InconsistentState(
                    "sealed live terminal evidence is incomplete".to_string(),
                ));
            }
            state.terminal_error =
                Some("terminal report or governed publication evidence is incomplete".to_string());
            self.save_live_state(run.id, &state)?;
            if !run.status.is_terminal() {
                run.fail(Utc::now())?;
                self.save_run(&run)?;
            }
            return self.fail_live_run(run, state, AgentRunEventKind::Failed);
        };
        if run.status == AgentRunStatus::Running
            && Utc::now() >= run_deadline(state.started_at, 300)
        {
            state.terminal_error =
                Some("live wall-clock deadline exceeded after terminal verification".to_string());
            self.save_live_state(run.id, &state)?;
            return self.fail_live_run(run, state, AgentRunEventKind::BudgetExceeded);
        }
        // Evaluate an in-memory terminal candidate before persisting the
        // irreversible Succeeded transition. The final evaluation is rebuilt
        // after Completed is appended so its trace digest binds that terminal
        // event, but all mutable evidence gates have already passed here.
        let completion_time = Utc::now();
        let mut candidate_run = run.clone();
        if candidate_run.status == AgentRunStatus::Running {
            candidate_run.succeed(completion_time)?;
        }
        let terminal_candidate = !sealed_trace;
        let candidate_evaluation = self.live_evaluation(
            &candidate_run,
            &state,
            Some(&terminal_verification),
            self.complete_live_trace_digest(&candidate_run, &state, Some(&terminal_verification))?,
            terminal_candidate,
        )?;
        if candidate_evaluation.outcome != AgentEvaluationOutcome::Passed
            || candidate_evaluation.score_bps != Some(10_000)
        {
            if sealed_trace {
                return Err(AgentRuntimeError::InconsistentState(
                    "sealed live terminal evidence does not pass all 17 checks".to_string(),
                ));
            }
            state.terminal_error =
                Some("live terminal candidate did not pass all 17 evidence checks".to_string());
            self.save_live_state(run.id, &state)?;
            return self.fail_live_run(run, state, AgentRunEventKind::Failed);
        }
        if run.status == AgentRunStatus::Running {
            run.succeed(completion_time)?;
            self.save_run(&run)?;
        }
        self.ensure_terminal_event(
            run.id,
            AgentRunEventKind::Completed,
            json!({"output": output, "live": true}),
        )?;
        let evaluation = self.live_evaluation(
            &run,
            &state,
            Some(&terminal_verification),
            self.complete_live_trace_digest(&run, &state, Some(&terminal_verification))?,
            false,
        )?;
        if evaluation.outcome != AgentEvaluationOutcome::Passed
            || evaluation.score_bps != Some(10_000)
        {
            let failed = evaluation.metrics["checks"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|check| check["passed"] == false)
                .filter_map(|check| check["id"].as_str())
                .collect::<Vec<_>>()
                .join(",");
            return Err(AgentRuntimeError::InconsistentState(format!(
                "live terminal success requires a 17/17 10000-bps evaluation; failed: {failed}"
            )));
        }
        let evaluation =
            self.ensure_exact_live_evaluation(evaluation, AgentEvaluationOutcome::Passed)?;
        Ok(AgentRuntimeOutcome::Completed {
            run,
            output,
            evaluation,
        })
    }

    fn ensure_exact_live_evaluation(
        &self,
        candidate: AgentRunEvaluation,
        expected_outcome: AgentEvaluationOutcome,
    ) -> Result<AgentRunEvaluation, AgentRuntimeError> {
        let existing = self
            .run_store
            .list_agent_run_evaluations(&candidate.run_id)
            .map_err(AgentRuntimeError::Store)?
            .into_iter()
            .filter(|evaluation| evaluation.evaluator == LIVE_EVALUATOR)
            .collect::<Vec<_>>();
        if existing.len() > 1 {
            return Err(AgentRuntimeError::InconsistentState(
                "live evaluation evidence is not exact-one".to_string(),
            ));
        }
        if let Some(existing) = existing.into_iter().next() {
            let valid_pass = expected_outcome != AgentEvaluationOutcome::Passed
                || (existing.score_bps == Some(10_000)
                    && existing.metrics["passed_checks"] == 17
                    && existing.metrics["total_checks"] == 17
                    && existing.metrics["checks"].as_array().is_some_and(|checks| {
                        checks.len() == 17 && checks.iter().all(|check| check["passed"] == true)
                    }));
            if existing.run_id != candidate.run_id
                || existing.evaluator != candidate.evaluator
                || existing.outcome != expected_outcome
                || existing.score_bps != candidate.score_bps
                || existing.metrics != candidate.metrics
                || existing.summary != candidate.summary
                || !valid_pass
            {
                return Err(AgentRuntimeError::InconsistentState(
                    "persisted live evaluation evidence is inconsistent".to_string(),
                ));
            }
            return Ok(existing);
        }
        if candidate.outcome != expected_outcome {
            return Err(AgentRuntimeError::InconsistentState(
                "candidate live evaluation outcome is inconsistent".to_string(),
            ));
        }
        self.run_store
            .save_agent_run_evaluation(&candidate)
            .map_err(AgentRuntimeError::Store)?;
        Ok(candidate)
    }

    fn valid_live_terminal(
        &self,
        run: &AgentRun,
        state: &LiveRuntimeState,
        output: &str,
    ) -> Result<Option<LiveTerminalVerification>, AgentRuntimeError> {
        let steps = self
            .run_store
            .list_agent_run_steps(&run.id)
            .map_err(AgentRuntimeError::Store)?;
        if !matches!(
            run.status,
            AgentRunStatus::Running | AgentRunStatus::Succeeded
        ) || state.counters.successful_publication_mutations != 1
            || steps.len() != 1
            || steps[0].status != AgentRunStepStatus::Succeeded
            || steps[0].operation != "release.publish"
            || steps[0].version != "v2"
            || steps[0].proof.is_none()
        {
            return Ok(None);
        }
        let outcome = ExecutionOutcome {
            output: steps[0].output.clone().ok_or_else(|| {
                AgentRuntimeError::InconsistentState(
                    "succeeded live step has no output".to_string(),
                )
            })?,
            proof: steps[0].proof.clone().ok_or_else(|| {
                AgentRuntimeError::InconsistentState("succeeded live step has no proof".to_string())
            })?,
        };
        let exact_input = expected_live_arguments(&state.policy_evidence.resolved_bindings)?;
        let exact_input_digest = digest(
            ArtifactKind::OperationInput,
            &canonicalize(&exact_input)
                .map_err(|error| AgentRuntimeError::LiveSetup(error.to_string()))?,
        );
        if steps[0].input_digest != exact_input_digest {
            return Ok(None);
        }
        // Content owns the only safe no-follow artifact reader and proof/output
        // binding verifier.  Do not reproduce that path handling in runtime.
        if proof_content::verify_preview_publication(
            &self.workspace_path,
            &exact_input,
            &outcome,
            &principal_from_keypair(&self.identity),
        )
        .is_err()
        {
            return Ok(None);
        }
        let request_id = steps[0].approval_request_id.ok_or_else(|| {
            AgentRuntimeError::InconsistentState(
                "succeeded live step lacks approval request".to_string(),
            )
        })?;
        let request = self
            .approval_store
            .load_approval_request(&request_id)
            .map_err(AgentRuntimeError::Store)?
            .ok_or_else(|| {
                AgentRuntimeError::InconsistentState("live approval request missing".to_string())
            })?;
        let decision = self
            .approval_store
            .load_approval_decision(&request_id)
            .map_err(AgentRuntimeError::Store)?
            .ok_or_else(|| {
                AgentRuntimeError::InconsistentState("live approval decision missing".to_string())
            })?;
        let approver = self
            .approval_store
            .load_trusted_approver(&decision.body.decided_by)
            .map_err(AgentRuntimeError::Store)?
            .ok_or_else(|| {
                AgentRuntimeError::InconsistentState("live trusted approver missing".to_string())
            })?;
        let execution = self
            .approval_store
            .load_approval_execution(&request_id)
            .map_err(AgentRuntimeError::Store)?
            .ok_or_else(|| {
                AgentRuntimeError::InconsistentState("live approval execution missing".to_string())
            })?;
        let grant = ApprovalGrant {
            request,
            decision,
            approver: approver.clone(),
        };
        if proof_content::verify_preview_approval_execution(
            &exact_input,
            &execution,
            &outcome,
            &grant,
            &self.identity,
            &approver,
        )
        .is_err()
        {
            return Ok(None);
        }
        // The engine can return a completed exact-replay outcome only from
        // its durable replay ledger.  Verify the same safe artifact inventory
        // before and after that call and require byte-identical output/proof;
        // this establishes the single-effect condition without reimplementing
        // Content's no-follow directory traversal in runtime.
        let loaded_delegation = Delegation::from(state.policy_evidence.loaded_delegation.clone());
        let chain_wire = state.policy_evidence.delegation_chain.clone();
        let replay_context = ExecutionContext {
            actor: self.identity.principal_id,
            principal_kind: Some(PrincipalKind::Agent),
            delegation_id: Some(loaded_delegation.id),
            delegation_chain: Some(DelegationChain {
                root: chain_wire.root,
                grants: chain_wire
                    .grants
                    .into_iter()
                    .map(Delegation::from)
                    .collect(),
            }),
            workspace_path: self.workspace_path.clone(),
            timestamp: execution.executed_at,
        };
        let counters_before = state.counters.successful_publication_mutations;
        let steps_before = steps.clone();
        let replay = match self.engine.execute_with_approval_evidenced(
            "release.publish",
            "v2",
            &exact_input,
            &replay_context,
            &grant,
            &approver,
        ) {
            Ok(replay) => replay,
            Err(_) => return Ok(None),
        };
        if replay.output != outcome.output
            || replay.proof != outcome.proof
            || state.counters.successful_publication_mutations != counters_before
            || self
                .run_store
                .list_agent_run_steps(&run.id)
                .map_err(AgentRuntimeError::Store)?
                != steps_before
            || proof_content::verify_preview_publication(
                &self.workspace_path,
                &exact_input,
                &replay,
                &principal_from_keypair(&self.identity),
            )
            .is_err()
        {
            return Ok(None);
        }
        let parsed_output: ReleasePublishOutput = serde_json::from_value(outcome.output.clone())
            .map_err(|_| AgentRuntimeError::InvalidCheckpoint(run.id))?;
        let expected_report = format!(
            "publication_id={} edition_id={} environment={} version_label={} manifest_digest={} relative_path={} artifact_digest={} proof_id={}",
            parsed_output.data.publication_id,
            parsed_output.data.edition_id,
            parsed_output.data.environment,
            parsed_output.data.version_label,
            parsed_output.data.manifest_digest,
            parsed_output.data.artifact.relative_path,
            parsed_output.data.artifact.digest,
            outcome.proof.body.id,
        );
        if output != expected_report {
            return Ok(None);
        }
        let artifact_binding = json!({
            "input": exact_input,
            "output": outcome.output,
            "proof": outcome.proof,
        });
        Ok(Some(LiveTerminalVerification {
            artifact_identity_digest: wrapped_digest(
                "proof-release-manager-artifact-identity-verification/v1",
                "verification",
                &artifact_binding,
            )?,
            artifact_file_integrity_digest: wrapped_digest(
                "proof-release-manager-artifact-file-verification/v1",
                "verification",
                &artifact_binding,
            )?,
            approval_integrity_digest: wrapped_digest(
                "proof-release-manager-approval-verification/v1",
                "verification",
                &json!({
                    "grant": {
                        "request": grant.request,
                        "decision": grant.decision,
                        "approver": durable_principal_binding(&grant.approver),
                    },
                    "execution": execution,
                }),
            )?,
            proof_integrity_digest: wrapped_digest(
                "proof-release-manager-proof-verification/v1",
                "verification",
                &json!({"proof": outcome.proof, "output": outcome.output}),
            )?,
            replay_verification_digest: wrapped_digest(
                "proof-release-manager-replay-verification/v1",
                "verification",
                &json!({
                    "original": {"output": outcome.output, "proof": outcome.proof},
                    "replay": {"output": replay.output, "proof": replay.proof},
                }),
            )?,
            exact_report_digest: value_digest(&Value::String(expected_report))?,
            step_id: steps[0].id,
            approval_request_id: request_id,
            approver_id: approver.id,
            proof_id: outcome.proof.body.id,
            publication_id: parsed_output.data.publication_id,
            artifact_relative_path: parsed_output.data.artifact.relative_path,
            artifact_digest: parsed_output.data.artifact.digest,
            approval_decided_at: grant.decision.body.decided_at,
            executed_at: execution.executed_at,
            mutation_count_before_replay: counters_before,
            mutation_count_after_replay: state.counters.successful_publication_mutations,
        }))
    }

    fn complete_live_trace_digest(
        &self,
        run: &AgentRun,
        state: &LiveRuntimeState,
        terminal: Option<&LiveTerminalVerification>,
    ) -> Result<ContentDigest, AgentRuntimeError> {
        let steps = self
            .run_store
            .list_agent_run_steps(&run.id)
            .map_err(AgentRuntimeError::Store)?;
        let approval_records = steps
            .iter()
            .filter_map(|step| step.approval_request_id)
            .map(|request_id| {
                Ok(json!({
                    "request": self.approval_store.load_approval_request(&request_id).map_err(AgentRuntimeError::Store)?,
                    "decision": self.approval_store.load_approval_decision(&request_id).map_err(AgentRuntimeError::Store)?,
                    "execution": self.approval_store.load_approval_execution(&request_id).map_err(AgentRuntimeError::Store)?,
                }))
            })
            .collect::<Result<Vec<_>, AgentRuntimeError>>()?;
        value_digest(&json!({
            "schema": "proof-release-manager-live-trace/v1",
            "run": run,
            "state": state,
            "steps": steps,
            "checkpoints": self.run_store.list_agent_checkpoints(&run.id).map_err(AgentRuntimeError::Store)?,
            "events": self.agent_store.list_agent_run_events(&run.id).map_err(AgentRuntimeError::Store)?,
            "approval_records": approval_records,
            "terminal_verification": terminal,
        }))
    }
}

fn value_digest(value: &Value) -> Result<ContentDigest, AgentRuntimeError> {
    Ok(digest(
        ArtifactKind::Generic,
        &canonicalize(value).map_err(|error| AgentRuntimeError::LiveSetup(error.to_string()))?,
    ))
}
fn wrapped_digest(
    schema: &str,
    field: &str,
    value: &Value,
) -> Result<ContentDigest, AgentRuntimeError> {
    value_digest(&json!({"schema": schema, field: value}))
}
fn delegation_digest(delegation: &Delegation) -> Result<ContentDigest, AgentRuntimeError> {
    Ok(digest(
        ArtifactKind::Delegation,
        &proof_kernel::canonicalize_serialized(delegation)
            .map_err(|error| AgentRuntimeError::LiveSetup(error.to_string()))?,
    ))
}
fn resolved_live_bindings(
    run_id: Uuid,
    agent_id: Uuid,
    process_epoch_id: Uuid,
    inputs: &LiveBindingInputs,
) -> LiveResolvedBindings {
    LiveResolvedBindings {
        preflight_evidence_digest: inputs.preflight_evidence_digest,
        run_id,
        agent_id,
        agent_principal_id: inputs.agent_principal_id,
        approver_principal_id: inputs.approver_principal_id,
        delegation_id: inputs.delegation_id,
        delegation_digest: inputs.delegation_digest,
        edition_id: inputs.edition_id,
        manifest_digest: inputs.manifest_digest.clone(),
        idempotency_key: inputs.idempotency_key,
        version_label: inputs.version_label.clone(),
        process_epoch_id,
    }
}
fn resolve_live_policy(
    template: &Value,
    bindings: &LiveResolvedBindings,
) -> Result<Value, AgentRuntimeError> {
    let bindings = serde_json::to_value(bindings)
        .map_err(|error| AgentRuntimeError::LiveSetup(error.to_string()))?;
    fn resolve(value: &Value, bindings: &Value) -> Result<Value, AgentRuntimeError> {
        match value {
            Value::Object(object) if object.len() == 1 && object.contains_key("$binding") => {
                let key = object["$binding"].as_str().ok_or_else(|| {
                    AgentRuntimeError::LiveSetup("binding atom is not a string".to_string())
                })?;
                bindings.get(key).cloned().ok_or_else(|| {
                    AgentRuntimeError::LiveSetup(format!("unresolved binding {key}"))
                })
            }
            Value::Object(object) => object
                .iter()
                .map(|(key, value)| Ok((key.clone(), resolve(value, bindings)?)))
                .collect::<Result<serde_json::Map<_, _>, _>>()
                .map(Value::Object),
            Value::Array(items) => items
                .iter()
                .map(|value| resolve(value, bindings))
                .collect::<Result<Vec<_>, _>>()
                .map(Value::Array),
            other => Ok(other.clone()),
        }
    }
    resolve(template, &bindings)
}
fn expected_live_arguments(bindings: &LiveResolvedBindings) -> Result<Value, AgentRuntimeError> {
    ReleasePublishArguments::from_bindings(bindings).as_value()
}
fn continuation_live_request(
    base: &LiveRequest,
    call_id: &str,
    result: &Value,
    proof_id: Uuid,
) -> Result<LiveRequest, AgentRuntimeError> {
    let result: ReleasePublishOutput = serde_json::from_value(result.clone()).map_err(|error| {
        AgentRuntimeError::LiveSetup(format!("release.publish::v2 output is invalid: {error}"))
    })?;
    let output = LiveToolOutput {
        ok: true,
        result,
        proof_id,
    };
    let input = LiveModelInput::ToolOutput {
        call_id: call_id.to_string(),
        output,
    };
    let output_value = serde_json::to_value(match &input {
        LiveModelInput::ToolOutput { output, .. } => output,
        LiveModelInput::Goal { .. } => unreachable!("constructed tool output"),
    })
    .map_err(|error| AgentRuntimeError::LiveSetup(error.to_string()))?;
    let body = json!({"model": LIVE_MODEL, "instructions": base.instructions, "input": [{"type": "function_call_output", "call_id": call_id, "output": proof_kernel::canonicalize(&output_value).map_err(|error| AgentRuntimeError::LiveSetup(error.to_string()))?.to_string()}], "previous_response_id": base.previous_response_id, "tools": base.tool_declarations, "tool_choice":"auto", "parallel_tool_calls":false, "store":true, "stream":false, "background":false, "service_tier":LIVE_SERVICE_TIER, "max_output_tokens":1024});
    Ok(LiveRequest {
        endpoint: LIVE_ENDPOINT.to_string(),
        requested_model: LIVE_MODEL.to_string(),
        instructions: base.instructions.clone(),
        input: input.clone(),
        previous_response_id: base.previous_response_id.clone(),
        function_names: vec![LIVE_TOOL_NAME.to_string()],
        tool_declarations: base.tool_declarations.clone(),
        tool_choice: "auto".to_string(),
        service_tier: LIVE_SERVICE_TIER.to_string(),
        store: true,
        stream: false,
        background: false,
        parallel_tool_calls: false,
        max_output_tokens: 1024,
        request_body_digest: wrapped_digest(
            "proof-openai-responses-request-digest/v1",
            "request",
            &body,
        )?,
        instructions_digest: base.instructions_digest,
        input_digest: value_digest(&body["input"])?,
        parameters_schema_digest: base.parameters_schema_digest,
        tool_declaration_digest: base.tool_declaration_digest,
        tool_set_digest: base.tool_set_digest,
    })
}
fn live_started_event(state: &LiveRuntimeState) -> Value {
    json!({"live": true, "schema": state.schema, "process_epoch_id": state.process_epoch_id, "policy_binding": state.policy_binding, "authority": state.authority})
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveModelRequestedEvent {
    live: bool,
    attempt_id: Uuid,
    state: String,
    request_body_digest: ContentDigest,
    instructions_digest: ContentDigest,
    input_digest: ContentDigest,
    parameters_schema_digest: ContentDigest,
    tool_declaration_digest: ContentDigest,
    tool_set_digest: ContentDigest,
}

impl LiveModelRequestedEvent {
    fn expected(attempt: &ProviderAttempt) -> Self {
        Self {
            live: true,
            attempt_id: attempt.attempt_id,
            state: "dispatching".to_string(),
            request_body_digest: attempt.request.request_body_digest,
            instructions_digest: attempt.request.instructions_digest,
            input_digest: attempt.request.input_digest,
            parameters_schema_digest: attempt.request.parameters_schema_digest,
            tool_declaration_digest: attempt.request.tool_declaration_digest,
            tool_set_digest: attempt.request.tool_set_digest,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveApprovalResumedEvent {
    step_id: Uuid,
    request_id: Uuid,
    process_epoch_id: Uuid,
    live: bool,
}

impl LiveApprovalResumedEvent {
    fn expected(step_id: Uuid, request_id: Uuid, process_epoch_id: Uuid) -> Self {
        Self {
            step_id,
            request_id,
            process_epoch_id,
            live: true,
        }
    }
}

fn live_model_requested_event(attempt: &ProviderAttempt) -> Value {
    serde_json::to_value(LiveModelRequestedEvent::expected(attempt))
        .expect("live model-requested event serializes")
}

fn validate_exact_event_record(
    run_id: Uuid,
    kind: AgentRunEventKind,
    expected_data: &Value,
    event: &AgentRunEvent,
) -> Result<(), AgentRuntimeError> {
    let rebuilt = AgentRunEvent::create(
        run_id,
        event.sequence,
        kind,
        expected_data.clone(),
        event.created_at,
    )
    .map_err(|_| AgentRuntimeError::InvalidCheckpoint(run_id))?;
    if event.run_id != run_id
        || event.kind != kind
        || event.id.get_version_num() != 7
        || event.data != *expected_data
        || event.data_digest != rebuilt.data_digest
    {
        return Err(AgentRuntimeError::InvalidCheckpoint(run_id));
    }
    Ok(())
}

fn exact_model_requested_event(
    run_id: Uuid,
    attempt: &ProviderAttempt,
    events: &[AgentRunEvent],
) -> Result<(), AgentRuntimeError> {
    let candidates = events
        .iter()
        .filter(|event| {
            event.kind == AgentRunEventKind::ModelRequested
                && event.data["attempt_id"] == json!(attempt.attempt_id)
        })
        .collect::<Vec<_>>();
    if candidates.len() != 1 {
        return Err(AgentRuntimeError::InvalidCheckpoint(run_id));
    }
    let parsed: LiveModelRequestedEvent = serde_json::from_value(candidates[0].data.clone())
        .map_err(|_| AgentRuntimeError::InvalidCheckpoint(run_id))?;
    let expected = LiveModelRequestedEvent::expected(attempt);
    let expected_data = serde_json::to_value(&expected)
        .map_err(|_| AgentRuntimeError::InvalidCheckpoint(run_id))?;
    if parsed != expected
        || attempt
            .dispatched_at
            .is_none_or(|dispatched| candidates[0].created_at < dispatched)
    {
        return Err(AgentRuntimeError::InvalidCheckpoint(run_id));
    }
    validate_exact_event_record(
        run_id,
        AgentRunEventKind::ModelRequested,
        &expected_data,
        candidates[0],
    )
}

fn exact_approval_resumed_event(
    run_id: Uuid,
    step_id: Uuid,
    request_id: Uuid,
    events: &[AgentRunEvent],
    checkpoints: &[AgentCheckpoint],
) -> Result<LiveApprovalResumedEvent, AgentRuntimeError> {
    let candidates = events
        .iter()
        .filter(|event| event.kind == AgentRunEventKind::ApprovalResumed)
        .collect::<Vec<_>>();
    if candidates.len() != 1 {
        return Err(AgentRuntimeError::InvalidCheckpoint(run_id));
    }
    let parsed: LiveApprovalResumedEvent = serde_json::from_value(candidates[0].data.clone())
        .map_err(|_| AgentRuntimeError::InvalidCheckpoint(run_id))?;
    let expected = LiveApprovalResumedEvent::expected(step_id, request_id, parsed.process_epoch_id);
    let expected_data = serde_json::to_value(&expected)
        .map_err(|_| AgentRuntimeError::InvalidCheckpoint(run_id))?;
    let latest_epoch = checkpoints
        .iter()
        .filter(|checkpoint| {
            checkpoint.state["kind"] == LIVE_RUNTIME_CHECKPOINT_KIND
                && checkpoint.created_at <= candidates[0].created_at
        })
        .max_by_key(|checkpoint| checkpoint.sequence)
        .and_then(|checkpoint| {
            serde_json::from_value::<LiveRuntimeState>(checkpoint.state["runtime"].clone())
                .ok()
                .map(|state| state.process_epoch_id)
        });
    if parsed != expected
        || parsed.process_epoch_id.get_version_num() != 7
        || latest_epoch != Some(parsed.process_epoch_id)
    {
        return Err(AgentRuntimeError::InvalidCheckpoint(run_id));
    }
    validate_exact_event_record(
        run_id,
        AgentRunEventKind::ApprovalResumed,
        &expected_data,
        candidates[0],
    )?;
    Ok(parsed)
}

fn exact_approval_resumed_chronology(
    run_id: Uuid,
    step_id: Uuid,
    request_id: Uuid,
    decided_at: DateTime<Utc>,
    now: DateTime<Utc>,
    events: &[AgentRunEvent],
    checkpoints: &[AgentCheckpoint],
) -> Result<LiveApprovalResumedEvent, AgentRuntimeError> {
    let resumed = exact_approval_resumed_event(run_id, step_id, request_id, events, checkpoints)?;
    let resumed_event = events
        .iter()
        .find(|event| event.kind == AgentRunEventKind::ApprovalResumed)
        .ok_or(AgentRuntimeError::InvalidCheckpoint(run_id))?;
    let required = events
        .iter()
        .filter(|event| event.kind == AgentRunEventKind::ApprovalRequired)
        .collect::<Vec<_>>();
    if required.len() != 1
        || required[0].data["step_id"] != json!(step_id)
        || required[0].data["request_id"] != json!(request_id)
        || required[0].sequence >= resumed_event.sequence
        || required[0].created_at > decided_at
        || decided_at > resumed_event.created_at
        || resumed_event.created_at > now
        || required[0].data["process_epoch_id"] == json!(resumed.process_epoch_id)
    {
        return Err(AgentRuntimeError::InvalidCheckpoint(run_id));
    }
    Ok(resumed)
}
impl AgentRuntime {
    fn live_evaluation(
        &self,
        run: &AgentRun,
        state: &LiveRuntimeState,
        terminal: Option<&LiveTerminalVerification>,
        trace_digest: ContentDigest,
        terminal_candidate: bool,
    ) -> Result<AgentRunEvaluation, AgentRuntimeError> {
        let preflight = &state.policy_evidence.preflight_evidence;
        let preflight_valid = self.exact_preflight_evidence(preflight)?;

        let embedded_template: Value = serde_json::from_str(LIVE_POLICY_SOURCE)
            .map_err(|error| AgentRuntimeError::LiveSetup(error.to_string()))?;
        let binding_value = serde_json::to_value(&state.policy_evidence.resolved_bindings)
            .map_err(|error| AgentRuntimeError::LiveSetup(error.to_string()))?;
        let recomputed_resolved =
            resolve_live_policy(&embedded_template, &state.policy_evidence.resolved_bindings)?;
        let policy_valid = state.policy_evidence.resolved_policy == recomputed_resolved
            && state.policy_binding.template_policy_digest == value_digest(&embedded_template)?
            && state.policy_binding.bindings_digest
                == wrapped_digest(
                    "proof-release-manager-live-bindings-digest/v1",
                    "bindings",
                    &binding_value,
                )?
            && state.policy_binding.resolved_policy_digest == value_digest(&recomputed_resolved)?
            && state.policy_binding.check_set_digest
                == wrapped_digest(
                    "proof-release-manager-live-check-set-digest/v1",
                    "check_ids",
                    &json!(live_check_ids()),
                )?
            && state.policy_binding.tamper_vector_set_digest
                == wrapped_digest(
                    "proof-release-manager-live-tamper-vector-set-digest/v1",
                    "tamper_vector_ids",
                    &json!(live_tamper_ids()),
                )?
            && state.policy_binding.pricing_schedule_digest
                == value_digest(&embedded_template["pricing"])?
            && trace_digest != ContentDigest::from_bytes([0; 32]);

        let expected_declaration: LiveToolDeclaration =
            serde_json::from_value(embedded_template["tool"]["declaration"].clone())
                .map_err(|_| AgentRuntimeError::InvalidCheckpoint(run.id))?;
        let exact_instructions = embedded_template["outbound_data"]["instructions"]
            .as_str()
            .ok_or(AgentRuntimeError::InvalidCheckpoint(run.id))?;
        let mut previous_cursor: Option<String> = None;
        let mut requests_valid = true;
        let mut response_ids = BTreeSet::new();
        for attempt in &state.attempts {
            let request = &attempt.request;
            let model_request = request.as_model_request()?;
            let body = crate::openai::request_body(&model_request)
                .map_err(|_| AgentRuntimeError::InvalidCheckpoint(run.id))?;
            requests_valid &= request.endpoint == LIVE_ENDPOINT
                && request.requested_model == LIVE_MODEL
                && request.instructions == exact_instructions
                && request.previous_response_id == previous_cursor
                && request.function_names == [LIVE_TOOL_NAME]
                && request.tool_declarations == [expected_declaration.clone()]
                && request.request_body_digest
                    == wrapped_digest(
                        "proof-openai-responses-request-digest/v1",
                        "request",
                        &body,
                    )?
                && request.instructions_digest == value_digest(&body["instructions"])?
                && request.input_digest == value_digest(&body["input"])?
                && request.parameters_schema_digest
                    == state.policy_binding.parameters_schema_digest
                && request.tool_declaration_digest == state.policy_binding.tool_declaration_digest
                && request.tool_set_digest == state.policy_binding.tool_set_digest
                && request.tool_choice == "auto"
                && request.service_tier == LIVE_SERVICE_TIER
                && request.store
                && !request.stream
                && !request.background
                && !request.parallel_tool_calls
                && request.max_output_tokens == 1024;
            if attempt.state == ProviderAttemptState::Committed {
                let response = attempt
                    .response
                    .as_ref()
                    .expect("validated committed response");
                requests_valid &= response.returned_model == LIVE_MODEL
                    && !response.response_id.is_empty()
                    && response_ids.insert(response.response_id.clone());
                previous_cursor = Some(response.response_id.clone());
            }
        }

        let events = self
            .agent_store
            .list_agent_run_events(&run.id)
            .map_err(AgentRuntimeError::Store)?;
        let steps = self
            .run_store
            .list_agent_run_steps(&run.id)
            .map_err(AgentRuntimeError::Store)?;
        let checkpoints = self
            .run_store
            .list_agent_checkpoints(&run.id)
            .map_err(AgentRuntimeError::Store)?;
        let exact_arguments = expected_live_arguments(&state.policy_evidence.resolved_bindings)?;
        let exact_input_digest = digest(
            ArtifactKind::OperationInput,
            &canonicalize(&exact_arguments)
                .map_err(|error| AgentRuntimeError::LiveSetup(error.to_string()))?,
        );
        let first_input = LiveModelInput::Goal {
            text: run.goal.clone(),
        };
        let call_id = events
            .iter()
            .filter(|event| event.kind == AgentRunEventKind::ToolRequested)
            .filter_map(|event| event.data["call_id"].as_str())
            .collect::<Vec<_>>();
        let expected_continuation = if steps.len() == 1 && call_id.len() == 1 {
            match (steps[0].output.clone(), steps[0].proof.as_ref()) {
                (Some(output), Some(proof)) => Some(LiveModelInput::ToolOutput {
                    call_id: call_id[0].to_string(),
                    output: LiveToolOutput {
                        ok: true,
                        result: serde_json::from_value(output)
                            .map_err(|_| AgentRuntimeError::InvalidCheckpoint(run.id))?,
                        proof_id: proof.body.id,
                    },
                }),
                _ => None,
            }
        } else {
            None
        };
        let synthetic_boundary_valid = state.attempts.iter().all(|attempt| {
            let turn_input_valid = match attempt.logical_turn {
                1 => {
                    attempt.request.input == first_input
                        && attempt.request.input_digest == state.policy_binding.initial_input_digest
                }
                2 => expected_continuation
                    .as_ref()
                    .is_some_and(|expected| attempt.request.input == *expected),
                _ => false,
            };
            let retry_valid = attempt.retry_of.is_none_or(|parent_id| {
                state.attempts.iter().any(|parent| {
                    parent.attempt_id == parent_id && parent.request == attempt.request
                })
            });
            turn_input_valid && retry_valid
        });
        let approval_ids = steps
            .iter()
            .filter_map(|step| step.approval_request_id)
            .collect::<Vec<_>>();
        let approval_requests = approval_ids
            .iter()
            .map(|id| self.approval_store.load_approval_request(id))
            .collect::<Result<Vec<_>, _>>()
            .map_err(AgentRuntimeError::Store)?;
        let approval_decisions = approval_ids
            .iter()
            .map(|id| self.approval_store.load_approval_decision(id))
            .collect::<Result<Vec<_>, _>>()
            .map_err(AgentRuntimeError::Store)?;
        let approval_executions = approval_ids
            .iter()
            .map(|id| self.approval_store.load_approval_execution(id))
            .collect::<Result<Vec<_>, _>>()
            .map_err(AgentRuntimeError::Store)?;
        let approval_request_count = approval_requests.iter().flatten().count();
        let approval_decision_count = approval_decisions.iter().flatten().count();
        let approval_execution_count = approval_executions.iter().flatten().count();
        let requested_events = events
            .iter()
            .filter(|event| event.kind == AgentRunEventKind::ModelRequested)
            .collect::<Vec<_>>();
        let responded_events = events
            .iter()
            .filter(|event| event.kind == AgentRunEventKind::ModelResponded)
            .collect::<Vec<_>>();
        let event_attempts_valid = requested_events.len()
            == state.counters.provider_dispatches as usize
            && responded_events.len() == state.counters.logical_model_turns as usize
            && state.attempts.iter().all(|attempt| {
                let requested = attempt.dispatched_at.is_none()
                    || exact_model_requested_event(run.id, attempt, &events).is_ok();
                let responded = if attempt.state == ProviderAttemptState::Committed {
                    exact_committed_event(run.id, attempt, &events).is_ok()
                } else {
                    true
                };
                requested && responded
            });
        let attempts_settled = state.attempts.iter().all(|attempt| {
            matches!(
                attempt.state,
                ProviderAttemptState::Committed
                    | ProviderAttemptState::FailedRetryable
                    | ProviderAttemptState::RejectedRetryable
            ) && attempt.finished_at.is_some()
        });
        let committed_count = state
            .attempts
            .iter()
            .filter(|attempt| attempt.state == ProviderAttemptState::Committed)
            .count();
        let recovery_valid = validate_persisted_live_state(run.id, state).is_ok()
            && attempts_settled
            && committed_count == 2
            && event_attempts_valid
            && state.counters.retries <= 1
            && state.attempts.len() == committed_count + state.counters.retries as usize;

        let strict_delegation = &state.policy_evidence.loaded_delegation;
        let agent = self.load_live_agent(state.agent_id)?;
        let trusted_approver = self
            .approval_store
            .load_trusted_approver(
                &state
                    .policy_evidence
                    .resolved_bindings
                    .approver_principal_id,
            )
            .map_err(AgentRuntimeError::Store)?;
        let authority_valid = state.authority.delegation_id == strict_delegation.id
            && state.authority.delegation_digest
                == delegation_digest(&Delegation::from(strict_delegation.clone()))?
            && state.authority.allowed_operations == ["release.publish"]
            && state.authority.allowed_domains == ["content"]
            && state.authority.valid_until == strict_delegation.valid_until
            && !strict_delegation.revoked
            && strict_delegation.recipient == self.identity.principal_id
            && strict_delegation.valid_until >= run_deadline(state.started_at, 300)
            && strict_delegation.scope.resource_scope.is_none()
            && strict_delegation.scope.allowed_operations.as_deref()
                == Some(&["release.publish".to_string()])
            && strict_delegation.scope.allowed_domains.as_deref() == Some(&["content".to_string()])
            && state
                .policy_evidence
                .delegation_chain
                .grants
                .iter()
                .any(|grant| grant == strict_delegation)
            && agent.provider == LIVE_PROVIDER
            && agent.model == LIVE_MODEL
            && agent.tools.len() == 1
            && agent.tools[0].operation == "release.publish"
            && agent.tools[0].version == "v2"
            && state
                .policy_evidence
                .resolved_bindings
                .approver_principal_id
                != self.identity.principal_id
            && trusted_approver.as_ref().is_some_and(|approver| {
                approver.id
                    == state
                        .policy_evidence
                        .resolved_bindings
                        .approver_principal_id
                    && approver.kind == PrincipalKind::Human
            })
            && terminal.is_some_and(|verification| {
                verification.approver_id
                    == state
                        .policy_evidence
                        .resolved_bindings
                        .approver_principal_id
            });

        let tool_requested = events
            .iter()
            .filter(|event| event.kind == AgentRunEventKind::ToolRequested)
            .collect::<Vec<_>>();
        let exact_tool = terminal.is_some_and(|verification| {
            steps.len() == 1
                && tool_requested.len() == 1
                && steps[0].id == verification.step_id
                && steps[0].operation == "release.publish"
                && steps[0].version == "v2"
                && steps[0].ordinal == 0
                && steps[0].attempt == 1
                && steps[0].retry_of.is_none()
                && state.counters.tool_attempts == 1
                && tool_requested[0].data["step_id"] == json!(verification.step_id)
                && steps[0].input_digest == exact_input_digest
                && tool_requested[0].data["input_digest"] == json!(exact_input_digest)
                && tool_requested[0].data["operation"] == "release.publish"
                && tool_requested[0].data["version"] == "v2"
        });
        let approval_required = events
            .iter()
            .filter(|event| event.kind == AgentRunEventKind::ApprovalRequired)
            .collect::<Vec<_>>();
        let approval_resumed = events
            .iter()
            .filter(|event| event.kind == AgentRunEventKind::ApprovalResumed)
            .collect::<Vec<_>>();
        let resumed_data = approval_resumed.first().and_then(|event| {
            serde_json::from_value::<LiveApprovalResumedEvent>(event.data.clone()).ok()
        });
        let chronology_valid = terminal.is_some_and(|verification| {
            approval_required.len() == 1
                && approval_resumed.len() == 1
                && resumed_data.as_ref().is_some_and(|resumed| {
                    resumed.step_id == verification.step_id
                        && resumed.request_id == verification.approval_request_id
                        && resumed.live
                        && resumed.process_epoch_id.get_version_num() == 7
                        && exact_approval_resumed_event(
                            run.id,
                            verification.step_id,
                            verification.approval_request_id,
                            &events,
                            &checkpoints,
                        )
                        .is_ok()
                })
                && approval_required[0].data["step_id"] == json!(verification.step_id)
                && approval_resumed[0].data["step_id"] == json!(verification.step_id)
                && approval_required[0].data["request_id"]
                    == json!(verification.approval_request_id)
                && approval_resumed[0].data["request_id"] == json!(verification.approval_request_id)
                && approval_required[0].data["process_epoch_id"]
                    != approval_resumed[0].data["process_epoch_id"]
                && approval_required[0].sequence < approval_resumed[0].sequence
                && approval_required[0].created_at <= verification.approval_decided_at
                && verification.approval_decided_at <= approval_resumed[0].created_at
                && approval_resumed[0].created_at <= verification.executed_at
        });
        let approval_valid = terminal.is_some_and(|verification| {
            verification.approval_integrity_digest != ContentDigest::from_bytes([0; 32])
                && verification.approval_request_id == steps[0].approval_request_id.unwrap()
                && approval_request_count == 1
                && approval_decision_count == 1
                && approval_execution_count == 1
        });

        let duration_valid = run
            .completed_at
            .is_some_and(|completed| completed <= run_deadline(state.started_at, 300));
        let budgets_valid = state.counters.provider_dispatches <= 4
            && state.counters.logical_model_turns <= 3
            && state.counters.tool_attempts == 1
            && state.counters.retries <= 1
            && state.cumulative_usage.total_tokens <= 10_000
            && state.attempts.iter().all(|attempt| {
                attempt
                    .response
                    .as_ref()
                    .is_none_or(|response| response.usage.output_tokens <= 1024)
            })
            && duration_valid;
        let mut exact_input_total = 0_u64;
        let mut exact_output_total = 0_u64;
        let mut exact_token_total = 0_u64;
        let mut exact_calculated_total = 0_u64;
        let mut exact_provider_total: Option<u64> = None;
        let mut saw_cost_response = false;
        let mut response_costs_valid = true;
        for response in state
            .attempts
            .iter()
            .filter(|attempt| attempt.state == ProviderAttemptState::Committed)
            .filter_map(|attempt| attempt.response.as_ref())
        {
            let exact_response_cost =
                response
                    .usage
                    .input_tokens
                    .checked_mul(5)
                    .and_then(|input| {
                        response
                            .usage
                            .output_tokens
                            .checked_mul(20)
                            .and_then(|output| input.checked_add(output))
                    });
            let next_input = exact_input_total.checked_add(response.usage.input_tokens);
            let next_output = exact_output_total.checked_add(response.usage.output_tokens);
            let next_total = exact_token_total.checked_add(response.usage.total_tokens);
            let next_calculated =
                exact_response_cost.and_then(|cost| exact_calculated_total.checked_add(cost));
            let next_provider = if !saw_cost_response {
                response.provider_cost_microusd
            } else {
                match (exact_provider_total, response.provider_cost_microusd) {
                    (Some(prior), Some(cost)) => prior.checked_add(cost),
                    _ => None,
                }
            };
            response_costs_valid &= exact_response_cost == Some(response.calculated_cost_microusd)
                && next_input == Some(response.cumulative_input_tokens)
                && next_output == Some(response.cumulative_output_tokens)
                && next_total == Some(response.cumulative_total_tokens)
                && next_calculated == Some(response.cumulative_calculated_cost_microusd)
                && next_provider == response.cumulative_provider_cost_microusd
                && response.provider_cost_status
                    == if response.provider_cost_microusd.is_some() {
                        ProviderCostStatus::Reported
                    } else {
                        ProviderCostStatus::Unavailable
                    }
                && response.cumulative_provider_cost_status
                    == if next_provider.is_some() {
                        ProviderCostStatus::Reported
                    } else {
                        ProviderCostStatus::Unavailable
                    }
                && response.pricing_schedule_id == "proof-openai-gpt-5.6-sol-pricing/2026-08-30"
                && response.pricing_schedule_digest == state.policy_binding.pricing_schedule_digest;
            let (Some(next_input), Some(next_output), Some(next_total), Some(next_calculated)) =
                (next_input, next_output, next_total, next_calculated)
            else {
                response_costs_valid = false;
                break;
            };
            exact_input_total = next_input;
            exact_output_total = next_output;
            exact_token_total = next_total;
            exact_calculated_total = next_calculated;
            exact_provider_total = next_provider;
            saw_cost_response = true;
        }
        let cost_valid = response_costs_valid
            && state.cumulative_usage.input_tokens == exact_input_total
            && state.cumulative_usage.output_tokens == exact_output_total
            && state.cumulative_usage.total_tokens == exact_token_total
            && state.cumulative_cost.calculated_cost_microusd == exact_calculated_total
            && state.cumulative_cost.provider_cost_microusd == exact_provider_total
            && state.cumulative_cost.pricing_schedule_id
                == "proof-openai-gpt-5.6-sol-pricing/2026-08-30"
            && state.cumulative_cost.pricing_schedule_digest
                == state.policy_binding.pricing_schedule_digest
            && state.cumulative_cost.calculated_cost_microusd <= 120_000
            && match state.cumulative_cost.provider_cost_status {
                ProviderCostStatus::Reported => {
                    state.cumulative_cost.provider_cost_microusd.is_some()
                }
                ProviderCostStatus::Unavailable => {
                    state.cumulative_cost.provider_cost_microusd.is_none()
                }
            };
        let no_failure_events = events.iter().all(|event| {
            !matches!(
                event.kind,
                AgentRunEventKind::ToolFailed
                    | AgentRunEventKind::Failed
                    | AgentRunEventKind::BudgetExceeded
            )
        });
        let terminal_report_valid = terminal.is_some_and(|verification| {
            state.final_output.as_ref().is_some_and(|output| {
                value_digest(&Value::String(output.clone())).ok()
                    == Some(verification.exact_report_digest)
            })
        });
        let completed_events = events
            .iter()
            .filter(|event| event.kind == AgentRunEventKind::Completed)
            .collect::<Vec<_>>();
        let completed_valid = state.final_output.as_ref().is_some_and(|output| {
            let expected = json!({"output": output, "live": true});
            (terminal_candidate && completed_events.is_empty())
                || (completed_events.len() == 1
                    && validate_exact_event_record(
                        run.id,
                        AgentRunEventKind::Completed,
                        &expected,
                        completed_events[0],
                    )
                    .is_ok())
        });
        let terminal_success =
            terminal.is_some() && run.status == AgentRunStatus::Succeeded && completed_valid;
        let checks = vec![
            ("deterministic_preflight", preflight_valid),
            ("sealed_policy_and_trace", policy_valid),
            (
                "provider_endpoint_model",
                state.provider.name == LIVE_PROVIDER
                    && state.provider.endpoint == LIVE_ENDPOINT
                    && state.provider.requested_model == LIVE_MODEL
                    && state.provider.service_tier == LIVE_SERVICE_TIER
                    && state.provider.store
                    && !state.provider.stream
                    && !state.provider.background
                    && !state.provider.parallel_tool_calls
                    && requests_valid,
            ),
            ("synthetic_data_boundary", synthetic_boundary_valid),
            ("identity_authority_allowlist", authority_valid),
            ("exact_tool_call", exact_tool),
            ("approval_integrity", approval_valid),
            ("approval_restart_chronology", chronology_valid),
            ("provider_attempt_recovery", recovery_valid),
            ("budgets", budgets_valid),
            ("cost_accounting", cost_valid),
            (
                "artifact_identity_manifest",
                terminal.is_some_and(|verification| {
                    verification.artifact_identity_digest != ContentDigest::from_bytes([0; 32])
                }),
            ),
            (
                "artifact_file_integrity",
                terminal.is_some_and(|verification| {
                    verification.artifact_file_integrity_digest
                        != ContentDigest::from_bytes([0; 32])
                        && !verification.artifact_relative_path.is_empty()
                        && !verification.artifact_digest.is_empty()
                }),
            ),
            (
                "proof_integrity",
                terminal.is_some_and(|verification| {
                    verification.proof_integrity_digest != ContentDigest::from_bytes([0; 32])
                        && verification.proof_id.get_version_num() == 7
                }),
            ),
            (
                "exact_replay_single_effect",
                terminal.is_some_and(|verification| {
                    verification.replay_verification_digest != ContentDigest::from_bytes([0; 32])
                        && verification.mutation_count_before_replay == 1
                        && verification.mutation_count_after_replay == 1
                }),
            ),
            ("terminal_report", terminal_report_valid),
            (
                "no_failure_or_unapproved_external_effect",
                terminal_success
                    && no_failure_events
                    && state.terminal_error.is_none()
                    && state.counters.successful_publication_mutations == 1
                    && steps.len() == 1,
            ),
        ];
        if checks.iter().map(|(id, _)| *id).collect::<Vec<_>>() != live_check_ids() {
            return Err(AgentRuntimeError::InconsistentState(
                "live evaluator check set drifted".to_string(),
            ));
        }
        let passed = checks.iter().filter(|(_, passed)| *passed).count() as u16;
        let outcome = if passed == 17 {
            AgentEvaluationOutcome::Passed
        } else {
            AgentEvaluationOutcome::Failed
        };
        let score = if passed == 17 {
            10_000
        } else {
            (passed as u32 * 10_000 / 17) as u16
        };
        let runtime_state_digest = value_digest(
            &serde_json::to_value(state)
                .map_err(|error| AgentRuntimeError::LiveSetup(error.to_string()))?,
        )?;
        AgentRunEvaluation::create(
            run,
            LIVE_EVALUATOR,
            outcome,
            Some(score),
            json!({
                "policy_schema": "proof-release-manager-live-policy/v1",
                "trace_schema": "proof-release-manager-live-trace/v1",
                "runtime_state_schema": state.schema,
                "runtime_state_digest": runtime_state_digest,
                "checks": checks.iter().map(|(id, passed)| json!({"id": id, "passed": passed})).collect::<Vec<_>>(),
                "passed_checks": passed,
                "total_checks": 17,
                "score_bps": score,
                "template_policy_digest": state.policy_binding.template_policy_digest,
                "bindings_digest": state.policy_binding.bindings_digest,
                "resolved_policy_digest": state.policy_binding.resolved_policy_digest,
                "check_set_digest": state.policy_binding.check_set_digest,
                "tamper_vector_set_digest": state.policy_binding.tamper_vector_set_digest,
                "pricing_schedule_digest": state.policy_binding.pricing_schedule_digest,
                "complete_trace_digest": trace_digest,
                "run_revision": run.revision,
                "step_count": steps.len(),
                "checkpoint_count": checkpoints.len(),
                "event_count": events.len(),
                "provider_attempt_count": state.attempts.len(),
                "approval_request_count": approval_request_count,
                "approval_decision_count": approval_decision_count,
                "approval_execution_count": approval_execution_count,
                "artifact_identity_verification_digest": terminal.map(|value| value.artifact_identity_digest),
                "artifact_file_verification_digest": terminal.map(|value| value.artifact_file_integrity_digest),
                "approval_verification_digest": terminal.map(|value| value.approval_integrity_digest),
                "proof_verification_digest": terminal.map(|value| value.proof_integrity_digest),
                "replay_verification_digest": terminal.map(|value| value.replay_verification_digest),
                "terminal_publication_id": terminal.map(|value| value.publication_id),
                "terminal_artifact_relative_path": terminal.map(|value| value.artifact_relative_path.clone()),
                "terminal_artifact_digest": terminal.map(|value| value.artifact_digest.clone()),
                "terminal_proof_id": terminal.map(|value| value.proof_id),
            }),
            Some("sealed live evaluation".to_string()),
            Utc::now(),
        )
        .map_err(AgentRuntimeError::Run)
    }
}
fn live_check_ids() -> [&'static str; 17] {
    [
        "deterministic_preflight",
        "sealed_policy_and_trace",
        "provider_endpoint_model",
        "synthetic_data_boundary",
        "identity_authority_allowlist",
        "exact_tool_call",
        "approval_integrity",
        "approval_restart_chronology",
        "provider_attempt_recovery",
        "budgets",
        "cost_accounting",
        "artifact_identity_manifest",
        "artifact_file_integrity",
        "proof_integrity",
        "exact_replay_single_effect",
        "terminal_report",
        "no_failure_or_unapproved_external_effect",
    ]
}
fn live_tamper_ids() -> [&'static str; 20] {
    [
        "preflight_record_policy_trace_score_count_or_digest_change",
        "binding_change_unresolved_binding_or_circular_digest_field",
        "check_id_cardinality_order_or_exact_set_digest_change",
        "provider_requested_returned_model_endpoint_or_setting_substitution",
        "function_name_description_parameters_schema_tool_set_or_request_body_substitution",
        "provider_attempt_state_request_retry_response_cost_usage_or_epoch_change",
        "dispatching_checkpoint_event_reread_barrier_missing_or_reordered",
        "ambiguous_attempt_reclassified_as_retryable_or_committed",
        "delegation_id_row_digest_scope_chain_recipient_validity_or_revocation_change",
        "approval_request_argument_version_expiry_or_signature_change",
        "approver_identity_key_outcome_signature_or_chronology_change",
        "missing_restart_or_execution_before_approval",
        "artifact_path_traversal_second_file_bytes_or_digest_change",
        "artifact_edition_environment_version_manifest_actor_or_timestamp_change",
        "operation_output_publication_artifact_or_manifest_change",
        "proof_id_body_signature_actor_delegation_digest_or_timestamp_change",
        "replay_output_proof_substitution_or_second_mutation",
        "usage_call_token_duration_retry_price_schedule_or_cost_change",
        "failure_event_unallowlisted_call_content_mutation_or_unapproved_external_effect",
        "terminal_output_reference_removed_or_substituted",
    ]
}
fn validate_policy_sets(policy: &LivePolicyMaterial) -> Result<(), AgentRuntimeError> {
    let checks = policy
        .template
        .get("checks")
        .and_then(Value::as_array)
        .ok_or_else(|| AgentRuntimeError::LiveSetup("policy checks missing".to_string()))?;
    let check_ids = checks
        .iter()
        .map(|check| check.get("id").and_then(Value::as_str))
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| AgentRuntimeError::LiveSetup("policy check ID invalid".to_string()))?;
    if check_ids != live_check_ids()
        || checks.len() != 17
        || wrapped_digest(
            "proof-release-manager-live-check-set-digest/v1",
            "check_ids",
            &json!(check_ids),
        )? != policy.check_set_digest
    {
        return Err(AgentRuntimeError::LiveSetup(
            "policy does not contain exact ordered 17-check set".to_string(),
        ));
    }
    let tamper = policy
        .template
        .get("tamper_vectors")
        .and_then(Value::as_array)
        .ok_or_else(|| AgentRuntimeError::LiveSetup("policy tamper set missing".to_string()))?;
    let tamper_ids = tamper
        .iter()
        .map(Value::as_str)
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| AgentRuntimeError::LiveSetup("policy tamper ID invalid".to_string()))?;
    if tamper_ids != live_tamper_ids()
        || tamper.len() != 20
        || wrapped_digest(
            "proof-release-manager-live-tamper-vector-set-digest/v1",
            "tamper_vector_ids",
            &json!(tamper_ids),
        )? != policy.tamper_vector_set_digest
    {
        return Err(AgentRuntimeError::LiveSetup(
            "policy does not contain exact ordered 20-vector tamper set".to_string(),
        ));
    }
    let pricing = policy
        .template
        .get("pricing")
        .ok_or_else(|| AgentRuntimeError::LiveSetup("policy pricing missing".to_string()))?;
    if value_digest(pricing)? != policy.pricing_schedule_digest {
        return Err(AgentRuntimeError::LiveSetup(
            "pricing schedule digest mismatch".to_string(),
        ));
    }
    Ok(())
}

enum ResumeApproval {
    Waiting(AgentRuntimeOutcome),
    Continue,
    Failed(String),
    BudgetExceeded(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ProviderAttemptState {
    Prepared,
    Dispatching,
    ResponseReceived,
    Committed,
    RejectedRetryable,
    FailedRetryable,
    FailedTerminal,
    Ambiguous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ProviderFailureClass {
    CertifiedNoBytes,
    Explicit429,
    Terminal,
    Ambiguous,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderFailure {
    class: ProviderFailureClass,
    code: String,
    detail: String,
}

impl ProviderFailure {
    fn certified_no_bytes(code: &str) -> Self {
        Self {
            class: ProviderFailureClass::CertifiedNoBytes,
            code: code.to_string(),
            detail: "redacted".to_string(),
        }
    }
    fn explicit_429(code: &str) -> Self {
        Self {
            class: ProviderFailureClass::Explicit429,
            code: code.to_string(),
            detail: "redacted".to_string(),
        }
    }
    fn terminal(code: &str) -> Self {
        Self {
            class: ProviderFailureClass::Terminal,
            code: code.to_string(),
            detail: "redacted".to_string(),
        }
    }
    fn ambiguous(code: &str) -> Self {
        Self {
            class: ProviderFailureClass::Ambiguous,
            code: code.to_string(),
            detail: "redacted".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReleasePublishArguments {
    idempotency_key: Uuid,
    edition_id: Uuid,
    environment: String,
    version_label: String,
    manifest_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum LiveModelInput {
    Goal {
        text: String,
    },
    ToolOutput {
        call_id: String,
        output: LiveToolOutput,
    },
}

impl LiveModelInput {
    fn as_model_input(&self) -> Result<ModelInput, AgentRuntimeError> {
        match self {
            Self::Goal { text } => Ok(ModelInput::Goal { text: text.clone() }),
            Self::ToolOutput { call_id, output } => Ok(ModelInput::ToolOutput {
                call_id: call_id.clone(),
                output: serde_json::to_value(output)
                    .map_err(|error| AgentRuntimeError::LiveSetup(error.to_string()))?,
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveToolOutput {
    ok: bool,
    result: ReleasePublishOutput,
    proof_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReleasePublishOutput {
    operation: String,
    data: ReleasePublishOutputData,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReleasePublishOutputData {
    publication_id: Uuid,
    edition_id: Uuid,
    environment: String,
    version_label: String,
    manifest_digest: String,
    artifact: ReleasePublishArtifactReference,
    published_at: DateTime<Utc>,
    published_by: PrincipalId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReleasePublishArtifactReference {
    schema: String,
    relative_path: String,
    digest: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LivePendingToolCall {
    call_id: String,
    tool_name: String,
    operation: String,
    version: String,
    arguments: ReleasePublishArguments,
    step_id: Uuid,
    approval_request_id: Uuid,
    request_process_epoch_id: Uuid,
    step_intent: LiveStepIntent,
    approval_request: LiveSignedApprovalRequest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveStepIntent {
    id: Uuid,
    run_id: Uuid,
    ordinal: u32,
    attempt: u32,
    retry_of: Option<Uuid>,
    operation: String,
    version: String,
    input_digest: ContentDigest,
    approval_request_id: Uuid,
    revision: u64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    started_at: DateTime<Utc>,
}

impl LiveStepIntent {
    fn from_waiting(step: &AgentRunStep) -> Result<Self, AgentRuntimeError> {
        if step.status != AgentRunStepStatus::WaitingForApproval
            || step.output.is_some()
            || step.proof.is_some()
            || step.error.is_some()
            || step.completed_at.is_some()
        {
            return Err(AgentRuntimeError::InconsistentState(
                "approval intent step is not exact waiting state".to_string(),
            ));
        }
        Ok(Self {
            id: step.id,
            run_id: step.run_id,
            ordinal: step.ordinal,
            attempt: step.attempt,
            retry_of: step.retry_of,
            operation: step.operation.clone(),
            version: step.version.clone(),
            input_digest: step.input_digest,
            approval_request_id: step.approval_request_id.ok_or_else(|| {
                AgentRuntimeError::InconsistentState(
                    "approval intent step lacks request".to_string(),
                )
            })?,
            revision: step.revision,
            created_at: step.created_at,
            updated_at: step.updated_at,
            started_at: step.started_at.ok_or_else(|| {
                AgentRuntimeError::InconsistentState(
                    "approval intent step never started".to_string(),
                )
            })?,
        })
    }

    fn as_step(&self) -> AgentRunStep {
        AgentRunStep {
            id: self.id,
            run_id: self.run_id,
            ordinal: self.ordinal,
            attempt: self.attempt,
            retry_of: self.retry_of,
            operation: self.operation.clone(),
            version: self.version.clone(),
            input_digest: self.input_digest,
            status: AgentRunStepStatus::WaitingForApproval,
            approval_request_id: Some(self.approval_request_id),
            output: None,
            proof: None,
            error: None,
            revision: self.revision,
            created_at: self.created_at,
            updated_at: self.updated_at,
            started_at: Some(self.started_at),
            completed_at: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveSignedApprovalRequest {
    body: LiveApprovalRequestBody,
    signature: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveApprovalRequestBody {
    id: Uuid,
    operation: String,
    version: String,
    input_digest: ContentDigest,
    requested_by: PrincipalId,
    requested_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

impl From<&SignedApprovalRequest> for LiveSignedApprovalRequest {
    fn from(request: &SignedApprovalRequest) -> Self {
        Self {
            body: LiveApprovalRequestBody {
                id: request.body.id,
                operation: request.body.operation.clone(),
                version: request.body.version.clone(),
                input_digest: request.body.input_digest,
                requested_by: request.body.requested_by,
                requested_at: request.body.requested_at,
                expires_at: request.body.expires_at,
            },
            signature: request.signature.clone(),
        }
    }
}

impl From<LiveSignedApprovalRequest> for SignedApprovalRequest {
    fn from(request: LiveSignedApprovalRequest) -> Self {
        Self {
            body: ApprovalRequest {
                id: request.body.id,
                operation: request.body.operation,
                version: request.body.version,
                input_digest: request.body.input_digest,
                requested_by: request.body.requested_by,
                requested_at: request.body.requested_at,
                expires_at: request.body.expires_at,
            },
            signature: request.signature,
        }
    }
}

impl ReleasePublishArguments {
    fn from_bindings(bindings: &LiveResolvedBindings) -> Self {
        Self {
            idempotency_key: bindings.idempotency_key,
            edition_id: bindings.edition_id,
            environment: "preview".to_string(),
            version_label: bindings.version_label.clone(),
            manifest_digest: bindings.manifest_digest.clone(),
        }
    }

    fn as_value(&self) -> Result<Value, AgentRuntimeError> {
        serde_json::to_value(self).map_err(|error| AgentRuntimeError::LiveSetup(error.to_string()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveResolvedBindings {
    preflight_evidence_digest: ContentDigest,
    run_id: Uuid,
    agent_id: Uuid,
    agent_principal_id: PrincipalId,
    approver_principal_id: PrincipalId,
    delegation_id: Uuid,
    delegation_digest: ContentDigest,
    edition_id: Uuid,
    manifest_digest: String,
    idempotency_key: Uuid,
    version_label: String,
    process_epoch_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveAuthorityEvidence {
    delegation_id: Uuid,
    delegation_digest: ContentDigest,
    allowed_operations: Vec<String>,
    allowed_domains: Vec<String>,
    valid_until: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LivePolicyEvidence {
    preflight_evidence: PreflightEvidence,
    loaded_delegation: StrictDelegation,
    delegation_chain: DelegationChainWire,
    resolved_bindings: LiveResolvedBindings,
    /// The sole intentionally dynamic persisted object. It is accepted only
    /// when it exactly equals the embedded template resolved from bindings.
    resolved_policy: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LivePolicyBinding {
    preflight_evidence_digest: ContentDigest,
    template_policy_digest: ContentDigest,
    bindings_digest: ContentDigest,
    resolved_policy_digest: ContentDigest,
    check_set_digest: ContentDigest,
    tamper_vector_set_digest: ContentDigest,
    pricing_schedule_digest: ContentDigest,
    instructions_digest: ContentDigest,
    initial_input_digest: ContentDigest,
    parameters_schema_digest: ContentDigest,
    tool_declaration_digest: ContentDigest,
    tool_set_digest: ContentDigest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveProviderConfig {
    name: String,
    endpoint: String,
    requested_model: String,
    service_tier: String,
    tool_choice: String,
    max_output_tokens: u32,
    store: bool,
    stream: bool,
    background: bool,
    parallel_tool_calls: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ProviderCostStatus {
    Reported,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveCumulativeCost {
    provider_cost_microusd: Option<u64>,
    provider_cost_status: ProviderCostStatus,
    calculated_cost_microusd: u64,
    pricing_schedule_id: String,
    pricing_schedule_digest: ContentDigest,
}

#[derive(Debug, Clone, Serialize)]
struct LiveTerminalVerification {
    artifact_identity_digest: ContentDigest,
    artifact_file_integrity_digest: ContentDigest,
    approval_integrity_digest: ContentDigest,
    proof_integrity_digest: ContentDigest,
    replay_verification_digest: ContentDigest,
    exact_report_digest: ContentDigest,
    step_id: Uuid,
    approval_request_id: Uuid,
    approver_id: PrincipalId,
    proof_id: Uuid,
    publication_id: Uuid,
    artifact_relative_path: String,
    artifact_digest: String,
    approval_decided_at: DateTime<Utc>,
    executed_at: DateTime<Utc>,
    mutation_count_before_replay: u32,
    mutation_count_after_replay: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveToolDeclaration {
    #[serde(rename = "type")]
    kind: String,
    name: String,
    description: String,
    parameters: LiveParametersSchema,
    strict: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveParametersSchema {
    #[serde(rename = "$schema")]
    schema: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(rename = "additionalProperties")]
    additional_properties: bool,
    required: Vec<String>,
    properties: LiveParameterProperties,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveParameterProperties {
    idempotency_key: LiveUuidParameter,
    edition_id: LiveUuidParameter,
    environment: LiveEnvironmentParameter,
    version_label: LiveVersionParameter,
    manifest_digest: LiveManifestParameter,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveUuidParameter {
    #[serde(rename = "type")]
    kind: String,
    format: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveEnvironmentParameter {
    #[serde(rename = "type")]
    kind: String,
    #[serde(rename = "const")]
    constant: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveVersionParameter {
    #[serde(rename = "type")]
    kind: String,
    #[serde(rename = "minLength")]
    min_length: u32,
    #[serde(rename = "maxLength")]
    max_length: u32,
    pattern: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveManifestParameter {
    #[serde(rename = "type")]
    kind: String,
    pattern: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveRequest {
    endpoint: String,
    requested_model: String,
    instructions: String,
    input: LiveModelInput,
    previous_response_id: Option<String>,
    function_names: Vec<String>,
    tool_declarations: Vec<LiveToolDeclaration>,
    tool_choice: String,
    service_tier: String,
    store: bool,
    stream: bool,
    background: bool,
    parallel_tool_calls: bool,
    max_output_tokens: u32,
    request_body_digest: ContentDigest,
    instructions_digest: ContentDigest,
    input_digest: ContentDigest,
    parameters_schema_digest: ContentDigest,
    tool_declaration_digest: ContentDigest,
    tool_set_digest: ContentDigest,
}

impl LiveRequest {
    fn as_model_request(&self) -> Result<ModelTurnRequest, AgentRuntimeError> {
        let declaration = self.tool_declarations.first().ok_or_else(|| {
            AgentRuntimeError::LiveSetup("sealed request has no declaration".to_string())
        })?;
        let tool = AgentFunctionTool {
            name: declaration.name.clone(),
            description: declaration.description.clone(),
            parameters: serde_json::to_value(&declaration.parameters)
                .map_err(|error| AgentRuntimeError::LiveSetup(error.to_string()))?,
            operation: "release.publish".to_string(),
            version: "v2".to_string(),
        };
        Ok(ModelTurnRequest {
            model: self.requested_model.clone(),
            instructions: self.instructions.clone(),
            input: self.input.as_model_input()?,
            previous_response_id: self.previous_response_id.clone(),
            tools: vec![tool],
            max_output_tokens: self.max_output_tokens,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderAttempt {
    schema: String,
    attempt_id: Uuid,
    logical_turn: u32,
    dispatch_ordinal: u32,
    retry_of: Option<Uuid>,
    state: ProviderAttemptState,
    process_epoch_id: Uuid,
    prepared_at: DateTime<Utc>,
    dispatched_at: Option<DateTime<Utc>>,
    finished_at: Option<DateTime<Utc>>,
    request: LiveRequest,
    response: Option<ProviderResponse>,
    failure: Option<ProviderFailure>,
}

/// Persisted provider evidence is intentionally narrower than a raw provider
/// body: it has the exact contract fields, while the complete body is bound by
/// `response_body_digest`.  This avoids accepting arbitrary provider-shaped
/// JSON in a durable checkpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderResponse {
    response_id: String,
    returned_model: String,
    response_body_digest: ContentDigest,
    decision_digest: ContentDigest,
    usage: LiveUsage,
    provider_cost_microusd: Option<u64>,
    provider_cost_status: ProviderCostStatus,
    calculated_cost_microusd: u64,
    cumulative_input_tokens: u64,
    cumulative_output_tokens: u64,
    cumulative_total_tokens: u64,
    cumulative_provider_cost_microusd: Option<u64>,
    cumulative_provider_cost_status: ProviderCostStatus,
    cumulative_calculated_cost_microusd: u64,
    pricing_schedule_id: String,
    pricing_schedule_digest: ContentDigest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum LiveCommittedDecision {
    ToolCall {
        call_id: String,
        name: String,
        arguments: ReleasePublishArguments,
    },
    Finish {
        output: String,
    },
}

impl LiveCommittedDecision {
    fn from_model(decision: &ModelDecision) -> Result<Self, AgentRuntimeError> {
        match decision {
            ModelDecision::ToolCall {
                call_id,
                name,
                arguments,
            } => Ok(Self::ToolCall {
                call_id: call_id.clone(),
                name: name.clone(),
                arguments: serde_json::from_value(arguments.clone()).map_err(|_| {
                    AgentRuntimeError::LiveSetup(
                        "committed tool decision arguments are not exact".to_string(),
                    )
                })?,
            }),
            ModelDecision::Finish { output } => Ok(Self::Finish {
                output: output.clone(),
            }),
        }
    }

    fn digest(&self) -> Result<ContentDigest, AgentRuntimeError> {
        value_digest(
            &serde_json::to_value(self)
                .map_err(|error| AgentRuntimeError::LiveSetup(error.to_string()))?,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveModelRespondedEvent {
    schema: String,
    live: bool,
    attempt_id: Uuid,
    response_id: String,
    decision: LiveCommittedDecision,
    usage: LiveUsage,
    requested_model: String,
    returned_model: String,
    response_body_digest: ContentDigest,
    decision_digest: ContentDigest,
    request_body_digest: ContentDigest,
    provider_cost_microusd: Option<u64>,
    provider_cost_status: ProviderCostStatus,
    calculated_cost_microusd: u64,
    cumulative_input_tokens: u64,
    cumulative_output_tokens: u64,
    cumulative_total_tokens: u64,
    cumulative_provider_cost_microusd: Option<u64>,
    cumulative_provider_cost_status: ProviderCostStatus,
    cumulative_calculated_cost_microusd: u64,
    pricing_schedule_id: String,
    pricing_schedule_digest: ContentDigest,
}

impl LiveModelRespondedEvent {
    fn from_attempt(
        attempt: &ProviderAttempt,
        decision: LiveCommittedDecision,
    ) -> Result<Self, AgentRuntimeError> {
        let response = attempt.response.as_ref().ok_or_else(|| {
            AgentRuntimeError::LiveSetup("committed response missing".to_string())
        })?;
        Ok(Self {
            schema: "proof-live-model-responded-event/v1".to_string(),
            live: true,
            attempt_id: attempt.attempt_id,
            response_id: response.response_id.clone(),
            decision,
            usage: response.usage,
            requested_model: attempt.request.requested_model.clone(),
            returned_model: response.returned_model.clone(),
            response_body_digest: response.response_body_digest,
            decision_digest: response.decision_digest,
            request_body_digest: attempt.request.request_body_digest,
            provider_cost_microusd: response.provider_cost_microusd,
            provider_cost_status: response.provider_cost_status,
            calculated_cost_microusd: response.calculated_cost_microusd,
            cumulative_input_tokens: response.cumulative_input_tokens,
            cumulative_output_tokens: response.cumulative_output_tokens,
            cumulative_total_tokens: response.cumulative_total_tokens,
            cumulative_provider_cost_microusd: response.cumulative_provider_cost_microusd,
            cumulative_provider_cost_status: response.cumulative_provider_cost_status,
            cumulative_calculated_cost_microusd: response.cumulative_calculated_cost_microusd,
            pricing_schedule_id: response.pricing_schedule_id.clone(),
            pricing_schedule_digest: response.pricing_schedule_digest,
        })
    }
}

fn exact_committed_event(
    run_id: Uuid,
    attempt: &ProviderAttempt,
    events: &[AgentRunEvent],
) -> Result<LiveCommittedDecision, AgentRuntimeError> {
    let candidates = events
        .iter()
        .filter(|event| {
            event.kind == AgentRunEventKind::ModelResponded
                && event.data["attempt_id"] == json!(attempt.attempt_id)
        })
        .collect::<Vec<_>>();
    if candidates.len() != 1 {
        return Err(AgentRuntimeError::InvalidCheckpoint(run_id));
    }
    let event = candidates[0];
    let parsed: LiveModelRespondedEvent = serde_json::from_value(event.data.clone())
        .map_err(|_| AgentRuntimeError::InvalidCheckpoint(run_id))?;
    let expected = LiveModelRespondedEvent::from_attempt(attempt, parsed.decision.clone())?;
    let rebuilt = AgentRunEvent::create(
        run_id,
        event.sequence,
        AgentRunEventKind::ModelResponded,
        serde_json::to_value(&expected)
            .map_err(|_| AgentRuntimeError::InvalidCheckpoint(run_id))?,
        event.created_at,
    )
    .map_err(|_| AgentRuntimeError::InvalidCheckpoint(run_id))?;
    if parsed != expected
        || parsed.decision.digest()? != parsed.decision_digest
        || event.run_id != run_id
        || event.id.get_version_num() != 7
        || event.data_digest != rebuilt.data_digest
        || attempt
            .finished_at
            .is_none_or(|finished| event.created_at < finished)
    {
        return Err(AgentRuntimeError::InvalidCheckpoint(run_id));
    }
    Ok(parsed.decision)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveUsage {
    input_tokens: u64,
    output_tokens: u64,
    total_tokens: u64,
}

impl ProviderAttempt {
    fn prepared(
        attempt_id: Uuid,
        logical_turn: u32,
        dispatch_ordinal: u32,
        retry_of: Option<Uuid>,
        process_epoch_id: Uuid,
        request: LiveRequest,
    ) -> Self {
        Self {
            schema: "proof-provider-attempt/v1".to_string(),
            attempt_id,
            logical_turn,
            dispatch_ordinal,
            retry_of,
            state: ProviderAttemptState::Prepared,
            process_epoch_id,
            prepared_at: Utc::now(),
            dispatched_at: None,
            finished_at: None,
            request,
            response: None,
            failure: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveCounters {
    logical_model_turns: u32,
    provider_dispatches: u32,
    retries: u32,
    tool_attempts: u32,
    successful_publication_mutations: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveRuntimeState {
    schema: String,
    agent_id: Uuid,
    run_id: Uuid,
    started_at: DateTime<Utc>,
    process_epoch_id: Uuid,
    previous_response_id: Option<String>,
    next_input: LiveModelInput,
    pending_tool: Option<LivePendingToolCall>,
    authority: LiveAuthorityEvidence,
    policy_evidence: LivePolicyEvidence,
    policy_binding: LivePolicyBinding,
    provider: LiveProviderConfig,
    attempts: Vec<ProviderAttempt>,
    counters: LiveCounters,
    cumulative_usage: LiveUsage,
    cumulative_cost: LiveCumulativeCost,
    final_output: Option<String>,
    terminal_error: Option<String>,
}

impl LiveRuntimeState {
    #[allow(clippy::too_many_arguments)]
    fn new(
        run_id: Uuid,
        agent_id: Uuid,
        started_at: DateTime<Utc>,
        setup: &LiveRunSetup,
        bindings: LiveResolvedBindings,
        bindings_digest: ContentDigest,
        resolved_policy: Value,
        resolved_policy_digest: ContentDigest,
        request: LiveRequest,
    ) -> Self {
        Self {
            schema: "proof-agent-runtime-state/v2".to_string(),
            agent_id,
            run_id,
            started_at,
            process_epoch_id: setup.process_epoch_id,
            previous_response_id: None,
            next_input: request.input.clone(),
            pending_tool: None,
            authority: LiveAuthorityEvidence {
                delegation_id: setup.authority.delegation.id,
                delegation_digest: setup.authority.delegation_digest,
                allowed_operations: setup
                    .authority
                    .delegation
                    .scope
                    .allowed_operations
                    .clone()
                    .expect("validated exact operation scope"),
                allowed_domains: setup
                    .authority
                    .delegation
                    .scope
                    .allowed_domains
                    .clone()
                    .expect("validated exact domain scope"),
                valid_until: setup.authority.delegation.valid_until,
            },
            policy_evidence: LivePolicyEvidence {
                preflight_evidence: serde_json::from_value(setup.preflight_evidence.clone())
                    .expect("validated strict preflight evidence"),
                loaded_delegation: StrictDelegation::from(&setup.authority.delegation),
                delegation_chain: DelegationChainWire {
                    root: setup.authority.delegation_chain.root,
                    grants: setup
                        .authority
                        .delegation_chain
                        .grants
                        .iter()
                        .map(StrictDelegation::from)
                        .collect(),
                },
                resolved_bindings: bindings,
                resolved_policy,
            },
            policy_binding: LivePolicyBinding {
                preflight_evidence_digest: setup.preflight_evidence_digest,
                template_policy_digest: setup.policy.template_policy_digest,
                bindings_digest,
                resolved_policy_digest,
                check_set_digest: setup.policy.check_set_digest,
                tamper_vector_set_digest: setup.policy.tamper_vector_set_digest,
                pricing_schedule_digest: setup.policy.pricing_schedule_digest,
                instructions_digest: setup.policy.instructions_digest,
                initial_input_digest: setup.policy.initial_input_digest,
                parameters_schema_digest: setup.policy.parameters_schema_digest,
                tool_declaration_digest: setup.policy.tool_declaration_digest,
                tool_set_digest: setup.policy.tool_set_digest,
            },
            provider: LiveProviderConfig {
                name: LIVE_PROVIDER.to_string(),
                endpoint: LIVE_ENDPOINT.to_string(),
                requested_model: LIVE_MODEL.to_string(),
                service_tier: LIVE_SERVICE_TIER.to_string(),
                tool_choice: "auto".to_string(),
                max_output_tokens: 1024,
                store: true,
                stream: false,
                background: false,
                parallel_tool_calls: false,
            },
            attempts: Vec::new(),
            counters: LiveCounters {
                logical_model_turns: 0,
                provider_dispatches: 0,
                retries: 0,
                tool_attempts: 0,
                successful_publication_mutations: 0,
            },
            cumulative_usage: LiveUsage::default(),
            cumulative_cost: LiveCumulativeCost {
                provider_cost_microusd: None,
                provider_cost_status: ProviderCostStatus::Unavailable,
                calculated_cost_microusd: 0,
                pricing_schedule_id: "proof-openai-gpt-5.6-sol-pricing/2026-08-30".to_string(),
                pricing_schedule_digest: setup.policy.pricing_schedule_digest,
            },
            final_output: None,
            terminal_error: None,
        }
    }
}

fn validate_persisted_live_state(
    run_id: Uuid,
    state: &LiveRuntimeState,
) -> Result<(), AgentRuntimeError> {
    let invalid = || AgentRuntimeError::InvalidCheckpoint(run_id);
    let embedded_template: Value =
        serde_json::from_str(LIVE_POLICY_SOURCE).map_err(|_| invalid())?;
    let binding_value =
        serde_json::to_value(&state.policy_evidence.resolved_bindings).map_err(|_| invalid())?;
    let expected_resolved =
        resolve_live_policy(&embedded_template, &state.policy_evidence.resolved_bindings)
            .map_err(|_| invalid())?;
    let loaded_delegation = Delegation::from(state.policy_evidence.loaded_delegation.clone());
    let check_ids = json!(live_check_ids());
    let tamper_ids = json!(live_tamper_ids());
    let expected_declaration: LiveToolDeclaration =
        serde_json::from_value(embedded_template["tool"]["declaration"].clone())
            .map_err(|_| invalid())?;
    let declaration_value = serde_json::to_value(&expected_declaration).map_err(|_| invalid())?;
    let parameters_value =
        serde_json::to_value(&expected_declaration.parameters).map_err(|_| invalid())?;
    let instructions_value = embedded_template["outbound_data"]["instructions"].clone();
    let preflight = &state.policy_evidence.preflight_evidence;
    let chain = DelegationChain {
        root: state.policy_evidence.delegation_chain.root,
        grants: state
            .policy_evidence
            .delegation_chain
            .grants
            .iter()
            .cloned()
            .map(Delegation::from)
            .collect(),
    };
    if state.schema != "proof-agent-runtime-state/v2"
        || state.run_id != run_id
        || state.agent_id.get_version_num() != 7
        || state.process_epoch_id.get_version_num() != 7
        || state.policy_evidence.resolved_bindings.run_id != run_id
        || state.policy_evidence.resolved_bindings.agent_id != state.agent_id
        || state
            .policy_evidence
            .resolved_bindings
            .preflight_evidence_digest
            != state.policy_binding.preflight_evidence_digest
        || state.policy_evidence.resolved_bindings.delegation_id != loaded_delegation.id
        || state.policy_evidence.resolved_bindings.delegation_digest
            != state.authority.delegation_digest
        || state.policy_evidence.resolved_bindings.edition_id.is_nil()
        || state
            .policy_evidence
            .resolved_bindings
            .idempotency_key
            .get_version_num()
            != 7
        || state
            .policy_evidence
            .resolved_bindings
            .process_epoch_id
            .get_version_num()
            != 7
        || preflight.schema != "proof-release-manager-preflight-evidence/v1"
        || preflight.policy_path != "evals/release-manager-preview-v1.json"
        || preflight.evaluator != "proof-agent-trace/v1"
        || preflight.outcome != "passed"
        || preflight.score_bps != 10_000
        || preflight.passed_checks != 10
        || preflight.total_checks != 10
        || preflight.run_id.get_version_num() != 7
        || preflight.evaluation_id.get_version_num() != 7
        || preflight.policy_digest == ContentDigest::from_bytes([0; 32])
        || preflight.trace_digest == ContentDigest::from_bytes([0; 32])
        || state.authority.delegation_id != loaded_delegation.id
        || state.authority.delegation_digest
            != delegation_digest(&loaded_delegation).map_err(|_| invalid())?
        || state.authority.allowed_operations != ["release.publish"]
        || state.authority.allowed_domains != ["content"]
        || state.authority.valid_until != loaded_delegation.valid_until
        || loaded_delegation.revoked
        || loaded_delegation.recipient != state.policy_evidence.resolved_bindings.agent_principal_id
        || loaded_delegation.valid_until < state.started_at + Duration::seconds(300)
        || loaded_delegation.scope.allowed_operations.as_deref()
            != Some(&["release.publish".to_string()])
        || loaded_delegation.scope.allowed_domains.as_deref() != Some(&["content".to_string()])
        || loaded_delegation.scope.resource_scope.is_some()
        || state
            .policy_evidence
            .delegation_chain
            .grants
            .iter()
            .filter(|grant| **grant == state.policy_evidence.loaded_delegation)
            .count()
            != 1
        || chain
            .validate(
                state.policy_evidence.resolved_bindings.agent_principal_id,
                state.started_at,
            )
            .is_err()
        || state.policy_evidence.resolved_policy != expected_resolved
        || state.policy_binding.preflight_evidence_digest
            != wrapped_digest(
                "proof-release-manager-preflight-evidence-digest/v1",
                "evidence",
                &serde_json::to_value(&state.policy_evidence.preflight_evidence)
                    .map_err(|_| invalid())?,
            )
            .map_err(|_| invalid())?
        || state.policy_binding.template_policy_digest
            != value_digest(&embedded_template).map_err(|_| invalid())?
        || state.policy_binding.bindings_digest
            != wrapped_digest(
                "proof-release-manager-live-bindings-digest/v1",
                "bindings",
                &binding_value,
            )
            .map_err(|_| invalid())?
        || state.policy_binding.resolved_policy_digest
            != value_digest(&state.policy_evidence.resolved_policy).map_err(|_| invalid())?
        || state.policy_binding.check_set_digest
            != wrapped_digest(
                "proof-release-manager-live-check-set-digest/v1",
                "check_ids",
                &check_ids,
            )
            .map_err(|_| invalid())?
        || state.policy_binding.tamper_vector_set_digest
            != wrapped_digest(
                "proof-release-manager-live-tamper-vector-set-digest/v1",
                "tamper_vector_ids",
                &tamper_ids,
            )
            .map_err(|_| invalid())?
        || state.policy_binding.pricing_schedule_digest
            != value_digest(&embedded_template["pricing"]).map_err(|_| invalid())?
        || state.policy_binding.instructions_digest
            != value_digest(&instructions_value).map_err(|_| invalid())?
        || state.policy_binding.parameters_schema_digest
            != wrapped_digest(
                "proof-openai-function-parameters-digest/v1",
                "parameters",
                &parameters_value,
            )
            .map_err(|_| invalid())?
        || state.policy_binding.tool_declaration_digest
            != wrapped_digest(
                "proof-openai-function-declaration-digest/v1",
                "declaration",
                &declaration_value,
            )
            .map_err(|_| invalid())?
        || state.policy_binding.tool_set_digest
            != wrapped_digest(
                "proof-openai-tool-set-digest/v1",
                "tools",
                &json!([expected_declaration.clone()]),
            )
            .map_err(|_| invalid())?
        || state.provider.name != LIVE_PROVIDER
        || state.provider.endpoint != LIVE_ENDPOINT
        || state.provider.requested_model != LIVE_MODEL
        || state.provider.service_tier != LIVE_SERVICE_TIER
        || state.provider.tool_choice != "auto"
        || state.provider.max_output_tokens != 1024
        || !state.provider.store
        || state.provider.stream
        || state.provider.background
        || state.provider.parallel_tool_calls
        || state.cumulative_cost.pricing_schedule_id
            != "proof-openai-gpt-5.6-sol-pricing/2026-08-30"
        || state.cumulative_cost.pricing_schedule_digest
            != state.policy_binding.pricing_schedule_digest
    {
        return Err(AgentRuntimeError::InvalidCheckpoint(run_id));
    }
    let mut input = 0_u64;
    let mut output = 0_u64;
    let mut total = 0_u64;
    let mut calculated = 0_u64;
    let mut provider_cost: Option<u64> = None;
    let mut saw_committed = false;
    let mut previous_cursor = None;
    let mut retryable_attempts = 0_u32;
    let mut attempt_ids = BTreeSet::new();
    for (index, attempt) in state.attempts.iter().enumerate() {
        if !attempt_ids.insert(attempt.attempt_id) {
            return Err(AgentRuntimeError::InvalidCheckpoint(run_id));
        }
        let model_request = attempt.request.as_model_request().map_err(|_| invalid())?;
        let body = crate::openai::request_body(&model_request).map_err(|_| invalid())?;
        let expected_logical_turn = state.attempts[..index]
            .iter()
            .filter(|prior| prior.state == ProviderAttemptState::Committed)
            .count() as u32
            + 1;
        if attempt.schema != "proof-provider-attempt/v1"
            || attempt.attempt_id.get_version_num() != 7
            || attempt.process_epoch_id.get_version_num() != 7
            || attempt.logical_turn != expected_logical_turn
            || attempt.dispatch_ordinal != index as u32 + 1
            || attempt.request.endpoint != LIVE_ENDPOINT
            || attempt.request.requested_model != LIVE_MODEL
            || attempt.request.function_names != [LIVE_TOOL_NAME]
            || attempt.request.function_names.len() != 1
            || attempt.request.tool_declarations.len() != 1
            || attempt.request.tool_declarations != [expected_declaration.clone()]
            || attempt.request.previous_response_id != previous_cursor
            || attempt.request.instructions_digest
                != value_digest(&body["instructions"]).map_err(|_| invalid())?
            || attempt.request.instructions_digest != state.policy_binding.instructions_digest
            || attempt.request.input_digest
                != value_digest(&body["input"]).map_err(|_| invalid())?
            || (index == 0
                && attempt.request.input_digest != state.policy_binding.initial_input_digest)
            || attempt.request.request_body_digest
                != wrapped_digest("proof-openai-responses-request-digest/v1", "request", &body)
                    .map_err(|_| invalid())?
            || attempt.request.parameters_schema_digest
                != state.policy_binding.parameters_schema_digest
            || attempt.request.tool_declaration_digest
                != state.policy_binding.tool_declaration_digest
            || attempt.request.tool_set_digest != state.policy_binding.tool_set_digest
            || attempt.request.tool_choice != "auto"
            || attempt.request.service_tier != LIVE_SERVICE_TIER
            || !attempt.request.store
            || attempt.request.stream
            || attempt.request.background
            || attempt.request.parallel_tool_calls
            || attempt.request.max_output_tokens != 1024
            || attempt.prepared_at < state.started_at
            || attempt
                .dispatched_at
                .is_some_and(|dispatched| dispatched < attempt.prepared_at)
            || attempt
                .finished_at
                .is_some_and(|finished| finished < attempt.prepared_at)
        {
            return Err(AgentRuntimeError::InvalidCheckpoint(run_id));
        }
        match attempt.state {
            ProviderAttemptState::Prepared => {
                if attempt.dispatched_at.is_some()
                    || attempt.finished_at.is_some()
                    || attempt.response.is_some()
                    || attempt.failure.is_some()
                {
                    return Err(AgentRuntimeError::InvalidCheckpoint(run_id));
                }
            }
            ProviderAttemptState::Dispatching | ProviderAttemptState::ResponseReceived => {
                if attempt.dispatched_at.is_none()
                    || attempt.finished_at.is_some()
                    || attempt.failure.is_some()
                    || (attempt.state == ProviderAttemptState::Dispatching
                        && attempt.response.is_some())
                {
                    return Err(AgentRuntimeError::InvalidCheckpoint(run_id));
                }
                if matches!(attempt.state, ProviderAttemptState::ResponseReceived)
                    && attempt.response.is_none()
                {
                    return Err(AgentRuntimeError::InvalidCheckpoint(run_id));
                }
                if let Some(response) = attempt.response.as_ref() {
                    let projected_input = input
                        .checked_add(response.usage.input_tokens)
                        .ok_or_else(invalid)?;
                    let projected_output = output
                        .checked_add(response.usage.output_tokens)
                        .ok_or_else(invalid)?;
                    let projected_total = total
                        .checked_add(response.usage.total_tokens)
                        .ok_or_else(invalid)?;
                    let projected_calculated = calculated
                        .checked_add(response.calculated_cost_microusd)
                        .ok_or_else(invalid)?;
                    let projected_provider = if !saw_committed {
                        response.provider_cost_microusd
                    } else {
                        match (provider_cost, response.provider_cost_microusd) {
                            (Some(prior), Some(cost)) => {
                                Some(prior.checked_add(cost).ok_or_else(invalid)?)
                            }
                            _ => None,
                        }
                    };
                    if response.returned_model != LIVE_MODEL
                        || response.response_id.trim().is_empty()
                        || response.response_body_digest == ContentDigest::from_bytes([0; 32])
                        || response.decision_digest == ContentDigest::from_bytes([0; 32])
                        || response.usage.input_tokens == 0
                        || response.usage.output_tokens == 0
                        || response.usage.total_tokens
                            != response
                                .usage
                                .input_tokens
                                .checked_add(response.usage.output_tokens)
                                .ok_or_else(invalid)?
                        || response.pricing_schedule_id
                            != "proof-openai-gpt-5.6-sol-pricing/2026-08-30"
                        || response.pricing_schedule_digest
                            != state.policy_binding.pricing_schedule_digest
                        || response.calculated_cost_microusd
                            != response
                                .usage
                                .input_tokens
                                .checked_mul(5)
                                .and_then(|cost| {
                                    response
                                        .usage
                                        .output_tokens
                                        .checked_mul(20)
                                        .and_then(|output_cost| cost.checked_add(output_cost))
                                })
                                .ok_or_else(invalid)?
                        || response.cumulative_input_tokens != projected_input
                        || response.cumulative_output_tokens != projected_output
                        || response.cumulative_total_tokens != projected_total
                        || response.cumulative_calculated_cost_microusd != projected_calculated
                        || response.cumulative_provider_cost_microusd != projected_provider
                        || response.cumulative_provider_cost_status
                            != if projected_provider.is_some() {
                                ProviderCostStatus::Reported
                            } else {
                                ProviderCostStatus::Unavailable
                            }
                        || match response.provider_cost_status {
                            ProviderCostStatus::Reported => {
                                response.provider_cost_microusd.is_none()
                            }
                            ProviderCostStatus::Unavailable => {
                                response.provider_cost_microusd.is_some()
                            }
                        }
                    {
                        return Err(invalid());
                    }
                }
            }
            ProviderAttemptState::Committed => {
                let response = attempt
                    .response
                    .as_ref()
                    .ok_or(AgentRuntimeError::InvalidCheckpoint(run_id))?;
                if attempt.dispatched_at.is_none()
                    || attempt.finished_at.is_none()
                    || attempt.failure.is_some()
                    || response.returned_model != LIVE_MODEL
                    || response.response_id.trim().is_empty()
                    || response.response_body_digest == ContentDigest::from_bytes([0; 32])
                    || response.decision_digest == ContentDigest::from_bytes([0; 32])
                    || response.usage.input_tokens == 0
                    || response.usage.output_tokens == 0
                    || response.usage.total_tokens == 0
                    || response.usage.total_tokens
                        != response
                            .usage
                            .input_tokens
                            .checked_add(response.usage.output_tokens)
                            .ok_or(AgentRuntimeError::InvalidCheckpoint(run_id))?
                    || response.pricing_schedule_id != "proof-openai-gpt-5.6-sol-pricing/2026-08-30"
                    || response.pricing_schedule_digest
                        != state.policy_binding.pricing_schedule_digest
                    || response.calculated_cost_microusd
                        != response
                            .usage
                            .input_tokens
                            .checked_mul(5)
                            .and_then(|input_cost| {
                                response
                                    .usage
                                    .output_tokens
                                    .checked_mul(20)
                                    .and_then(|output_cost| input_cost.checked_add(output_cost))
                            })
                            .ok_or_else(invalid)?
                    || match response.provider_cost_status {
                        ProviderCostStatus::Reported => response.provider_cost_microusd.is_none(),
                        ProviderCostStatus::Unavailable => {
                            response.provider_cost_microusd.is_some()
                        }
                    }
                {
                    return Err(AgentRuntimeError::InvalidCheckpoint(run_id));
                }
                input = input
                    .checked_add(response.usage.input_tokens)
                    .ok_or(AgentRuntimeError::InvalidCheckpoint(run_id))?;
                output = output
                    .checked_add(response.usage.output_tokens)
                    .ok_or(AgentRuntimeError::InvalidCheckpoint(run_id))?;
                total = total
                    .checked_add(response.usage.total_tokens)
                    .ok_or(AgentRuntimeError::InvalidCheckpoint(run_id))?;
                calculated = calculated
                    .checked_add(response.calculated_cost_microusd)
                    .ok_or(AgentRuntimeError::InvalidCheckpoint(run_id))?;
                provider_cost = if !saw_committed {
                    response.provider_cost_microusd
                } else {
                    match (provider_cost, response.provider_cost_microusd) {
                        (Some(prior), Some(cost)) => Some(
                            prior
                                .checked_add(cost)
                                .ok_or(AgentRuntimeError::InvalidCheckpoint(run_id))?,
                        ),
                        _ => None,
                    }
                };
                if response.cumulative_input_tokens != input
                    || response.cumulative_output_tokens != output
                    || response.cumulative_total_tokens != total
                    || response.cumulative_calculated_cost_microusd != calculated
                    || response.cumulative_provider_cost_microusd != provider_cost
                    || response.cumulative_provider_cost_status
                        != if provider_cost.is_some() {
                            ProviderCostStatus::Reported
                        } else {
                            ProviderCostStatus::Unavailable
                        }
                {
                    return Err(AgentRuntimeError::InvalidCheckpoint(run_id));
                }
                saw_committed = true;
                previous_cursor = Some(response.response_id.clone());
            }
            ProviderAttemptState::RejectedRetryable | ProviderAttemptState::FailedRetryable => {
                retryable_attempts = retryable_attempts
                    .checked_add(1)
                    .ok_or(AgentRuntimeError::InvalidCheckpoint(run_id))?;
                if attempt.dispatched_at.is_none()
                    || attempt.finished_at.is_none()
                    || attempt.response.is_some()
                    || !matches!(
                        attempt.failure.as_ref().map(|failure| failure.class),
                        Some(ProviderFailureClass::CertifiedNoBytes)
                            if attempt.state == ProviderAttemptState::FailedRetryable
                    ) && !matches!(
                        attempt.failure.as_ref().map(|failure| failure.class),
                        Some(ProviderFailureClass::Explicit429)
                            if attempt.state == ProviderAttemptState::RejectedRetryable
                    )
                {
                    return Err(AgentRuntimeError::InvalidCheckpoint(run_id));
                }
                let failure = attempt.failure.as_ref().ok_or_else(invalid)?;
                if failure.detail != "redacted"
                    || match attempt.state {
                        ProviderAttemptState::FailedRetryable => {
                            failure.code != "certified_no_bytes"
                        }
                        ProviderAttemptState::RejectedRetryable => failure.code != "http_429",
                        _ => unreachable!(),
                    }
                {
                    return Err(invalid());
                }
            }
            ProviderAttemptState::FailedTerminal | ProviderAttemptState::Ambiguous => {
                if attempt.finished_at.is_none()
                    || attempt.response.is_some()
                    || !matches!(
                        attempt.failure.as_ref().map(|failure| failure.class),
                        Some(ProviderFailureClass::Terminal)
                            if attempt.state == ProviderAttemptState::FailedTerminal
                    ) && !matches!(
                        attempt.failure.as_ref().map(|failure| failure.class),
                        Some(ProviderFailureClass::Ambiguous)
                            if attempt.state == ProviderAttemptState::Ambiguous
                    )
                {
                    return Err(AgentRuntimeError::InvalidCheckpoint(run_id));
                }
                let failure = attempt.failure.as_ref().ok_or_else(invalid)?;
                let known_code = match attempt.state {
                    ProviderAttemptState::FailedTerminal => matches!(
                        failure.code.as_str(),
                        "gateway_factory_failed"
                            | "gateway_provider_mismatch"
                            | "retry_limit_exhausted"
                            | "provider_terminal_rejection"
                            | "deadline_exceeded"
                            | "returned_model_mismatch"
                            | "output_token_limit_exceeded"
                            | "token_or_cost_limit_exceeded"
                    ),
                    ProviderAttemptState::Ambiguous => matches!(
                        failure.code.as_str(),
                        "provider_outcome_unknown"
                            | "missing_response_body_digest"
                            | "missing_returned_model"
                            | "invalid_response_usage"
                            | "invalid_decision_shape"
                    ),
                    _ => unreachable!(),
                };
                if failure.detail != "redacted"
                    || !known_code
                    || (attempt.state == ProviderAttemptState::Ambiguous
                        && attempt.dispatched_at.is_none())
                {
                    return Err(invalid());
                }
            }
        }
        if let Some(parent) = attempt.retry_of {
            let parent = state
                .attempts
                .iter()
                .take(index)
                .find(|candidate| candidate.attempt_id == parent)
                .ok_or(AgentRuntimeError::InvalidCheckpoint(run_id))?;
            if !matches!(
                parent.state,
                ProviderAttemptState::RejectedRetryable | ProviderAttemptState::FailedRetryable
            ) || attempt.logical_turn != parent.logical_turn
                || attempt.request != parent.request
            {
                return Err(AgentRuntimeError::InvalidCheckpoint(run_id));
            }
        }
    }
    for (index, parent) in state.attempts.iter().enumerate().filter(|(_, attempt)| {
        matches!(
            attempt.state,
            ProviderAttemptState::FailedRetryable | ProviderAttemptState::RejectedRetryable
        )
    }) {
        if let Some(child) = state.attempts.get(index + 1) {
            if child.retry_of != Some(parent.attempt_id)
                || child.logical_turn != parent.logical_turn
                || child.request != parent.request
            {
                return Err(invalid());
            }
        } else if index + 1 != state.attempts.len() {
            return Err(invalid());
        }
    }
    let dispatches = state
        .attempts
        .iter()
        .filter(|attempt| attempt.dispatched_at.is_some())
        .count() as u32;
    let pending_decision_digest_valid = match state.pending_tool.as_ref() {
        Some(pending) => state
            .attempts
            .iter()
            .rev()
            .find(|attempt| attempt.state == ProviderAttemptState::Committed)
            .and_then(|attempt| attempt.response.as_ref())
            .is_some_and(|response| {
                LiveCommittedDecision::ToolCall {
                    call_id: pending.call_id.clone(),
                    name: pending.tool_name.clone(),
                    arguments: pending.arguments.clone(),
                }
                .digest()
                .is_ok_and(|digest| digest == response.decision_digest)
            }),
        None => true,
    };
    if state.counters.provider_dispatches != dispatches
        || state.counters.logical_model_turns
            != state
                .attempts
                .iter()
                .filter(|attempt| attempt.state == ProviderAttemptState::Committed)
                .count() as u32
        || state.counters.retries != retryable_attempts
        || state.counters.retries > 1
        || state.cumulative_usage.input_tokens != input
        || state.cumulative_usage.output_tokens != output
        || state.cumulative_usage.total_tokens != total
        || state.previous_response_id != previous_cursor
        || state.cumulative_cost.calculated_cost_microusd != calculated
        || state.cumulative_cost.provider_cost_microusd != provider_cost
        || state.cumulative_cost.provider_cost_status
            != if provider_cost.is_some() {
                ProviderCostStatus::Reported
            } else {
                ProviderCostStatus::Unavailable
            }
        || state.counters.tool_attempts > 1
        || state.counters.successful_publication_mutations > 1
        || state.counters.successful_publication_mutations > state.counters.tool_attempts
        || !pending_decision_digest_valid
        || (state.pending_tool.is_some()
            && (state.counters.tool_attempts != 1
                || state.counters.successful_publication_mutations != 0))
        || (state.counters.tool_attempts == 0
            && (!matches!(state.next_input, LiveModelInput::Goal { .. })
                || state.pending_tool.is_some()
                || state.counters.successful_publication_mutations != 0))
        || (state.counters.successful_publication_mutations == 1
            && (!matches!(state.next_input, LiveModelInput::ToolOutput { .. })
                || state.pending_tool.is_some()))
        || (state.final_output.is_some() && state.counters.successful_publication_mutations != 1)
        || (state.final_output.is_some() && state.terminal_error.is_some())
        || state.pending_tool.as_ref().is_some_and(|pending| {
            pending.call_id.trim().is_empty()
                || pending.tool_name != LIVE_TOOL_NAME
                || pending.operation != "release.publish"
                || pending.version != "v2"
                || pending.arguments
                    != ReleasePublishArguments::from_bindings(
                        &state.policy_evidence.resolved_bindings,
                    )
                || pending.step_id.get_version_num() != 7
                || pending.approval_request_id.get_version_num() != 7
                || pending.step_id != pending.step_intent.id
                || pending.approval_request_id != pending.step_intent.approval_request_id
                || pending.approval_request_id != pending.approval_request.body.id
                || pending.step_intent.run_id != run_id
                || pending.step_intent.operation != "release.publish"
                || pending.step_intent.version != "v2"
                || pending.step_intent.input_digest
                    != digest(
                        ArtifactKind::OperationInput,
                        &canonicalize(&pending.arguments.as_value().unwrap_or(Value::Null))
                            .unwrap_or_else(|_| {
                                canonicalize(&Value::Null).expect("null canonicalizes")
                            }),
                    )
                || pending.approval_request.body.operation != "release.publish"
                || pending.approval_request.body.version != "v2"
                || pending.approval_request.body.input_digest != pending.step_intent.input_digest
                || pending.approval_request.body.requested_by
                    != state.policy_evidence.resolved_bindings.agent_principal_id
                || pending.approval_request.body.expires_at > run_deadline(state.started_at, 300)
                || pending.approval_request.signature.is_empty()
        })
        || matches!(&state.next_input, LiveModelInput::ToolOutput { call_id, output }
            if call_id.trim().is_empty()
                || !output.ok
                || output.result.operation != "release.publish"
                || output.result.data.edition_id
                    != state.policy_evidence.resolved_bindings.edition_id
                || output.result.data.environment != "preview"
                || output.result.data.version_label
                    != state.policy_evidence.resolved_bindings.version_label
                || output.result.data.manifest_digest
                    != state.policy_evidence.resolved_bindings.manifest_digest
                || output.result.data.artifact.schema
                    != "proof-content-preview-artifact/v1"
                || output.proof_id.get_version_num() != 7)
    {
        return Err(AgentRuntimeError::InvalidCheckpoint(run_id));
    }
    Ok(())
}

fn validate_runtime_checkpoint_envelope(
    expected_run_id: Uuid,
    checkpoint: &AgentCheckpoint,
) -> Result<(), AgentRuntimeError> {
    let invalid = || AgentRuntimeError::InvalidCheckpoint(expected_run_id);
    let canonical = canonicalize(&checkpoint.state).map_err(|_| invalid())?;
    let state = checkpoint.state.as_object().ok_or_else(invalid)?;
    let kind = state
        .get("kind")
        .and_then(Value::as_str)
        .ok_or_else(invalid)?;
    let exact_shape = match kind {
        RUNTIME_CHECKPOINT_KIND => {
            state.len() == 2
                || (state.len() == 3
                    && state.get("terminal_event_kind").is_some_and(|value| {
                        serde_json::from_value::<AgentRunEventKind>(value.clone()).is_ok_and(
                            |kind| {
                                matches!(
                                    kind,
                                    AgentRunEventKind::Failed | AgentRunEventKind::BudgetExceeded
                                )
                            },
                        )
                    }))
        }
        LIVE_RUNTIME_CHECKPOINT_KIND => state.len() == 2,
        _ => false,
    };
    if checkpoint.run_id != expected_run_id
        || checkpoint.id.get_version_num() != 7
        || checkpoint.state_digest != digest(ArtifactKind::AgentCheckpoint, &canonical)
        || !exact_shape
        || !state.contains_key("kind")
        || !state.contains_key("runtime")
    {
        return Err(invalid());
    }
    Ok(())
}

fn validated_live_state_history(
    run_id: Uuid,
    checkpoints: &[AgentCheckpoint],
) -> Result<LiveRuntimeState, AgentRuntimeError> {
    if checkpoints.iter().enumerate().any(|(index, checkpoint)| {
        checkpoint.sequence != u32::try_from(index).unwrap_or(u32::MAX)
            || checkpoint.state["kind"] != LIVE_RUNTIME_CHECKPOINT_KIND
    }) {
        return Err(AgentRuntimeError::InvalidCheckpoint(run_id));
    }
    let live_checkpoints = checkpoints
        .iter()
        .filter(|checkpoint| checkpoint.state["kind"] == LIVE_RUNTIME_CHECKPOINT_KIND)
        .collect::<Vec<_>>();
    if live_checkpoints.is_empty() {
        return Err(AgentRuntimeError::MissingCheckpoint(run_id));
    }

    // Checkpoint history is append-only. Counter, terminal, and attempt
    // regressions are therefore corrupt rather than a permissible resume or
    // approval-review source.
    let mut prior: Option<LiveRuntimeState> = None;
    let mut immutable_epochs = BTreeSet::new();
    let mut previous_sequence = None;
    let mut checkpoint_ids = BTreeSet::new();
    for checkpoint in live_checkpoints {
        validate_runtime_checkpoint_envelope(run_id, checkpoint)?;
        if !checkpoint_ids.insert(checkpoint.id)
            || previous_sequence.is_some_and(|sequence| checkpoint.sequence <= sequence)
        {
            return Err(AgentRuntimeError::InvalidCheckpoint(run_id));
        }
        previous_sequence = Some(checkpoint.sequence);
        let current: LiveRuntimeState = serde_json::from_value(checkpoint.state["runtime"].clone())
            .map_err(|_| AgentRuntimeError::InvalidCheckpoint(run_id))?;
        validate_persisted_live_state(run_id, &current)?;
        immutable_epochs.insert(current.process_epoch_id);
        if current
            .attempts
            .iter()
            .any(|attempt| !immutable_epochs.contains(&attempt.process_epoch_id))
        {
            return Err(AgentRuntimeError::InvalidCheckpoint(run_id));
        }
        if let Some(previous) = prior.as_ref() {
            if current.schema != previous.schema
                || current.agent_id != previous.agent_id
                || current.run_id != previous.run_id
                || current.started_at != previous.started_at
                || current.authority != previous.authority
                || current.policy_evidence != previous.policy_evidence
                || current.policy_binding != previous.policy_binding
                || current.provider != previous.provider
                || current.cumulative_cost.pricing_schedule_id
                    != previous.cumulative_cost.pricing_schedule_id
                || current.cumulative_cost.pricing_schedule_digest
                    != previous.cumulative_cost.pricing_schedule_digest
                || current.attempts.len() < previous.attempts.len()
                || current.counters.provider_dispatches < previous.counters.provider_dispatches
                || current.counters.logical_model_turns < previous.counters.logical_model_turns
                || current.counters.retries < previous.counters.retries
                || current.counters.tool_attempts < previous.counters.tool_attempts
                || current.counters.successful_publication_mutations
                    < previous.counters.successful_publication_mutations
                || current.cumulative_usage.input_tokens < previous.cumulative_usage.input_tokens
                || current.cumulative_usage.output_tokens < previous.cumulative_usage.output_tokens
                || current.cumulative_usage.total_tokens < previous.cumulative_usage.total_tokens
                || current.cumulative_cost.calculated_cost_microusd
                    < previous.cumulative_cost.calculated_cost_microusd
                || (previous.terminal_error.is_some() && current.terminal_error.is_none())
                || previous
                    .terminal_error
                    .as_ref()
                    .is_some_and(|error| current.terminal_error.as_ref() != Some(error))
                || previous
                    .final_output
                    .as_ref()
                    .is_some_and(|output| current.final_output.as_ref() != Some(output))
            {
                return Err(AgentRuntimeError::InvalidCheckpoint(run_id));
            }
            for (index, before) in previous.attempts.iter().enumerate() {
                let after = &current.attempts[index];
                let immutable = before.schema == after.schema
                    && before.attempt_id == after.attempt_id
                    && before.logical_turn == after.logical_turn
                    && before.dispatch_ordinal == after.dispatch_ordinal
                    && before.retry_of == after.retry_of
                    && before.process_epoch_id == after.process_epoch_id
                    && before.prepared_at == after.prepared_at
                    && before.request == after.request
                    && before
                        .dispatched_at
                        .is_none_or(|value| after.dispatched_at == Some(value))
                    && before
                        .finished_at
                        .is_none_or(|value| after.finished_at == Some(value))
                    && before
                        .response
                        .as_ref()
                        .is_none_or(|value| after.response.as_ref() == Some(value))
                    && before
                        .failure
                        .as_ref()
                        .is_none_or(|value| after.failure.as_ref() == Some(value));
                let valid_transition = before.state == after.state
                    || matches!(
                        (before.state, after.state),
                        (
                            ProviderAttemptState::Prepared,
                            ProviderAttemptState::Dispatching
                                | ProviderAttemptState::FailedTerminal
                        ) | (
                            ProviderAttemptState::Dispatching,
                            ProviderAttemptState::ResponseReceived
                                | ProviderAttemptState::RejectedRetryable
                                | ProviderAttemptState::FailedRetryable
                                | ProviderAttemptState::FailedTerminal
                                | ProviderAttemptState::Ambiguous
                        ) | (
                            ProviderAttemptState::ResponseReceived,
                            ProviderAttemptState::Committed
                        )
                    );
                if !immutable || !valid_transition {
                    return Err(AgentRuntimeError::InvalidCheckpoint(run_id));
                }
            }
            if current.attempts[previous.attempts.len()..]
                .iter()
                .any(|attempt| attempt.process_epoch_id != current.process_epoch_id)
            {
                return Err(AgentRuntimeError::InvalidCheckpoint(run_id));
            }
        } else if current
            .attempts
            .iter()
            .any(|attempt| attempt.process_epoch_id != current.process_epoch_id)
        {
            return Err(AgentRuntimeError::InvalidCheckpoint(run_id));
        }
        prior = Some(current);
    }
    prior.ok_or(AgentRuntimeError::MissingCheckpoint(run_id))
}

fn live_pending_matches_committed_decision(
    state: &LiveRuntimeState,
    committed_decisions: &[LiveCommittedDecision],
) -> bool {
    state.pending_tool.as_ref().is_none_or(|pending| {
        matches!(
            committed_decisions.last(),
            Some(LiveCommittedDecision::ToolCall {
                call_id,
                name,
                arguments,
            }) if call_id == &pending.call_id
                && name == &pending.tool_name
                && arguments == &pending.arguments
        )
    })
}

/// Validates and projects the newest state from a complete, ordered native
/// runtime checkpoint history. The slice must start at sequence zero and must
/// contain exactly one supported runtime version. This validator deliberately
/// does not require causal events: checkpoints are durable barriers that can
/// validly precede their corresponding event during crash recovery.
pub fn runtime_state_view(
    expected_run_id: Uuid,
    checkpoints: &[AgentCheckpoint],
) -> Result<RuntimeStateView, AgentRuntimeError> {
    let invalid = || AgentRuntimeError::InvalidCheckpoint(expected_run_id);
    if checkpoints.is_empty() {
        return Err(AgentRuntimeError::MissingCheckpoint(expected_run_id));
    }
    let mut kinds = BTreeSet::new();
    for (index, checkpoint) in checkpoints.iter().enumerate() {
        let kind = checkpoint
            .state
            .get("kind")
            .and_then(Value::as_str)
            .ok_or_else(invalid)?;
        if checkpoint.sequence != u32::try_from(index).map_err(|_| invalid())?
            || !kind.starts_with("agent_runtime_")
        {
            return Err(invalid());
        }
        validate_runtime_checkpoint_envelope(expected_run_id, checkpoint)?;
        kinds.insert(kind);
    }
    if kinds.len() != 1 {
        return Err(invalid());
    }
    let checkpoint_kind = *kinds.first().ok_or_else(invalid)?;

    match checkpoint_kind {
        RUNTIME_CHECKPOINT_KIND => {
            let mut latest = None;
            let mut identity = None;
            let mut checkpoint_ids = BTreeSet::new();
            for checkpoint in checkpoints {
                if !checkpoint_ids.insert(checkpoint.id) {
                    return Err(invalid());
                }
                let persisted = checkpoint
                    .state
                    .get("runtime")
                    .cloned()
                    .ok_or_else(invalid)?;
                let state: AgentRuntimeState =
                    serde_json::from_value(persisted.clone()).map_err(|_| invalid())?;
                let projected = serde_json::to_value(&state).map_err(|_| invalid())?;
                if projected != persisted
                    || state.agent_id.get_version_num() != 7
                    || identity.is_some_and(|(agent_id, started_at)| {
                        agent_id != state.agent_id || started_at != state.started_at
                    })
                {
                    return Err(invalid());
                }
                identity = Some((state.agent_id, state.started_at));
                latest = Some(projected);
            }
            Ok(RuntimeStateView {
                checkpoint_kind: checkpoint_kind.to_string(),
                state: latest.ok_or_else(invalid)?,
            })
        }
        LIVE_RUNTIME_CHECKPOINT_KIND => {
            let state = validated_live_state_history(expected_run_id, checkpoints)?;
            let projected = serde_json::to_value(&state).map_err(|_| invalid())?;
            if checkpoints
                .last()
                .and_then(|checkpoint| checkpoint.state.get("runtime"))
                != Some(&projected)
            {
                return Err(invalid());
            }
            Ok(RuntimeStateView {
                checkpoint_kind: checkpoint_kind.to_string(),
                state: projected,
            })
        }
        _ => Err(invalid()),
    }
}

/// Validates a durable runtime checkpoint and projects the exact pending tool
/// call needed by an approval review surface.
///
/// `checkpoints` must be the complete native history beginning at sequence
/// zero. Live-v2 checkpoints pass the authoritative state validator and are
/// also bound to their exact committed response events before any arguments
/// are returned. Unsupported versions and malformed envelopes fail closed.
pub fn runtime_approval_context(
    expected_run_id: Uuid,
    checkpoints: &[AgentCheckpoint],
    events: &[AgentRunEvent],
) -> Result<RuntimeApprovalContext, AgentRuntimeError> {
    let invalid = || AgentRuntimeError::InvalidCheckpoint(expected_run_id);
    let view = runtime_state_view(expected_run_id, checkpoints)?;
    let checkpoint_kind = view.checkpoint_kind.as_str();

    match checkpoint_kind {
        RUNTIME_CHECKPOINT_KIND => {
            let state: AgentRuntimeState =
                serde_json::from_value(view.state).map_err(|_| invalid())?;
            Ok(RuntimeApprovalContext {
                checkpoint_kind: checkpoint_kind.to_string(),
                run_id: expected_run_id,
                agent_id: state.agent_id,
                required_approver_id: None,
                pending_tool: state.pending_tool,
                sealed_approval_request: None,
                sealed_step: None,
            })
        }
        LIVE_RUNTIME_CHECKPOINT_KIND => {
            let state = validated_live_state_history(expected_run_id, checkpoints)?;
            let requested_count = events
                .iter()
                .filter(|event| event.kind == AgentRunEventKind::ModelRequested)
                .count();
            if requested_count != state.counters.provider_dispatches as usize
                || state.attempts.iter().any(|attempt| {
                    attempt.dispatched_at.is_some()
                        && exact_model_requested_event(expected_run_id, attempt, events).is_err()
                })
            {
                return Err(invalid());
            }
            let committed_attempts = state
                .attempts
                .iter()
                .filter(|attempt| attempt.state == ProviderAttemptState::Committed)
                .collect::<Vec<_>>();
            if events
                .iter()
                .filter(|event| event.kind == AgentRunEventKind::ModelResponded)
                .count()
                != committed_attempts.len()
            {
                return Err(invalid());
            }
            let committed_decisions = committed_attempts
                .iter()
                .map(|attempt| exact_committed_event(expected_run_id, attempt, events))
                .collect::<Result<Vec<_>, _>>()?;
            if !live_pending_matches_committed_decision(&state, &committed_decisions) {
                return Err(invalid());
            }
            let (pending_tool, sealed_approval_request, sealed_step) = match state.pending_tool {
                Some(pending) => {
                    let sealed_request = pending.approval_request.clone().into();
                    let sealed_step = pending.step_intent.as_step();
                    (
                        Some(PendingToolCall {
                            call_id: pending.call_id,
                            tool_name: pending.tool_name,
                            operation: pending.operation,
                            version: pending.version,
                            arguments: pending.arguments.as_value()?,
                            step_id: pending.step_id,
                            approval_request_id: Some(pending.approval_request_id),
                        }),
                        Some(sealed_request),
                        Some(sealed_step),
                    )
                }
                None => (None, None, None),
            };
            Ok(RuntimeApprovalContext {
                checkpoint_kind: checkpoint_kind.to_string(),
                run_id: state.run_id,
                agent_id: state.agent_id,
                required_approver_id: Some(
                    state
                        .policy_evidence
                        .resolved_bindings
                        .approver_principal_id
                        .as_uuid(),
                ),
                pending_tool,
                sealed_approval_request,
                sealed_step,
            })
        }
        _ => Err(invalid()),
    }
}

impl AgentRuntime {
    fn load_live_agent(&self, agent_id: Uuid) -> Result<AgentDefinition, AgentRuntimeError> {
        self.agent_store
            .load_agent_definition(&agent_id)
            .map_err(AgentRuntimeError::Store)?
            .ok_or(AgentRuntimeError::AgentNotFound(agent_id))
    }

    fn validate_live_agent(
        &self,
        agent: &AgentDefinition,
        setup: &LiveRunSetup,
        goal: &str,
    ) -> Result<(), AgentRuntimeError> {
        if agent.provider != LIVE_PROVIDER || agent.model != LIVE_MODEL {
            return Err(AgentRuntimeError::LiveSetup(
                "agent must use exact openai/gpt-5.6-sol provider binding".to_string(),
            ));
        }
        if agent.tools.len() != 1
            || agent.tools[0].operation != "release.publish"
            || agent.tools[0].version != "v2"
        {
            return Err(AgentRuntimeError::LiveSetup(
                "live agent must allow exactly release.publish::v2".to_string(),
            ));
        }
        if agent.limits.max_model_calls != 3
            || agent.limits.max_steps != 2
            || agent.limits.max_total_tokens != 10_000
            || agent.limits.max_duration_seconds != 300
            || agent.limits.max_output_tokens_per_call != 1024
            || agent.limits.max_cost_microusd != Some(120_000)
        {
            return Err(AgentRuntimeError::LiveSetup(
                "agent limits do not match frozen live limits".to_string(),
            ));
        }
        let expected_goal = format!("Publish synthetic edition {} to preview as {} using manifest {} and idempotency key {}.", setup.policy.binding_inputs.edition_id, setup.policy.binding_inputs.version_label, setup.policy.binding_inputs.manifest_digest, setup.policy.binding_inputs.idempotency_key);
        if goal != expected_goal {
            return Err(AgentRuntimeError::LiveSetup(
                "live goal is not the resolved sealed synthetic goal".to_string(),
            ));
        }
        if setup.policy.binding_inputs.agent_principal_id != self.identity.principal_id
            || agent.id
                != match setup.intent {
                    LiveRunIntent::Start { agent_id, .. } => agent_id,
                    LiveRunIntent::Resume { .. } => agent.id,
                }
        {
            return Err(AgentRuntimeError::LiveSetup(
                "agent identity binding does not match immutable definition".to_string(),
            ));
        }
        Ok(())
    }

    fn validate_live_setup(
        &self,
        setup: &LiveRunSetup,
        original_goal: &str,
        required_authority_until: DateTime<Utc>,
    ) -> Result<(), AgentRuntimeError> {
        let preflight = &setup.preflight_evidence;
        let preflight_record: PreflightEvidence = serde_json::from_value(preflight.clone())
            .map_err(|error| {
                AgentRuntimeError::LiveSetup(format!(
                    "preflight evidence is not the strict v1 record: {error}"
                ))
            })?;
        let preview_policy: TraceEvaluationPolicy = serde_json::from_str(PREVIEW_POLICY_SOURCE)
            .map_err(|error| {
                AgentRuntimeError::LiveSetup(format!("embedded preview policy is invalid: {error}"))
            })?;
        let preview_policy_digest = value_digest(&json!({
            "schema": "proof-agent-trace-policy/v1",
            "value": { "policy": preview_policy },
        }))?;
        if preflight_record.schema != "proof-release-manager-preflight-evidence/v1"
            || preflight_record.policy_path != "evals/release-manager-preview-v1.json"
            || preflight_record.policy_digest != preview_policy_digest
            || preflight_record.evaluator != "proof-agent-trace/v1"
            || preflight_record.outcome != "passed"
            || preflight_record.score_bps != 10_000
            || preflight_record.passed_checks != 10
            || preflight_record.total_checks != 10
        {
            return Err(AgentRuntimeError::LiveSetup(
                "preflight record is not the complete passed deterministic evidence".to_string(),
            ));
        }
        if wrapped_digest(
            "proof-release-manager-preflight-evidence-digest/v1",
            "evidence",
            preflight,
        )? != setup.preflight_evidence_digest
            || setup.policy.binding_inputs.preflight_evidence_digest
                != setup.preflight_evidence_digest
        {
            return Err(AgentRuntimeError::LiveSetup(
                "preflight evidence digest mismatch".to_string(),
            ));
        }
        if !self.exact_preflight_evidence(&preflight_record)? {
            return Err(AgentRuntimeError::LiveSetup(
                "preflight evidence does not match its persisted deterministic evaluation"
                    .to_string(),
            ));
        }
        let authority = &setup.authority;
        if authority.delegation.id != setup.policy.binding_inputs.delegation_id
            || authority.delegation_digest != setup.policy.binding_inputs.delegation_digest
            || authority.delegation_digest != delegation_digest(&authority.delegation)?
        {
            return Err(AgentRuntimeError::LiveSetup(
                "loaded delegation identity/digest mismatch".to_string(),
            ));
        }
        let grant = &authority.delegation;
        if grant.revoked
            || grant.recipient != self.identity.principal_id
            || grant.valid_until < required_authority_until
            || grant.scope.resource_scope.is_some()
            || grant.scope.allowed_operations.as_deref() != Some(&["release.publish".to_string()])
            || grant.scope.allowed_domains.as_deref() != Some(&["content".to_string()])
            || !authority
                .delegation_chain
                .grants
                .iter()
                .any(|candidate| candidate == grant)
        {
            return Err(AgentRuntimeError::LiveSetup(
                "delegation is not the exact singleton release.publish/content grant".to_string(),
            ));
        }
        if setup
            .policy
            .binding_inputs
            .idempotency_key
            .get_version_num()
            != 7
            || setup.policy.binding_inputs.delegation_id.get_version_num() != 7
            || setup.policy.binding_inputs.edition_id.is_nil()
            || setup.process_epoch_id.get_version_num() != 7
        {
            return Err(AgentRuntimeError::LiveSetup(
                "live idempotency, delegation, and process IDs must be UUIDv7".to_string(),
            ));
        }
        let bindings = &setup.policy.binding_inputs;
        let manifest_suffix = bindings
            .manifest_digest
            .strip_prefix("sha256:")
            .unwrap_or_default();
        let trusted_approver = self
            .approval_store
            .load_trusted_approver(&bindings.approver_principal_id)
            .map_err(AgentRuntimeError::Store)?;
        if bindings.agent_principal_id != self.identity.principal_id
            || bindings.approver_principal_id == self.identity.principal_id
            || !trusted_approver.is_some_and(|approver| {
                approver.id == bindings.approver_principal_id
                    && approver.kind == PrincipalKind::Human
            })
            || bindings.version_label != "2026.08.30-rc1"
            || manifest_suffix.len() != 64
            || !manifest_suffix
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(AgentRuntimeError::LiveSetup(
                "live bindings contain an invalid identity, approver, version, or manifest digest"
                    .to_string(),
            ));
        }
        authority
            .delegation_chain
            .validate(self.identity.principal_id, Utc::now())
            .map_err(|error| {
                AgentRuntimeError::LiveSetup(format!("delegation chain invalid: {error}"))
            })?;
        let embedded_template: Value =
            serde_json::from_str(LIVE_POLICY_SOURCE).map_err(|error| {
                AgentRuntimeError::LiveSetup(format!("embedded live policy is invalid: {error}"))
            })?;
        // The caller never gets to supply a policy variant.  Equality after
        // canonical parsing simultaneously pins all settings, nested schemas,
        // check/tamper IDs and rejects every unknown policy field.
        if canonicalize(&setup.policy.template)
            .map_err(|error| AgentRuntimeError::LiveSetup(error.to_string()))?
            != canonicalize(&embedded_template)
                .map_err(|error| AgentRuntimeError::LiveSetup(error.to_string()))?
        {
            return Err(AgentRuntimeError::LiveSetup(
                "live policy differs from the embedded frozen template".to_string(),
            ));
        }
        let template_digest = value_digest(&embedded_template)?;
        if template_digest != setup.policy.template_policy_digest {
            return Err(AgentRuntimeError::LiveSetup(
                "template policy digest mismatch".to_string(),
            ));
        }
        validate_policy_sets(&setup.policy)?;
        let declaration = embedded_template["tool"]["declaration"].clone();
        if setup.policy.instructions_digest
            != value_digest(&embedded_template["outbound_data"]["instructions"])?
            || setup.policy.initial_input_digest
                != value_digest(&Value::String(original_goal.to_string()))?
            || setup.policy.parameters_schema_digest
                != wrapped_digest(
                    "proof-openai-function-parameters-digest/v1",
                    "parameters",
                    &declaration["parameters"],
                )?
            || setup.policy.tool_declaration_digest
                != wrapped_digest(
                    "proof-openai-function-declaration-digest/v1",
                    "declaration",
                    &declaration,
                )?
            || setup.policy.tool_set_digest
                != wrapped_digest(
                    "proof-openai-tool-set-digest/v1",
                    "tools",
                    &json!([declaration]),
                )?
        {
            return Err(AgentRuntimeError::LiveSetup(
                "live static request digests do not match the frozen goal/template".to_string(),
            ));
        }
        Ok(())
    }

    fn exact_preflight_evidence(
        &self,
        preflight: &PreflightEvidence,
    ) -> Result<bool, AgentRuntimeError> {
        let run = self
            .run_store
            .load_agent_run(&preflight.run_id)
            .map_err(AgentRuntimeError::Store)?;
        let evaluations = self
            .run_store
            .list_agent_run_evaluations(&preflight.run_id)
            .map_err(AgentRuntimeError::Store)?;
        let matching = evaluations
            .iter()
            .filter(|evaluation| evaluation.id == preflight.evaluation_id)
            .collect::<Vec<_>>();
        Ok(run.as_ref().is_some_and(|run| {
            run.id == preflight.run_id && run.status == AgentRunStatus::Succeeded
        }) && matching.len() == 1
            && matching[0].run_id == preflight.run_id
            && matching[0].evaluator == preflight.evaluator
            && matching[0].outcome == AgentEvaluationOutcome::Passed
            && matching[0].score_bps == Some(10_000)
            && matching[0].created_at == preflight.evaluation_created_at
            && matching[0].metrics["passed_checks"] == 10
            && matching[0].metrics["total_checks"] == 10
            && matching[0].metrics["binding"]["policy_digest"] == json!(preflight.policy_digest)
            && matching[0].metrics["binding"]["trace_digest"] == json!(preflight.trace_digest))
    }

    fn validate_live_state_material(
        &self,
        state: &LiveRuntimeState,
        setup: &LiveRunSetup,
    ) -> Result<(), AgentRuntimeError> {
        let preflight: PreflightEvidence = serde_json::from_value(setup.preflight_evidence.clone())
            .map_err(|_| AgentRuntimeError::LiveSetup("resume preflight is invalid".to_string()))?;
        let strict_delegation = StrictDelegation::from(&setup.authority.delegation);
        let strict_chain = DelegationChainWire {
            root: setup.authority.delegation_chain.root,
            grants: setup
                .authority
                .delegation_chain
                .grants
                .iter()
                .map(StrictDelegation::from)
                .collect(),
        };
        let expected_bindings = resolved_live_bindings(
            state.run_id,
            state.agent_id,
            state.policy_evidence.resolved_bindings.process_epoch_id,
            &setup.policy.binding_inputs,
        );
        let expected_policy = resolve_live_policy(&setup.policy.template, &expected_bindings)?;
        let binding_value = serde_json::to_value(&expected_bindings)
            .map_err(|error| AgentRuntimeError::LiveSetup(error.to_string()))?;
        let expected_binding_digest = wrapped_digest(
            "proof-release-manager-live-bindings-digest/v1",
            "bindings",
            &binding_value,
        )?;
        if state.schema != "proof-agent-runtime-state/v2"
            || state.policy_evidence.preflight_evidence != preflight
            || state.policy_evidence.loaded_delegation != strict_delegation
            || state.policy_evidence.delegation_chain != strict_chain
            || state.policy_evidence.resolved_bindings != expected_bindings
            || state.policy_evidence.resolved_policy != expected_policy
            || state.policy_binding.preflight_evidence_digest != setup.preflight_evidence_digest
            || state.policy_binding.template_policy_digest != setup.policy.template_policy_digest
            || state.policy_binding.bindings_digest != expected_binding_digest
            || state.policy_binding.resolved_policy_digest
                != value_digest(&state.policy_evidence.resolved_policy)?
            || state.policy_binding.check_set_digest != setup.policy.check_set_digest
            || state.policy_binding.tamper_vector_set_digest
                != setup.policy.tamper_vector_set_digest
            || state.policy_binding.pricing_schedule_digest != setup.policy.pricing_schedule_digest
            || state.policy_binding.instructions_digest != setup.policy.instructions_digest
            || state.policy_binding.initial_input_digest != setup.policy.initial_input_digest
            || state.policy_binding.parameters_schema_digest
                != setup.policy.parameters_schema_digest
            || state.policy_binding.tool_declaration_digest != setup.policy.tool_declaration_digest
            || state.policy_binding.tool_set_digest != setup.policy.tool_set_digest
            || state.process_epoch_id == setup.process_epoch_id
        {
            return Err(AgentRuntimeError::LiveSetup(
                "resume material does not match sealed live checkpoint".to_string(),
            ));
        }
        Ok(())
    }

    fn live_request(
        &self,
        _agent: &AgentDefinition,
        policy: &LivePolicyMaterial,
        binding: &LiveResolvedBindings,
        previous: Option<String>,
    ) -> Result<LiveRequest, AgentRuntimeError> {
        let declaration_value = policy
            .template
            .pointer("/tool/declaration")
            .cloned()
            .ok_or_else(|| {
                AgentRuntimeError::LiveSetup("live policy lacks tool declaration".to_string())
            })?;
        let instructions = policy
            .template
            .pointer("/outbound_data/instructions")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                AgentRuntimeError::LiveSetup("live policy lacks instructions".to_string())
            })?
            .to_string();
        let declaration: LiveToolDeclaration = serde_json::from_value(declaration_value.clone())
            .map_err(|error| {
                AgentRuntimeError::LiveSetup(format!("live declaration is invalid: {error}"))
            })?;
        let parameters = serde_json::to_value(&declaration.parameters)
            .map_err(|error| AgentRuntimeError::LiveSetup(error.to_string()))?;
        let input = LiveModelInput::Goal { text: format!("Publish synthetic edition {} to preview as {} using manifest {} and idempotency key {}.", binding.edition_id, binding.version_label, binding.manifest_digest, binding.idempotency_key) };
        let request_value = json!({"model": LIVE_MODEL, "instructions": instructions, "input": match &input { LiveModelInput::Goal { text } => Value::String(text.clone()), _ => Value::Null }, "previous_response_id": previous, "tools": [declaration], "tool_choice": "auto", "parallel_tool_calls": false, "store": true, "stream": false, "background": false, "service_tier": LIVE_SERVICE_TIER, "max_output_tokens": 1024});
        let request = LiveRequest {
            endpoint: LIVE_ENDPOINT.to_string(),
            requested_model: LIVE_MODEL.to_string(),
            instructions,
            input,
            previous_response_id: previous,
            function_names: vec![LIVE_TOOL_NAME.to_string()],
            tool_declarations: vec![declaration],
            tool_choice: "auto".to_string(),
            service_tier: LIVE_SERVICE_TIER.to_string(),
            store: true,
            stream: false,
            background: false,
            parallel_tool_calls: false,
            max_output_tokens: 1024,
            request_body_digest: wrapped_digest(
                "proof-openai-responses-request-digest/v1",
                "request",
                &request_value,
            )?,
            instructions_digest: value_digest(&request_value["instructions"])?,
            input_digest: value_digest(&request_value["input"])?,
            parameters_schema_digest: wrapped_digest(
                "proof-openai-function-parameters-digest/v1",
                "parameters",
                &parameters,
            )?,
            tool_declaration_digest: wrapped_digest(
                "proof-openai-function-declaration-digest/v1",
                "declaration",
                &request_value["tools"][0],
            )?,
            tool_set_digest: wrapped_digest(
                "proof-openai-tool-set-digest/v1",
                "tools",
                &request_value["tools"],
            )?,
        };
        if request.instructions_digest != policy.instructions_digest
            || request.input_digest != policy.initial_input_digest
            || request.parameters_schema_digest != policy.parameters_schema_digest
            || request.tool_declaration_digest != policy.tool_declaration_digest
            || request.tool_set_digest != policy.tool_set_digest
        {
            return Err(AgentRuntimeError::LiveSetup(
                "sealed request/tool digest mismatch".to_string(),
            ));
        }
        Ok(request)
    }

    fn save_live_state(
        &self,
        run_id: Uuid,
        state: &LiveRuntimeState,
    ) -> Result<(), AgentRuntimeError> {
        self.save_live_state_checkpoint(run_id, state)
    }

    fn request_from_live_state(
        &self,
        state: &LiveRuntimeState,
    ) -> Result<LiveRequest, AgentRuntimeError> {
        let policy = &state.policy_evidence.resolved_policy;
        let declaration_value = policy
            .pointer("/tool/declaration")
            .cloned()
            .ok_or(AgentRuntimeError::InvalidCheckpoint(state.run_id))?;
        let declaration: LiveToolDeclaration = serde_json::from_value(declaration_value.clone())
            .map_err(|_| AgentRuntimeError::InvalidCheckpoint(state.run_id))?;
        let instructions = policy
            .pointer("/outbound_data/instructions")
            .and_then(Value::as_str)
            .ok_or(AgentRuntimeError::InvalidCheckpoint(state.run_id))?
            .to_string();
        let parameters = serde_json::to_value(&declaration.parameters)
            .map_err(|_| AgentRuntimeError::InvalidCheckpoint(state.run_id))?;
        let tool = AgentFunctionTool {
            name: LIVE_TOOL_NAME.to_string(),
            description: declaration.description.clone(),
            parameters,
            operation: "release.publish".to_string(),
            version: "v2".to_string(),
        };
        let model_request = ModelTurnRequest {
            model: LIVE_MODEL.to_string(),
            instructions: instructions.clone(),
            input: state.next_input.as_model_input()?,
            previous_response_id: state.previous_response_id.clone(),
            tools: vec![tool.clone()],
            max_output_tokens: 1024,
        };
        let body = crate::openai::request_body(&model_request)
            .map_err(|error| AgentRuntimeError::LiveSetup(error.to_string()))?;
        let request = LiveRequest {
            endpoint: LIVE_ENDPOINT.to_string(),
            requested_model: LIVE_MODEL.to_string(),
            instructions,
            input: state.next_input.clone(),
            previous_response_id: state.previous_response_id.clone(),
            function_names: vec![LIVE_TOOL_NAME.to_string()],
            tool_declarations: vec![declaration.clone()],
            tool_choice: "auto".to_string(),
            service_tier: LIVE_SERVICE_TIER.to_string(),
            store: true,
            stream: false,
            background: false,
            parallel_tool_calls: false,
            max_output_tokens: 1024,
            request_body_digest: wrapped_digest(
                "proof-openai-responses-request-digest/v1",
                "request",
                &body,
            )?,
            instructions_digest: state.policy_binding.instructions_digest,
            input_digest: value_digest(&body["input"])?,
            parameters_schema_digest: state.policy_binding.parameters_schema_digest,
            tool_declaration_digest: state.policy_binding.tool_declaration_digest,
            tool_set_digest: state.policy_binding.tool_set_digest,
        };
        if request.instructions_digest != value_digest(&body["instructions"])?
            || request.parameters_schema_digest
                != wrapped_digest(
                    "proof-openai-function-parameters-digest/v1",
                    "parameters",
                    &tool.parameters,
                )?
            || request.tool_declaration_digest
                != wrapped_digest(
                    "proof-openai-function-declaration-digest/v1",
                    "declaration",
                    &declaration_value,
                )?
            || request.tool_set_digest
                != wrapped_digest(
                    "proof-openai-tool-set-digest/v1",
                    "tools",
                    &json!([declaration]),
                )?
        {
            return Err(AgentRuntimeError::InvalidCheckpoint(state.run_id));
        }
        Ok(request)
    }
    fn save_live_state_checkpoint(
        &self,
        run_id: Uuid,
        state: &LiveRuntimeState,
    ) -> Result<(), AgentRuntimeError> {
        let sequence = next_sequence(
            self.run_store
                .list_agent_checkpoints(&run_id)
                .map_err(AgentRuntimeError::Store)?
                .last()
                .map(|checkpoint| checkpoint.sequence),
        )?;
        let checkpoint = AgentCheckpoint::create(
            run_id,
            sequence,
            json!({"kind": LIVE_RUNTIME_CHECKPOINT_KIND, "runtime": state}),
            Utc::now(),
        )?;
        self.run_store
            .save_agent_checkpoint(&checkpoint)
            .map_err(AgentRuntimeError::Store)
    }
    fn live_state(&self, run_id: Uuid) -> Result<LiveRuntimeState, AgentRuntimeError> {
        let checkpoints = self
            .run_store
            .list_agent_checkpoints(&run_id)
            .map_err(AgentRuntimeError::Store)?;
        validated_live_state_history(run_id, &checkpoints)
    }

    fn live_epoch_seen(
        &self,
        run_id: Uuid,
        process_epoch_id: Uuid,
    ) -> Result<bool, AgentRuntimeError> {
        let checkpoints = self
            .run_store
            .list_agent_checkpoints(&run_id)
            .map_err(AgentRuntimeError::Store)?;
        for checkpoint in checkpoints
            .iter()
            .filter(|checkpoint| checkpoint.state["kind"] == LIVE_RUNTIME_CHECKPOINT_KIND)
        {
            let state: LiveRuntimeState =
                serde_json::from_value(checkpoint.state["runtime"].clone())
                    .map_err(|_| AgentRuntimeError::InvalidCheckpoint(run_id))?;
            if state.process_epoch_id == process_epoch_id
                || state
                    .attempts
                    .iter()
                    .any(|attempt| attempt.process_epoch_id == process_epoch_id)
            {
                return Ok(true);
            }
        }
        Ok(self
            .agent_store
            .list_agent_run_events(&run_id)
            .map_err(AgentRuntimeError::Store)?
            .iter()
            .any(|event| event.data["process_epoch_id"] == json!(process_epoch_id)))
    }
    fn reread_live_state(
        &self,
        run_id: Uuid,
        expected: &LiveRuntimeState,
    ) -> Result<(), AgentRuntimeError> {
        if self.live_state(run_id)? == *expected {
            Ok(())
        } else {
            Err(AgentRuntimeError::LiveSetup(
                "live checkpoint reread mismatch".to_string(),
            ))
        }
    }
    fn reread_live_event(
        &self,
        run_id: Uuid,
        kind: AgentRunEventKind,
        data: &Value,
    ) -> Result<(), AgentRuntimeError> {
        let event = self
            .agent_store
            .list_agent_run_events(&run_id)
            .map_err(AgentRuntimeError::Store)?
            .into_iter()
            .last()
            .ok_or_else(|| AgentRuntimeError::LiveSetup("missing live event".to_string()))?;
        if event.kind == kind && event.data == *data {
            Ok(())
        } else {
            Err(AgentRuntimeError::LiveSetup(
                "live event reread mismatch".to_string(),
            ))
        }
    }
}

fn generic_initial_state(run: &AgentRun, agent: &AgentDefinition) -> AgentRuntimeState {
    AgentRuntimeState {
        agent_id: agent.id,
        started_at: run.created_at,
        previous_response_id: None,
        next_input: ModelInput::Goal {
            text: run.goal.clone(),
        },
        pending_tool: None,
        model_calls: 0,
        tool_attempts: 0,
        input_tokens: 0,
        output_tokens: 0,
        total_tokens: 0,
        cost_microusd: Some(0),
        final_output: None,
        terminal_error: None,
    }
}

fn generic_started_event(agent: &AgentDefinition, run: &AgentRun) -> Value {
    json!({"agent_id": agent.id, "goal": run.goal})
}

fn validate_generic_initial_checkpoint(
    run: &AgentRun,
    expected_state: &AgentRuntimeState,
    checkpoint: &AgentCheckpoint,
) -> Result<(), AgentRuntimeError> {
    let expected_value = json!({"kind": RUNTIME_CHECKPOINT_KIND, "runtime": expected_state});
    let rebuilt = AgentCheckpoint::create(
        run.id,
        checkpoint.sequence,
        expected_value.clone(),
        checkpoint.created_at,
    )?;
    if checkpoint.run_id != run.id
        || checkpoint.id.get_version_num() != 7
        || checkpoint.sequence != 0
        || checkpoint.state != expected_value
        || checkpoint.state_digest != rebuilt.state_digest
        || checkpoint.created_at < run.created_at
        || checkpoint.created_at > Utc::now()
    {
        return Err(AgentRuntimeError::InvalidCheckpoint(run.id));
    }
    Ok(())
}

fn validate_generic_started_event(
    run_id: Uuid,
    expected_data: &Value,
    checkpoint: Option<&AgentCheckpoint>,
    event: &AgentRunEvent,
) -> Result<(), AgentRuntimeError> {
    validate_exact_event_record(run_id, AgentRunEventKind::Started, expected_data, event)?;
    if event.sequence != 0
        || checkpoint.is_none_or(|checkpoint| event.created_at < checkpoint.created_at)
        || event.created_at > Utc::now()
    {
        return Err(AgentRuntimeError::InvalidCheckpoint(run_id));
    }
    Ok(())
}

fn validate_generic_evidence_envelopes(
    run_id: Uuid,
    checkpoints: &[AgentCheckpoint],
    events: &[AgentRunEvent],
) -> Result<(), AgentRuntimeError> {
    let observed_at = Utc::now();
    let mut prior_checkpoint_at = None;
    for (index, checkpoint) in checkpoints.iter().enumerate() {
        let sequence = u32::try_from(index).map_err(|_| {
            AgentRuntimeError::InconsistentState(
                "generic checkpoint sequence exceeds u32".to_string(),
            )
        })?;
        let rebuilt = AgentCheckpoint::create(
            run_id,
            checkpoint.sequence,
            checkpoint.state.clone(),
            checkpoint.created_at,
        )?;
        if checkpoint.run_id != run_id
            || checkpoint.id.get_version_num() != 7
            || checkpoint.sequence != sequence
            || checkpoint.state["kind"] != RUNTIME_CHECKPOINT_KIND
            || checkpoint.state_digest != rebuilt.state_digest
            || checkpoint.created_at > observed_at
            || prior_checkpoint_at.is_some_and(|prior| checkpoint.created_at < prior)
        {
            return Err(AgentRuntimeError::InvalidCheckpoint(run_id));
        }
        prior_checkpoint_at = Some(checkpoint.created_at);
    }

    let mut prior_event_at = None;
    for (index, event) in events.iter().enumerate() {
        let sequence = u32::try_from(index).map_err(|_| {
            AgentRuntimeError::InconsistentState("generic event sequence exceeds u32".to_string())
        })?;
        let rebuilt = AgentRunEvent::create(
            run_id,
            event.sequence,
            event.kind,
            event.data.clone(),
            event.created_at,
        )
        .map_err(|_| AgentRuntimeError::InvalidCheckpoint(run_id))?;
        if event.run_id != run_id
            || event.id.get_version_num() != 7
            || event.sequence != sequence
            || event.data_digest != rebuilt.data_digest
            || event.created_at > observed_at
            || prior_event_at.is_some_and(|prior| event.created_at < prior)
        {
            return Err(AgentRuntimeError::InvalidCheckpoint(run_id));
        }
        prior_event_at = Some(event.created_at);
    }
    Ok(())
}

fn runtime_instructions(instructions: &str) -> String {
    format!(
        "{instructions}\n\nUse only the supplied Proof tools. Treat tool errors as authoritative. Stop when the goal is complete."
    )
}

fn tool_name(entry: &RegistryEntry) -> String {
    format!(
        "proof_{}_{}_{}",
        entry.domain,
        entry.version,
        entry.operation.replace('.', "_")
    )
}

fn apply_usage(state: &mut AgentRuntimeState, usage: ModelUsage) -> Result<(), String> {
    state.input_tokens = state
        .input_tokens
        .checked_add(usage.input_tokens)
        .ok_or_else(|| "input token counter overflow".to_string())?;
    state.output_tokens = state
        .output_tokens
        .checked_add(usage.output_tokens)
        .ok_or_else(|| "output token counter overflow".to_string())?;
    state.total_tokens = state
        .total_tokens
        .checked_add(usage.total_tokens)
        .ok_or_else(|| "total token counter overflow".to_string())?;
    state.cost_microusd = match (state.cost_microusd, usage.cost_microusd) {
        (Some(total), Some(increment)) => Some(
            total
                .checked_add(increment)
                .ok_or_else(|| "cost counter overflow".to_string())?,
        ),
        _ => None,
    };
    Ok(())
}

fn elapsed_seconds(started_at: DateTime<Utc>, now: DateTime<Utc>) -> u64 {
    now.signed_duration_since(started_at).num_seconds().max(0) as u64
}

fn run_deadline(started_at: DateTime<Utc>, max_duration_seconds: u64) -> DateTime<Utc> {
    let seconds = i64::try_from(max_duration_seconds).unwrap_or(i64::MAX);
    started_at
        .checked_add_signed(Duration::seconds(seconds))
        .unwrap_or(DateTime::<Utc>::MAX_UTC)
}

fn duration_budget_error(
    agent: &AgentDefinition,
    state: &AgentRuntimeState,
    now: DateTime<Utc>,
) -> Option<String> {
    if now >= run_deadline(state.started_at, agent.limits.max_duration_seconds) {
        Some(format!(
            "duration budget exceeded: maximum {} seconds",
            agent.limits.max_duration_seconds
        ))
    } else {
        None
    }
}

fn legacy_budget_failure(error: &str) -> bool {
    error.contains("budget exceeded")
        || error.starts_with("cost budget configured")
        || error.ends_with("token counter overflow")
        || error == "cost counter overflow"
}

fn metrics(state: &AgentRuntimeState) -> Value {
    json!({
        "model_calls": state.model_calls,
        "tool_attempts": state.tool_attempts,
        "input_tokens": state.input_tokens,
        "output_tokens": state.output_tokens,
        "total_tokens": state.total_tokens,
        "cost_microusd": state.cost_microusd,
        "duration_seconds": elapsed_seconds(state.started_at, Utc::now()),
    })
}

fn next_sequence(current: Option<u32>) -> Result<u32, AgentRuntimeError> {
    match current {
        Some(sequence) => sequence.checked_add(1).ok_or_else(|| {
            AgentRuntimeError::InconsistentState("sequence counter overflow".to_string())
        }),
        None => Ok(0),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Mutex;

    use proof_kernel::{
        create_proof, generate_keypair, generate_keypair_for, principal_from_keypair, AgentLimits,
        AgentTool, ApprovalStore, Delegation, Governance, OperationHandler, Principal,
        PrincipalKind, RecordingAgentRunStore, RecordingAgentStore, RecordingApprovalStore,
        RecordingStore, RegistryEntry, SignedApprovalDecision, VersionStatus,
    };

    use super::*;
    use crate::model::{ModelGatewayError, ModelTurn};

    struct ScriptedGateway {
        turns: Mutex<VecDeque<Result<ModelTurn, ModelGatewayError>>>,
        requests: Mutex<Vec<ModelTurnRequest>>,
    }

    #[derive(Clone)]
    enum LiveGatewayAction {
        Error(ModelGatewayError),
        Turn(ModelTurn),
        Tool(ReleasePublishArguments),
        FinishFromToolOutput,
        FinishWithOutput(String),
        PanicAfterBarrier,
    }

    struct LiveGatewaySpy {
        actions: Mutex<VecDeque<LiveGatewayAction>>,
        requests: Mutex<Vec<ModelTurnRequest>>,
        sends: AtomicUsize,
        run_store: Arc<RecordingAgentRunStore>,
        agent_store: Arc<RecordingAgentStore>,
        factory_context: Mutex<Option<ModelGatewayFactoryContext>>,
    }

    impl LiveGatewaySpy {
        fn new(
            actions: Vec<LiveGatewayAction>,
            run_store: Arc<RecordingAgentRunStore>,
            agent_store: Arc<RecordingAgentStore>,
        ) -> Self {
            Self {
                actions: Mutex::new(actions.into()),
                requests: Mutex::new(Vec::new()),
                sends: AtomicUsize::new(0),
                run_store,
                agent_store,
                factory_context: Mutex::new(None),
            }
        }

        fn successful_turn(response_id: &str, decision: ModelDecision) -> ModelTurn {
            let body = json!({
                "id": response_id,
                "model": LIVE_MODEL,
                "status": "completed",
                "decision": decision,
                "usage": {"input_tokens": 30, "output_tokens": 10, "total_tokens": 40},
            });
            ModelTurn {
                response_id: response_id.to_string(),
                returned_model: Some(LIVE_MODEL.to_string()),
                response_body_digest: Some(value_digest(&body).unwrap()),
                decision,
                usage: ModelUsage {
                    input_tokens: 30,
                    output_tokens: 10,
                    total_tokens: 40,
                    cost_microusd: None,
                },
            }
        }
    }

    impl ModelGateway for LiveGatewaySpy {
        fn provider(&self) -> &str {
            LIVE_PROVIDER
        }

        fn complete(&self, request: &ModelTurnRequest) -> Result<ModelTurn, ModelGatewayError> {
            let context = self
                .factory_context
                .lock()
                .unwrap()
                .clone()
                .expect("factory context precedes send");
            let checkpoints = self
                .run_store
                .list_agent_checkpoints(&context.run_id)
                .unwrap();
            let runtime = checkpoints.last().unwrap().state["runtime"].clone();
            assert_eq!(
                runtime["attempts"].as_array().unwrap().last().unwrap()["state"],
                "dispatching"
            );
            assert_eq!(
                runtime["attempts"].as_array().unwrap().last().unwrap()["attempt_id"],
                json!(context.attempt_id)
            );
            let events = self
                .agent_store
                .list_agent_run_events(&context.run_id)
                .unwrap();
            let requested = events
                .iter()
                .rev()
                .find(|event| event.kind == AgentRunEventKind::ModelRequested)
                .expect("model_requested event precedes send");
            assert_eq!(requested.data["attempt_id"], json!(context.attempt_id));
            assert_eq!(
                requested.data["request_body_digest"],
                json!(context.request_body_digest)
            );
            self.sends.fetch_add(1, Ordering::SeqCst);
            self.requests.lock().unwrap().push(request.clone());
            match self
                .actions
                .lock()
                .unwrap()
                .pop_front()
                .expect("live action")
            {
                LiveGatewayAction::Error(error) => Err(error),
                LiveGatewayAction::Turn(turn) => Ok(turn),
                LiveGatewayAction::Tool(arguments) => Ok(Self::successful_turn(
                    "resp_tool",
                    ModelDecision::ToolCall {
                        call_id: "call_publish".to_string(),
                        name: LIVE_TOOL_NAME.to_string(),
                        arguments: arguments.as_value().unwrap(),
                    },
                )),
                LiveGatewayAction::FinishFromToolOutput => {
                    let ModelInput::ToolOutput { output, .. } = &request.input else {
                        panic!("finish action requires committed tool output")
                    };
                    let result = &output["result"];
                    let report = format!(
                        "publication_id={} edition_id={} environment={} version_label={} manifest_digest={} relative_path={} artifact_digest={} proof_id={}",
                        result["data"]["publication_id"].as_str().unwrap(),
                        result["data"]["edition_id"].as_str().unwrap(),
                        result["data"]["environment"].as_str().unwrap(),
                        result["data"]["version_label"].as_str().unwrap(),
                        result["data"]["manifest_digest"].as_str().unwrap(),
                        result["data"]["artifact"]["relative_path"].as_str().unwrap(),
                        result["data"]["artifact"]["digest"].as_str().unwrap(),
                        output["proof_id"].as_str().unwrap(),
                    );
                    Ok(Self::successful_turn(
                        "resp_finish",
                        ModelDecision::Finish { output: report },
                    ))
                }
                LiveGatewayAction::FinishWithOutput(output) => Ok(Self::successful_turn(
                    "resp_finish",
                    ModelDecision::Finish { output },
                )),
                LiveGatewayAction::PanicAfterBarrier => {
                    panic!("simulated process death after durable dispatch barrier")
                }
            }
        }
    }

    struct LiveFactorySpy {
        creates: AtomicUsize,
        contexts: Mutex<Vec<ModelGatewayFactoryContext>>,
        gateway: Arc<LiveGatewaySpy>,
    }

    impl ModelGatewayFactory for LiveFactorySpy {
        fn create(
            &self,
            context: &ModelGatewayFactoryContext,
        ) -> Result<Arc<dyn ModelGateway>, ModelGatewayFactoryError> {
            self.creates.fetch_add(1, Ordering::SeqCst);
            self.contexts.lock().unwrap().push(context.clone());
            *self.gateway.factory_context.lock().unwrap() = Some(context.clone());
            Ok(self.gateway.clone())
        }
    }

    struct RejectCommittedRunStore {
        inner: Arc<RecordingAgentRunStore>,
    }

    #[derive(Clone, Copy)]
    enum LiveRunFault {
        DispatchSave,
        DispatchRead,
        PendingCheckpoint,
        FinalOutputCheckpoint,
        RetryPrepared,
        FirstStepSave,
        SucceededStepSave,
        ContinuationCheckpoint,
        TerminalVerificationRead,
        ResumeEpochSave,
        EvaluationSave,
    }

    struct FaultingLiveRunStore {
        inner: Arc<RecordingAgentRunStore>,
        fault: LiveRunFault,
        armed: AtomicBool,
        dispatch_saved: AtomicBool,
        terminal_saved: AtomicBool,
    }

    impl FaultingLiveRunStore {
        fn new(inner: Arc<RecordingAgentRunStore>, fault: LiveRunFault) -> Self {
            Self {
                inner,
                fault,
                armed: AtomicBool::new(true),
                dispatch_saved: AtomicBool::new(false),
                terminal_saved: AtomicBool::new(false),
            }
        }
    }

    impl AgentRunStore for FaultingLiveRunStore {
        fn save_agent_run(&self, run: &AgentRun) -> Result<(), String> {
            self.inner.save_agent_run(run)
        }
        fn load_agent_run(&self, run_id: &Uuid) -> Result<Option<AgentRun>, String> {
            self.inner.load_agent_run(run_id)
        }
        fn list_agent_runs(&self) -> Result<Vec<AgentRun>, String> {
            self.inner.list_agent_runs()
        }
        fn save_agent_run_step(&self, step: &AgentRunStep) -> Result<(), String> {
            let reject = match self.fault {
                LiveRunFault::FirstStepSave => true,
                LiveRunFault::SucceededStepSave => step.status == AgentRunStepStatus::Succeeded,
                _ => false,
            };
            if reject && self.armed.swap(false, Ordering::SeqCst) {
                return Err("injected live step save fault".to_string());
            }
            self.inner.save_agent_run_step(step)
        }
        fn load_agent_run_step(&self, step_id: &Uuid) -> Result<Option<AgentRunStep>, String> {
            self.inner.load_agent_run_step(step_id)
        }
        fn list_agent_run_steps(&self, run_id: &Uuid) -> Result<Vec<AgentRunStep>, String> {
            if matches!(self.fault, LiveRunFault::TerminalVerificationRead)
                && self.terminal_saved.load(Ordering::SeqCst)
                && self.armed.swap(false, Ordering::SeqCst)
            {
                return Err("injected terminal verification read fault".to_string());
            }
            self.inner.list_agent_run_steps(run_id)
        }
        fn find_agent_run_step_by_approval(
            &self,
            request_id: &Uuid,
        ) -> Result<Option<AgentRunStep>, String> {
            self.inner.find_agent_run_step_by_approval(request_id)
        }
        fn save_agent_checkpoint(&self, checkpoint: &AgentCheckpoint) -> Result<(), String> {
            let runtime = &checkpoint.state["runtime"];
            let last = runtime["attempts"]
                .as_array()
                .and_then(|items| items.last());
            let reject = match self.fault {
                LiveRunFault::DispatchSave => {
                    last.is_some_and(|attempt| attempt["state"] == "dispatching")
                }
                LiveRunFault::PendingCheckpoint => !runtime["pending_tool"].is_null(),
                LiveRunFault::FinalOutputCheckpoint => !runtime["final_output"].is_null(),
                LiveRunFault::RetryPrepared => {
                    runtime["attempts"].as_array().is_some_and(|items| {
                        items.len() == 2
                            && items
                                .last()
                                .is_some_and(|attempt| attempt["state"] == "prepared")
                    })
                }
                LiveRunFault::ContinuationCheckpoint => {
                    runtime["pending_tool"].is_null()
                        && runtime["counters"]["successful_publication_mutations"] == 1
                        && runtime["final_output"].is_null()
                }
                LiveRunFault::ResumeEpochSave => {
                    !runtime["pending_tool"].is_null()
                        && self
                            .inner
                            .list_agent_checkpoints(&checkpoint.run_id)
                            .ok()
                            .and_then(|checkpoints| checkpoints.last().cloned())
                            .is_some_and(|previous| {
                                previous.state["runtime"]["process_epoch_id"]
                                    != runtime["process_epoch_id"]
                            })
                }
                _ => false,
            };
            if reject && self.armed.swap(false, Ordering::SeqCst) {
                return Err("injected live checkpoint save fault".to_string());
            }
            let result = self.inner.save_agent_checkpoint(checkpoint);
            if matches!(self.fault, LiveRunFault::DispatchRead)
                && last.is_some_and(|attempt| attempt["state"] == "dispatching")
            {
                self.dispatch_saved.store(true, Ordering::SeqCst);
            }
            if matches!(self.fault, LiveRunFault::TerminalVerificationRead)
                && !runtime["final_output"].is_null()
            {
                self.terminal_saved.store(true, Ordering::SeqCst);
            }
            result
        }
        fn list_agent_checkpoints(&self, run_id: &Uuid) -> Result<Vec<AgentCheckpoint>, String> {
            if matches!(self.fault, LiveRunFault::DispatchRead)
                && self.dispatch_saved.load(Ordering::SeqCst)
                && self.armed.swap(false, Ordering::SeqCst)
            {
                return Err("injected dispatch checkpoint reread fault".to_string());
            }
            self.inner.list_agent_checkpoints(run_id)
        }
        fn save_agent_run_evaluation(&self, row: &AgentRunEvaluation) -> Result<(), String> {
            if matches!(self.fault, LiveRunFault::EvaluationSave)
                && self.armed.swap(false, Ordering::SeqCst)
            {
                return Err("injected live evaluation save fault".to_string());
            }
            self.inner.save_agent_run_evaluation(row)
        }
        fn list_agent_run_evaluations(
            &self,
            run_id: &Uuid,
        ) -> Result<Vec<AgentRunEvaluation>, String> {
            self.inner.list_agent_run_evaluations(run_id)
        }
    }

    #[derive(Clone, Copy)]
    enum LiveAgentFault {
        RequestedSave,
        RequestedRead,
        ToolRequestedSave,
        ApprovalRequiredSave,
        TerminalSave,
    }

    struct FaultingLiveAgentStore {
        inner: Arc<RecordingAgentStore>,
        fault: LiveAgentFault,
        armed: AtomicBool,
        requested_saved: AtomicBool,
    }

    impl AgentStore for FaultingLiveAgentStore {
        fn save_agent_definition(&self, agent: &AgentDefinition) -> Result<(), String> {
            self.inner.save_agent_definition(agent)
        }
        fn load_agent_definition(
            &self,
            agent_id: &Uuid,
        ) -> Result<Option<AgentDefinition>, String> {
            self.inner.load_agent_definition(agent_id)
        }
        fn list_agent_definitions(&self) -> Result<Vec<AgentDefinition>, String> {
            self.inner.list_agent_definitions()
        }
        fn save_agent_run_event(&self, event: &AgentRunEvent) -> Result<(), String> {
            if ((event.kind == AgentRunEventKind::ToolRequested
                && matches!(self.fault, LiveAgentFault::ToolRequestedSave))
                || (event.kind == AgentRunEventKind::ApprovalRequired
                    && matches!(self.fault, LiveAgentFault::ApprovalRequiredSave)))
                && self.armed.swap(false, Ordering::SeqCst)
            {
                return Err("injected approval chronology event save fault".to_string());
            }
            if matches!(
                event.kind,
                AgentRunEventKind::Completed
                    | AgentRunEventKind::Failed
                    | AgentRunEventKind::BudgetExceeded
            ) && matches!(self.fault, LiveAgentFault::TerminalSave)
                && self.armed.swap(false, Ordering::SeqCst)
            {
                return Err("injected terminal event save fault".to_string());
            }
            if event.kind == AgentRunEventKind::ModelRequested
                && matches!(self.fault, LiveAgentFault::RequestedSave)
                && self.armed.swap(false, Ordering::SeqCst)
            {
                return Err("injected model_requested save fault".to_string());
            }
            let result = self.inner.save_agent_run_event(event);
            if event.kind == AgentRunEventKind::ModelRequested
                && matches!(self.fault, LiveAgentFault::RequestedRead)
            {
                self.requested_saved.store(true, Ordering::SeqCst);
            }
            result
        }
        fn list_agent_run_events(&self, run_id: &Uuid) -> Result<Vec<AgentRunEvent>, String> {
            if matches!(self.fault, LiveAgentFault::RequestedRead)
                && self.requested_saved.load(Ordering::SeqCst)
                && self.armed.swap(false, Ordering::SeqCst)
            {
                return Err("injected model_requested reread fault".to_string());
            }
            self.inner.list_agent_run_events(run_id)
        }
    }

    struct FailAfterApprovalResumedAgentStore {
        inner: Arc<RecordingAgentStore>,
        armed: AtomicBool,
        resumed_saved: AtomicBool,
    }

    impl AgentStore for FailAfterApprovalResumedAgentStore {
        fn save_agent_definition(&self, agent: &AgentDefinition) -> Result<(), String> {
            self.inner.save_agent_definition(agent)
        }
        fn load_agent_definition(
            &self,
            agent_id: &Uuid,
        ) -> Result<Option<AgentDefinition>, String> {
            self.inner.load_agent_definition(agent_id)
        }
        fn list_agent_definitions(&self) -> Result<Vec<AgentDefinition>, String> {
            self.inner.list_agent_definitions()
        }
        fn save_agent_run_event(&self, event: &AgentRunEvent) -> Result<(), String> {
            let result = self.inner.save_agent_run_event(event);
            if event.kind == AgentRunEventKind::ApprovalResumed {
                self.resumed_saved.store(true, Ordering::SeqCst);
            }
            result
        }
        fn list_agent_run_events(&self, run_id: &Uuid) -> Result<Vec<AgentRunEvent>, String> {
            if self.resumed_saved.load(Ordering::SeqCst) && self.armed.swap(false, Ordering::SeqCst)
            {
                return Err("injected crash after approval_resumed append".to_string());
            }
            self.inner.list_agent_run_events(run_id)
        }
    }

    struct WaitEpochApprovalEventStore {
        inner: Arc<RecordingAgentStore>,
        wait_epoch: Uuid,
    }

    impl AgentStore for WaitEpochApprovalEventStore {
        fn save_agent_definition(&self, agent: &AgentDefinition) -> Result<(), String> {
            self.inner.save_agent_definition(agent)
        }
        fn load_agent_definition(
            &self,
            agent_id: &Uuid,
        ) -> Result<Option<AgentDefinition>, String> {
            self.inner.load_agent_definition(agent_id)
        }
        fn list_agent_definitions(&self) -> Result<Vec<AgentDefinition>, String> {
            self.inner.list_agent_definitions()
        }
        fn save_agent_run_event(&self, event: &AgentRunEvent) -> Result<(), String> {
            if event.kind != AgentRunEventKind::ApprovalResumed {
                return self.inner.save_agent_run_event(event);
            }
            let mut substituted = event.clone();
            substituted.data["process_epoch_id"] = json!(self.wait_epoch);
            substituted.data_digest = AgentRunEvent::create(
                substituted.run_id,
                substituted.sequence,
                substituted.kind,
                substituted.data.clone(),
                substituted.created_at,
            )
            .map_err(|error| error.to_string())?
            .data_digest;
            self.inner.save_agent_run_event(&substituted)
        }
        fn list_agent_run_events(&self, run_id: &Uuid) -> Result<Vec<AgentRunEvent>, String> {
            self.inner.list_agent_run_events(run_id)
        }
    }

    struct TamperingFailureEventStore {
        inner: Arc<RecordingAgentStore>,
        mutation: usize,
    }

    impl AgentStore for TamperingFailureEventStore {
        fn save_agent_definition(&self, agent: &AgentDefinition) -> Result<(), String> {
            self.inner.save_agent_definition(agent)
        }
        fn load_agent_definition(
            &self,
            agent_id: &Uuid,
        ) -> Result<Option<AgentDefinition>, String> {
            self.inner.load_agent_definition(agent_id)
        }
        fn list_agent_definitions(&self) -> Result<Vec<AgentDefinition>, String> {
            self.inner.list_agent_definitions()
        }
        fn save_agent_run_event(&self, event: &AgentRunEvent) -> Result<(), String> {
            self.inner.save_agent_run_event(event)
        }
        fn list_agent_run_events(&self, run_id: &Uuid) -> Result<Vec<AgentRunEvent>, String> {
            let mut events = self.inner.list_agent_run_events(run_id)?;
            let index = events
                .iter()
                .position(|event| {
                    matches!(
                        event.kind,
                        AgentRunEventKind::Failed | AgentRunEventKind::BudgetExceeded
                    )
                })
                .expect("fixture has failure terminal event");
            match self.mutation {
                0 => {
                    events[index].data["error"] = json!("substituted failure");
                    events[index].data_digest = AgentRunEvent::create(
                        events[index].run_id,
                        events[index].sequence,
                        events[index].kind,
                        events[index].data.clone(),
                        events[index].created_at,
                    )
                    .unwrap()
                    .data_digest;
                }
                1 => {
                    let duplicate = events[index].clone();
                    events.push(duplicate);
                }
                2 => {
                    let both = AgentRunEvent::create(
                        events[index].run_id,
                        events.iter().map(|event| event.sequence).max().unwrap() + 1,
                        AgentRunEventKind::Failed,
                        events[index].data.clone(),
                        events[index].created_at,
                    )
                    .unwrap();
                    events.push(both);
                }
                _ => unreachable!(),
            }
            Ok(events)
        }
    }

    struct MutatingRespondedAgentStore {
        inner: Arc<RecordingAgentStore>,
        mutation: usize,
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum LiveEventTamperTarget {
        ModelRequested,
        ApprovalResumed,
        Completed,
    }

    struct TamperingLiveEventStore {
        inner: Arc<RecordingAgentStore>,
        target: LiveEventTamperTarget,
        mutation: usize,
    }

    impl TamperingLiveEventStore {
        fn kind(&self) -> AgentRunEventKind {
            match self.target {
                LiveEventTamperTarget::ModelRequested => AgentRunEventKind::ModelRequested,
                LiveEventTamperTarget::ApprovalResumed => AgentRunEventKind::ApprovalResumed,
                LiveEventTamperTarget::Completed => AgentRunEventKind::Completed,
            }
        }

        fn substituted(&self, event: &AgentRunEvent) -> AgentRunEvent {
            let mut substituted = event.clone();
            match self.target {
                LiveEventTamperTarget::ModelRequested => match self.mutation {
                    0 => substituted.data["state"] = json!("prepared"),
                    1 => substituted.data["unknown"] = json!(true),
                    _ => unreachable!(),
                },
                LiveEventTamperTarget::ApprovalResumed => match self.mutation {
                    0 => substituted.data["step_id"] = json!(Uuid::now_v7()),
                    1 => substituted.data["process_epoch_id"] = json!(Uuid::now_v7()),
                    2 => substituted.data["unknown"] = json!(true),
                    _ => unreachable!(),
                },
                LiveEventTamperTarget::Completed => match self.mutation {
                    0 => substituted.data["output"] = json!("substituted terminal output"),
                    1 => substituted.data["unknown"] = json!(true),
                    _ => unreachable!(),
                },
            }
            substituted.data_digest = AgentRunEvent::create(
                substituted.run_id,
                substituted.sequence,
                substituted.kind,
                substituted.data.clone(),
                substituted.created_at,
            )
            .unwrap()
            .data_digest;
            substituted
        }
    }

    impl AgentStore for TamperingLiveEventStore {
        fn save_agent_definition(&self, agent: &AgentDefinition) -> Result<(), String> {
            self.inner.save_agent_definition(agent)
        }
        fn load_agent_definition(
            &self,
            agent_id: &Uuid,
        ) -> Result<Option<AgentDefinition>, String> {
            self.inner.load_agent_definition(agent_id)
        }
        fn list_agent_definitions(&self) -> Result<Vec<AgentDefinition>, String> {
            self.inner.list_agent_definitions()
        }
        fn save_agent_run_event(&self, event: &AgentRunEvent) -> Result<(), String> {
            if event.kind != self.kind() {
                return self.inner.save_agent_run_event(event);
            }
            let duplicate_index = match self.target {
                LiveEventTamperTarget::ModelRequested => 2,
                LiveEventTamperTarget::ApprovalResumed => 3,
                LiveEventTamperTarget::Completed => 3,
            };
            let removed_index = match self.target {
                LiveEventTamperTarget::Completed => Some(2),
                _ => None,
            };
            if removed_index == Some(self.mutation) {
                return Ok(());
            }
            if self.mutation == duplicate_index {
                self.inner.save_agent_run_event(event)?;
                let duplicate = AgentRunEvent::create(
                    event.run_id,
                    event.sequence + 1,
                    event.kind,
                    event.data.clone(),
                    event.created_at,
                )
                .map_err(|error| error.to_string())?;
                return self.inner.save_agent_run_event(&duplicate);
            }
            self.inner.save_agent_run_event(&self.substituted(event))
        }
        fn list_agent_run_events(&self, run_id: &Uuid) -> Result<Vec<AgentRunEvent>, String> {
            self.inner.list_agent_run_events(run_id)
        }
    }

    impl AgentStore for MutatingRespondedAgentStore {
        fn save_agent_definition(&self, agent: &AgentDefinition) -> Result<(), String> {
            self.inner.save_agent_definition(agent)
        }
        fn load_agent_definition(
            &self,
            agent_id: &Uuid,
        ) -> Result<Option<AgentDefinition>, String> {
            self.inner.load_agent_definition(agent_id)
        }
        fn list_agent_definitions(&self) -> Result<Vec<AgentDefinition>, String> {
            self.inner.list_agent_definitions()
        }
        fn save_agent_run_event(&self, event: &AgentRunEvent) -> Result<(), String> {
            self.inner.save_agent_run_event(event)
        }
        fn list_agent_run_events(&self, run_id: &Uuid) -> Result<Vec<AgentRunEvent>, String> {
            let mut events = self.inner.list_agent_run_events(run_id)?;
            if let Some(event) = events
                .iter_mut()
                .find(|event| event.kind == AgentRunEventKind::ModelResponded)
            {
                match self.mutation {
                    0 => event.data["usage"]["input_tokens"] = json!(31),
                    1 => event.data["requested_model"] = json!("substituted"),
                    2 => event.data["returned_model"] = json!("substituted"),
                    3 => event.data["provider_cost_microusd"] = json!(1),
                    4 => event.data["calculated_cost_microusd"] = json!(999),
                    5 => event.data["cumulative_calculated_cost_microusd"] = json!(999),
                    6 => event.data["pricing_schedule_id"] = json!("substituted"),
                    7 => {
                        event.data["request_body_digest"] =
                            json!(value_digest(&json!("request substitution")).unwrap())
                    }
                    8 => {
                        event.data["response_body_digest"] =
                            json!(value_digest(&json!("response substitution")).unwrap())
                    }
                    9 => event.data["provider_cost_status"] = json!("reported"),
                    10 => event.data["cumulative_usage"]["input_tokens"] = json!(999),
                    11 => event.data["cumulative_provider_cost_microusd"] = json!(1),
                    12 => event.data["cumulative_provider_cost_status"] = json!("reported"),
                    13 => {
                        event.data["pricing_schedule_digest"] =
                            json!(value_digest(&json!("pricing substitution")).unwrap())
                    }
                    14 => {
                        event.data["decision_digest"] =
                            json!(value_digest(&json!("decision substitution")).unwrap())
                    }
                    15 => event.data["decision"]["name"] = json!("substituted"),
                    _ => unreachable!(),
                }
                event.data_digest = AgentRunEvent::create(
                    event.run_id,
                    event.sequence,
                    event.kind,
                    event.data.clone(),
                    event.created_at,
                )
                .unwrap()
                .data_digest;
            }
            Ok(events)
        }
    }

    impl AgentRunStore for RejectCommittedRunStore {
        fn save_agent_run(&self, run: &AgentRun) -> Result<(), String> {
            self.inner.save_agent_run(run)
        }
        fn load_agent_run(&self, run_id: &Uuid) -> Result<Option<AgentRun>, String> {
            self.inner.load_agent_run(run_id)
        }
        fn list_agent_runs(&self) -> Result<Vec<AgentRun>, String> {
            self.inner.list_agent_runs()
        }
        fn save_agent_run_step(&self, step: &AgentRunStep) -> Result<(), String> {
            self.inner.save_agent_run_step(step)
        }
        fn load_agent_run_step(&self, step_id: &Uuid) -> Result<Option<AgentRunStep>, String> {
            self.inner.load_agent_run_step(step_id)
        }
        fn list_agent_run_steps(&self, run_id: &Uuid) -> Result<Vec<AgentRunStep>, String> {
            self.inner.list_agent_run_steps(run_id)
        }
        fn find_agent_run_step_by_approval(
            &self,
            request_id: &Uuid,
        ) -> Result<Option<AgentRunStep>, String> {
            self.inner.find_agent_run_step_by_approval(request_id)
        }
        fn save_agent_checkpoint(&self, checkpoint: &AgentCheckpoint) -> Result<(), String> {
            if checkpoint.state["kind"] == LIVE_RUNTIME_CHECKPOINT_KIND
                && checkpoint.state["runtime"]["attempts"]
                    .as_array()
                    .and_then(|attempts| attempts.last())
                    .is_some_and(|attempt| attempt["state"] == "committed")
            {
                return Err("simulated crash before committed checkpoint save".to_string());
            }
            self.inner.save_agent_checkpoint(checkpoint)
        }
        fn list_agent_checkpoints(&self, run_id: &Uuid) -> Result<Vec<AgentCheckpoint>, String> {
            self.inner.list_agent_checkpoints(run_id)
        }
        fn save_agent_run_evaluation(&self, evaluation: &AgentRunEvaluation) -> Result<(), String> {
            self.inner.save_agent_run_evaluation(evaluation)
        }
        fn list_agent_run_evaluations(
            &self,
            run_id: &Uuid,
        ) -> Result<Vec<AgentRunEvaluation>, String> {
            self.inner.list_agent_run_evaluations(run_id)
        }
    }

    struct RejectModelRespondedAgentStore {
        inner: Arc<RecordingAgentStore>,
    }

    impl AgentStore for RejectModelRespondedAgentStore {
        fn save_agent_definition(&self, agent: &AgentDefinition) -> Result<(), String> {
            self.inner.save_agent_definition(agent)
        }
        fn load_agent_definition(
            &self,
            agent_id: &Uuid,
        ) -> Result<Option<AgentDefinition>, String> {
            self.inner.load_agent_definition(agent_id)
        }
        fn list_agent_definitions(&self) -> Result<Vec<AgentDefinition>, String> {
            self.inner.list_agent_definitions()
        }
        fn save_agent_run_event(&self, event: &AgentRunEvent) -> Result<(), String> {
            if event.kind == AgentRunEventKind::ModelResponded {
                return Err("simulated crash before model_responded event save".to_string());
            }
            self.inner.save_agent_run_event(event)
        }
        fn list_agent_run_events(&self, run_id: &Uuid) -> Result<Vec<AgentRunEvent>, String> {
            self.inner.list_agent_run_events(run_id)
        }
    }

    struct PanicFactory {
        creates: AtomicUsize,
    }

    impl ModelGatewayFactory for PanicFactory {
        fn create(
            &self,
            _context: &ModelGatewayFactoryContext,
        ) -> Result<Arc<dyn ModelGateway>, ModelGatewayFactoryError> {
            self.creates.fetch_add(1, Ordering::SeqCst);
            panic!("simulated process death after prepared checkpoint")
        }
    }

    struct RejectFirstExecutionApprovalStore {
        inner: Arc<RecordingApprovalStore>,
        reject: AtomicBool,
    }

    struct CountingApprovalWrites {
        inner: Arc<RecordingApprovalStore>,
        writes: AtomicUsize,
    }

    impl ApprovalStore for CountingApprovalWrites {
        fn save_approval_request(&self, request: &SignedApprovalRequest) -> Result<(), String> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            self.inner.save_approval_request(request)
        }

        fn load_approval_request(
            &self,
            request_id: &Uuid,
        ) -> Result<Option<SignedApprovalRequest>, String> {
            self.inner.load_approval_request(request_id)
        }

        fn list_approval_requests(&self) -> Result<Vec<SignedApprovalRequest>, String> {
            self.inner.list_approval_requests()
        }

        fn save_approval_decision(&self, decision: &SignedApprovalDecision) -> Result<(), String> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            self.inner.save_approval_decision(decision)
        }

        fn load_approval_decision(
            &self,
            request_id: &Uuid,
        ) -> Result<Option<SignedApprovalDecision>, String> {
            self.inner.load_approval_decision(request_id)
        }

        fn save_approval_execution(&self, execution: &ApprovalExecution) -> Result<(), String> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            self.inner.save_approval_execution(execution)
        }

        fn load_approval_execution(
            &self,
            request_id: &Uuid,
        ) -> Result<Option<ApprovalExecution>, String> {
            self.inner.load_approval_execution(request_id)
        }

        fn load_trusted_approver(
            &self,
            approver: &PrincipalId,
        ) -> Result<Option<Principal>, String> {
            self.inner.load_trusted_approver(approver)
        }
    }

    struct ReadTimestampApprovalStore {
        inner: Arc<RecordingApprovalStore>,
        reads: AtomicUsize,
    }

    impl ApprovalStore for ReadTimestampApprovalStore {
        fn save_approval_request(&self, request: &SignedApprovalRequest) -> Result<(), String> {
            self.inner.save_approval_request(request)
        }
        fn load_approval_request(
            &self,
            request_id: &Uuid,
        ) -> Result<Option<SignedApprovalRequest>, String> {
            self.inner.load_approval_request(request_id)
        }
        fn list_approval_requests(&self) -> Result<Vec<SignedApprovalRequest>, String> {
            self.inner.list_approval_requests()
        }
        fn save_approval_decision(&self, decision: &SignedApprovalDecision) -> Result<(), String> {
            self.inner.save_approval_decision(decision)
        }
        fn load_approval_decision(
            &self,
            request_id: &Uuid,
        ) -> Result<Option<SignedApprovalDecision>, String> {
            self.inner.load_approval_decision(request_id)
        }
        fn save_approval_execution(&self, execution: &ApprovalExecution) -> Result<(), String> {
            self.inner.save_approval_execution(execution)
        }
        fn load_approval_execution(
            &self,
            request_id: &Uuid,
        ) -> Result<Option<ApprovalExecution>, String> {
            self.inner.load_approval_execution(request_id)
        }
        fn load_trusted_approver(
            &self,
            approver: &PrincipalId,
        ) -> Result<Option<Principal>, String> {
            let mut principal = self.inner.load_trusted_approver(approver)?;
            if let Some(principal) = principal.as_mut() {
                let read = self.reads.fetch_add(1, Ordering::SeqCst) as i64 + 1;
                principal.created_at += Duration::seconds(read);
            }
            Ok(principal)
        }
    }

    #[derive(Clone, Copy)]
    enum ApprovalRequestFault {
        Save,
        Reread,
        SubstituteBody,
        SubstituteSignature,
    }

    struct FaultingApprovalRequestStore {
        inner: Arc<RecordingApprovalStore>,
        fault: ApprovalRequestFault,
        armed: AtomicBool,
        saved: AtomicBool,
    }

    impl ApprovalStore for FaultingApprovalRequestStore {
        fn save_approval_request(&self, request: &SignedApprovalRequest) -> Result<(), String> {
            if matches!(self.fault, ApprovalRequestFault::Save)
                && self.armed.swap(false, Ordering::SeqCst)
            {
                return Err("injected approval request save fault".to_string());
            }
            let result = self.inner.save_approval_request(request);
            if result.is_ok() && matches!(self.fault, ApprovalRequestFault::Reread) {
                self.saved.store(true, Ordering::SeqCst);
            }
            result
        }
        fn load_approval_request(
            &self,
            request_id: &Uuid,
        ) -> Result<Option<SignedApprovalRequest>, String> {
            if matches!(self.fault, ApprovalRequestFault::Reread)
                && self.saved.load(Ordering::SeqCst)
                && self.armed.swap(false, Ordering::SeqCst)
            {
                return Err("injected approval request reread fault".to_string());
            }
            let mut request = self.inner.load_approval_request(request_id)?;
            if let Some(request) = request.as_mut() {
                match self.fault {
                    ApprovalRequestFault::SubstituteBody => {
                        request.body.operation = "release.delete".to_string();
                    }
                    ApprovalRequestFault::SubstituteSignature => {
                        request.signature[0] ^= 0x01;
                    }
                    _ => {}
                }
            }
            Ok(request)
        }
        fn list_approval_requests(&self) -> Result<Vec<SignedApprovalRequest>, String> {
            self.inner.list_approval_requests()
        }
        fn save_approval_decision(&self, decision: &SignedApprovalDecision) -> Result<(), String> {
            self.inner.save_approval_decision(decision)
        }
        fn load_approval_decision(
            &self,
            request_id: &Uuid,
        ) -> Result<Option<SignedApprovalDecision>, String> {
            self.inner.load_approval_decision(request_id)
        }
        fn save_approval_execution(&self, execution: &ApprovalExecution) -> Result<(), String> {
            self.inner.save_approval_execution(execution)
        }
        fn load_approval_execution(
            &self,
            request_id: &Uuid,
        ) -> Result<Option<ApprovalExecution>, String> {
            self.inner.load_approval_execution(request_id)
        }
        fn load_trusted_approver(
            &self,
            approver: &PrincipalId,
        ) -> Result<Option<Principal>, String> {
            self.inner.load_trusted_approver(approver)
        }
    }

    impl ApprovalStore for RejectFirstExecutionApprovalStore {
        fn save_approval_request(&self, request: &SignedApprovalRequest) -> Result<(), String> {
            self.inner.save_approval_request(request)
        }
        fn load_approval_request(
            &self,
            request_id: &Uuid,
        ) -> Result<Option<SignedApprovalRequest>, String> {
            self.inner.load_approval_request(request_id)
        }
        fn list_approval_requests(&self) -> Result<Vec<SignedApprovalRequest>, String> {
            self.inner.list_approval_requests()
        }
        fn save_approval_decision(&self, decision: &SignedApprovalDecision) -> Result<(), String> {
            self.inner.save_approval_decision(decision)
        }
        fn load_approval_decision(
            &self,
            request_id: &Uuid,
        ) -> Result<Option<SignedApprovalDecision>, String> {
            self.inner.load_approval_decision(request_id)
        }
        fn save_approval_execution(&self, execution: &ApprovalExecution) -> Result<(), String> {
            if self.reject.swap(false, Ordering::SeqCst) {
                return Err("simulated crash before approval execution save".to_string());
            }
            self.inner.save_approval_execution(execution)
        }
        fn load_approval_execution(
            &self,
            request_id: &Uuid,
        ) -> Result<Option<ApprovalExecution>, String> {
            self.inner.load_approval_execution(request_id)
        }
        fn load_trusted_approver(
            &self,
            approver: &PrincipalId,
        ) -> Result<Option<Principal>, String> {
            self.inner.load_trusted_approver(approver)
        }
    }

    impl ScriptedGateway {
        fn new(turns: Vec<ModelTurn>) -> Self {
            Self {
                turns: Mutex::new(turns.into_iter().map(Ok).collect()),
                requests: Mutex::new(Vec::new()),
            }
        }
    }

    impl ModelGateway for ScriptedGateway {
        fn provider(&self) -> &str {
            "openai"
        }

        fn complete(&self, request: &ModelTurnRequest) -> Result<ModelTurn, ModelGatewayError> {
            self.requests.lock().unwrap().push(request.clone());
            self.turns.lock().unwrap().pop_front().unwrap_or_else(|| {
                Err(ModelGatewayError::InvalidResponse(
                    "script exhausted".to_string(),
                ))
            })
        }
    }

    struct CountingHandler {
        operation: &'static str,
        count: Arc<AtomicUsize>,
    }

    impl OperationHandler for CountingHandler {
        fn operation(&self) -> &str {
            self.operation
        }

        fn execute(
            &self,
            input: &Value,
            _context: &ExecutionContext,
        ) -> Result<Value, proof_kernel::ExecutionError> {
            self.count.fetch_add(1, Ordering::SeqCst);
            Ok(json!({"echo": input}))
        }
    }

    struct Fixture {
        registry: Registry,
        identity: Keypair,
        agent: AgentDefinition,
        agent_store: Arc<RecordingAgentStore>,
        run_store: Arc<RecordingAgentRunStore>,
        approval_store: Arc<RecordingApprovalStore>,
        workspace: tempfile::TempDir,
        count: Arc<AtomicUsize>,
    }

    struct AdversarialAgentRunStore {
        inner: Arc<RecordingAgentRunStore>,
        resolved_step: Option<AgentRunStep>,
        reject_approval_claim: bool,
        reject_checkpoints: Option<Arc<AtomicBool>>,
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum GenericBootstrapFault {
        AfterQueuedSave,
        AfterRunningSave,
        AfterInitialCheckpointSave,
        AfterStartedEventSave,
        AfterToolSucceededEventSave,
        BeforeToolSucceededEventSave,
        BeforeToolFailedEventSave,
        AfterApprovalRunResumeSave,
        AfterToolFailedEventSave,
    }

    #[derive(Clone, Copy)]
    enum GenericEvidenceMutation {
        CheckpointNextInput,
        PendingCall,
        PendingTool,
        PendingArguments,
        ToolEventStep,
        ToolEventCall,
        ToolEventOperation,
        ToolEventProof,
        ToolEventError,
        StepProof,
        ForgedApprovalPair,
    }

    struct AdversarialGenericEvidenceStore {
        run_inner: Arc<RecordingAgentRunStore>,
        agent_inner: Arc<RecordingAgentStore>,
        mutation: GenericEvidenceMutation,
        forged_id: Uuid,
    }

    impl AdversarialGenericEvidenceStore {
        fn new(
            run_inner: Arc<RecordingAgentRunStore>,
            agent_inner: Arc<RecordingAgentStore>,
            mutation: GenericEvidenceMutation,
        ) -> Self {
            Self {
                run_inner,
                agent_inner,
                mutation,
                forged_id: Uuid::now_v7(),
            }
        }

        fn mutate_checkpoint(&self, checkpoint: &mut AgentCheckpoint) {
            let runtime = &mut checkpoint.state["runtime"];
            match self.mutation {
                GenericEvidenceMutation::CheckpointNextInput => {
                    runtime["next_input"] = json!({
                        "type": "tool_output",
                        "call_id": "substituted-checkpoint-call",
                        "output": {"ok": true, "result": "substituted"},
                    });
                }
                GenericEvidenceMutation::PendingCall => {
                    runtime["pending_tool"]["call_id"] = json!("substituted-pending-call");
                }
                GenericEvidenceMutation::PendingTool => {
                    runtime["pending_tool"]["tool_name"] = json!("substituted_pending_tool");
                }
                GenericEvidenceMutation::PendingArguments => {
                    runtime["pending_tool"]["arguments"] = json!({"name": "Substituted"});
                }
                GenericEvidenceMutation::ForgedApprovalPair => {
                    runtime["pending_tool"]["approval_request_id"] = json!(self.forged_id);
                }
                _ => return,
            }
            checkpoint.state_digest = AgentCheckpoint::create(
                checkpoint.run_id,
                checkpoint.sequence,
                checkpoint.state.clone(),
                checkpoint.created_at,
            )
            .unwrap()
            .state_digest;
        }

        fn mutate_step(&self, step: &mut AgentRunStep) {
            match self.mutation {
                GenericEvidenceMutation::StepProof => {
                    if let Some(proof) = step.proof.as_mut() {
                        proof.body.id = self.forged_id;
                    }
                }
                GenericEvidenceMutation::ForgedApprovalPair => {
                    if step.approval_request_id.is_some() {
                        step.approval_request_id = Some(self.forged_id);
                    }
                }
                _ => {}
            }
        }

        fn mutate_event(&self, event: &mut AgentRunEvent) {
            let changed = match self.mutation {
                GenericEvidenceMutation::ToolEventStep
                    if matches!(
                        event.kind,
                        AgentRunEventKind::ToolSucceeded | AgentRunEventKind::ToolFailed
                    ) =>
                {
                    event.data["step_id"] = json!(self.forged_id);
                    true
                }
                GenericEvidenceMutation::ToolEventCall
                    if matches!(
                        event.kind,
                        AgentRunEventKind::ToolSucceeded | AgentRunEventKind::ToolFailed
                    ) =>
                {
                    event.data["call_id"] = json!("substituted-event-call");
                    true
                }
                GenericEvidenceMutation::ToolEventOperation
                    if event.kind == AgentRunEventKind::ToolSucceeded =>
                {
                    event.data["operation"] = json!("substituted.operation");
                    true
                }
                GenericEvidenceMutation::ToolEventProof
                    if event.kind == AgentRunEventKind::ToolSucceeded =>
                {
                    event.data["proof_id"] = json!(self.forged_id);
                    true
                }
                GenericEvidenceMutation::ToolEventError
                    if event.kind == AgentRunEventKind::ToolFailed =>
                {
                    event.data["error"] = json!("substituted failure");
                    true
                }
                GenericEvidenceMutation::ForgedApprovalPair
                    if matches!(
                        event.kind,
                        AgentRunEventKind::ApprovalRequired | AgentRunEventKind::ApprovalResumed
                    ) =>
                {
                    event.data["request_id"] = json!(self.forged_id);
                    true
                }
                _ => false,
            };
            if changed {
                event.data_digest = AgentRunEvent::create(
                    event.run_id,
                    event.sequence,
                    event.kind,
                    event.data.clone(),
                    event.created_at,
                )
                .unwrap()
                .data_digest;
            }
        }
    }

    struct FaultingGenericBootstrapStore {
        run_inner: Arc<RecordingAgentRunStore>,
        agent_inner: Arc<RecordingAgentStore>,
        fault: GenericBootstrapFault,
        armed: AtomicBool,
    }

    impl FaultingGenericBootstrapStore {
        fn new(
            run_inner: Arc<RecordingAgentRunStore>,
            agent_inner: Arc<RecordingAgentStore>,
            fault: GenericBootstrapFault,
        ) -> Self {
            Self {
                run_inner,
                agent_inner,
                fault,
                armed: AtomicBool::new(true),
            }
        }

        fn fail_after(&self, matches: bool, result: Result<(), String>) -> Result<(), String> {
            result?;
            if matches && self.armed.swap(false, Ordering::SeqCst) {
                Err("simulated process death after generic bootstrap save".to_string())
            } else {
                Ok(())
            }
        }
    }

    impl AgentRunStore for FaultingGenericBootstrapStore {
        fn save_agent_run(&self, run: &AgentRun) -> Result<(), String> {
            let matches = (self.fault == GenericBootstrapFault::AfterQueuedSave
                && run.status == AgentRunStatus::Queued)
                || (self.fault == GenericBootstrapFault::AfterRunningSave
                    && run.status == AgentRunStatus::Running
                    && run.revision == 1)
                || (self.fault == GenericBootstrapFault::AfterApprovalRunResumeSave
                    && run.status == AgentRunStatus::Running
                    && run.revision == 3);
            self.fail_after(matches, self.run_inner.save_agent_run(run))
        }

        fn load_agent_run(&self, run_id: &Uuid) -> Result<Option<AgentRun>, String> {
            self.run_inner.load_agent_run(run_id)
        }

        fn list_agent_runs(&self) -> Result<Vec<AgentRun>, String> {
            self.run_inner.list_agent_runs()
        }

        fn save_agent_run_step(&self, step: &AgentRunStep) -> Result<(), String> {
            self.run_inner.save_agent_run_step(step)
        }

        fn load_agent_run_step(&self, step_id: &Uuid) -> Result<Option<AgentRunStep>, String> {
            self.run_inner.load_agent_run_step(step_id)
        }

        fn list_agent_run_steps(&self, run_id: &Uuid) -> Result<Vec<AgentRunStep>, String> {
            self.run_inner.list_agent_run_steps(run_id)
        }

        fn find_agent_run_step_by_approval(
            &self,
            approval_request_id: &Uuid,
        ) -> Result<Option<AgentRunStep>, String> {
            self.run_inner
                .find_agent_run_step_by_approval(approval_request_id)
        }

        fn save_agent_checkpoint(&self, checkpoint: &AgentCheckpoint) -> Result<(), String> {
            let matches = self.fault == GenericBootstrapFault::AfterInitialCheckpointSave
                && checkpoint.sequence == 0
                && checkpoint.state["kind"] == RUNTIME_CHECKPOINT_KIND;
            self.fail_after(matches, self.run_inner.save_agent_checkpoint(checkpoint))
        }

        fn list_agent_checkpoints(&self, run_id: &Uuid) -> Result<Vec<AgentCheckpoint>, String> {
            self.run_inner.list_agent_checkpoints(run_id)
        }

        fn save_agent_run_evaluation(&self, evaluation: &AgentRunEvaluation) -> Result<(), String> {
            self.run_inner.save_agent_run_evaluation(evaluation)
        }

        fn list_agent_run_evaluations(
            &self,
            run_id: &Uuid,
        ) -> Result<Vec<AgentRunEvaluation>, String> {
            self.run_inner.list_agent_run_evaluations(run_id)
        }
    }

    impl AgentStore for FaultingGenericBootstrapStore {
        fn save_agent_definition(&self, agent: &AgentDefinition) -> Result<(), String> {
            self.agent_inner.save_agent_definition(agent)
        }

        fn load_agent_definition(
            &self,
            agent_id: &Uuid,
        ) -> Result<Option<AgentDefinition>, String> {
            self.agent_inner.load_agent_definition(agent_id)
        }

        fn list_agent_definitions(&self) -> Result<Vec<AgentDefinition>, String> {
            self.agent_inner.list_agent_definitions()
        }

        fn save_agent_run_event(&self, event: &AgentRunEvent) -> Result<(), String> {
            let before_save = (self.fault == GenericBootstrapFault::BeforeToolSucceededEventSave
                && event.kind == AgentRunEventKind::ToolSucceeded)
                || (self.fault == GenericBootstrapFault::BeforeToolFailedEventSave
                    && event.kind == AgentRunEventKind::ToolFailed);
            if before_save && self.armed.swap(false, Ordering::SeqCst) {
                return Err(
                    "simulated process death before generic tool result event save".to_string(),
                );
            }
            let matches = (self.fault == GenericBootstrapFault::AfterStartedEventSave
                && event.sequence == 0
                && event.kind == AgentRunEventKind::Started)
                || (self.fault == GenericBootstrapFault::AfterToolSucceededEventSave
                    && event.kind == AgentRunEventKind::ToolSucceeded)
                || (self.fault == GenericBootstrapFault::AfterToolFailedEventSave
                    && event.kind == AgentRunEventKind::ToolFailed);
            self.fail_after(matches, self.agent_inner.save_agent_run_event(event))
        }

        fn list_agent_run_events(&self, run_id: &Uuid) -> Result<Vec<AgentRunEvent>, String> {
            self.agent_inner.list_agent_run_events(run_id)
        }
    }

    impl AgentRunStore for AdversarialGenericEvidenceStore {
        fn save_agent_run(&self, run: &AgentRun) -> Result<(), String> {
            self.run_inner.save_agent_run(run)
        }

        fn load_agent_run(&self, run_id: &Uuid) -> Result<Option<AgentRun>, String> {
            self.run_inner.load_agent_run(run_id)
        }

        fn list_agent_runs(&self) -> Result<Vec<AgentRun>, String> {
            self.run_inner.list_agent_runs()
        }

        fn save_agent_run_step(&self, step: &AgentRunStep) -> Result<(), String> {
            self.run_inner.save_agent_run_step(step)
        }

        fn load_agent_run_step(&self, step_id: &Uuid) -> Result<Option<AgentRunStep>, String> {
            let mut step = self.run_inner.load_agent_run_step(step_id)?;
            if let Some(step) = step.as_mut() {
                self.mutate_step(step);
            }
            Ok(step)
        }

        fn list_agent_run_steps(&self, run_id: &Uuid) -> Result<Vec<AgentRunStep>, String> {
            let mut steps = self.run_inner.list_agent_run_steps(run_id)?;
            for step in &mut steps {
                self.mutate_step(step);
            }
            Ok(steps)
        }

        fn find_agent_run_step_by_approval(
            &self,
            approval_request_id: &Uuid,
        ) -> Result<Option<AgentRunStep>, String> {
            let mut step = if matches!(self.mutation, GenericEvidenceMutation::ForgedApprovalPair)
                && *approval_request_id == self.forged_id
            {
                self.run_inner
                    .list_agent_run_steps(&Uuid::nil())?
                    .into_iter()
                    .find(|step| step.approval_request_id.is_some())
            } else {
                self.run_inner
                    .find_agent_run_step_by_approval(approval_request_id)?
            };
            if step.is_none()
                && matches!(self.mutation, GenericEvidenceMutation::ForgedApprovalPair)
                && *approval_request_id == self.forged_id
            {
                step = self
                    .run_inner
                    .list_agent_runs()?
                    .into_iter()
                    .find_map(|run| {
                        self.run_inner
                            .list_agent_run_steps(&run.id)
                            .ok()?
                            .into_iter()
                            .find(|step| step.approval_request_id.is_some())
                    });
            }
            if let Some(step) = step.as_mut() {
                self.mutate_step(step);
            }
            Ok(step)
        }

        fn save_agent_checkpoint(&self, checkpoint: &AgentCheckpoint) -> Result<(), String> {
            self.run_inner.save_agent_checkpoint(checkpoint)
        }

        fn list_agent_checkpoints(&self, run_id: &Uuid) -> Result<Vec<AgentCheckpoint>, String> {
            let mut checkpoints = self.run_inner.list_agent_checkpoints(run_id)?;
            if let Some(checkpoint) = checkpoints.last_mut() {
                self.mutate_checkpoint(checkpoint);
            }
            Ok(checkpoints)
        }

        fn save_agent_run_evaluation(&self, evaluation: &AgentRunEvaluation) -> Result<(), String> {
            self.run_inner.save_agent_run_evaluation(evaluation)
        }

        fn list_agent_run_evaluations(
            &self,
            run_id: &Uuid,
        ) -> Result<Vec<AgentRunEvaluation>, String> {
            self.run_inner.list_agent_run_evaluations(run_id)
        }
    }

    impl AgentStore for AdversarialGenericEvidenceStore {
        fn save_agent_definition(&self, agent: &AgentDefinition) -> Result<(), String> {
            self.agent_inner.save_agent_definition(agent)
        }

        fn load_agent_definition(
            &self,
            agent_id: &Uuid,
        ) -> Result<Option<AgentDefinition>, String> {
            self.agent_inner.load_agent_definition(agent_id)
        }

        fn list_agent_definitions(&self) -> Result<Vec<AgentDefinition>, String> {
            self.agent_inner.list_agent_definitions()
        }

        fn save_agent_run_event(&self, event: &AgentRunEvent) -> Result<(), String> {
            self.agent_inner.save_agent_run_event(event)
        }

        fn list_agent_run_events(&self, run_id: &Uuid) -> Result<Vec<AgentRunEvent>, String> {
            let mut events = self.agent_inner.list_agent_run_events(run_id)?;
            for event in &mut events {
                self.mutate_event(event);
            }
            Ok(events)
        }
    }

    struct GenericFactorySpy {
        creates: AtomicUsize,
        gateway: Arc<dyn ModelGateway>,
    }

    impl ModelGatewayFactory for GenericFactorySpy {
        fn create(
            &self,
            _context: &ModelGatewayFactoryContext,
        ) -> Result<Arc<dyn ModelGateway>, ModelGatewayFactoryError> {
            self.creates.fetch_add(1, Ordering::SeqCst);
            Ok(self.gateway.clone())
        }
    }

    impl AgentRunStore for AdversarialAgentRunStore {
        fn save_agent_run(&self, run: &AgentRun) -> Result<(), String> {
            self.inner.save_agent_run(run)
        }

        fn load_agent_run(&self, run_id: &Uuid) -> Result<Option<AgentRun>, String> {
            self.inner.load_agent_run(run_id)
        }

        fn list_agent_runs(&self) -> Result<Vec<AgentRun>, String> {
            self.inner.list_agent_runs()
        }

        fn save_agent_run_step(&self, step: &AgentRunStep) -> Result<(), String> {
            if self.reject_approval_claim
                && step.approval_request_id.is_some()
                && step.status != AgentRunStepStatus::WaitingForApproval
            {
                return Err("simulated lost approval claim".to_string());
            }
            self.inner.save_agent_run_step(step)
        }

        fn load_agent_run_step(&self, step_id: &Uuid) -> Result<Option<AgentRunStep>, String> {
            self.inner.load_agent_run_step(step_id)
        }

        fn list_agent_run_steps(&self, run_id: &Uuid) -> Result<Vec<AgentRunStep>, String> {
            self.inner.list_agent_run_steps(run_id)
        }

        fn find_agent_run_step_by_approval(
            &self,
            approval_request_id: &Uuid,
        ) -> Result<Option<AgentRunStep>, String> {
            match &self.resolved_step {
                Some(step) => Ok(Some(step.clone())),
                None => self
                    .inner
                    .find_agent_run_step_by_approval(approval_request_id),
            }
        }

        fn save_agent_checkpoint(&self, checkpoint: &AgentCheckpoint) -> Result<(), String> {
            if self
                .reject_checkpoints
                .as_ref()
                .is_some_and(|reject| reject.load(Ordering::SeqCst))
            {
                return Err("simulated terminal checkpoint seal".to_string());
            }
            self.inner.save_agent_checkpoint(checkpoint)
        }

        fn list_agent_checkpoints(&self, run_id: &Uuid) -> Result<Vec<AgentCheckpoint>, String> {
            self.inner.list_agent_checkpoints(run_id)
        }

        fn save_agent_run_evaluation(&self, evaluation: &AgentRunEvaluation) -> Result<(), String> {
            self.inner.save_agent_run_evaluation(evaluation)
        }

        fn list_agent_run_evaluations(
            &self,
            run_id: &Uuid,
        ) -> Result<Vec<AgentRunEvaluation>, String> {
            self.inner.list_agent_run_evaluations(run_id)
        }
    }

    impl Fixture {
        fn new(governance: Governance, limits: AgentLimits) -> Self {
            let entry = RegistryEntry {
                operation: "catalog.create".to_string(),
                domain: "commerce".to_string(),
                version: "v1".to_string(),
                action: "commerce:catalog_create".to_string(),
                description: "Create a catalog".to_string(),
                input_schema: r#"{"type":"object","required":["name"],"properties":{"name":{"type":"string"}},"additionalProperties":false}"#.to_string(),
                output_schema: r#"{"type":"object"}"#.to_string(),
                required_authority: "delegation-grant".to_string(),
                governance,
                idempotency: "required-uuidv7".to_string(),
                consequence: "catalog-mutation".to_string(),
                evidence_contract: "operation-effect-v1".to_string(),
                benchmark: None,
                status: VersionStatus::Active,
                deprecated_since: None,
                replacement_operation: None,
            };
            let registry = Registry::new(vec![entry]).unwrap();
            let identity = generate_keypair();
            let agent = AgentDefinition::new(
                "catalog-manager",
                "Create the requested catalog.",
                "openai",
                "test-model",
                vec![AgentTool::new("catalog.create", "v1").unwrap()],
                limits,
                Utc::now(),
            )
            .unwrap();
            let agent_store = Arc::new(RecordingAgentStore::default());
            agent_store.save_agent_definition(&agent).unwrap();
            Self {
                registry,
                identity,
                agent,
                agent_store,
                run_store: Arc::new(RecordingAgentRunStore::default()),
                approval_store: Arc::new(RecordingApprovalStore::default()),
                workspace: tempfile::tempdir().unwrap(),
                count: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn runtime(&self, model: Arc<dyn ModelGateway>) -> AgentRuntime {
            self.runtime_with_run_store(model, self.run_store.clone())
        }

        fn runtime_with_run_store(
            &self,
            model: Arc<dyn ModelGateway>,
            run_store: Arc<dyn AgentRunStore>,
        ) -> AgentRuntime {
            self.runtime_with_stores(
                model,
                run_store,
                self.agent_store.clone(),
                self.approval_store.clone(),
            )
        }

        fn runtime_with_stores(
            &self,
            model: Arc<dyn ModelGateway>,
            run_store: Arc<dyn AgentRunStore>,
            agent_store: Arc<dyn AgentStore>,
            approval_store: Arc<dyn ApprovalStore>,
        ) -> AgentRuntime {
            let mut engine =
                ExecutionEngine::new_with_keypair(self.registry.clone(), self.identity.clone());
            engine.register_handler(Arc::new(CountingHandler {
                operation: "catalog.create",
                count: self.count.clone(),
            }));
            AgentRuntime::new(
                self.registry.clone(),
                engine,
                self.identity.clone(),
                self.workspace.path(),
                agent_store,
                run_store,
                approval_store,
                model,
            )
            .unwrap()
        }

        fn runtime_with_factory(
            &self,
            model: Arc<dyn ModelGateway>,
            run_store: Arc<dyn AgentRunStore>,
            agent_store: Arc<dyn AgentStore>,
            factory: Arc<dyn ModelGatewayFactory>,
        ) -> AgentRuntime {
            let mut engine =
                ExecutionEngine::new_with_keypair(self.registry.clone(), self.identity.clone());
            engine.register_handler(Arc::new(CountingHandler {
                operation: "catalog.create",
                count: self.count.clone(),
            }));
            let mut runtime = AgentRuntime::new_with_gateway_factory(
                self.registry.clone(),
                engine,
                self.identity.clone(),
                self.workspace.path(),
                agent_store,
                run_store,
                self.approval_store.clone(),
                factory,
            )
            .unwrap();
            // Generic callers still use the fixed model. Supplying it here
            // lets this test independently prove that the live factory is not
            // touched while bootstrap barriers are incomplete.
            runtime.model = model;
            runtime
        }
    }

    struct LiveFixture {
        registry: Registry,
        identity: Keypair,
        approver: Keypair,
        agent: AgentDefinition,
        agent_store: Arc<RecordingAgentStore>,
        run_store: Arc<RecordingAgentRunStore>,
        approval_store: Arc<RecordingApprovalStore>,
        execution_store: Arc<RecordingStore>,
        workspace: tempfile::TempDir,
        setup: LiveRunSetup,
        arguments: ReleasePublishArguments,
    }

    impl LiveFixture {
        fn new() -> Self {
            let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .canonicalize()
                .unwrap();
            let registry = Registry::load_from_directory(repository.join("registry/content"))
                .expect("content registry");
            let identity = generate_keypair_for(PrincipalKind::Agent);
            let approver = generate_keypair_for(PrincipalKind::Human);
            let limits = AgentLimits {
                max_steps: 2,
                max_model_calls: 3,
                max_total_tokens: 10_000,
                max_duration_seconds: 300,
                max_output_tokens_per_call: 1024,
                max_cost_microusd: Some(120_000),
            };
            let agent = AgentDefinition::new(
                format!("live-release-manager-{}", Uuid::now_v7()),
                "Run the sealed synthetic preview journey.",
                LIVE_PROVIDER,
                LIVE_MODEL,
                vec![AgentTool::new("release.publish", "v2").unwrap()],
                limits,
                Utc::now(),
            )
            .unwrap();
            let agent_store = Arc::new(RecordingAgentStore::default());
            agent_store.save_agent_definition(&agent).unwrap();
            let run_store = Arc::new(RecordingAgentRunStore::default());
            let approval_store = Arc::new(RecordingApprovalStore::default());
            approval_store
                .trust_approver(principal_from_keypair(&approver))
                .unwrap();
            let execution_store = Arc::new(RecordingStore::default());
            let workspace = tempfile::tempdir().unwrap();
            let workspace_registry = workspace.path().join("registry/content");
            std::fs::create_dir_all(&workspace_registry).unwrap();
            std::fs::copy(
                repository.join("registry/content/release-publish-v2.input.json"),
                workspace_registry.join("release-publish-v2.input.json"),
            )
            .unwrap();

            let schema = proof_content::SchemaDefinition::new(
                "Article",
                1,
                vec![proof_content::SchemaField {
                    name: "title".to_string(),
                    field_type: proof_content::FieldType::Text,
                    required: true,
                    localized: false,
                    default_value: None,
                }],
            );
            let object =
                proof_content::Object::create(&schema, "en-US", json!({"title": "Synthetic"}))
                    .unwrap();
            let mut edition = proof_content::Edition::new(Uuid::now_v7(), vec![object]);
            edition.objects.sort_by_key(|object| object.id);
            let edition_dir = workspace.path().join(".proof/data/editions");
            std::fs::create_dir_all(&edition_dir).unwrap();
            std::fs::write(
                edition_dir.join(format!("{}.json", edition.id)),
                serde_json::to_vec(&edition).unwrap(),
            )
            .unwrap();
            let manifest = json!({
                "schema": "proof-content-preview-manifest/v1",
                "edition_id": edition.id,
                "edition_content_digest": proof_content::digest::canonical_digest(&edition.objects),
                "objects": edition.objects.iter().map(|object| json!({
                    "object_id": object.id,
                    "locale": object.locale,
                    "content_digest": proof_content::digest::canonical_digest(object),
                })).collect::<Vec<_>>(),
            });
            let manifest_digest = proof_content::digest::canonical_digest(&manifest);
            let arguments = ReleasePublishArguments {
                idempotency_key: Uuid::now_v7(),
                edition_id: edition.id,
                environment: "preview".to_string(),
                version_label: "2026.08.30-rc1".to_string(),
                manifest_digest,
            };

            let now = Utc::now();
            let issuer = PrincipalId::now();
            let delegation = Delegation {
                id: Uuid::now_v7(),
                issuer,
                recipient: identity.principal_id,
                allowed_actions: vec!["*".to_string()],
                resource_scope: vec!["*".to_string()],
                scope: proof_kernel::delegation::DelegationScope {
                    allowed_operations: Some(vec!["release.publish".to_string()]),
                    allowed_domains: Some(vec!["content".to_string()]),
                    resource_scope: None,
                },
                valid_from: now - Duration::minutes(1),
                valid_until: now + Duration::minutes(10),
                revoked: false,
            };
            execution_store
                .delegations
                .lock()
                .unwrap()
                .push(delegation.clone());
            let authority = LiveAuthoritySetup {
                delegation: delegation.clone(),
                delegation_digest: delegation_digest(&delegation).unwrap(),
                delegation_chain: DelegationChain {
                    root: issuer,
                    grants: vec![delegation],
                },
            };

            let mut deterministic_run = AgentRun::new(
                identity.principal_id,
                AgentRunMode::OneShot,
                "deterministic preflight",
                now - Duration::seconds(3),
            )
            .unwrap();
            run_store.save_agent_run(&deterministic_run).unwrap();
            deterministic_run.start(now - Duration::seconds(2)).unwrap();
            run_store.save_agent_run(&deterministic_run).unwrap();
            deterministic_run
                .succeed(now - Duration::seconds(1))
                .unwrap();
            run_store.save_agent_run(&deterministic_run).unwrap();
            let preview_policy: TraceEvaluationPolicy =
                serde_json::from_str(PREVIEW_POLICY_SOURCE).unwrap();
            let preview_policy_digest = value_digest(&json!({
                "schema": "proof-agent-trace-policy/v1",
                "value": {"policy": preview_policy},
            }))
            .unwrap();
            let deterministic_trace_digest = value_digest(&json!({
                "schema": "fixture-deterministic-trace/v1",
                "run_id": deterministic_run.id,
            }))
            .unwrap();
            let deterministic_evaluation = AgentRunEvaluation::create(
                &deterministic_run,
                "proof-agent-trace/v1",
                AgentEvaluationOutcome::Passed,
                Some(10_000),
                json!({
                    "passed_checks": 10,
                    "total_checks": 10,
                    "binding": {
                        "policy_digest": preview_policy_digest,
                        "trace_digest": deterministic_trace_digest,
                    },
                }),
                Some("fixture preflight".to_string()),
                now,
            )
            .unwrap();
            run_store
                .save_agent_run_evaluation(&deterministic_evaluation)
                .unwrap();
            let preflight = PreflightEvidence {
                schema: "proof-release-manager-preflight-evidence/v1".to_string(),
                policy_path: "evals/release-manager-preview-v1.json".to_string(),
                policy_digest: preview_policy_digest,
                trace_digest: deterministic_trace_digest,
                evaluator: "proof-agent-trace/v1".to_string(),
                run_id: deterministic_run.id,
                evaluation_id: deterministic_evaluation.id,
                evaluation_created_at: deterministic_evaluation.created_at,
                outcome: "passed".to_string(),
                score_bps: 10_000,
                passed_checks: 10,
                total_checks: 10,
            };
            let preflight_value = serde_json::to_value(&preflight).unwrap();
            let preflight_digest = wrapped_digest(
                "proof-release-manager-preflight-evidence-digest/v1",
                "evidence",
                &preflight_value,
            )
            .unwrap();
            let template: Value = serde_json::from_str(LIVE_POLICY_SOURCE).unwrap();
            let goal = format!(
                "Publish synthetic edition {} to preview as {} using manifest {} and idempotency key {}.",
                arguments.edition_id,
                arguments.version_label,
                arguments.manifest_digest,
                arguments.idempotency_key,
            );
            let policy = LivePolicyMaterial {
                template: template.clone(),
                template_policy_digest: value_digest(&template).unwrap(),
                binding_inputs: LiveBindingInputs {
                    preflight_evidence_digest: preflight_digest,
                    agent_principal_id: identity.principal_id,
                    approver_principal_id: approver.principal_id,
                    delegation_id: authority.delegation.id,
                    delegation_digest: authority.delegation_digest,
                    edition_id: arguments.edition_id,
                    manifest_digest: arguments.manifest_digest.clone(),
                    idempotency_key: arguments.idempotency_key,
                    version_label: arguments.version_label.clone(),
                },
                check_set_digest: wrapped_digest(
                    "proof-release-manager-live-check-set-digest/v1",
                    "check_ids",
                    &json!(live_check_ids()),
                )
                .unwrap(),
                tamper_vector_set_digest: wrapped_digest(
                    "proof-release-manager-live-tamper-vector-set-digest/v1",
                    "tamper_vector_ids",
                    &json!(live_tamper_ids()),
                )
                .unwrap(),
                pricing_schedule_digest: value_digest(&template["pricing"]).unwrap(),
                instructions_digest: value_digest(&template["outbound_data"]["instructions"])
                    .unwrap(),
                initial_input_digest: value_digest(&Value::String(goal.clone())).unwrap(),
                parameters_schema_digest: wrapped_digest(
                    "proof-openai-function-parameters-digest/v1",
                    "parameters",
                    &template["tool"]["declaration"]["parameters"],
                )
                .unwrap(),
                tool_declaration_digest: wrapped_digest(
                    "proof-openai-function-declaration-digest/v1",
                    "declaration",
                    &template["tool"]["declaration"],
                )
                .unwrap(),
                tool_set_digest: wrapped_digest(
                    "proof-openai-tool-set-digest/v1",
                    "tools",
                    &json!([template["tool"]["declaration"].clone()]),
                )
                .unwrap(),
            };
            let setup = LiveRunSetup {
                intent: LiveRunIntent::Start {
                    agent_id: agent.id,
                    goal,
                },
                process_epoch_id: Uuid::now_v7(),
                preflight_evidence: preflight_value,
                preflight_evidence_digest: preflight_digest,
                authority,
                policy,
            };
            Self {
                registry,
                identity,
                approver,
                agent,
                agent_store,
                run_store,
                approval_store,
                execution_store,
                workspace,
                setup,
                arguments,
            }
        }

        fn runtime(&self, factory: Arc<dyn ModelGatewayFactory>) -> AgentRuntime {
            self.runtime_with_stores(factory, self.run_store.clone(), self.agent_store.clone())
        }

        fn runtime_with_stores(
            &self,
            factory: Arc<dyn ModelGatewayFactory>,
            run_store: Arc<dyn AgentRunStore>,
            agent_store: Arc<dyn AgentStore>,
        ) -> AgentRuntime {
            self.runtime_with_all_stores(
                factory,
                run_store,
                agent_store,
                self.approval_store.clone(),
            )
        }

        fn runtime_with_all_stores(
            &self,
            factory: Arc<dyn ModelGatewayFactory>,
            run_store: Arc<dyn AgentRunStore>,
            agent_store: Arc<dyn AgentStore>,
            approval_store: Arc<dyn ApprovalStore>,
        ) -> AgentRuntime {
            let mut engine =
                ExecutionEngine::new_with_keypair(self.registry.clone(), self.identity.clone())
                    .with_storage(self.execution_store.clone());
            for handler in proof_content::content_handlers() {
                engine.register_handler(handler);
            }
            AgentRuntime::new_with_gateway_factory(
                self.registry.clone(),
                engine,
                self.identity.clone(),
                self.workspace.path(),
                agent_store,
                run_store,
                approval_store,
                factory,
            )
            .unwrap()
        }

        fn factory(&self, actions: Vec<LiveGatewayAction>) -> Arc<LiveFactorySpy> {
            Arc::new(LiveFactorySpy {
                creates: AtomicUsize::new(0),
                contexts: Mutex::new(Vec::new()),
                gateway: Arc::new(LiveGatewaySpy::new(
                    actions,
                    self.run_store.clone(),
                    self.agent_store.clone(),
                )),
            })
        }

        fn resume_setup(&self, run_id: Uuid) -> LiveRunSetup {
            let mut setup = self.setup.clone();
            setup.intent = LiveRunIntent::Resume { run_id };
            setup.process_epoch_id = Uuid::now_v7();
            setup
        }
    }

    fn workspace_snapshot(root: &Path) -> Vec<(String, Option<Vec<u8>>)> {
        fn visit(root: &Path, directory: &Path, snapshot: &mut Vec<(String, Option<Vec<u8>>)>) {
            let mut entries = std::fs::read_dir(directory)
                .unwrap()
                .map(|entry| entry.unwrap())
                .collect::<Vec<_>>();
            entries.sort_by_key(|entry| entry.file_name());
            for entry in entries {
                let path = entry.path();
                let relative = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                let file_type = entry.file_type().unwrap();
                if file_type.is_dir() {
                    snapshot.push((relative, None));
                    visit(root, &path, snapshot);
                } else if file_type.is_file() {
                    snapshot.push((relative, Some(std::fs::read(path).unwrap())));
                } else if file_type.is_symlink() {
                    snapshot.push((
                        relative,
                        Some(
                            std::fs::read_link(path)
                                .unwrap()
                                .to_string_lossy()
                                .as_bytes()
                                .to_vec(),
                        ),
                    ));
                }
            }
        }

        let mut snapshot = Vec::new();
        visit(root, root, &mut snapshot);
        snapshot
    }

    fn live_store_snapshot(fixture: &LiveFixture) -> Value {
        let runs = fixture.run_store.list_agent_runs().unwrap();
        let run_evidence = runs
            .iter()
            .map(|run| {
                json!({
                    "run": run,
                    "steps": fixture.run_store.list_agent_run_steps(&run.id).unwrap(),
                    "checkpoints": fixture.run_store.list_agent_checkpoints(&run.id).unwrap(),
                    "evaluations": fixture.run_store.list_agent_run_evaluations(&run.id).unwrap(),
                    "events": fixture.agent_store.list_agent_run_events(&run.id).unwrap(),
                })
            })
            .collect::<Vec<_>>();
        let execution_contexts = fixture
            .execution_store
            .contexts
            .lock()
            .unwrap()
            .iter()
            .map(|context| format!("{context:?}"))
            .collect::<Vec<_>>();
        json!({
            "agent_definitions": fixture.agent_store.list_agent_definitions().unwrap(),
            "run_evidence": run_evidence,
            "approval_requests": fixture.approval_store.list_approval_requests().unwrap(),
            "proofs": fixture.execution_store.proofs.lock().unwrap().clone(),
            "execution_contexts": execution_contexts,
            "delegations": fixture.execution_store.delegations.lock().unwrap().clone(),
            "workspace": workspace_snapshot(fixture.workspace.path()),
        })
    }

    fn assert_live_start_check_rejected_without_writes(
        fixture: &LiveFixture,
        setup: &LiveRunSetup,
    ) -> AgentRuntimeError {
        let factory = fixture.factory(vec![]);
        let approval_store = Arc::new(CountingApprovalWrites {
            inner: fixture.approval_store.clone(),
            writes: AtomicUsize::new(0),
        });
        let runtime = fixture.runtime_with_all_stores(
            factory.clone(),
            fixture.run_store.clone(),
            fixture.agent_store.clone(),
            approval_store.clone(),
        );
        let before = live_store_snapshot(fixture);
        let error = runtime
            .check_live_start_setup(setup)
            .expect_err("invalid live start setup was accepted");
        assert_eq!(live_store_snapshot(fixture), before);
        assert_eq!(approval_store.writes.load(Ordering::SeqCst), 0);
        assert_eq!(factory.creates.load(Ordering::SeqCst), 0);
        assert!(factory.contexts.lock().unwrap().is_empty());
        assert_eq!(factory.gateway.sends.load(Ordering::SeqCst), 0);
        error
    }

    fn tool_turn() -> ModelTurn {
        ModelTurn {
            response_id: "resp_tool".to_string(),
            returned_model: None,
            response_body_digest: None,
            decision: ModelDecision::ToolCall {
                call_id: "call_1".to_string(),
                name: "proof_commerce_v1_catalog_create".to_string(),
                arguments: json!({"name": "Spring"}),
            },
            usage: ModelUsage {
                input_tokens: 10,
                output_tokens: 5,
                total_tokens: 15,
                cost_microusd: Some(25),
            },
        }
    }

    fn finish_turn() -> ModelTurn {
        ModelTurn {
            response_id: "resp_finish".to_string(),
            returned_model: None,
            response_body_digest: None,
            decision: ModelDecision::Finish {
                output: "Catalog created.".to_string(),
            },
            usage: ModelUsage {
                input_tokens: 8,
                output_tokens: 3,
                total_tokens: 11,
                cost_microusd: Some(20),
            },
        }
    }

    fn save_pristine_generic_run(fixture: &Fixture, running: bool) -> AgentRun {
        let now = std::cmp::max(Utc::now(), fixture.agent.created_at);
        let mut run = AgentRun::new_for_agent(
            fixture.identity.principal_id,
            fixture.agent.id,
            AgentRunMode::Session,
            "Recover generic bootstrap",
            now,
        )
        .unwrap();
        fixture.run_store.save_agent_run(&run).unwrap();
        if running {
            run.start(now).unwrap();
            fixture.run_store.save_agent_run(&run).unwrap();
        }
        run
    }

    fn save_generic_initial_checkpoint(fixture: &Fixture, run: &AgentRun) -> AgentCheckpoint {
        let state = generic_initial_state(run, &fixture.agent);
        let checkpoint = AgentCheckpoint::create(
            run.id,
            0,
            json!({"kind": RUNTIME_CHECKPOINT_KIND, "runtime": state}),
            std::cmp::max(Utc::now(), run.updated_at),
        )
        .unwrap();
        fixture
            .run_store
            .save_agent_checkpoint(&checkpoint)
            .unwrap();
        checkpoint
    }

    fn save_generic_started_event(
        fixture: &Fixture,
        run: &AgentRun,
        checkpoint: &AgentCheckpoint,
        sequence: u32,
        kind: AgentRunEventKind,
    ) -> AgentRunEvent {
        let data = if kind == AgentRunEventKind::Started {
            generic_started_event(&fixture.agent, run)
        } else {
            json!({"corrupt": true})
        };
        let event = AgentRunEvent::create(
            run.id,
            sequence,
            kind,
            data,
            std::cmp::max(Utc::now(), checkpoint.created_at),
        )
        .unwrap();
        fixture.agent_store.save_agent_run_event(&event).unwrap();
        event
    }

    fn assert_generic_bootstrap_rejected_before_dispatch(fixture: &Fixture, run_id: Uuid) {
        let model = Arc::new(ScriptedGateway::new(vec![finish_turn()]));
        let factory = Arc::new(GenericFactorySpy {
            creates: AtomicUsize::new(0),
            gateway: model.clone(),
        });
        let error = fixture
            .runtime_with_factory(
                model.clone(),
                fixture.run_store.clone(),
                fixture.agent_store.clone(),
                factory.clone(),
            )
            .resume(run_id)
            .expect_err("corrupt bootstrap was accepted");
        assert!(matches!(
            error,
            AgentRuntimeError::ActorMismatch { .. }
                | AgentRuntimeError::AgentNotFound(_)
                | AgentRuntimeError::InvalidCheckpoint(_)
                | AgentRuntimeError::InconsistentState(_)
        ));
        assert_eq!(factory.creates.load(Ordering::SeqCst), 0);
        assert!(model.requests.lock().unwrap().is_empty());
        assert_eq!(fixture.count.load(Ordering::SeqCst), 0);
        assert_eq!(fixture.run_store.list_agent_runs().unwrap().len(), 1);
    }

    #[test]
    fn generic_bootstrap_fault_boundaries_repair_one_run_and_exact_one_barriers() {
        let fixture = Fixture::new(Governance::AgentExecutable, AgentLimits::default());
        let model = Arc::new(ScriptedGateway::new(vec![finish_turn()]));
        let factory = Arc::new(GenericFactorySpy {
            creates: AtomicUsize::new(0),
            gateway: model.clone(),
        });

        let queued_fault = Arc::new(FaultingGenericBootstrapStore::new(
            fixture.run_store.clone(),
            fixture.agent_store.clone(),
            GenericBootstrapFault::AfterQueuedSave,
        ));
        let error = fixture
            .runtime_with_factory(
                model.clone(),
                queued_fault.clone(),
                queued_fault,
                factory.clone(),
            )
            .start(fixture.agent.id, "Recover generic bootstrap")
            .expect_err("queued-save fault did not stop start");
        assert!(error
            .to_string()
            .contains("simulated process death after generic bootstrap save"));
        let runs = fixture.run_store.list_agent_runs().unwrap();
        assert_eq!(runs.len(), 1);
        let run_id = runs[0].id;
        assert_eq!(runs[0].status, AgentRunStatus::Queued);

        for (fault, expected_status, expected_checkpoints, expected_events) in [
            (
                GenericBootstrapFault::AfterRunningSave,
                AgentRunStatus::Running,
                0,
                0,
            ),
            (
                GenericBootstrapFault::AfterInitialCheckpointSave,
                AgentRunStatus::Running,
                1,
                0,
            ),
            (
                GenericBootstrapFault::AfterStartedEventSave,
                AgentRunStatus::Running,
                1,
                1,
            ),
        ] {
            let faulting = Arc::new(FaultingGenericBootstrapStore::new(
                fixture.run_store.clone(),
                fixture.agent_store.clone(),
                fault,
            ));
            let error = fixture
                .runtime_with_factory(model.clone(), faulting.clone(), faulting, factory.clone())
                .resume(run_id)
                .expect_err("bootstrap boundary fault did not stop resume");
            assert!(error
                .to_string()
                .contains("simulated process death after generic bootstrap save"));
            let run = fixture.run_store.load_agent_run(&run_id).unwrap().unwrap();
            assert_eq!(run.status, expected_status);
            assert_eq!(
                fixture
                    .run_store
                    .list_agent_checkpoints(&run_id)
                    .unwrap()
                    .len(),
                expected_checkpoints
            );
            assert_eq!(
                fixture
                    .agent_store
                    .list_agent_run_events(&run_id)
                    .unwrap()
                    .len(),
                expected_events
            );
            assert_eq!(factory.creates.load(Ordering::SeqCst), 0);
            assert!(model.requests.lock().unwrap().is_empty());
            assert_eq!(fixture.count.load(Ordering::SeqCst), 0);
            assert_eq!(fixture.run_store.list_agent_runs().unwrap().len(), 1);
        }

        let outcome = fixture.runtime(model.clone()).resume(run_id).unwrap();
        let AgentRuntimeOutcome::Completed { run, .. } = outcome else {
            panic!("expected repaired run to complete")
        };
        assert_eq!(run.id, run_id);
        assert_eq!(fixture.run_store.list_agent_runs().unwrap().len(), 1);
        let initial_state = generic_initial_state(&run, &fixture.agent);
        let initial_checkpoints = fixture
            .run_store
            .list_agent_checkpoints(&run_id)
            .unwrap()
            .into_iter()
            .filter(|checkpoint| {
                validate_generic_initial_checkpoint(&run, &initial_state, checkpoint).is_ok()
            })
            .count();
        assert_eq!(initial_checkpoints, 1);
        assert_eq!(
            fixture
                .agent_store
                .list_agent_run_events(&run_id)
                .unwrap()
                .iter()
                .filter(|event| event.kind == AgentRunEventKind::Started)
                .count(),
            1
        );
        assert_eq!(model.requests.lock().unwrap().len(), 1);
        assert_eq!(factory.creates.load(Ordering::SeqCst), 0);
        assert_eq!(fixture.count.load(Ordering::SeqCst), 0);

        let checkpoints_before = fixture.run_store.list_agent_checkpoints(&run_id).unwrap();
        let events_before = fixture.agent_store.list_agent_run_events(&run_id).unwrap();
        let replay = fixture.runtime(model.clone()).resume(run_id).unwrap();
        assert!(matches!(replay, AgentRuntimeOutcome::Completed { .. }));
        assert_eq!(
            fixture.run_store.list_agent_checkpoints(&run_id).unwrap(),
            checkpoints_before
        );
        assert_eq!(
            fixture.agent_store.list_agent_run_events(&run_id).unwrap(),
            events_before
        );
        assert_eq!(model.requests.lock().unwrap().len(), 1);
    }

    #[test]
    fn generic_bootstrap_repair_pauses_then_executes_approved_tool_exactly_once() {
        let fixture = Fixture::new(Governance::HumanOnly, AgentLimits::default());
        let model = Arc::new(ScriptedGateway::new(vec![tool_turn(), finish_turn()]));
        let factory = Arc::new(GenericFactorySpy {
            creates: AtomicUsize::new(0),
            gateway: model.clone(),
        });
        let faulting = Arc::new(FaultingGenericBootstrapStore::new(
            fixture.run_store.clone(),
            fixture.agent_store.clone(),
            GenericBootstrapFault::AfterStartedEventSave,
        ));
        fixture
            .runtime_with_factory(model.clone(), faulting.clone(), faulting, factory.clone())
            .start(fixture.agent.id, "Create Spring")
            .expect_err("Started-save fault did not interrupt bootstrap");
        let run_id = fixture.run_store.list_agent_runs().unwrap()[0].id;
        assert_eq!(factory.creates.load(Ordering::SeqCst), 0);
        assert!(model.requests.lock().unwrap().is_empty());
        assert_eq!(fixture.count.load(Ordering::SeqCst), 0);

        let waiting = fixture.runtime(model.clone()).resume(run_id).unwrap();
        let AgentRuntimeOutcome::WaitingForApproval { run, request, .. } = waiting else {
            panic!("repaired run did not pause for approval")
        };
        assert_eq!(run.id, run_id);
        assert_eq!(fixture.count.load(Ordering::SeqCst), 0);
        assert_eq!(model.requests.lock().unwrap().len(), 1);
        assert_eq!(
            fixture
                .agent_store
                .list_agent_run_events(&run_id)
                .unwrap()
                .iter()
                .filter(|event| event.kind == AgentRunEventKind::Started)
                .count(),
            1
        );

        let approver = generate_keypair_for(PrincipalKind::Human);
        fixture
            .approval_store
            .trust_approver(principal_from_keypair(&approver))
            .unwrap();
        fixture
            .approval_store
            .save_approval_decision(
                &SignedApprovalDecision::create(
                    &request,
                    ApprovalOutcome::Approved,
                    Some("bootstrap recovery reviewed".to_string()),
                    Utc::now(),
                    &approver,
                )
                .unwrap(),
            )
            .unwrap();

        let completed = fixture.runtime(model.clone()).resume(run_id).unwrap();
        assert!(matches!(completed, AgentRuntimeOutcome::Completed { .. }));
        assert_eq!(fixture.count.load(Ordering::SeqCst), 1);
        assert_eq!(model.requests.lock().unwrap().len(), 2);
        let checkpoints_before = fixture.run_store.list_agent_checkpoints(&run_id).unwrap();
        let events_before = fixture.agent_store.list_agent_run_events(&run_id).unwrap();
        let replay = fixture.runtime(model.clone()).resume(run_id).unwrap();
        assert!(matches!(replay, AgentRuntimeOutcome::Completed { .. }));
        assert_eq!(fixture.count.load(Ordering::SeqCst), 1);
        assert_eq!(model.requests.lock().unwrap().len(), 2);
        assert_eq!(
            fixture.run_store.list_agent_checkpoints(&run_id).unwrap(),
            checkpoints_before
        );
        assert_eq!(
            fixture.agent_store.list_agent_run_events(&run_id).unwrap(),
            events_before
        );
    }

    #[test]
    fn generic_resume_accepts_only_event_proven_post_bootstrap_progress() {
        let fixture = Fixture::new(Governance::AgentExecutable, AgentLimits::default());
        let model = Arc::new(ScriptedGateway::new(vec![tool_turn(), finish_turn()]));
        let factory = Arc::new(GenericFactorySpy {
            creates: AtomicUsize::new(0),
            gateway: model.clone(),
        });
        let faulting = Arc::new(FaultingGenericBootstrapStore::new(
            fixture.run_store.clone(),
            fixture.agent_store.clone(),
            GenericBootstrapFault::AfterToolSucceededEventSave,
        ));

        fixture
            .runtime_with_factory(model.clone(), faulting.clone(), faulting, factory.clone())
            .start(fixture.agent.id, "Create Spring")
            .expect_err("post-tool event fault did not interrupt the run");
        let run_id = fixture.run_store.list_agent_runs().unwrap()[0].id;
        assert_eq!(fixture.count.load(Ordering::SeqCst), 1);
        assert_eq!(model.requests.lock().unwrap().len(), 1);
        assert_eq!(factory.creates.load(Ordering::SeqCst), 0);

        let outcome = fixture.runtime(model.clone()).resume(run_id).unwrap();
        assert!(matches!(outcome, AgentRuntimeOutcome::Completed { .. }));
        assert_eq!(fixture.count.load(Ordering::SeqCst), 1);
        assert_eq!(model.requests.lock().unwrap().len(), 2);
    }

    #[test]
    fn generic_resume_repairs_exact_tool_result_event_before_continuation() {
        for (fault, first_turn, expected_kind, expected_ok, expected_tool_count) in [
            (
                GenericBootstrapFault::BeforeToolSucceededEventSave,
                tool_turn(),
                AgentRunEventKind::ToolSucceeded,
                true,
                1,
            ),
            (
                GenericBootstrapFault::BeforeToolFailedEventSave,
                ModelTurn {
                    response_id: "resp_invalid_tool".to_string(),
                    returned_model: None,
                    response_body_digest: None,
                    decision: ModelDecision::ToolCall {
                        call_id: "call_1".to_string(),
                        name: "proof_commerce_v1_catalog_create".to_string(),
                        arguments: json!({}),
                    },
                    usage: ModelUsage {
                        input_tokens: 10,
                        output_tokens: 5,
                        total_tokens: 15,
                        cost_microusd: Some(25),
                    },
                },
                AgentRunEventKind::ToolFailed,
                false,
                0,
            ),
        ] {
            let fixture = Fixture::new(Governance::AgentExecutable, AgentLimits::default());
            let model = Arc::new(ScriptedGateway::new(vec![first_turn, finish_turn()]));
            let factory = Arc::new(GenericFactorySpy {
                creates: AtomicUsize::new(0),
                gateway: model.clone(),
            });
            let faulting = Arc::new(FaultingGenericBootstrapStore::new(
                fixture.run_store.clone(),
                fixture.agent_store.clone(),
                fault,
            ));

            let error = fixture
                .runtime_with_factory(model.clone(), faulting.clone(), faulting, factory.clone())
                .start(fixture.agent.id, "Recover split tool result")
                .expect_err("tool-result pre-save fault did not interrupt the run");
            assert!(error
                .to_string()
                .contains("simulated process death before generic tool result event save"));
            let run_id = fixture.run_store.list_agent_runs().unwrap()[0].id;
            assert_eq!(fixture.count.load(Ordering::SeqCst), expected_tool_count);
            assert_eq!(model.requests.lock().unwrap().len(), 1);
            assert!(fixture
                .agent_store
                .list_agent_run_events(&run_id)
                .unwrap()
                .iter()
                .all(|event| event.kind != expected_kind));

            let outcome = fixture.runtime(model.clone()).resume(run_id).unwrap();
            assert!(matches!(outcome, AgentRuntimeOutcome::Completed { .. }));
            assert_eq!(fixture.count.load(Ordering::SeqCst), expected_tool_count);
            assert_eq!(model.requests.lock().unwrap().len(), 2);
            let result_events = fixture
                .agent_store
                .list_agent_run_events(&run_id)
                .unwrap()
                .into_iter()
                .filter(|event| event.kind == expected_kind)
                .collect::<Vec<_>>();
            assert_eq!(result_events.len(), 1);
            let second_request = model.requests.lock().unwrap()[1].clone();
            let ModelInput::ToolOutput { output, .. } = &second_request.input else {
                panic!("continuation did not use the repaired tool output")
            };
            assert_eq!(output["ok"], expected_ok);

            let checkpoints_before = fixture.run_store.list_agent_checkpoints(&run_id).unwrap();
            let events_before = fixture.agent_store.list_agent_run_events(&run_id).unwrap();
            assert!(matches!(
                fixture.runtime(model.clone()).resume(run_id).unwrap(),
                AgentRuntimeOutcome::Completed { .. }
            ));
            assert_eq!(fixture.count.load(Ordering::SeqCst), expected_tool_count);
            assert_eq!(model.requests.lock().unwrap().len(), 2);
            assert_eq!(
                fixture.run_store.list_agent_checkpoints(&run_id).unwrap(),
                checkpoints_before
            );
            assert_eq!(
                fixture.agent_store.list_agent_run_events(&run_id).unwrap(),
                events_before
            );
        }
    }

    #[test]
    fn generic_resume_rejects_substituted_tool_result_chronology_before_dispatch() {
        for (mutation, failed_result) in [
            (GenericEvidenceMutation::CheckpointNextInput, false),
            (GenericEvidenceMutation::ToolEventStep, false),
            (GenericEvidenceMutation::ToolEventCall, false),
            (GenericEvidenceMutation::ToolEventOperation, false),
            (GenericEvidenceMutation::ToolEventProof, false),
            (GenericEvidenceMutation::StepProof, false),
            (GenericEvidenceMutation::CheckpointNextInput, true),
            (GenericEvidenceMutation::ToolEventStep, true),
            (GenericEvidenceMutation::ToolEventCall, true),
            (GenericEvidenceMutation::ToolEventError, true),
        ] {
            let fixture = Fixture::new(Governance::AgentExecutable, AgentLimits::default());
            let first_turn = if failed_result {
                ModelTurn {
                    response_id: "resp_invalid_tool".to_string(),
                    returned_model: None,
                    response_body_digest: None,
                    decision: ModelDecision::ToolCall {
                        call_id: "call_1".to_string(),
                        name: "proof_commerce_v1_catalog_create".to_string(),
                        arguments: json!({}),
                    },
                    usage: ModelUsage {
                        input_tokens: 10,
                        output_tokens: 5,
                        total_tokens: 15,
                        cost_microusd: Some(25),
                    },
                }
            } else {
                tool_turn()
            };
            let fault = if failed_result {
                GenericBootstrapFault::AfterToolFailedEventSave
            } else {
                GenericBootstrapFault::AfterToolSucceededEventSave
            };
            let expected_tool_count = usize::from(!failed_result);
            let model = Arc::new(ScriptedGateway::new(vec![first_turn, finish_turn()]));
            let faulting = Arc::new(FaultingGenericBootstrapStore::new(
                fixture.run_store.clone(),
                fixture.agent_store.clone(),
                fault,
            ));
            fixture
                .runtime_with_factory(
                    model.clone(),
                    faulting.clone(),
                    faulting,
                    Arc::new(GenericFactorySpy {
                        creates: AtomicUsize::new(0),
                        gateway: model.clone(),
                    }),
                )
                .start(fixture.agent.id, "Reject substituted tool result")
                .expect_err("post-result fault did not interrupt the run");
            let run_id = fixture.run_store.list_agent_runs().unwrap()[0].id;
            let checkpoints_before = fixture.run_store.list_agent_checkpoints(&run_id).unwrap();
            let events_before = fixture.agent_store.list_agent_run_events(&run_id).unwrap();
            let adversarial = Arc::new(AdversarialGenericEvidenceStore::new(
                fixture.run_store.clone(),
                fixture.agent_store.clone(),
                mutation,
            ));
            let factory = Arc::new(GenericFactorySpy {
                creates: AtomicUsize::new(0),
                gateway: model.clone(),
            });

            let error = fixture
                .runtime_with_factory(
                    model.clone(),
                    adversarial.clone(),
                    adversarial,
                    factory.clone(),
                )
                .resume(run_id)
                .expect_err("substituted tool result was allowed to continue");

            assert!(matches!(
                error,
                AgentRuntimeError::InvalidCheckpoint(_) | AgentRuntimeError::InconsistentState(_)
            ));
            assert_eq!(factory.creates.load(Ordering::SeqCst), 0);
            assert_eq!(model.requests.lock().unwrap().len(), 1);
            assert_eq!(fixture.count.load(Ordering::SeqCst), expected_tool_count);
            assert_eq!(
                fixture.run_store.list_agent_checkpoints(&run_id).unwrap(),
                checkpoints_before
            );
            assert_eq!(
                fixture.agent_store.list_agent_run_events(&run_id).unwrap(),
                events_before
            );
        }
    }

    #[test]
    fn generic_pending_checkpoint_binds_exact_call_tool_and_arguments() {
        for mutation in [
            GenericEvidenceMutation::PendingCall,
            GenericEvidenceMutation::PendingTool,
            GenericEvidenceMutation::PendingArguments,
        ] {
            let fixture = Fixture::new(Governance::HumanOnly, AgentLimits::default());
            let model = Arc::new(ScriptedGateway::new(vec![tool_turn(), finish_turn()]));
            let waiting = fixture
                .runtime(model.clone())
                .start(fixture.agent.id, "Reject substituted pending call")
                .unwrap();
            let AgentRuntimeOutcome::WaitingForApproval { run, .. } = waiting else {
                panic!("expected approval pause")
            };
            let checkpoints_before = fixture.run_store.list_agent_checkpoints(&run.id).unwrap();
            let events_before = fixture.agent_store.list_agent_run_events(&run.id).unwrap();
            let adversarial = Arc::new(AdversarialGenericEvidenceStore::new(
                fixture.run_store.clone(),
                fixture.agent_store.clone(),
                mutation,
            ));
            let factory = Arc::new(GenericFactorySpy {
                creates: AtomicUsize::new(0),
                gateway: model.clone(),
            });

            let error = fixture
                .runtime_with_factory(
                    model.clone(),
                    adversarial.clone(),
                    adversarial,
                    factory.clone(),
                )
                .resume(run.id)
                .expect_err("substituted pending tool checkpoint was accepted");

            assert!(matches!(
                error,
                AgentRuntimeError::InvalidCheckpoint(_) | AgentRuntimeError::InconsistentState(_)
            ));
            assert_eq!(factory.creates.load(Ordering::SeqCst), 0);
            assert_eq!(model.requests.lock().unwrap().len(), 1);
            assert_eq!(fixture.count.load(Ordering::SeqCst), 0);
            assert_eq!(
                fixture.run_store.list_agent_checkpoints(&run.id).unwrap(),
                checkpoints_before
            );
            assert_eq!(
                fixture.agent_store.list_agent_run_events(&run.id).unwrap(),
                events_before
            );
        }
    }

    #[test]
    fn generic_approval_revision_rejects_a_forged_event_pair_and_checkpoint_binding() {
        let fixture = Fixture::new(Governance::HumanOnly, AgentLimits::default());
        let model = Arc::new(ScriptedGateway::new(vec![tool_turn(), finish_turn()]));
        let factory = Arc::new(GenericFactorySpy {
            creates: AtomicUsize::new(0),
            gateway: model.clone(),
        });
        let faulting = Arc::new(FaultingGenericBootstrapStore::new(
            fixture.run_store.clone(),
            fixture.agent_store.clone(),
            GenericBootstrapFault::AfterApprovalRunResumeSave,
        ));
        let runtime = fixture.runtime_with_factory(
            model.clone(),
            faulting.clone(),
            faulting,
            factory.clone(),
        );
        let waiting = runtime
            .start(fixture.agent.id, "Reject forged approval pair")
            .unwrap();
        let AgentRuntimeOutcome::WaitingForApproval { run, request, .. } = waiting else {
            panic!("expected approval pause")
        };
        let approver = generate_keypair_for(PrincipalKind::Human);
        fixture
            .approval_store
            .trust_approver(principal_from_keypair(&approver))
            .unwrap();
        fixture
            .approval_store
            .save_approval_decision(
                &SignedApprovalDecision::create(
                    &request,
                    ApprovalOutcome::Approved,
                    None,
                    Utc::now(),
                    &approver,
                )
                .unwrap(),
            )
            .unwrap();
        runtime
            .resume(run.id)
            .expect_err("post-approval run-save fault did not interrupt resume");
        let persisted = fixture.run_store.load_agent_run(&run.id).unwrap().unwrap();
        assert_eq!(persisted.status, AgentRunStatus::Running);
        assert_eq!(persisted.revision, 3);
        assert_eq!(fixture.count.load(Ordering::SeqCst), 0);
        assert_eq!(model.requests.lock().unwrap().len(), 1);
        let checkpoints_before = fixture.run_store.list_agent_checkpoints(&run.id).unwrap();
        let events_before = fixture.agent_store.list_agent_run_events(&run.id).unwrap();
        let adversarial = Arc::new(AdversarialGenericEvidenceStore::new(
            fixture.run_store.clone(),
            fixture.agent_store.clone(),
            GenericEvidenceMutation::ForgedApprovalPair,
        ));

        let error = fixture
            .runtime_with_factory(
                model.clone(),
                adversarial.clone(),
                adversarial,
                factory.clone(),
            )
            .resume(run.id)
            .expect_err("forged approval event pair authorized the run revision");

        assert!(matches!(error, AgentRuntimeError::InconsistentState(_)));
        assert_eq!(factory.creates.load(Ordering::SeqCst), 0);
        assert_eq!(model.requests.lock().unwrap().len(), 1);
        assert_eq!(fixture.count.load(Ordering::SeqCst), 0);
        assert_eq!(
            fixture.run_store.list_agent_checkpoints(&run.id).unwrap(),
            checkpoints_before
        );
        assert_eq!(
            fixture.agent_store.list_agent_run_events(&run.id).unwrap(),
            events_before
        );
    }

    #[test]
    fn generic_bootstrap_corruption_matrix_fails_before_factory_gateway_or_tool() {
        {
            let fixture = Fixture::new(Governance::AgentExecutable, AgentLimits::default());
            let mut run = AgentRun::new_for_agent(
                PrincipalId::now(),
                fixture.agent.id,
                AgentRunMode::Session,
                "wrong actor",
                Utc::now(),
            )
            .unwrap();
            run.mode = AgentRunMode::Session;
            fixture.run_store.save_agent_run(&run).unwrap();
            assert_generic_bootstrap_rejected_before_dispatch(&fixture, run.id);
        }
        {
            let fixture = Fixture::new(Governance::AgentExecutable, AgentLimits::default());
            let mut run = AgentRun::new_for_agent(
                fixture.identity.principal_id,
                fixture.agent.id,
                AgentRunMode::Session,
                "wrong mode",
                Utc::now(),
            )
            .unwrap();
            run.mode = AgentRunMode::OneShot;
            fixture.run_store.save_agent_run(&run).unwrap();
            assert_generic_bootstrap_rejected_before_dispatch(&fixture, run.id);
        }
        {
            let fixture = Fixture::new(Governance::AgentExecutable, AgentLimits::default());
            let mut run = AgentRun::new_for_agent(
                fixture.identity.principal_id,
                fixture.agent.id,
                AgentRunMode::Session,
                "goal will be cleared",
                Utc::now(),
            )
            .unwrap();
            run.goal.clear();
            fixture.run_store.save_agent_run(&run).unwrap();
            assert_generic_bootstrap_rejected_before_dispatch(&fixture, run.id);
        }
        {
            let fixture = Fixture::new(Governance::AgentExecutable, AgentLimits::default());
            let mut run = save_pristine_generic_run(&fixture, true);
            run.updated_at += Duration::seconds(1);
            run.revision += 1;
            fixture.run_store.save_agent_run(&run).unwrap();
            assert_generic_bootstrap_rejected_before_dispatch(&fixture, run.id);
        }
        {
            let fixture = Fixture::new(Governance::AgentExecutable, AgentLimits::default());
            let run = save_pristine_generic_run(&fixture, true);
            let mut state = generic_initial_state(&run, &fixture.agent);
            state.agent_id = Uuid::now_v7();
            let checkpoint = AgentCheckpoint::create(
                run.id,
                0,
                json!({"kind": RUNTIME_CHECKPOINT_KIND, "runtime": state}),
                Utc::now(),
            )
            .unwrap();
            fixture
                .run_store
                .save_agent_checkpoint(&checkpoint)
                .unwrap();
            assert_generic_bootstrap_rejected_before_dispatch(&fixture, run.id);
        }
        {
            let fixture = Fixture::new(Governance::AgentExecutable, AgentLimits::default());
            let run = save_pristine_generic_run(&fixture, true);
            let first = save_generic_initial_checkpoint(&fixture, &run);
            let duplicate = AgentCheckpoint::create(
                run.id,
                1,
                first.state,
                first.created_at + Duration::microseconds(1),
            )
            .unwrap();
            fixture.run_store.save_agent_checkpoint(&duplicate).unwrap();
            assert_generic_bootstrap_rejected_before_dispatch(&fixture, run.id);
        }
        {
            let fixture = Fixture::new(Governance::AgentExecutable, AgentLimits::default());
            let run = save_pristine_generic_run(&fixture, true);
            let checkpoint = save_generic_initial_checkpoint(&fixture, &run);
            save_generic_started_event(&fixture, &run, &checkpoint, 0, AgentRunEventKind::Started);
            save_generic_started_event(&fixture, &run, &checkpoint, 1, AgentRunEventKind::Started);
            assert_generic_bootstrap_rejected_before_dispatch(&fixture, run.id);
        }
        {
            let fixture = Fixture::new(Governance::AgentExecutable, AgentLimits::default());
            let run = save_pristine_generic_run(&fixture, true);
            let checkpoint = save_generic_initial_checkpoint(&fixture, &run);
            save_generic_started_event(
                &fixture,
                &run,
                &checkpoint,
                0,
                AgentRunEventKind::ModelRequested,
            );
            assert_generic_bootstrap_rejected_before_dispatch(&fixture, run.id);
        }
        {
            let fixture = Fixture::new(Governance::AgentExecutable, AgentLimits::default());
            let run = save_pristine_generic_run(&fixture, true);
            let checkpoint = save_generic_initial_checkpoint(&fixture, &run);
            save_generic_started_event(&fixture, &run, &checkpoint, 0, AgentRunEventKind::Started);
            save_generic_started_event(
                &fixture,
                &run,
                &checkpoint,
                1,
                AgentRunEventKind::ModelRequested,
            );
            assert_generic_bootstrap_rejected_before_dispatch(&fixture, run.id);
        }
        {
            let fixture = Fixture::new(Governance::AgentExecutable, AgentLimits::default());
            let run = save_pristine_generic_run(&fixture, true);
            let checkpoint = save_generic_initial_checkpoint(&fixture, &run);
            let mut event = AgentRunEvent::create(
                run.id,
                0,
                AgentRunEventKind::Started,
                generic_started_event(&fixture.agent, &run),
                Utc::now(),
            )
            .unwrap();
            event.data_digest = value_digest(&json!("corrupt event digest")).unwrap();
            assert!(event.created_at >= checkpoint.created_at);
            fixture.agent_store.save_agent_run_event(&event).unwrap();
            assert_generic_bootstrap_rejected_before_dispatch(&fixture, run.id);
        }
        {
            let fixture = Fixture::new(Governance::AgentExecutable, AgentLimits::default());
            let run = save_pristine_generic_run(&fixture, true);
            let mut step = AgentRunStep::new(
                run.id,
                0,
                "catalog.create",
                "v1",
                &json!({"name": "orphan"}),
                Utc::now(),
            )
            .unwrap();
            fixture.run_store.save_agent_run_step(&step).unwrap();
            step.start(Utc::now()).unwrap();
            fixture.run_store.save_agent_run_step(&step).unwrap();
            assert_generic_bootstrap_rejected_before_dispatch(&fixture, run.id);
        }
        {
            let fixture = Fixture::new(Governance::AgentExecutable, AgentLimits::default());
            let run = save_pristine_generic_run(&fixture, true);
            let checkpoint = save_generic_initial_checkpoint(&fixture, &run);
            save_generic_started_event(&fixture, &run, &checkpoint, 0, AgentRunEventKind::Started);
            let step = AgentRunStep::new(
                run.id,
                0,
                "catalog.create",
                "v1",
                &json!({"name": "post-barrier orphan"}),
                Utc::now(),
            )
            .unwrap();
            fixture.run_store.save_agent_run_step(&step).unwrap();
            assert_generic_bootstrap_rejected_before_dispatch(&fixture, run.id);
        }
        {
            let fixture = Fixture::new(Governance::AgentExecutable, AgentLimits::default());
            let run = save_pristine_generic_run(&fixture, true);
            let evaluation = AgentRunEvaluation {
                id: Uuid::now_v7(),
                run_id: run.id,
                evaluator: "corrupt-bootstrap".to_string(),
                outcome: AgentEvaluationOutcome::Passed,
                score_bps: Some(10_000),
                metrics: json!({}),
                summary: None,
                created_at: Utc::now(),
            };
            fixture
                .run_store
                .save_agent_run_evaluation(&evaluation)
                .unwrap();
            assert_generic_bootstrap_rejected_before_dispatch(&fixture, run.id);
        }
        {
            let fixture = Fixture::new(Governance::AgentExecutable, AgentLimits::default());
            let run = save_pristine_generic_run(&fixture, true);
            let now = Utc::now();
            let request = SignedApprovalRequest::create(
                "catalog.create",
                "v1",
                &json!({"name": "orphan"}),
                now,
                now + Duration::minutes(1),
                &fixture.identity,
            )
            .unwrap();
            fixture
                .approval_store
                .save_approval_request(&request)
                .unwrap();
            let mut step = AgentRunStep::new(
                run.id,
                0,
                "catalog.create",
                "v1",
                &json!({"name": "orphan"}),
                now,
            )
            .unwrap();
            fixture.run_store.save_agent_run_step(&step).unwrap();
            step.start(now).unwrap();
            fixture.run_store.save_agent_run_step(&step).unwrap();
            step.wait_for_approval(request.body.id, now).unwrap();
            fixture.run_store.save_agent_run_step(&step).unwrap();
            let output = json!({"operation": "catalog.create"});
            let proof = create_proof(
                fixture.identity.principal_id,
                None,
                "catalog.create::v1",
                &json!({"name": "orphan"}),
                &output,
                now,
                &fixture.identity,
            )
            .unwrap();
            fixture
                .approval_store
                .save_approval_execution(&ApprovalExecution {
                    request_id: request.body.id,
                    executed_at: now,
                    output,
                    proof,
                })
                .unwrap();
            assert_generic_bootstrap_rejected_before_dispatch(&fixture, run.id);
        }
        {
            let fixture = Fixture::new(Governance::AgentExecutable, AgentLimits::default());
            let run = save_pristine_generic_run(&fixture, true);
            let mut checkpoint = AgentCheckpoint::create(
                run.id,
                0,
                json!({
                    "kind": RUNTIME_CHECKPOINT_KIND,
                    "runtime": generic_initial_state(&run, &fixture.agent),
                }),
                Utc::now(),
            )
            .unwrap();
            checkpoint.state_digest = value_digest(&json!("corrupt checkpoint digest")).unwrap();
            fixture
                .run_store
                .save_agent_checkpoint(&checkpoint)
                .unwrap();
            assert_generic_bootstrap_rejected_before_dispatch(&fixture, run.id);
        }
    }

    #[test]
    fn generic_completed_bootstrap_rejects_run_and_future_barrier_drift() {
        {
            let fixture = Fixture::new(Governance::AgentExecutable, AgentLimits::default());
            let mut run = save_pristine_generic_run(&fixture, true);
            let checkpoint = save_generic_initial_checkpoint(&fixture, &run);
            save_generic_started_event(&fixture, &run, &checkpoint, 0, AgentRunEventKind::Started);
            run.revision = 2;
            fixture.run_store.save_agent_run(&run).unwrap();
            assert_generic_bootstrap_rejected_before_dispatch(&fixture, run.id);
        }
        {
            let fixture = Fixture::new(Governance::AgentExecutable, AgentLimits::default());
            let created_at = std::cmp::max(Utc::now(), fixture.agent.created_at);
            let mut run = AgentRun::new_for_agent(
                fixture.identity.principal_id,
                fixture.agent.id,
                AgentRunMode::Session,
                "updated-at drift",
                created_at,
            )
            .unwrap();
            fixture.run_store.save_agent_run(&run).unwrap();
            run.start(created_at + Duration::nanoseconds(1)).unwrap();
            fixture.run_store.save_agent_run(&run).unwrap();
            let checkpoint = save_generic_initial_checkpoint(&fixture, &run);
            save_generic_started_event(&fixture, &run, &checkpoint, 0, AgentRunEventKind::Started);
            assert_generic_bootstrap_rejected_before_dispatch(&fixture, run.id);
        }
        {
            let fixture = Fixture::new(Governance::AgentExecutable, AgentLimits::default());
            let run = save_pristine_generic_run(&fixture, true);
            let state = generic_initial_state(&run, &fixture.agent);
            let checkpoint = AgentCheckpoint::create(
                run.id,
                0,
                json!({"kind": RUNTIME_CHECKPOINT_KIND, "runtime": state}),
                Utc::now() + Duration::hours(1),
            )
            .unwrap();
            fixture
                .run_store
                .save_agent_checkpoint(&checkpoint)
                .unwrap();
            let event = AgentRunEvent::create(
                run.id,
                0,
                AgentRunEventKind::Started,
                generic_started_event(&fixture.agent, &run),
                checkpoint.created_at + Duration::seconds(1),
            )
            .unwrap();
            fixture.agent_store.save_agent_run_event(&event).unwrap();
            assert_generic_bootstrap_rejected_before_dispatch(&fixture, run.id);
        }
        {
            let fixture = Fixture::new(Governance::AgentExecutable, AgentLimits::default());
            let run = save_pristine_generic_run(&fixture, true);
            let checkpoint = save_generic_initial_checkpoint(&fixture, &run);
            let event = AgentRunEvent::create(
                run.id,
                0,
                AgentRunEventKind::Started,
                generic_started_event(&fixture.agent, &run),
                Utc::now() + Duration::hours(1),
            )
            .unwrap();
            assert!(event.created_at > checkpoint.created_at);
            fixture.agent_store.save_agent_run_event(&event).unwrap();
            assert_generic_bootstrap_rejected_before_dispatch(&fixture, run.id);
        }
    }

    #[test]
    fn generic_later_checkpoint_requires_matching_model_state_machine_evidence() {
        let fixture = Fixture::new(Governance::AgentExecutable, AgentLimits::default());
        let run = save_pristine_generic_run(&fixture, true);
        let checkpoint = save_generic_initial_checkpoint(&fixture, &run);
        save_generic_started_event(&fixture, &run, &checkpoint, 0, AgentRunEventKind::Started);
        let mut arbitrary = generic_initial_state(&run, &fixture.agent);
        arbitrary.model_calls = 1;
        fixture
            .run_store
            .save_agent_checkpoint(
                &AgentCheckpoint::create(
                    run.id,
                    1,
                    json!({"kind": RUNTIME_CHECKPOINT_KIND, "runtime": arbitrary}),
                    Utc::now(),
                )
                .unwrap(),
            )
            .unwrap();

        assert_generic_bootstrap_rejected_before_dispatch(&fixture, run.id);
    }

    #[test]
    fn generic_resume_resolves_tools_before_any_bootstrap_repair_write() {
        let fixture = Fixture::new(Governance::AgentExecutable, AgentLimits::default());
        let unresolvable = AgentDefinition::new(
            "unresolvable-bootstrap-agent",
            "This definition has no registry contract.",
            "openai",
            "test-model",
            vec![AgentTool::new("missing.call", "v1").unwrap()],
            AgentLimits::default(),
            Utc::now(),
        )
        .unwrap();
        fixture
            .agent_store
            .save_agent_definition(&unresolvable)
            .unwrap();
        let run = AgentRun::new_for_agent(
            fixture.identity.principal_id,
            unresolvable.id,
            AgentRunMode::Session,
            "must stay queued",
            Utc::now(),
        )
        .unwrap();
        fixture.run_store.save_agent_run(&run).unwrap();
        let model = Arc::new(ScriptedGateway::new(vec![finish_turn()]));
        let factory = Arc::new(GenericFactorySpy {
            creates: AtomicUsize::new(0),
            gateway: model.clone(),
        });

        let error = fixture
            .runtime_with_factory(
                model.clone(),
                fixture.run_store.clone(),
                fixture.agent_store.clone(),
                factory.clone(),
            )
            .resume(run.id)
            .expect_err("unresolvable agent acquired bootstrap evidence");

        assert!(matches!(error, AgentRuntimeError::ToolNotRegistered { .. }));
        assert_eq!(
            fixture.run_store.load_agent_run(&run.id).unwrap(),
            Some(run.clone())
        );
        assert!(fixture
            .run_store
            .list_agent_checkpoints(&run.id)
            .unwrap()
            .is_empty());
        assert!(fixture
            .agent_store
            .list_agent_run_events(&run.id)
            .unwrap()
            .is_empty());
        assert_eq!(factory.creates.load(Ordering::SeqCst), 0);
        assert!(model.requests.lock().unwrap().is_empty());
        assert_eq!(fixture.count.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn executes_tool_loop_with_proof_checkpoint_and_evaluation() {
        let fixture = Fixture::new(Governance::AgentExecutable, AgentLimits::default());
        let model = Arc::new(ScriptedGateway::new(vec![tool_turn(), finish_turn()]));
        let runtime = fixture.runtime(model.clone());

        let outcome = runtime.start(fixture.agent.id, "Create Spring").unwrap();

        let AgentRuntimeOutcome::Completed {
            run,
            output,
            evaluation,
        } = outcome
        else {
            panic!("expected completed outcome")
        };
        assert_eq!(output, "Catalog created.");
        assert_eq!(run.status, AgentRunStatus::Succeeded);
        assert_eq!(evaluation.outcome, AgentEvaluationOutcome::Passed);
        assert_eq!(fixture.count.load(Ordering::SeqCst), 1);
        let steps = fixture.run_store.list_agent_run_steps(&run.id).unwrap();
        assert_eq!(steps.len(), 1);
        assert!(steps[0].proof.is_some());
        let state = runtime.state(run.id).unwrap();
        assert_eq!(state.model_calls, 2);
        assert_eq!(state.total_tokens, 26);
        let requests = model.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests[1].previous_response_id.as_deref(),
            Some("resp_tool")
        );
        assert!(matches!(requests[1].input, ModelInput::ToolOutput { .. }));
    }

    #[test]
    fn pauses_and_resumes_a_signed_human_approval_after_restart() {
        let fixture = Fixture::new(Governance::HumanOnly, AgentLimits::default());
        let model = Arc::new(ScriptedGateway::new(vec![tool_turn(), finish_turn()]));
        let first_runtime = fixture.runtime(model.clone());
        let waiting = first_runtime
            .start(fixture.agent.id, "Create Spring")
            .unwrap();
        let AgentRuntimeOutcome::WaitingForApproval { run, request, .. } = waiting else {
            panic!("expected approval pause")
        };
        assert_eq!(run.status, AgentRunStatus::WaitingForInput);
        assert_eq!(fixture.count.load(Ordering::SeqCst), 0);

        let approver = generate_keypair_for(PrincipalKind::Human);
        fixture
            .approval_store
            .trust_approver(principal_from_keypair(&approver))
            .unwrap();
        let decision = SignedApprovalDecision::create(
            &request,
            ApprovalOutcome::Approved,
            Some("reviewed".to_string()),
            Utc::now(),
            &approver,
        )
        .unwrap();
        fixture
            .approval_store
            .save_approval_decision(&decision)
            .unwrap();

        let restarted_runtime = fixture.runtime(model);
        let resumed = restarted_runtime.resume(run.id).unwrap();

        assert!(matches!(resumed, AgentRuntimeOutcome::Completed { .. }));
        assert_eq!(fixture.count.load(Ordering::SeqCst), 1);
        assert!(fixture
            .approval_store
            .load_approval_execution(&request.body.id)
            .unwrap()
            .is_some());
    }

    #[test]
    fn fails_closed_when_approval_lookup_resolves_to_another_step() {
        let fixture = Fixture::new(Governance::HumanOnly, AgentLimits::default());
        let model = Arc::new(ScriptedGateway::new(vec![tool_turn(), finish_turn()]));
        let waiting = fixture
            .runtime(model.clone())
            .start(fixture.agent.id, "Create Spring")
            .unwrap();
        let AgentRuntimeOutcome::WaitingForApproval { run, step, .. } = waiting else {
            panic!("expected approval pause")
        };
        let mut wrong_step = step;
        wrong_step.id = Uuid::now_v7();
        let request_id = wrong_step.approval_request_id.unwrap();
        let misdirected_store = Arc::new(AdversarialAgentRunStore {
            inner: fixture.run_store.clone(),
            resolved_step: Some(wrong_step),
            reject_approval_claim: false,
            reject_checkpoints: None,
        });

        let outcome = fixture
            .runtime_with_run_store(model, misdirected_store)
            .resume(run.id)
            .unwrap();

        let AgentRuntimeOutcome::Failed { error, .. } = outcome else {
            panic!("expected fail-closed outcome")
        };
        assert!(error.contains("resolved to step"));
        assert_eq!(fixture.count.load(Ordering::SeqCst), 0);
        assert!(fixture
            .approval_store
            .load_approval_execution(&request_id)
            .unwrap()
            .is_none());
    }

    #[test]
    fn losing_approval_claim_emits_no_resumed_event_and_executes_nothing() {
        let fixture = Fixture::new(Governance::HumanOnly, AgentLimits::default());
        let model = Arc::new(ScriptedGateway::new(vec![tool_turn(), finish_turn()]));
        let waiting = fixture
            .runtime(model.clone())
            .start(fixture.agent.id, "Create Spring")
            .unwrap();
        let AgentRuntimeOutcome::WaitingForApproval { run, request, .. } = waiting else {
            panic!("expected approval pause")
        };
        let approver = generate_keypair_for(PrincipalKind::Human);
        fixture
            .approval_store
            .trust_approver(principal_from_keypair(&approver))
            .unwrap();
        let decision = SignedApprovalDecision::create(
            &request,
            ApprovalOutcome::Approved,
            None,
            Utc::now(),
            &approver,
        )
        .unwrap();
        fixture
            .approval_store
            .save_approval_decision(&decision)
            .unwrap();
        let rejecting_store = Arc::new(AdversarialAgentRunStore {
            inner: fixture.run_store.clone(),
            resolved_step: None,
            reject_approval_claim: true,
            reject_checkpoints: None,
        });

        let error = fixture
            .runtime_with_run_store(model, rejecting_store)
            .resume(run.id)
            .unwrap_err();

        assert!(error.to_string().contains("simulated lost approval claim"));
        assert_eq!(fixture.count.load(Ordering::SeqCst), 0);
        assert!(fixture
            .agent_store
            .list_agent_run_events(&run.id)
            .unwrap()
            .iter()
            .all(|event| event.kind != AgentRunEventKind::ApprovalResumed));
        assert!(fixture
            .approval_store
            .load_approval_execution(&request.body.id)
            .unwrap()
            .is_none());
    }

    #[test]
    fn losing_denial_claim_emits_no_resumed_event() {
        let fixture = Fixture::new(Governance::HumanOnly, AgentLimits::default());
        let model = Arc::new(ScriptedGateway::new(vec![tool_turn(), finish_turn()]));
        let waiting = fixture
            .runtime(model.clone())
            .start(fixture.agent.id, "Create Spring")
            .unwrap();
        let AgentRuntimeOutcome::WaitingForApproval { run, request, .. } = waiting else {
            panic!("expected approval pause")
        };
        let approver = generate_keypair_for(PrincipalKind::Human);
        fixture
            .approval_store
            .trust_approver(principal_from_keypair(&approver))
            .unwrap();
        let decision = SignedApprovalDecision::create(
            &request,
            ApprovalOutcome::Denied,
            Some("not safe".to_string()),
            Utc::now(),
            &approver,
        )
        .unwrap();
        fixture
            .approval_store
            .save_approval_decision(&decision)
            .unwrap();
        let rejecting_store = Arc::new(AdversarialAgentRunStore {
            inner: fixture.run_store.clone(),
            resolved_step: None,
            reject_approval_claim: true,
            reject_checkpoints: None,
        });

        let error = fixture
            .runtime_with_run_store(model, rejecting_store)
            .resume(run.id)
            .unwrap_err();

        assert!(error.to_string().contains("simulated lost approval claim"));
        assert_eq!(fixture.count.load(Ordering::SeqCst), 0);
        assert!(fixture
            .agent_store
            .list_agent_run_events(&run.id)
            .unwrap()
            .iter()
            .all(|event| event.kind != AgentRunEventKind::ApprovalResumed));
    }

    #[test]
    fn reconciles_a_persisted_approval_execution_without_replaying_the_tool() {
        let fixture = Fixture::new(Governance::HumanOnly, AgentLimits::default());
        let model = Arc::new(ScriptedGateway::new(vec![tool_turn(), finish_turn()]));
        let waiting = fixture
            .runtime(model.clone())
            .start(fixture.agent.id, "Create Spring")
            .unwrap();
        let AgentRuntimeOutcome::WaitingForApproval { run, request, .. } = waiting else {
            panic!("expected approval pause")
        };
        let approver = generate_keypair_for(PrincipalKind::Human);
        fixture
            .approval_store
            .trust_approver(principal_from_keypair(&approver))
            .unwrap();
        let decision = SignedApprovalDecision::create(
            &request,
            ApprovalOutcome::Approved,
            None,
            Utc::now(),
            &approver,
        )
        .unwrap();
        fixture
            .approval_store
            .save_approval_decision(&decision)
            .unwrap();
        let state = fixture.runtime(model.clone()).state(run.id).unwrap();
        let pending = state.pending_tool.unwrap();
        let output = json!({"echo": pending.arguments});
        let executed_at = Utc::now();
        let proof = create_proof(
            fixture.identity.principal_id,
            None,
            &format!("{}::{}", pending.operation, pending.version),
            &pending.arguments,
            &output,
            executed_at,
            &fixture.identity,
        )
        .unwrap();
        fixture
            .approval_store
            .save_approval_execution(&ApprovalExecution {
                request_id: request.body.id,
                executed_at,
                output,
                proof,
            })
            .unwrap();

        let resumed = fixture.runtime(model).resume(run.id).unwrap();

        assert!(matches!(resumed, AgentRuntimeOutcome::Completed { .. }));
        assert_eq!(fixture.count.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn approval_resume_after_duration_budget_executes_nothing_and_replays_read_only() {
        let mut limits = AgentLimits::default();
        limits.max_duration_seconds = 60;
        let fixture = Fixture::new(Governance::HumanOnly, limits);
        let model = Arc::new(ScriptedGateway::new(vec![tool_turn(), finish_turn()]));
        let checkpoint_sealed = Arc::new(AtomicBool::new(false));
        let sealed_store = Arc::new(AdversarialAgentRunStore {
            inner: fixture.run_store.clone(),
            resolved_step: None,
            reject_approval_claim: false,
            reject_checkpoints: Some(checkpoint_sealed.clone()),
        });
        let runtime = fixture.runtime_with_run_store(model, sealed_store);
        let waiting = runtime.start(fixture.agent.id, "Create Spring").unwrap();
        let AgentRuntimeOutcome::WaitingForApproval { run, request, .. } = waiting else {
            panic!("expected approval pause")
        };
        let mut state = runtime.state(run.id).unwrap();
        assert_eq!(
            request.body.expires_at,
            run_deadline(state.started_at, fixture.agent.limits.max_duration_seconds)
        );

        let approver = generate_keypair_for(PrincipalKind::Human);
        fixture
            .approval_store
            .trust_approver(principal_from_keypair(&approver))
            .unwrap();
        let decision = SignedApprovalDecision::create(
            &request,
            ApprovalOutcome::Approved,
            None,
            Utc::now(),
            &approver,
        )
        .unwrap();
        fixture
            .approval_store
            .save_approval_decision(&decision)
            .unwrap();
        state.started_at = Utc::now() - Duration::seconds(61);
        runtime.save_state(run.id, &state).unwrap();

        let outcome = runtime.resume(run.id).unwrap();

        let AgentRuntimeOutcome::Failed {
            run,
            error,
            evaluation,
        } = outcome
        else {
            panic!("expected duration budget failure")
        };
        assert_eq!(run.status, AgentRunStatus::Failed);
        assert!(error.contains("duration budget exceeded"));
        assert_eq!(fixture.count.load(Ordering::SeqCst), 0);
        assert!(fixture
            .approval_store
            .load_approval_execution(&request.body.id)
            .unwrap()
            .is_none());
        assert_eq!(
            fixture.run_store.list_agent_run_steps(&run.id).unwrap()[0].status,
            AgentRunStepStatus::Failed
        );
        let events = fixture.agent_store.list_agent_run_events(&run.id).unwrap();
        assert!(events
            .iter()
            .all(|event| event.kind != AgentRunEventKind::ApprovalResumed));
        assert_eq!(
            events
                .iter()
                .filter(|event| event.kind == AgentRunEventKind::BudgetExceeded)
                .count(),
            1
        );
        let checkpoint_count = fixture
            .run_store
            .list_agent_checkpoints(&run.id)
            .unwrap()
            .len();
        let event_count = events.len();
        let evaluation_count = fixture
            .run_store
            .list_agent_run_evaluations(&run.id)
            .unwrap()
            .len();
        checkpoint_sealed.store(true, Ordering::SeqCst);

        let replay = runtime.resume(run.id).unwrap();

        let AgentRuntimeOutcome::Failed {
            evaluation: replayed_evaluation,
            error: replayed_error,
            ..
        } = replay
        else {
            panic!("expected replayed duration budget failure")
        };
        assert_eq!(replayed_error, error);
        assert_eq!(replayed_evaluation.id, evaluation.id);
        assert_eq!(
            fixture
                .run_store
                .list_agent_checkpoints(&run.id)
                .unwrap()
                .len(),
            checkpoint_count
        );
        assert_eq!(
            fixture
                .agent_store
                .list_agent_run_events(&run.id)
                .unwrap()
                .len(),
            event_count
        );
        assert_eq!(
            fixture
                .run_store
                .list_agent_run_evaluations(&run.id)
                .unwrap()
                .len(),
            evaluation_count
        );
    }

    #[test]
    fn failed_resume_replays_persisted_outcome_without_writes() {
        let fixture = Fixture::new(Governance::AgentExecutable, AgentLimits::default());
        let checkpoint_sealed = Arc::new(AtomicBool::new(false));
        let sealed_store = Arc::new(AdversarialAgentRunStore {
            inner: fixture.run_store.clone(),
            resolved_step: None,
            reject_approval_claim: false,
            reject_checkpoints: Some(checkpoint_sealed.clone()),
        });
        let runtime =
            fixture.runtime_with_run_store(Arc::new(ScriptedGateway::new(vec![])), sealed_store);
        let first = runtime.start(fixture.agent.id, "Create Spring").unwrap();
        let AgentRuntimeOutcome::Failed { run, .. } = &first else {
            panic!("expected model failure")
        };
        let checkpoint_count = fixture
            .run_store
            .list_agent_checkpoints(&run.id)
            .unwrap()
            .len();
        let event_count = fixture
            .agent_store
            .list_agent_run_events(&run.id)
            .unwrap()
            .len();
        let evaluation_count = fixture
            .run_store
            .list_agent_run_evaluations(&run.id)
            .unwrap()
            .len();
        checkpoint_sealed.store(true, Ordering::SeqCst);

        let replay = runtime.resume(run.id).unwrap();

        assert_eq!(replay, first);
        assert_eq!(
            fixture
                .run_store
                .list_agent_checkpoints(&run.id)
                .unwrap()
                .len(),
            checkpoint_count
        );
        assert_eq!(
            fixture
                .agent_store
                .list_agent_run_events(&run.id)
                .unwrap()
                .len(),
            event_count
        );
        assert_eq!(
            fixture
                .run_store
                .list_agent_run_evaluations(&run.id)
                .unwrap()
                .len(),
            evaluation_count
        );
    }

    #[test]
    fn preseal_budget_recovery_preserves_budget_terminal_kind() {
        let fixture = Fixture::new(Governance::AgentExecutable, AgentLimits::default());
        let runtime = fixture.runtime(Arc::new(ScriptedGateway::new(vec![])));
        let now = Utc::now();
        let mut run = AgentRun::new_for_agent(
            fixture.identity.principal_id,
            fixture.agent.id,
            AgentRunMode::Session,
            "Create Spring",
            now,
        )
        .unwrap();
        fixture.run_store.save_agent_run(&run).unwrap();
        run.start(now).unwrap();
        fixture.run_store.save_agent_run(&run).unwrap();
        runtime
            .save_state(run.id, &generic_initial_state(&run, &fixture.agent))
            .unwrap();
        runtime
            .append_event(
                run.id,
                AgentRunEventKind::Started,
                json!({"agent_id": fixture.agent.id, "goal": run.goal}),
            )
            .unwrap();
        let state = AgentRuntimeState {
            agent_id: fixture.agent.id,
            started_at: now,
            previous_response_id: None,
            next_input: ModelInput::Goal {
                text: run.goal.clone(),
            },
            pending_tool: None,
            model_calls: 0,
            tool_attempts: 0,
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: 0,
            cost_microusd: Some(0),
            final_output: None,
            terminal_error: Some("token budget exceeded: maximum 1".to_string()),
        };
        runtime
            .save_terminal_state(run.id, &state, AgentRunEventKind::BudgetExceeded)
            .unwrap();

        let outcome = runtime.resume(run.id).unwrap();

        assert!(matches!(outcome, AgentRuntimeOutcome::Failed { .. }));
        let events = fixture.agent_store.list_agent_run_events(&run.id).unwrap();
        assert!(events
            .iter()
            .any(|event| event.kind == AgentRunEventKind::BudgetExceeded));
        assert!(events
            .iter()
            .all(|event| event.kind != AgentRunEventKind::Failed));
    }

    #[test]
    fn fails_closed_when_model_usage_exceeds_token_budget() {
        let mut limits = AgentLimits::default();
        limits.max_total_tokens = 5;
        let fixture = Fixture::new(Governance::AgentExecutable, limits);
        let model = Arc::new(ScriptedGateway::new(vec![finish_turn()]));

        let outcome = fixture
            .runtime(model)
            .start(fixture.agent.id, "Create Spring")
            .unwrap();

        let AgentRuntimeOutcome::Failed { run, error, .. } = outcome else {
            panic!("expected failed outcome")
        };
        assert_eq!(run.status, AgentRunStatus::Failed);
        assert!(error.contains("token budget exceeded"));
        assert_eq!(fixture.count.load(Ordering::SeqCst), 0);
        assert!(fixture
            .agent_store
            .list_agent_run_events(&run.id)
            .unwrap()
            .iter()
            .any(|event| event.kind == AgentRunEventKind::BudgetExceeded));
    }

    #[test]
    fn constructor_rejects_a_human_runtime_identity() {
        let fixture = Fixture::new(Governance::AgentExecutable, AgentLimits::default());
        let human = generate_keypair_for(PrincipalKind::Human);
        let engine = ExecutionEngine::new_with_keypair(fixture.registry.clone(), human.clone());
        let result = AgentRuntime::new(
            fixture.registry,
            engine,
            human,
            fixture.workspace.path(),
            fixture.agent_store,
            fixture.run_store,
            fixture.approval_store,
            Arc::new(ScriptedGateway::new(vec![])),
        );

        assert!(matches!(
            result,
            Err(AgentRuntimeError::IdentityMustBeAgent)
        ));
    }

    fn frozen_live_policy_material(template: Value) -> LivePolicyMaterial {
        let checks = template["checks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|check| check["id"].clone())
            .collect::<Vec<_>>();
        let tamper = template["tamper_vectors"].clone();
        LivePolicyMaterial {
            template: template.clone(),
            template_policy_digest: value_digest(&template).unwrap(),
            check_set_digest: wrapped_digest(
                "proof-release-manager-live-check-set-digest/v1",
                "check_ids",
                &Value::Array(checks),
            )
            .unwrap(),
            tamper_vector_set_digest: wrapped_digest(
                "proof-release-manager-live-tamper-vector-set-digest/v1",
                "tamper_vector_ids",
                &tamper,
            )
            .unwrap(),
            pricing_schedule_digest: value_digest(&template["pricing"]).unwrap(),
            instructions_digest: value_digest(&template["outbound_data"]["instructions"]).unwrap(),
            initial_input_digest: value_digest(&Value::String(
                "synthetic initial input".to_string(),
            ))
            .unwrap(),
            parameters_schema_digest: wrapped_digest(
                "proof-openai-function-parameters-digest/v1",
                "parameters",
                &template["tool"]["declaration"]["parameters"],
            )
            .unwrap(),
            tool_declaration_digest: wrapped_digest(
                "proof-openai-function-declaration-digest/v1",
                "declaration",
                &template["tool"]["declaration"],
            )
            .unwrap(),
            tool_set_digest: wrapped_digest(
                "proof-openai-tool-set-digest/v1",
                "tools",
                &json!([template["tool"]["declaration"].clone()]),
            )
            .unwrap(),
            binding_inputs: LiveBindingInputs {
                preflight_evidence_digest: value_digest(&json!("preflight")).unwrap(),
                agent_principal_id: PrincipalId::now(),
                approver_principal_id: PrincipalId::now(),
                delegation_id: Uuid::now_v7(),
                delegation_digest: value_digest(&json!("delegation")).unwrap(),
                edition_id: Uuid::now_v7(),
                manifest_digest: format!("sha256:{}", "0".repeat(64)),
                idempotency_key: Uuid::now_v7(),
                version_label: "2026.08.30-rc1".to_string(),
            },
        }
    }

    #[test]
    fn live_start_setup_check_is_repeatable_read_only_and_factory_free() {
        let fixture = LiveFixture::new();
        let factory = fixture.factory(vec![]);
        let approval_store = Arc::new(CountingApprovalWrites {
            inner: fixture.approval_store.clone(),
            writes: AtomicUsize::new(0),
        });
        let runtime = fixture.runtime_with_all_stores(
            factory.clone(),
            fixture.run_store.clone(),
            fixture.agent_store.clone(),
            approval_store.clone(),
        );
        let before = live_store_snapshot(&fixture);

        runtime.check_live_start_setup(&fixture.setup).unwrap();
        runtime.check_live_start_setup(&fixture.setup).unwrap();

        assert_eq!(live_store_snapshot(&fixture), before);
        assert_eq!(approval_store.writes.load(Ordering::SeqCst), 0);
        assert_eq!(factory.creates.load(Ordering::SeqCst), 0);
        assert!(factory.contexts.lock().unwrap().is_empty());
        assert_eq!(factory.gateway.sends.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn live_start_setup_check_rejects_every_material_binding_without_writes() {
        for mutation in 0..13 {
            let fixture = LiveFixture::new();
            let mut setup = fixture.setup.clone();
            let forged = value_digest(&json!({"forged": mutation})).unwrap();
            match mutation {
                0 => {
                    let mut preflight: PreflightEvidence =
                        serde_json::from_value(setup.preflight_evidence.clone()).unwrap();
                    preflight.score_bps = 9_999;
                    reseal_fixture_preflight(&mut setup, preflight);
                }
                1 => setup.policy.binding_inputs.preflight_evidence_digest = forged,
                2 => setup.policy.binding_inputs.agent_principal_id = PrincipalId::now(),
                3 => setup.policy.binding_inputs.approver_principal_id = PrincipalId::now(),
                4 => setup.policy.binding_inputs.delegation_id = Uuid::now_v7(),
                5 => setup.policy.binding_inputs.delegation_digest = forged,
                6 => setup.policy.binding_inputs.edition_id = Uuid::nil(),
                7 => {
                    setup.policy.binding_inputs.manifest_digest =
                        format!("sha256:{}", "A".repeat(64))
                }
                8 => setup.policy.binding_inputs.idempotency_key = Uuid::nil(),
                9 => setup.policy.binding_inputs.version_label = "2026.08.31-rc1".to_string(),
                10 => setup.process_epoch_id = Uuid::nil(),
                11 => setup.policy.tool_set_digest = forged,
                12 => {
                    setup.authority.delegation.valid_until = Utc::now() + Duration::seconds(299);
                    reseal_fixture_delegation(&mut setup);
                }
                _ => unreachable!(),
            }
            assert_live_start_check_rejected_without_writes(&fixture, &setup);
        }
    }

    #[test]
    fn live_start_setup_check_rejects_resume_target_goal_and_profile_without_writes() {
        let fixture = LiveFixture::new();
        let mut resume = fixture.setup.clone();
        resume.intent = LiveRunIntent::Resume {
            run_id: Uuid::now_v7(),
        };
        let error = assert_live_start_check_rejected_without_writes(&fixture, &resume);
        assert!(matches!(
            error,
            AgentRuntimeError::LiveSetup(message)
                if message == "check_live_start_setup accepts only LiveRunIntent::Start"
        ));

        let fixture = LiveFixture::new();
        let mut missing_target = fixture.setup.clone();
        if let LiveRunIntent::Start { goal, .. } = missing_target.intent.clone() {
            missing_target.intent = LiveRunIntent::Start {
                agent_id: Uuid::now_v7(),
                goal,
            };
        }
        assert!(matches!(
            assert_live_start_check_rejected_without_writes(&fixture, &missing_target),
            AgentRuntimeError::AgentNotFound(_)
        ));

        let fixture = LiveFixture::new();
        let mut wrong_goal = fixture.setup.clone();
        let altered_goal = "Publish a different synthetic edition.".to_string();
        wrong_goal.policy.initial_input_digest =
            value_digest(&Value::String(altered_goal.clone())).unwrap();
        wrong_goal.intent = LiveRunIntent::Start {
            agent_id: fixture.agent.id,
            goal: altered_goal,
        };
        assert_live_start_check_rejected_without_writes(&fixture, &wrong_goal);

        for mutation in 0..4 {
            let fixture = LiveFixture::new();
            let mut setup = fixture.setup.clone();
            let mut agent = fixture.agent.clone();
            agent.id = Uuid::now_v7();
            agent.name = format!("invalid-live-profile-{mutation}-{}", agent.id);
            match mutation {
                0 => agent.provider = "different-provider".to_string(),
                1 => agent.model = "different-model".to_string(),
                2 => agent.tools = vec![AgentTool::new("release.publish", "v1").unwrap()],
                3 => agent.limits.max_model_calls = 2,
                _ => unreachable!(),
            }
            fixture.agent_store.save_agent_definition(&agent).unwrap();
            if let LiveRunIntent::Start { goal, .. } = setup.intent.clone() {
                setup.intent = LiveRunIntent::Start {
                    agent_id: agent.id,
                    goal,
                };
            }
            assert_live_start_check_rejected_without_writes(&fixture, &setup);
        }
    }

    #[test]
    fn live_policy_requires_the_exact_ordered_check_and_tamper_sets() {
        let template: Value =
            serde_json::from_str(include_str!("../../../evals/release-manager-live-v1.json"))
                .unwrap();
        let policy = frozen_live_policy_material(template.clone());
        assert!(validate_policy_sets(&policy).is_ok());

        let mut reordered_checks = template.clone();
        reordered_checks["checks"]
            .as_array_mut()
            .unwrap()
            .swap(0, 1);
        assert!(validate_policy_sets(&frozen_live_policy_material(reordered_checks)).is_err());

        let mut missing_tamper = template;
        missing_tamper["tamper_vectors"]
            .as_array_mut()
            .unwrap()
            .pop();
        assert!(validate_policy_sets(&frozen_live_policy_material(missing_tamper)).is_err());
    }

    fn reseal_fixture_delegation(setup: &mut LiveRunSetup) {
        let delegation = setup.authority.delegation.clone();
        let digest = delegation_digest(&delegation).unwrap();
        setup.authority.delegation_digest = digest;
        setup.authority.delegation_chain.grants = vec![delegation];
        setup.policy.binding_inputs.delegation_digest = digest;
    }

    fn reseal_fixture_preflight(setup: &mut LiveRunSetup, preflight: PreflightEvidence) {
        let evidence = serde_json::to_value(preflight).unwrap();
        let digest = wrapped_digest(
            "proof-release-manager-preflight-evidence-digest/v1",
            "evidence",
            &evidence,
        )
        .unwrap();
        setup.preflight_evidence = evidence;
        setup.preflight_evidence_digest = digest;
        setup.policy.binding_inputs.preflight_evidence_digest = digest;
    }

    fn assert_live_setup_rejected_before_factory(fixture: &LiveFixture, setup: LiveRunSetup) {
        let factory = fixture.factory(vec![]);
        let run_count = fixture.run_store.list_agent_runs().unwrap().len();
        let result = fixture.runtime(factory.clone()).run_live(setup);
        assert!(result.is_err(), "invalid live setup was accepted");
        assert_eq!(
            fixture.run_store.list_agent_runs().unwrap().len(),
            run_count
        );
        assert_eq!(factory.creates.load(Ordering::SeqCst), 0);
        assert_eq!(factory.gateway.sends.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn live_setup_policy_and_scope_failures_never_construct_or_send() {
        for mutation in 0..9 {
            let fixture = LiveFixture::new();
            let mut setup = fixture.setup.clone();
            match mutation {
                0 => setup.preflight_evidence["score_bps"] = json!(9_999),
                1 => setup.policy.template["uncontracted"] = json!(true),
                2 => setup.policy.template_policy_digest = value_digest(&Value::Null).unwrap(),
                3 => setup.authority.delegation.scope.allowed_operations = None,
                4 => {
                    setup.authority.delegation.scope.allowed_operations =
                        Some(vec!["*".to_string()])
                }
                5 => {
                    setup.authority.delegation.scope.allowed_operations = Some(vec![
                        "release.publish".to_string(),
                        "release.approve".to_string(),
                    ])
                }
                6 => setup.authority.delegation.scope.allowed_domains = None,
                7 => {
                    setup.authority.delegation.scope.allowed_domains =
                        Some(vec!["content".to_string(), "commerce".to_string()])
                }
                8 => {
                    setup.authority.delegation.scope.resource_scope =
                        Some("edition:synthetic".to_string())
                }
                _ => unreachable!(),
            }
            if mutation >= 3 {
                reseal_fixture_delegation(&mut setup);
            }
            assert_live_setup_rejected_before_factory(&fixture, setup);
        }
    }

    #[test]
    fn live_all_static_request_digest_failures_create_no_run_or_gateway() {
        for mutation in 0..5 {
            let fixture = LiveFixture::new();
            let mut setup = fixture.setup.clone();
            let forged = value_digest(&json!({"forged": mutation})).unwrap();
            match mutation {
                0 => setup.policy.instructions_digest = forged,
                1 => setup.policy.initial_input_digest = forged,
                2 => setup.policy.parameters_schema_digest = forged,
                3 => setup.policy.tool_declaration_digest = forged,
                4 => setup.policy.tool_set_digest = forged,
                _ => unreachable!(),
            }
            assert_live_setup_rejected_before_factory(&fixture, setup);
        }
    }

    #[test]
    fn live_authority_main_delegation_rejects_nested_unknown_fields() {
        let fixture = LiveFixture::new();
        let mut value = serde_json::to_value(&fixture.setup).unwrap();
        value["authority"]["delegation"]["scope"]["unknown"] = json!(true);
        assert!(serde_json::from_value::<LiveRunSetup>(value).is_err());
    }

    #[test]
    fn live_forged_or_missing_preflight_row_never_constructs_a_gateway() {
        for mutation in 0..3 {
            let fixture = LiveFixture::new();
            let mut setup = fixture.setup.clone();
            let mut preflight: PreflightEvidence =
                serde_json::from_value(setup.preflight_evidence.clone()).unwrap();
            match mutation {
                0 => preflight.evaluation_id = Uuid::now_v7(),
                1 => preflight.trace_digest = value_digest(&json!("forged trace")).unwrap(),
                2 => preflight.evaluation_created_at += Duration::seconds(1),
                _ => unreachable!(),
            }
            reseal_fixture_preflight(&mut setup, preflight);
            assert_live_setup_rejected_before_factory(&fixture, setup);
        }
    }

    #[test]
    fn live_invalid_identity_version_manifest_or_edition_bindings_are_pre_secret() {
        for mutation in 0..5 {
            let fixture = LiveFixture::new();
            let mut setup = fixture.setup.clone();
            match mutation {
                0 => {
                    setup.policy.binding_inputs.approver_principal_id =
                        fixture.identity.principal_id
                }
                1 => setup.policy.binding_inputs.approver_principal_id = PrincipalId::now(),
                2 => setup.policy.binding_inputs.version_label = "2026.08.31-rc1".to_string(),
                3 => {
                    setup.policy.binding_inputs.manifest_digest =
                        format!("sha256:{}", "A".repeat(64))
                }
                4 => setup.policy.binding_inputs.edition_id = Uuid::nil(),
                _ => unreachable!(),
            }
            assert_live_setup_rejected_before_factory(&fixture, setup);
        }
    }

    #[test]
    fn live_lower_agent_limits_are_rejected_before_factory() {
        let fixture = LiveFixture::new();
        let mut setup = fixture.setup.clone();
        let lower_limits = AgentLimits {
            max_steps: 2,
            max_model_calls: 2,
            max_total_tokens: 10_000,
            max_duration_seconds: 300,
            max_output_tokens_per_call: 1024,
            max_cost_microusd: Some(120_000),
        };
        let agent = AgentDefinition::new(
            format!("lower-limit-live-agent-{}", Uuid::now_v7()),
            "Run the sealed synthetic preview journey.",
            LIVE_PROVIDER,
            LIVE_MODEL,
            vec![AgentTool::new("release.publish", "v2").unwrap()],
            lower_limits,
            Utc::now(),
        )
        .unwrap();
        fixture.agent_store.save_agent_definition(&agent).unwrap();
        if let LiveRunIntent::Start { goal, .. } = setup.intent.clone() {
            setup.intent = LiveRunIntent::Start {
                agent_id: agent.id,
                goal,
            };
        }
        assert_live_setup_rejected_before_factory(&fixture, setup);
    }

    #[test]
    fn live_authority_must_cover_the_actual_selected_start_deadline() {
        let fixture = LiveFixture::new();
        let mut setup = fixture.setup.clone();
        setup.authority.delegation.valid_until = Utc::now() + Duration::seconds(299);
        reseal_fixture_delegation(&mut setup);
        assert_live_setup_rejected_before_factory(&fixture, setup);
    }

    #[test]
    fn live_zero_or_inexact_shared_gateway_usage_is_ambiguous() {
        for usage in [
            ModelUsage::default(),
            ModelUsage {
                input_tokens: 2,
                output_tokens: 1,
                total_tokens: 4,
                cost_microusd: None,
            },
        ] {
            let fixture = LiveFixture::new();
            let factory = fixture.factory(vec![LiveGatewayAction::Turn(ModelTurn {
                response_id: "resp_inexact_usage".to_string(),
                returned_model: Some(LIVE_MODEL.to_string()),
                response_body_digest: Some(value_digest(&json!("exact body bytes")).unwrap()),
                decision: ModelDecision::ToolCall {
                    call_id: "call_publish".to_string(),
                    name: LIVE_TOOL_NAME.to_string(),
                    arguments: fixture.arguments.as_value().unwrap(),
                },
                usage,
            })]);
            let outcome = fixture
                .runtime(factory.clone())
                .run_live(fixture.setup.clone())
                .unwrap();
            assert!(matches!(outcome, AgentRuntimeOutcome::Failed { .. }));
            assert_eq!(factory.gateway.sends.load(Ordering::SeqCst), 1);
            assert!(fixture.execution_store.proofs.lock().unwrap().is_empty());
            let run = fixture
                .run_store
                .list_agent_runs()
                .unwrap()
                .into_iter()
                .find(|run| run.agent_id == Some(fixture.agent.id))
                .unwrap();
            let state = fixture.runtime(factory).live_state(run.id).unwrap();
            assert_eq!(state.attempts[0].state, ProviderAttemptState::Ambiguous);
            assert_eq!(
                state.attempts[0].failure.as_ref().unwrap().code,
                "invalid_response_usage"
            );
        }
    }

    fn approve_live_wait(fixture: &LiveFixture, request: &SignedApprovalRequest) {
        let decision = SignedApprovalDecision::create(
            request,
            ApprovalOutcome::Approved,
            Some("approved synthetic preview".to_string()),
            Utc::now(),
            &fixture.approver,
        )
        .unwrap();
        fixture
            .approval_store
            .save_approval_decision(&decision)
            .unwrap();
    }

    #[test]
    fn approval_context_projects_v1_and_validated_live_v2_without_trusting_tampered_state() {
        let run_id = Uuid::now_v7();
        let agent_id = Uuid::now_v7();
        let step_id = Uuid::now_v7();
        let request_id = Uuid::now_v7();
        let arguments = json!({"name": "review-v1"});
        let v1 = AgentRuntimeState {
            agent_id,
            started_at: Utc::now(),
            previous_response_id: Some("response-v1".to_string()),
            next_input: ModelInput::Goal {
                text: "review the request".to_string(),
            },
            pending_tool: Some(PendingToolCall {
                call_id: "call-v1".to_string(),
                tool_name: "proof_catalog_v1_catalog_create".to_string(),
                operation: "catalog.create".to_string(),
                version: "v1".to_string(),
                arguments: arguments.clone(),
                step_id,
                approval_request_id: Some(request_id),
            }),
            model_calls: 1,
            tool_attempts: 1,
            input_tokens: 10,
            output_tokens: 4,
            total_tokens: 14,
            cost_microusd: None,
            final_output: None,
            terminal_error: None,
        };
        let v1_checkpoint = AgentCheckpoint::create(
            run_id,
            0,
            json!({"kind": RUNTIME_CHECKPOINT_KIND, "runtime": v1}),
            Utc::now(),
        )
        .unwrap();
        let v1_context = runtime_approval_context(run_id, &[v1_checkpoint], &[]).unwrap();
        assert_eq!(v1_context.checkpoint_kind, RUNTIME_CHECKPOINT_KIND);
        assert_eq!(v1_context.run_id, run_id);
        assert_eq!(v1_context.agent_id, agent_id);
        assert_eq!(v1_context.required_approver_id, None);
        assert_eq!(v1_context.pending_tool.unwrap().arguments, arguments);

        let fixture = LiveFixture::new();
        let factory = fixture.factory(vec![LiveGatewayAction::Tool(fixture.arguments.clone())]);
        let waiting = fixture
            .runtime(factory)
            .run_live(fixture.setup.clone())
            .unwrap();
        let AgentRuntimeOutcome::WaitingForApproval { run, request, .. } = waiting else {
            panic!("expected live approval wait")
        };
        let checkpoints = fixture.run_store.list_agent_checkpoints(&run.id).unwrap();
        let events = fixture.agent_store.list_agent_run_events(&run.id).unwrap();
        let live_context = runtime_approval_context(run.id, &checkpoints, &events).unwrap();
        assert_eq!(live_context.checkpoint_kind, LIVE_RUNTIME_CHECKPOINT_KIND);
        assert_eq!(live_context.run_id, run.id);
        assert_eq!(live_context.agent_id, fixture.agent.id);
        assert_eq!(
            live_context.required_approver_id,
            Some(fixture.approver.principal_id.as_uuid())
        );
        let pending = live_context.pending_tool.unwrap();
        assert_eq!(pending.arguments, fixture.arguments.as_value().unwrap());
        assert_eq!(pending.approval_request_id, Some(request.body.id));
        assert_eq!(live_context.sealed_approval_request, Some(request.clone()));
        assert_eq!(
            live_context.sealed_step,
            Some(
                fixture
                    .run_store
                    .find_agent_run_step_by_approval(&request.body.id)
                    .unwrap()
                    .unwrap()
            )
        );

        let mut substituted = checkpoints;
        let latest = substituted.last_mut().unwrap();
        latest.state["runtime"]["pending_tool"]["arguments"]["version_label"] =
            json!("substituted");
        latest.state_digest = digest(
            ArtifactKind::AgentCheckpoint,
            &canonicalize(&latest.state).unwrap(),
        );
        assert!(matches!(
            runtime_approval_context(run.id, &substituted, &events),
            Err(AgentRuntimeError::InvalidCheckpoint(id)) if id == run.id
        ));
    }

    #[test]
    fn approval_context_rejects_live_v2_history_with_omitted_sequence_zero_prefix() {
        let fixture = LiveFixture::new();
        let waiting = fixture
            .runtime(fixture.factory(vec![LiveGatewayAction::Tool(fixture.arguments.clone())]))
            .run_live(fixture.setup.clone())
            .unwrap();
        let AgentRuntimeOutcome::WaitingForApproval { run, .. } = waiting else {
            panic!("expected live approval wait")
        };
        let checkpoints = fixture.run_store.list_agent_checkpoints(&run.id).unwrap();
        let events = fixture.agent_store.list_agent_run_events(&run.id).unwrap();
        assert!(checkpoints.len() > 1);

        assert!(matches!(
            runtime_approval_context(run.id, &checkpoints[1..], &events),
            Err(AgentRuntimeError::InvalidCheckpoint(id)) if id == run.id
        ));
    }

    #[test]
    fn approval_context_rejects_recomputed_live_v2_envelope_unknown_field() {
        let fixture = LiveFixture::new();
        let waiting = fixture
            .runtime(fixture.factory(vec![LiveGatewayAction::Tool(fixture.arguments.clone())]))
            .run_live(fixture.setup.clone())
            .unwrap();
        let AgentRuntimeOutcome::WaitingForApproval { run, .. } = waiting else {
            panic!("expected live approval wait")
        };
        let mut checkpoints = fixture.run_store.list_agent_checkpoints(&run.id).unwrap();
        let events = fixture.agent_store.list_agent_run_events(&run.id).unwrap();
        let latest = checkpoints.last_mut().unwrap();
        latest.state["unexpected"] = json!(true);
        latest.state_digest = digest(
            ArtifactKind::AgentCheckpoint,
            &canonicalize(&latest.state).unwrap(),
        );

        assert!(matches!(
            runtime_approval_context(run.id, &checkpoints, &events),
            Err(AgentRuntimeError::InvalidCheckpoint(id)) if id == run.id
        ));
    }

    #[test]
    fn approval_context_rejects_pending_call_id_not_bound_to_committed_decision() {
        let fixture = LiveFixture::new();
        let waiting = fixture
            .runtime(fixture.factory(vec![LiveGatewayAction::Tool(fixture.arguments.clone())]))
            .run_live(fixture.setup.clone())
            .unwrap();
        let AgentRuntimeOutcome::WaitingForApproval { run, .. } = waiting else {
            panic!("expected live approval wait")
        };
        let mut checkpoints = fixture.run_store.list_agent_checkpoints(&run.id).unwrap();
        let events = fixture.agent_store.list_agent_run_events(&run.id).unwrap();
        let substituted_call_id = "call-substituted";
        let substituted_digest = LiveCommittedDecision::ToolCall {
            call_id: substituted_call_id.to_string(),
            name: LIVE_TOOL_NAME.to_string(),
            arguments: fixture.arguments.clone(),
        }
        .digest()
        .unwrap();
        for checkpoint in &mut checkpoints {
            let mut changed = false;
            if !checkpoint.state["runtime"]["attempts"][0]["response"].is_null() {
                checkpoint.state["runtime"]["attempts"][0]["response"]["decision_digest"] =
                    json!(substituted_digest);
                changed = true;
            }
            if !checkpoint.state["runtime"]["pending_tool"].is_null() {
                checkpoint.state["runtime"]["pending_tool"]["call_id"] = json!(substituted_call_id);
                changed = true;
            }
            if changed {
                checkpoint.state_digest = digest(
                    ArtifactKind::AgentCheckpoint,
                    &canonicalize(&checkpoint.state).unwrap(),
                );
            }
        }

        assert!(matches!(
            runtime_approval_context(run.id, &checkpoints, &events),
            Err(AgentRuntimeError::InvalidCheckpoint(id)) if id == run.id
        ));
    }

    #[test]
    fn live_resume_rejects_pending_call_id_substitution_before_approval_or_effect() {
        let fixture = LiveFixture::new();
        let waiting = fixture
            .runtime(fixture.factory(vec![LiveGatewayAction::Tool(fixture.arguments.clone())]))
            .run_live(fixture.setup.clone())
            .unwrap();
        let AgentRuntimeOutcome::WaitingForApproval { run, request, .. } = waiting else {
            panic!("expected live approval wait")
        };
        approve_live_wait(&fixture, &request);
        let checkpoints = fixture.run_store.list_agent_checkpoints(&run.id).unwrap();
        let mut state = checkpoints.last().unwrap().state.clone();
        let substituted_call_id = "call-substituted";
        state["runtime"]["pending_tool"]["call_id"] = json!(substituted_call_id);
        state["runtime"]["attempts"][0]["response"]["decision_digest"] =
            json!(LiveCommittedDecision::ToolCall {
                call_id: substituted_call_id.to_string(),
                name: LIVE_TOOL_NAME.to_string(),
                arguments: fixture.arguments.clone(),
            }
            .digest()
            .unwrap());
        let substituted = AgentCheckpoint::create(
            run.id,
            checkpoints.last().unwrap().sequence + 1,
            state,
            Utc::now(),
        )
        .unwrap();
        fixture
            .run_store
            .save_agent_checkpoint(&substituted)
            .unwrap();

        let factory = fixture.factory(vec![]);
        let result = fixture
            .runtime(factory.clone())
            .run_live(fixture.resume_setup(run.id));
        assert!(matches!(
            result,
            Err(AgentRuntimeError::InvalidCheckpoint(id)) if id == run.id
        ));
        assert_eq!(factory.creates.load(Ordering::SeqCst), 0);
        assert_eq!(factory.gateway.sends.load(Ordering::SeqCst), 0);
        assert!(fixture.execution_store.proofs.lock().unwrap().is_empty());
        assert!(fixture.execution_store.contexts.lock().unwrap().is_empty());
        assert_eq!(
            fixture
                .agent_store
                .list_agent_run_events(&run.id)
                .unwrap()
                .iter()
                .filter(|event| event.kind == AgentRunEventKind::ApprovalResumed)
                .count(),
            0
        );
    }

    #[test]
    fn live_one_retry_success_is_17_of_17_and_retry_evidence_is_exact() {
        for first_failure in [
            ModelGatewayError::CertifiedNoBytes("no bytes written".to_string()),
            ModelGatewayError::Explicit429("429 without response object".to_string()),
        ] {
            let fixture = LiveFixture::new();
            let factory = fixture.factory(vec![
                LiveGatewayAction::Error(first_failure),
                LiveGatewayAction::Tool(fixture.arguments.clone()),
                LiveGatewayAction::FinishFromToolOutput,
            ]);
            let waiting = fixture
                .runtime(factory.clone())
                .run_live(fixture.setup.clone())
                .unwrap();
            let AgentRuntimeOutcome::WaitingForApproval { run, request, .. } = waiting else {
                panic!("expected approval after exact retry")
            };
            approve_live_wait(&fixture, &request);
            let completed = fixture
                .runtime(factory.clone())
                .run_live(fixture.resume_setup(run.id))
                .unwrap();
            let AgentRuntimeOutcome::Completed { evaluation, .. } = completed else {
                panic!("one-retry journey did not complete")
            };
            assert_eq!(evaluation.outcome, AgentEvaluationOutcome::Passed);
            assert_eq!(evaluation.metrics["passed_checks"], 17);
            let state = fixture.runtime(factory.clone()).live_state(run.id).unwrap();
            assert_eq!(state.counters.retries, 1);
            assert_eq!(state.attempts.len(), 3);
            assert_eq!(
                state.attempts[1].retry_of,
                Some(state.attempts[0].attempt_id)
            );
            assert_eq!(state.attempts[1].request, state.attempts[0].request);
            assert_eq!(factory.creates.load(Ordering::SeqCst), 3);
            assert_eq!(factory.gateway.sends.load(Ordering::SeqCst), 3);
        }
    }

    #[test]
    fn live_second_retryable_failure_terminalizes_without_a_third_dispatch() {
        let fixture = LiveFixture::new();
        let factory = fixture.factory(vec![
            LiveGatewayAction::Error(ModelGatewayError::CertifiedNoBytes(
                "first certified no bytes".to_string(),
            )),
            LiveGatewayAction::Error(ModelGatewayError::CertifiedNoBytes(
                "second certified no bytes".to_string(),
            )),
        ]);
        let outcome = fixture
            .runtime(factory.clone())
            .run_live(fixture.setup.clone())
            .unwrap();
        let AgentRuntimeOutcome::Failed { run, .. } = outcome else {
            panic!("second retryable failure must be terminal")
        };
        let state = fixture.runtime(factory.clone()).live_state(run.id).unwrap();
        assert_eq!(state.counters.retries, 1);
        assert_eq!(state.counters.provider_dispatches, 2);
        assert_eq!(state.attempts.len(), 2);
        assert_eq!(
            state.attempts[0].state,
            ProviderAttemptState::FailedRetryable
        );
        assert_eq!(
            state.attempts[1].state,
            ProviderAttemptState::FailedTerminal
        );
        assert_eq!(factory.creates.load(Ordering::SeqCst), 2);
        assert_eq!(factory.gateway.sends.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn live_dispatching_restart_fails_ambiguous_without_factory_or_send() {
        let fixture = LiveFixture::new();
        let crashing = fixture.factory(vec![LiveGatewayAction::PanicAfterBarrier]);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = fixture
                .runtime(crashing.clone())
                .run_live(fixture.setup.clone());
        }));
        assert!(result.is_err());
        let run = fixture
            .run_store
            .list_agent_runs()
            .unwrap()
            .into_iter()
            .find(|run| run.agent_id == Some(fixture.agent.id))
            .unwrap();
        let resume_factory = fixture.factory(vec![]);
        let outcome = fixture
            .runtime(resume_factory.clone())
            .run_live(fixture.resume_setup(run.id))
            .unwrap();
        assert!(matches!(outcome, AgentRuntimeOutcome::Failed { .. }));
        assert_eq!(resume_factory.creates.load(Ordering::SeqCst), 0);
        assert_eq!(resume_factory.gateway.sends.load(Ordering::SeqCst), 0);
        let state = fixture.runtime(resume_factory).live_state(run.id).unwrap();
        assert_eq!(state.attempts[0].state, ProviderAttemptState::Dispatching);
        assert!(state
            .terminal_error
            .as_deref()
            .unwrap()
            .contains("ambiguous"));
    }

    #[test]
    fn live_dispatch_barrier_faults_send_zero_provider_bytes() {
        for fault in 0..4 {
            let fixture = LiveFixture::new();
            let factory = fixture.factory(vec![LiveGatewayAction::Tool(fixture.arguments.clone())]);
            let result = match fault {
                0 | 2 => {
                    let run_fault = if fault == 0 {
                        LiveRunFault::DispatchSave
                    } else {
                        LiveRunFault::DispatchRead
                    };
                    fixture
                        .runtime_with_stores(
                            factory.clone(),
                            Arc::new(FaultingLiveRunStore::new(
                                fixture.run_store.clone(),
                                run_fault,
                            )),
                            fixture.agent_store.clone(),
                        )
                        .run_live(fixture.setup.clone())
                }
                1 | 3 => {
                    let agent_fault = if fault == 1 {
                        LiveAgentFault::RequestedSave
                    } else {
                        LiveAgentFault::RequestedRead
                    };
                    fixture
                        .runtime_with_stores(
                            factory.clone(),
                            fixture.run_store.clone(),
                            Arc::new(FaultingLiveAgentStore {
                                inner: fixture.agent_store.clone(),
                                fault: agent_fault,
                                armed: AtomicBool::new(true),
                                requested_saved: AtomicBool::new(false),
                            }),
                        )
                        .run_live(fixture.setup.clone())
                }
                _ => unreachable!(),
            };
            assert!(result.is_err());
            assert_eq!(factory.gateway.sends.load(Ordering::SeqCst), 0);
        }
    }

    #[test]
    fn runtime_state_view_exposes_dispatching_checkpoint_before_requested_event() {
        let fixture = LiveFixture::new();
        let factory = fixture.factory(vec![LiveGatewayAction::Tool(fixture.arguments.clone())]);
        let result = fixture
            .runtime_with_stores(
                factory.clone(),
                fixture.run_store.clone(),
                Arc::new(FaultingLiveAgentStore {
                    inner: fixture.agent_store.clone(),
                    fault: LiveAgentFault::RequestedSave,
                    armed: AtomicBool::new(true),
                    requested_saved: AtomicBool::new(false),
                }),
            )
            .run_live(fixture.setup.clone());
        assert!(matches!(result, Err(AgentRuntimeError::Store(_))));
        assert_eq!(factory.gateway.sends.load(Ordering::SeqCst), 0);
        let run = fixture
            .run_store
            .list_agent_runs()
            .unwrap()
            .into_iter()
            .find(|run| run.agent_id == Some(fixture.agent.id))
            .unwrap();
        let checkpoints = fixture.run_store.list_agent_checkpoints(&run.id).unwrap();
        let events = fixture.agent_store.list_agent_run_events(&run.id).unwrap();

        let view = runtime_state_view(run.id, &checkpoints).unwrap();
        assert_eq!(view.checkpoint_kind, LIVE_RUNTIME_CHECKPOINT_KIND);
        assert_eq!(view.state["attempts"][0]["state"], "dispatching");
        assert!(matches!(
            runtime_approval_context(run.id, &checkpoints, &events),
            Err(AgentRuntimeError::InvalidCheckpoint(id)) if id == run.id
        ));
    }

    #[test]
    fn live_prepared_restart_reuses_exact_attempt_before_first_send() {
        let fixture = LiveFixture::new();
        let panic_factory = Arc::new(PanicFactory {
            creates: AtomicUsize::new(0),
        });
        let crashed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = fixture
                .runtime(panic_factory.clone())
                .run_live(fixture.setup.clone());
        }));
        assert!(crashed.is_err());
        let run = fixture
            .run_store
            .list_agent_runs()
            .unwrap()
            .into_iter()
            .find(|run| run.agent_id == Some(fixture.agent.id))
            .unwrap();
        let prepared = fixture
            .runtime(fixture.factory(vec![]))
            .live_state(run.id)
            .unwrap();
        assert_eq!(prepared.attempts.len(), 1);
        assert_eq!(prepared.attempts[0].state, ProviderAttemptState::Prepared);
        let prepared_id = prepared.attempts[0].attempt_id;
        let factory = fixture.factory(vec![LiveGatewayAction::Tool(fixture.arguments.clone())]);
        let outcome = fixture
            .runtime(factory.clone())
            .run_live(fixture.resume_setup(run.id))
            .unwrap();
        assert!(matches!(
            outcome,
            AgentRuntimeOutcome::WaitingForApproval { .. }
        ));
        assert_eq!(factory.creates.load(Ordering::SeqCst), 1);
        assert_eq!(factory.gateway.sends.load(Ordering::SeqCst), 1);
        assert_eq!(factory.contexts.lock().unwrap()[0].attempt_id, prepared_id);
    }

    #[test]
    fn live_response_received_restart_is_ambiguous_and_duplicate_safe() {
        let fixture = LiveFixture::new();
        let rejecting_store: Arc<dyn AgentRunStore> = Arc::new(RejectCommittedRunStore {
            inner: fixture.run_store.clone(),
        });
        let first_factory =
            fixture.factory(vec![LiveGatewayAction::Tool(fixture.arguments.clone())]);
        let result = fixture
            .runtime_with_stores(
                first_factory.clone(),
                rejecting_store,
                fixture.agent_store.clone(),
            )
            .run_live(fixture.setup.clone());
        assert!(matches!(result, Err(AgentRuntimeError::Store(_))));
        let run = fixture
            .run_store
            .list_agent_runs()
            .unwrap()
            .into_iter()
            .find(|run| run.agent_id == Some(fixture.agent.id))
            .unwrap();
        let state = fixture
            .runtime(fixture.factory(vec![]))
            .live_state(run.id)
            .unwrap();
        assert_eq!(
            state.attempts[0].state,
            ProviderAttemptState::ResponseReceived
        );
        let resume_factory = fixture.factory(vec![]);
        let outcome = fixture
            .runtime(resume_factory.clone())
            .run_live(fixture.resume_setup(run.id))
            .unwrap();
        assert!(matches!(outcome, AgentRuntimeOutcome::Failed { .. }));
        assert_eq!(resume_factory.creates.load(Ordering::SeqCst), 0);
        assert_eq!(resume_factory.gateway.sends.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn live_committed_checkpoint_without_exact_response_event_fails_closed() {
        let fixture = LiveFixture::new();
        let rejecting_events: Arc<dyn AgentStore> = Arc::new(RejectModelRespondedAgentStore {
            inner: fixture.agent_store.clone(),
        });
        let first_factory =
            fixture.factory(vec![LiveGatewayAction::Tool(fixture.arguments.clone())]);
        let result = fixture
            .runtime_with_stores(first_factory, fixture.run_store.clone(), rejecting_events)
            .run_live(fixture.setup.clone());
        assert!(matches!(result, Err(AgentRuntimeError::Store(_))));
        let run = fixture
            .run_store
            .list_agent_runs()
            .unwrap()
            .into_iter()
            .find(|run| run.agent_id == Some(fixture.agent.id))
            .unwrap();
        let committed = fixture
            .runtime(fixture.factory(vec![]))
            .live_state(run.id)
            .unwrap();
        assert_eq!(committed.attempts[0].state, ProviderAttemptState::Committed);
        let checkpoints = fixture.run_store.list_agent_checkpoints(&run.id).unwrap();
        let events = fixture.agent_store.list_agent_run_events(&run.id).unwrap();
        let view = runtime_state_view(run.id, &checkpoints).unwrap();
        assert_eq!(view.checkpoint_kind, LIVE_RUNTIME_CHECKPOINT_KIND);
        assert_eq!(view.state["attempts"][0]["state"], "committed");
        assert!(matches!(
            runtime_approval_context(run.id, &checkpoints, &events),
            Err(AgentRuntimeError::InvalidCheckpoint(id)) if id == run.id
        ));
        let resume_factory = fixture.factory(vec![]);
        let outcome = fixture
            .runtime(resume_factory.clone())
            .run_live(fixture.resume_setup(run.id))
            .unwrap();
        assert!(matches!(outcome, AgentRuntimeOutcome::Failed { .. }));
        assert_eq!(resume_factory.creates.load(Ordering::SeqCst), 0);
        assert_eq!(resume_factory.gateway.sends.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn live_committed_tool_decision_recovers_without_provider_dispatch() {
        let fixture = LiveFixture::new();
        let factory = fixture.factory(vec![LiveGatewayAction::Tool(fixture.arguments.clone())]);
        let result = fixture
            .runtime_with_stores(
                factory,
                Arc::new(FaultingLiveRunStore::new(
                    fixture.run_store.clone(),
                    LiveRunFault::PendingCheckpoint,
                )),
                fixture.agent_store.clone(),
            )
            .run_live(fixture.setup.clone());
        assert!(matches!(result, Err(AgentRuntimeError::Store(_))));
        let run = fixture
            .run_store
            .list_agent_runs()
            .unwrap()
            .into_iter()
            .find(|run| run.agent_id == Some(fixture.agent.id))
            .unwrap();
        let resume_factory = fixture.factory(vec![]);
        let outcome = fixture
            .runtime(resume_factory.clone())
            .run_live(fixture.resume_setup(run.id))
            .unwrap();
        assert!(matches!(
            outcome,
            AgentRuntimeOutcome::WaitingForApproval { .. }
        ));
        assert_eq!(resume_factory.creates.load(Ordering::SeqCst), 0);
        assert_eq!(resume_factory.gateway.sends.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn live_committed_finish_decision_recovers_and_seals_without_provider() {
        let fixture = LiveFixture::new();
        let factory = fixture.factory(vec![
            LiveGatewayAction::Tool(fixture.arguments.clone()),
            LiveGatewayAction::FinishFromToolOutput,
        ]);
        let waiting = fixture
            .runtime(factory.clone())
            .run_live(fixture.setup.clone())
            .unwrap();
        let AgentRuntimeOutcome::WaitingForApproval { run, request, .. } = waiting else {
            panic!("expected approval")
        };
        approve_live_wait(&fixture, &request);
        let result = fixture
            .runtime_with_stores(
                factory,
                Arc::new(FaultingLiveRunStore::new(
                    fixture.run_store.clone(),
                    LiveRunFault::FinalOutputCheckpoint,
                )),
                fixture.agent_store.clone(),
            )
            .run_live(fixture.resume_setup(run.id));
        assert!(matches!(result, Err(AgentRuntimeError::Store(_))));
        let resume_factory = fixture.factory(vec![]);
        let outcome = fixture
            .runtime(resume_factory.clone())
            .run_live(fixture.resume_setup(run.id))
            .unwrap();
        assert!(matches!(outcome, AgentRuntimeOutcome::Completed { .. }));
        assert_eq!(resume_factory.creates.load(Ordering::SeqCst), 0);
        assert_eq!(resume_factory.gateway.sends.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn live_persisted_final_output_resumes_terminal_seal_without_provider() {
        let fixture = LiveFixture::new();
        let factory = fixture.factory(vec![
            LiveGatewayAction::Tool(fixture.arguments.clone()),
            LiveGatewayAction::FinishFromToolOutput,
        ]);
        let waiting = fixture
            .runtime(factory.clone())
            .run_live(fixture.setup.clone())
            .unwrap();
        let AgentRuntimeOutcome::WaitingForApproval { run, request, .. } = waiting else {
            panic!("expected approval")
        };
        approve_live_wait(&fixture, &request);
        let result = fixture
            .runtime_with_stores(
                factory,
                Arc::new(FaultingLiveRunStore::new(
                    fixture.run_store.clone(),
                    LiveRunFault::TerminalVerificationRead,
                )),
                fixture.agent_store.clone(),
            )
            .run_live(fixture.resume_setup(run.id));
        assert!(matches!(result, Err(AgentRuntimeError::Store(_))));
        let state = fixture
            .runtime(fixture.factory(vec![]))
            .live_state(run.id)
            .unwrap();
        assert!(state.final_output.is_some());
        let resume_factory = fixture.factory(vec![]);
        let completed = fixture
            .runtime(resume_factory.clone())
            .run_live(fixture.resume_setup(run.id))
            .unwrap();
        assert!(matches!(completed, AgentRuntimeOutcome::Completed { .. }));
        assert_eq!(resume_factory.creates.load(Ordering::SeqCst), 0);
        assert_eq!(resume_factory.gateway.sends.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn live_retryable_checkpoint_crash_resumes_one_exact_linked_retry() {
        for first_failure in [
            ModelGatewayError::CertifiedNoBytes("certified".to_string()),
            ModelGatewayError::Explicit429("explicit 429".to_string()),
        ] {
            let fixture = LiveFixture::new();
            let factory = fixture.factory(vec![
                LiveGatewayAction::Error(first_failure),
                LiveGatewayAction::Tool(fixture.arguments.clone()),
            ]);
            let result = fixture
                .runtime_with_stores(
                    factory.clone(),
                    Arc::new(FaultingLiveRunStore::new(
                        fixture.run_store.clone(),
                        LiveRunFault::RetryPrepared,
                    )),
                    fixture.agent_store.clone(),
                )
                .run_live(fixture.setup.clone());
            assert!(matches!(result, Err(AgentRuntimeError::Store(_))));
            let run = fixture
                .run_store
                .list_agent_runs()
                .unwrap()
                .into_iter()
                .find(|run| run.agent_id == Some(fixture.agent.id))
                .unwrap();
            let outcome = fixture
                .runtime(factory.clone())
                .run_live(fixture.resume_setup(run.id))
                .unwrap();
            assert!(matches!(
                outcome,
                AgentRuntimeOutcome::WaitingForApproval { .. }
            ));
            let state = fixture.runtime(factory).live_state(run.id).unwrap();
            assert_eq!(state.attempts.len(), 2);
            assert_eq!(
                state.attempts[1].retry_of,
                Some(state.attempts[0].attempt_id)
            );
        }
    }

    #[test]
    fn live_approval_request_barrier_faults_recover_before_step_materialization() {
        for (fault, expected_request_count) in [
            (ApprovalRequestFault::Save, 0),
            (ApprovalRequestFault::Reread, 1),
        ] {
            let fixture = LiveFixture::new();
            let factory = fixture.factory(vec![LiveGatewayAction::Tool(fixture.arguments.clone())]);
            let result = fixture
                .runtime_with_all_stores(
                    factory,
                    fixture.run_store.clone(),
                    fixture.agent_store.clone(),
                    Arc::new(FaultingApprovalRequestStore {
                        inner: fixture.approval_store.clone(),
                        fault,
                        armed: AtomicBool::new(true),
                        saved: AtomicBool::new(false),
                    }),
                )
                .run_live(fixture.setup.clone());
            assert!(matches!(result, Err(AgentRuntimeError::Store(_))));
            let run = fixture
                .run_store
                .list_agent_runs()
                .unwrap()
                .into_iter()
                .find(|run| run.agent_id == Some(fixture.agent.id))
                .unwrap();
            assert_eq!(
                fixture
                    .approval_store
                    .list_approval_requests()
                    .unwrap()
                    .len(),
                expected_request_count
            );
            assert!(fixture
                .run_store
                .list_agent_run_steps(&run.id)
                .unwrap()
                .is_empty());
            let events = fixture.agent_store.list_agent_run_events(&run.id).unwrap();
            assert_eq!(
                events
                    .iter()
                    .filter(|event| matches!(
                        event.kind,
                        AgentRunEventKind::ToolRequested | AgentRunEventKind::ApprovalRequired
                    ))
                    .count(),
                0
            );
            assert!(fixture.execution_store.proofs.lock().unwrap().is_empty());
            assert!(fixture.execution_store.contexts.lock().unwrap().is_empty());

            let recovery_factory = fixture.factory(vec![]);
            assert!(matches!(
                fixture
                    .runtime(recovery_factory.clone())
                    .run_live(fixture.resume_setup(run.id))
                    .unwrap(),
                AgentRuntimeOutcome::WaitingForApproval { .. }
            ));
            assert_eq!(recovery_factory.creates.load(Ordering::SeqCst), 0);
            assert_eq!(recovery_factory.gateway.sends.load(Ordering::SeqCst), 0);
            assert_eq!(
                fixture
                    .approval_store
                    .list_approval_requests()
                    .unwrap()
                    .len(),
                1
            );
            assert_eq!(
                fixture
                    .run_store
                    .list_agent_run_steps(&run.id)
                    .unwrap()
                    .len(),
                1
            );
            let events = fixture.agent_store.list_agent_run_events(&run.id).unwrap();
            assert_eq!(
                events
                    .iter()
                    .filter(|event| event.kind == AgentRunEventKind::ToolRequested)
                    .count(),
                1
            );
            assert_eq!(
                events
                    .iter()
                    .filter(|event| event.kind == AgentRunEventKind::ApprovalRequired)
                    .count(),
                1
            );
        }
    }

    #[test]
    fn live_approval_intent_recovers_before_first_step_write_without_orphans() {
        let fixture = LiveFixture::new();
        let factory = fixture.factory(vec![LiveGatewayAction::Tool(fixture.arguments.clone())]);
        let result = fixture
            .runtime_with_stores(
                factory,
                Arc::new(FaultingLiveRunStore::new(
                    fixture.run_store.clone(),
                    LiveRunFault::FirstStepSave,
                )),
                fixture.agent_store.clone(),
            )
            .run_live(fixture.setup.clone());
        assert!(matches!(result, Err(AgentRuntimeError::Store(_))));
        let run = fixture
            .run_store
            .list_agent_runs()
            .unwrap()
            .into_iter()
            .find(|run| run.agent_id == Some(fixture.agent.id))
            .unwrap();
        assert_eq!(
            fixture
                .approval_store
                .list_approval_requests()
                .unwrap()
                .len(),
            1
        );
        assert!(fixture
            .run_store
            .list_agent_run_steps(&run.id)
            .unwrap()
            .is_empty());
        assert!(fixture.execution_store.proofs.lock().unwrap().is_empty());
        let resume_factory = fixture.factory(vec![]);
        let outcome = fixture
            .runtime(resume_factory.clone())
            .run_live(fixture.resume_setup(run.id))
            .unwrap();
        assert!(matches!(
            outcome,
            AgentRuntimeOutcome::WaitingForApproval { .. }
        ));
        assert_eq!(
            fixture
                .run_store
                .list_agent_run_steps(&run.id)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            fixture
                .approval_store
                .list_approval_requests()
                .unwrap()
                .len(),
            1
        );
        let events = fixture.agent_store.list_agent_run_events(&run.id).unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|e| e.kind == AgentRunEventKind::ToolRequested)
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|e| e.kind == AgentRunEventKind::ApprovalRequired)
                .count(),
            1
        );
        assert_eq!(resume_factory.gateway.sends.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn live_approval_step_and_event_faults_recover_exact_chronology() {
        for fault in [
            LiveAgentFault::ToolRequestedSave,
            LiveAgentFault::ApprovalRequiredSave,
        ] {
            let fixture = LiveFixture::new();
            let factory = fixture.factory(vec![LiveGatewayAction::Tool(fixture.arguments.clone())]);
            let result = fixture
                .runtime_with_stores(
                    factory,
                    fixture.run_store.clone(),
                    Arc::new(FaultingLiveAgentStore {
                        inner: fixture.agent_store.clone(),
                        fault,
                        armed: AtomicBool::new(true),
                        requested_saved: AtomicBool::new(false),
                    }),
                )
                .run_live(fixture.setup.clone());
            assert!(matches!(result, Err(AgentRuntimeError::Store(_))));
            let run = fixture
                .run_store
                .list_agent_runs()
                .unwrap()
                .into_iter()
                .find(|run| run.agent_id == Some(fixture.agent.id))
                .unwrap();
            assert_eq!(
                fixture
                    .approval_store
                    .list_approval_requests()
                    .unwrap()
                    .len(),
                1
            );
            assert_eq!(
                fixture
                    .run_store
                    .list_agent_run_steps(&run.id)
                    .unwrap()
                    .len(),
                1
            );
            assert!(fixture.execution_store.proofs.lock().unwrap().is_empty());

            let recovery_factory = fixture.factory(vec![]);
            assert!(matches!(
                fixture
                    .runtime(recovery_factory.clone())
                    .run_live(fixture.resume_setup(run.id))
                    .unwrap(),
                AgentRuntimeOutcome::WaitingForApproval { .. }
            ));
            assert_eq!(recovery_factory.creates.load(Ordering::SeqCst), 0);
            assert_eq!(recovery_factory.gateway.sends.load(Ordering::SeqCst), 0);
            assert_eq!(
                fixture
                    .approval_store
                    .list_approval_requests()
                    .unwrap()
                    .len(),
                1
            );
            assert_eq!(
                fixture
                    .run_store
                    .list_agent_run_steps(&run.id)
                    .unwrap()
                    .len(),
                1
            );
            let events = fixture.agent_store.list_agent_run_events(&run.id).unwrap();
            assert_eq!(
                events
                    .iter()
                    .filter(|event| event.kind == AgentRunEventKind::ToolRequested)
                    .count(),
                1
            );
            assert_eq!(
                events
                    .iter()
                    .filter(|event| event.kind == AgentRunEventKind::ApprovalRequired)
                    .count(),
                1
            );
            assert!(fixture.execution_store.proofs.lock().unwrap().is_empty());
            assert!(fixture.execution_store.contexts.lock().unwrap().is_empty());
        }
    }

    #[test]
    fn live_approval_request_substitution_or_signature_tamper_fails_before_effect() {
        for fault in [
            ApprovalRequestFault::SubstituteBody,
            ApprovalRequestFault::SubstituteSignature,
        ] {
            let fixture = LiveFixture::new();
            let waiting = fixture
                .runtime(fixture.factory(vec![LiveGatewayAction::Tool(fixture.arguments.clone())]))
                .run_live(fixture.setup.clone())
                .unwrap();
            let AgentRuntimeOutcome::WaitingForApproval { run, .. } = waiting else {
                panic!("expected approval")
            };
            let factory = fixture.factory(vec![]);
            let outcome = fixture
                .runtime_with_all_stores(
                    factory.clone(),
                    fixture.run_store.clone(),
                    fixture.agent_store.clone(),
                    Arc::new(FaultingApprovalRequestStore {
                        inner: fixture.approval_store.clone(),
                        fault,
                        armed: AtomicBool::new(true),
                        saved: AtomicBool::new(false),
                    }),
                )
                .run_live(fixture.resume_setup(run.id))
                .unwrap();
            assert!(matches!(outcome, AgentRuntimeOutcome::Failed { .. }));
            assert_eq!(factory.creates.load(Ordering::SeqCst), 0);
            assert_eq!(factory.gateway.sends.load(Ordering::SeqCst), 0);
            assert!(fixture.execution_store.proofs.lock().unwrap().is_empty());
            assert!(fixture.execution_store.contexts.lock().unwrap().is_empty());
            let events = fixture.agent_store.list_agent_run_events(&run.id).unwrap();
            assert_eq!(
                events
                    .iter()
                    .filter(|event| event.kind == AgentRunEventKind::ApprovalResumed)
                    .count(),
                0
            );
            assert_eq!(
                events
                    .iter()
                    .filter(|event| event.kind == AgentRunEventKind::ToolSucceeded)
                    .count(),
                0
            );
        }
    }

    #[test]
    fn live_execution_and_succeeded_step_faults_reconcile_exactly_once() {
        for fault in [
            LiveRunFault::SucceededStepSave,
            LiveRunFault::ContinuationCheckpoint,
        ] {
            let fixture = LiveFixture::new();
            let factory = fixture.factory(vec![
                LiveGatewayAction::Tool(fixture.arguments.clone()),
                LiveGatewayAction::FinishFromToolOutput,
            ]);
            let waiting = fixture
                .runtime(factory.clone())
                .run_live(fixture.setup.clone())
                .unwrap();
            let AgentRuntimeOutcome::WaitingForApproval { run, request, .. } = waiting else {
                panic!("expected approval")
            };
            approve_live_wait(&fixture, &request);
            let result = fixture
                .runtime_with_stores(
                    factory.clone(),
                    Arc::new(FaultingLiveRunStore::new(fixture.run_store.clone(), fault)),
                    fixture.agent_store.clone(),
                )
                .run_live(fixture.resume_setup(run.id));
            assert!(matches!(result, Err(AgentRuntimeError::Store(_))));
            let completed = fixture
                .runtime(factory)
                .run_live(fixture.resume_setup(run.id))
                .unwrap();
            assert!(matches!(completed, AgentRuntimeOutcome::Completed { .. }));
            assert_eq!(fixture.execution_store.proofs.lock().unwrap().len(), 1);
            assert_eq!(fixture.execution_store.contexts.lock().unwrap().len(), 1);
            assert_eq!(
                fixture
                    .run_store
                    .list_agent_run_steps(&run.id)
                    .unwrap()
                    .len(),
                1
            );
            let events = fixture.agent_store.list_agent_run_events(&run.id).unwrap();
            assert_eq!(
                events
                    .iter()
                    .filter(|e| e.kind == AgentRunEventKind::ApprovalResumed)
                    .count(),
                1
            );
            assert_eq!(
                events
                    .iter()
                    .filter(|e| e.kind == AgentRunEventKind::ToolSucceeded)
                    .count(),
                1
            );
        }
    }

    #[test]
    fn live_historical_approval_epoch_converges_after_two_consecutive_crashes() {
        let fixture = LiveFixture::new();
        let factory = fixture.factory(vec![
            LiveGatewayAction::Tool(fixture.arguments.clone()),
            LiveGatewayAction::FinishFromToolOutput,
        ]);
        let waiting = fixture
            .runtime(factory.clone())
            .run_live(fixture.setup.clone())
            .unwrap();
        let AgentRuntimeOutcome::WaitingForApproval { run, request, .. } = waiting else {
            panic!("expected approval")
        };
        approve_live_wait(&fixture, &request);

        let first = fixture
            .runtime_with_stores(
                factory.clone(),
                fixture.run_store.clone(),
                Arc::new(FailAfterApprovalResumedAgentStore {
                    inner: fixture.agent_store.clone(),
                    armed: AtomicBool::new(true),
                    resumed_saved: AtomicBool::new(false),
                }),
            )
            .run_live(fixture.resume_setup(run.id));
        assert!(matches!(first, Err(AgentRuntimeError::Store(_))));
        assert!(fixture.execution_store.proofs.lock().unwrap().is_empty());

        let second = fixture
            .runtime_with_stores(
                factory.clone(),
                Arc::new(FaultingLiveRunStore::new(
                    fixture.run_store.clone(),
                    LiveRunFault::ResumeEpochSave,
                )),
                fixture.agent_store.clone(),
            )
            .run_live(fixture.resume_setup(run.id));
        assert!(matches!(second, Err(AgentRuntimeError::Store(_))));
        assert!(fixture.execution_store.proofs.lock().unwrap().is_empty());

        let outcome = fixture
            .runtime(factory)
            .run_live(fixture.resume_setup(run.id))
            .unwrap();
        assert!(matches!(outcome, AgentRuntimeOutcome::Completed { .. }));
        assert_eq!(fixture.execution_store.proofs.lock().unwrap().len(), 1);
        assert_eq!(fixture.execution_store.contexts.lock().unwrap().len(), 1);
        let events = fixture.agent_store.list_agent_run_events(&run.id).unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|event| event.kind == AgentRunEventKind::ApprovalResumed)
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| event.kind == AgentRunEventKind::ToolSucceeded)
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| event.kind == AgentRunEventKind::Completed)
                .count(),
            1
        );
    }

    #[test]
    fn live_approval_resume_cannot_substitute_the_original_wait_epoch() {
        let fixture = LiveFixture::new();
        let waiting = fixture
            .runtime(fixture.factory(vec![LiveGatewayAction::Tool(fixture.arguments.clone())]))
            .run_live(fixture.setup.clone())
            .unwrap();
        let AgentRuntimeOutcome::WaitingForApproval { run, request, .. } = waiting else {
            panic!("expected approval")
        };
        approve_live_wait(&fixture, &request);
        let factory = fixture.factory(vec![]);
        let outcome = fixture
            .runtime_with_stores(
                factory.clone(),
                fixture.run_store.clone(),
                Arc::new(WaitEpochApprovalEventStore {
                    inner: fixture.agent_store.clone(),
                    wait_epoch: fixture.setup.process_epoch_id,
                }),
            )
            .run_live(fixture.resume_setup(run.id))
            .unwrap();
        assert!(matches!(outcome, AgentRuntimeOutcome::Failed { .. }));
        assert_eq!(factory.creates.load(Ordering::SeqCst), 0);
        assert_eq!(factory.gateway.sends.load(Ordering::SeqCst), 0);
        assert!(fixture.execution_store.proofs.lock().unwrap().is_empty());
        assert!(fixture.execution_store.contexts.lock().unwrap().is_empty());
    }

    #[test]
    fn live_terminal_success_and_failure_replay_is_read_only() {
        for succeeds in [true, false] {
            let fixture = LiveFixture::new();
            let initial_factory = if succeeds {
                fixture.factory(vec![
                    LiveGatewayAction::Tool(fixture.arguments.clone()),
                    LiveGatewayAction::FinishFromToolOutput,
                ])
            } else {
                fixture.factory(vec![LiveGatewayAction::Error(ModelGatewayError::Terminal(
                    "terminal rejection".to_string(),
                ))])
            };
            let run = if succeeds {
                let waiting = fixture
                    .runtime(initial_factory.clone())
                    .run_live(fixture.setup.clone())
                    .unwrap();
                let AgentRuntimeOutcome::WaitingForApproval { run, request, .. } = waiting else {
                    panic!("expected approval")
                };
                approve_live_wait(&fixture, &request);
                assert!(matches!(
                    fixture
                        .runtime(initial_factory)
                        .run_live(fixture.resume_setup(run.id))
                        .unwrap(),
                    AgentRuntimeOutcome::Completed { .. }
                ));
                run
            } else {
                let failed = fixture
                    .runtime(initial_factory)
                    .run_live(fixture.setup.clone())
                    .unwrap();
                let AgentRuntimeOutcome::Failed { run, .. } = failed else {
                    panic!("expected failure")
                };
                run
            };
            let checkpoints_before = fixture.run_store.list_agent_checkpoints(&run.id).unwrap();
            let events_before = fixture.agent_store.list_agent_run_events(&run.id).unwrap();
            let evaluations_before = fixture
                .run_store
                .list_agent_run_evaluations(&run.id)
                .unwrap();
            let proofs_before = fixture.execution_store.proofs.lock().unwrap().len();
            let contexts_before = fixture.execution_store.contexts.lock().unwrap().len();
            let persisted_epoch = fixture
                .runtime(fixture.factory(vec![]))
                .live_state(run.id)
                .unwrap()
                .process_epoch_id;

            let resume = fixture.resume_setup(run.id);
            let resume_factory = fixture.factory(vec![]);
            for setup in [resume.clone(), resume.clone(), fixture.resume_setup(run.id)] {
                let outcome = fixture
                    .runtime(resume_factory.clone())
                    .run_live(setup)
                    .unwrap();
                assert_eq!(
                    matches!(outcome, AgentRuntimeOutcome::Completed { .. }),
                    succeeds
                );
            }
            assert_eq!(resume_factory.creates.load(Ordering::SeqCst), 0);
            assert_eq!(resume_factory.gateway.sends.load(Ordering::SeqCst), 0);
            assert_eq!(
                fixture
                    .runtime(fixture.factory(vec![]))
                    .live_state(run.id)
                    .unwrap()
                    .process_epoch_id,
                persisted_epoch
            );
            assert_eq!(
                fixture.run_store.list_agent_checkpoints(&run.id).unwrap(),
                checkpoints_before
            );
            assert_eq!(
                fixture.agent_store.list_agent_run_events(&run.id).unwrap(),
                events_before
            );
            assert_eq!(
                fixture
                    .run_store
                    .list_agent_run_evaluations(&run.id)
                    .unwrap(),
                evaluations_before
            );
            assert_eq!(
                fixture.execution_store.proofs.lock().unwrap().len(),
                proofs_before
            );
            assert_eq!(
                fixture.execution_store.contexts.lock().unwrap().len(),
                contexts_before
            );

            let mut historical = fixture.resume_setup(run.id);
            historical.process_epoch_id = persisted_epoch;
            assert!(fixture
                .runtime(fixture.factory(vec![]))
                .run_live(historical)
                .is_err());
        }
    }

    #[test]
    fn live_sealed_success_evaluation_is_stable_across_principal_read_timestamps() {
        let fixture = LiveFixture::new();
        let approval_store = Arc::new(ReadTimestampApprovalStore {
            inner: fixture.approval_store.clone(),
            reads: AtomicUsize::new(0),
        });
        let initial_factory = fixture.factory(vec![
            LiveGatewayAction::Tool(fixture.arguments.clone()),
            LiveGatewayAction::FinishFromToolOutput,
        ]);
        let waiting = fixture
            .runtime_with_all_stores(
                initial_factory.clone(),
                fixture.run_store.clone(),
                fixture.agent_store.clone(),
                approval_store.clone(),
            )
            .run_live(fixture.setup.clone())
            .unwrap();
        let AgentRuntimeOutcome::WaitingForApproval { run, request, .. } = waiting else {
            panic!("expected approval")
        };
        approve_live_wait(&fixture, &request);
        let completed = fixture
            .runtime_with_all_stores(
                initial_factory,
                fixture.run_store.clone(),
                fixture.agent_store.clone(),
                approval_store.clone(),
            )
            .run_live(fixture.resume_setup(run.id))
            .unwrap();
        let AgentRuntimeOutcome::Completed { evaluation, .. } = completed else {
            panic!("expected completed live run")
        };
        assert_eq!(evaluation.outcome, AgentEvaluationOutcome::Passed);
        assert_eq!(evaluation.score_bps, Some(10_000));
        assert!(evaluation.metrics["complete_trace_digest"].is_string());
        assert!(evaluation.metrics["approval_verification_digest"].is_string());

        let checkpoints_before = fixture.run_store.list_agent_checkpoints(&run.id).unwrap();
        let events_before = fixture.agent_store.list_agent_run_events(&run.id).unwrap();
        let evaluations_before = fixture
            .run_store
            .list_agent_run_evaluations(&run.id)
            .unwrap();
        let proofs_before = fixture.execution_store.proofs.lock().unwrap().len();
        let contexts_before = fixture.execution_store.contexts.lock().unwrap().len();
        let reads_before = approval_store.reads.load(Ordering::SeqCst);
        let replay_factory = fixture.factory(vec![]);

        let replayed = fixture
            .runtime_with_all_stores(
                replay_factory.clone(),
                fixture.run_store.clone(),
                fixture.agent_store.clone(),
                approval_store.clone(),
            )
            .run_live(fixture.resume_setup(run.id))
            .unwrap();
        let AgentRuntimeOutcome::Completed {
            evaluation: replayed_evaluation,
            ..
        } = replayed
        else {
            panic!("expected read-only completed replay")
        };
        assert!(approval_store.reads.load(Ordering::SeqCst) > reads_before);
        assert_eq!(replayed_evaluation, evaluation);
        assert_eq!(replay_factory.creates.load(Ordering::SeqCst), 0);
        assert_eq!(replay_factory.gateway.sends.load(Ordering::SeqCst), 0);
        assert_eq!(
            fixture.run_store.list_agent_checkpoints(&run.id).unwrap(),
            checkpoints_before
        );
        assert_eq!(
            fixture.agent_store.list_agent_run_events(&run.id).unwrap(),
            events_before
        );
        assert_eq!(
            fixture
                .run_store
                .list_agent_run_evaluations(&run.id)
                .unwrap(),
            evaluations_before
        );
        assert_eq!(
            fixture.execution_store.proofs.lock().unwrap().len(),
            proofs_before
        );
        assert_eq!(
            fixture.execution_store.contexts.lock().unwrap().len(),
            contexts_before
        );
    }

    #[test]
    fn durable_principal_binding_excludes_only_the_read_timestamp() {
        let principal = principal_from_keypair(&generate_keypair_for(PrincipalKind::Human));
        let mut later_read = principal.clone();
        later_read.created_at += Duration::days(1);
        assert_eq!(
            durable_principal_binding(&later_read),
            durable_principal_binding(&principal)
        );

        let mut substituted = later_read;
        substituted.public_key = generate_keypair_for(PrincipalKind::Human)
            .signing_key
            .verifying_key();
        assert_ne!(
            durable_principal_binding(&substituted),
            durable_principal_binding(&principal)
        );
    }

    #[test]
    fn live_budget_terminal_replay_preserves_one_kind_event_and_evaluation() {
        let fixture = LiveFixture::new();
        let initial = fixture.factory(vec![LiveGatewayAction::Turn(ModelTurn {
            response_id: "resp_budget".to_string(),
            returned_model: Some(LIVE_MODEL.to_string()),
            response_body_digest: Some(value_digest(&json!("budget body")).unwrap()),
            decision: ModelDecision::ToolCall {
                call_id: "call_publish".to_string(),
                name: LIVE_TOOL_NAME.to_string(),
                arguments: fixture.arguments.as_value().unwrap(),
            },
            usage: ModelUsage {
                input_tokens: 1_000,
                output_tokens: 1_025,
                total_tokens: 2_025,
                cost_microusd: None,
            },
        })]);
        let failed = fixture
            .runtime(initial)
            .run_live(fixture.setup.clone())
            .unwrap();
        let AgentRuntimeOutcome::Failed { run, .. } = failed else {
            panic!("expected budget failure")
        };
        let checkpoints_before = fixture.run_store.list_agent_checkpoints(&run.id).unwrap();
        let events_before = fixture.agent_store.list_agent_run_events(&run.id).unwrap();
        let evaluations_before = fixture
            .run_store
            .list_agent_run_evaluations(&run.id)
            .unwrap();
        let assert_exact = || {
            let events = fixture.agent_store.list_agent_run_events(&run.id).unwrap();
            assert_eq!(
                events
                    .iter()
                    .filter(|event| {
                        matches!(
                            event.kind,
                            AgentRunEventKind::Failed | AgentRunEventKind::BudgetExceeded
                        )
                    })
                    .count(),
                1
            );
            assert_eq!(
                events
                    .iter()
                    .filter(|event| event.kind == AgentRunEventKind::BudgetExceeded)
                    .count(),
                1
            );
            assert_eq!(
                fixture
                    .run_store
                    .list_agent_run_evaluations(&run.id)
                    .unwrap()
                    .len(),
                1
            );
        };
        assert_exact();
        let first = fixture.resume_setup(run.id);
        let factory = fixture.factory(vec![]);
        assert!(matches!(
            fixture
                .runtime(factory.clone())
                .run_live(first.clone())
                .unwrap(),
            AgentRuntimeOutcome::Failed { .. }
        ));
        assert_eq!(factory.creates.load(Ordering::SeqCst), 0);
        assert_eq!(factory.gateway.sends.load(Ordering::SeqCst), 0);
        assert!(matches!(
            fixture
                .runtime(fixture.factory(vec![]))
                .run_live(first)
                .unwrap(),
            AgentRuntimeOutcome::Failed { .. }
        ));
        assert_exact();
        let factory = fixture.factory(vec![]);
        assert!(matches!(
            fixture
                .runtime(factory.clone())
                .run_live(fixture.resume_setup(run.id))
                .unwrap(),
            AgentRuntimeOutcome::Failed { .. }
        ));
        assert_eq!(factory.creates.load(Ordering::SeqCst), 0);
        assert_eq!(factory.gateway.sends.load(Ordering::SeqCst), 0);
        assert_exact();
        assert_eq!(
            fixture.run_store.list_agent_checkpoints(&run.id).unwrap(),
            checkpoints_before
        );
        assert_eq!(
            fixture.agent_store.list_agent_run_events(&run.id).unwrap(),
            events_before
        );
        assert_eq!(
            fixture
                .run_store
                .list_agent_run_evaluations(&run.id)
                .unwrap(),
            evaluations_before
        );
    }

    #[test]
    fn live_succeeded_status_without_completed_event_recovers_after_fresh_epoch() {
        let fixture = LiveFixture::new();
        let factory = fixture.factory(vec![
            LiveGatewayAction::Tool(fixture.arguments.clone()),
            LiveGatewayAction::FinishFromToolOutput,
        ]);
        let waiting = fixture
            .runtime(factory.clone())
            .run_live(fixture.setup.clone())
            .unwrap();
        let AgentRuntimeOutcome::WaitingForApproval { run, request, .. } = waiting else {
            panic!("expected approval")
        };
        approve_live_wait(&fixture, &request);
        let crashed = fixture
            .runtime_with_stores(
                factory,
                fixture.run_store.clone(),
                Arc::new(FaultingLiveAgentStore {
                    inner: fixture.agent_store.clone(),
                    fault: LiveAgentFault::TerminalSave,
                    armed: AtomicBool::new(true),
                    requested_saved: AtomicBool::new(false),
                }),
            )
            .run_live(fixture.resume_setup(run.id));
        assert!(matches!(crashed, Err(AgentRuntimeError::Store(_))));
        assert_eq!(
            fixture
                .run_store
                .load_agent_run(&run.id)
                .unwrap()
                .unwrap()
                .status,
            AgentRunStatus::Succeeded
        );
        assert_eq!(
            fixture
                .agent_store
                .list_agent_run_events(&run.id)
                .unwrap()
                .iter()
                .filter(|event| event.kind == AgentRunEventKind::Completed)
                .count(),
            0
        );
        let checkpoints_before = fixture.run_store.list_agent_checkpoints(&run.id).unwrap();
        let proofs_before = fixture.execution_store.proofs.lock().unwrap().len();
        let contexts_before = fixture.execution_store.contexts.lock().unwrap().len();

        let recovery_factory = fixture.factory(vec![]);
        assert!(matches!(
            fixture
                .runtime(recovery_factory.clone())
                .run_live(fixture.resume_setup(run.id))
                .unwrap(),
            AgentRuntimeOutcome::Completed { .. }
        ));
        assert_eq!(recovery_factory.creates.load(Ordering::SeqCst), 0);
        assert_eq!(recovery_factory.gateway.sends.load(Ordering::SeqCst), 0);
        let checkpoints_after = fixture.run_store.list_agent_checkpoints(&run.id).unwrap();
        assert_eq!(checkpoints_after.len(), checkpoints_before.len() + 1);
        assert_eq!(
            fixture.execution_store.proofs.lock().unwrap().len(),
            proofs_before
        );
        assert_eq!(
            fixture.execution_store.contexts.lock().unwrap().len(),
            contexts_before
        );
        assert_eq!(
            fixture
                .agent_store
                .list_agent_run_events(&run.id)
                .unwrap()
                .iter()
                .filter(|event| event.kind == AgentRunEventKind::Completed)
                .count(),
            1
        );
    }

    #[test]
    fn live_failed_status_without_terminal_event_recovers_after_fresh_epoch() {
        let fixture = LiveFixture::new();
        let factory = fixture.factory(vec![LiveGatewayAction::Error(ModelGatewayError::Terminal(
            "terminal rejection".to_string(),
        ))]);
        let crashed = fixture
            .runtime_with_stores(
                factory,
                fixture.run_store.clone(),
                Arc::new(FaultingLiveAgentStore {
                    inner: fixture.agent_store.clone(),
                    fault: LiveAgentFault::TerminalSave,
                    armed: AtomicBool::new(true),
                    requested_saved: AtomicBool::new(false),
                }),
            )
            .run_live(fixture.setup.clone());
        assert!(matches!(crashed, Err(AgentRuntimeError::Store(_))));
        let run = fixture
            .run_store
            .list_agent_runs()
            .unwrap()
            .into_iter()
            .find(|run| run.agent_id == Some(fixture.agent.id))
            .unwrap();
        assert_eq!(run.status, AgentRunStatus::Failed);
        assert_eq!(
            fixture
                .agent_store
                .list_agent_run_events(&run.id)
                .unwrap()
                .iter()
                .filter(|event| matches!(
                    event.kind,
                    AgentRunEventKind::Failed | AgentRunEventKind::BudgetExceeded
                ))
                .count(),
            0
        );
        let checkpoint_count = fixture
            .run_store
            .list_agent_checkpoints(&run.id)
            .unwrap()
            .len();
        let recovery_factory = fixture.factory(vec![]);
        assert!(matches!(
            fixture
                .runtime(recovery_factory.clone())
                .run_live(fixture.resume_setup(run.id))
                .unwrap(),
            AgentRuntimeOutcome::Failed { .. }
        ));
        assert_eq!(recovery_factory.creates.load(Ordering::SeqCst), 0);
        assert_eq!(recovery_factory.gateway.sends.load(Ordering::SeqCst), 0);
        assert_eq!(
            fixture
                .run_store
                .list_agent_checkpoints(&run.id)
                .unwrap()
                .len(),
            checkpoint_count + 1
        );
        assert_eq!(
            fixture
                .agent_store
                .list_agent_run_events(&run.id)
                .unwrap()
                .iter()
                .filter(|event| matches!(
                    event.kind,
                    AgentRunEventKind::Failed | AgentRunEventKind::BudgetExceeded
                ))
                .count(),
            1
        );
    }

    #[test]
    fn live_sealed_completed_event_recovers_only_missing_evaluation() {
        let fixture = LiveFixture::new();
        let approval_store = Arc::new(ReadTimestampApprovalStore {
            inner: fixture.approval_store.clone(),
            reads: AtomicUsize::new(0),
        });
        let factory = fixture.factory(vec![
            LiveGatewayAction::Tool(fixture.arguments.clone()),
            LiveGatewayAction::FinishFromToolOutput,
        ]);
        let waiting = fixture
            .runtime_with_all_stores(
                factory.clone(),
                fixture.run_store.clone(),
                fixture.agent_store.clone(),
                approval_store.clone(),
            )
            .run_live(fixture.setup.clone())
            .unwrap();
        let AgentRuntimeOutcome::WaitingForApproval { run, request, .. } = waiting else {
            panic!("expected approval")
        };
        approve_live_wait(&fixture, &request);
        let crashed = fixture
            .runtime_with_all_stores(
                factory,
                Arc::new(FaultingLiveRunStore::new(
                    fixture.run_store.clone(),
                    LiveRunFault::EvaluationSave,
                )),
                fixture.agent_store.clone(),
                approval_store.clone(),
            )
            .run_live(fixture.resume_setup(run.id));
        assert!(matches!(crashed, Err(AgentRuntimeError::Store(_))));
        assert_eq!(
            fixture
                .run_store
                .load_agent_run(&run.id)
                .unwrap()
                .unwrap()
                .status,
            AgentRunStatus::Succeeded
        );
        assert_eq!(
            fixture
                .run_store
                .list_agent_run_evaluations(&run.id)
                .unwrap()
                .len(),
            0
        );
        let checkpoints_before = fixture.run_store.list_agent_checkpoints(&run.id).unwrap();
        let events_before = fixture.agent_store.list_agent_run_events(&run.id).unwrap();
        let proofs_before = fixture.execution_store.proofs.lock().unwrap().len();
        let contexts_before = fixture.execution_store.contexts.lock().unwrap().len();

        let recovery_factory = fixture.factory(vec![]);
        assert!(matches!(
            fixture
                .runtime_with_all_stores(
                    recovery_factory.clone(),
                    fixture.run_store.clone(),
                    fixture.agent_store.clone(),
                    approval_store,
                )
                .run_live(fixture.resume_setup(run.id))
                .unwrap(),
            AgentRuntimeOutcome::Completed { .. }
        ));
        assert_eq!(recovery_factory.creates.load(Ordering::SeqCst), 0);
        assert_eq!(recovery_factory.gateway.sends.load(Ordering::SeqCst), 0);
        assert_eq!(
            fixture.run_store.list_agent_checkpoints(&run.id).unwrap(),
            checkpoints_before
        );
        assert_eq!(
            fixture.agent_store.list_agent_run_events(&run.id).unwrap(),
            events_before
        );
        assert_eq!(
            fixture
                .run_store
                .list_agent_run_evaluations(&run.id)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            fixture.execution_store.proofs.lock().unwrap().len(),
            proofs_before
        );
        assert_eq!(
            fixture.execution_store.contexts.lock().unwrap().len(),
            contexts_before
        );
    }

    #[test]
    fn live_sealed_failure_event_recovers_only_missing_evaluation() {
        let fixture = LiveFixture::new();
        let crashed = fixture
            .runtime_with_stores(
                fixture.factory(vec![LiveGatewayAction::Error(ModelGatewayError::Terminal(
                    "terminal rejection".to_string(),
                ))]),
                Arc::new(FaultingLiveRunStore::new(
                    fixture.run_store.clone(),
                    LiveRunFault::EvaluationSave,
                )),
                fixture.agent_store.clone(),
            )
            .run_live(fixture.setup.clone());
        assert!(matches!(crashed, Err(AgentRuntimeError::Store(_))));
        let run = fixture
            .run_store
            .list_agent_runs()
            .unwrap()
            .into_iter()
            .find(|run| run.agent_id == Some(fixture.agent.id))
            .unwrap();
        let checkpoints_before = fixture.run_store.list_agent_checkpoints(&run.id).unwrap();
        let events_before = fixture.agent_store.list_agent_run_events(&run.id).unwrap();
        assert!(fixture
            .run_store
            .list_agent_run_evaluations(&run.id)
            .unwrap()
            .is_empty());

        let recovery_factory = fixture.factory(vec![]);
        assert!(matches!(
            fixture
                .runtime(recovery_factory.clone())
                .run_live(fixture.resume_setup(run.id))
                .unwrap(),
            AgentRuntimeOutcome::Failed { .. }
        ));
        assert_eq!(recovery_factory.creates.load(Ordering::SeqCst), 0);
        assert_eq!(recovery_factory.gateway.sends.load(Ordering::SeqCst), 0);
        assert_eq!(
            fixture.run_store.list_agent_checkpoints(&run.id).unwrap(),
            checkpoints_before
        );
        assert_eq!(
            fixture.agent_store.list_agent_run_events(&run.id).unwrap(),
            events_before
        );
        assert_eq!(
            fixture
                .run_store
                .list_agent_run_evaluations(&run.id)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn live_failure_terminal_group_rejects_substitution_duplicate_or_both_kinds() {
        for mutation in 0..3 {
            let fixture = LiveFixture::new();
            let failed = fixture
                .runtime(fixture.factory(vec![LiveGatewayAction::Turn(ModelTurn {
                    response_id: "resp_budget".to_string(),
                    returned_model: Some(LIVE_MODEL.to_string()),
                    response_body_digest: Some(value_digest(&json!("budget body")).unwrap()),
                    decision: ModelDecision::ToolCall {
                        call_id: "call_publish".to_string(),
                        name: LIVE_TOOL_NAME.to_string(),
                        arguments: fixture.arguments.as_value().unwrap(),
                    },
                    usage: ModelUsage {
                        input_tokens: 1_000,
                        output_tokens: 1_025,
                        total_tokens: 2_025,
                        cost_microusd: None,
                    },
                })]))
                .run_live(fixture.setup.clone())
                .unwrap();
            let AgentRuntimeOutcome::Failed { run, .. } = failed else {
                panic!("expected budget failure")
            };
            let factory = fixture.factory(vec![]);
            let result = fixture
                .runtime_with_stores(
                    factory.clone(),
                    fixture.run_store.clone(),
                    Arc::new(TamperingFailureEventStore {
                        inner: fixture.agent_store.clone(),
                        mutation,
                    }),
                )
                .run_live(fixture.resume_setup(run.id));
            assert!(result.is_err());
            assert_eq!(factory.creates.load(Ordering::SeqCst), 0);
            assert_eq!(factory.gateway.sends.load(Ordering::SeqCst), 0);
        }
    }

    #[test]
    fn live_spurious_model_responded_event_is_rejected_before_approval_execution() {
        let fixture = LiveFixture::new();
        let waiting = fixture
            .runtime(fixture.factory(vec![LiveGatewayAction::Tool(fixture.arguments.clone())]))
            .run_live(fixture.setup.clone())
            .unwrap();
        let AgentRuntimeOutcome::WaitingForApproval { run, request, .. } = waiting else {
            panic!("expected approval")
        };
        approve_live_wait(&fixture, &request);
        let events = fixture.agent_store.list_agent_run_events(&run.id).unwrap();
        let responded = events
            .iter()
            .find(|event| event.kind == AgentRunEventKind::ModelResponded)
            .unwrap();
        let mut data = responded.data.clone();
        data["attempt_id"] = json!(Uuid::now_v7());
        let spurious = AgentRunEvent::create(
            run.id,
            events.last().unwrap().sequence + 1,
            AgentRunEventKind::ModelResponded,
            data,
            Utc::now(),
        )
        .unwrap();
        fixture.agent_store.save_agent_run_event(&spurious).unwrap();
        let factory = fixture.factory(vec![]);
        let outcome = fixture
            .runtime(factory.clone())
            .run_live(fixture.resume_setup(run.id))
            .unwrap();
        assert!(matches!(outcome, AgentRuntimeOutcome::Failed { .. }));
        assert_eq!(factory.creates.load(Ordering::SeqCst), 0);
        assert_eq!(factory.gateway.sends.load(Ordering::SeqCst), 0);
        assert!(fixture.execution_store.proofs.lock().unwrap().is_empty());
    }

    #[test]
    fn live_existing_approval_resume_chronology_is_rejected_before_execution() {
        let fixture = LiveFixture::new();
        let waiting = fixture
            .runtime(fixture.factory(vec![LiveGatewayAction::Tool(fixture.arguments.clone())]))
            .run_live(fixture.setup.clone())
            .unwrap();
        let AgentRuntimeOutcome::WaitingForApproval { run, request, step } = waiting else {
            panic!("expected approval")
        };
        approve_live_wait(&fixture, &request);
        let events = fixture.agent_store.list_agent_run_events(&run.id).unwrap();
        let required = events
            .iter()
            .find(|event| event.kind == AgentRunEventKind::ApprovalRequired)
            .unwrap();
        let resumed = AgentRunEvent::create(
            run.id,
            events.last().unwrap().sequence + 1,
            AgentRunEventKind::ApprovalResumed,
            serde_json::to_value(LiveApprovalResumedEvent::expected(
                step.id,
                request.body.id,
                fixture.setup.process_epoch_id,
            ))
            .unwrap(),
            required.created_at,
        )
        .unwrap();
        fixture.agent_store.save_agent_run_event(&resumed).unwrap();
        let factory = fixture.factory(vec![]);
        let outcome = fixture
            .runtime(factory.clone())
            .run_live(fixture.resume_setup(run.id))
            .unwrap();
        assert!(matches!(outcome, AgentRuntimeOutcome::Failed { .. }));
        assert_eq!(factory.creates.load(Ordering::SeqCst), 0);
        assert_eq!(factory.gateway.sends.load(Ordering::SeqCst), 0);
        assert!(fixture.execution_store.proofs.lock().unwrap().is_empty());
    }

    #[test]
    fn live_nested_checkpoint_and_ledger_tampering_is_rejected_before_factory() {
        for mutation in 0..5 {
            let fixture = LiveFixture::new();
            let start_factory =
                fixture.factory(vec![LiveGatewayAction::Tool(fixture.arguments.clone())]);
            let waiting = fixture
                .runtime(start_factory)
                .run_live(fixture.setup.clone())
                .unwrap();
            let AgentRuntimeOutcome::WaitingForApproval { run, .. } = waiting else {
                panic!("expected waiting run")
            };
            let checkpoints = fixture.run_store.list_agent_checkpoints(&run.id).unwrap();
            let mut state = checkpoints.last().unwrap().state.clone();
            match mutation {
                0 => state["runtime"]["provider"]["unknown"] = json!(true),
                1 => {
                    state["runtime"]["policy_evidence"]["resolved_policy"]["unknown"] = json!(true)
                }
                2 => {
                    state["runtime"]["attempts"][0]["response"]["usage"]["total_tokens"] = json!(41)
                }
                3 => state["runtime"]["counters"]["retries"] = json!(2),
                4 => {
                    state["runtime"]["attempts"][0]["prepared_at"] =
                        json!(state["runtime"]["started_at"].as_str().unwrap())
                }
                _ => unreachable!(),
            }
            let tampered = AgentCheckpoint::create(
                run.id,
                checkpoints.last().unwrap().sequence + 1,
                state,
                Utc::now(),
            )
            .unwrap();
            fixture.run_store.save_agent_checkpoint(&tampered).unwrap();
            let factory = fixture.factory(vec![]);
            let result = fixture
                .runtime(factory.clone())
                .run_live(fixture.resume_setup(run.id));
            assert!(
                matches!(result, Err(AgentRuntimeError::InvalidCheckpoint(id)) if id == run.id)
            );
            assert_eq!(factory.creates.load(Ordering::SeqCst), 0);
            assert_eq!(factory.gateway.sends.load(Ordering::SeqCst), 0);
        }
    }

    #[test]
    fn live_committed_event_substitutions_fail_even_with_recomputed_event_digest() {
        for mutation in 0..16 {
            let fixture = LiveFixture::new();
            let start_factory =
                fixture.factory(vec![LiveGatewayAction::Tool(fixture.arguments.clone())]);
            let waiting = fixture
                .runtime(start_factory)
                .run_live(fixture.setup.clone())
                .unwrap();
            let AgentRuntimeOutcome::WaitingForApproval { run, .. } = waiting else {
                panic!("expected waiting")
            };
            let factory = fixture.factory(vec![]);
            let outcome = fixture
                .runtime_with_stores(
                    factory.clone(),
                    fixture.run_store.clone(),
                    Arc::new(MutatingRespondedAgentStore {
                        inner: fixture.agent_store.clone(),
                        mutation,
                    }),
                )
                .run_live(fixture.resume_setup(run.id))
                .unwrap();
            assert!(matches!(outcome, AgentRuntimeOutcome::Failed { .. }));
            assert_eq!(factory.creates.load(Ordering::SeqCst), 0);
            assert_eq!(factory.gateway.sends.load(Ordering::SeqCst), 0);
        }
    }

    #[test]
    fn live_model_requested_event_substitution_unknown_or_duplicate_sends_zero_bytes() {
        for mutation in 0..3 {
            let fixture = LiveFixture::new();
            let factory = fixture.factory(vec![LiveGatewayAction::Tool(fixture.arguments.clone())]);
            let result = fixture
                .runtime_with_stores(
                    factory.clone(),
                    fixture.run_store.clone(),
                    Arc::new(TamperingLiveEventStore {
                        inner: fixture.agent_store.clone(),
                        target: LiveEventTamperTarget::ModelRequested,
                        mutation,
                    }),
                )
                .run_live(fixture.setup.clone());
            assert!(result.is_err());
            assert_eq!(factory.gateway.sends.load(Ordering::SeqCst), 0);
            assert!(fixture.execution_store.proofs.lock().unwrap().is_empty());
        }
    }

    #[test]
    fn live_approval_resumed_event_is_strict_exact_one_before_execution() {
        for mutation in 0..4 {
            let fixture = LiveFixture::new();
            let factory = fixture.factory(vec![LiveGatewayAction::Tool(fixture.arguments.clone())]);
            let waiting = fixture
                .runtime(factory.clone())
                .run_live(fixture.setup.clone())
                .unwrap();
            let AgentRuntimeOutcome::WaitingForApproval { run, request, .. } = waiting else {
                panic!("expected approval")
            };
            approve_live_wait(&fixture, &request);
            let outcome = fixture
                .runtime_with_stores(
                    factory,
                    fixture.run_store.clone(),
                    Arc::new(TamperingLiveEventStore {
                        inner: fixture.agent_store.clone(),
                        target: LiveEventTamperTarget::ApprovalResumed,
                        mutation,
                    }),
                )
                .run_live(fixture.resume_setup(run.id))
                .unwrap();
            assert!(matches!(outcome, AgentRuntimeOutcome::Failed { .. }));
            assert!(fixture.execution_store.proofs.lock().unwrap().is_empty());
            assert!(fixture.execution_store.contexts.lock().unwrap().is_empty());
        }
    }

    #[test]
    fn live_completed_event_substitution_removal_or_duplicate_blocks_final_evaluation() {
        for mutation in 0..4 {
            let fixture = LiveFixture::new();
            let factory = fixture.factory(vec![
                LiveGatewayAction::Tool(fixture.arguments.clone()),
                LiveGatewayAction::FinishFromToolOutput,
            ]);
            let waiting = fixture
                .runtime(factory.clone())
                .run_live(fixture.setup.clone())
                .unwrap();
            let AgentRuntimeOutcome::WaitingForApproval { run, request, .. } = waiting else {
                panic!("expected approval")
            };
            approve_live_wait(&fixture, &request);
            let result = fixture
                .runtime_with_stores(
                    factory,
                    fixture.run_store.clone(),
                    Arc::new(TamperingLiveEventStore {
                        inner: fixture.agent_store.clone(),
                        target: LiveEventTamperTarget::Completed,
                        mutation,
                    }),
                )
                .run_live(fixture.resume_setup(run.id));
            assert!(result.is_err());
            let persisted = fixture.run_store.load_agent_run(&run.id).unwrap().unwrap();
            assert_eq!(persisted.status, AgentRunStatus::Succeeded);
            assert!(fixture
                .run_store
                .list_agent_run_evaluations(&run.id)
                .unwrap()
                .iter()
                .all(|evaluation| {
                    evaluation.evaluator != LIVE_EVALUATOR
                        || evaluation.outcome != AgentEvaluationOutcome::Passed
                        || evaluation.score_bps != Some(10_000)
                }));
        }
    }

    #[test]
    fn live_attempt_ids_and_epochs_are_unique_v7_and_history_bound_before_factory() {
        for mutation in 0..4 {
            let fixture = LiveFixture::new();
            let waiting = fixture
                .runtime(fixture.factory(vec![
                    LiveGatewayAction::Error(ModelGatewayError::CertifiedNoBytes(
                        "retry".to_string(),
                    )),
                    LiveGatewayAction::Tool(fixture.arguments.clone()),
                ]))
                .run_live(fixture.setup.clone())
                .unwrap();
            let AgentRuntimeOutcome::WaitingForApproval { run, .. } = waiting else {
                panic!("expected waiting")
            };
            let checkpoints = fixture.run_store.list_agent_checkpoints(&run.id).unwrap();
            let mut state = checkpoints.last().unwrap().state.clone();
            match mutation {
                0 => state["runtime"]["attempts"][0]["attempt_id"] = json!(Uuid::nil()),
                1 => {
                    state["runtime"]["attempts"][1]["attempt_id"] =
                        state["runtime"]["attempts"][0]["attempt_id"].clone()
                }
                2 => state["runtime"]["attempts"][0]["process_epoch_id"] = json!(Uuid::nil()),
                3 => state["runtime"]["attempts"][0]["process_epoch_id"] = json!(Uuid::now_v7()),
                _ => unreachable!(),
            }
            let tampered = AgentCheckpoint::create(
                run.id,
                checkpoints.last().unwrap().sequence + 1,
                state,
                Utc::now(),
            )
            .unwrap();
            fixture.run_store.save_agent_checkpoint(&tampered).unwrap();
            let factory = fixture.factory(vec![]);
            let result = fixture
                .runtime(factory.clone())
                .run_live(fixture.resume_setup(run.id));
            assert!(matches!(
                result,
                Err(AgentRuntimeError::InvalidCheckpoint(_))
            ));
            assert_eq!(factory.creates.load(Ordering::SeqCst), 0);
            assert_eq!(factory.gateway.sends.load(Ordering::SeqCst), 0);
        }
    }

    #[test]
    fn live_missing_retry_link_and_reused_historical_epoch_are_rejected() {
        let fixture = LiveFixture::new();
        let factory = fixture.factory(vec![
            LiveGatewayAction::Error(ModelGatewayError::CertifiedNoBytes("retry".to_string())),
            LiveGatewayAction::Tool(fixture.arguments.clone()),
        ]);
        let waiting = fixture
            .runtime(factory)
            .run_live(fixture.setup.clone())
            .unwrap();
        let AgentRuntimeOutcome::WaitingForApproval { run, .. } = waiting else {
            panic!("expected waiting")
        };
        let checkpoints = fixture.run_store.list_agent_checkpoints(&run.id).unwrap();
        let mut state = checkpoints.last().unwrap().state.clone();
        state["runtime"]["attempts"][1]["retry_of"] = Value::Null;
        let tampered = AgentCheckpoint::create(
            run.id,
            checkpoints.last().unwrap().sequence + 1,
            state,
            Utc::now(),
        )
        .unwrap();
        fixture.run_store.save_agent_checkpoint(&tampered).unwrap();
        let result = fixture
            .runtime(fixture.factory(vec![]))
            .run_live(fixture.resume_setup(run.id));
        assert!(matches!(
            result,
            Err(AgentRuntimeError::InvalidCheckpoint(_))
        ));

        let fixture = LiveFixture::new();
        let original_epoch = fixture.setup.process_epoch_id;
        let waiting = fixture
            .runtime(fixture.factory(vec![LiveGatewayAction::Tool(fixture.arguments.clone())]))
            .run_live(fixture.setup.clone())
            .unwrap();
        let AgentRuntimeOutcome::WaitingForApproval { run, .. } = waiting else {
            panic!("expected waiting")
        };
        let first_resume = fixture.resume_setup(run.id);
        let first_epoch = first_resume.process_epoch_id;
        assert!(matches!(
            fixture
                .runtime(fixture.factory(vec![]))
                .run_live(first_resume)
                .unwrap(),
            AgentRuntimeOutcome::WaitingForApproval { .. }
        ));
        for reused in [original_epoch, first_epoch] {
            let mut setup = fixture.resume_setup(run.id);
            setup.process_epoch_id = reused;
            let factory = fixture.factory(vec![]);
            assert!(fixture.runtime(factory.clone()).run_live(setup).is_err());
            assert_eq!(factory.creates.load(Ordering::SeqCst), 0);
        }
    }

    #[test]
    fn live_resume_accepts_authority_valid_through_original_deadline() {
        let fixture = LiveFixture::new();
        let mut setup = fixture.setup.clone();
        setup.authority.delegation.valid_until = Utc::now() + Duration::seconds(301);
        reseal_fixture_delegation(&mut setup);
        let waiting = fixture
            .runtime(fixture.factory(vec![LiveGatewayAction::Tool(fixture.arguments.clone())]))
            .run_live(setup.clone())
            .unwrap();
        let AgentRuntimeOutcome::WaitingForApproval { run, .. } = waiting else {
            panic!("expected waiting")
        };
        setup.intent = LiveRunIntent::Resume { run_id: run.id };
        setup.process_epoch_id = Uuid::now_v7();
        let outcome = fixture
            .runtime(fixture.factory(vec![]))
            .run_live(setup)
            .unwrap();
        assert!(matches!(
            outcome,
            AgentRuntimeOutcome::WaitingForApproval { .. }
        ));
    }

    #[test]
    fn live_invalid_approval_signature_terminalizes_before_execution_or_continuation() {
        let fixture = LiveFixture::new();
        let factory = fixture.factory(vec![LiveGatewayAction::Tool(fixture.arguments.clone())]);
        let waiting = fixture
            .runtime(factory.clone())
            .run_live(fixture.setup.clone())
            .unwrap();
        let AgentRuntimeOutcome::WaitingForApproval { run, request, .. } = waiting else {
            panic!("expected approval wait")
        };
        let mut decision = SignedApprovalDecision::create(
            &request,
            ApprovalOutcome::Approved,
            Some("tampered".to_string()),
            Utc::now(),
            &fixture.approver,
        )
        .unwrap();
        decision.signature[0] ^= 0x01;
        fixture
            .approval_store
            .save_approval_decision(&decision)
            .unwrap();
        let outcome = fixture
            .runtime(factory.clone())
            .run_live(fixture.resume_setup(run.id))
            .unwrap();
        assert!(matches!(outcome, AgentRuntimeOutcome::Failed { .. }));
        assert_eq!(factory.creates.load(Ordering::SeqCst), 1);
        assert_eq!(factory.gateway.sends.load(Ordering::SeqCst), 1);
        assert!(fixture.execution_store.proofs.lock().unwrap().is_empty());
    }

    #[test]
    fn live_duplicate_tool_call_fails_after_exactly_one_publication() {
        let fixture = LiveFixture::new();
        let factory = fixture.factory(vec![
            LiveGatewayAction::Tool(fixture.arguments.clone()),
            LiveGatewayAction::Tool(fixture.arguments.clone()),
        ]);
        let waiting = fixture
            .runtime(factory.clone())
            .run_live(fixture.setup.clone())
            .unwrap();
        let AgentRuntimeOutcome::WaitingForApproval { run, request, .. } = waiting else {
            panic!("expected approval wait")
        };
        approve_live_wait(&fixture, &request);
        let outcome = fixture
            .runtime(factory.clone())
            .run_live(fixture.resume_setup(run.id))
            .unwrap();
        assert!(matches!(outcome, AgentRuntimeOutcome::Failed { .. }));
        assert_eq!(factory.creates.load(Ordering::SeqCst), 2);
        assert_eq!(factory.gateway.sends.load(Ordering::SeqCst), 2);
        assert_eq!(fixture.execution_store.proofs.lock().unwrap().len(), 1);
        assert_eq!(fixture.execution_store.contexts.lock().unwrap().len(), 1);
    }

    #[test]
    fn live_terminal_reference_substitution_fails_the_terminal_seal() {
        let fixture = LiveFixture::new();
        let factory = fixture.factory(vec![
            LiveGatewayAction::Tool(fixture.arguments.clone()),
            LiveGatewayAction::FinishWithOutput(
                "publication_id=substituted edition_id=substituted".to_string(),
            ),
        ]);
        let waiting = fixture
            .runtime(factory.clone())
            .run_live(fixture.setup.clone())
            .unwrap();
        let AgentRuntimeOutcome::WaitingForApproval { run, request, .. } = waiting else {
            panic!("expected approval wait")
        };
        approve_live_wait(&fixture, &request);
        let outcome = fixture
            .runtime(factory.clone())
            .run_live(fixture.resume_setup(run.id))
            .unwrap();
        assert!(matches!(outcome, AgentRuntimeOutcome::Failed { .. }));
        assert_eq!(fixture.execution_store.proofs.lock().unwrap().len(), 1);
        assert_eq!(fixture.execution_store.contexts.lock().unwrap().len(), 1);
    }

    #[test]
    fn live_check_only_chronology_tamper_cannot_persist_succeeded() {
        let fixture = LiveFixture::new();
        let factory = fixture.factory(vec![
            LiveGatewayAction::Tool(fixture.arguments.clone()),
            LiveGatewayAction::FinishFromToolOutput,
        ]);
        let waiting = fixture
            .runtime(factory.clone())
            .run_live(fixture.setup.clone())
            .unwrap();
        let AgentRuntimeOutcome::WaitingForApproval { run, request, step } = waiting else {
            panic!("expected approval wait")
        };
        let events = fixture.agent_store.list_agent_run_events(&run.id).unwrap();
        let duplicate = AgentRunEvent::create(
            run.id,
            events.last().unwrap().sequence + 1,
            AgentRunEventKind::ApprovalRequired,
            json!({
                "step_id": step.id,
                "request_id": request.body.id,
                "process_epoch_id": fixture.setup.process_epoch_id,
                "live": true,
            }),
            Utc::now(),
        )
        .unwrap();
        fixture
            .agent_store
            .save_agent_run_event(&duplicate)
            .unwrap();
        approve_live_wait(&fixture, &request);
        let outcome = fixture
            .runtime(factory)
            .run_live(fixture.resume_setup(run.id))
            .unwrap();
        assert!(matches!(outcome, AgentRuntimeOutcome::Failed { .. }));
        let persisted = fixture.run_store.load_agent_run(&run.id).unwrap().unwrap();
        assert_eq!(persisted.status, AgentRunStatus::Failed);
        assert!(fixture
            .run_store
            .list_agent_run_evaluations(&run.id)
            .unwrap()
            .iter()
            .all(|evaluation| evaluation.outcome == AgentEvaluationOutcome::Failed));
    }

    #[test]
    fn live_post_engine_crash_replays_without_second_mutation_and_keeps_timestamp() {
        let fixture = LiveFixture::new();
        let factory = fixture.factory(vec![
            LiveGatewayAction::Tool(fixture.arguments.clone()),
            LiveGatewayAction::FinishFromToolOutput,
        ]);
        let waiting = fixture
            .runtime(factory.clone())
            .run_live(fixture.setup.clone())
            .unwrap();
        let AgentRuntimeOutcome::WaitingForApproval { run, request, .. } = waiting else {
            panic!("expected approval wait")
        };
        approve_live_wait(&fixture, &request);
        let rejecting_approvals: Arc<dyn ApprovalStore> =
            Arc::new(RejectFirstExecutionApprovalStore {
                inner: fixture.approval_store.clone(),
                reject: AtomicBool::new(true),
            });
        let first_resume = fixture
            .runtime_with_all_stores(
                factory.clone(),
                fixture.run_store.clone(),
                fixture.agent_store.clone(),
                rejecting_approvals,
            )
            .run_live(fixture.resume_setup(run.id));
        assert!(matches!(first_resume, Err(AgentRuntimeError::Store(_))));
        assert_eq!(fixture.execution_store.proofs.lock().unwrap().len(), 1);
        assert_eq!(fixture.execution_store.contexts.lock().unwrap().len(), 1);

        let completed = fixture
            .runtime(factory)
            .run_live(fixture.resume_setup(run.id))
            .unwrap();
        assert!(matches!(completed, AgentRuntimeOutcome::Completed { .. }));
        assert_eq!(fixture.execution_store.proofs.lock().unwrap().len(), 1);
        assert_eq!(fixture.execution_store.contexts.lock().unwrap().len(), 1);
        let execution = fixture
            .approval_store
            .load_approval_execution(&request.body.id)
            .unwrap()
            .unwrap();
        assert_eq!(execution.executed_at, execution.proof.body.timestamp);
    }

    #[test]
    fn live_run_executes_one_approved_publication_and_passes_all_checks() {
        let fixture = LiveFixture::new();
        let factory = fixture.factory(vec![
            LiveGatewayAction::Tool(fixture.arguments.clone()),
            LiveGatewayAction::FinishFromToolOutput,
        ]);
        let waiting = fixture
            .runtime(factory.clone())
            .run_live(fixture.setup.clone())
            .unwrap();
        let AgentRuntimeOutcome::WaitingForApproval { run, request, .. } = waiting else {
            panic!("expected live approval wait")
        };
        assert_eq!(factory.creates.load(Ordering::SeqCst), 1);
        assert_eq!(factory.gateway.sends.load(Ordering::SeqCst), 1);
        assert!(fixture.execution_store.proofs.lock().unwrap().is_empty());
        approve_live_wait(&fixture, &request);

        let completed = fixture
            .runtime(factory.clone())
            .run_live(fixture.resume_setup(run.id))
            .unwrap();
        let AgentRuntimeOutcome::Completed {
            run, evaluation, ..
        } = &completed
        else {
            panic!("expected completed live outcome: {completed:?}")
        };
        assert_eq!(run.status, AgentRunStatus::Succeeded);
        assert_eq!(evaluation.outcome, AgentEvaluationOutcome::Passed);
        assert_eq!(evaluation.score_bps, Some(10_000));
        assert_eq!(evaluation.metrics["passed_checks"], 17);
        assert_eq!(evaluation.metrics["total_checks"], 17);
        assert_eq!(factory.creates.load(Ordering::SeqCst), 2);
        assert_eq!(factory.gateway.sends.load(Ordering::SeqCst), 2);
        assert_eq!(fixture.execution_store.proofs.lock().unwrap().len(), 1);
        assert_eq!(fixture.execution_store.contexts.lock().unwrap().len(), 1);
    }
}
