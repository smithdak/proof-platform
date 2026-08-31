//! Frozen E0001 live Release Manager setup and credential boundary.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::Path;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use chrono::{Duration, Utc};
use proof_agent_runtime::{
    AgentRuntime, AgentRuntimeOutcome, ApprovalEvidence, DeterministicTraceEvaluator,
    LiveAuthoritySetup, LiveBindingInputs, LivePolicyMaterial, LiveRunIntent, LiveRunSetup,
    ModelGateway, ModelGatewayFactory, ModelGatewayFactoryContext, ModelGatewayFactoryError,
    OpenAiResponsesGateway, TraceEvaluationPolicy, DEFAULT_OPENAI_BASE_URL,
};
use proof_kernel::{
    canonicalize, canonicalize_serialized, digest, AgentDefinition, AgentEvaluationOutcome,
    AgentRunEvaluation, ArtifactKind, ContentDigest, Delegation, DelegationChain, ExecutionEngine,
    PrincipalId, PrincipalKind, Registry,
};
use proof_storage::SqliteStore;
use rusqlite::OptionalExtension;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{load_registry, open_store, Cli, Workspace};

const LIVE_POLICY_SOURCE: &str = include_str!("../../../../evals/release-manager-live-v1.json");
const PREVIEW_POLICY_SOURCE: &str =
    include_str!("../../../../evals/release-manager-preview-v1.json");
const LIVE_ENDPOINT: &str = "https://api.openai.com/v1/responses";
const LIVE_MODEL: &str = "gpt-5.6-sol";
const LIVE_PROVIDER: &str = "openai";
const LIVE_SERVICE_TIER: &str = "default";
const LIVE_VERSION_LABEL: &str = "2026.08.30-rc1";

const DETERMINISTIC_CHECK_IDS: [&str; 10] = [
    "run_succeeded",
    "run_bound_to_agent",
    "trusted_run_actor",
    "expected_tool_calls",
    "successful_steps_with_valid_proofs",
    "calls_allowlisted",
    "required_approvals",
    "final_output_references",
    "lifecycle_integrity",
    "no_failure_events",
];

const LIVE_CHECK_IDS: [&str; 17] = [
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
];

const LIVE_TAMPER_IDS: [&str; 20] = [
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
];

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct PreflightEvidence {
    schema: String,
    policy_path: String,
    policy_digest: ContentDigest,
    trace_digest: ContentDigest,
    evaluator: String,
    run_id: Uuid,
    evaluation_id: Uuid,
    evaluation_created_at: chrono::DateTime<Utc>,
    outcome: String,
    score_bps: u16,
    passed_checks: u16,
    total_checks: u16,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ResumeBindings {
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedLiveGoal {
    edition_id: Uuid,
    version_label: String,
    manifest_digest: String,
    idempotency_key: Uuid,
}

trait ProviderEnvironment: Send + Sync {
    fn base_url(&self) -> Option<OsString>;
    fn api_key(&self) -> std::result::Result<String, std::env::VarError>;
}

trait GatewayConstructor: Send + Sync {
    fn construct(
        &self,
        api_key: String,
    ) -> std::result::Result<Arc<dyn ModelGateway>, ModelGatewayFactoryError>;
}

struct ProcessEnvironment;

struct DirectGatewayConstructor;

impl ProviderEnvironment for ProcessEnvironment {
    fn base_url(&self) -> Option<OsString> {
        std::env::var_os("OPENAI_BASE_URL")
    }

    fn api_key(&self) -> std::result::Result<String, std::env::VarError> {
        std::env::var("OPENAI_API_KEY")
    }
}

impl GatewayConstructor for DirectGatewayConstructor {
    fn construct(
        &self,
        api_key: String,
    ) -> std::result::Result<Arc<dyn ModelGateway>, ModelGatewayFactoryError> {
        let gateway =
            OpenAiResponsesGateway::new(api_key, DEFAULT_OPENAI_BASE_URL).map_err(|_| {
                ModelGatewayFactoryError::Construction(
                    "direct OpenAI gateway construction failed".to_string(),
                )
            })?;
        Ok(Arc::new(gateway))
    }
}

struct CliOpenAiGatewayFactory {
    environment: Arc<dyn ProviderEnvironment>,
    constructor: Arc<dyn GatewayConstructor>,
    #[cfg(test)]
    calls: Option<Arc<std::sync::atomic::AtomicUsize>>,
}

impl CliOpenAiGatewayFactory {
    fn new() -> Self {
        Self {
            environment: Arc::new(ProcessEnvironment),
            constructor: Arc::new(DirectGatewayConstructor),
            #[cfg(test)]
            calls: None,
        }
    }

    #[cfg(test)]
    fn with_test_seams(
        environment: Arc<dyn ProviderEnvironment>,
        constructor: Arc<dyn GatewayConstructor>,
        calls: Arc<std::sync::atomic::AtomicUsize>,
    ) -> Self {
        Self {
            environment,
            constructor,
            calls: Some(calls),
        }
    }
}

impl ModelGatewayFactory for CliOpenAiGatewayFactory {
    fn create(
        &self,
        context: &ModelGatewayFactoryContext,
    ) -> std::result::Result<Arc<dyn ModelGateway>, ModelGatewayFactoryError> {
        #[cfg(test)]
        if let Some(calls) = &self.calls {
            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        if context.provider != LIVE_PROVIDER
            || context.endpoint != LIVE_ENDPOINT
            || context.requested_model != LIVE_MODEL
            || context.service_tier != LIVE_SERVICE_TIER
            || context.request_body_digest == ContentDigest::from_bytes([0; 32])
        {
            return Err(ModelGatewayFactoryError::Configuration(
                "live gateway context does not match the frozen direct OpenAI policy".to_string(),
            ));
        }
        if self.environment.base_url().is_some() {
            return Err(ModelGatewayFactoryError::Configuration(
                "OPENAI_BASE_URL must be unset for the direct live profile".to_string(),
            ));
        }
        let api_key = self.environment.api_key().map_err(|_| {
            ModelGatewayFactoryError::Configuration(
                "OPENAI_API_KEY is required at the live credential boundary".to_string(),
            )
        })?;
        if api_key.trim().is_empty() {
            return Err(ModelGatewayFactoryError::Configuration(
                "OPENAI_API_KEY must not be empty at the live credential boundary".to_string(),
            ));
        }
        self.constructor.construct(api_key)
    }
}

pub(crate) fn cmd_agent_live_start(
    cli: &Cli,
    agent_id: &str,
    goal: &str,
    policy_file: &Path,
    preflight_evaluation_id: &str,
    delegation_id: &str,
) -> Result<()> {
    let workspace = Workspace::open(&cli.workspace)?;
    let store = Arc::new(open_store(&workspace.root)?);
    let agent_id = parse_uuid(agent_id, "agent")?;
    let preflight_evaluation_id = parse_uuid(preflight_evaluation_id, "preflight evaluation")?;
    let delegation_id = parse_uuid(delegation_id, "delegation")?;
    let setup = start_setup(
        &workspace,
        &store,
        agent_id,
        goal,
        policy_file,
        preflight_evaluation_id,
        delegation_id,
    )?;
    let runtime = build_live_runtime(
        &workspace,
        store,
        load_registry(&workspace.root)?,
        Arc::new(CliOpenAiGatewayFactory::new()),
    )?;
    print_live_outcome(runtime.run_live(setup)?)
}

pub(crate) fn cmd_agent_live_resume(cli: &Cli, run_id: &str, policy_file: &Path) -> Result<()> {
    let workspace = Workspace::open(&cli.workspace)?;
    let store = Arc::new(open_store(&workspace.root)?);
    let run_id = parse_uuid(run_id, "run")?;
    let setup = resume_setup(&workspace, &store, run_id, policy_file)?;
    let runtime = build_live_runtime(
        &workspace,
        store,
        load_registry(&workspace.root)?,
        Arc::new(CliOpenAiGatewayFactory::new()),
    )?;
    print_live_outcome(runtime.run_live(setup)?)
}

pub(super) fn start_setup(
    workspace: &Workspace,
    store: &SqliteStore,
    agent_id: Uuid,
    goal: &str,
    policy_file: &Path,
    preflight_evaluation_id: Uuid,
    delegation_id: Uuid,
) -> Result<LiveRunSetup> {
    let (preflight_evidence, preflight_evidence_digest) =
        verify_preflight(store, preflight_evaluation_id)?;
    let parsed_goal = parse_live_goal(goal)?;
    verify_synthetic_edition(&workspace.root, &parsed_goal)?;
    let agent = store
        .load_agent_definition(&agent_id)?
        .with_context(|| format!("agent definition not found: {agent_id}"))?;
    validate_live_agent(&agent)?;
    let (authority, delegation_digest) =
        load_live_authority(store, delegation_id, workspace.actor)?;
    let approver_principal_id = sole_live_approver(workspace, store)?;
    let binding_inputs = LiveBindingInputs {
        preflight_evidence_digest,
        agent_principal_id: workspace.actor,
        approver_principal_id,
        delegation_id,
        delegation_digest,
        edition_id: parsed_goal.edition_id,
        manifest_digest: parsed_goal.manifest_digest,
        idempotency_key: parsed_goal.idempotency_key,
        version_label: parsed_goal.version_label,
    };
    let policy = verify_live_policy(policy_file, goal, binding_inputs)?;
    Ok(LiveRunSetup {
        intent: LiveRunIntent::Start {
            agent_id,
            goal: goal.to_string(),
        },
        process_epoch_id: Uuid::now_v7(),
        preflight_evidence,
        preflight_evidence_digest,
        authority,
        policy,
    })
}

/// Runs every live-start check that does not require the edition leaf to exist.
/// Preparation uses this before publishing its sole synthetic input, then calls
/// `start_setup` itself after publication for the authoritative check-only pass.
pub(super) fn prevalidate_start_without_edition(
    workspace: &Workspace,
    store: &SqliteStore,
    agent_id: Uuid,
    goal: &str,
    policy_file: &Path,
    preflight_evaluation_id: Uuid,
    delegation_id: Uuid,
) -> Result<()> {
    let (_, preflight_evidence_digest) = verify_preflight(store, preflight_evaluation_id)?;
    let parsed_goal = parse_live_goal(goal)?;
    let agent = store
        .load_agent_definition(&agent_id)?
        .with_context(|| format!("agent definition not found: {agent_id}"))?;
    validate_live_agent(&agent)?;
    let (_, delegation_digest) = load_live_authority(store, delegation_id, workspace.actor)?;
    let approver_principal_id = sole_live_approver(workspace, store)?;
    let binding_inputs = LiveBindingInputs {
        preflight_evidence_digest,
        agent_principal_id: workspace.actor,
        approver_principal_id,
        delegation_id,
        delegation_digest,
        edition_id: parsed_goal.edition_id,
        manifest_digest: parsed_goal.manifest_digest,
        idempotency_key: parsed_goal.idempotency_key,
        version_label: parsed_goal.version_label,
    };
    verify_live_policy(policy_file, goal, binding_inputs)?;
    Ok(())
}

fn resume_setup(
    workspace: &Workspace,
    store: &SqliteStore,
    run_id: Uuid,
    policy_file: &Path,
) -> Result<LiveRunSetup> {
    let run = store
        .load_agent_run(&run_id)?
        .with_context(|| format!("agent run not found: {run_id}"))?;
    let checkpoint = store
        .list_agent_checkpoints(&run_id)?
        .into_iter()
        .rev()
        .find(|checkpoint| checkpoint.state["kind"] == "agent_runtime_v2")
        .with_context(|| format!("agent run {run_id} has no sealed live checkpoint"))?;
    let runtime = checkpoint
        .state
        .get("runtime")
        .and_then(Value::as_object)
        .context("sealed live checkpoint runtime is missing or malformed")?;
    let policy_evidence = runtime
        .get("policy_evidence")
        .and_then(Value::as_object)
        .context("sealed live checkpoint policy evidence is missing or malformed")?;
    let stored_preflight = policy_evidence
        .get("preflight_evidence")
        .cloned()
        .context("sealed live checkpoint is missing preflight evidence")?;
    let preflight: PreflightEvidence = serde_json::from_value(stored_preflight.clone())
        .context("sealed live checkpoint preflight evidence is malformed")?;
    let (preflight_evidence, preflight_evidence_digest) =
        verify_preflight(store, preflight.evaluation_id)?;
    if preflight_evidence != stored_preflight {
        bail!("sealed live checkpoint preflight evidence no longer matches its trace");
    }
    let resolved_bindings: ResumeBindings = serde_json::from_value(
        policy_evidence
            .get("resolved_bindings")
            .cloned()
            .context("sealed live checkpoint is missing resolved bindings")?,
    )
    .context("sealed live checkpoint resolved bindings are malformed")?;
    if resolved_bindings.run_id != run_id
        || resolved_bindings.agent_id != run.agent_id.context("live run is not agent-bound")?
        || resolved_bindings.preflight_evidence_digest != preflight_evidence_digest
        || resolved_bindings.process_epoch_id.get_version_num() != 7
    {
        bail!("sealed live checkpoint identity or preflight binding is inconsistent");
    }
    let parsed_goal = parse_live_goal(&run.goal)?;
    if parsed_goal.edition_id != resolved_bindings.edition_id
        || parsed_goal.manifest_digest != resolved_bindings.manifest_digest
        || parsed_goal.idempotency_key != resolved_bindings.idempotency_key
        || parsed_goal.version_label != resolved_bindings.version_label
    {
        bail!("sealed live checkpoint bindings do not match the original goal");
    }
    verify_synthetic_edition(&workspace.root, &parsed_goal)?;
    if resolved_bindings.agent_principal_id != workspace.actor {
        bail!("sealed live checkpoint agent principal does not match this workspace");
    }
    require_live_approver(workspace, store, resolved_bindings.approver_principal_id)?;
    let (authority, delegation_digest) =
        load_live_authority(store, resolved_bindings.delegation_id, workspace.actor)?;
    if delegation_digest != resolved_bindings.delegation_digest {
        bail!("sealed live checkpoint delegation digest no longer matches the loaded grant");
    }
    let stored_delegation = policy_evidence
        .get("loaded_delegation")
        .context("sealed live checkpoint is missing its loaded delegation")?;
    if canonicalize(stored_delegation)?
        != canonicalize(&strict_delegation_value(&authority.delegation))?
    {
        bail!("sealed live checkpoint delegation differs from the loaded grant");
    }
    let stored_chain = policy_evidence
        .get("delegation_chain")
        .context("sealed live checkpoint is missing its delegation chain")?;
    let expected_chain = json!({
        "root": authority.delegation_chain.root,
        "grants": authority
            .delegation_chain
            .grants
            .iter()
            .map(strict_delegation_value)
            .collect::<Vec<_>>(),
    });
    if canonicalize(stored_chain)? != canonicalize(&expected_chain)? {
        bail!("sealed live checkpoint delegation chain differs from the exact loaded grant");
    }
    let binding_inputs = LiveBindingInputs {
        preflight_evidence_digest,
        agent_principal_id: resolved_bindings.agent_principal_id,
        approver_principal_id: resolved_bindings.approver_principal_id,
        delegation_id: resolved_bindings.delegation_id,
        delegation_digest,
        edition_id: resolved_bindings.edition_id,
        manifest_digest: resolved_bindings.manifest_digest,
        idempotency_key: resolved_bindings.idempotency_key,
        version_label: resolved_bindings.version_label,
    };
    let policy = verify_live_policy(policy_file, &run.goal, binding_inputs)?;
    Ok(LiveRunSetup {
        intent: LiveRunIntent::Resume { run_id },
        process_epoch_id: Uuid::now_v7(),
        preflight_evidence,
        preflight_evidence_digest,
        authority,
        policy,
    })
}

fn verify_preflight(store: &SqliteStore, evaluation_id: Uuid) -> Result<(Value, ContentDigest)> {
    let run_id: String = store
        .connection()
        .query_row(
            "SELECT run_id FROM agent_run_evaluations WHERE id = ?1",
            [evaluation_id.to_string()],
            |row| row.get(0),
        )
        .optional()?
        .with_context(|| format!("preflight evaluation not found: {evaluation_id}"))?;
    let run_id = Uuid::parse_str(&run_id).context("preflight evaluation has invalid run ID")?;
    let evaluation = store
        .list_agent_run_evaluations(&run_id)?
        .into_iter()
        .filter(|evaluation| evaluation.id == evaluation_id)
        .collect::<Vec<_>>();
    if evaluation.len() != 1 {
        bail!("preflight evaluation ID is not unique in its run ledger");
    }
    let evaluation = &evaluation[0];
    let recomputed = recompute_deterministic_evaluation(store, evaluation)?;
    if evaluation.run_id != recomputed.run_id
        || evaluation.evaluator != recomputed.evaluator
        || evaluation.outcome != recomputed.outcome
        || evaluation.score_bps != recomputed.score_bps
        || evaluation.metrics != recomputed.metrics
        || evaluation.summary != recomputed.summary
        || evaluation.created_at != recomputed.created_at
    {
        bail!("preflight evaluation does not match an independent trace recomputation");
    }
    let check_ids = evaluation.metrics["checks"]
        .as_array()
        .context("preflight evaluation is missing deterministic checks")?
        .iter()
        .map(|check| {
            if check["passed"] != true {
                bail!("preflight evaluation contains a failed deterministic check");
            }
            check["name"]
                .as_str()
                .context("preflight evaluation contains an unnamed check")
        })
        .collect::<Result<Vec<_>>>()?;
    if check_ids != DETERMINISTIC_CHECK_IDS
        || evaluation.outcome != AgentEvaluationOutcome::Passed
        || evaluation.score_bps != Some(10_000)
        || evaluation.metrics["passed_checks"] != 10
        || evaluation.metrics["total_checks"] != 10
        || evaluation.metrics["score_bps"] != 10_000
        || evaluation.evaluator != "proof-agent-trace/v1"
    {
        bail!("preflight evaluation is not the exact independently verified 10/10 record");
    }
    let preview_policy: TraceEvaluationPolicy = serde_json::from_str(PREVIEW_POLICY_SOURCE)
        .context("embedded deterministic preflight policy is invalid")?;
    let policy_digest = value_digest(&json!({
        "schema": "proof-agent-trace-policy/v1",
        "value": {"policy": preview_policy},
    }))?;
    if evaluation.metrics["binding"]["policy_digest"] != json!(policy_digest) {
        bail!("preflight policy digest does not match the frozen deterministic policy");
    }
    let trace_digest: ContentDigest =
        serde_json::from_value(evaluation.metrics["binding"]["trace_digest"].clone())
            .context("preflight trace digest is missing or malformed")?;
    let evidence = PreflightEvidence {
        schema: "proof-release-manager-preflight-evidence/v1".to_string(),
        policy_path: "evals/release-manager-preview-v1.json".to_string(),
        policy_digest,
        trace_digest,
        evaluator: evaluation.evaluator.clone(),
        run_id,
        evaluation_id,
        evaluation_created_at: evaluation.created_at,
        outcome: "passed".to_string(),
        score_bps: 10_000,
        passed_checks: 10,
        total_checks: 10,
    };
    let value = serde_json::to_value(evidence)?;
    let evidence_digest = wrapped_digest(
        "proof-release-manager-preflight-evidence-digest/v1",
        "evidence",
        &value,
    )?;
    Ok((value, evidence_digest))
}

fn recompute_deterministic_evaluation(
    store: &SqliteStore,
    evaluation: &AgentRunEvaluation,
) -> Result<AgentRunEvaluation> {
    deterministic_evaluation(
        store,
        evaluation.run_id,
        &evaluation.evaluator,
        evaluation.created_at,
    )
}

pub(super) fn deterministic_evaluation(
    store: &SqliteStore,
    run_id: Uuid,
    evaluator: &str,
    created_at: chrono::DateTime<Utc>,
) -> Result<AgentRunEvaluation> {
    let run = store
        .load_agent_run(&run_id)?
        .with_context(|| format!("preflight run not found: {run_id}"))?;
    let agent_id = run.agent_id.context("preflight run is not agent-bound")?;
    let agent = store
        .load_agent_definition(&agent_id)?
        .with_context(|| format!("preflight agent not found: {agent_id}"))?;
    let actor = store
        .load_principal(&run.actor)
        .context("preflight run actor is not enrolled")?;
    let steps = store.list_agent_run_steps(&run.id)?;
    let events = store.list_agent_run_events(&run.id)?;
    let mut approvals = Vec::new();
    let mut trusted_approvers = Vec::new();
    for request_id in steps.iter().filter_map(|step| step.approval_request_id) {
        let request = store
            .load_approval_request(&request_id)?
            .with_context(|| format!("preflight approval request missing: {request_id}"))?;
        let decision = store
            .load_approval_decision(&request_id)?
            .with_context(|| format!("preflight approval decision missing: {request_id}"))?;
        let execution = store
            .load_approval_execution(&request_id)?
            .with_context(|| format!("preflight approval execution missing: {request_id}"))?;
        let approver = store
            .load_principal(&decision.body.decided_by)
            .context("preflight approver is not enrolled")?;
        if !trusted_approvers
            .iter()
            .any(|trusted: &proof_kernel::Principal| trusted == &approver)
        {
            trusted_approvers.push(approver.clone());
        }
        approvals.push(ApprovalEvidence::new(
            request, decision, approver, execution,
        ));
    }
    let policy: TraceEvaluationPolicy = serde_json::from_str(PREVIEW_POLICY_SOURCE)
        .context("embedded deterministic preflight policy is invalid")?;
    DeterministicTraceEvaluator::new(policy)?
        .evaluate(
            &run,
            &agent,
            &actor,
            &trusted_approvers,
            &steps,
            &events,
            &approvals,
            evaluator,
            created_at,
        )
        .map_err(anyhow::Error::from)
}

fn verify_live_policy(
    policy_file: &Path,
    goal: &str,
    binding_inputs: LiveBindingInputs,
) -> Result<LivePolicyMaterial> {
    let supplied: Value = serde_json::from_str(
        &std::fs::read_to_string(policy_file)
            .with_context(|| format!("could not read live policy: {}", policy_file.display()))?,
    )
    .with_context(|| format!("invalid live policy: {}", policy_file.display()))?;
    let embedded: Value =
        serde_json::from_str(LIVE_POLICY_SOURCE).context("embedded live policy is invalid")?;
    if canonicalize(&supplied)? != canonicalize(&embedded)? {
        bail!("live policy differs from the frozen release-manager-live-v1 template");
    }
    let check_ids = supplied["checks"]
        .as_array()
        .context("live policy is missing its check set")?
        .iter()
        .map(|check| {
            check["id"]
                .as_str()
                .context("live policy check ID is invalid")
        })
        .collect::<Result<Vec<_>>>()?;
    let tamper_ids = supplied["tamper_vectors"]
        .as_array()
        .context("live policy is missing its tamper-vector set")?
        .iter()
        .map(|value| {
            value
                .as_str()
                .context("live policy tamper-vector ID is invalid")
        })
        .collect::<Result<Vec<_>>>()?;
    if check_ids != LIVE_CHECK_IDS
        || check_ids.iter().collect::<BTreeSet<_>>().len() != 17
        || tamper_ids != LIVE_TAMPER_IDS
        || tamper_ids.iter().collect::<BTreeSet<_>>().len() != 20
    {
        bail!("live policy does not contain the exact ordered 17-check and 20-vector sets");
    }
    let declaration = supplied
        .pointer("/tool/declaration")
        .context("live policy tool declaration is missing")?;
    Ok(LivePolicyMaterial {
        template_policy_digest: value_digest(&supplied)?,
        check_set_digest: wrapped_digest(
            "proof-release-manager-live-check-set-digest/v1",
            "check_ids",
            &json!(check_ids),
        )?,
        tamper_vector_set_digest: wrapped_digest(
            "proof-release-manager-live-tamper-vector-set-digest/v1",
            "tamper_vector_ids",
            &json!(tamper_ids),
        )?,
        pricing_schedule_digest: value_digest(&supplied["pricing"])?,
        instructions_digest: value_digest(&supplied["outbound_data"]["instructions"])?,
        initial_input_digest: value_digest(&Value::String(goal.to_string()))?,
        parameters_schema_digest: wrapped_digest(
            "proof-openai-function-parameters-digest/v1",
            "parameters",
            &declaration["parameters"],
        )?,
        tool_declaration_digest: wrapped_digest(
            "proof-openai-function-declaration-digest/v1",
            "declaration",
            declaration,
        )?,
        tool_set_digest: wrapped_digest(
            "proof-openai-tool-set-digest/v1",
            "tools",
            &json!([declaration]),
        )?,
        template: supplied,
        binding_inputs,
    })
}

fn load_live_authority(
    store: &SqliteStore,
    delegation_id: Uuid,
    agent_principal_id: PrincipalId,
) -> Result<(LiveAuthoritySetup, ContentDigest)> {
    let delegation = store
        .load_delegation(&delegation_id)?
        .with_context(|| format!("delegation not found: {delegation_id}"))?;
    let now = Utc::now();
    if delegation.id.get_version_num() != 7
        || delegation.recipient != agent_principal_id
        || delegation.revoked
        || delegation.valid_from > now
        || delegation.valid_until < now + Duration::seconds(300)
        || delegation.scope.allowed_operations.as_deref() != Some(&["release.publish".to_string()])
        || delegation.scope.allowed_domains.as_deref() != Some(&["content".to_string()])
        || delegation.scope.resource_scope.is_some()
    {
        bail!("delegation is not the exact active singleton release.publish/content grant");
    }
    let chain = DelegationChain {
        root: delegation.issuer,
        grants: vec![delegation.clone()],
    };
    chain
        .validate(agent_principal_id, now)
        .map_err(|error| anyhow::anyhow!("delegation chain is invalid: {error}"))?;
    let delegation_digest = digest(
        ArtifactKind::Delegation,
        &canonicalize_serialized(&delegation)?,
    );
    Ok((
        LiveAuthoritySetup {
            delegation,
            delegation_digest,
            delegation_chain: chain,
        },
        delegation_digest,
    ))
}

fn strict_delegation_value(delegation: &Delegation) -> Value {
    json!({
        "id": delegation.id,
        "issuer": delegation.issuer,
        "recipient": delegation.recipient,
        "allowed_actions": delegation.allowed_actions,
        "resource_scope": delegation.resource_scope,
        "scope": {
            "allowed_operations": delegation.scope.allowed_operations,
            "allowed_domains": delegation.scope.allowed_domains,
            "resource_scope": delegation.scope.resource_scope,
        },
        "valid_from": delegation.valid_from,
        "valid_until": delegation.valid_until,
        "revoked": delegation.revoked,
    })
}

pub(super) fn sole_live_approver(
    workspace: &Workspace,
    store: &SqliteStore,
) -> Result<PrincipalId> {
    let approvers = crate::commands::approval::trusted_approver_ids(&workspace.root, store)?;
    if approvers.len() != 1 {
        bail!("live start requires exactly one locally enrolled trusted human approver");
    }
    let approver = PrincipalId::new(approvers[0]);
    require_live_approver(workspace, store, approver)?;
    Ok(approver)
}

fn require_live_approver(
    workspace: &Workspace,
    store: &SqliteStore,
    approver: PrincipalId,
) -> Result<()> {
    if approver == workspace.actor {
        bail!("live approver must be distinct from the agent principal");
    }
    let trusted = store
        .load_principal(&approver)
        .context("live approver is not enrolled")?;
    if trusted.kind != PrincipalKind::Human
        || !crate::commands::approval::trusted_approver_ids(&workspace.root, store)?
            .contains(&approver.as_uuid())
    {
        bail!("live approver is not backed by the enrolled local human key");
    }
    Ok(())
}

fn validate_live_agent(agent: &AgentDefinition) -> Result<()> {
    if agent.provider != LIVE_PROVIDER
        || agent.model != LIVE_MODEL
        || agent.tools.len() != 1
        || agent.tools[0].operation != "release.publish"
        || agent.tools[0].version != "v2"
        || agent.limits.max_steps != 2
        || agent.limits.max_model_calls != 3
        || agent.limits.max_total_tokens != 10_000
        || agent.limits.max_duration_seconds != 300
        || agent.limits.max_output_tokens_per_call != 1024
        || agent.limits.max_cost_microusd != Some(120_000)
    {
        bail!("agent definition does not match the frozen release-manager live profile");
    }
    Ok(())
}

fn parse_live_goal(goal: &str) -> Result<ParsedLiveGoal> {
    let body = goal
        .strip_prefix("Publish synthetic edition ")
        .and_then(|value| value.strip_suffix('.'))
        .context("live goal does not match the frozen synthetic template")?;
    let (edition, rest) = body
        .split_once(" to preview as ")
        .context("live goal is missing the preview edition binding")?;
    let (version_label, rest) = rest
        .split_once(" using manifest ")
        .context("live goal is missing the version binding")?;
    let (manifest_digest, idempotency_key) = rest
        .split_once(" and idempotency key ")
        .context("live goal is missing the manifest or idempotency binding")?;
    let edition_id = parse_canonical_uuid(edition, "edition")?;
    let idempotency_key = parse_canonical_uuid(idempotency_key, "idempotency")?;
    if idempotency_key.get_version_num() != 7
        || version_label != LIVE_VERSION_LABEL
        || !valid_sha256_digest(manifest_digest)
    {
        bail!("live goal contains an invalid version, manifest, or UUIDv7 binding");
    }
    let parsed = ParsedLiveGoal {
        edition_id,
        version_label: version_label.to_string(),
        manifest_digest: manifest_digest.to_string(),
        idempotency_key,
    };
    if goal != resolved_goal(&parsed) {
        bail!("live goal is not the canonical resolved synthetic goal");
    }
    Ok(parsed)
}

fn resolved_goal(parsed: &ParsedLiveGoal) -> String {
    format!(
        "Publish synthetic edition {} to preview as {} using manifest {} and idempotency key {}.",
        parsed.edition_id, parsed.version_label, parsed.manifest_digest, parsed.idempotency_key
    )
}

fn verify_synthetic_edition(root: &Path, bindings: &ParsedLiveGoal) -> Result<()> {
    let directory =
        crate::commands::secure_fs::open_descendant(root, &[".proof", "data", "editions"])?;
    let name = format!("{}.json", bindings.edition_id);
    let bytes = directory
        .read_optional(&name)?
        .with_context(|| format!("synthetic edition not found: {}", bindings.edition_id))?;
    let raw: Value =
        serde_json::from_slice(&bytes).context("synthetic edition record is malformed")?;
    require_exact_keys(
        &raw,
        &[
            "id",
            "changeset_id",
            "objects",
            "created_at",
            "content_digest",
        ],
        "edition",
    )?;
    for object in raw["objects"]
        .as_array()
        .context("synthetic edition objects must be an array")?
    {
        require_exact_keys(
            object,
            &[
                "id",
                "schema_id",
                "schema_version",
                "locale",
                "content",
                "revision",
                "created_at",
                "updated_at",
                "status",
            ],
            "edition object",
        )?;
    }
    let edition: proof_content::Edition =
        serde_json::from_value(raw).context("synthetic edition record is invalid")?;
    if edition.id != bindings.edition_id {
        bail!("synthetic edition ID does not match the live goal");
    }
    let mut objects = edition.objects.clone();
    objects.sort_by(|left, right| left.id.cmp(&right.id));
    let edition_content_digest = proof_content::digest::canonical_digest(&objects);
    if edition.content_digest != edition_content_digest {
        bail!("synthetic edition content digest does not match its objects");
    }
    let manifest = json!({
        "schema": "proof-content-preview-manifest/v1",
        "edition_id": edition.id,
        "edition_content_digest": edition_content_digest,
        "objects": objects.iter().map(|object| json!({
            "object_id": object.id,
            "locale": object.locale,
            "content_digest": proof_content::digest::canonical_digest(object),
        })).collect::<Vec<_>>(),
    });
    if proof_content::digest::canonical_digest(&manifest) != bindings.manifest_digest {
        bail!("live manifest digest does not match the synthetic edition");
    }
    Ok(())
}

fn require_exact_keys(value: &Value, expected: &[&str], label: &str) -> Result<()> {
    let object = value
        .as_object()
        .with_context(|| format!("{label} must be an object"))?;
    let actual = object.keys().map(String::as_str).collect::<BTreeSet<_>>();
    let expected = expected.iter().copied().collect::<BTreeSet<_>>();
    if actual != expected {
        bail!("{label} contains missing or unknown fields");
    }
    Ok(())
}

fn valid_sha256_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|suffix| {
        suffix.len() == 64
            && suffix
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

pub(super) fn build_live_runtime(
    workspace: &Workspace,
    store: Arc<SqliteStore>,
    registry: Registry,
    factory: Arc<dyn ModelGatewayFactory>,
) -> Result<AgentRuntime> {
    let mut engine = ExecutionEngine::new_with_keypair(registry.clone(), workspace.keypair.clone())
        .with_storage(store.clone());
    for handler in proof_content::content_handlers() {
        engine.register_handler(handler);
    }
    for handler in proof_commerce::commerce_handlers() {
        engine.register_handler(handler);
    }
    for handler in proof_workflow::workflow_handlers() {
        engine.register_handler(handler);
    }
    for handler in proof_analytics::analytics_handlers() {
        engine.register_handler(handler);
    }
    AgentRuntime::new_with_gateway_factory(
        registry,
        engine,
        workspace.keypair.clone(),
        workspace.root.clone(),
        store.clone(),
        store.clone(),
        store,
        factory,
    )
    .map_err(anyhow::Error::from)
}

fn print_live_outcome(outcome: AgentRuntimeOutcome) -> Result<()> {
    let next = match &outcome {
        AgentRuntimeOutcome::WaitingForApproval { run, request, .. } => Some(json!({
            "approve": format!("proof approval approve {} --approver <approver-id>", request.body.id),
            "deny": format!("proof approval deny {} --approver <approver-id>", request.body.id),
            "resume": format!("proof agent live-resume {} --policy-file evals/release-manager-live-v1.json", run.id),
        })),
        AgentRuntimeOutcome::Completed { run, .. } | AgentRuntimeOutcome::Failed { run, .. } => {
            Some(json!({"watch": format!("proof agent watch {}", run.id)}))
        }
    };
    let mut value = serde_json::to_value(outcome)?;
    if let Some(next) = next {
        value["next"] = next;
    }
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

fn value_digest(value: &Value) -> Result<ContentDigest> {
    Ok(digest(ArtifactKind::Generic, &canonicalize(value)?))
}

fn wrapped_digest(schema: &str, field: &str, value: &Value) -> Result<ContentDigest> {
    let mut wrapper = serde_json::Map::new();
    wrapper.insert("schema".to_string(), Value::String(schema.to_string()));
    wrapper.insert(field.to_string(), value.clone());
    value_digest(&Value::Object(wrapper))
}

fn parse_uuid(value: &str, label: &str) -> Result<Uuid> {
    Uuid::parse_str(value).with_context(|| format!("invalid {label} ID"))
}

fn parse_canonical_uuid(value: &str, label: &str) -> Result<Uuid> {
    let id = Uuid::parse_str(value).with_context(|| format!("invalid {label} ID"))?;
    if id.to_string() != value {
        bail!("{label} ID must use lowercase hyphenated UUID spelling");
    }
    Ok(id)
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::VecDeque;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    use clap::Parser;
    use proof_agent_runtime::{
        ModelDecision, ModelGatewayError, ModelInput, ModelTurn, ModelTurnRequest, ModelUsage,
    };
    use proof_kernel::{
        create_proof, AgentLimits, AgentRun, AgentRunEvent, AgentRunEventKind, AgentRunMode,
        AgentRunStatus, AgentRunStep, AgentTool, ApprovalExecution, ApprovalOutcome, Delegation,
        SignedApprovalDecision, SignedApprovalRequest,
    };

    use super::*;

    struct CountingEnvironment {
        base_url: Option<OsString>,
        api_key: Option<String>,
        base_reads: AtomicUsize,
        key_reads: AtomicUsize,
    }

    impl CountingEnvironment {
        fn missing_key() -> Self {
            Self {
                base_url: None,
                api_key: None,
                base_reads: AtomicUsize::new(0),
                key_reads: AtomicUsize::new(0),
            }
        }
    }

    impl ProviderEnvironment for CountingEnvironment {
        fn base_url(&self) -> Option<OsString> {
            self.base_reads.fetch_add(1, Ordering::SeqCst);
            self.base_url.clone()
        }

        fn api_key(&self) -> std::result::Result<String, std::env::VarError> {
            self.key_reads.fetch_add(1, Ordering::SeqCst);
            self.api_key.clone().ok_or(std::env::VarError::NotPresent)
        }
    }

    struct CountingConstructor {
        constructions: AtomicUsize,
        sends: Arc<AtomicUsize>,
    }

    impl CountingConstructor {
        fn new(sends: Arc<AtomicUsize>) -> Self {
            Self {
                constructions: AtomicUsize::new(0),
                sends,
            }
        }
    }

    impl GatewayConstructor for CountingConstructor {
        fn construct(
            &self,
            _api_key: String,
        ) -> std::result::Result<Arc<dyn ModelGateway>, ModelGatewayFactoryError> {
            self.constructions.fetch_add(1, Ordering::SeqCst);
            Ok(Arc::new(CountingGateway {
                sends: self.sends.clone(),
            }))
        }
    }

    struct CountingGateway {
        sends: Arc<AtomicUsize>,
    }

    impl ModelGateway for CountingGateway {
        fn provider(&self) -> &str {
            LIVE_PROVIDER
        }

        fn complete(
            &self,
            _request: &ModelTurnRequest,
        ) -> std::result::Result<ModelTurn, ModelGatewayError> {
            self.sends.fetch_add(1, Ordering::SeqCst);
            Ok(ModelTurn {
                response_id: "unused".to_string(),
                decision: ModelDecision::Finish {
                    output: "unused".to_string(),
                },
                usage: ModelUsage::default(),
                returned_model: Some(LIVE_MODEL.to_string()),
                response_body_digest: None,
            })
        }
    }

    enum ScriptedLiveAction {
        Tool { name: String, arguments: Value },
        FinishFromToolOutput,
    }

    struct ScriptedLiveGateway {
        actions: Mutex<VecDeque<ScriptedLiveAction>>,
        sends: Arc<AtomicUsize>,
    }

    impl ModelGateway for ScriptedLiveGateway {
        fn provider(&self) -> &str {
            LIVE_PROVIDER
        }

        fn complete(
            &self,
            request: &ModelTurnRequest,
        ) -> std::result::Result<ModelTurn, ModelGatewayError> {
            self.sends.fetch_add(1, Ordering::SeqCst);
            let action = self
                .actions
                .lock()
                .unwrap()
                .pop_front()
                .expect("scripted live action");
            let (response_id, decision) = match action {
                ScriptedLiveAction::Tool { name, arguments } => (
                    "response_tool",
                    ModelDecision::ToolCall {
                        call_id: "call_publish".to_string(),
                        name,
                        arguments,
                    },
                ),
                ScriptedLiveAction::FinishFromToolOutput => {
                    let ModelInput::ToolOutput { output, .. } = &request.input else {
                        panic!("finish action requires the committed tool output");
                    };
                    let result = &output["result"];
                    let report = format!(
                        "publication_id={} edition_id={} environment={} version_label={} manifest_digest={} relative_path={} artifact_digest={} proof_id={}",
                        result["data"]["publication_id"].as_str().unwrap(),
                        result["data"]["edition_id"].as_str().unwrap(),
                        result["data"]["environment"].as_str().unwrap(),
                        result["data"]["version_label"].as_str().unwrap(),
                        result["data"]["manifest_digest"].as_str().unwrap(),
                        result["data"]["artifact"]["relative_path"]
                            .as_str()
                            .unwrap(),
                        result["data"]["artifact"]["digest"].as_str().unwrap(),
                        output["proof_id"].as_str().unwrap(),
                    );
                    ("response_finish", ModelDecision::Finish { output: report })
                }
            };
            let usage = ModelUsage {
                input_tokens: 30,
                output_tokens: 10,
                total_tokens: 40,
                cost_microusd: None,
            };
            let response_body_digest = independent_value_digest(&json!({
                "id": response_id,
                "model": LIVE_MODEL,
                "status": "completed",
                "decision": decision,
                "usage": usage,
            }));
            Ok(ModelTurn {
                response_id: response_id.to_string(),
                returned_model: Some(LIVE_MODEL.to_string()),
                response_body_digest: Some(response_body_digest),
                decision,
                usage,
            })
        }
    }

    struct ScriptedLiveFactory {
        creates: Arc<AtomicUsize>,
        gateway: Arc<ScriptedLiveGateway>,
    }

    impl ModelGatewayFactory for ScriptedLiveFactory {
        fn create(
            &self,
            _context: &ModelGatewayFactoryContext,
        ) -> std::result::Result<Arc<dyn ModelGateway>, ModelGatewayFactoryError> {
            self.creates.fetch_add(1, Ordering::SeqCst);
            Ok(self.gateway.clone())
        }
    }

    struct LiveFixture {
        _directory: assert_fs::TempDir,
        workspace: Workspace,
        store: Arc<SqliteStore>,
        agent: AgentDefinition,
        goal: String,
        delegation: Delegation,
        preflight_evaluation: AgentRunEvaluation,
        policy_path: PathBuf,
    }

    fn live_fixture() -> LiveFixture {
        let directory = assert_fs::TempDir::new().unwrap();
        let cli = Cli::parse_from(["proof", "-w", directory.path().to_str().unwrap(), "init"]);
        crate::commands::content::cmd_init(&cli).unwrap();
        crate::commands::approval::cmd_approver_init(&cli).unwrap();
        let workspace = Workspace::open(&cli.workspace).unwrap();
        let store = Arc::new(open_store(&workspace.root).unwrap());
        let preflight_evaluation = persist_passing_preflight(&workspace, &store);

        let edition = proof_content::Edition::new(Uuid::now_v7(), Vec::new());
        std::fs::write(
            workspace
                .root
                .join(".proof/data/editions")
                .join(format!("{}.json", edition.id)),
            serde_json::to_vec_pretty(&edition).unwrap(),
        )
        .unwrap();
        let manifest = json!({
            "schema": "proof-content-preview-manifest/v1",
            "edition_id": edition.id,
            "edition_content_digest": edition.content_digest,
            "objects": [],
        });
        let manifest_digest = proof_content::digest::canonical_digest(&manifest);
        let parsed = ParsedLiveGoal {
            edition_id: edition.id,
            version_label: LIVE_VERSION_LABEL.to_string(),
            manifest_digest,
            idempotency_key: Uuid::now_v7(),
        };
        let goal = resolved_goal(&parsed);
        let agent = AgentDefinition::new(
            "live-release-manager",
            "Use only the frozen release publication tool.",
            LIVE_PROVIDER,
            LIVE_MODEL,
            vec![AgentTool::new("release.publish", "v2").unwrap()],
            AgentLimits {
                max_steps: 2,
                max_model_calls: 3,
                max_total_tokens: 10_000,
                max_duration_seconds: 300,
                max_output_tokens_per_call: 1024,
                max_cost_microusd: Some(120_000),
            },
            Utc::now(),
        )
        .unwrap();
        store.save_agent_definition(&agent).unwrap();
        let delegation = Delegation {
            id: Uuid::now_v7(),
            issuer: workspace.actor,
            recipient: workspace.actor,
            allowed_actions: Vec::new(),
            resource_scope: Vec::new(),
            scope: proof_kernel::delegation::DelegationScope {
                allowed_operations: Some(vec!["release.publish".to_string()]),
                allowed_domains: Some(vec!["content".to_string()]),
                resource_scope: None,
            },
            valid_from: Utc::now() - Duration::seconds(5),
            valid_until: Utc::now() + Duration::minutes(10),
            revoked: false,
        };
        store.save_delegation(&delegation).unwrap();
        let policy_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../evals/release-manager-live-v1.json");
        LiveFixture {
            _directory: directory,
            workspace,
            store,
            agent,
            goal,
            delegation,
            preflight_evaluation,
            policy_path,
        }
    }

    pub(crate) struct ApprovalLiveFixture {
        pub(crate) _directory: assert_fs::TempDir,
        pub(crate) workspace: Workspace,
        pub(crate) store: Arc<SqliteStore>,
        pub(crate) run_id: Uuid,
        pub(crate) request: SignedApprovalRequest,
        pub(crate) arguments: Value,
        pub(crate) approver_id: Uuid,
    }

    pub(crate) fn approval_live_fixture() -> ApprovalLiveFixture {
        let fixture = live_fixture();
        let registry_source =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../registry/content");
        let registry_target = fixture.workspace.root.join(".proof/registry/content");
        std::fs::create_dir_all(&registry_target).unwrap();
        for file in [
            "release-publish-v2.json",
            "release-publish-v2.input.json",
            "release-publish-v2.output.json",
        ] {
            std::fs::copy(registry_source.join(file), registry_target.join(file)).unwrap();
        }
        let parsed = parse_live_goal(&fixture.goal).unwrap();
        let template: Value = serde_json::from_str(LIVE_POLICY_SOURCE).unwrap();
        let tool_name = template["tool"]["declaration"]["name"]
            .as_str()
            .unwrap()
            .to_string();
        let arguments = json!({
            "idempotency_key": parsed.idempotency_key,
            "edition_id": parsed.edition_id,
            "environment": "preview",
            "version_label": parsed.version_label,
            "manifest_digest": parsed.manifest_digest,
        });
        let factory = Arc::new(ScriptedLiveFactory {
            creates: Arc::new(AtomicUsize::new(0)),
            gateway: Arc::new(ScriptedLiveGateway {
                actions: Mutex::new(
                    vec![ScriptedLiveAction::Tool {
                        name: tool_name,
                        arguments: arguments.clone(),
                    }]
                    .into(),
                ),
                sends: Arc::new(AtomicUsize::new(0)),
            }),
        });
        let start = start_setup(
            &fixture.workspace,
            &fixture.store,
            fixture.agent.id,
            &fixture.goal,
            &fixture.policy_path,
            fixture.preflight_evaluation.id,
            fixture.delegation.id,
        )
        .unwrap();
        let runtime = build_live_runtime(
            &fixture.workspace,
            fixture.store.clone(),
            load_registry(&fixture.workspace.root).unwrap(),
            factory,
        )
        .unwrap();
        let AgentRuntimeOutcome::WaitingForApproval { run, request, .. } =
            runtime.run_live(start).unwrap()
        else {
            panic!("exact live fixture must wait for approval")
        };
        let approver_id = sole_live_approver(&fixture.workspace, &fixture.store)
            .unwrap()
            .as_uuid();
        let LiveFixture {
            _directory,
            workspace,
            store,
            ..
        } = fixture;
        ApprovalLiveFixture {
            _directory,
            workspace,
            store,
            run_id: run.id,
            request,
            arguments,
            approver_id,
        }
    }

    fn persist_passing_preflight(workspace: &Workspace, store: &SqliteStore) -> AgentRunEvaluation {
        let now = Utc::now() - Duration::minutes(1);
        let approver_id =
            crate::commands::approval::trusted_approver_ids(&workspace.root, store).unwrap()[0];
        let approver =
            crate::commands::approval::load_approver_keypair(&workspace.root, approver_id).unwrap();
        let agent = AgentDefinition::new(
            "deterministic-release-manager",
            "Publish the exact approved deterministic preview.",
            "scripted",
            "scripted-model",
            vec![AgentTool::new("release.publish", "v1").unwrap()],
            AgentLimits::default(),
            now,
        )
        .unwrap();
        store.save_agent_definition(&agent).unwrap();
        let mut run = AgentRun::new_for_agent(
            workspace.actor,
            agent.id,
            AgentRunMode::OneShot,
            "Publish the deterministic preview release.",
            now,
        )
        .unwrap();
        store.save_agent_run(&run).unwrap();
        run.start(now + Duration::seconds(1)).unwrap();
        store.save_agent_run(&run).unwrap();
        run.wait_for_input(now + Duration::seconds(3)).unwrap();
        store.save_agent_run(&run).unwrap();

        let input = json!({"environment": "preview", "version_label": "2026.08.29-rc1"});
        let output = json!({
            "operation": "release.publish",
            "data": {
                "release": {
                    "id": Uuid::now_v7(),
                    "edition_id": Uuid::now_v7(),
                    "environment": "preview",
                    "published_at": (now + Duration::seconds(5)),
                    "published_by": workspace.actor,
                },
                "version_label": "2026.08.29-rc1",
            }
        });
        let mut step = AgentRunStep::new(
            run.id,
            0,
            "release.publish",
            "v1",
            &input,
            now + Duration::seconds(1),
        )
        .unwrap();
        store.save_agent_run_step(&step).unwrap();
        step.start(now + Duration::seconds(1)).unwrap();
        store.save_agent_run_step(&step).unwrap();
        let request = SignedApprovalRequest::create(
            "release.publish",
            "v1",
            &input,
            now + Duration::seconds(2),
            now + Duration::minutes(5),
            &workspace.keypair,
        )
        .unwrap();
        store.save_approval_request(&request).unwrap();
        step.wait_for_approval(request.body.id, now + Duration::seconds(2))
            .unwrap();
        store.save_agent_run_step(&step).unwrap();
        let decision = SignedApprovalDecision::create(
            &request,
            ApprovalOutcome::Approved,
            Some("approved synthetic preview".to_string()),
            now + Duration::seconds(3),
            &approver,
        )
        .unwrap();
        store.save_approval_decision(&decision).unwrap();
        step.resume_from_approval(now + Duration::seconds(4))
            .unwrap();
        store.save_agent_run_step(&step).unwrap();
        let proof = create_proof(
            workspace.actor,
            None,
            "release.publish::v1",
            &input,
            &output,
            now + Duration::seconds(5),
            &workspace.keypair,
        )
        .unwrap();
        step.succeed(output.clone(), proof.clone(), now + Duration::seconds(6))
            .unwrap();
        store.save_agent_run_step(&step).unwrap();
        store
            .save_approval_execution(&ApprovalExecution {
                request_id: request.body.id,
                executed_at: now + Duration::seconds(5),
                output: output.clone(),
                proof: proof.clone(),
            })
            .unwrap();
        run.resume(now + Duration::seconds(6)).unwrap();
        store.save_agent_run(&run).unwrap();

        let final_output = format!(
            "Release {} for edition {} published to preview as 2026.08.29-rc1 with proof {}.",
            output["data"]["release"]["id"].as_str().unwrap(),
            output["data"]["release"]["edition_id"].as_str().unwrap(),
            proof.body.id,
        );
        let event_data = vec![
            (
                AgentRunEventKind::Started,
                json!({"agent_id": agent.id, "goal": run.goal}),
            ),
            (
                AgentRunEventKind::ModelRequested,
                json!({"model": agent.model, "model_call": 1, "previous_response_id": null}),
            ),
            (
                AgentRunEventKind::ModelResponded,
                json!({
                    "response_id": "resp_release",
                    "decision": {
                        "type": "tool_call",
                        "call_id": "call_release",
                        "name": "proof_release_publish_v1",
                        "arguments": input,
                    },
                    "usage": {}
                }),
            ),
            (
                AgentRunEventKind::ToolRequested,
                json!({
                    "step_id": step.id,
                    "call_id": "call_release",
                    "tool": "proof_release_publish_v1",
                    "operation": "release.publish",
                    "version": "v1",
                    "arguments": input,
                }),
            ),
            (
                AgentRunEventKind::ApprovalRequired,
                json!({
                    "step_id": step.id,
                    "request_id": request.body.id,
                    "operation": request.body.operation,
                    "version": request.body.version,
                    "expires_at": request.body.expires_at,
                }),
            ),
            (
                AgentRunEventKind::ApprovalResumed,
                json!({
                    "step_id": step.id,
                    "request_id": request.body.id,
                    "decided_by": decision.body.decided_by,
                    "outcome": decision.body.outcome,
                }),
            ),
            (
                AgentRunEventKind::ToolSucceeded,
                json!({
                    "step_id": step.id,
                    "call_id": "call_release",
                    "operation": "release.publish",
                    "version": "v1",
                    "proof_id": proof.body.id,
                }),
            ),
            (
                AgentRunEventKind::ModelRequested,
                json!({"model": agent.model, "model_call": 2, "previous_response_id": "resp_release"}),
            ),
            (
                AgentRunEventKind::ModelResponded,
                json!({"response_id": "resp_finish", "decision": {"type": "finish", "output": final_output}, "usage": {}}),
            ),
            (
                AgentRunEventKind::Completed,
                json!({"output": final_output, "evaluation_id": Uuid::now_v7()}),
            ),
        ];
        for (sequence, (kind, data)) in event_data.into_iter().enumerate() {
            if kind == AgentRunEventKind::Completed {
                run.succeed(now + Duration::seconds(8)).unwrap();
                store.save_agent_run(&run).unwrap();
            }
            let created_at = match sequence {
                0 => now,
                1 => now + Duration::seconds(1),
                2 => now + Duration::milliseconds(1_100),
                3 => now + Duration::milliseconds(1_200),
                4 => now + Duration::seconds(2),
                5 => now + Duration::seconds(3),
                6 => now + Duration::seconds(6),
                7 => now + Duration::seconds(7),
                8 => now + Duration::milliseconds(7_100),
                9 => now + Duration::seconds(8),
                _ => unreachable!(),
            };
            store
                .save_agent_run_event(
                    &AgentRunEvent::create(run.id, sequence as u32, kind, data, created_at)
                        .unwrap(),
                )
                .unwrap();
        }
        let events = store.list_agent_run_events(&run.id).unwrap();
        let approval = ApprovalEvidence::new(
            request,
            decision,
            store.load_principal(&approver.principal_id).unwrap(),
            store
                .load_approval_execution(&step.approval_request_id.unwrap())
                .unwrap()
                .unwrap(),
        );
        let policy: TraceEvaluationPolicy = serde_json::from_str(PREVIEW_POLICY_SOURCE).unwrap();
        let evaluation = DeterministicTraceEvaluator::new(policy)
            .unwrap()
            .evaluate(
                &run,
                &agent,
                &store.load_principal(&workspace.actor).unwrap(),
                &[store.load_principal(&approver.principal_id).unwrap()],
                &[step],
                &events,
                &[approval],
                "proof-agent-trace/v1",
                now + Duration::seconds(9),
            )
            .unwrap();
        assert_eq!(
            evaluation.outcome,
            AgentEvaluationOutcome::Passed,
            "{}",
            evaluation.metrics
        );
        assert_eq!(evaluation.metrics["passed_checks"], 10);
        store.save_agent_run_evaluation(&evaluation).unwrap();
        evaluation
    }

    fn test_factory(
        environment: Arc<CountingEnvironment>,
        constructor: Arc<CountingConstructor>,
        calls: Arc<AtomicUsize>,
    ) -> Arc<dyn ModelGatewayFactory> {
        Arc::new(CliOpenAiGatewayFactory::with_test_seams(
            environment,
            constructor,
            calls,
        ))
    }

    #[test]
    fn exact_policy_sets_and_static_digests_are_recomputed() {
        let fixture = live_fixture();
        let parsed = parse_live_goal(&fixture.goal).unwrap();
        verify_synthetic_edition(&fixture.workspace.root, &parsed).unwrap();
        let (evidence, evidence_digest) =
            verify_preflight(&fixture.store, fixture.preflight_evaluation.id).unwrap();
        let (authority, delegation_digest) = load_live_authority(
            &fixture.store,
            fixture.delegation.id,
            fixture.workspace.actor,
        )
        .unwrap();
        let policy = verify_live_policy(
            &fixture.policy_path,
            &fixture.goal,
            LiveBindingInputs {
                preflight_evidence_digest: evidence_digest,
                agent_principal_id: fixture.workspace.actor,
                approver_principal_id: sole_live_approver(&fixture.workspace, &fixture.store)
                    .unwrap(),
                delegation_id: fixture.delegation.id,
                delegation_digest,
                edition_id: parsed.edition_id.clone(),
                manifest_digest: parsed.manifest_digest.clone(),
                idempotency_key: parsed.idempotency_key.clone(),
                version_label: parsed.version_label.clone(),
            },
        )
        .unwrap();
        assert_eq!(policy.template["checks"].as_array().unwrap().len(), 17);
        assert_eq!(
            policy.template["tamper_vectors"].as_array().unwrap().len(),
            20
        );
        let declaration = &policy.template["tool"]["declaration"];
        assert_eq!(
            policy.template_policy_digest,
            independent_value_digest(&policy.template)
        );
        assert_eq!(
            policy.check_set_digest,
            independent_wrapped_digest(
                "proof-release-manager-live-check-set-digest/v1",
                "check_ids",
                &json!(LIVE_CHECK_IDS)
            )
        );
        assert_eq!(
            policy.tamper_vector_set_digest,
            independent_wrapped_digest(
                "proof-release-manager-live-tamper-vector-set-digest/v1",
                "tamper_vector_ids",
                &json!(LIVE_TAMPER_IDS)
            )
        );
        assert_eq!(
            policy.pricing_schedule_digest,
            independent_value_digest(&policy.template["pricing"])
        );
        assert_eq!(
            policy.instructions_digest,
            independent_value_digest(&policy.template["outbound_data"]["instructions"])
        );
        assert_eq!(
            policy.initial_input_digest,
            independent_value_digest(&Value::String(fixture.goal.clone()))
        );
        assert_eq!(
            policy.parameters_schema_digest,
            independent_wrapped_digest(
                "proof-openai-function-parameters-digest/v1",
                "parameters",
                &declaration["parameters"]
            )
        );
        assert_eq!(
            policy.tool_declaration_digest,
            independent_wrapped_digest(
                "proof-openai-function-declaration-digest/v1",
                "declaration",
                declaration
            )
        );
        assert_eq!(
            policy.tool_set_digest,
            independent_wrapped_digest(
                "proof-openai-tool-set-digest/v1",
                "tools",
                &json!([declaration])
            )
        );
        assert_eq!(
            policy.binding_inputs.preflight_evidence_digest,
            evidence_digest
        );
        assert_eq!(
            policy.binding_inputs.agent_principal_id,
            fixture.workspace.actor
        );
        assert_eq!(policy.binding_inputs.delegation_id, fixture.delegation.id);
        assert_eq!(policy.binding_inputs.delegation_digest, delegation_digest);
        assert_eq!(policy.binding_inputs.edition_id, parsed.edition_id);
        assert_eq!(
            policy.binding_inputs.manifest_digest,
            parsed.manifest_digest
        );
        assert_eq!(
            policy.binding_inputs.idempotency_key,
            parsed.idempotency_key
        );
        assert_eq!(policy.binding_inputs.version_label, LIVE_VERSION_LABEL);
        assert_eq!(authority.delegation, fixture.delegation);
        assert_eq!(evidence["score_bps"], 10_000);
    }

    #[test]
    fn tampered_claimed_preflight_metrics_fail_independent_recomputation() {
        let fixture = live_fixture();
        let mut evaluation = fixture.preflight_evaluation.clone();
        evaluation.metrics["passed_checks"] = json!(9);
        fixture
            .store
            .connection()
            .execute(
                "UPDATE agent_run_evaluations SET evaluation_json = ?1 WHERE id = ?2",
                rusqlite::params![
                    serde_json::to_string(&evaluation).unwrap(),
                    evaluation.id.to_string()
                ],
            )
            .unwrap();
        let error = verify_preflight(&fixture.store, evaluation.id).unwrap_err();
        assert!(error
            .to_string()
            .contains("independent trace recomputation"));
    }

    #[test]
    fn presecret_setup_failure_reads_no_environment_or_constructs_gateway() {
        let fixture = live_fixture();
        let mut setup = start_setup(
            &fixture.workspace,
            &fixture.store,
            fixture.agent.id,
            &fixture.goal,
            &fixture.policy_path,
            fixture.preflight_evaluation.id,
            fixture.delegation.id,
        )
        .unwrap();
        setup.policy.check_set_digest = ContentDigest::from_bytes([7; 32]);
        let environment = Arc::new(CountingEnvironment::missing_key());
        let sends = Arc::new(AtomicUsize::new(0));
        let constructor = Arc::new(CountingConstructor::new(sends.clone()));
        let factory_calls = Arc::new(AtomicUsize::new(0));
        let runtime = build_live_runtime(
            &fixture.workspace,
            fixture.store.clone(),
            load_registry(&fixture.workspace.root).unwrap(),
            test_factory(
                environment.clone(),
                constructor.clone(),
                factory_calls.clone(),
            ),
        )
        .unwrap();
        let error = runtime.run_live(setup).unwrap_err();
        assert!(error.to_string().contains("17-check"));
        assert_eq!(factory_calls.load(Ordering::SeqCst), 0);
        assert_eq!(environment.base_reads.load(Ordering::SeqCst), 0);
        assert_eq!(environment.key_reads.load(Ordering::SeqCst), 0);
        assert_eq!(constructor.constructions.load(Ordering::SeqCst), 0);
        assert_eq!(sends.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn valid_local_setup_with_missing_key_fails_only_at_credential_boundary() {
        let fixture = live_fixture();
        let setup = start_setup(
            &fixture.workspace,
            &fixture.store,
            fixture.agent.id,
            &fixture.goal,
            &fixture.policy_path,
            fixture.preflight_evaluation.id,
            fixture.delegation.id,
        )
        .unwrap();
        let environment = Arc::new(CountingEnvironment::missing_key());
        let sends = Arc::new(AtomicUsize::new(0));
        let constructor = Arc::new(CountingConstructor::new(sends.clone()));
        let factory_calls = Arc::new(AtomicUsize::new(0));
        let expected_setup = setup.clone();
        let runtime = build_live_runtime(
            &fixture.workspace,
            fixture.store.clone(),
            load_registry(&fixture.workspace.root).unwrap(),
            test_factory(
                environment.clone(),
                constructor.clone(),
                factory_calls.clone(),
            ),
        )
        .unwrap();
        let outcome = runtime.run_live(setup).unwrap();
        let AgentRuntimeOutcome::Failed { run, error, .. } = outcome else {
            panic!("missing credential must seal a failed live run");
        };
        assert!(error.contains("gateway factory failed"));
        assert!(!error.contains("OPENAI_API_KEY"));
        assert_eq!(factory_calls.load(Ordering::SeqCst), 1);
        assert_eq!(environment.base_reads.load(Ordering::SeqCst), 1);
        assert_eq!(environment.key_reads.load(Ordering::SeqCst), 1);
        assert_eq!(constructor.constructions.load(Ordering::SeqCst), 0);
        assert_eq!(sends.load(Ordering::SeqCst), 0);
        let checkpoint = fixture
            .store
            .list_agent_checkpoints(&run.id)
            .unwrap()
            .into_iter()
            .rev()
            .find(|checkpoint| checkpoint.state["kind"] == "agent_runtime_v2")
            .unwrap();
        let resolved = checkpoint.state["runtime"]["policy_evidence"]["resolved_bindings"].clone();
        assert_eq!(resolved["run_id"], json!(run.id));
        assert_eq!(resolved["agent_id"], json!(fixture.agent.id));
        assert_eq!(
            resolved["process_epoch_id"],
            json!(expected_setup.process_epoch_id)
        );
        assert_eq!(
            resolved["preflight_evidence_digest"],
            json!(expected_setup.preflight_evidence_digest)
        );
        assert_eq!(
            resolved["agent_principal_id"],
            json!(expected_setup.policy.binding_inputs.agent_principal_id)
        );
        assert_eq!(
            resolved["approver_principal_id"],
            json!(expected_setup.policy.binding_inputs.approver_principal_id)
        );
        assert_eq!(resolved["delegation_id"], json!(fixture.delegation.id));
        assert_eq!(
            resolved["delegation_digest"],
            json!(expected_setup.policy.binding_inputs.delegation_digest)
        );
        assert_eq!(
            resolved["edition_id"],
            json!(expected_setup.policy.binding_inputs.edition_id)
        );
        assert_eq!(
            resolved["manifest_digest"],
            json!(expected_setup.policy.binding_inputs.manifest_digest)
        );
        assert_eq!(
            resolved["idempotency_key"],
            json!(expected_setup.policy.binding_inputs.idempotency_key)
        );
        assert_eq!(
            resolved["version_label"],
            json!(expected_setup.policy.binding_inputs.version_label)
        );
        assert_eq!(
            checkpoint.state["runtime"]["policy_binding"]["bindings_digest"],
            json!(independent_wrapped_digest(
                "proof-release-manager-live-bindings-digest/v1",
                "bindings",
                &resolved
            ))
        );
        let policy_binding = &checkpoint.state["runtime"]["policy_binding"];
        assert_eq!(
            policy_binding["preflight_evidence_digest"],
            json!(expected_setup.preflight_evidence_digest)
        );
        assert_eq!(
            policy_binding["template_policy_digest"],
            json!(expected_setup.policy.template_policy_digest)
        );
        assert_eq!(
            policy_binding["check_set_digest"],
            json!(expected_setup.policy.check_set_digest)
        );
        assert_eq!(
            policy_binding["tamper_vector_set_digest"],
            json!(expected_setup.policy.tamper_vector_set_digest)
        );
        assert_eq!(
            policy_binding["pricing_schedule_digest"],
            json!(expected_setup.policy.pricing_schedule_digest)
        );
        assert_eq!(
            policy_binding["instructions_digest"],
            json!(expected_setup.policy.instructions_digest)
        );
        assert_eq!(
            policy_binding["initial_input_digest"],
            json!(expected_setup.policy.initial_input_digest)
        );
        assert_eq!(
            policy_binding["parameters_schema_digest"],
            json!(expected_setup.policy.parameters_schema_digest)
        );
        assert_eq!(
            policy_binding["tool_declaration_digest"],
            json!(expected_setup.policy.tool_declaration_digest)
        );
        assert_eq!(
            policy_binding["tool_set_digest"],
            json!(expected_setup.policy.tool_set_digest)
        );
        assert_eq!(
            policy_binding["resolved_policy_digest"],
            json!(independent_value_digest(
                &checkpoint.state["runtime"]["policy_evidence"]["resolved_policy"]
            ))
        );
    }

    #[test]
    fn factory_rejects_base_url_before_key_read_and_redacts_values() {
        let environment = Arc::new(CountingEnvironment {
            base_url: Some(OsString::from("https://secret-proxy.invalid")),
            api_key: Some("super-secret-key".to_string()),
            base_reads: AtomicUsize::new(0),
            key_reads: AtomicUsize::new(0),
        });
        let sends = Arc::new(AtomicUsize::new(0));
        let constructor = Arc::new(CountingConstructor::new(sends.clone()));
        let calls = Arc::new(AtomicUsize::new(0));
        let factory = CliOpenAiGatewayFactory::with_test_seams(
            environment.clone(),
            constructor.clone(),
            calls.clone(),
        );
        let error = match factory.create(&ModelGatewayFactoryContext {
            run_id: Uuid::now_v7(),
            attempt_id: Uuid::now_v7(),
            process_epoch_id: Uuid::now_v7(),
            provider: LIVE_PROVIDER.to_string(),
            endpoint: LIVE_ENDPOINT.to_string(),
            requested_model: LIVE_MODEL.to_string(),
            service_tier: LIVE_SERVICE_TIER.to_string(),
            request_body_digest: ContentDigest::from_bytes([1; 32]),
        }) {
            Ok(_) => panic!("base URL override must be rejected"),
            Err(error) => error,
        };
        let text = error.to_string();
        assert!(text.contains("OPENAI_BASE_URL must be unset"));
        assert!(!text.contains("secret-proxy"));
        assert!(!text.contains("super-secret-key"));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(environment.base_reads.load(Ordering::SeqCst), 1);
        assert_eq!(environment.key_reads.load(Ordering::SeqCst), 0);
        assert_eq!(constructor.constructions.load(Ordering::SeqCst), 0);
        assert_eq!(sends.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn resume_reloads_exact_evidence_and_replays_sealed_failure_read_only() {
        let fixture = live_fixture();
        let setup = start_setup(
            &fixture.workspace,
            &fixture.store,
            fixture.agent.id,
            &fixture.goal,
            &fixture.policy_path,
            fixture.preflight_evaluation.id,
            fixture.delegation.id,
        )
        .unwrap();
        let first_environment = Arc::new(CountingEnvironment::missing_key());
        let first_sends = Arc::new(AtomicUsize::new(0));
        let first_constructor = Arc::new(CountingConstructor::new(first_sends));
        let first_calls = Arc::new(AtomicUsize::new(0));
        let runtime = build_live_runtime(
            &fixture.workspace,
            fixture.store.clone(),
            load_registry(&fixture.workspace.root).unwrap(),
            test_factory(first_environment, first_constructor, first_calls.clone()),
        )
        .unwrap();
        let AgentRuntimeOutcome::Failed {
            run,
            error: original_error,
            evaluation: original_evaluation,
        } = runtime.run_live(setup).unwrap()
        else {
            panic!("missing credential must produce the terminal fixture");
        };
        assert_eq!(first_calls.load(Ordering::SeqCst), 1);

        let checkpoints_before = fixture.store.list_agent_checkpoints(&run.id).unwrap();
        let events_before = fixture.store.list_agent_run_events(&run.id).unwrap();
        let evaluations_before = fixture.store.list_agent_run_evaluations(&run.id).unwrap();
        let stored_process_epoch = checkpoints_before
            .iter()
            .rev()
            .find(|checkpoint| checkpoint.state["kind"] == "agent_runtime_v2")
            .unwrap()
            .state["runtime"]["process_epoch_id"]
            .clone();
        let complete_trace_evidence = original_evaluation.metrics["complete_trace_digest"].clone();
        assert!(!complete_trace_evidence.is_null());

        let resume = resume_setup(
            &fixture.workspace,
            &fixture.store,
            run.id,
            &fixture.policy_path,
        )
        .unwrap();
        assert!(matches!(resume.intent, LiveRunIntent::Resume { run_id } if run_id == run.id));
        assert_eq!(
            resume.authority.delegation.id, fixture.delegation.id,
            "resume must reload the originally sealed delegation ID"
        );
        assert_ne!(json!(resume.process_epoch_id), stored_process_epoch);
        let environment = Arc::new(CountingEnvironment::missing_key());
        let sends = Arc::new(AtomicUsize::new(0));
        let constructor = Arc::new(CountingConstructor::new(sends.clone()));
        let calls = Arc::new(AtomicUsize::new(0));
        let runtime = build_live_runtime(
            &fixture.workspace,
            fixture.store.clone(),
            load_registry(&fixture.workspace.root).unwrap(),
            test_factory(environment.clone(), constructor.clone(), calls.clone()),
        )
        .unwrap();
        let AgentRuntimeOutcome::Failed {
            run: replayed_run,
            error: replayed_error,
            evaluation: replayed_evaluation,
        } = runtime.run_live(resume).unwrap()
        else {
            panic!("sealed failure must replay its exact terminal outcome");
        };
        assert_eq!(replayed_run, run);
        assert_eq!(replayed_error, original_error);
        assert_eq!(replayed_evaluation, original_evaluation);

        let checkpoints_after = fixture.store.list_agent_checkpoints(&run.id).unwrap();
        let events_after = fixture.store.list_agent_run_events(&run.id).unwrap();
        let evaluations_after = fixture.store.list_agent_run_evaluations(&run.id).unwrap();
        assert_eq!(checkpoints_after.len(), checkpoints_before.len());
        assert_eq!(events_after.len(), events_before.len());
        assert_eq!(evaluations_after.len(), evaluations_before.len());
        assert_eq!(checkpoints_after, checkpoints_before);
        assert_eq!(events_after, events_before);
        assert_eq!(evaluations_after, evaluations_before);
        assert_eq!(
            checkpoints_after
                .iter()
                .rev()
                .find(|checkpoint| checkpoint.state["kind"] == "agent_runtime_v2")
                .unwrap()
                .state["runtime"]["process_epoch_id"],
            stored_process_epoch
        );
        assert_eq!(
            replayed_evaluation.metrics["complete_trace_digest"],
            complete_trace_evidence
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(environment.base_reads.load(Ordering::SeqCst), 0);
        assert_eq!(environment.key_reads.load(Ordering::SeqCst), 0);
        assert_eq!(constructor.constructions.load(Ordering::SeqCst), 0);
        assert_eq!(sends.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn sqlite_success_seals_before_completed_event_and_replays_read_only() {
        let fixture = live_fixture();
        let registry_source =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../registry/content");
        let registry_target = fixture.workspace.root.join(".proof/registry/content");
        std::fs::create_dir_all(&registry_target).unwrap();
        for file in [
            "release-publish-v2.json",
            "release-publish-v2.input.json",
            "release-publish-v2.output.json",
        ] {
            std::fs::copy(registry_source.join(file), registry_target.join(file)).unwrap();
        }
        let parsed = parse_live_goal(&fixture.goal).unwrap();
        let template: Value = serde_json::from_str(LIVE_POLICY_SOURCE).unwrap();
        let tool_name = template["tool"]["declaration"]["name"]
            .as_str()
            .unwrap()
            .to_string();
        let tool_arguments = json!({
            "idempotency_key": parsed.idempotency_key,
            "edition_id": parsed.edition_id,
            "environment": "preview",
            "version_label": parsed.version_label,
            "manifest_digest": parsed.manifest_digest,
        });
        let scripted_sends = Arc::new(AtomicUsize::new(0));
        let scripted_creates = Arc::new(AtomicUsize::new(0));
        let scripted_factory = Arc::new(ScriptedLiveFactory {
            creates: scripted_creates.clone(),
            gateway: Arc::new(ScriptedLiveGateway {
                actions: Mutex::new(
                    vec![
                        ScriptedLiveAction::Tool {
                            name: tool_name,
                            arguments: tool_arguments,
                        },
                        ScriptedLiveAction::FinishFromToolOutput,
                    ]
                    .into(),
                ),
                sends: scripted_sends.clone(),
            }),
        });
        let start = start_setup(
            &fixture.workspace,
            &fixture.store,
            fixture.agent.id,
            &fixture.goal,
            &fixture.policy_path,
            fixture.preflight_evaluation.id,
            fixture.delegation.id,
        )
        .unwrap();
        let runtime = build_live_runtime(
            &fixture.workspace,
            fixture.store.clone(),
            load_registry(&fixture.workspace.root).unwrap(),
            scripted_factory.clone(),
        )
        .unwrap();
        let AgentRuntimeOutcome::WaitingForApproval { run, request, .. } =
            runtime.run_live(start).unwrap()
        else {
            panic!("exact tool call must wait for signed approval");
        };
        assert_eq!(scripted_creates.load(Ordering::SeqCst), 1);
        assert_eq!(scripted_sends.load(Ordering::SeqCst), 1);

        let approver = sole_live_approver(&fixture.workspace, &fixture.store).unwrap();
        let approval_cli = Cli::parse_from([
            "proof",
            "-w",
            fixture.workspace.root.to_str().unwrap(),
            "approval",
            "list",
        ]);
        crate::commands::approval::cmd_approval_approve(
            &approval_cli,
            &request.body.id.to_string(),
            &approver.to_string(),
            Some("approved synthetic preview"),
        )
        .unwrap();

        let resume = resume_setup(
            &fixture.workspace,
            &fixture.store,
            run.id,
            &fixture.policy_path,
        )
        .unwrap();
        let runtime = build_live_runtime(
            &fixture.workspace,
            fixture.store.clone(),
            load_registry(&fixture.workspace.root).unwrap(),
            scripted_factory,
        )
        .unwrap();
        let AgentRuntimeOutcome::Completed {
            run: completed_run,
            output: completed_output,
            evaluation: completed_evaluation,
        } = runtime.run_live(resume).unwrap()
        else {
            panic!("approved exact tool call must complete");
        };
        assert_eq!(scripted_creates.load(Ordering::SeqCst), 2);
        assert_eq!(scripted_sends.load(Ordering::SeqCst), 2);
        assert_eq!(completed_run.status, AgentRunStatus::Succeeded);
        assert_eq!(
            completed_evaluation.evaluator,
            "proof-release-manager-live/v1"
        );
        assert_eq!(completed_evaluation.outcome, AgentEvaluationOutcome::Passed);
        assert_eq!(completed_evaluation.score_bps, Some(10_000));
        assert_eq!(completed_evaluation.metrics["passed_checks"], 17);
        assert_eq!(completed_evaluation.metrics["total_checks"], 17);
        assert_eq!(
            fixture.store.load_agent_run(&run.id).unwrap().unwrap(),
            completed_run
        );

        let checkpoints_before = fixture.store.list_agent_checkpoints(&run.id).unwrap();
        let events_before = fixture.store.list_agent_run_events(&run.id).unwrap();
        let evaluations_before = fixture.store.list_agent_run_evaluations(&run.id).unwrap();
        assert_eq!(
            events_before
                .iter()
                .filter(|event| event.kind == AgentRunEventKind::Completed)
                .count(),
            1
        );
        let stored_process_epoch = checkpoints_before
            .iter()
            .rev()
            .find(|checkpoint| checkpoint.state["kind"] == "agent_runtime_v2")
            .unwrap()
            .state["runtime"]["process_epoch_id"]
            .clone();
        let complete_trace_evidence = completed_evaluation.metrics["complete_trace_digest"].clone();
        assert!(!complete_trace_evidence.is_null());

        let replay = resume_setup(
            &fixture.workspace,
            &fixture.store,
            run.id,
            &fixture.policy_path,
        )
        .unwrap();
        assert_ne!(json!(replay.process_epoch_id), stored_process_epoch);
        let environment = Arc::new(CountingEnvironment::missing_key());
        let sends = Arc::new(AtomicUsize::new(0));
        let constructor = Arc::new(CountingConstructor::new(sends.clone()));
        let calls = Arc::new(AtomicUsize::new(0));
        let runtime = build_live_runtime(
            &fixture.workspace,
            fixture.store.clone(),
            load_registry(&fixture.workspace.root).unwrap(),
            test_factory(environment.clone(), constructor.clone(), calls.clone()),
        )
        .unwrap();
        let AgentRuntimeOutcome::Completed {
            run: replayed_run,
            output: replayed_output,
            evaluation: replayed_evaluation,
        } = runtime.run_live(replay).unwrap()
        else {
            panic!("sealed success must replay its exact terminal outcome");
        };
        assert_eq!(replayed_run, completed_run);
        assert_eq!(replayed_output, completed_output);
        assert_eq!(replayed_evaluation, completed_evaluation);

        let checkpoints_after = fixture.store.list_agent_checkpoints(&run.id).unwrap();
        let events_after = fixture.store.list_agent_run_events(&run.id).unwrap();
        let evaluations_after = fixture.store.list_agent_run_evaluations(&run.id).unwrap();
        assert_eq!(checkpoints_after.len(), checkpoints_before.len());
        assert_eq!(events_after.len(), events_before.len());
        assert_eq!(evaluations_after.len(), evaluations_before.len());
        assert_eq!(checkpoints_after, checkpoints_before);
        assert_eq!(events_after, events_before);
        assert_eq!(evaluations_after, evaluations_before);
        assert_eq!(
            checkpoints_after
                .iter()
                .rev()
                .find(|checkpoint| checkpoint.state["kind"] == "agent_runtime_v2")
                .unwrap()
                .state["runtime"]["process_epoch_id"],
            stored_process_epoch
        );
        assert_eq!(
            replayed_evaluation.metrics["complete_trace_digest"],
            complete_trace_evidence
        );
        assert_no_provider_activity(&environment, &constructor, &calls, &sends);
    }

    #[test]
    fn authority_loader_rejects_every_nonexact_live_scope() {
        let fixture = live_fixture();
        let environment = Arc::new(CountingEnvironment::missing_key());
        let sends = Arc::new(AtomicUsize::new(0));
        let constructor = Arc::new(CountingConstructor::new(sends.clone()));
        let calls = Arc::new(AtomicUsize::new(0));
        let _registered_runtime = build_live_runtime(
            &fixture.workspace,
            fixture.store.clone(),
            load_registry(&fixture.workspace.root).unwrap(),
            test_factory(environment.clone(), constructor.clone(), calls.clone()),
        )
        .unwrap();
        let mut invalid = Vec::new();

        let mut revoked = fixture.delegation.clone();
        revoked.id = Uuid::now_v7();
        revoked.revoked = true;
        invalid.push(revoked);

        let mut expired = fixture.delegation.clone();
        expired.id = Uuid::now_v7();
        expired.valid_until = Utc::now() - Duration::seconds(1);
        invalid.push(expired);

        let mut not_yet_valid = fixture.delegation.clone();
        not_yet_valid.id = Uuid::now_v7();
        not_yet_valid.valid_from = Utc::now() + Duration::seconds(1);
        invalid.push(not_yet_valid);

        let mut shorter_than_deadline = fixture.delegation.clone();
        shorter_than_deadline.id = Uuid::now_v7();
        shorter_than_deadline.valid_until = Utc::now() + Duration::seconds(299);
        invalid.push(shorter_than_deadline);

        let mut recipient = fixture.delegation.clone();
        recipient.id = Uuid::now_v7();
        recipient.recipient = sole_live_approver(&fixture.workspace, &fixture.store).unwrap();
        invalid.push(recipient);

        let mut unbounded = fixture.delegation.clone();
        unbounded.id = Uuid::now_v7();
        unbounded.scope = Default::default();
        invalid.push(unbounded);

        let mut structured_resource = fixture.delegation.clone();
        structured_resource.id = Uuid::now_v7();
        structured_resource.scope.resource_scope = Some("preview/*".to_string());
        invalid.push(structured_resource);

        let mut additional_operation = fixture.delegation.clone();
        additional_operation.id = Uuid::now_v7();
        additional_operation
            .scope
            .allowed_operations
            .as_mut()
            .unwrap()
            .push("release.delete".to_string());
        invalid.push(additional_operation);

        let mut wildcard = fixture.delegation.clone();
        wildcard.id = Uuid::now_v7();
        wildcard.scope.allowed_operations = Some(vec!["release.*".to_string()]);
        invalid.push(wildcard);

        let mut wrong_operation = fixture.delegation.clone();
        wrong_operation.id = Uuid::now_v7();
        wrong_operation.scope.allowed_operations = Some(vec!["schema.create".to_string()]);
        invalid.push(wrong_operation);

        let mut additional_domain = fixture.delegation.clone();
        additional_domain.id = Uuid::now_v7();
        additional_domain
            .scope
            .allowed_domains
            .as_mut()
            .unwrap()
            .push("commerce".to_string());
        invalid.push(additional_domain);

        let mut wrong_domain = fixture.delegation.clone();
        wrong_domain.id = Uuid::now_v7();
        wrong_domain.scope.allowed_domains = Some(vec!["commerce".to_string()]);
        invalid.push(wrong_domain);

        for delegation in invalid {
            fixture.store.save_delegation(&delegation).unwrap();
            let error = start_setup(
                &fixture.workspace,
                &fixture.store,
                fixture.agent.id,
                &fixture.goal,
                &fixture.policy_path,
                fixture.preflight_evaluation.id,
                delegation.id,
            )
            .unwrap_err();
            assert!(error.to_string().contains("exact active singleton"));
        }
        let missing = start_setup(
            &fixture.workspace,
            &fixture.store,
            fixture.agent.id,
            &fixture.goal,
            &fixture.policy_path,
            fixture.preflight_evaluation.id,
            Uuid::now_v7(),
        )
        .unwrap_err();
        assert!(missing.to_string().contains("delegation not found"));
        assert_no_provider_activity(&environment, &constructor, &calls, &sends);
    }

    #[test]
    fn malformed_policy_preflight_and_checkpoint_fail_locally() {
        let fixture = live_fixture();
        let environment = Arc::new(CountingEnvironment::missing_key());
        let sends = Arc::new(AtomicUsize::new(0));
        let constructor = Arc::new(CountingConstructor::new(sends.clone()));
        let calls = Arc::new(AtomicUsize::new(0));
        let _registered_runtime = build_live_runtime(
            &fixture.workspace,
            fixture.store.clone(),
            load_registry(&fixture.workspace.root).unwrap(),
            test_factory(environment.clone(), constructor.clone(), calls.clone()),
        )
        .unwrap();
        let missing_preflight = start_setup(
            &fixture.workspace,
            &fixture.store,
            fixture.agent.id,
            &fixture.goal,
            &fixture.policy_path,
            Uuid::now_v7(),
            fixture.delegation.id,
        )
        .unwrap_err();
        assert!(missing_preflight
            .to_string()
            .contains("preflight evaluation not found"));

        let mut tampered: Value = serde_json::from_str(LIVE_POLICY_SOURCE).unwrap();
        tampered["checks"].as_array_mut().unwrap().swap(0, 1);
        let tampered_path = fixture.workspace.root.join("tampered-live-policy.json");
        std::fs::write(&tampered_path, serde_json::to_vec(&tampered).unwrap()).unwrap();
        let policy_error = start_setup(
            &fixture.workspace,
            &fixture.store,
            fixture.agent.id,
            &fixture.goal,
            &tampered_path,
            fixture.preflight_evaluation.id,
            fixture.delegation.id,
        )
        .unwrap_err();
        assert!(policy_error.to_string().contains("differs from the frozen"));

        let checkpoint_error = resume_setup(
            &fixture.workspace,
            &fixture.store,
            fixture.preflight_evaluation.run_id,
            &fixture.policy_path,
        )
        .unwrap_err();
        assert!(checkpoint_error
            .to_string()
            .contains("has no sealed live checkpoint"));
        assert_no_provider_activity(&environment, &constructor, &calls, &sends);
    }

    #[test]
    fn malformed_persisted_resume_bindings_fail_with_registered_factory_untouched() {
        let fixture = live_fixture();
        let setup = start_setup(
            &fixture.workspace,
            &fixture.store,
            fixture.agent.id,
            &fixture.goal,
            &fixture.policy_path,
            fixture.preflight_evaluation.id,
            fixture.delegation.id,
        )
        .unwrap();
        let initial_environment = Arc::new(CountingEnvironment::missing_key());
        let initial_sends = Arc::new(AtomicUsize::new(0));
        let initial_constructor = Arc::new(CountingConstructor::new(initial_sends));
        let initial_calls = Arc::new(AtomicUsize::new(0));
        let runtime = build_live_runtime(
            &fixture.workspace,
            fixture.store.clone(),
            load_registry(&fixture.workspace.root).unwrap(),
            test_factory(initial_environment, initial_constructor, initial_calls),
        )
        .unwrap();
        let AgentRuntimeOutcome::Failed { run, .. } = runtime.run_live(setup).unwrap() else {
            panic!("missing credential must create a sealed live checkpoint");
        };
        let mut checkpoint = fixture
            .store
            .list_agent_checkpoints(&run.id)
            .unwrap()
            .into_iter()
            .rev()
            .find(|checkpoint| checkpoint.state["kind"] == "agent_runtime_v2")
            .unwrap();
        checkpoint.state["runtime"]["policy_evidence"]["resolved_bindings"]["unknown"] =
            json!(true);
        fixture
            .store
            .connection()
            .execute(
                "UPDATE agent_checkpoints SET checkpoint_json = ?1 WHERE id = ?2",
                rusqlite::params![
                    serde_json::to_string(&checkpoint).unwrap(),
                    checkpoint.id.to_string()
                ],
            )
            .unwrap();

        let environment = Arc::new(CountingEnvironment::missing_key());
        let sends = Arc::new(AtomicUsize::new(0));
        let constructor = Arc::new(CountingConstructor::new(sends.clone()));
        let calls = Arc::new(AtomicUsize::new(0));
        let _registered_runtime = build_live_runtime(
            &fixture.workspace,
            fixture.store.clone(),
            load_registry(&fixture.workspace.root).unwrap(),
            test_factory(environment.clone(), constructor.clone(), calls.clone()),
        )
        .unwrap();
        let error = resume_setup(
            &fixture.workspace,
            &fixture.store,
            run.id,
            &fixture.policy_path,
        )
        .unwrap_err();
        assert!(error
            .to_string()
            .contains("resolved bindings are malformed"));
        assert_no_provider_activity(&environment, &constructor, &calls, &sends);
    }

    fn assert_no_provider_activity(
        environment: &CountingEnvironment,
        constructor: &CountingConstructor,
        calls: &AtomicUsize,
        sends: &AtomicUsize,
    ) {
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(environment.base_reads.load(Ordering::SeqCst), 0);
        assert_eq!(environment.key_reads.load(Ordering::SeqCst), 0);
        assert_eq!(constructor.constructions.load(Ordering::SeqCst), 0);
        assert_eq!(sends.load(Ordering::SeqCst), 0);
    }

    fn independent_value_digest(value: &Value) -> ContentDigest {
        digest(ArtifactKind::Generic, &canonicalize(value).unwrap())
    }

    fn independent_wrapped_digest(schema: &str, field: &str, value: &Value) -> ContentDigest {
        let mut wrapper = serde_json::Map::new();
        wrapper.insert("schema".to_string(), Value::String(schema.to_string()));
        wrapper.insert(field.to_string(), value.clone());
        independent_value_digest(&Value::Object(wrapper))
    }
}
