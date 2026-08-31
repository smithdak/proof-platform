use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::extract::{Path as AxumPath, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE, HOST, ORIGIN};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use proof_agent_runtime::{runtime_approval_context, RuntimeApprovalContext};
use proof_kernel::{
    canonicalize, digest, AgentRunStatus, AgentRunStep, AgentRunStepStatus, ApprovalOutcome,
    ArtifactKind, Governance, Registry, SignedApprovalRequest,
};
use proof_storage::SqliteStore;
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use super::approval::{sign_approval_decision, trusted_approver_ids};
use crate::{load_registry, open_store, Cli, Workspace};

const SESSION_HEADER: &str = "x-proof-session";
const APPROVAL_UI_HTML: &str = include_str!("approval_ui.html");

#[derive(Clone)]
pub(crate) struct ApprovalUiState {
    root: Arc<PathBuf>,
    store: Arc<SqliteStore>,
    registry: Arc<Registry>,
    session_token: Arc<str>,
    expected_host: Arc<str>,
    expected_origin: Arc<str>,
}

impl ApprovalUiState {
    fn open(root: PathBuf, session_token: String, port: u16) -> Result<Self> {
        let registry = load_registry(&root)?;
        let store = open_store(&root)?;
        let expected_host = format!("127.0.0.1:{port}");
        let expected_origin = format!("http://{expected_host}");
        Ok(Self {
            root: Arc::new(root),
            store: Arc::new(store),
            registry: Arc::new(registry),
            session_token: Arc::from(session_token),
            expected_host: Arc::from(expected_host),
            expected_origin: Arc::from(expected_origin),
        })
    }

    #[cfg(test)]
    fn from_parts(
        root: PathBuf,
        store: SqliteStore,
        registry: Registry,
        session_token: impl Into<String>,
        port: u16,
    ) -> Self {
        let expected_host = format!("127.0.0.1:{port}");
        let expected_origin = format!("http://{expected_host}");
        Self {
            root: Arc::new(root),
            store: Arc::new(store),
            registry: Arc::new(registry),
            session_token: Arc::from(session_token.into()),
            expected_host: Arc::from(expected_host),
            expected_origin: Arc::from(expected_origin),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum ApprovalReviewStatus {
    Pending,
    Expired,
    Approved,
    Denied,
    Executed,
}

#[derive(Debug, Clone, Serialize)]
struct OperationReview {
    name: String,
    version: String,
    description: Option<String>,
    consequence: Option<String>,
    governance: Option<Governance>,
}

#[derive(Debug, Clone, Serialize)]
struct RunReview {
    id: Uuid,
    goal: String,
    status: AgentRunStatus,
}

#[derive(Debug, Clone, Serialize)]
struct StepReview {
    id: Uuid,
    ordinal: u32,
    attempt: u32,
    status: AgentRunStepStatus,
}

#[derive(Debug, Clone, Serialize)]
struct AgentReview {
    id: Uuid,
    name: String,
    provider: String,
    model: String,
}

#[derive(Debug, Clone, Serialize)]
struct DecisionReview {
    outcome: ApprovalOutcome,
    decided_by: String,
    decided_at: DateTime<Utc>,
    reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct ApprovalReview {
    request_id: Uuid,
    status: ApprovalReviewStatus,
    actionable: bool,
    blocked_reasons: Vec<String>,
    operation: OperationReview,
    arguments: Option<Value>,
    input_digest: String,
    requested_by: String,
    requested_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    run: Option<RunReview>,
    step: Option<StepReview>,
    agent: Option<AgentReview>,
    #[serde(skip_serializing_if = "Option::is_none")]
    runtime_checkpoint_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    required_approver_id: Option<Uuid>,
    decision: Option<DecisionReview>,
    execution_proof_id: Option<Uuid>,
}

#[derive(Debug, Serialize)]
struct ApprovalInboxItem {
    request_id: Uuid,
    status: ApprovalReviewStatus,
    actionable: bool,
    operation: String,
    version: String,
    goal: Option<String>,
    agent_name: Option<String>,
    requested_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

impl From<&ApprovalReview> for ApprovalInboxItem {
    fn from(review: &ApprovalReview) -> Self {
        Self {
            request_id: review.request_id,
            status: review.status,
            actionable: review.actionable,
            operation: review.operation.name.clone(),
            version: review.operation.version.clone(),
            goal: review.run.as_ref().map(|run| run.goal.clone()),
            agent_name: review.agent.as_ref().map(|agent| agent.name.clone()),
            requested_at: review.requested_at,
            expires_at: review.expires_at,
        }
    }
}

#[derive(Debug, Serialize)]
struct InboxResponse {
    approvals: Vec<ApprovalInboxItem>,
    approvers: Vec<Uuid>,
}

#[derive(Debug, Serialize)]
struct DetailResponse {
    approval: ApprovalReview,
    approvers: Vec<Uuid>,
}

#[derive(Debug, Deserialize)]
struct DecisionRequest {
    approver_id: Uuid,
    outcome: ApprovalOutcome,
    #[serde(default)]
    reason: Option<String>,
}

pub(crate) fn cmd_approval_ui(cli: &Cli, port: u16) -> Result<()> {
    let workspace = Workspace::open(&cli.workspace)?;
    let session_token = new_session_token();
    let runtime = tokio::runtime::Runtime::new().context("could not start approval UI runtime")?;
    runtime.block_on(async move {
        let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let listener = tokio::net::TcpListener::bind(address)
            .await
            .context("could not bind local approval UI")?;
        let local = listener.local_addr()?;
        let state = ApprovalUiState::open(workspace.root, session_token.clone(), local.port())?;
        println!(
            "Proof approval UI: http://127.0.0.1:{}/#{}",
            local.port(),
            session_token
        );
        axum::serve(listener, approval_ui_router(state))
            .await
            .context("approval UI server failed")
    })
}

fn new_session_token() -> String {
    let mut bytes = [0_u8; 32];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

fn approval_ui_router(state: ApprovalUiState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/api/approvals", get(list_approvals))
        .route("/api/approvals/:request_id", get(get_approval))
        .route("/api/approvals/:request_id/decision", post(post_decision))
        .with_state(state)
}

async fn index() -> Response {
    let mut response = Html(APPROVAL_UI_HTML).into_response();
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; connect-src 'self'; img-src 'none'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'",
        ),
    );
    response
        .headers_mut()
        .insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    response
        .headers_mut()
        .insert("x-frame-options", HeaderValue::from_static("DENY"));
    response
}

async fn list_approvals(State(state): State<ApprovalUiState>, headers: HeaderMap) -> Response {
    if let Err(response) = require_session(&state, &headers) {
        return response;
    }
    let approvers = match trusted_approver_ids(&state.root, &state.store) {
        Ok(approvers) => approvers,
        Err(error) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    };
    let reviews = match build_reviews(&state, &approvers, Utc::now()) {
        Ok(reviews) => reviews,
        Err(error) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    };
    let approvals = reviews.iter().map(ApprovalInboxItem::from).collect();
    api_json(
        StatusCode::OK,
        &InboxResponse {
            approvals,
            approvers,
        },
    )
}

async fn get_approval(
    State(state): State<ApprovalUiState>,
    AxumPath(request_id): AxumPath<Uuid>,
    headers: HeaderMap,
) -> Response {
    if let Err(response) = require_session(&state, &headers) {
        return response;
    }
    let approvers = match trusted_approver_ids(&state.root, &state.store) {
        Ok(approvers) => approvers,
        Err(error) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    };
    let Some(request) = (match state.store.load_approval_request(&request_id) {
        Ok(request) => request,
        Err(error) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    }) else {
        return api_error(StatusCode::NOT_FOUND, "approval request not found");
    };
    let review = match build_approval_review(
        &state.store,
        &state.registry,
        &request,
        &approvers,
        Utc::now(),
    ) {
        Ok(review) => review,
        Err(error) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    };
    let approvers = match review.required_approver_id {
        Some(required) => approvers
            .into_iter()
            .filter(|approver| *approver == required)
            .collect(),
        None => approvers,
    };
    api_json(
        StatusCode::OK,
        &DetailResponse {
            approval: review,
            approvers,
        },
    )
}

async fn post_decision(
    State(state): State<ApprovalUiState>,
    AxumPath(request_id): AxumPath<Uuid>,
    headers: HeaderMap,
    Json(input): Json<DecisionRequest>,
) -> Response {
    if let Err(response) = require_mutation_headers(&state, &headers) {
        return response;
    }
    if !is_json_content_type(&headers) {
        return api_error(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Content-Type must be application/json",
        );
    }
    let approvers = match trusted_approver_ids(&state.root, &state.store) {
        Ok(approvers) => approvers,
        Err(error) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    };
    if !approvers.contains(&input.approver_id) {
        return api_error(
            StatusCode::BAD_REQUEST,
            "approver is not an enrolled local human",
        );
    }
    let Some(request) = (match state.store.load_approval_request(&request_id) {
        Ok(request) => request,
        Err(error) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    }) else {
        return api_error(StatusCode::NOT_FOUND, "approval request not found");
    };
    let review_time = Utc::now();
    let review = match build_approval_review(
        &state.store,
        &state.registry,
        &request,
        &approvers,
        review_time,
    ) {
        Ok(review) => review,
        Err(error) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    };
    if !review.actionable {
        return api_json(
            StatusCode::CONFLICT,
            &json!({
                "error": "approval request is not actionable",
                "approval": review,
            }),
        );
    }
    if !decision_approver_matches(review.required_approver_id, input.approver_id) {
        return api_error(
            StatusCode::BAD_REQUEST,
            "approver does not match the sealed live approver",
        );
    }
    let resume_command = resume_command_for_review(
        review.runtime_checkpoint_kind.as_deref(),
        review.run.as_ref().map(|run| run.id),
    );
    let reason = input
        .reason
        .map(|reason| reason.trim().to_string())
        .filter(|reason| !reason.is_empty());
    let decision = match sign_approval_decision(
        &state.root,
        &state.store,
        request_id,
        input.approver_id,
        input.outcome,
        reason,
        Utc::now(),
    ) {
        Ok(decision) => decision,
        Err(error) => {
            let message = error.to_string();
            let status = if message.contains("already decided")
                || message.contains("already has a different decision")
                || message.contains("validity window")
            {
                StatusCode::CONFLICT
            } else {
                StatusCode::BAD_REQUEST
            };
            return api_error(status, message);
        }
    };
    let status = match input.outcome {
        ApprovalOutcome::Approved => "approved",
        ApprovalOutcome::Denied => "denied",
    };
    api_json(
        StatusCode::OK,
        &json!({
            "status": status,
            "decision": decision,
            "resume_command": resume_command,
        }),
    )
}

fn build_reviews(
    state: &ApprovalUiState,
    approvers: &[Uuid],
    now: DateTime<Utc>,
) -> Result<Vec<ApprovalReview>> {
    let mut reviews = state
        .store
        .list_approval_requests()?
        .iter()
        .map(|request| {
            build_approval_review(&state.store, &state.registry, request, approvers, now)
        })
        .collect::<Result<Vec<_>>>()?;
    reviews.sort_by(|left, right| {
        right
            .actionable
            .cmp(&left.actionable)
            .then_with(|| left.expires_at.cmp(&right.expires_at))
            .then_with(|| left.requested_at.cmp(&right.requested_at))
    });
    Ok(reviews)
}

fn step_approval_link_mismatch(step_request_id: Option<Uuid>, request_id: Uuid) -> bool {
    step_request_id != Some(request_id)
}

fn decision_approver_matches(required: Option<Uuid>, candidate: Uuid) -> bool {
    required.is_none_or(|required| required == candidate)
}

fn resume_command_for_review(
    runtime_checkpoint_kind: Option<&str>,
    run_id: Option<Uuid>,
) -> Option<String> {
    match (runtime_checkpoint_kind, run_id) {
        (Some("agent_runtime_v1"), Some(run_id)) => Some(format!("proof agent resume {run_id}")),
        _ => None,
    }
}

fn validated_runtime_approval_context(
    store: &SqliteStore,
    run_id: Uuid,
) -> std::result::Result<RuntimeApprovalContext, String> {
    let checkpoints = store
        .list_agent_checkpoints(&run_id)
        .map_err(|error| error.to_string())?;
    let events = store
        .list_agent_run_events(&run_id)
        .map_err(|error| error.to_string())?;
    runtime_approval_context(run_id, &checkpoints, &events).map_err(|_| {
        "native runtime checkpoint history is missing, mixed, unsupported, or invalid".to_string()
    })
}

fn bind_runtime_approval_context(
    runtime: RuntimeApprovalContext,
    run_agent_id: Option<Uuid>,
    step: &AgentRunStep,
    request: &SignedApprovalRequest,
    arguments: &mut Option<Value>,
    runtime_checkpoint_kind: &mut Option<String>,
    required_approver_id: &mut Option<Uuid>,
    blocked_reasons: &mut Vec<String>,
) {
    *runtime_checkpoint_kind = Some(runtime.checkpoint_kind.clone());
    *required_approver_id = runtime.required_approver_id;
    let is_live_v2 = runtime.checkpoint_kind == "agent_runtime_v2";
    if runtime.run_id != step.run_id {
        blocked_reasons.push("runtime checkpoint references a different run".to_string());
    }
    if Some(runtime.agent_id) != run_agent_id {
        blocked_reasons.push("runtime checkpoint references a different agent".to_string());
    }
    if is_live_v2 && runtime.sealed_approval_request.as_ref() != Some(request) {
        blocked_reasons.push(
            "runtime checkpoint does not match the exact signed approval request".to_string(),
        );
    }
    if is_live_v2 && runtime.sealed_step.as_ref() != Some(step) {
        blocked_reasons
            .push("runtime checkpoint does not match the exact durable waiting step".to_string());
    }
    match runtime.pending_tool {
        Some(pending) => {
            *arguments = Some(pending.arguments.clone());
            if pending.approval_request_id != Some(request.body.id)
                || pending.step_id != step.id
                || pending.operation != request.body.operation
                || pending.version != request.body.version
            {
                blocked_reasons
                    .push("runtime checkpoint does not match the pending approval".to_string());
            }
            match canonicalize(&pending.arguments) {
                Ok(canonical) => {
                    let actual = digest(ArtifactKind::OperationInput, &canonical);
                    if actual != request.body.input_digest || actual != step.input_digest {
                        blocked_reasons.push(
                            "displayed arguments do not match the signed input digest".to_string(),
                        );
                    }
                }
                Err(_) => {
                    blocked_reasons.push("pending arguments cannot be canonicalized".to_string())
                }
            }
        }
        None => blocked_reasons.push("runtime checkpoint has no pending tool call".to_string()),
    }
}

fn build_approval_review(
    store: &SqliteStore,
    registry: &Registry,
    request: &SignedApprovalRequest,
    available_approvers: &[Uuid],
    now: DateTime<Utc>,
) -> Result<ApprovalReview> {
    let decision = store.load_approval_decision(&request.body.id)?;
    let execution = store.load_approval_execution(&request.body.id)?;
    let status = match (&decision, &execution) {
        (_, Some(_)) => ApprovalReviewStatus::Executed,
        (Some(decision), None) if decision.body.outcome == ApprovalOutcome::Approved => {
            ApprovalReviewStatus::Approved
        }
        (Some(_), None) => ApprovalReviewStatus::Denied,
        (None, None) if now > request.body.expires_at => ApprovalReviewStatus::Expired,
        (None, None) => ApprovalReviewStatus::Pending,
    };
    let mut blocked_reasons = Vec::new();
    match status {
        ApprovalReviewStatus::Pending => {}
        ApprovalReviewStatus::Expired => {
            blocked_reasons.push("approval request has expired".to_string())
        }
        ApprovalReviewStatus::Approved => {
            blocked_reasons.push("approval request was already approved".to_string())
        }
        ApprovalReviewStatus::Denied => {
            blocked_reasons.push("approval request was already denied".to_string())
        }
        ApprovalReviewStatus::Executed => {
            blocked_reasons.push("approved operation was already executed".to_string())
        }
    }
    if status == ApprovalReviewStatus::Pending && available_approvers.is_empty() {
        blocked_reasons.push("no enrolled local human approver is available".to_string());
    }

    let registry_entry = registry.find(&request.body.operation, &request.body.version);
    if registry_entry.is_none() {
        blocked_reasons.push("operation is missing from the workspace registry".to_string());
    } else if registry_entry.is_some_and(|entry| entry.governance != Governance::HumanOnly) {
        blocked_reasons.push("operation is not governed as human-only".to_string());
    }
    let operation = OperationReview {
        name: request.body.operation.clone(),
        version: request.body.version.clone(),
        description: registry_entry.map(|entry| entry.description.clone()),
        consequence: registry_entry.map(|entry| entry.consequence.clone()),
        governance: registry_entry.map(|entry| entry.governance),
    };

    match store.load_principal(&request.body.requested_by) {
        Ok(requester) => {
            if let Err(error) = request.verify(&requester) {
                blocked_reasons.push(format!("approval request signature is invalid: {error}"));
            }
        }
        Err(_) => blocked_reasons.push("requesting agent is not enrolled".to_string()),
    }

    let mut run_review = None;
    let mut step_review = None;
    let mut agent_review = None;
    let mut arguments = None;
    let mut runtime_checkpoint_kind = None;
    let mut required_approver_id = None;
    match store.find_agent_run_step_by_approval(&request.body.id)? {
        Some(step) => {
            step_review = Some(StepReview {
                id: step.id,
                ordinal: step.ordinal,
                attempt: step.attempt,
                status: step.status,
            });
            if step_approval_link_mismatch(step.approval_request_id, request.body.id) {
                blocked_reasons
                    .push("native run step references a different approval request".to_string());
            }
            if step.status != AgentRunStepStatus::WaitingForApproval {
                blocked_reasons.push("native run step is not waiting for approval".to_string());
            }
            if step.operation != request.body.operation || step.version != request.body.version {
                blocked_reasons.push("run step does not match the signed operation".to_string());
            }
            if step.input_digest != request.body.input_digest {
                blocked_reasons
                    .push("run step input digest does not match the request".to_string());
            }
            match store.load_agent_run(&step.run_id)? {
                Some(run) => {
                    run_review = Some(RunReview {
                        id: run.id,
                        goal: run.goal.clone(),
                        status: run.status,
                    });
                    if run.status != AgentRunStatus::WaitingForInput {
                        blocked_reasons.push("native run is not waiting for input".to_string());
                    }
                    if run.actor != request.body.requested_by {
                        blocked_reasons
                            .push("run actor does not match the requesting agent".to_string());
                    }
                    match run.agent_id {
                        Some(agent_id) => match store.load_agent_definition(&agent_id)? {
                            Some(agent) => {
                                if !agent.tools.iter().any(|tool| {
                                    tool.operation == request.body.operation
                                        && tool.version == request.body.version
                                }) {
                                    blocked_reasons.push(
                                        "operation is not in the immutable agent allowlist"
                                            .to_string(),
                                    );
                                }
                                agent_review = Some(AgentReview {
                                    id: agent.id,
                                    name: agent.name,
                                    provider: agent.provider,
                                    model: agent.model,
                                });
                            }
                            None => blocked_reasons
                                .push("immutable agent definition is missing".to_string()),
                        },
                        None => blocked_reasons
                            .push("approval is not attached to a native agent run".to_string()),
                    }

                    match validated_runtime_approval_context(store, run.id) {
                        Ok(runtime) => bind_runtime_approval_context(
                            runtime,
                            run.agent_id,
                            &step,
                            request,
                            &mut arguments,
                            &mut runtime_checkpoint_kind,
                            &mut required_approver_id,
                            &mut blocked_reasons,
                        ),
                        Err(reason) => blocked_reasons.push(reason),
                    }
                }
                None => blocked_reasons.push("agent run is missing".to_string()),
            }
        }
        None => blocked_reasons.push("native approval context is missing".to_string()),
    }
    if status == ApprovalReviewStatus::Pending
        && required_approver_id.is_some_and(|required| !available_approvers.contains(&required))
    {
        blocked_reasons
            .push("sealed live approver is not an enrolled local human approver".to_string());
    }

    Ok(ApprovalReview {
        request_id: request.body.id,
        status,
        actionable: status == ApprovalReviewStatus::Pending && blocked_reasons.is_empty(),
        blocked_reasons,
        operation,
        arguments,
        input_digest: request.body.input_digest.to_string(),
        requested_by: request.body.requested_by.to_string(),
        requested_at: request.body.requested_at,
        expires_at: request.body.expires_at,
        run: run_review,
        step: step_review,
        agent: agent_review,
        runtime_checkpoint_kind,
        required_approver_id,
        decision: decision.map(|decision| DecisionReview {
            outcome: decision.body.outcome,
            decided_by: decision.body.decided_by.to_string(),
            decided_at: decision.body.decided_at,
            reason: decision.body.reason,
        }),
        execution_proof_id: execution.map(|execution| execution.proof.body.id),
    })
}

fn require_session(state: &ApprovalUiState, headers: &HeaderMap) -> Result<(), Response> {
    let provided = headers
        .get(SESSION_HEADER)
        .and_then(|value| value.to_str().ok());
    if provided == Some(state.session_token.as_ref()) {
        return Ok(());
    }
    Err(api_error(
        StatusCode::UNAUTHORIZED,
        "missing or invalid approval UI session token",
    ))
}

fn require_mutation_headers(state: &ApprovalUiState, headers: &HeaderMap) -> Result<(), Response> {
    require_session(state, headers)?;
    let host = headers.get(HOST).and_then(|value| value.to_str().ok());
    if host != Some(state.expected_host.as_ref()) {
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "mutating requests must target the bound loopback host",
        ));
    }
    let origin = headers.get(ORIGIN).and_then(|value| value.to_str().ok());
    if origin != Some(state.expected_origin.as_ref()) {
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "mutating requests must have the same origin",
        ));
    }
    Ok(())
}

fn is_json_content_type(headers: &HeaderMap) -> bool {
    headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"))
}

fn api_error(status: StatusCode, message: impl Into<String>) -> Response {
    api_json(status, &json!({"error": message.into()}))
}

fn api_json<T: Serialize>(status: StatusCode, value: &T) -> Response {
    let mut response = (status, Json(value)).into_response();
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

#[cfg(test)]
mod tests {
    use axum::body::{to_bytes, Body};
    use axum::http::{Method, Request};
    use clap::Parser;
    use proof_agent_runtime::{AgentRuntimeState, ModelInput, PendingToolCall};
    use proof_kernel::{
        AgentCheckpoint, AgentDefinition, AgentLimits, AgentRun, AgentRunMode, AgentRunStep,
        AgentTool, PrincipalKind, RegistryEntry, SignedApprovalRequest, VersionStatus,
    };
    use tower::ServiceExt;

    use super::*;
    use crate::commands::approval::{cmd_approver_init, load_approver_keypair};

    const TOKEN: &str = "test-session-token";
    const HOST_VALUE: &str = "127.0.0.1:4173";
    const ORIGIN_VALUE: &str = "http://127.0.0.1:4173";
    const V1_RUNTIME_CHECKPOINT_KIND: &str = "agent_runtime_v1";

    struct NativeFixture {
        request: SignedApprovalRequest,
        run_id: Uuid,
        arguments: Value,
    }

    fn initialized_workspace() -> (assert_fs::TempDir, Cli, Workspace, SqliteStore, Uuid) {
        let directory = assert_fs::TempDir::new().unwrap();
        let cli = Cli::parse_from(["proof", "-w", directory.path().to_str().unwrap(), "init"]);
        crate::commands::content::cmd_init(&cli).unwrap();
        cmd_approver_init(&cli).unwrap();
        let workspace = Workspace::open(&cli.workspace).unwrap();
        let store = open_store(&workspace.root).unwrap();
        let approver_id = trusted_approver_ids(&workspace.root, &store)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        (directory, cli, workspace, store, approver_id)
    }

    fn human_only_registry() -> Registry {
        Registry::new(vec![RegistryEntry {
            operation: "content.release".to_string(),
            domain: "content".to_string(),
            version: "v1".to_string(),
            action: "content:release".to_string(),
            description: "Release approved content for publication".to_string(),
            input_schema: "content/release.input.json".to_string(),
            output_schema: "content/release.output.json".to_string(),
            required_authority: "delegation-grant".to_string(),
            governance: Governance::HumanOnly,
            idempotency: "required-uuidv7".to_string(),
            consequence: "content-release".to_string(),
            evidence_contract: "operation-effect-v1".to_string(),
            benchmark: None,
            status: VersionStatus::Active,
            deprecated_since: None,
            replacement_operation: None,
        }])
        .unwrap()
    }

    fn save_native_approval(
        store: &SqliteStore,
        workspace: &Workspace,
        arguments: Value,
        checkpoint_arguments: Value,
        requested_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    ) -> NativeFixture {
        let suffix = Uuid::now_v7();
        let agent = AgentDefinition::new(
            format!("release-manager-{suffix}"),
            "Release only after an operator reviews the exact target.",
            "openai",
            "test-model",
            vec![AgentTool::new("content.release", "v1").unwrap()],
            AgentLimits::default(),
            requested_at,
        )
        .unwrap();
        store.save_agent_definition(&agent).unwrap();

        let mut run = AgentRun::new_for_agent(
            workspace.actor,
            agent.id,
            AgentRunMode::Session,
            "Publish the approved release to preview",
            requested_at,
        )
        .unwrap();
        store.save_agent_run(&run).unwrap();
        run.start(requested_at).unwrap();
        store.save_agent_run(&run).unwrap();

        let mut step =
            AgentRunStep::new(run.id, 0, "content.release", "v1", &arguments, requested_at)
                .unwrap();
        store.save_agent_run_step(&step).unwrap();
        step.start(requested_at).unwrap();
        store.save_agent_run_step(&step).unwrap();

        let request = SignedApprovalRequest::create(
            "content.release",
            "v1",
            &arguments,
            requested_at,
            expires_at,
            &workspace.keypair,
        )
        .unwrap();
        store.save_approval_request(&request).unwrap();
        step.wait_for_approval(request.body.id, requested_at)
            .unwrap();
        store.save_agent_run_step(&step).unwrap();
        run.wait_for_input(requested_at).unwrap();
        store.save_agent_run(&run).unwrap();

        let runtime = AgentRuntimeState {
            agent_id: agent.id,
            started_at: requested_at,
            previous_response_id: Some("response-1".to_string()),
            next_input: ModelInput::Goal {
                text: run.goal.clone(),
            },
            pending_tool: Some(PendingToolCall {
                call_id: "call-release".to_string(),
                tool_name: "proof_content_v1_content_release".to_string(),
                operation: "content.release".to_string(),
                version: "v1".to_string(),
                arguments: checkpoint_arguments,
                step_id: step.id,
                approval_request_id: Some(request.body.id),
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
        let checkpoint = AgentCheckpoint::create(
            run.id,
            0,
            json!({"kind": V1_RUNTIME_CHECKPOINT_KIND, "runtime": runtime}),
            requested_at,
        )
        .unwrap();
        store.save_agent_checkpoint(&checkpoint).unwrap();

        NativeFixture {
            request,
            run_id: run.id,
            arguments,
        }
    }

    fn save_contextless_request(
        store: &SqliteStore,
        workspace: &Workspace,
        requested_at: DateTime<Utc>,
    ) -> SignedApprovalRequest {
        let request = SignedApprovalRequest::create(
            "content.release",
            "v1",
            &json!({"release_id": Uuid::now_v7()}),
            requested_at,
            requested_at + chrono::Duration::minutes(15),
            &workspace.keypair,
        )
        .unwrap();
        store.save_approval_request(&request).unwrap();
        request
    }

    fn state(root: PathBuf, store: SqliteStore) -> ApprovalUiState {
        ApprovalUiState::from_parts(root, store, human_only_registry(), TOKEN, 4173)
    }

    fn request(
        method: Method,
        uri: &str,
        token: Option<&str>,
        origin: Option<&str>,
        body: Option<Value>,
    ) -> Request<Body> {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(HOST, HOST_VALUE);
        if let Some(token) = token {
            builder = builder.header(SESSION_HEADER, token);
        }
        if let Some(origin) = origin {
            builder = builder.header(ORIGIN, origin);
        }
        let body = match body {
            Some(body) => {
                builder = builder.header(CONTENT_TYPE, "application/json");
                Body::from(serde_json::to_vec(&body).unwrap())
            }
            None => Body::empty(),
        };
        builder.body(body).unwrap()
    }

    async fn json_body(response: Response) -> Value {
        let bytes = to_bytes(response.into_body(), 1_000_000).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    fn append_resealed_runtime_checkpoint(
        store: &SqliteStore,
        run_id: Uuid,
        mutate: impl FnOnce(&mut Value),
    ) {
        let checkpoints = store.list_agent_checkpoints(&run_id).unwrap();
        let latest = checkpoints.last().unwrap();
        let mut state = latest.state.clone();
        mutate(&mut state);
        let checkpoint =
            AgentCheckpoint::create(run_id, latest.sequence + 1, state, Utc::now()).unwrap();
        store.save_agent_checkpoint(&checkpoint).unwrap();
    }

    fn recompute_pending_decision_digest(state: &mut Value) {
        let pending = &state["runtime"]["pending_tool"];
        let decision = json!({
            "type": "tool_call",
            "call_id": pending["call_id"],
            "name": pending["tool_name"],
            "arguments": pending["arguments"],
        });
        state["runtime"]["attempts"][0]["response"]["decision_digest"] = json!(digest(
            ArtifactKind::Generic,
            &canonicalize(&decision).unwrap(),
        ));
    }

    #[test]
    fn embedded_ui_guards_detail_selection_and_confirmation_identity() {
        assert!(APPROVAL_UI_HTML.contains("const generation = ++selectionGeneration"));
        assert!(APPROVAL_UI_HTML
            .contains("generation !== selectionGeneration || selectedId !== requestId"));
        assert!(APPROVAL_UI_HTML.contains(
            "selectedId = requestId;\n        selectedApproval = null;\n        updateActions();"
        ));
        assert!(APPROVAL_UI_HTML.contains("approval.request_id !== expectedRequestId"));
        assert!(APPROVAL_UI_HTML.contains("Request: ${requestId}\\nGoal: ${goal}"));
    }

    #[tokio::test]
    async fn index_is_private_and_not_embeddable() {
        let (_directory, _cli, workspace, store, _approver_id) = initialized_workspace();
        let app = approval_ui_router(state(workspace.root.clone(), store));

        let response = app
            .oneshot(request(Method::GET, "/", None, None, None))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get(CACHE_CONTROL).unwrap(), "no-store");
        assert!(response
            .headers()
            .get(CONTENT_SECURITY_POLICY)
            .unwrap()
            .to_str()
            .unwrap()
            .contains("frame-ancestors 'none'"));
        assert_eq!(
            response.headers().get("referrer-policy").unwrap(),
            "no-referrer"
        );
        assert_eq!(response.headers().get("x-frame-options").unwrap(), "DENY");
    }

    #[test]
    fn view_model_joins_and_verifies_native_approval_context() {
        let (_directory, _cli, workspace, store, approver_id) = initialized_workspace();
        let now = Utc::now();
        let fixture = save_native_approval(
            &store,
            &workspace,
            json!({"release_id": "release-1", "environment": "preview"}),
            json!({"release_id": "release-1", "environment": "preview"}),
            now,
            now + chrono::Duration::minutes(15),
        );

        let review = build_approval_review(
            &store,
            &human_only_registry(),
            &fixture.request,
            &[approver_id],
            now,
        )
        .unwrap();

        assert_eq!(review.status, ApprovalReviewStatus::Pending);
        assert!(review.actionable, "{:?}", review.blocked_reasons);
        assert_eq!(review.arguments, Some(fixture.arguments));
        assert_eq!(review.run.as_ref().unwrap().id, fixture.run_id);
        assert_eq!(
            review.run.as_ref().unwrap().goal,
            "Publish the approved release to preview"
        );
        assert!(review
            .agent
            .as_ref()
            .unwrap()
            .name
            .starts_with("release-manager-"));
        assert_eq!(
            review.operation.description.as_deref(),
            Some("Release approved content for publication")
        );
        assert_eq!(
            review.operation.consequence.as_deref(),
            Some("content-release")
        );
        assert_eq!(
            review.input_digest,
            fixture.request.body.input_digest.to_string()
        );
    }

    #[test]
    fn view_binding_renders_a_validated_live_v2_projection() {
        let (_directory, _cli, workspace, store, approver_id) = initialized_workspace();
        let now = Utc::now();
        let fixture = save_native_approval(
            &store,
            &workspace,
            json!({
                "idempotency_key": Uuid::now_v7(),
                "edition_id": Uuid::now_v7(),
                "environment": "preview",
                "version_label": "2026.08.31-rc1",
                "manifest_digest": "sha256:fixture"
            }),
            json!({"unused": true}),
            now,
            now + chrono::Duration::minutes(15),
        );
        let step = store
            .find_agent_run_step_by_approval(&fixture.request.body.id)
            .unwrap()
            .unwrap();
        let run = store.load_agent_run(&fixture.run_id).unwrap().unwrap();
        let runtime = RuntimeApprovalContext {
            checkpoint_kind: "agent_runtime_v2".to_string(),
            run_id: fixture.run_id,
            agent_id: run.agent_id.unwrap(),
            required_approver_id: Some(approver_id),
            pending_tool: Some(PendingToolCall {
                call_id: "call-live-v2".to_string(),
                tool_name: "proof_content_v2_release_publish".to_string(),
                operation: fixture.request.body.operation.clone(),
                version: fixture.request.body.version.clone(),
                arguments: fixture.arguments.clone(),
                step_id: step.id,
                approval_request_id: Some(fixture.request.body.id),
            }),
            sealed_approval_request: Some(fixture.request.clone()),
            sealed_step: Some(step.clone()),
        };
        let mut arguments = None;
        let mut runtime_checkpoint_kind = None;
        let mut required_approver_id = None;
        let mut blocked_reasons = Vec::new();

        bind_runtime_approval_context(
            runtime,
            run.agent_id,
            &step,
            &fixture.request,
            &mut arguments,
            &mut runtime_checkpoint_kind,
            &mut required_approver_id,
            &mut blocked_reasons,
        );

        assert!(blocked_reasons.is_empty(), "{blocked_reasons:?}");
        assert_eq!(arguments, Some(fixture.arguments));
        assert_eq!(runtime_checkpoint_kind.as_deref(), Some("agent_runtime_v2"));
        assert_eq!(required_approver_id, Some(approver_id));
        assert!(decision_approver_matches(required_approver_id, approver_id));
        assert!(!decision_approver_matches(
            required_approver_id,
            Uuid::now_v7()
        ));
        assert_eq!(
            resume_command_for_review(runtime_checkpoint_kind.as_deref(), Some(fixture.run_id)),
            None
        );
    }

    #[test]
    fn approval_review_from_sqlite_live_v2_history_is_actionable_and_renders_exact_five_arguments()
    {
        let fixture = crate::commands::live::tests::approval_live_fixture();
        let registry = load_registry(&fixture.workspace.root).unwrap();
        let review = build_approval_review(
            &fixture.store,
            &registry,
            &fixture.request,
            &[fixture.approver_id],
            Utc::now(),
        )
        .unwrap();

        assert!(review.actionable, "{:?}", review.blocked_reasons);
        assert_eq!(review.run.as_ref().unwrap().id, fixture.run_id);
        assert_eq!(
            review.runtime_checkpoint_kind.as_deref(),
            Some("agent_runtime_v2")
        );
        assert_eq!(review.required_approver_id, Some(fixture.approver_id));
        assert_eq!(review.arguments, Some(fixture.arguments.clone()));
        assert_eq!(
            review
                .arguments
                .as_ref()
                .unwrap()
                .as_object()
                .unwrap()
                .len(),
            5
        );
        assert_eq!(
            review
                .arguments
                .as_ref()
                .unwrap()
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<std::collections::BTreeSet<_>>(),
            [
                "edition_id".to_string(),
                "environment".to_string(),
                "idempotency_key".to_string(),
                "manifest_digest".to_string(),
                "version_label".to_string(),
            ]
            .into_iter()
            .collect()
        );
    }

    #[test]
    fn approval_review_rejects_recomputed_pending_call_argument_request_and_step_substitutions() {
        for mutation in 0..4 {
            let fixture = crate::commands::live::tests::approval_live_fixture();
            append_resealed_runtime_checkpoint(&fixture.store, fixture.run_id, |state| {
                match mutation {
                    0 => {
                        let byte = state["runtime"]["pending_tool"]["approval_request"]
                            ["signature"][0]
                            .as_u64()
                            .unwrap();
                        state["runtime"]["pending_tool"]["approval_request"]["signature"][0] =
                            json!((byte + 1) % 256);
                    }
                    1 => {
                        let created_at = state["runtime"]["pending_tool"]["step_intent"]
                            ["created_at"]
                            .as_str()
                            .unwrap()
                            .parse::<DateTime<Utc>>()
                            .unwrap();
                        state["runtime"]["pending_tool"]["step_intent"]["updated_at"] =
                            json!(created_at + chrono::Duration::seconds(1));
                    }
                    2 => {
                        state["runtime"]["pending_tool"]["call_id"] = json!("call-substituted");
                        recompute_pending_decision_digest(state);
                    }
                    3 => {
                        state["runtime"]["pending_tool"]["arguments"]["version_label"] =
                            json!("2026.08.31-substituted");
                        recompute_pending_decision_digest(state);
                    }
                    _ => unreachable!(),
                }
            });
            let registry = load_registry(&fixture.workspace.root).unwrap();
            let review = build_approval_review(
                &fixture.store,
                &registry,
                &fixture.request,
                &[fixture.approver_id],
                Utc::now(),
            )
            .unwrap();

            assert!(!review.actionable, "mutation {mutation} was actionable");
            let expected = match mutation {
                0 => "exact signed approval request",
                1 => "exact durable waiting step",
                2 | 3 => "native runtime checkpoint history",
                _ => unreachable!(),
            };
            assert!(
                review
                    .blocked_reasons
                    .iter()
                    .any(|reason| reason.contains(expected)),
                "mutation {mutation}: {:?}",
                review.blocked_reasons
            );
        }
    }

    #[tokio::test]
    async fn live_v2_approval_response_never_recommends_generic_resume() {
        let fixture = crate::commands::live::tests::approval_live_fixture();
        let cli = Cli::parse_from([
            "proof",
            "-w",
            fixture.workspace.root.to_str().unwrap(),
            "approval",
            "approver-init",
        ]);
        cmd_approver_init(&cli).unwrap();
        let store = open_store(&fixture.workspace.root).unwrap();
        let wrong_approver = trusted_approver_ids(&fixture.workspace.root, &store)
            .unwrap()
            .into_iter()
            .find(|candidate| *candidate != fixture.approver_id)
            .unwrap();
        let registry = load_registry(&fixture.workspace.root).unwrap();
        let app = approval_ui_router(ApprovalUiState::from_parts(
            fixture.workspace.root.clone(),
            store,
            registry,
            TOKEN,
            4173,
        ));
        let uri = format!("/api/approvals/{}/decision", fixture.request.body.id);

        let detail = app
            .clone()
            .oneshot(request(
                Method::GET,
                &format!("/api/approvals/{}", fixture.request.body.id),
                Some(TOKEN),
                None,
                None,
            ))
            .await
            .unwrap();
        assert_eq!(detail.status(), StatusCode::OK);
        let detail = json_body(detail).await;
        assert_eq!(detail["approvers"], json!([fixture.approver_id]));
        assert_eq!(detail["approval"]["actionable"], true);

        let wrong = app
            .clone()
            .oneshot(request(
                Method::POST,
                &uri,
                Some(TOKEN),
                Some(ORIGIN_VALUE),
                Some(json!({
                    "approver_id": wrong_approver,
                    "outcome": "approved",
                    "reason": "wrong sealed human",
                })),
            ))
            .await
            .unwrap();
        assert_eq!(wrong.status(), StatusCode::BAD_REQUEST);

        let approved = app
            .oneshot(request(
                Method::POST,
                &uri,
                Some(TOKEN),
                Some(ORIGIN_VALUE),
                Some(json!({
                    "approver_id": fixture.approver_id,
                    "outcome": "approved",
                    "reason": "exact sealed live review",
                })),
            ))
            .await
            .unwrap();
        assert_eq!(approved.status(), StatusCode::OK);
        let approved = json_body(approved).await;
        assert_eq!(approved["status"], "approved");
        assert_eq!(approved["resume_command"], Value::Null);
    }

    #[test]
    fn view_model_fails_closed_for_mixed_malformed_and_unsupported_runtime_history() {
        let (_directory, _cli, workspace, store, approver_id) = initialized_workspace();
        let now = Utc::now();
        let mixed = save_native_approval(
            &store,
            &workspace,
            json!({"release_id": "mixed"}),
            json!({"release_id": "mixed"}),
            now,
            now + chrono::Duration::minutes(15),
        );
        let mixed_checkpoint = AgentCheckpoint::create(
            mixed.run_id,
            1,
            json!({"kind": "agent_runtime_v2", "runtime": {}}),
            now,
        )
        .unwrap();
        store.save_agent_checkpoint(&mixed_checkpoint).unwrap();
        let mixed_review = build_approval_review(
            &store,
            &human_only_registry(),
            &mixed.request,
            &[approver_id],
            now,
        )
        .unwrap();
        assert!(!mixed_review.actionable);
        assert!(mixed_review
            .blocked_reasons
            .iter()
            .any(|reason| reason.contains("mixed, unsupported, or invalid")));

        let malformed = save_native_approval(
            &store,
            &workspace,
            json!({"release_id": "malformed"}),
            json!({"release_id": "malformed"}),
            now,
            now + chrono::Duration::minutes(15),
        );
        let malformed_checkpoint = AgentCheckpoint::create(
            malformed.run_id,
            1,
            json!({"kind": V1_RUNTIME_CHECKPOINT_KIND, "runtime": {}}),
            now,
        )
        .unwrap();
        store.save_agent_checkpoint(&malformed_checkpoint).unwrap();
        let malformed_review = build_approval_review(
            &store,
            &human_only_registry(),
            &malformed.request,
            &[approver_id],
            now,
        )
        .unwrap();
        assert!(!malformed_review.actionable);
        assert!(malformed_review
            .blocked_reasons
            .iter()
            .any(|reason| reason.contains("mixed, unsupported, or invalid")));

        let unsupported = save_native_approval(
            &store,
            &workspace,
            json!({"release_id": "unsupported"}),
            json!({"release_id": "unsupported"}),
            now,
            now + chrono::Duration::minutes(15),
        );
        let connection =
            rusqlite::Connection::open(workspace.root.join(".proof/storage/storage.db")).unwrap();
        connection
            .execute(
                "DELETE FROM agent_checkpoints WHERE run_id = ?1",
                [unsupported.run_id.to_string()],
            )
            .unwrap();
        let unsupported_checkpoint = AgentCheckpoint::create(
            unsupported.run_id,
            0,
            json!({"kind": "agent_runtime_v3", "runtime": {}}),
            now,
        )
        .unwrap();
        store
            .save_agent_checkpoint(&unsupported_checkpoint)
            .unwrap();
        let unsupported_review = build_approval_review(
            &store,
            &human_only_registry(),
            &unsupported.request,
            &[approver_id],
            now,
        )
        .unwrap();
        assert!(!unsupported_review.actionable);
        assert!(unsupported_review
            .blocked_reasons
            .iter()
            .any(|reason| reason.contains("mixed, unsupported, or invalid")));
    }

    #[test]
    fn view_model_fails_closed_for_expired_mismatched_and_missing_context() {
        let (_directory, _cli, workspace, store, approver_id) = initialized_workspace();
        let now = Utc::now();
        let expired = save_native_approval(
            &store,
            &workspace,
            json!({"release_id": "expired"}),
            json!({"release_id": "expired"}),
            now - chrono::Duration::minutes(30),
            now - chrono::Duration::minutes(15),
        );
        let mismatched = save_native_approval(
            &store,
            &workspace,
            json!({"release_id": "signed"}),
            json!({"release_id": "substituted"}),
            now,
            now + chrono::Duration::minutes(15),
        );
        let missing = save_contextless_request(&store, &workspace, now);
        let registry = human_only_registry();

        let expired =
            build_approval_review(&store, &registry, &expired.request, &[approver_id], now)
                .unwrap();
        let mismatched =
            build_approval_review(&store, &registry, &mismatched.request, &[approver_id], now)
                .unwrap();
        let missing =
            build_approval_review(&store, &registry, &missing, &[approver_id], now).unwrap();

        assert_eq!(expired.status, ApprovalReviewStatus::Expired);
        assert!(!expired.actionable);
        assert!(mismatched
            .blocked_reasons
            .iter()
            .any(|reason| reason.contains("displayed arguments")));
        assert!(!mismatched.actionable);
        assert!(missing
            .blocked_reasons
            .iter()
            .any(|reason| reason.contains("native approval context")));
        assert!(!missing.actionable);
    }

    #[test]
    fn view_model_fails_closed_when_step_payload_points_to_another_approval() {
        let (_directory, _cli, workspace, store, approver_id) = initialized_workspace();
        let now = Utc::now();
        let fixture = save_native_approval(
            &store,
            &workspace,
            json!({"release_id": "mislinked-step"}),
            json!({"release_id": "mislinked-step"}),
            now,
            now + chrono::Duration::minutes(15),
        );
        let step = store
            .find_agent_run_step_by_approval(&fixture.request.body.id)
            .unwrap()
            .unwrap();
        let other_request_id = Uuid::now_v7();
        assert!(!step_approval_link_mismatch(
            Some(fixture.request.body.id),
            fixture.request.body.id
        ));
        assert!(step_approval_link_mismatch(
            Some(other_request_id),
            fixture.request.body.id
        ));
        assert!(step_approval_link_mismatch(None, fixture.request.body.id));
        let mut step_json = serde_json::to_value(&step).unwrap();
        step_json["approval_request_id"] = json!(other_request_id);
        let connection =
            rusqlite::Connection::open(workspace.root.join(".proof/storage/storage.db")).unwrap();
        connection
            .execute(
                "UPDATE agent_run_steps SET step_json = ?1 WHERE id = ?2",
                rusqlite::params![
                    serde_json::to_string(&step_json).unwrap(),
                    step.id.to_string()
                ],
            )
            .unwrap();

        let error = build_approval_review(
            &store,
            &human_only_registry(),
            &fixture.request,
            &[approver_id],
            now,
        )
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("approval binding does not match indexed request"));
    }

    #[tokio::test]
    async fn router_inbox_and_detail_require_token_and_expose_verified_context() {
        let (_directory, _cli, workspace, store, _approver_id) = initialized_workspace();
        let now = Utc::now();
        let fixture = save_native_approval(
            &store,
            &workspace,
            json!({"release_id": "release-router"}),
            json!({"release_id": "release-router"}),
            now,
            now + chrono::Duration::minutes(15),
        );
        let app = approval_ui_router(state(workspace.root.clone(), store));

        let unauthorized = app
            .clone()
            .oneshot(request(
                Method::GET,
                "/api/approvals",
                Some("wrong-token"),
                None,
                None,
            ))
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let inbox = app
            .clone()
            .oneshot(request(
                Method::GET,
                "/api/approvals",
                Some(TOKEN),
                None,
                None,
            ))
            .await
            .unwrap();
        assert_eq!(inbox.status(), StatusCode::OK);
        let inbox = json_body(inbox).await;
        assert_eq!(
            inbox["approvals"][0]["request_id"],
            fixture.request.body.id.to_string()
        );
        assert_eq!(inbox["approvals"][0]["actionable"], true);

        let detail = app
            .oneshot(request(
                Method::GET,
                &format!("/api/approvals/{}", fixture.request.body.id),
                Some(TOKEN),
                None,
                None,
            ))
            .await
            .unwrap();
        assert_eq!(detail.status(), StatusCode::OK);
        let detail = json_body(detail).await;
        assert_eq!(detail["approval"]["arguments"], fixture.arguments);
        assert_eq!(
            detail["approval"]["run"]["goal"],
            "Publish the approved release to preview"
        );
        assert_eq!(
            detail["approval"]["operation"]["consequence"],
            "content-release"
        );
    }

    #[tokio::test]
    async fn router_signs_approve_and_deny_without_resuming_or_executing() {
        let (_directory, _cli, workspace, store, approver_id) = initialized_workspace();
        let now = Utc::now();
        let approved = save_native_approval(
            &store,
            &workspace,
            json!({"release_id": "approve-me"}),
            json!({"release_id": "approve-me"}),
            now,
            now + chrono::Duration::minutes(15),
        );
        let denied = save_native_approval(
            &store,
            &workspace,
            json!({"release_id": "deny-me"}),
            json!({"release_id": "deny-me"}),
            now,
            now + chrono::Duration::minutes(15),
        );
        let app = approval_ui_router(state(workspace.root.clone(), store));

        for (fixture, outcome, reason) in [
            (&approved, "approved", "reviewed target"),
            (&denied, "denied", "policy blocked"),
        ] {
            let response = app
                .clone()
                .oneshot(request(
                    Method::POST,
                    &format!("/api/approvals/{}/decision", fixture.request.body.id),
                    Some(TOKEN),
                    Some(ORIGIN_VALUE),
                    Some(json!({
                        "approver_id": approver_id,
                        "outcome": outcome,
                        "reason": reason,
                    })),
                ))
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                StatusCode::OK,
                "{}",
                json_body(response).await
            );
        }

        let store = open_store(&workspace.root).unwrap();
        assert_eq!(
            store
                .load_approval_decision(&approved.request.body.id)
                .unwrap()
                .unwrap()
                .body
                .outcome,
            ApprovalOutcome::Approved
        );
        assert_eq!(
            store
                .load_approval_decision(&denied.request.body.id)
                .unwrap()
                .unwrap()
                .body
                .outcome,
            ApprovalOutcome::Denied
        );
        for fixture in [&approved, &denied] {
            assert_eq!(
                store
                    .load_agent_run(&fixture.run_id)
                    .unwrap()
                    .unwrap()
                    .status,
                AgentRunStatus::WaitingForInput
            );
            assert!(store
                .load_approval_execution(&fixture.request.body.id)
                .unwrap()
                .is_none());
        }
    }

    #[tokio::test]
    async fn router_rejects_unauthorized_cross_origin_and_non_json_mutations() {
        let (_directory, _cli, workspace, store, approver_id) = initialized_workspace();
        let now = Utc::now();
        let fixture = save_native_approval(
            &store,
            &workspace,
            json!({"release_id": "secure-request"}),
            json!({"release_id": "secure-request"}),
            now,
            now + chrono::Duration::minutes(15),
        );
        let app = approval_ui_router(state(workspace.root.clone(), store));
        let uri = format!("/api/approvals/{}/decision", fixture.request.body.id);
        let body = json!({
            "approver_id": approver_id,
            "outcome": "approved",
            "reason": "reviewed",
        });

        let unauthorized = app
            .clone()
            .oneshot(request(
                Method::POST,
                &uri,
                Some("wrong-token"),
                Some(ORIGIN_VALUE),
                Some(body.clone()),
            ))
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

        let cross_origin = app
            .clone()
            .oneshot(request(
                Method::POST,
                &uri,
                Some(TOKEN),
                Some("http://malicious.invalid"),
                Some(body.clone()),
            ))
            .await
            .unwrap();
        assert_eq!(cross_origin.status(), StatusCode::FORBIDDEN);

        let forged_host_and_origin = Request::builder()
            .method(Method::POST)
            .uri(&uri)
            .header(HOST, "attacker.invalid")
            .header(ORIGIN, "http://attacker.invalid")
            .header(SESSION_HEADER, TOKEN)
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let forged_host_and_origin = app.clone().oneshot(forged_host_and_origin).await.unwrap();
        assert_eq!(forged_host_and_origin.status(), StatusCode::FORBIDDEN);

        let non_json = Request::builder()
            .method(Method::POST)
            .uri(&uri)
            .header(HOST, HOST_VALUE)
            .header(ORIGIN, ORIGIN_VALUE)
            .header(SESSION_HEADER, TOKEN)
            .header(CONTENT_TYPE, "text/plain")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let non_json = app.clone().oneshot(non_json).await.unwrap();
        assert_eq!(non_json.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);

        let accepted = app
            .oneshot(request(
                Method::POST,
                &uri,
                Some(TOKEN),
                Some(ORIGIN_VALUE),
                Some(body),
            ))
            .await
            .unwrap();
        assert_eq!(accepted.status(), StatusCode::OK);

        let store = open_store(&workspace.root).unwrap();
        assert_eq!(
            store
                .load_approval_decision(&fixture.request.body.id)
                .unwrap()
                .unwrap()
                .body
                .outcome,
            ApprovalOutcome::Approved
        );
    }

    #[tokio::test]
    async fn router_rejects_expired_mismatched_missing_context_and_double_submit() {
        let (_directory, _cli, workspace, store, approver_id) = initialized_workspace();
        let now = Utc::now();
        let expired = save_native_approval(
            &store,
            &workspace,
            json!({"release_id": "expired-router"}),
            json!({"release_id": "expired-router"}),
            now - chrono::Duration::minutes(30),
            now - chrono::Duration::minutes(15),
        );
        let mismatched = save_native_approval(
            &store,
            &workspace,
            json!({"release_id": "signed-router"}),
            json!({"release_id": "substituted-router"}),
            now,
            now + chrono::Duration::minutes(15),
        );
        let missing = save_contextless_request(&store, &workspace, now);
        let actionable = save_native_approval(
            &store,
            &workspace,
            json!({"release_id": "one-decision"}),
            json!({"release_id": "one-decision"}),
            now,
            now + chrono::Duration::minutes(15),
        );
        let app = approval_ui_router(state(workspace.root.clone(), store));
        let decision_body = |outcome: &str| {
            json!({
                "approver_id": approver_id,
                "outcome": outcome,
                "reason": "tested",
            })
        };

        for request_id in [
            expired.request.body.id,
            mismatched.request.body.id,
            missing.body.id,
        ] {
            let response = app
                .clone()
                .oneshot(request(
                    Method::POST,
                    &format!("/api/approvals/{request_id}/decision"),
                    Some(TOKEN),
                    Some(ORIGIN_VALUE),
                    Some(decision_body("approved")),
                ))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::CONFLICT);
        }

        let uri = format!("/api/approvals/{}/decision", actionable.request.body.id);
        let first = app
            .clone()
            .oneshot(request(
                Method::POST,
                &uri,
                Some(TOKEN),
                Some(ORIGIN_VALUE),
                Some(decision_body("approved")),
            ))
            .await
            .unwrap();
        assert_eq!(first.status(), StatusCode::OK);
        let second = app
            .oneshot(request(
                Method::POST,
                &uri,
                Some(TOKEN),
                Some(ORIGIN_VALUE),
                Some(decision_body("denied")),
            ))
            .await
            .unwrap();
        assert_eq!(second.status(), StatusCode::CONFLICT);

        let store = open_store(&workspace.root).unwrap();
        let decision = store
            .load_approval_decision(&actionable.request.body.id)
            .unwrap()
            .unwrap();
        assert_eq!(decision.body.outcome, ApprovalOutcome::Approved);
        assert!(store
            .load_approval_execution(&actionable.request.body.id)
            .unwrap()
            .is_none());
        let loaded_keypair = load_approver_keypair(&workspace.root, approver_id).unwrap();
        assert_eq!(loaded_keypair.kind, PrincipalKind::Human);
    }
}
