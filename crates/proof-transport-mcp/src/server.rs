use std::fs;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::Engine as _;
use proof_kernel::{
    canonicalize, digest, principal_from_keypair, AgentRun, AgentRunMode, AgentRunStatus,
    AgentRunStep, AgentRunStepStatus, AgentRunStore, ApprovalExecution, ApprovalGrant,
    ApprovalOutcome, ApprovalStore, ArtifactKind, ExecutionEngine, ExecutionStore, Governance,
    Keypair, OperationHandler, PrincipalId, PrincipalKind, RecordingAgentRunStore,
    RecordingApprovalStore, Registry, RegistryEntry, RegistryError, SignedApprovalRequest,
};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use thiserror::Error;

use crate::{
    handle_tool_call_with_approval, handle_tool_call_with_keypair, list_tools, schema_value,
    tool_name, validate_value, McpToolCall, McpToolResult,
};

pub const CURRENT_PROTOCOL_VERSION: &str = "2026-07-28";
pub const LEGACY_PROTOCOL_VERSION: &str = "2025-11-25";

const SERVER_NAME: &str = "proof-mcp";
const TOOL_CACHE_TTL_MS: u64 = 60_000;
const DISCOVERY_CACHE_TTL_MS: u64 = 3_600_000;
const IDENTITY_META_KEY: &str = "com.proofplatform/identity";
const EVIDENCE_META_KEY: &str = "com.proofplatform/evidence";
const APPROVAL_META_KEY: &str = "com.proofplatform/approval";
const RUN_META_KEY: &str = "com.proofplatform/run";
const APPROVAL_TTL_MINUTES: i64 = 15;

#[derive(Debug, Error)]
pub enum McpServerError {
    #[error("workspace keypair not found at {0}; run `proof init` first")]
    WorkspaceKeypairMissing(PathBuf),
    #[error("workspace registry not found at {0}; populate `.proof/registry` first")]
    RegistryMissing(PathBuf),
    #[error("failed to access {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid JSON in {path}: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("invalid base64 signing key: {0}")]
    SigningKeyEncoding(#[from] base64::DecodeError),
    #[error("stored signing key must be 32 bytes")]
    SigningKeyLength,
    #[error("stored keypair public key does not match its signing key")]
    PublicKeyMismatch,
    #[error(transparent)]
    Registry(#[from] RegistryError),
}

#[derive(Debug, Deserialize)]
struct StoredKeypair {
    principal_id: uuid::Uuid,
    kind: PrincipalKind,
    created_at: chrono::DateTime<chrono::Utc>,
    public_key: [u8; 32],
    signing_key: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProtocolEra {
    Modern,
    Legacy,
}

struct RunInvocation {
    run: AgentRun,
    step: AgentRunStep,
    replay: bool,
}

pub struct McpServer {
    registry: Registry,
    engine: ExecutionEngine,
    identity: Keypair,
    workspace_path: PathBuf,
    approval_store: Arc<dyn ApprovalStore>,
    run_store: Arc<dyn AgentRunStore>,
    legacy_protocol_version: Option<String>,
}

impl McpServer {
    pub fn new(registry: Registry, identity: Keypair, workspace_path: PathBuf) -> Self {
        let engine = ExecutionEngine::new_with_keypair(registry.clone(), identity.clone());
        Self {
            registry,
            engine,
            identity,
            workspace_path,
            approval_store: Arc::new(RecordingApprovalStore::default()),
            run_store: Arc::new(RecordingAgentRunStore::default()),
            legacy_protocol_version: None,
        }
    }

    /// Creates a server whose execution, approval, and run ledgers share one
    /// durable store while retaining [`McpServer::new`] for legacy callers.
    pub fn new_with_storage<S>(
        registry: Registry,
        identity: Keypair,
        workspace_path: PathBuf,
        storage: Arc<S>,
    ) -> Self
    where
        S: ExecutionStore + ApprovalStore + AgentRunStore + 'static,
    {
        let engine = ExecutionEngine::new_with_keypair(registry.clone(), identity.clone())
            .with_storage(storage.clone());
        Self {
            registry,
            engine,
            identity,
            workspace_path,
            approval_store: storage.clone(),
            run_store: storage,
            legacy_protocol_version: None,
        }
    }

    /// Uses a durable store for signed approval requests and resumable results.
    pub fn with_approval_store(mut self, approval_store: Arc<dyn ApprovalStore>) -> Self {
        self.approval_store = approval_store;
        self
    }

    /// Uses a durable store for agent runs, attempts, checkpoints, and evaluations.
    pub fn with_run_store(mut self, run_store: Arc<dyn AgentRunStore>) -> Self {
        self.run_store = run_store;
        self
    }

    pub fn register_handler(&mut self, handler: Arc<dyn OperationHandler>) {
        self.engine.register_handler(handler);
    }

    pub fn principal_id(&self) -> PrincipalId {
        self.identity.principal_id
    }

    pub fn handle_message(&mut self, message: Value) -> Option<Value> {
        let request = match message.as_object() {
            Some(request) => request,
            None => return Some(error_response(Value::Null, -32600, "Invalid Request", None)),
        };
        let id = request.get("id").cloned();
        if id.is_none() {
            return None;
        }
        let id = id.unwrap_or(Value::Null);
        if request.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return Some(error_response(id, -32600, "Invalid Request", None));
        }
        let Some(method) = request.get("method").and_then(Value::as_str) else {
            return Some(error_response(id, -32600, "Invalid Request", None));
        };
        let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
        if !params.is_object() {
            return Some(error_response(id, -32602, "Invalid params", None));
        }

        match method {
            "server/discover" => Some(self.discover(id, &params)),
            "initialize" => Some(self.initialize(id, &params)),
            "tools/list" => Some(self.list_tools(id, &params)),
            "tools/call" => Some(self.call_tool(id, &params)),
            "ping" => Some(self.ping(id, &params)),
            _ => Some(error_response(id, -32601, "Method not found", None)),
        }
    }

    pub fn serve_stdio(
        &mut self,
        mut input: impl BufRead,
        mut output: impl Write,
    ) -> Result<(), McpServerError> {
        let mut line = String::new();
        loop {
            line.clear();
            let bytes_read = input
                .read_line(&mut line)
                .map_err(|source| McpServerError::Io {
                    path: PathBuf::from("stdin"),
                    source,
                })?;
            if bytes_read == 0 {
                return Ok(());
            }
            let frame = line.trim_end_matches(['\r', '\n']);
            if frame.is_empty() {
                continue;
            }
            let response = match serde_json::from_str::<Value>(frame) {
                Ok(message) => self.handle_message(message),
                Err(_) => Some(error_response(Value::Null, -32700, "Parse error", None)),
            };
            if let Some(response) = response {
                serde_json::to_writer(&mut output, &response).map_err(|source| {
                    McpServerError::Json {
                        path: PathBuf::from("stdout"),
                        source,
                    }
                })?;
                output
                    .write_all(b"\n")
                    .map_err(|source| McpServerError::Io {
                        path: PathBuf::from("stdout"),
                        source,
                    })?;
                output.flush().map_err(|source| McpServerError::Io {
                    path: PathBuf::from("stdout"),
                    source,
                })?;
            }
        }
    }

    fn discover(&self, id: Value, params: &Value) -> Value {
        if let Err(response) = validate_modern_request(id.clone(), params) {
            return response;
        }
        success_response(
            id,
            json!({
                "resultType": "complete",
                "supportedVersions": [CURRENT_PROTOCOL_VERSION, LEGACY_PROTOCOL_VERSION],
                "capabilities": { "tools": { "listChanged": false } },
                "instructions": "Proof provides governed agent operations. Tool results include signed execution evidence in `com.proofplatform/evidence` metadata.",
                "ttlMs": DISCOVERY_CACHE_TTL_MS,
                "cacheScope": "private",
                "_meta": self.result_meta(None, None, None),
            }),
        )
    }

    fn initialize(&mut self, id: Value, params: &Value) -> Value {
        let requested = params
            .get("protocolVersion")
            .and_then(Value::as_str)
            .unwrap_or(LEGACY_PROTOCOL_VERSION);
        let selected = if requested == LEGACY_PROTOCOL_VERSION {
            requested
        } else {
            LEGACY_PROTOCOL_VERSION
        };
        self.legacy_protocol_version = Some(selected.to_string());
        success_response(
            id,
            json!({
                "protocolVersion": selected,
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": server_info(),
                "instructions": "Proof provides governed agent operations with signed execution evidence.",
            }),
        )
    }

    fn list_tools(&self, id: Value, params: &Value) -> Value {
        let era = match self.protocol_era(id.clone(), params) {
            Ok(era) => era,
            Err(response) => return response,
        };
        let cursor = params.get("cursor").and_then(Value::as_str);
        let tools = match list_tools(self.engine_registry(), cursor) {
            Ok(tools) => tools,
            Err(_) => return error_response(id, -32602, "Invalid tools cursor", None),
        };
        let mut result = serde_json::to_value(tools).expect("MCP tools are serializable");
        if era == ProtocolEra::Modern {
            let result = result
                .as_object_mut()
                .expect("serialized MCP tools list is an object");
            result.insert("resultType".to_string(), json!("complete"));
            result.insert("ttlMs".to_string(), json!(TOOL_CACHE_TTL_MS));
            result.insert("cacheScope".to_string(), json!("private"));
            result.insert("_meta".to_string(), self.result_meta(None, None, None));
        }
        success_response(id, result)
    }

    fn call_tool(&self, id: Value, params: &Value) -> Value {
        let era = match self.protocol_era(id.clone(), params) {
            Ok(era) => era,
            Err(response) => return response,
        };
        let Some(name) = params.get("name").and_then(Value::as_str) else {
            return error_response(id, -32602, "Tool name is required", None);
        };
        let Some(entry) = self
            .engine
            .operations()
            .iter()
            .find(|entry| tool_name(entry) == name)
        else {
            return error_response(id, -32602, "Unknown tool", Some(json!({ "name": name })));
        };
        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        if !arguments.is_object() {
            return error_response(id, -32602, "Tool arguments must be an object", None);
        }
        let call = McpToolCall {
            name: name.to_string(),
            arguments,
        };
        if let Err(error) =
            validate_value(&schema_value(entry, &entry.input_schema), &call.arguments)
        {
            return self.tool_error_response(
                id,
                era,
                format!("Invalid input for {}: {error}", entry.operation),
                None,
                None,
            );
        }
        let invocation = match self.begin_run_invocation(params, entry, &call) {
            Ok(invocation) => invocation,
            Err(error) => {
                return self.tool_error_response(id, era, error, None, None);
            }
        };
        if entry.governance == Governance::HumanOnly {
            return self.call_human_only_tool(id, era, params, entry, &call, invocation);
        }
        let result = handle_tool_call_with_keypair(
            &call,
            &self.engine,
            &self.identity,
            self.workspace_path.clone(),
        );
        let mut invocation = invocation;
        if let Err(error) = self.finish_run_invocation(&mut invocation, &result) {
            return error_response(
                id,
                -32603,
                "Agent run persistence failed after execution",
                Some(json!({"detail": error, "run": run_metadata(&invocation)})),
            );
        }
        self.tool_result_response(id, era, result, None, Some(run_metadata(&invocation)))
    }

    fn begin_run_invocation(
        &self,
        params: &Value,
        entry: &RegistryEntry,
        call: &McpToolCall,
    ) -> Result<RunInvocation, String> {
        if let Some(request_id) = params
            .get("requestState")
            .and_then(Value::as_str)
            .and_then(|request_id| uuid::Uuid::parse_str(request_id).ok())
        {
            if let Some(mut step) = self
                .run_store
                .find_agent_run_step_by_approval(&request_id)?
            {
                validate_step_call(&step, entry, call)?;
                let mut run = self
                    .run_store
                    .load_agent_run(&step.run_id)?
                    .ok_or_else(|| format!("agent run not found: {}", step.run_id))?;
                self.ensure_run_actor(&run)?;
                return match step.status {
                    AgentRunStepStatus::WaitingForApproval => {
                        if run.status != AgentRunStatus::WaitingForInput {
                            return Err(format!(
                                "agent run {} is not waiting for approval",
                                run.id
                            ));
                        }
                        run.resume(chrono::Utc::now())
                            .map_err(|error| error.to_string())?;
                        self.run_store.save_agent_run(&run)?;
                        step.resume_from_approval(chrono::Utc::now())
                            .map_err(|error| error.to_string())?;
                        self.run_store.save_agent_run_step(&step)?;
                        Ok(RunInvocation {
                            run,
                            step,
                            replay: false,
                        })
                    }
                    status if status.is_terminal() => Ok(RunInvocation {
                        run,
                        step,
                        replay: true,
                    }),
                    _ => Err(format!(
                        "agent run step {} cannot resume from approval while {:?}",
                        step.id, step.status
                    )),
                };
            }
        }

        let run_meta = params.get("_meta").and_then(|meta| meta.get(RUN_META_KEY));
        if run_meta.is_some_and(|meta| !meta.is_object()) {
            return Err(format!("{RUN_META_KEY} metadata must be an object"));
        }
        let run_id = run_meta
            .and_then(|meta| meta.get("runId"))
            .and_then(Value::as_str)
            .map(uuid::Uuid::parse_str)
            .transpose()
            .map_err(|_| "invalid agent run ID".to_string())?;
        let step_id = run_meta
            .and_then(|meta| meta.get("stepId"))
            .and_then(Value::as_str)
            .map(uuid::Uuid::parse_str)
            .transpose()
            .map_err(|_| "invalid agent run step ID".to_string())?;
        if step_id.is_some() && run_id.is_none() {
            return Err("agent run step metadata requires runId".to_string());
        }

        if let Some(run_id) = run_id {
            let mut run = self
                .run_store
                .load_agent_run(&run_id)?
                .ok_or_else(|| format!("agent run not found: {run_id}"))?;
            self.ensure_run_actor(&run)?;
            if run.status == AgentRunStatus::Queued {
                run.start(chrono::Utc::now())
                    .map_err(|error| error.to_string())?;
                self.run_store.save_agent_run(&run)?;
            }
            if run.status != AgentRunStatus::Running {
                return Err(format!(
                    "agent run {run_id} cannot accept a tool call while {:?}",
                    run.status
                ));
            }
            let mut step = match step_id {
                Some(step_id) => {
                    let step = self
                        .run_store
                        .load_agent_run_step(&step_id)?
                        .ok_or_else(|| format!("agent run step not found: {step_id}"))?;
                    if step.run_id != run.id {
                        return Err(format!(
                            "agent run step {step_id} does not belong to run {}",
                            run.id
                        ));
                    }
                    if step.status != AgentRunStepStatus::Pending {
                        return Err(format!(
                            "agent run step {step_id} cannot start while {:?}",
                            step.status
                        ));
                    }
                    validate_step_call(&step, entry, call)?;
                    step
                }
                None => {
                    let steps = self.run_store.list_agent_run_steps(&run.id)?;
                    let ordinal = match steps.iter().map(|step| step.ordinal).max() {
                        Some(ordinal) => ordinal
                            .checked_add(1)
                            .ok_or_else(|| "agent run step ordinal overflow".to_string())?,
                        None => 0,
                    };
                    let step = AgentRunStep::new(
                        run.id,
                        ordinal,
                        &entry.operation,
                        &entry.version,
                        &call.arguments,
                        chrono::Utc::now(),
                    )
                    .map_err(|error| error.to_string())?;
                    self.run_store.save_agent_run_step(&step)?;
                    step
                }
            };
            step.start(chrono::Utc::now())
                .map_err(|error| error.to_string())?;
            self.run_store.save_agent_run_step(&step)?;
            return Ok(RunInvocation {
                run,
                step,
                replay: false,
            });
        }

        let mode = match run_meta
            .and_then(|meta| meta.get("mode"))
            .and_then(Value::as_str)
        {
            Some("session") => AgentRunMode::Session,
            Some("one_shot") | None => AgentRunMode::OneShot,
            Some(mode) => return Err(format!("unknown agent run mode: {mode}")),
        };
        let goal = run_meta
            .and_then(|meta| meta.get("goal"))
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .unwrap_or_else(|| format!("Execute {} {}", entry.operation, entry.version));
        let mut run = AgentRun::new(self.identity.principal_id, mode, goal, chrono::Utc::now())
            .map_err(|error| error.to_string())?;
        self.run_store.save_agent_run(&run)?;
        run.start(chrono::Utc::now())
            .map_err(|error| error.to_string())?;
        self.run_store.save_agent_run(&run)?;
        let mut step = AgentRunStep::new(
            run.id,
            0,
            &entry.operation,
            &entry.version,
            &call.arguments,
            chrono::Utc::now(),
        )
        .map_err(|error| error.to_string())?;
        self.run_store.save_agent_run_step(&step)?;
        step.start(chrono::Utc::now())
            .map_err(|error| error.to_string())?;
        self.run_store.save_agent_run_step(&step)?;
        Ok(RunInvocation {
            run,
            step,
            replay: false,
        })
    }

    fn finish_run_invocation(
        &self,
        invocation: &mut RunInvocation,
        result: &McpToolResult,
    ) -> Result<(), String> {
        if invocation.replay {
            return Ok(());
        }
        let now = chrono::Utc::now();
        if result.is_error {
            let message = result
                .content
                .iter()
                .map(|content| content.text.as_str())
                .collect::<Vec<_>>()
                .join("; ");
            invocation
                .step
                .fail(message, now)
                .map_err(|error| error.to_string())?;
            self.run_store.save_agent_run_step(&invocation.step)?;
            invocation
                .run
                .fail(now)
                .map_err(|error| error.to_string())?;
            self.run_store.save_agent_run(&invocation.run)?;
            return Ok(());
        }
        let structured = result
            .structured_content
            .as_ref()
            .ok_or_else(|| "successful tool result has no structured content".to_string())?;
        let output = structured.get("result").cloned().unwrap_or(Value::Null);
        let proof = structured
            .get("proof")
            .cloned()
            .ok_or_else(|| "successful tool result has no proof".to_string())?;
        let proof = serde_json::from_value(proof).map_err(|error| error.to_string())?;
        invocation
            .step
            .succeed(output, proof, now)
            .map_err(|error| error.to_string())?;
        self.run_store.save_agent_run_step(&invocation.step)?;
        if invocation.run.mode == AgentRunMode::OneShot {
            invocation
                .run
                .succeed(now)
                .map_err(|error| error.to_string())?;
            self.run_store.save_agent_run(&invocation.run)?;
        }
        Ok(())
    }

    fn wait_run_invocation(
        &self,
        invocation: &mut RunInvocation,
        approval_request_id: uuid::Uuid,
    ) -> Result<(), String> {
        if invocation.replay {
            return Ok(());
        }
        let now = chrono::Utc::now();
        invocation
            .step
            .wait_for_approval(approval_request_id, now)
            .map_err(|error| error.to_string())?;
        self.run_store.save_agent_run_step(&invocation.step)?;
        invocation
            .run
            .wait_for_input(now)
            .map_err(|error| error.to_string())?;
        self.run_store.save_agent_run(&invocation.run)
    }

    fn fail_run_invocation(
        &self,
        invocation: &mut RunInvocation,
        message: &str,
    ) -> Result<(), String> {
        if invocation.replay {
            return Ok(());
        }
        let now = chrono::Utc::now();
        invocation
            .step
            .fail(message, now)
            .map_err(|error| error.to_string())?;
        self.run_store.save_agent_run_step(&invocation.step)?;
        invocation
            .run
            .fail(now)
            .map_err(|error| error.to_string())?;
        self.run_store.save_agent_run(&invocation.run)
    }

    fn ensure_run_actor(&self, run: &AgentRun) -> Result<(), String> {
        if run.actor != self.identity.principal_id {
            return Err(format!(
                "agent run {} belongs to a different principal",
                run.id
            ));
        }
        Ok(())
    }

    fn call_human_only_tool(
        &self,
        id: Value,
        era: ProtocolEra,
        params: &Value,
        entry: &RegistryEntry,
        call: &McpToolCall,
        mut invocation: RunInvocation,
    ) -> Value {
        let request_state = match params.get("requestState") {
            Some(Value::String(request_state)) => Some(request_state.as_str()),
            Some(_) => {
                return self.fail_run_tool_response(
                    id,
                    era,
                    "requestState must be a string".to_string(),
                    None,
                    &mut invocation,
                );
            }
            None => None,
        };
        let request = match request_state {
            Some(request_state) => {
                let request_id = match uuid::Uuid::parse_str(request_state) {
                    Ok(request_id) => request_id,
                    Err(_) => {
                        return self.fail_run_tool_response(
                            id,
                            era,
                            "Invalid approval requestState".to_string(),
                            None,
                            &mut invocation,
                        );
                    }
                };
                match self.approval_store.load_approval_request(&request_id) {
                    Ok(Some(request)) => request,
                    Ok(None) => {
                        return self.fail_run_tool_response(
                            id,
                            era,
                            format!("Approval request not found: {request_id}"),
                            None,
                            &mut invocation,
                        );
                    }
                    Err(error) => {
                        return self.fail_run_tool_response(
                            id,
                            era,
                            format!("Approval storage failed: {error}"),
                            None,
                            &mut invocation,
                        );
                    }
                }
            }
            None => {
                let requested_at = chrono::Utc::now();
                let request = match SignedApprovalRequest::create(
                    &entry.operation,
                    &entry.version,
                    &call.arguments,
                    requested_at,
                    requested_at + chrono::Duration::minutes(APPROVAL_TTL_MINUTES),
                    &self.identity,
                ) {
                    Ok(request) => request,
                    Err(error) => {
                        return self.fail_run_tool_response(
                            id,
                            era,
                            format!("Could not create approval request: {error}"),
                            None,
                            &mut invocation,
                        );
                    }
                };
                if let Err(error) = self.approval_store.save_approval_request(&request) {
                    return self.fail_run_tool_response(
                        id,
                        era,
                        format!("Approval storage failed: {error}"),
                        None,
                        &mut invocation,
                    );
                }
                if let Err(error) = self.wait_run_invocation(&mut invocation, request.body.id) {
                    return error_response(
                        id,
                        -32603,
                        "Agent run persistence failed while awaiting approval",
                        Some(json!({"detail": error, "run": run_metadata(&invocation)})),
                    );
                }
                return self.approval_required_response(
                    id,
                    era,
                    &request,
                    Some(run_metadata(&invocation)),
                );
            }
        };

        let requester = principal_from_keypair(&self.identity);
        if let Err(error) = request.verify_for_call(
            &requester,
            &entry.operation,
            &entry.version,
            &call.arguments,
            self.identity.principal_id,
            chrono::Utc::now(),
        ) {
            return self.fail_run_tool_response(
                id,
                era,
                format!("Approval request does not authorize this call: {error}"),
                Some(json!({"status": "invalid", "request": request})),
                &mut invocation,
            );
        }

        let decision = match self.approval_store.load_approval_decision(&request.body.id) {
            Ok(Some(decision)) => decision,
            Ok(None) => {
                if let Err(error) = self.wait_run_invocation(&mut invocation, request.body.id) {
                    return error_response(
                        id,
                        -32603,
                        "Agent run persistence failed while awaiting approval",
                        Some(json!({"detail": error, "run": run_metadata(&invocation)})),
                    );
                }
                return self.approval_required_response(
                    id,
                    era,
                    &request,
                    Some(run_metadata(&invocation)),
                );
            }
            Err(error) => {
                return self.fail_run_tool_response(
                    id,
                    era,
                    format!("Approval storage failed: {error}"),
                    None,
                    &mut invocation,
                );
            }
        };
        let approver = match self
            .approval_store
            .load_trusted_approver(&decision.body.decided_by)
        {
            Ok(Some(approver)) => approver,
            Ok(None) => {
                return self.fail_run_tool_response(
                    id,
                    era,
                    "Approval decision was not signed by an enrolled human".to_string(),
                    Some(json!({
                        "status": "untrusted",
                        "request": request,
                        "decision": decision,
                    })),
                    &mut invocation,
                );
            }
            Err(error) => {
                return self.fail_run_tool_response(
                    id,
                    era,
                    format!("Approval storage failed: {error}"),
                    None,
                    &mut invocation,
                );
            }
        };
        let grant = ApprovalGrant {
            request,
            decision,
            approver,
        };
        let trusted_approver = grant.approver.clone();
        if let Err(error) = grant.verify_decision(&requester, &trusted_approver) {
            return self.fail_run_tool_response(
                id,
                era,
                format!("Approval decision verification failed: {error}"),
                Some(approval_metadata("invalid", &grant)),
                &mut invocation,
            );
        }
        if grant.decision.body.outcome == ApprovalOutcome::Denied {
            return self.fail_run_tool_response(
                id,
                era,
                grant
                    .decision
                    .body
                    .reason
                    .as_deref()
                    .map(|reason| format!("Human approval denied: {reason}"))
                    .unwrap_or_else(|| "Human approval denied".to_string()),
                Some(approval_metadata("denied", &grant)),
                &mut invocation,
            );
        }
        if let Err(error) = grant.verify_for_execution(
            &self.identity,
            &trusted_approver,
            &entry.operation,
            &entry.version,
            &call.arguments,
            self.identity.principal_id,
            chrono::Utc::now(),
        ) {
            return self.fail_run_tool_response(
                id,
                era,
                format!("Approval does not authorize execution: {error}"),
                Some(approval_metadata("invalid", &grant)),
                &mut invocation,
            );
        }

        match self
            .approval_store
            .load_approval_execution(&grant.request.body.id)
        {
            Ok(Some(execution)) => {
                if let Err(error) = self.verify_approval_execution(&execution, &grant, entry) {
                    return self.fail_run_tool_response(
                        id,
                        era,
                        format!("Stored approval execution failed verification: {error}"),
                        Some(approval_metadata("invalid", &grant)),
                        &mut invocation,
                    );
                }
                let proof = match serde_json::to_value(&execution.proof) {
                    Ok(proof) => proof,
                    Err(error) => {
                        return self.fail_run_tool_response(
                            id,
                            era,
                            format!("Stored approval proof is invalid: {error}"),
                            Some(approval_metadata("invalid", &grant)),
                            &mut invocation,
                        );
                    }
                };
                let result = McpToolResult::execution(execution.output, proof);
                if let Err(error) = self.finish_run_invocation(&mut invocation, &result) {
                    return error_response(
                        id,
                        -32603,
                        "Agent run persistence failed after approval replay",
                        Some(json!({"detail": error, "run": run_metadata(&invocation)})),
                    );
                }
                return self.tool_result_response(
                    id,
                    era,
                    result,
                    Some(approval_metadata("executed", &grant)),
                    Some(run_metadata(&invocation)),
                );
            }
            Ok(None) => {}
            Err(error) => {
                return self.fail_run_tool_response(
                    id,
                    era,
                    format!("Approval storage failed: {error}"),
                    None,
                    &mut invocation,
                );
            }
        }

        let result = handle_tool_call_with_approval(
            call,
            &self.engine,
            &self.identity,
            self.workspace_path.clone(),
            &grant,
            &trusted_approver,
        );
        if result.is_error {
            if let Err(error) = self.finish_run_invocation(&mut invocation, &result) {
                return error_response(
                    id,
                    -32603,
                    "Agent run persistence failed after approved execution",
                    Some(json!({"detail": error, "run": run_metadata(&invocation)})),
                );
            }
            return self.tool_result_response(
                id,
                era,
                result,
                Some(approval_metadata("execution_failed", &grant)),
                Some(run_metadata(&invocation)),
            );
        }
        let Some(structured) = result.structured_content.as_ref() else {
            return self.fail_run_tool_response(
                id,
                era,
                "Approved execution returned no structured result".to_string(),
                Some(approval_metadata("execution_failed", &grant)),
                &mut invocation,
            );
        };
        let Some(proof_value) = structured.get("proof") else {
            return self.fail_run_tool_response(
                id,
                era,
                "Approved execution returned no proof".to_string(),
                Some(approval_metadata("execution_failed", &grant)),
                &mut invocation,
            );
        };
        let proof: proof_kernel::Proof = match serde_json::from_value(proof_value.clone()) {
            Ok(proof) => proof,
            Err(error) => {
                return self.fail_run_tool_response(
                    id,
                    era,
                    format!("Approved execution returned an invalid proof: {error}"),
                    Some(approval_metadata("execution_failed", &grant)),
                    &mut invocation,
                );
            }
        };
        let execution = ApprovalExecution {
            request_id: grant.request.body.id,
            executed_at: proof.body.timestamp,
            output: structured.get("result").cloned().unwrap_or(Value::Null),
            proof,
        };
        if let Err(error) = self.verify_approval_execution(&execution, &grant, entry) {
            return self.fail_run_tool_response(
                id,
                era,
                format!("Approved execution failed integrity verification: {error}"),
                Some(approval_metadata("execution_failed", &grant)),
                &mut invocation,
            );
        }
        if let Err(error) = self.approval_store.save_approval_execution(&execution) {
            return self.fail_run_tool_response(
                id,
                era,
                format!("Approved operation executed, but its replay record failed: {error}"),
                Some(approval_metadata("persistence_failed", &grant)),
                &mut invocation,
            );
        }
        if let Err(error) = self.finish_run_invocation(&mut invocation, &result) {
            return error_response(
                id,
                -32603,
                "Agent run persistence failed after approved execution",
                Some(json!({"detail": error, "run": run_metadata(&invocation)})),
            );
        }
        self.tool_result_response(
            id,
            era,
            result,
            Some(approval_metadata("executed", &grant)),
            Some(run_metadata(&invocation)),
        )
    }

    fn verify_approval_execution(
        &self,
        execution: &ApprovalExecution,
        grant: &ApprovalGrant,
        entry: &RegistryEntry,
    ) -> Result<(), String> {
        if execution.request_id != grant.request.body.id {
            return Err("request ID mismatch".to_string());
        }
        if execution.proof.body.actor != self.identity.principal_id {
            return Err("proof actor mismatch".to_string());
        }
        if execution.proof.body.operation != format!("{}::{}", entry.operation, entry.version) {
            return Err("proof operation mismatch".to_string());
        }
        if execution.proof.body.input_digest != grant.request.body.input_digest {
            return Err("proof input digest mismatch".to_string());
        }
        if execution.executed_at != execution.proof.body.timestamp {
            return Err("execution timestamp does not match proof timestamp".to_string());
        }
        if execution.proof.body.timestamp < grant.request.body.requested_at
            || execution.proof.body.timestamp > grant.request.body.expires_at
        {
            return Err("proof timestamp falls outside the approval window".to_string());
        }
        execution
            .proof
            .verify(&self.identity.signing_key.verifying_key())
            .map_err(|error| error.to_string())?;
        let output = canonicalize(&execution.output).map_err(|error| error.to_string())?;
        if execution.proof.body.output_digest != digest(ArtifactKind::OperationOutput, &output) {
            return Err("proof output digest mismatch".to_string());
        }
        Ok(())
    }

    fn approval_required_response(
        &self,
        id: Value,
        era: ProtocolEra,
        request: &SignedApprovalRequest,
        run: Option<Value>,
    ) -> Value {
        let request_id = request.body.id.to_string();
        let message = format!(
            "Human approval required for {} {}. Run `proof approval approve {} --approver <ID>` or `proof approval deny {} --approver <ID>`, then retry with requestState `{}`.",
            request.body.operation,
            request.body.version,
            request_id,
            request_id,
            request_id,
        );
        let approval = json!({"status": "pending", "request": request});
        if era == ProtocolEra::Modern {
            return success_response(
                id,
                json!({
                    "resultType": "input_required",
                    "inputRequests": {
                        "human_approval": {
                            "method": "elicitation/create",
                            "params": {
                                "mode": "form",
                                "message": message,
                                "requestedSchema": {
                                    "type": "object",
                                    "properties": {
                                        "approvalRequestId": {
                                            "type": "string",
                                            "const": request_id,
                                        }
                                    },
                                    "required": ["approvalRequestId"],
                                    "additionalProperties": false,
                                }
                            }
                        }
                    },
                    "requestState": request.body.id.to_string(),
                    "content": [{"type": "text", "text": message}],
                    "isError": false,
                    "_meta": self.result_meta(None, Some(approval), run),
                }),
            );
        }
        success_response(
            id,
            json!({
                "content": [{"type": "text", "text": message}],
                "isError": true,
                "structuredContent": {
                    "approvalRequestId": request.body.id,
                    "expiresAt": request.body.expires_at,
                },
                "_meta": self.result_meta(None, Some(approval), run),
            }),
        )
    }

    fn fail_run_tool_response(
        &self,
        id: Value,
        era: ProtocolEra,
        message: String,
        approval: Option<Value>,
        invocation: &mut RunInvocation,
    ) -> Value {
        if let Err(error) = self.fail_run_invocation(invocation, &message) {
            return error_response(
                id,
                -32603,
                "Agent run persistence failed while recording an error",
                Some(json!({
                    "detail": error,
                    "toolError": message,
                    "run": run_metadata(invocation),
                })),
            );
        }
        self.tool_error_response(id, era, message, approval, Some(run_metadata(invocation)))
    }

    fn tool_error_response(
        &self,
        id: Value,
        era: ProtocolEra,
        message: String,
        approval: Option<Value>,
        run: Option<Value>,
    ) -> Value {
        self.tool_result_response(id, era, McpToolResult::error(message), approval, run)
    }

    fn tool_result_response(
        &self,
        id: Value,
        era: ProtocolEra,
        result: McpToolResult,
        approval: Option<Value>,
        run: Option<Value>,
    ) -> Value {
        let mut response = Map::new();
        if era == ProtocolEra::Modern {
            response.insert("resultType".to_string(), json!("complete"));
        }
        response.insert(
            "content".to_string(),
            serde_json::to_value(&result.content).expect("MCP content is serializable"),
        );
        response.insert("isError".to_string(), json!(result.is_error));

        let mut proof = None;
        if let Some(structured) = result.structured_content {
            let output = structured.get("result").cloned().unwrap_or(Value::Null);
            proof = structured.get("proof").cloned();
            response.insert("structuredContent".to_string(), output);
        }
        if let Some(proof_value) = proof.as_ref() {
            if let Err(error) = self.persist_proof(proof_value) {
                response.insert("isError".to_string(), json!(true));
                response.insert(
                    "content".to_string(),
                    json!([{
                        "type": "text",
                        "text": format!("Operation executed, but proof persistence failed: {error}"),
                    }]),
                );
            }
        }
        response.insert("_meta".to_string(), self.result_meta(proof, approval, run));
        success_response(id, Value::Object(response))
    }

    fn ping(&self, id: Value, params: &Value) -> Value {
        match self.protocol_era(id.clone(), params) {
            Ok(ProtocolEra::Legacy) => success_response(id, json!({})),
            Ok(ProtocolEra::Modern) => error_response(id, -32601, "Method not found", None),
            Err(response) => response,
        }
    }

    fn protocol_era(&self, id: Value, params: &Value) -> Result<ProtocolEra, Value> {
        if params
            .get("_meta")
            .and_then(|meta| meta.get("io.modelcontextprotocol/protocolVersion"))
            .is_some()
        {
            validate_modern_request(id, params)?;
            return Ok(ProtocolEra::Modern);
        }
        if self.legacy_protocol_version.is_some() {
            return Ok(ProtocolEra::Legacy);
        }
        Err(error_response(
            id,
            -32600,
            "Call `server/discover` with modern request metadata or initialize a legacy session first",
            None,
        ))
    }

    fn result_meta(
        &self,
        proof: Option<Value>,
        approval: Option<Value>,
        run: Option<Value>,
    ) -> Value {
        let mut meta = Map::new();
        meta.insert(
            "io.modelcontextprotocol/serverInfo".to_string(),
            server_info(),
        );
        meta.insert(IDENTITY_META_KEY.to_string(), self.identity_metadata());
        if let Some(proof) = proof {
            meta.insert(
                EVIDENCE_META_KEY.to_string(),
                json!({
                    "proof": proof,
                    "algorithm": "Ed25519",
                    "verificationKey": base64::engine::general_purpose::STANDARD
                        .encode(self.identity.signing_key.verifying_key().to_bytes()),
                }),
            );
        }
        if let Some(approval) = approval {
            meta.insert(APPROVAL_META_KEY.to_string(), approval);
        }
        if let Some(run) = run {
            meta.insert(RUN_META_KEY.to_string(), run);
        }
        Value::Object(meta)
    }

    fn identity_metadata(&self) -> Value {
        json!({
            "principalId": self.identity.principal_id,
            "kind": self.identity.kind,
            "publicKey": base64::engine::general_purpose::STANDARD
                .encode(self.identity.signing_key.verifying_key().to_bytes()),
        })
    }

    fn persist_proof(&self, proof: &Value) -> Result<(), McpServerError> {
        let proof_id = proof
            .pointer("/body/id")
            .and_then(Value::as_str)
            .unwrap_or("unknown-proof");
        let directory = self.workspace_path.join(".proof/data/proofs");
        fs::create_dir_all(&directory).map_err(|source| McpServerError::Io {
            path: directory.clone(),
            source,
        })?;
        let destination = directory.join(format!("{proof_id}.json"));
        let temporary = directory.join(format!(".{proof_id}.json.tmp"));
        let encoded = serde_json::to_vec_pretty(proof).map_err(|source| McpServerError::Json {
            path: temporary.clone(),
            source,
        })?;
        fs::write(&temporary, encoded).map_err(|source| McpServerError::Io {
            path: temporary.clone(),
            source,
        })?;
        fs::rename(&temporary, &destination).map_err(|source| McpServerError::Io {
            path: destination,
            source,
        })
    }

    fn engine_registry(&self) -> &Registry {
        &self.registry
    }
}

fn run_metadata(invocation: &RunInvocation) -> Value {
    json!({
        "runId": invocation.run.id,
        "stepId": invocation.step.id,
        "run": invocation.run,
        "step": invocation.step,
        "replay": invocation.replay,
    })
}

fn validate_step_call(
    step: &AgentRunStep,
    entry: &RegistryEntry,
    call: &McpToolCall,
) -> Result<(), String> {
    if step.operation != entry.operation || step.version != entry.version {
        return Err(format!(
            "agent run step {} targets {} {}, not {} {}",
            step.id, step.operation, step.version, entry.operation, entry.version
        ));
    }
    let input = canonicalize(&call.arguments).map_err(|error| error.to_string())?;
    if step.input_digest != digest(ArtifactKind::OperationInput, &input) {
        return Err(format!(
            "agent run step {} input does not match the recorded attempt",
            step.id
        ));
    }
    Ok(())
}

fn approval_metadata(status: &str, grant: &ApprovalGrant) -> Value {
    json!({
        "status": status,
        "request": grant.request,
        "decision": grant.decision,
        "approver": grant.approver,
    })
}

pub fn load_workspace_keypair(workspace_path: &Path) -> Result<Keypair, McpServerError> {
    let path = workspace_path.join(".proof/keypair.json");
    if !path.exists() {
        return Err(McpServerError::WorkspaceKeypairMissing(path));
    }
    let raw = fs::read_to_string(&path).map_err(|source| McpServerError::Io {
        path: path.clone(),
        source,
    })?;
    let stored: StoredKeypair =
        serde_json::from_str(&raw).map_err(|source| McpServerError::Json {
            path: path.clone(),
            source,
        })?;
    let signing_key = base64::engine::general_purpose::STANDARD.decode(stored.signing_key)?;
    let signing_key: [u8; 32] = signing_key
        .try_into()
        .map_err(|_| McpServerError::SigningKeyLength)?;
    let signing_key = ed25519_dalek::SigningKey::from_bytes(&signing_key);
    if signing_key.verifying_key().to_bytes() != stored.public_key {
        return Err(McpServerError::PublicKeyMismatch);
    }
    Ok(Keypair {
        principal_id: PrincipalId::new(stored.principal_id),
        kind: stored.kind,
        created_at: stored.created_at,
        signing_key,
    })
}

pub fn load_workspace_registry(workspace_path: &Path) -> Result<Registry, McpServerError> {
    let registry_path = workspace_path.join(".proof/registry");
    if !registry_path.exists() {
        return Err(McpServerError::RegistryMissing(registry_path));
    }
    let mut entries = Vec::new();
    collect_registry_entries(&registry_path, &registry_path, &mut entries)?;
    Registry::new(entries).map_err(McpServerError::from)
}

fn collect_registry_entries(
    registry_root: &Path,
    directory: &Path,
    entries: &mut Vec<RegistryEntry>,
) -> Result<(), McpServerError> {
    let items = fs::read_dir(directory).map_err(|source| McpServerError::Io {
        path: directory.to_path_buf(),
        source,
    })?;
    for item in items {
        let item = item.map_err(|source| McpServerError::Io {
            path: directory.to_path_buf(),
            source,
        })?;
        let path = item.path();
        if path.is_dir() {
            collect_registry_entries(registry_root, &path, entries)?;
            continue;
        }
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if path.extension().and_then(|extension| extension.to_str()) != Some("json")
            || file_name.ends_with(".input.json")
            || file_name.ends_with(".output.json")
        {
            continue;
        }
        let raw = fs::read_to_string(&path).map_err(|source| McpServerError::Io {
            path: path.clone(),
            source,
        })?;
        let mut entry: RegistryEntry =
            serde_json::from_str(&raw).map_err(|source| McpServerError::Json {
                path: path.clone(),
                source,
            })?;
        entry.input_schema = inline_schema(registry_root, &entry.input_schema)?;
        entry.output_schema = inline_schema(registry_root, &entry.output_schema)?;
        entries.push(entry);
    }
    Ok(())
}

fn inline_schema(registry_root: &Path, schema: &str) -> Result<String, McpServerError> {
    if serde_json::from_str::<Value>(schema).is_ok() {
        return Ok(schema.to_string());
    }
    let path = registry_root.join(schema);
    let raw = fs::read_to_string(&path).map_err(|source| McpServerError::Io {
        path: path.clone(),
        source,
    })?;
    let schema_value: Value =
        serde_json::from_str(&raw).map_err(|source| McpServerError::Json {
            path: path.clone(),
            source,
        })?;
    serde_json::to_string(&schema_value).map_err(|source| McpServerError::Json { path, source })
}

fn validate_modern_request(id: Value, params: &Value) -> Result<(), Value> {
    let Some(meta) = params.get("_meta").and_then(Value::as_object) else {
        return Err(error_response(
            id,
            -32602,
            "Modern MCP requests require `_meta`",
            None,
        ));
    };
    let Some(version) = meta
        .get("io.modelcontextprotocol/protocolVersion")
        .and_then(Value::as_str)
    else {
        return Err(error_response(
            id,
            -32602,
            "Modern MCP requests require a protocol version",
            None,
        ));
    };
    if version != CURRENT_PROTOCOL_VERSION {
        return Err(error_response(
            id,
            -32022,
            "Unsupported protocol version",
            Some(json!({
                "supported": [CURRENT_PROTOCOL_VERSION, LEGACY_PROTOCOL_VERSION],
                "requested": version,
            })),
        ));
    }
    if !meta
        .get("io.modelcontextprotocol/clientCapabilities")
        .is_some_and(Value::is_object)
    {
        return Err(error_response(
            id,
            -32602,
            "Modern MCP requests require client capabilities",
            None,
        ));
    }
    Ok(())
}

fn server_info() -> Value {
    json!({
        "name": SERVER_NAME,
        "version": env!("CARGO_PKG_VERSION"),
    })
}

fn success_response(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error_response(id: Value, code: i64, message: &str, data: Option<Value>) -> Value {
    let mut error = Map::new();
    error.insert("code".to_string(), json!(code));
    error.insert("message".to_string(), json!(message));
    if let Some(data) = data {
        error.insert("data".to_string(), data);
    }
    json!({ "jsonrpc": "2.0", "id": id, "error": error })
}
