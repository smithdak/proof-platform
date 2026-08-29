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
    ApprovalStore, ArtifactKind, ExecutionContext, ExecutionEngine, Governance, Keypair,
    PrincipalKind, Registry, RegistryEntry, SignedApprovalRequest,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;
use uuid::Uuid;

use crate::model::{
    AgentFunctionTool, ModelDecision, ModelGateway, ModelInput, ModelTurnRequest, ModelUsage,
};

const RUNTIME_CHECKPOINT_KIND: &str = "agent_runtime_v1";
const RUNTIME_EVALUATOR: &str = "proof-agent-runtime/v1";

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
        if identity.kind != PrincipalKind::Agent {
            return Err(AgentRuntimeError::IdentityMustBeAgent);
        }
        Ok(Self {
            registry,
            engine,
            identity,
            workspace_path: workspace_path.into(),
            agent_store,
            run_store,
            approval_store,
            model,
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
        let state = AgentRuntimeState {
            agent_id: agent.id,
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
            terminal_error: None,
        };
        self.save_state(run.id, &state)?;
        self.append_event(
            run.id,
            AgentRunEventKind::Started,
            json!({"agent_id": agent.id, "goal": run.goal}),
        )?;
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
        let tools = self.resolve_tools(&agent)?;
        let state = self.state(run_id)?;
        if state.agent_id != agent_id {
            return Err(AgentRuntimeError::InvalidCheckpoint(run_id));
        }
        match run.status {
            AgentRunStatus::Succeeded | AgentRunStatus::Failed => self.terminal_outcome(run, state),
            AgentRunStatus::Running | AgentRunStatus::WaitingForInput => {
                self.drive(run, agent, tools, state)
            }
            status => Err(AgentRuntimeError::RunNotResumable { run_id, status }),
        }
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
        if self
            .agent_store
            .list_agent_run_events(&run_id)
            .map_err(AgentRuntimeError::Store)?
            .iter()
            .any(|event| event.kind == kind)
        {
            return Ok(());
        }
        self.append_event(run_id, kind, data)
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

    fn load_step(&self, step_id: Uuid) -> Result<AgentRunStep, AgentRuntimeError> {
        self.run_store
            .load_agent_run_step(&step_id)
            .map_err(AgentRuntimeError::Store)?
            .ok_or_else(|| {
                AgentRuntimeError::InconsistentState(format!("agent run step not found: {step_id}"))
            })
    }
}

enum ResumeApproval {
    Waiting(AgentRuntimeOutcome),
    Continue,
    Failed(String),
    BudgetExceeded(String),
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
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Mutex;

    use proof_kernel::{
        create_proof, generate_keypair, generate_keypair_for, principal_from_keypair, AgentLimits,
        AgentTool, ApprovalStore, Governance, OperationHandler, PrincipalKind,
        RecordingAgentRunStore, RecordingAgentStore, RecordingApprovalStore, RegistryEntry,
        SignedApprovalDecision, VersionStatus,
    };

    use super::*;
    use crate::model::{ModelGatewayError, ModelTurn};

    struct ScriptedGateway {
        turns: Mutex<VecDeque<Result<ModelTurn, ModelGatewayError>>>,
        requests: Mutex<Vec<ModelTurnRequest>>,
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
                self.agent_store.clone(),
                run_store,
                self.approval_store.clone(),
                model,
            )
            .unwrap()
        }
    }

    fn tool_turn() -> ModelTurn {
        ModelTurn {
            response_id: "resp_tool".to_string(),
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
}
