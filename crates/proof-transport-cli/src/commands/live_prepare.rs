//! Credential-free deterministic prerequisite materialization for E0001.

use std::path::Path;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use proof_agent_runtime::{
    AgentRuntime, AgentRuntimeOutcome, ModelDecision, ModelGateway, ModelGatewayError,
    ModelGatewayFactory, ModelGatewayFactoryContext, ModelGatewayFactoryError, ModelInput,
    ModelTurn, ModelTurnRequest, ModelUsage,
};
use proof_kernel::{
    canonicalize, canonicalize_serialized, digest, AgentDefinition, AgentEvaluationOutcome,
    AgentLimits, AgentRunEvaluation, AgentRunStatus, AgentTool, ApprovalOutcome, ArtifactKind,
    ContentDigest, ExecutionEngine, Governance, PrincipalId, Registry, RegistryEntry,
};
use proof_storage::SqliteStore;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use super::secure_fs::{open_descendant, open_trusted_absolute_directory, SecureDirectory};
use crate::{load_registry, Cli, Workspace};

const PREP_SCHEMA: &str = "proof-release-manager-live-preparation/v1";
const PREP_PROVIDER: &str = "proof-scripted-preflight";
const PREP_MODEL: &str = "proof-scripted-preflight-v1";
const PREP_INSTRUCTIONS: &str =
    "Call the one deterministic release tool exactly once, then report its durable references.";
const PREP_RUNTIME_INSTRUCTIONS: &str = "Call the one deterministic release tool exactly once, then report its durable references.\n\nUse only the supplied Proof tools. Treat tool errors as authoritative. Stop when the goal is complete.";
const PREP_VERSION_LABEL: &str = "2026.08.29-rc1";
const LIVE_VERSION_LABEL: &str = "2026.08.30-rc1";
const PREP_TOOL_NAME: &str = "proof_content_v1_release_publish";
const PREP_CALL_ID: &str = "live_prepare_release_call";
const PREP_TOOL_RESPONSE_ID: &str = "live_prepare_tool_response";
const PREP_FINISH_RESPONSE_ID: &str = "live_prepare_finish_response";
const TRACE_EVALUATOR: &str = "proof-agent-trace/v1";

const PREP_CHECK_IDS: [&str; 10] = [
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct InitializedRecord {
    schema: String,
    preparation_id: Uuid,
    edition: proof_content::Edition,
    idempotency_key: Uuid,
    deterministic_agent: AgentDefinition,
    agent_principal_id: PrincipalId,
    approver_principal_id: PrincipalId,
    goal: String,
    binding_digest: ContentDigest,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct DispatchRecord {
    schema: String,
    preparation_id: Uuid,
    initialized_digest: ContentDigest,
    binding_digest: ContentDigest,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct AwaitingRecord {
    schema: String,
    preparation_id: Uuid,
    initialized_digest: ContentDigest,
    run_id: Uuid,
    step_id: Uuid,
    request_id: Uuid,
    request_digest: ContentDigest,
    binding_digest: ContentDigest,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct EvaluatedRecord {
    schema: String,
    preparation_id: Uuid,
    awaiting_digest: ContentDigest,
    evaluation_id: Uuid,
    evaluation_digest: ContentDigest,
    binding_digest: ContentDigest,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct MaterializationRecord {
    schema: String,
    preparation_id: Uuid,
    evaluated_digest: ContentDigest,
    edition_id: Uuid,
    edition_bytes_digest: ContentDigest,
    relative_path: String,
    binding_digest: ContentDigest,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct ReadyRecord {
    schema: String,
    preparation_id: Uuid,
    evaluated_digest: ContentDigest,
    materialization_digest: ContentDigest,
    packet: ReadinessPacket,
    binding_digest: ContentDigest,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct ReadinessPacket {
    schema: String,
    preparation_id: Uuid,
    checked_at: DateTime<Utc>,
    preflight: PreflightPacket,
    live_policy: LivePolicyPacket,
    bindings: ReadinessBindings,
    next_argv: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct PreflightPacket {
    run_id: Uuid,
    evaluation_id: Uuid,
    evaluation_digest: ContentDigest,
    evidence: Value,
    evidence_digest: ContentDigest,
    policy_digest: ContentDigest,
    trace_digest: ContentDigest,
    score_bps: u16,
    passed_checks: u16,
    total_checks: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct LivePolicyPacket {
    template_policy_digest: ContentDigest,
    check_set_digest: ContentDigest,
    tamper_vector_set_digest: ContentDigest,
    pricing_schedule_digest: ContentDigest,
    instructions_digest: ContentDigest,
    initial_input_digest: ContentDigest,
    parameters_schema_digest: ContentDigest,
    tool_declaration_digest: ContentDigest,
    tool_set_digest: ContentDigest,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ReadinessBindings {
    preflight_evidence_digest: ContentDigest,
    agent_id: Uuid,
    agent_principal_id: PrincipalId,
    approver_principal_id: PrincipalId,
    delegation_id: Uuid,
    delegation_digest: ContentDigest,
    edition_id: Uuid,
    manifest_digest: String,
    idempotency_key: Uuid,
    version_label: String,
    goal: String,
}

struct DeterministicGateway {
    goal: String,
}

struct CheckOnlyGatewayFactory;

struct PreparationWorkspaceAccess {
    root_path: std::path::PathBuf,
    _root: SecureDirectory,
    proof: SecureDirectory,
    storage: SecureDirectory,
}

impl PreparationWorkspaceAccess {
    fn open(path: &Path) -> Result<Self> {
        let (root, root_path) = open_trusted_absolute_directory(path)?;
        root.validate_private_current_user("root")?;
        let proof = root.open_child(".proof")?;
        proof.validate_private_current_user(".proof directory")?;
        let storage = proof.open_child("storage")?;
        storage.validate_private_current_user("storage directory")?;
        Ok(Self {
            root_path,
            _root: root,
            proof,
            storage,
        })
    }

    fn open_workspace(&self) -> Result<Workspace> {
        Workspace::open_from_secure_directory(self.root_path.clone(), &self.proof)
    }

    fn open_store(&self) -> Result<Arc<SqliteStore>> {
        let expected_storage = self.root_path.join(".proof/storage");
        let store = SqliteStore::open_existing_nofollow_in_trusted_directory(
            self.storage.try_clone_handle()?,
            &expected_storage,
            "storage.db",
        )?;
        Ok(Arc::new(store))
    }
}

impl ModelGatewayFactory for CheckOnlyGatewayFactory {
    fn create(
        &self,
        _context: &ModelGatewayFactoryContext,
    ) -> std::result::Result<Arc<dyn ModelGateway>, ModelGatewayFactoryError> {
        Err(ModelGatewayFactoryError::Configuration(
            "credential-free readiness validation must never invoke a gateway factory".to_string(),
        ))
    }
}

impl ModelGateway for DeterministicGateway {
    fn provider(&self) -> &str {
        PREP_PROVIDER
    }

    fn complete(
        &self,
        request: &ModelTurnRequest,
    ) -> std::result::Result<ModelTurn, ModelGatewayError> {
        if request.model != PREP_MODEL
            || request.instructions != PREP_RUNTIME_INSTRUCTIONS
            || request.tools.len() != 1
            || request.tools[0].name != PREP_TOOL_NAME
            || request.tools[0].operation != "release.publish"
            || request.tools[0].version != "v1"
            || request.max_output_tokens != 512
        {
            return Err(ModelGatewayError::Terminal(
                "deterministic preparation request binding drift".to_string(),
            ));
        }
        let (response_id, decision) = match &request.input {
            ModelInput::Goal { text }
                if text == &self.goal && request.previous_response_id.is_none() =>
            {
                (
                    PREP_TOOL_RESPONSE_ID,
                    ModelDecision::ToolCall {
                        call_id: PREP_CALL_ID.to_string(),
                        name: PREP_TOOL_NAME.to_string(),
                        arguments: json!({
                            "environment": "preview",
                            "version_label": PREP_VERSION_LABEL,
                        }),
                    },
                )
            }
            ModelInput::ToolOutput { call_id, output }
                if call_id == PREP_CALL_ID
                    && request.previous_response_id.as_deref() == Some(PREP_TOOL_RESPONSE_ID) =>
            {
                let release_id = required_string(output, "/result/data/release/id")?;
                let edition_id = required_string(output, "/result/data/release/edition_id")?;
                let proof_id = required_string(output, "/proof_id")?;
                (
                    PREP_FINISH_RESPONSE_ID,
                    ModelDecision::Finish {
                        output: format!(
                            "Release {release_id} for edition {edition_id} published to preview as {PREP_VERSION_LABEL} with proof {proof_id}."
                        ),
                    },
                )
            }
            _ => {
                return Err(ModelGatewayError::Terminal(
                    "deterministic preparation turn does not match persisted input".to_string(),
                ))
            }
        };
        Ok(ModelTurn {
            response_id: response_id.to_string(),
            returned_model: Some(PREP_MODEL.to_string()),
            response_body_digest: Some(
                generic_digest(&json!({
                    "response_id": response_id,
                    "decision": decision,
                    "usage": ModelUsage::default(),
                }))
                .map_err(|error| ModelGatewayError::InvalidResponse(error.to_string()))?,
            ),
            decision,
            usage: ModelUsage::default(),
        })
    }
}

pub(crate) fn cmd_live_prepare_start(cli: &Cli, preparation_id: &str) -> Result<()> {
    let preparation_id = parse_preparation_id(preparation_id)?;
    let access = PreparationWorkspaceAccess::open(&cli.workspace)?;
    let (directory, preloaded_workspace) =
        if let Some(directory) = existing_preparation_directory(&access.proof, preparation_id)? {
            (directory, None)
        } else {
            let workspace = access.open_workspace()?;
            (
                preparation_directory_from_proof(&access.proof, preparation_id, true)?,
                Some(workspace),
            )
        };
    let _lock = directory.exclusive_lock("phase.lock")?;
    if let Some(initialized) = read_record::<InitializedRecord>(&directory, "initialized.json")? {
        validate_initialized_record(&initialized, preparation_id)?;
        if let Some(awaiting) = read_record::<AwaitingRecord>(&directory, "awaiting.json")? {
            validate_awaiting_chain(&initialized, &awaiting)?;
            return print_start_packet(&access.root_path, &initialized, &awaiting);
        }
    }

    let workspace = match preloaded_workspace {
        Some(workspace) => workspace,
        None => access.open_workspace()?,
    };
    let store = access.open_store()?;
    let initialized = load_or_create_initialized(&directory, &workspace, &store, preparation_id)?;
    ensure_initialized_bindings(&initialized, &workspace, &store, preparation_id)?;
    validate_live_registry(&workspace)?;
    save_exact_agent(&store, &initialized.deterministic_agent)?;

    let dispatch = sealed_record(DispatchRecord {
        schema: format!("{PREP_SCHEMA}/dispatch"),
        preparation_id,
        initialized_digest: initialized.binding_digest,
        binding_digest: ContentDigest::from_bytes([0; 32]),
    })?;
    let first_dispatch = write_record(&directory, "dispatch.json", &dispatch)?;
    let candidates = exact_run_candidates(&store, &initialized)?;
    let outcome = if first_dispatch {
        if !candidates.is_empty() {
            bail!("preparation dispatch found an unexpected pre-existing run");
        }
        build_runtime(&workspace, store.clone(), &initialized)?
            .start(initialized.deterministic_agent.id, initialized.goal.clone())?
    } else {
        match candidates.as_slice() {
            [] => build_runtime(&workspace, store.clone(), &initialized)?
                .start(initialized.deterministic_agent.id, initialized.goal.clone())?,
            [run] => build_runtime(&workspace, store.clone(), &initialized)?.resume(run.id)?,
            _ => bail!(
                "preparation recovery requires at most one saved agent+goal run; found {}",
                candidates.len()
            ),
        }
    };
    let (run, step, request) = match outcome {
        AgentRuntimeOutcome::WaitingForApproval { run, step, request } => (run, step, request),
        AgentRuntimeOutcome::Completed { .. } => {
            bail!("preparation start unexpectedly reached a terminal run")
        }
        AgentRuntimeOutcome::Failed { error, .. } => {
            bail!("deterministic preparation failed before approval: {error}")
        }
        AgentRuntimeOutcome::AlreadyStarted { .. } => {
            bail!("deterministic preparation unexpectedly returned a live start claim")
        }
    };
    let awaiting = sealed_record(AwaitingRecord {
        schema: format!("{PREP_SCHEMA}/awaiting-approval"),
        preparation_id,
        initialized_digest: initialized.binding_digest,
        run_id: run.id,
        step_id: step.id,
        request_id: request.body.id,
        request_digest: request.digest()?,
        binding_digest: ContentDigest::from_bytes([0; 32]),
    })?;
    write_record(&directory, "awaiting.json", &awaiting)?;
    validate_awaiting(&initialized, &awaiting, &store)?;
    print_start_packet(&workspace.root, &initialized, &awaiting)
}

pub(crate) fn cmd_live_prepare_finish(
    cli: &Cli,
    preparation_id: &str,
    agent_id: &str,
    delegation_id: &str,
    policy_file: &Path,
) -> Result<()> {
    finish_with_check_factory(
        cli,
        preparation_id,
        agent_id,
        delegation_id,
        policy_file,
        Arc::new(CheckOnlyGatewayFactory),
    )
}

fn finish_with_check_factory(
    cli: &Cli,
    preparation_id: &str,
    agent_id: &str,
    delegation_id: &str,
    policy_file: &Path,
    check_factory: Arc<dyn ModelGatewayFactory>,
) -> Result<()> {
    let preparation_id = parse_preparation_id(preparation_id)?;
    let agent_id = parse_canonical_uuid(agent_id, "live agent")?;
    let delegation_id = parse_canonical_uuid(delegation_id, "delegation")?;
    let access = PreparationWorkspaceAccess::open(&cli.workspace)?;
    let directory = preparation_directory_from_proof(&access.proof, preparation_id, false)?;
    let _lock = directory.exclusive_lock("phase.lock")?;
    let initialized = read_record::<InitializedRecord>(&directory, "initialized.json")?
        .context("live preparation has not been initialized")?;
    validate_initialized_record(&initialized, preparation_id)?;
    if let Some(ready) = read_record::<ReadyRecord>(&directory, "ready.json")? {
        let awaiting = read_record::<AwaitingRecord>(&directory, "awaiting.json")?
            .context("completed preparation has no awaiting-approval record")?;
        let evaluated = read_record::<EvaluatedRecord>(&directory, "evaluated.json")?
            .context("completed preparation has no evaluated record")?;
        let materialization =
            read_record::<MaterializationRecord>(&directory, "edition-bound.json")?
                .context("completed preparation has no edition binding record")?;
        validate_awaiting_chain(&initialized, &awaiting)?;
        validate_evaluated_chain(&initialized, &awaiting, &evaluated)?;
        validate_materialization_chain(&initialized, &evaluated, &materialization)?;
        validate_ready_chain(
            &access.root_path,
            &initialized,
            &awaiting,
            &evaluated,
            &materialization,
            &ready,
            agent_id,
            delegation_id,
            policy_file,
        )?;
        println!("{}", serde_json::to_string_pretty(&ready.packet)?);
        return Ok(());
    }
    let workspace = access.open_workspace()?;
    let store = access.open_store()?;
    ensure_initialized_bindings(&initialized, &workspace, &store, preparation_id)?;
    let awaiting = read_record::<AwaitingRecord>(&directory, "awaiting.json")?
        .context("live preparation has not reached signed approval")?;
    validate_awaiting(&initialized, &awaiting, &store)?;
    validate_approved_decision(&initialized, &awaiting, &store)?;
    validate_live_registry(&workspace)?;

    let outcome =
        build_runtime(&workspace, store.clone(), &initialized)?.resume(awaiting.run_id)?;
    let run = match outcome {
        AgentRuntimeOutcome::Completed { run, .. } => run,
        AgentRuntimeOutcome::WaitingForApproval { .. } => {
            bail!("live preparation approval is still pending")
        }
        AgentRuntimeOutcome::Failed { error, .. } => {
            bail!("deterministic preparation failed after approval: {error}")
        }
        AgentRuntimeOutcome::AlreadyStarted { .. } => {
            bail!("deterministic preparation unexpectedly returned a live start claim")
        }
    };
    if run.status != AgentRunStatus::Succeeded {
        bail!("deterministic preparation did not seal successfully");
    }
    let evaluation = load_or_append_exact_evaluation(&store, run.id)?;
    let evaluated = sealed_record(EvaluatedRecord {
        schema: format!("{PREP_SCHEMA}/evaluated"),
        preparation_id,
        awaiting_digest: awaiting.binding_digest,
        evaluation_id: evaluation.id,
        evaluation_digest: serialized_digest(&evaluation)?,
        binding_digest: ContentDigest::from_bytes([0; 32]),
    })?;
    write_record(&directory, "evaluated.json", &evaluated)?;

    let edition_bytes = serde_json::to_vec_pretty(&initialized.edition)?;
    let manifest_digest = edition_manifest_digest(&initialized.edition)?;
    let goal = live_goal(
        initialized.edition.id,
        &manifest_digest,
        initialized.idempotency_key,
    );

    // Validate every input that does not depend on the edition leaf before
    // making that sole synthetic filesystem effect.
    super::live::prevalidate_start_without_edition(
        &workspace,
        &store,
        agent_id,
        &goal,
        policy_file,
        evaluation.id,
        delegation_id,
    )?;
    let materialization = bind_materialization(
        &directory,
        &workspace,
        preparation_id,
        &evaluated,
        initialized.edition.id,
        &edition_bytes,
    )?;
    materialize_edition(&workspace, initialized.edition.id, &edition_bytes)?;
    if materialization.edition_bytes_digest != serialized_digest(&initialized.edition)? {
        bail!("synthetic edition materialization digest drift");
    }
    let setup = super::live::start_setup(
        &workspace,
        &store,
        agent_id,
        &goal,
        policy_file,
        evaluation.id,
        delegation_id,
    )?;
    let check_runtime = super::live::build_live_runtime(
        &workspace,
        store.clone(),
        load_registry(&workspace.root)?,
        check_factory,
    )?;
    check_runtime.check_live_start_setup(&setup)?;
    if setup.policy.binding_inputs.approver_principal_id != initialized.approver_principal_id {
        bail!("preparation approver differs from the checked live approver binding");
    }
    let packet = readiness_packet(
        &workspace,
        preparation_id,
        agent_id,
        policy_file,
        &evaluation,
        &evaluated,
        setup,
        goal,
        Utc::now(),
    )?;
    let ready = sealed_record(ReadyRecord {
        schema: format!("{PREP_SCHEMA}/ready"),
        preparation_id,
        evaluated_digest: evaluated.binding_digest,
        materialization_digest: materialization.binding_digest,
        packet,
        binding_digest: ContentDigest::from_bytes([0; 32]),
    })?;
    write_record(&directory, "ready.json", &ready)?;
    let stored = read_record::<ReadyRecord>(&directory, "ready.json")?
        .context("durable readiness record disappeared")?;
    if stored != ready {
        bail!("durable readiness packet binding drift");
    }
    println!("{}", serde_json::to_string_pretty(&stored.packet)?);
    Ok(())
}

fn load_or_create_initialized(
    directory: &SecureDirectory,
    workspace: &Workspace,
    store: &SqliteStore,
    preparation_id: Uuid,
) -> Result<InitializedRecord> {
    if let Some(record) = read_record(directory, "initialized.json")? {
        return Ok(record);
    }
    let approver_principal_id = super::live::sole_live_approver(workspace, store)?;
    let edition = proof_content::Edition::new(Uuid::now_v7(), Vec::new());
    let idempotency_key = Uuid::now_v7();
    let agent = AgentDefinition::new(
        format!("live-preparation-{preparation_id}"),
        PREP_INSTRUCTIONS,
        PREP_PROVIDER,
        PREP_MODEL,
        vec![AgentTool::new("release.publish", "v1")?],
        AgentLimits {
            max_steps: 1,
            max_model_calls: 2,
            max_total_tokens: 1_000,
            max_duration_seconds: 3_600,
            max_output_tokens_per_call: 512,
            max_cost_microusd: None,
        },
        Utc::now(),
    )?;
    let goal = preparation_goal(preparation_id, edition.id, idempotency_key);
    let initialized = sealed_record(InitializedRecord {
        schema: format!("{PREP_SCHEMA}/initialized"),
        preparation_id,
        edition,
        idempotency_key,
        deterministic_agent: agent,
        agent_principal_id: workspace.actor,
        approver_principal_id,
        goal,
        binding_digest: ContentDigest::from_bytes([0; 32]),
    })?;
    write_record(directory, "initialized.json", &initialized)?;
    Ok(initialized)
}

fn ensure_initialized_bindings(
    initialized: &InitializedRecord,
    workspace: &Workspace,
    store: &SqliteStore,
    preparation_id: Uuid,
) -> Result<()> {
    validate_initialized_record(initialized, preparation_id)?;
    if initialized.agent_principal_id != workspace.actor
        || initialized.approver_principal_id != super::live::sole_live_approver(workspace, store)?
    {
        bail!("initialized preparation authority binding drift");
    }
    Ok(())
}

fn validate_initialized_record(
    initialized: &InitializedRecord,
    preparation_id: Uuid,
) -> Result<()> {
    if initialized.schema != format!("{PREP_SCHEMA}/initialized")
        || initialized.preparation_id != preparation_id
        || initialized.preparation_id.get_version_num() != 7
        || initialized.edition.id.get_version_num() != 7
        || initialized.edition.changeset_id.get_version_num() != 7
        || initialized.idempotency_key.get_version_num() != 7
        || initialized.edition.objects.len() != 0
        || initialized.edition.content_digest
            != proof_content::digest::canonical_digest(&Vec::<proof_content::Object>::new())
        || initialized.goal
            != preparation_goal(
                preparation_id,
                initialized.edition.id,
                initialized.idempotency_key,
            )
        || initialized.deterministic_agent.name != format!("live-preparation-{preparation_id}")
        || initialized.deterministic_agent.instructions != PREP_INSTRUCTIONS
        || initialized.deterministic_agent.provider != PREP_PROVIDER
        || initialized.deterministic_agent.model != PREP_MODEL
        || initialized.deterministic_agent.tools != vec![AgentTool::new("release.publish", "v1")?]
        || initialized.deterministic_agent.id.get_version_num() != 7
        || initialized.deterministic_agent.limits
            != (AgentLimits {
                max_steps: 1,
                max_model_calls: 2,
                max_total_tokens: 1_000,
                max_duration_seconds: 3_600,
                max_output_tokens_per_call: 512,
                max_cost_microusd: None,
            })
    {
        bail!("initialized preparation binding drift");
    }
    Ok(())
}

fn save_exact_agent(store: &SqliteStore, agent: &AgentDefinition) -> Result<()> {
    match store.load_agent_definition(&agent.id)? {
        Some(existing) if existing != *agent => bail!("deterministic agent binding drift"),
        Some(_) => Ok(()),
        None => store
            .save_agent_definition(agent)
            .map_err(anyhow::Error::from),
    }
}

fn validate_awaiting(
    initialized: &InitializedRecord,
    awaiting: &AwaitingRecord,
    store: &SqliteStore,
) -> Result<()> {
    validate_awaiting_chain(initialized, awaiting)?;
    let candidates = exact_run_candidates(store, initialized)?;
    if candidates.len() != 1 || candidates[0].id != awaiting.run_id {
        bail!("awaiting-approval run is not the unique exact preparation run");
    }
    let steps = store.list_agent_run_steps(&awaiting.run_id)?;
    if steps.len() != 1
        || steps[0].id != awaiting.step_id
        || steps[0].approval_request_id != Some(awaiting.request_id)
        || steps[0].operation != "release.publish"
        || steps[0].version != "v1"
    {
        bail!("awaiting-approval step binding drift");
    }
    let request = store
        .load_approval_request(&awaiting.request_id)?
        .context("preparation approval request is missing")?;
    if request.digest()? != awaiting.request_digest
        || request.body.operation != "release.publish"
        || request.body.version != "v1"
        || request.body.requested_by != initialized.agent_principal_id
    {
        bail!("preparation approval request binding drift");
    }
    Ok(())
}

fn validate_awaiting_chain(
    initialized: &InitializedRecord,
    awaiting: &AwaitingRecord,
) -> Result<()> {
    if awaiting.schema != format!("{PREP_SCHEMA}/awaiting-approval")
        || awaiting.preparation_id != initialized.preparation_id
        || awaiting.initialized_digest != initialized.binding_digest
        || awaiting.run_id.get_version_num() != 7
        || awaiting.step_id.get_version_num() != 7
        || awaiting.request_id.get_version_num() != 7
    {
        bail!("awaiting-approval preparation binding drift");
    }
    Ok(())
}

fn validate_approved_decision(
    initialized: &InitializedRecord,
    awaiting: &AwaitingRecord,
    store: &SqliteStore,
) -> Result<()> {
    let request = store
        .load_approval_request(&awaiting.request_id)?
        .context("preparation approval request is missing")?;
    let decision = store
        .load_approval_decision(&awaiting.request_id)?
        .context("preparation approval has not been signed")?;
    let approver = store
        .load_principal(&initialized.approver_principal_id)
        .context("preparation approver is not enrolled")?;
    request.verify(
        &store
            .load_principal(&request.body.requested_by)
            .context("preparation requester is not enrolled")?,
    )?;
    decision.verify(&approver)?;
    if decision.body.outcome != ApprovalOutcome::Approved
        || decision.body.request_id != request.body.id
        || decision.body.request_digest != awaiting.request_digest
        || decision.body.decided_by != initialized.approver_principal_id
    {
        bail!("preparation requires the exact trusted Human approval");
    }
    Ok(())
}

fn exact_run_candidates(
    store: &SqliteStore,
    initialized: &InitializedRecord,
) -> Result<Vec<proof_kernel::AgentRun>> {
    Ok(store
        .list_agent_runs()?
        .into_iter()
        .filter(|run| {
            run.agent_id == Some(initialized.deterministic_agent.id)
                && run.actor == initialized.agent_principal_id
                && run.goal == initialized.goal
        })
        .collect())
}

fn build_runtime(
    workspace: &Workspace,
    store: Arc<SqliteStore>,
    initialized: &InitializedRecord,
) -> Result<AgentRuntime> {
    let registry = preparation_registry()?;
    let mut engine = ExecutionEngine::new_with_keypair(registry.clone(), workspace.keypair.clone())
        .with_storage(store.clone());
    for handler in proof_content::content_handlers() {
        engine.register_handler(handler);
    }
    AgentRuntime::new(
        registry,
        engine,
        workspace.keypair.clone(),
        workspace.root.clone(),
        store.clone(),
        store.clone(),
        store,
        Arc::new(DeterministicGateway {
            goal: initialized.goal.clone(),
        }),
    )
    .map_err(anyhow::Error::from)
}

fn preparation_registry() -> Result<Registry> {
    let mut entry: RegistryEntry = serde_json::from_str(include_str!(
        "../../../../registry/content/release-publish.json"
    ))?;
    entry.input_schema =
        include_str!("../../../../registry/content/release-publish.input.json").to_string();
    entry.output_schema =
        include_str!("../../../../registry/content/release-publish.output.json").to_string();
    if entry.operation != "release.publish"
        || entry.version != "v1"
        || entry.governance != Governance::HumanOnly
    {
        bail!("embedded deterministic preparation registry entry drift");
    }
    Registry::new(vec![entry]).map_err(anyhow::Error::from)
}

#[cfg(test)]
fn ensure_preparation_registry(workspace: &Workspace) -> Result<()> {
    let proof = open_descendant(&workspace.root, &[".proof"])?;
    let registry = proof.open_child("registry")?;
    let content = registry.ensure_child("content")?;
    for (name, bytes) in [
        (
            "release-publish.json",
            include_bytes!("../../../../registry/content/release-publish.json").as_slice(),
        ),
        (
            "release-publish.input.json",
            include_bytes!("../../../../registry/content/release-publish.input.json").as_slice(),
        ),
        (
            "release-publish.output.json",
            include_bytes!("../../../../registry/content/release-publish.output.json").as_slice(),
        ),
        (
            "release-publish-v2.json",
            include_bytes!("../../../../registry/content/release-publish-v2.json").as_slice(),
        ),
        (
            "release-publish-v2.input.json",
            include_bytes!("../../../../registry/content/release-publish-v2.input.json").as_slice(),
        ),
        (
            "release-publish-v2.output.json",
            include_bytes!("../../../../registry/content/release-publish-v2.output.json")
                .as_slice(),
        ),
    ] {
        content.publish_exact(name, bytes)?;
    }
    Ok(())
}

fn validate_live_registry(workspace: &Workspace) -> Result<()> {
    let content = open_descendant(&workspace.root, &[".proof", "registry", "content"])
        .context("frozen release.publish::v2 registry directory is missing or unsafe")?;
    let raw_entry = content
        .read_optional("release-publish-v2.json")?
        .context("frozen release.publish::v2 registry entry is missing")?;
    let actual_raw: Value = serde_json::from_slice(&raw_entry)
        .context("release.publish::v2 registry JSON is invalid")?;
    let expected_raw: Value = serde_json::from_str(include_str!(
        "../../../../registry/content/release-publish-v2.json"
    ))?;
    if canonicalize(&actual_raw)? != canonicalize(&expected_raw)? {
        bail!("release.publish::v2 raw registry entry differs from the frozen contract");
    }
    let registry = load_registry(&workspace.root)?;
    let expected: RegistryEntry = serde_json::from_str(include_str!(
        "../../../../registry/content/release-publish-v2.json"
    ))?;
    let actual = registry
        .find("release.publish", "v2")
        .context("frozen release.publish::v2 registry entry is missing")?;
    if actual != &expected || actual.status != proof_kernel::VersionStatus::Active {
        bail!("release.publish::v2 registry entry differs from the frozen active contract");
    }
    for (name, expected) in [
        (
            "release-publish-v2.input.json",
            include_bytes!("../../../../registry/content/release-publish-v2.input.json").as_slice(),
        ),
        (
            "release-publish-v2.output.json",
            include_bytes!("../../../../registry/content/release-publish-v2.output.json")
                .as_slice(),
        ),
    ] {
        if content.read_optional(name)?.as_deref() != Some(expected) {
            bail!("release.publish::v2 registry schema differs from the frozen contract: {name}");
        }
    }
    let handlers = proof_content::content_handlers()
        .into_iter()
        .filter(|handler| handler.operation() == "release.publish")
        .collect::<Vec<_>>();
    if handlers.len() != 1
        || handlers[0].idempotency_policy_for("v2")
            != proof_kernel::IdempotencyPolicy::RequiredUuidV7ExactReplay
    {
        bail!("release.publish::v2 Content handler is not exactly resolvable");
    }
    Ok(())
}

fn load_or_append_exact_evaluation(
    store: &SqliteStore,
    run_id: Uuid,
) -> Result<AgentRunEvaluation> {
    let matching = store
        .list_agent_run_evaluations(&run_id)?
        .into_iter()
        .filter(|evaluation| evaluation.evaluator == TRACE_EVALUATOR)
        .collect::<Vec<_>>();
    let evaluation = match matching.as_slice() {
        [] => {
            let evaluation =
                super::live::deterministic_evaluation(store, run_id, TRACE_EVALUATOR, Utc::now())?;
            verify_exact_evaluation(store, &evaluation)?;
            store.save_agent_run_evaluation(&evaluation)?;
            evaluation
        }
        [evaluation] => {
            verify_exact_evaluation(store, evaluation)?;
            evaluation.clone()
        }
        _ => bail!("preparation run has duplicate deterministic evaluations"),
    };
    Ok(evaluation)
}

fn verify_exact_evaluation(store: &SqliteStore, evaluation: &AgentRunEvaluation) -> Result<()> {
    let recomputed = super::live::deterministic_evaluation(
        store,
        evaluation.run_id,
        &evaluation.evaluator,
        evaluation.created_at,
    )?;
    if recomputed.run_id != evaluation.run_id
        || recomputed.evaluator != evaluation.evaluator
        || recomputed.outcome != evaluation.outcome
        || recomputed.score_bps != evaluation.score_bps
        || recomputed.metrics != evaluation.metrics
        || recomputed.summary != evaluation.summary
        || recomputed.created_at != evaluation.created_at
    {
        bail!("preparation evaluation differs from independent trace recomputation");
    }
    let checks = evaluation.metrics["checks"]
        .as_array()
        .context("preparation evaluation has no exact check list")?;
    let ids = checks
        .iter()
        .map(|check| {
            if check["passed"] != true {
                bail!("preparation evaluation includes a failed check");
            }
            check["name"]
                .as_str()
                .context("preparation evaluation includes an unnamed check")
        })
        .collect::<Result<Vec<_>>>()?;
    if evaluation.evaluator != TRACE_EVALUATOR
        || evaluation.outcome != AgentEvaluationOutcome::Passed
        || evaluation.score_bps != Some(10_000)
        || evaluation.metrics["passed_checks"] != 10
        || evaluation.metrics["total_checks"] != 10
        || evaluation.metrics["score_bps"] != 10_000
        || ids != PREP_CHECK_IDS
    {
        bail!("preparation evaluation is not the immutable exact 10/10 record");
    }
    Ok(())
}

fn materialize_edition(workspace: &Workspace, edition_id: Uuid, bytes: &[u8]) -> Result<()> {
    let editions = open_descendant(&workspace.root, &[".proof", "data", "editions"])?;
    let name = format!("{edition_id}.json");
    editions.publish_exact(&name, bytes)?;
    let recovered = editions
        .read_optional(&name)?
        .context("published synthetic edition disappeared")?;
    if recovered != bytes {
        bail!("published synthetic edition bytes differ from the preparation binding");
    }
    Ok(())
}

fn bind_materialization(
    directory: &SecureDirectory,
    workspace: &Workspace,
    preparation_id: Uuid,
    evaluated: &EvaluatedRecord,
    edition_id: Uuid,
    bytes: &[u8],
) -> Result<MaterializationRecord> {
    let expected = sealed_record(MaterializationRecord {
        schema: format!("{PREP_SCHEMA}/edition-materialization"),
        preparation_id,
        evaluated_digest: evaluated.binding_digest,
        edition_id,
        edition_bytes_digest: serialized_digest(&serde_json::from_slice::<Value>(bytes)?)?,
        relative_path: format!(".proof/data/editions/{edition_id}.json"),
        binding_digest: ContentDigest::from_bytes([0; 32]),
    })?;
    if let Some(existing) = read_record::<MaterializationRecord>(directory, "edition-bound.json")? {
        if existing != expected {
            bail!("synthetic edition materialization binding drift");
        }
        return Ok(existing);
    }
    let editions = open_descendant(&workspace.root, &[".proof", "data", "editions"])?;
    if editions
        .read_optional(&format!("{edition_id}.json"))?
        .is_some()
    {
        bail!("refusing an existing synthetic edition target not bound to this preparation");
    }
    write_record(directory, "edition-bound.json", &expected)?;
    Ok(expected)
}

fn validate_evaluated_chain(
    initialized: &InitializedRecord,
    awaiting: &AwaitingRecord,
    evaluated: &EvaluatedRecord,
) -> Result<()> {
    if evaluated.schema != format!("{PREP_SCHEMA}/evaluated")
        || evaluated.preparation_id != initialized.preparation_id
        || evaluated.awaiting_digest != awaiting.binding_digest
        || evaluated.evaluation_id.get_version_num() != 7
    {
        bail!("evaluated preparation phase binding drift");
    }
    Ok(())
}

fn validate_materialization_chain(
    initialized: &InitializedRecord,
    evaluated: &EvaluatedRecord,
    materialization: &MaterializationRecord,
) -> Result<()> {
    if materialization.schema != format!("{PREP_SCHEMA}/edition-materialization")
        || materialization.preparation_id != initialized.preparation_id
        || materialization.evaluated_digest != evaluated.binding_digest
        || materialization.edition_id != initialized.edition.id
        || materialization.edition_bytes_digest != serialized_digest(&initialized.edition)?
        || materialization.relative_path
            != format!(".proof/data/editions/{}.json", initialized.edition.id)
    {
        bail!("synthetic edition materialization phase binding drift");
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn validate_ready_chain(
    workspace_root: &Path,
    initialized: &InitializedRecord,
    awaiting: &AwaitingRecord,
    evaluated: &EvaluatedRecord,
    materialization: &MaterializationRecord,
    ready: &ReadyRecord,
    agent_id: Uuid,
    delegation_id: Uuid,
    policy_file: &Path,
) -> Result<()> {
    let packet = &ready.packet;
    let manifest_digest = edition_manifest_digest(&initialized.edition)?;
    let goal = live_goal(
        initialized.edition.id,
        &manifest_digest,
        initialized.idempotency_key,
    );
    if ready.schema != format!("{PREP_SCHEMA}/ready")
        || ready.preparation_id != initialized.preparation_id
        || ready.evaluated_digest != evaluated.binding_digest
        || ready.materialization_digest != materialization.binding_digest
        || packet.schema != "proof-release-manager-live-readiness/v1"
        || packet.preparation_id != initialized.preparation_id
        || packet.preflight.run_id != awaiting.run_id
        || packet.preflight.evaluation_id != evaluated.evaluation_id
        || packet.preflight.evaluation_digest != evaluated.evaluation_digest
        || packet.preflight.evidence_digest != packet.bindings.preflight_evidence_digest
        || packet.preflight.evidence_digest
            != generic_digest(&json!({
                "schema": "proof-release-manager-preflight-evidence-digest/v1",
                "evidence": packet.preflight.evidence,
            }))?
        || packet.preflight.score_bps != 10_000
        || packet.preflight.passed_checks != 10
        || packet.preflight.total_checks != 10
        || packet.bindings.agent_id != agent_id
        || packet.bindings.agent_principal_id != initialized.agent_principal_id
        || packet.bindings.approver_principal_id != initialized.approver_principal_id
        || packet.bindings.delegation_id != delegation_id
        || packet.bindings.edition_id != initialized.edition.id
        || packet.bindings.manifest_digest != manifest_digest
        || packet.bindings.idempotency_key != initialized.idempotency_key
        || packet.bindings.version_label != LIVE_VERSION_LABEL
        || packet.bindings.goal != goal
    {
        bail!("durable readiness phase binding drift");
    }
    let workspace_path = workspace_root
        .to_str()
        .context("workspace path is not valid UTF-8")?;
    let policy_path = policy_file
        .to_str()
        .context("live policy path is not valid UTF-8")?;
    let expected_next_argv = vec![
        "proof".to_string(),
        "--workspace".to_string(),
        workspace_path.to_string(),
        "agent".to_string(),
        "live-start".to_string(),
        agent_id.to_string(),
        "--goal".to_string(),
        goal,
        "--policy-file".to_string(),
        policy_path.to_string(),
        "--preflight-evaluation-id".to_string(),
        evaluated.evaluation_id.to_string(),
        "--delegation-id".to_string(),
        delegation_id.to_string(),
    ];
    if packet.next_argv != expected_next_argv {
        bail!("completed preparation replay arguments differ from the immutable readiness packet");
    }
    Ok(())
}

fn readiness_packet(
    workspace: &Workspace,
    preparation_id: Uuid,
    agent_id: Uuid,
    policy_file: &Path,
    evaluation: &AgentRunEvaluation,
    evaluated: &EvaluatedRecord,
    setup: proof_agent_runtime::LiveRunSetup,
    goal: String,
    checked_at: DateTime<Utc>,
) -> Result<ReadinessPacket> {
    let evidence = &setup.preflight_evidence;
    let policy_digest: ContentDigest = serde_json::from_value(evidence["policy_digest"].clone())
        .context("checked preflight evidence has no policy digest")?;
    let trace_digest: ContentDigest = serde_json::from_value(evidence["trace_digest"].clone())
        .context("checked preflight evidence has no trace digest")?;
    let run_id: Uuid = serde_json::from_value(evidence["run_id"].clone())?;
    let evaluation_id: Uuid = serde_json::from_value(evidence["evaluation_id"].clone())?;
    if evaluation_id != evaluation.id || run_id != evaluation.run_id {
        bail!("checked preflight evidence identity drift");
    }
    let policy = setup.policy;
    let bindings = policy.binding_inputs;
    let policy_path = policy_file
        .to_str()
        .context("live policy path is not valid UTF-8")?;
    let workspace_path = workspace
        .root
        .to_str()
        .context("workspace path is not valid UTF-8")?;
    Ok(ReadinessPacket {
        schema: "proof-release-manager-live-readiness/v1".to_string(),
        preparation_id,
        checked_at,
        preflight: PreflightPacket {
            run_id,
            evaluation_id,
            evaluation_digest: evaluated.evaluation_digest,
            evidence: evidence.clone(),
            evidence_digest: setup.preflight_evidence_digest,
            policy_digest,
            trace_digest,
            score_bps: 10_000,
            passed_checks: 10,
            total_checks: 10,
        },
        live_policy: LivePolicyPacket {
            template_policy_digest: policy.template_policy_digest,
            check_set_digest: policy.check_set_digest,
            tamper_vector_set_digest: policy.tamper_vector_set_digest,
            pricing_schedule_digest: policy.pricing_schedule_digest,
            instructions_digest: policy.instructions_digest,
            initial_input_digest: policy.initial_input_digest,
            parameters_schema_digest: policy.parameters_schema_digest,
            tool_declaration_digest: policy.tool_declaration_digest,
            tool_set_digest: policy.tool_set_digest,
        },
        bindings: ReadinessBindings {
            preflight_evidence_digest: setup.preflight_evidence_digest,
            agent_id,
            agent_principal_id: bindings.agent_principal_id,
            approver_principal_id: bindings.approver_principal_id,
            delegation_id: bindings.delegation_id,
            delegation_digest: bindings.delegation_digest,
            edition_id: bindings.edition_id,
            manifest_digest: bindings.manifest_digest,
            idempotency_key: bindings.idempotency_key,
            version_label: bindings.version_label,
            goal: goal.clone(),
        },
        next_argv: vec![
            "proof".to_string(),
            "--workspace".to_string(),
            workspace_path.to_string(),
            "agent".to_string(),
            "live-start".to_string(),
            agent_id.to_string(),
            "--goal".to_string(),
            goal,
            "--policy-file".to_string(),
            policy_path.to_string(),
            "--preflight-evaluation-id".to_string(),
            evaluation.id.to_string(),
            "--delegation-id".to_string(),
            bindings.delegation_id.to_string(),
        ],
    })
}

fn edition_manifest_digest(edition: &proof_content::Edition) -> Result<String> {
    let mut objects = edition.objects.clone();
    objects.sort_by(|left, right| left.id.cmp(&right.id));
    let content_digest = proof_content::digest::canonical_digest(&objects);
    if content_digest != edition.content_digest {
        bail!("preparation edition content digest drift");
    }
    Ok(proof_content::digest::canonical_digest(&json!({
        "schema": "proof-content-preview-manifest/v1",
        "edition_id": edition.id,
        "edition_content_digest": content_digest,
        "objects": objects.iter().map(|object| json!({
            "object_id": object.id,
            "locale": object.locale,
            "content_digest": proof_content::digest::canonical_digest(object),
        })).collect::<Vec<_>>(),
    })))
}

#[cfg(test)]
fn preparation_directory(workspace: &Workspace, preparation_id: Uuid) -> Result<SecureDirectory> {
    let access = PreparationWorkspaceAccess::open(&workspace.root)?;
    preparation_directory_from_proof(&access.proof, preparation_id, true)
}

fn preparation_directory_from_proof(
    proof: &SecureDirectory,
    preparation_id: Uuid,
    create: bool,
) -> Result<SecureDirectory> {
    let base = if create {
        proof.ensure_child("live-prepare")?
    } else {
        proof.open_child("live-prepare")?
    };
    if create {
        base.ensure_child(&preparation_id.to_string())
    } else {
        base.open_child(&preparation_id.to_string())
    }
}

fn existing_preparation_directory(
    proof: &SecureDirectory,
    preparation_id: Uuid,
) -> Result<Option<SecureDirectory>> {
    let Some(base) = proof.open_child_optional("live-prepare")? else {
        return Ok(None);
    };
    base.open_child_optional(&preparation_id.to_string())
}

fn preparation_goal(preparation_id: Uuid, edition_id: Uuid, idempotency_key: Uuid) -> String {
    format!(
        "Prepare deterministic release evidence {preparation_id} for synthetic edition {edition_id} with idempotency key {idempotency_key}."
    )
}

fn live_goal(edition_id: Uuid, manifest_digest: &str, idempotency_key: Uuid) -> String {
    format!(
        "Publish synthetic edition {edition_id} to preview as {LIVE_VERSION_LABEL} using manifest {manifest_digest} and idempotency key {idempotency_key}."
    )
}

fn print_start_packet(
    workspace_root: &Path,
    initialized: &InitializedRecord,
    awaiting: &AwaitingRecord,
) -> Result<()> {
    let workspace_path = workspace_root
        .to_str()
        .context("workspace path is not valid UTF-8")?;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "status": "waiting_for_approval",
            "preparation_id": initialized.preparation_id,
            "run_id": awaiting.run_id,
            "step_id": awaiting.step_id,
            "request_id": awaiting.request_id,
            "approver_id": initialized.approver_principal_id,
            "next_argv": [
                "proof",
                "--workspace",
                workspace_path,
                "approval",
                "approve",
                awaiting.request_id.to_string(),
                "--approver",
                initialized.approver_principal_id.to_string(),
            ],
        }))?
    );
    Ok(())
}

fn sealed_record<T>(mut record: T) -> Result<T>
where
    T: Serialize + DeserializeOwned,
{
    let mut value = serde_json::to_value(&record)?;
    let object = value
        .as_object_mut()
        .context("preparation record must be an object")?;
    object.insert(
        "binding_digest".to_string(),
        serde_json::to_value(ContentDigest::from_bytes([0; 32]))?,
    );
    object.remove("binding_digest");
    let binding_digest = generic_digest(&value)?;
    let mut value = serde_json::to_value(&record)?;
    value["binding_digest"] = serde_json::to_value(binding_digest)?;
    record = serde_json::from_value(value)?;
    Ok(record)
}

fn write_record<T: Serialize>(directory: &SecureDirectory, name: &str, record: &T) -> Result<bool> {
    let bytes = serde_json::to_vec_pretty(record)?;
    directory.publish_exact(name, &bytes)
}

fn read_record<T: DeserializeOwned + Serialize>(
    directory: &SecureDirectory,
    name: &str,
) -> Result<Option<T>> {
    directory
        .read_optional(name)?
        .map(|bytes| decode_record(&bytes))
        .transpose()
}

fn decode_record<T: DeserializeOwned + Serialize>(bytes: &[u8]) -> Result<T> {
    let mut value: Value =
        serde_json::from_slice(bytes).context("invalid strict preparation record")?;
    let original = value.clone();
    let stored: ContentDigest = serde_json::from_value(
        value
            .get("binding_digest")
            .cloned()
            .context("preparation record has no binding digest")?,
    )?;
    value
        .as_object_mut()
        .context("preparation record must be an object")?
        .remove("binding_digest");
    if generic_digest(&value)? != stored {
        bail!("preparation record binding digest mismatch");
    }
    let record: T = serde_json::from_value(original.clone())
        .context("invalid strict typed preparation record")?;
    if canonicalize(&serde_json::to_value(&record)?)? != canonicalize(&original)? {
        bail!("preparation record contains unknown or lossy nested fields");
    }
    Ok(record)
}

fn generic_digest(value: &Value) -> Result<ContentDigest> {
    Ok(digest(ArtifactKind::Generic, &canonicalize(value)?))
}

fn serialized_digest<T: Serialize>(value: &T) -> Result<ContentDigest> {
    Ok(digest(
        ArtifactKind::Generic,
        &canonicalize_serialized(value)?,
    ))
}

fn required_string<'a>(
    value: &'a Value,
    pointer: &str,
) -> std::result::Result<&'a str, ModelGatewayError> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ModelGatewayError::InvalidResponse(format!(
                "deterministic tool output is missing {pointer}"
            ))
        })
}

fn parse_preparation_id(value: &str) -> Result<Uuid> {
    let id = parse_canonical_uuid(value, "preparation")?;
    if id.get_version_num() != 7 {
        bail!("preparation ID must be UUIDv7");
    }
    Ok(id)
}

fn parse_canonical_uuid(value: &str, label: &str) -> Result<Uuid> {
    let id = Uuid::parse_str(value).with_context(|| format!("invalid {label} ID"))?;
    if id.to_string() != value {
        bail!("{label} ID must use lowercase hyphenated UUID spelling");
    }
    Ok(id)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use chrono::Duration;
    use clap::Parser;
    use proof_kernel::{delegation::DelegationScope, Delegation};

    use super::*;

    #[derive(Default)]
    struct BoundaryCounters {
        factory: AtomicUsize,
        base_reads: AtomicUsize,
        key_reads: AtomicUsize,
        constructions: AtomicUsize,
        sends: AtomicUsize,
    }

    struct BoundarySpyFactory {
        counters: Arc<BoundaryCounters>,
    }

    impl ModelGatewayFactory for BoundarySpyFactory {
        fn create(
            &self,
            _context: &ModelGatewayFactoryContext,
        ) -> std::result::Result<Arc<dyn ModelGateway>, ModelGatewayFactoryError> {
            self.counters.factory.fetch_add(1, Ordering::SeqCst);
            self.counters.base_reads.fetch_add(1, Ordering::SeqCst);
            self.counters.key_reads.fetch_add(1, Ordering::SeqCst);
            self.counters.constructions.fetch_add(1, Ordering::SeqCst);
            Err(ModelGatewayFactoryError::Configuration(
                "provider boundary spy was unexpectedly invoked".to_string(),
            ))
        }
    }

    struct Fixture {
        _directory: assert_fs::TempDir,
        cli: Cli,
        workspace: Workspace,
        store: Arc<SqliteStore>,
        preparation_id: Uuid,
        live_agent: AgentDefinition,
        delegation: Delegation,
    }

    fn fixture() -> Fixture {
        let directory = assert_fs::TempDir::new_in(env!("CARGO_MANIFEST_DIR")).unwrap();
        #[cfg(unix)]
        std::fs::set_permissions(
            directory.path(),
            std::os::unix::fs::PermissionsExt::from_mode(0o700),
        )
        .unwrap();
        let cli = Cli::parse_from([
            "proof",
            "--workspace",
            directory.path().to_str().unwrap(),
            "init",
        ]);
        crate::commands::content::cmd_init(&cli).unwrap();
        crate::commands::approval::cmd_approver_init(&cli).unwrap();
        let access = PreparationWorkspaceAccess::open(&cli.workspace).unwrap();
        let workspace = access.open_workspace().unwrap();
        let store = access.open_store().unwrap();
        ensure_preparation_registry(&workspace).unwrap();
        let live_agent = AgentDefinition::new(
            "live-release-manager",
            "Use only the frozen release publication tool.",
            "openai",
            "gpt-5.6-sol",
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
        store.save_agent_definition(&live_agent).unwrap();
        let delegation = Delegation {
            id: Uuid::now_v7(),
            issuer: workspace.actor,
            recipient: workspace.actor,
            allowed_actions: Vec::new(),
            resource_scope: Vec::new(),
            scope: DelegationScope {
                allowed_operations: Some(vec!["release.publish".to_string()]),
                allowed_domains: Some(vec!["content".to_string()]),
                resource_scope: None,
            },
            valid_from: Utc::now() - Duration::seconds(5),
            valid_until: Utc::now() + Duration::minutes(20),
            revoked: false,
        };
        store.save_delegation(&delegation).unwrap();
        Fixture {
            _directory: directory,
            cli,
            workspace,
            store,
            preparation_id: Uuid::now_v7(),
            live_agent,
            delegation,
        }
    }

    fn approve(fixture: &Fixture) -> AwaitingRecord {
        let directory = preparation_directory(&fixture.workspace, fixture.preparation_id).unwrap();
        let awaiting: AwaitingRecord = read_record(&directory, "awaiting.json").unwrap().unwrap();
        let initialized: InitializedRecord = read_record(&directory, "initialized.json")
            .unwrap()
            .unwrap();
        crate::commands::approval::cmd_approval_approve(
            &fixture.cli,
            &awaiting.request_id.to_string(),
            &initialized.approver_principal_id.to_string(),
            Some("approved deterministic synthetic rehearsal"),
        )
        .unwrap();
        awaiting
    }

    fn policy_path() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../evals/release-manager-live-v1.json")
    }

    #[test]
    fn real_sqlite_prepare_round_trip_is_exact_idempotent_and_provider_free() {
        let fixture = fixture();

        cmd_live_prepare_start(&fixture.cli, &fixture.preparation_id.to_string()).unwrap();
        let awaiting = approve(&fixture);
        let events_before_finish = fixture
            .store
            .list_agent_run_events(&awaiting.run_id)
            .unwrap()
            .len();
        assert!(events_before_finish > 0);
        cmd_live_prepare_finish(
            &fixture.cli,
            &fixture.preparation_id.to_string(),
            &fixture.live_agent.id.to_string(),
            &fixture.delegation.id.to_string(),
            &policy_path(),
        )
        .unwrap();

        let directory = preparation_directory(&fixture.workspace, fixture.preparation_id).unwrap();
        let ready: ReadyRecord = read_record(&directory, "ready.json").unwrap().unwrap();
        let run = fixture
            .store
            .load_agent_run(&awaiting.run_id)
            .unwrap()
            .unwrap();
        assert_eq!(run.status, AgentRunStatus::Succeeded);
        assert_eq!(ready.packet.preflight.run_id, awaiting.run_id);
        assert_eq!(ready.packet.preflight.score_bps, 10_000);
        assert_eq!(ready.packet.preflight.passed_checks, 10);
        assert_eq!(ready.packet.bindings.agent_id, fixture.live_agent.id);
        assert_eq!(ready.packet.bindings.delegation_id, fixture.delegation.id);
        assert!(!serde_json::to_string(&ready).unwrap().contains("OPENAI"));
        let live_policy: Value = serde_json::from_str(include_str!(
            "../../../../evals/release-manager-live-v1.json"
        ))
        .unwrap();
        let check_ids = live_policy["checks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|check| check["id"].clone())
            .collect::<Vec<_>>();
        let tamper_ids = live_policy["tamper_vectors"].clone();
        let declaration = live_policy.pointer("/tool/declaration").unwrap();
        assert_eq!(
            ready.packet.live_policy.template_policy_digest,
            generic_digest(&live_policy).unwrap()
        );
        assert_eq!(
            ready.packet.live_policy.check_set_digest,
            independent_wrapped_digest(
                "proof-release-manager-live-check-set-digest/v1",
                "check_ids",
                &json!(check_ids),
            )
        );
        assert_eq!(
            ready.packet.live_policy.tamper_vector_set_digest,
            independent_wrapped_digest(
                "proof-release-manager-live-tamper-vector-set-digest/v1",
                "tamper_vector_ids",
                &tamper_ids,
            )
        );
        assert_eq!(
            ready.packet.live_policy.pricing_schedule_digest,
            generic_digest(&live_policy["pricing"]).unwrap()
        );
        assert_eq!(
            ready.packet.live_policy.instructions_digest,
            generic_digest(&live_policy["outbound_data"]["instructions"]).unwrap()
        );
        assert_eq!(
            ready.packet.live_policy.initial_input_digest,
            generic_digest(&Value::String(ready.packet.bindings.goal.clone())).unwrap()
        );
        assert_eq!(
            ready.packet.live_policy.parameters_schema_digest,
            independent_wrapped_digest(
                "proof-openai-function-parameters-digest/v1",
                "parameters",
                &declaration["parameters"],
            )
        );
        assert_eq!(
            ready.packet.live_policy.tool_declaration_digest,
            independent_wrapped_digest(
                "proof-openai-function-declaration-digest/v1",
                "declaration",
                declaration,
            )
        );
        assert_eq!(
            ready.packet.live_policy.tool_set_digest,
            independent_wrapped_digest(
                "proof-openai-tool-set-digest/v1",
                "tools",
                &json!([declaration]),
            )
        );
        let event_count = fixture
            .store
            .list_agent_run_events(&awaiting.run_id)
            .unwrap()
            .len();
        let evaluation_count = fixture
            .store
            .list_agent_run_evaluations(&awaiting.run_id)
            .unwrap()
            .len();
        let edition_count = std::fs::read_dir(fixture.workspace.root.join(".proof/data/editions"))
            .unwrap()
            .filter_map(std::result::Result::ok)
            .filter(|entry| {
                entry.path().extension().and_then(|value| value.to_str()) == Some("json")
            })
            .count();

        cmd_live_prepare_start(&fixture.cli, &fixture.preparation_id.to_string()).unwrap();
        cmd_live_prepare_finish(
            &fixture.cli,
            &fixture.preparation_id.to_string(),
            &fixture.live_agent.id.to_string(),
            &fixture.delegation.id.to_string(),
            &policy_path(),
        )
        .unwrap();
        let replay: ReadyRecord = read_record(&directory, "ready.json").unwrap().unwrap();
        assert_eq!(replay, ready);
        assert_eq!(
            fixture
                .store
                .list_agent_run_events(&awaiting.run_id)
                .unwrap()
                .len(),
            event_count
        );
        assert_eq!(
            fixture
                .store
                .list_agent_run_evaluations(&awaiting.run_id)
                .unwrap()
                .len(),
            evaluation_count
        );
        assert_eq!(
            std::fs::read_dir(fixture.workspace.root.join(".proof/data/editions"))
                .unwrap()
                .filter_map(std::result::Result::ok)
                .filter(|entry| {
                    entry.path().extension().and_then(|value| value.to_str()) == Some("json")
                })
                .count(),
            edition_count
        );
        assert!(fixture
            .store
            .list_agent_runs()
            .unwrap()
            .iter()
            .flat_map(|run| fixture.store.list_agent_checkpoints(&run.id).unwrap())
            .all(|checkpoint| checkpoint.state["kind"] != "agent_runtime_v2"));
        assert!(!fixture.workspace.root.join(".proof/artifacts").exists());
    }

    #[test]
    fn completed_start_replays_after_registry_and_approver_drift() {
        let fixture = fixture();
        cmd_live_prepare_start(&fixture.cli, &fixture.preparation_id.to_string()).unwrap();
        let directory = preparation_directory(&fixture.workspace, fixture.preparation_id).unwrap();
        let before: AwaitingRecord = read_record(&directory, "awaiting.json").unwrap().unwrap();

        crate::commands::approval::cmd_approver_init(&fixture.cli).unwrap();
        std::fs::remove_file(
            fixture
                .workspace
                .root
                .join(".proof/registry/content/release-publish-v2.json"),
        )
        .unwrap();
        cmd_live_prepare_start(&fixture.cli, &fixture.preparation_id.to_string()).unwrap();

        let replay: AwaitingRecord = read_record(&directory, "awaiting.json").unwrap().unwrap();
        assert_eq!(replay, before);
        assert_eq!(fixture.store.list_agent_runs().unwrap().len(), 1);
    }

    #[test]
    fn completed_finish_replays_after_authority_registry_approver_and_policy_drift() {
        let fixture = fixture();
        let mutable_policy = fixture.workspace.root.join("frozen-live-policy.json");
        std::fs::copy(policy_path(), &mutable_policy).unwrap();
        cmd_live_prepare_start(&fixture.cli, &fixture.preparation_id.to_string()).unwrap();
        let awaiting = approve(&fixture);
        cmd_live_prepare_finish(
            &fixture.cli,
            &fixture.preparation_id.to_string(),
            &fixture.live_agent.id.to_string(),
            &fixture.delegation.id.to_string(),
            &mutable_policy,
        )
        .unwrap();
        let directory = preparation_directory(&fixture.workspace, fixture.preparation_id).unwrap();
        let before: ReadyRecord = read_record(&directory, "ready.json").unwrap().unwrap();
        let event_count = fixture
            .store
            .list_agent_run_events(&awaiting.run_id)
            .unwrap()
            .len();
        let evaluation_count = fixture
            .store
            .list_agent_run_evaluations(&awaiting.run_id)
            .unwrap()
            .len();

        let mut expired_and_revoked = fixture.delegation.clone();
        expired_and_revoked.valid_from = Utc::now() - Duration::minutes(10);
        expired_and_revoked.valid_until = Utc::now() - Duration::minutes(5);
        expired_and_revoked.revoked = true;
        fixture.store.save_delegation(&expired_and_revoked).unwrap();
        crate::commands::approval::cmd_approver_init(&fixture.cli).unwrap();
        std::fs::remove_file(
            fixture
                .workspace
                .root
                .join(".proof/registry/content/release-publish-v2.json"),
        )
        .unwrap();
        std::fs::write(&mutable_policy, b"{}").unwrap();

        let counters = Arc::new(BoundaryCounters::default());
        finish_with_check_factory(
            &fixture.cli,
            &fixture.preparation_id.to_string(),
            &fixture.live_agent.id.to_string(),
            &fixture.delegation.id.to_string(),
            &mutable_policy,
            Arc::new(BoundarySpyFactory {
                counters: counters.clone(),
            }),
        )
        .unwrap();
        let replay: ReadyRecord = read_record(&directory, "ready.json").unwrap().unwrap();
        assert_eq!(replay, before);
        assert_eq!(replay.packet.checked_at, before.packet.checked_at);
        assert_eq!(
            fixture
                .store
                .list_agent_run_events(&awaiting.run_id)
                .unwrap()
                .len(),
            event_count
        );
        assert_eq!(
            fixture
                .store
                .list_agent_run_evaluations(&awaiting.run_id)
                .unwrap()
                .len(),
            evaluation_count
        );
        assert_eq!(counters.factory.load(Ordering::SeqCst), 0);
        assert_eq!(counters.base_reads.load(Ordering::SeqCst), 0);
        assert_eq!(counters.key_reads.load(Ordering::SeqCst), 0);
        assert_eq!(counters.constructions.load(Ordering::SeqCst), 0);
        assert_eq!(counters.sends.load(Ordering::SeqCst), 0);

        for (agent_id, delegation_id, policy) in [
            (
                Uuid::now_v7().to_string(),
                fixture.delegation.id.to_string(),
                mutable_policy.clone(),
            ),
            (
                fixture.live_agent.id.to_string(),
                Uuid::now_v7().to_string(),
                mutable_policy.clone(),
            ),
            (
                fixture.live_agent.id.to_string(),
                fixture.delegation.id.to_string(),
                fixture.workspace.root.join("different-policy.json"),
            ),
        ] {
            assert!(finish_with_check_factory(
                &fixture.cli,
                &fixture.preparation_id.to_string(),
                &agent_id,
                &delegation_id,
                &policy,
                Arc::new(BoundarySpyFactory {
                    counters: counters.clone(),
                }),
            )
            .is_err());
        }
        assert_eq!(counters.factory.load(Ordering::SeqCst), 0);
        assert_eq!(counters.base_reads.load(Ordering::SeqCst), 0);
        assert_eq!(counters.key_reads.load(Ordering::SeqCst), 0);
        assert_eq!(counters.constructions.load(Ordering::SeqCst), 0);
        assert_eq!(counters.sends.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn authoritative_check_only_path_has_zero_provider_boundary_calls() {
        let fixture = fixture();
        cmd_live_prepare_start(&fixture.cli, &fixture.preparation_id.to_string()).unwrap();
        approve(&fixture);
        let counters = Arc::new(BoundaryCounters::default());
        finish_with_check_factory(
            &fixture.cli,
            &fixture.preparation_id.to_string(),
            &fixture.live_agent.id.to_string(),
            &fixture.delegation.id.to_string(),
            &policy_path(),
            Arc::new(BoundarySpyFactory {
                counters: counters.clone(),
            }),
        )
        .unwrap();
        assert_eq!(counters.factory.load(Ordering::SeqCst), 0);
        assert_eq!(counters.base_reads.load(Ordering::SeqCst), 0);
        assert_eq!(counters.key_reads.load(Ordering::SeqCst), 0);
        assert_eq!(counters.constructions.load(Ordering::SeqCst), 0);
        assert_eq!(counters.sends.load(Ordering::SeqCst), 0);
        assert_eq!(fixture.store.list_agent_runs().unwrap().len(), 1);
        assert!(fixture
            .store
            .list_agent_runs()
            .unwrap()
            .iter()
            .flat_map(|run| fixture.store.list_agent_checkpoints(&run.id).unwrap())
            .all(|checkpoint| checkpoint.state["kind"] != "agent_runtime_v2"));
    }

    #[test]
    fn raw_registry_and_nested_schema_unknown_fields_fail_closed() {
        let fixture = fixture();
        let content = fixture.workspace.root.join(".proof/registry/content");
        let entry_path = content.join("release-publish-v2.json");
        let original_entry = std::fs::read(&entry_path).unwrap();
        let mut entry: Value = serde_json::from_slice(&original_entry).unwrap();
        entry["unexpected"] = json!({"nested": true});
        std::fs::write(&entry_path, serde_json::to_vec(&entry).unwrap()).unwrap();
        let first =
            cmd_live_prepare_start(&fixture.cli, &fixture.preparation_id.to_string()).unwrap_err();
        assert!(first.to_string().contains("raw registry entry differs"));
        assert!(fixture.store.list_agent_runs().unwrap().is_empty());

        std::fs::write(&entry_path, original_entry).unwrap();
        let schema_path = content.join("release-publish-v2.input.json");
        let original_schema = std::fs::read(&schema_path).unwrap();
        let mut schema: Value = serde_json::from_slice(&original_schema).unwrap();
        schema["properties"]["edition_id"]["unexpected_nested_keyword"] = json!(true);
        std::fs::write(&schema_path, serde_json::to_vec(&schema).unwrap()).unwrap();
        let second_id = Uuid::now_v7();
        let second = cmd_live_prepare_start(&fixture.cli, &second_id.to_string()).unwrap_err();
        assert!(second.to_string().contains("schema differs"));
        assert!(fixture.store.list_agent_runs().unwrap().is_empty());
    }

    fn independent_wrapped_digest(schema: &str, field: &str, value: &Value) -> ContentDigest {
        let mut wrapper = serde_json::Map::new();
        wrapper.insert("schema".to_string(), Value::String(schema.to_string()));
        wrapper.insert(field.to_string(), value.clone());
        generic_digest(&Value::Object(wrapper)).unwrap()
    }

    #[test]
    fn dispatch_marker_without_run_recovers_once_and_lost_stdout_reuses_ids() {
        let fixture = fixture();
        let directory = preparation_directory(&fixture.workspace, fixture.preparation_id).unwrap();
        let lock = directory.exclusive_lock("phase.lock").unwrap();
        let initialized = load_or_create_initialized(
            &directory,
            &fixture.workspace,
            &fixture.store,
            fixture.preparation_id,
        )
        .unwrap();
        ensure_preparation_registry(&fixture.workspace).unwrap();
        save_exact_agent(&fixture.store, &initialized.deterministic_agent).unwrap();
        let dispatch = sealed_record(DispatchRecord {
            schema: format!("{PREP_SCHEMA}/dispatch"),
            preparation_id: fixture.preparation_id,
            initialized_digest: initialized.binding_digest,
            binding_digest: ContentDigest::from_bytes([0; 32]),
        })
        .unwrap();
        write_record(&directory, "dispatch.json", &dispatch).unwrap();
        drop(lock);

        cmd_live_prepare_start(&fixture.cli, &fixture.preparation_id.to_string()).unwrap();
        let first: AwaitingRecord = read_record(&directory, "awaiting.json").unwrap().unwrap();
        cmd_live_prepare_start(&fixture.cli, &fixture.preparation_id.to_string()).unwrap();
        let second: AwaitingRecord = read_record(&directory, "awaiting.json").unwrap().unwrap();
        assert_eq!(second, first);
        assert_eq!(
            fixture
                .store
                .list_agent_runs()
                .unwrap()
                .into_iter()
                .filter(|run| run.goal == initialized.goal)
                .count(),
            1
        );
    }

    #[test]
    fn malformed_id_pending_or_denied_approval_never_materializes_readiness() {
        let fixture = fixture();
        assert!(cmd_live_prepare_start(&fixture.cli, "../not-a-preparation").is_err());
        assert!(
            cmd_live_prepare_start(&fixture.cli, "550e8400-e29b-41d4-a716-446655440000")
                .unwrap_err()
                .to_string()
                .contains("UUIDv7")
        );
        assert!(fixture.store.list_agent_runs().unwrap().is_empty());

        cmd_live_prepare_start(&fixture.cli, &fixture.preparation_id.to_string()).unwrap();
        let directory = preparation_directory(&fixture.workspace, fixture.preparation_id).unwrap();
        let initialized: InitializedRecord = read_record(&directory, "initialized.json")
            .unwrap()
            .unwrap();
        let awaiting: AwaitingRecord = read_record(&directory, "awaiting.json").unwrap().unwrap();
        let pending = cmd_live_prepare_finish(
            &fixture.cli,
            &fixture.preparation_id.to_string(),
            &fixture.live_agent.id.to_string(),
            &fixture.delegation.id.to_string(),
            &policy_path(),
        )
        .unwrap_err();
        assert!(pending.to_string().contains("has not been signed"));
        crate::commands::approval::cmd_approval_deny(
            &fixture.cli,
            &awaiting.request_id.to_string(),
            &initialized.approver_principal_id.to_string(),
            Some("negative test denial"),
        )
        .unwrap();
        let denied = cmd_live_prepare_finish(
            &fixture.cli,
            &fixture.preparation_id.to_string(),
            &fixture.live_agent.id.to_string(),
            &fixture.delegation.id.to_string(),
            &policy_path(),
        )
        .unwrap_err();
        assert!(denied.to_string().contains("exact trusted Human approval"));
        assert!(directory.read_optional("ready.json").unwrap().is_none());
        assert!(
            std::fs::read_dir(fixture.workspace.root.join(".proof/data/editions"))
                .unwrap()
                .next()
                .is_none()
        );
    }

    #[test]
    fn invalid_live_bindings_and_unbound_target_fail_before_readiness() {
        let fixture = fixture();
        cmd_live_prepare_start(&fixture.cli, &fixture.preparation_id.to_string()).unwrap();
        approve(&fixture);
        let directory = preparation_directory(&fixture.workspace, fixture.preparation_id).unwrap();
        let initialized: InitializedRecord = read_record(&directory, "initialized.json")
            .unwrap()
            .unwrap();
        let bad_policy = fixture.workspace.root.join("bad-live-policy.json");
        std::fs::write(&bad_policy, b"{}").unwrap();
        let counters = Arc::new(BoundaryCounters::default());
        let factory = || {
            Arc::new(BoundarySpyFactory {
                counters: counters.clone(),
            }) as Arc<dyn ModelGatewayFactory>
        };

        assert!(finish_with_check_factory(
            &fixture.cli,
            &fixture.preparation_id.to_string(),
            &Uuid::now_v7().to_string(),
            &fixture.delegation.id.to_string(),
            &policy_path(),
            factory(),
        )
        .unwrap_err()
        .to_string()
        .contains("agent definition not found"));
        assert!(finish_with_check_factory(
            &fixture.cli,
            &fixture.preparation_id.to_string(),
            &fixture.live_agent.id.to_string(),
            &Uuid::now_v7().to_string(),
            &policy_path(),
            factory(),
        )
        .unwrap_err()
        .to_string()
        .contains("delegation not found"));

        let wrong_agent = AgentDefinition::new(
            "wrong-live-release-manager",
            "Use only the frozen release publication tool.",
            "not-openai",
            "gpt-5.6-sol",
            vec![AgentTool::new("release.publish", "v2").unwrap()],
            fixture.live_agent.limits.clone(),
            Utc::now(),
        )
        .unwrap();
        fixture.store.save_agent_definition(&wrong_agent).unwrap();
        let wrong_agent_error = finish_with_check_factory(
            &fixture.cli,
            &fixture.preparation_id.to_string(),
            &wrong_agent.id.to_string(),
            &fixture.delegation.id.to_string(),
            &policy_path(),
            factory(),
        )
        .unwrap_err();
        assert!(
            wrong_agent_error
                .to_string()
                .contains("frozen release-manager live profile"),
            "{wrong_agent_error:#}"
        );

        let mut wrong_scope = fixture.delegation.clone();
        wrong_scope.id = Uuid::now_v7();
        wrong_scope.scope.allowed_operations = Some(vec!["object.create".to_string()]);
        fixture.store.save_delegation(&wrong_scope).unwrap();
        let wrong_scope_error = finish_with_check_factory(
            &fixture.cli,
            &fixture.preparation_id.to_string(),
            &fixture.live_agent.id.to_string(),
            &wrong_scope.id.to_string(),
            &policy_path(),
            factory(),
        )
        .unwrap_err();
        assert!(
            wrong_scope_error
                .to_string()
                .contains("exact active singleton release.publish/content grant"),
            "{wrong_scope_error:#}"
        );

        assert!(finish_with_check_factory(
            &fixture.cli,
            &fixture.preparation_id.to_string(),
            &fixture.live_agent.id.to_string(),
            &fixture.delegation.id.to_string(),
            &bad_policy,
            factory(),
        )
        .unwrap_err()
        .to_string()
        .contains("differs from the frozen"));

        let edition_path = fixture
            .workspace
            .root
            .join(".proof/data/editions")
            .join(format!("{}.json", initialized.edition.id));
        std::fs::write(
            &edition_path,
            serde_json::to_vec_pretty(&initialized.edition).unwrap(),
        )
        .unwrap();
        let existing = finish_with_check_factory(
            &fixture.cli,
            &fixture.preparation_id.to_string(),
            &fixture.live_agent.id.to_string(),
            &fixture.delegation.id.to_string(),
            &policy_path(),
            factory(),
        )
        .unwrap_err();
        assert!(existing
            .to_string()
            .contains("not bound to this preparation"));
        assert!(directory.read_optional("ready.json").unwrap().is_none());
        assert_eq!(counters.factory.load(Ordering::SeqCst), 0);
        assert_eq!(counters.base_reads.load(Ordering::SeqCst), 0);
        assert_eq!(counters.key_reads.load(Ordering::SeqCst), 0);
        assert_eq!(counters.constructions.load(Ordering::SeqCst), 0);
        assert_eq!(counters.sends.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn record_tamper_and_invalid_evaluation_fail_closed() {
        let fixture = fixture();
        cmd_live_prepare_start(&fixture.cli, &fixture.preparation_id.to_string()).unwrap();
        approve(&fixture);
        let directory = preparation_directory(&fixture.workspace, fixture.preparation_id).unwrap();
        let initialized_path = fixture
            .workspace
            .root
            .join(".proof/live-prepare")
            .join(fixture.preparation_id.to_string())
            .join("initialized.json");
        let original = std::fs::read(&initialized_path).unwrap();
        let mut tampered: Value = serde_json::from_slice(&original).unwrap();
        tampered["goal"] = Value::String("binding drift".to_string());
        std::fs::write(
            &initialized_path,
            serde_json::to_vec_pretty(&tampered).unwrap(),
        )
        .unwrap();
        assert!(
            cmd_live_prepare_start(&fixture.cli, &fixture.preparation_id.to_string())
                .unwrap_err()
                .to_string()
                .contains("binding digest mismatch")
        );
        std::fs::write(&initialized_path, original).unwrap();

        let original = std::fs::read(&initialized_path).unwrap();
        let mut nested_unknown: Value = serde_json::from_slice(&original).unwrap();
        nested_unknown["edition"]["unexpected_nested_field"] = json!(true);
        let mut unhashed = nested_unknown.clone();
        unhashed.as_object_mut().unwrap().remove("binding_digest");
        nested_unknown["binding_digest"] =
            serde_json::to_value(generic_digest(&unhashed).unwrap()).unwrap();
        std::fs::write(
            &initialized_path,
            serde_json::to_vec_pretty(&nested_unknown).unwrap(),
        )
        .unwrap();
        assert!(
            cmd_live_prepare_start(&fixture.cli, &fixture.preparation_id.to_string())
                .unwrap_err()
                .to_string()
                .contains("unknown or lossy nested fields")
        );
        std::fs::write(&initialized_path, original).unwrap();

        let initialized: InitializedRecord = read_record(&directory, "initialized.json")
            .unwrap()
            .unwrap();
        let outcome = build_runtime(&fixture.workspace, fixture.store.clone(), &initialized)
            .unwrap()
            .resume(
                read_record::<AwaitingRecord>(&directory, "awaiting.json")
                    .unwrap()
                    .unwrap()
                    .run_id,
            )
            .unwrap();
        let AgentRuntimeOutcome::Completed {
            run, evaluation, ..
        } = outcome
        else {
            panic!("approved deterministic run must complete");
        };
        let mut invalid = evaluation;
        invalid.id = Uuid::now_v7();
        invalid.evaluator = TRACE_EVALUATOR.to_string();
        invalid.outcome = AgentEvaluationOutcome::Failed;
        invalid.score_bps = Some(0);
        fixture.store.save_agent_run_evaluation(&invalid).unwrap();
        let error = cmd_live_prepare_finish(
            &fixture.cli,
            &fixture.preparation_id.to_string(),
            &fixture.live_agent.id.to_string(),
            &fixture.delegation.id.to_string(),
            &policy_path(),
        )
        .unwrap_err();
        assert!(error
            .to_string()
            .contains("differs from independent trace recomputation"));
        assert_eq!(run.status, AgentRunStatus::Succeeded);
        assert!(directory.read_optional("ready.json").unwrap().is_none());
    }

    #[cfg(target_family = "unix")]
    #[test]
    fn symlinked_edition_leaf_is_rejected_without_readiness() {
        use std::os::unix::fs::symlink;

        let fixture = fixture();
        cmd_live_prepare_start(&fixture.cli, &fixture.preparation_id.to_string()).unwrap();
        approve(&fixture);
        let directory = preparation_directory(&fixture.workspace, fixture.preparation_id).unwrap();
        let initialized: InitializedRecord = read_record(&directory, "initialized.json")
            .unwrap()
            .unwrap();
        let outside = fixture.workspace.root.join("outside-edition.json");
        std::fs::write(
            &outside,
            serde_json::to_vec_pretty(&initialized.edition).unwrap(),
        )
        .unwrap();
        symlink(
            &outside,
            fixture
                .workspace
                .root
                .join(".proof/data/editions")
                .join(format!("{}.json", initialized.edition.id)),
        )
        .unwrap();
        assert!(cmd_live_prepare_finish(
            &fixture.cli,
            &fixture.preparation_id.to_string(),
            &fixture.live_agent.id.to_string(),
            &fixture.delegation.id.to_string(),
            &policy_path(),
        )
        .is_err());
        assert!(directory.read_optional("ready.json").unwrap().is_none());
    }

    #[test]
    fn duplicate_recovery_candidates_fail_without_selecting_latest() {
        let fixture = fixture();
        let directory = preparation_directory(&fixture.workspace, fixture.preparation_id).unwrap();
        let initialized = load_or_create_initialized(
            &directory,
            &fixture.workspace,
            &fixture.store,
            fixture.preparation_id,
        )
        .unwrap();
        ensure_preparation_registry(&fixture.workspace).unwrap();
        save_exact_agent(&fixture.store, &initialized.deterministic_agent).unwrap();
        let dispatch = sealed_record(DispatchRecord {
            schema: format!("{PREP_SCHEMA}/dispatch"),
            preparation_id: fixture.preparation_id,
            initialized_digest: initialized.binding_digest,
            binding_digest: ContentDigest::from_bytes([0; 32]),
        })
        .unwrap();
        write_record(&directory, "dispatch.json", &dispatch).unwrap();
        for _ in 0..2 {
            assert!(matches!(
                build_runtime(&fixture.workspace, fixture.store.clone(), &initialized)
                    .unwrap()
                    .start(initialized.deterministic_agent.id, initialized.goal.clone())
                    .unwrap(),
                AgentRuntimeOutcome::WaitingForApproval { .. }
            ));
        }
        let error =
            cmd_live_prepare_start(&fixture.cli, &fixture.preparation_id.to_string()).unwrap_err();
        assert!(error
            .to_string()
            .contains("at most one saved agent+goal run"));
        assert!(directory.read_optional("awaiting.json").unwrap().is_none());
    }

    #[test]
    fn exclusive_preparation_lock_serializes_concurrent_finish_replays() {
        let fixture = fixture();
        cmd_live_prepare_start(&fixture.cli, &fixture.preparation_id.to_string()).unwrap();
        let awaiting = approve(&fixture);
        let root = fixture.workspace.root.clone();
        let preparation_id = fixture.preparation_id.to_string();
        let agent_id = fixture.live_agent.id.to_string();
        let delegation_id = fixture.delegation.id.to_string();
        let policy = policy_path();
        let threads = (0..2)
            .map(|_| {
                let root = root.clone();
                let preparation_id = preparation_id.clone();
                let agent_id = agent_id.clone();
                let delegation_id = delegation_id.clone();
                let policy = policy.clone();
                std::thread::spawn(move || {
                    let cli =
                        Cli::parse_from(["proof", "--workspace", root.to_str().unwrap(), "status"]);
                    cmd_live_prepare_finish(
                        &cli,
                        &preparation_id,
                        &agent_id,
                        &delegation_id,
                        &policy,
                    )
                })
            })
            .collect::<Vec<_>>();
        for thread in threads {
            thread.join().unwrap().unwrap();
        }
        assert_eq!(
            fixture
                .store
                .list_agent_run_evaluations(&awaiting.run_id)
                .unwrap()
                .iter()
                .filter(|evaluation| evaluation.evaluator == TRACE_EVALUATOR)
                .count(),
            1
        );
        assert_eq!(
            fixture
                .store
                .list_agent_run_events(&awaiting.run_id)
                .unwrap()
                .iter()
                .filter(|event| event.kind == proof_kernel::AgentRunEventKind::Completed)
                .count(),
            1
        );
    }

    #[cfg(target_family = "unix")]
    #[test]
    fn symlinked_edition_directory_component_is_rejected() {
        use std::os::unix::fs::symlink;

        let fixture = fixture();
        cmd_live_prepare_start(&fixture.cli, &fixture.preparation_id.to_string()).unwrap();
        approve(&fixture);
        let directory = preparation_directory(&fixture.workspace, fixture.preparation_id).unwrap();
        let editions = fixture.workspace.root.join(".proof/data/editions");
        std::fs::remove_dir(&editions).unwrap();
        let outside = fixture.workspace.root.join("outside-editions");
        std::fs::create_dir(&outside).unwrap();
        symlink(&outside, &editions).unwrap();
        assert!(cmd_live_prepare_finish(
            &fixture.cli,
            &fixture.preparation_id.to_string(),
            &fixture.live_agent.id.to_string(),
            &fixture.delegation.id.to_string(),
            &policy_path(),
        )
        .is_err());
        assert!(directory.read_optional("ready.json").unwrap().is_none());
    }
}
