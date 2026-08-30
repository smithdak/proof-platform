use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use base64::Engine as _;
use proof_content::{
    ChangeSet, ChangeSetEdit, FieldType, Object, ObjectCreateEdit, ObjectDeleteEdit,
    SchemaDefinition, SchemaField,
};
use proof_kernel::{
    generate_keypair, generate_keypair_for, principal_from_keypair, AgentRun, AgentRunMode,
    AgentRunStatus, AgentRunStep, AgentRunStepStatus, AgentRunStore, ApprovalOutcome,
    ApprovalStore, ExecutionContext, ExecutionError, Governance, OperationHandler, PrincipalKind,
    Proof, RecordingAgentRunStore, RecordingApprovalStore, Registry, RegistryEntry,
    SignedApprovalDecision, SignedApprovalRequest, VersionStatus,
};
use proof_storage::SqliteStore;
use proof_transport_mcp::{load_workspace_keypair, load_workspace_registry, McpServer};
use serde_json::{json, Value};

const EVIDENCE_META_KEY: &str = "com.proofplatform/evidence";
const IDENTITY_META_KEY: &str = "com.proofplatform/identity";
const APPROVAL_META_KEY: &str = "com.proofplatform/approval";
const RUN_META_KEY: &str = "com.proofplatform/run";

struct TestWorkspace(PathBuf);

impl TestWorkspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "proof-mcp-server-test-{}",
            uuid::Uuid::now_v7().simple()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestWorkspace {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

fn copy_directory(source: &Path, destination: &Path) {
    std::fs::create_dir_all(destination).unwrap();
    for entry in std::fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if source_path.is_dir() {
            copy_directory(&source_path, &destination_path);
        } else {
            std::fs::copy(source_path, destination_path).unwrap();
        }
    }
}

struct EchoHandler;

impl OperationHandler for EchoHandler {
    fn operation(&self) -> &str {
        "test.echo"
    }

    fn execute(&self, input: &Value, _context: &ExecutionContext) -> Result<Value, ExecutionError> {
        Ok(json!({ "message": input["message"] }))
    }
}

struct HumanPublishHandler {
    calls: Arc<AtomicUsize>,
}

impl OperationHandler for HumanPublishHandler {
    fn operation(&self) -> &str {
        "test.publish"
    }

    fn execute(&self, input: &Value, _context: &ExecutionContext) -> Result<Value, ExecutionError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(json!({"published": input["message"]}))
    }
}

fn registry_entry() -> RegistryEntry {
    RegistryEntry {
        operation: "test.echo".to_string(),
        domain: "test".to_string(),
        version: "v1".to_string(),
        action: "test:echo".to_string(),
        description: "Echo a message".to_string(),
        input_schema: json!({
            "type": "object",
            "properties": { "message": { "type": "string", "minLength": 1 } },
            "required": ["message"],
            "additionalProperties": false
        })
        .to_string(),
        output_schema: json!({
            "type": "object",
            "properties": { "message": { "type": "string" } },
            "required": ["message"],
            "additionalProperties": false
        })
        .to_string(),
        required_authority: "delegation-grant".to_string(),
        governance: Governance::AgentExecutable,
        idempotency: "idempotent".to_string(),
        consequence: "read".to_string(),
        evidence_contract: "operation-effect-v1".to_string(),
        benchmark: None,
        status: VersionStatus::Active,
        deprecated_since: None,
        replacement_operation: None,
    }
}

fn human_registry_entry() -> RegistryEntry {
    let mut entry = registry_entry();
    entry.operation = "test.publish".to_string();
    entry.action = "test:publish".to_string();
    entry.description = "Publish after human approval".to_string();
    entry.governance = Governance::HumanOnly;
    entry.consequence = "publish".to_string();
    entry.output_schema = json!({
        "type": "object",
        "properties": { "published": { "type": "string" } },
        "required": ["published"],
        "additionalProperties": false
    })
    .to_string();
    entry
}

fn modern_meta() -> Value {
    json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientInfo": {
            "name": "proof-test-client",
            "version": "0.1.0"
        },
        "io.modelcontextprotocol/clientCapabilities": {}
    })
}

fn server(workspace: &TestWorkspace) -> (McpServer, proof_kernel::Keypair) {
    let identity = generate_keypair();
    let registry = Registry::new(vec![registry_entry()]).unwrap();
    let mut server = McpServer::new(registry, identity.clone(), workspace.path().to_path_buf());
    server.register_handler(Arc::new(EchoHandler));
    (server, identity)
}

fn copy_repository_registry(workspace: &TestWorkspace) {
    let repository_registry = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("registry");
    copy_directory(
        &repository_registry,
        &workspace.path().join(".proof/registry"),
    );
    copy_directory(&repository_registry, &workspace.path().join("registry"));
}

fn content_server(
    workspace: &TestWorkspace,
) -> (
    McpServer,
    proof_kernel::Keypair,
    Arc<SqliteStore>,
    ChangeSet,
    Object,
) {
    copy_repository_registry(workspace);
    let schema = SchemaDefinition::new(
        "Article",
        1,
        vec![SchemaField {
            name: "title".to_string(),
            field_type: FieldType::Text,
            required: true,
            localized: false,
            default_value: None,
        }],
    );
    let schema_directory = workspace.path().join(".proof/data/schemas");
    std::fs::create_dir_all(&schema_directory).unwrap();
    std::fs::write(
        schema_directory.join(format!("{}-{}.json", schema.id, schema.version)),
        serde_json::to_string(&schema).unwrap(),
    )
    .unwrap();

    let existing = Object::create(&schema, "en-US", json!({"title": "Existing"})).unwrap();
    let created = Object::create(&schema, "en-US", json!({"title": "Created"})).unwrap();
    let object_directory = workspace.path().join(".proof/data/objects");
    std::fs::create_dir_all(&object_directory).unwrap();
    std::fs::write(
        object_directory.join(format!("{}.json", existing.id)),
        serde_json::to_string(&existing).unwrap(),
    )
    .unwrap();

    let base_state = BTreeMap::from([(existing.id, existing.clone())]);
    let mut changeset = ChangeSet::new(
        "Replace object",
        &base_state,
        vec![
            ChangeSetEdit::ObjectCreate(ObjectCreateEdit { object: created }),
            ChangeSetEdit::ObjectDelete(ObjectDeleteEdit {
                object_id: existing.id,
                expected_revision: existing.revision,
            }),
        ],
    );
    changeset
        .transition_to(proof_content::ChangeSetStatus::Submitted)
        .unwrap();
    changeset
        .transition_to(proof_content::ChangeSetStatus::Approved)
        .unwrap();
    let changeset_directory = workspace.path().join(".proof/data/changesets");
    std::fs::create_dir_all(&changeset_directory).unwrap();
    std::fs::write(
        changeset_directory.join(format!("{}.json", changeset.id)),
        serde_json::to_string(&changeset).unwrap(),
    )
    .unwrap();

    let identity = generate_keypair();
    let registry = load_workspace_registry(workspace.path()).unwrap();
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let mut server = McpServer::new_with_storage(
        registry,
        identity.clone(),
        workspace.path().to_path_buf(),
        store.clone(),
    );
    for handler in proof_content::content_handlers() {
        server.register_handler(handler);
    }
    (server, identity, store, changeset, existing)
}

fn approval_server(
    workspace: &TestWorkspace,
) -> (
    McpServer,
    proof_kernel::Keypair,
    Arc<RecordingApprovalStore>,
    Arc<RecordingAgentRunStore>,
    Arc<AtomicUsize>,
) {
    let identity = generate_keypair();
    let registry = Registry::new(vec![human_registry_entry()]).unwrap();
    let approval_store = Arc::new(RecordingApprovalStore::default());
    let run_store = Arc::new(RecordingAgentRunStore::default());
    let calls = Arc::new(AtomicUsize::new(0));
    let mut server = McpServer::new(registry, identity.clone(), workspace.path().to_path_buf())
        .with_approval_store(approval_store.clone())
        .with_run_store(run_store.clone());
    server.register_handler(Arc::new(HumanPublishHandler {
        calls: calls.clone(),
    }));
    (server, identity, approval_store, run_store, calls)
}

fn approval_call(id: u64, request_state: Option<&str>, message: &str) -> Value {
    let mut params = json!({
        "name": "proof_test_v1_test_publish",
        "arguments": {"message": message}
    });
    if let Some(request_state) = request_state {
        params["requestState"] = json!(request_state);
    }
    modern_request(id, "tools/call", params)
}

fn modern_request(id: u64, method: &str, mut params: Value) -> Value {
    params
        .as_object_mut()
        .unwrap()
        .insert("_meta".to_string(), modern_meta());
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params
    })
}

#[test]
fn modern_discovery_reports_stable_proof_identity() {
    let workspace = TestWorkspace::new();
    let (mut server, identity) = server(&workspace);
    let first = server
        .handle_message(modern_request(1, "server/discover", json!({})))
        .unwrap();
    let second = server
        .handle_message(modern_request(2, "server/discover", json!({})))
        .unwrap();

    assert_eq!(first["result"]["resultType"], "complete");
    assert_eq!(first["result"]["supportedVersions"][0], "2026-07-28");
    assert_eq!(
        first["result"]["capabilities"]["tools"]["listChanged"],
        false
    );
    assert_eq!(
        first["result"]["_meta"][IDENTITY_META_KEY]["principalId"],
        identity.principal_id.to_string()
    );
    assert_eq!(
        first["result"]["_meta"][IDENTITY_META_KEY],
        second["result"]["_meta"][IDENTITY_META_KEY]
    );
}

#[test]
fn modern_tool_list_uses_current_mcp_field_names() {
    let workspace = TestWorkspace::new();
    let (mut server, _) = server(&workspace);
    let response = server
        .handle_message(modern_request(1, "tools/list", json!({})))
        .unwrap();
    let result = &response["result"];
    let tool = &result["tools"][0];

    assert_eq!(result["resultType"], "complete");
    assert_eq!(result["cacheScope"], "private");
    assert!(result["ttlMs"].as_u64().unwrap() > 0);
    assert_eq!(tool["name"], "proof_test_v1_test_echo");
    assert_eq!(tool["inputSchema"]["type"], "object");
    assert_eq!(tool["outputSchema"]["type"], "object");
    assert_eq!(tool["annotations"]["readOnlyHint"], true);
    assert_eq!(tool["annotations"]["destructiveHint"], false);
    assert_eq!(tool["annotations"]["idempotentHint"], true);
    assert!(tool["annotations"].get("readOnly").is_none());
}

#[test]
fn tool_calls_reuse_identity_and_persist_verifiable_proofs() {
    let workspace = TestWorkspace::new();
    let (server, identity) = server(&workspace);
    let run_store = Arc::new(RecordingAgentRunStore::default());
    let mut server = server.with_run_store(run_store.clone());
    let request = |id, message| {
        modern_request(
            id,
            "tools/call",
            json!({
                "name": "proof_test_v1_test_echo",
                "arguments": { "message": message }
            }),
        )
    };
    let first = server.handle_message(request(1, "first")).unwrap();
    let second = server.handle_message(request(2, "second")).unwrap();

    assert_eq!(
        first["result"]["structuredContent"],
        json!({ "message": "first" })
    );
    assert_eq!(
        second["result"]["structuredContent"],
        json!({ "message": "second" })
    );
    for response in [&first, &second] {
        assert_eq!(response["result"]["resultType"], "complete");
        assert_eq!(response["result"]["isError"], false);
        let proof_value = response["result"]["_meta"][EVIDENCE_META_KEY]["proof"].clone();
        let proof: Proof = serde_json::from_value(proof_value).unwrap();
        assert_eq!(proof.body.actor, identity.principal_id);
        assert_eq!(proof.body.operation, "test.echo::v1");
        proof.verify(&identity.signing_key.verifying_key()).unwrap();
        assert!(workspace
            .path()
            .join(".proof/data/proofs")
            .join(format!("{}.json", proof.body.id))
            .exists());
        let run: AgentRun =
            serde_json::from_value(response["result"]["_meta"][RUN_META_KEY]["run"].clone())
                .unwrap();
        let step: AgentRunStep =
            serde_json::from_value(response["result"]["_meta"][RUN_META_KEY]["step"].clone())
                .unwrap();
        assert_eq!(run.status, AgentRunStatus::Succeeded);
        assert_eq!(step.status, AgentRunStepStatus::Succeeded);
        assert_eq!(step.run_id, run.id);
    }
    assert_eq!(run_store.list_agent_runs().unwrap().len(), 2);
}

#[test]
fn governed_content_tools_replay_original_proof_and_preserve_run_metadata() {
    let workspace = TestWorkspace::new();
    let (mut server, identity, store, changeset, existing) = content_server(&workspace);
    let commit_key = uuid::Uuid::now_v7();
    let commit = |id, arguments| {
        modern_request(
            id,
            "tools/call",
            json!({
                "name": "proof_content_v1_changeset_commit",
                "arguments": arguments,
            }),
        )
    };
    let first = server
        .handle_message(commit(
            1,
            json!({
                "notes": "first commit",
                "changeset_id": changeset.id,
                "idempotency_key": commit_key,
            }),
        ))
        .unwrap();
    assert_eq!(first["result"]["isError"], false);
    assert_eq!(
        first["result"]["structuredContent"]["operation"],
        "changeset.commit"
    );
    let first_output = first["result"]["structuredContent"].clone();
    let first_proof = first["result"]["_meta"][EVIDENCE_META_KEY]["proof"].clone();
    let first_run = first["result"]["_meta"][RUN_META_KEY].clone();
    let deleted_path = workspace
        .path()
        .join(".proof/data/objects")
        .join(format!("{}.json", existing.id));
    let object_count_after_first = std::fs::read_dir(workspace.path().join(".proof/data/objects"))
        .unwrap()
        .count();
    assert_eq!(object_count_after_first, 1);
    assert_eq!(first_run["run"]["status"], "succeeded");
    assert_eq!(first_run["step"]["status"], "succeeded");
    let first_proof: proof_kernel::Proof = serde_json::from_value(first_proof.clone()).unwrap();
    assert_eq!(first_proof.body.actor, identity.principal_id);
    first_proof
        .verify(&identity.signing_key.verifying_key())
        .unwrap();

    let retry = server
        .handle_message(commit(
            2,
            json!({
                "idempotency_key": commit_key,
                "changeset_id": changeset.id,
                "notes": "first commit",
            }),
        ))
        .unwrap();
    assert_eq!(retry["result"]["structuredContent"], first_output);
    assert_eq!(
        retry["result"]["_meta"][EVIDENCE_META_KEY]["proof"],
        serde_json::to_value(&first_proof).unwrap()
    );
    assert_eq!(
        std::fs::read_dir(workspace.path().join(".proof/data/objects"))
            .unwrap()
            .count(),
        object_count_after_first
    );
    assert_eq!(
        store
            .list_proofs_for_operation("changeset.commit", Some("v1"))
            .unwrap()
            .len(),
        1
    );
    assert_eq!(store.list_agent_runs().unwrap().len(), 2);
    assert!(!deleted_path.exists());

    let conflict = server
        .handle_message(commit(
            3,
            json!({
                "idempotency_key": commit_key,
                "changeset_id": changeset.id,
                "notes": "different input",
            }),
        ))
        .unwrap();
    assert_eq!(conflict["result"]["isError"], true);
    assert!(conflict["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("already bound to different input"));
    assert_eq!(
        std::fs::read_dir(workspace.path().join(".proof/data/objects"))
            .unwrap()
            .count(),
        object_count_after_first
    );
    assert_eq!(
        store
            .list_proofs_for_operation("changeset.commit", Some("v1"))
            .unwrap()
            .len(),
        1
    );

    let edition_key = uuid::Uuid::now_v7();
    let edition = |id, arguments| {
        modern_request(
            id,
            "tools/call",
            json!({
                "name": "proof_content_v1_edition_create",
                "arguments": arguments,
            }),
        )
    };
    let edition_first = server
        .handle_message(edition(
            4,
            json!({
                "changeset_id": changeset.id,
                "idempotency_key": edition_key,
            }),
        ))
        .unwrap();
    assert_eq!(edition_first["result"]["isError"], false);
    let edition_output = edition_first["result"]["structuredContent"].clone();
    let edition_proof = edition_first["result"]["_meta"][EVIDENCE_META_KEY]["proof"].clone();
    let edition_id = edition_output["data"]["edition"]["id"].as_str().unwrap();
    let edition_count = std::fs::read_dir(workspace.path().join(".proof/data/editions"))
        .unwrap()
        .count();
    assert_eq!(edition_count, 1);

    let edition_retry = server
        .handle_message(edition(
            5,
            json!({
                "idempotency_key": edition_key,
                "changeset_id": changeset.id,
            }),
        ))
        .unwrap();
    assert_eq!(edition_retry["result"]["structuredContent"], edition_output);
    assert_eq!(
        edition_retry["result"]["_meta"][EVIDENCE_META_KEY]["proof"],
        edition_proof
    );
    assert_eq!(
        std::fs::read_dir(workspace.path().join(".proof/data/editions"))
            .unwrap()
            .count(),
        edition_count
    );
    assert_eq!(
        store
            .list_proofs_for_operation("edition.create", Some("v1"))
            .unwrap()
            .len(),
        1
    );
    assert!(workspace
        .path()
        .join(".proof/data/editions")
        .join(format!("{edition_id}.json"))
        .exists());

    let edition_conflict = server
        .handle_message(edition(
            6,
            json!({
                "idempotency_key": edition_key,
                "changeset_id": existing.id,
            }),
        ))
        .unwrap();
    assert_eq!(edition_conflict["result"]["isError"], true);
    assert!(edition_conflict["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("already bound to different input"));
    assert_eq!(
        std::fs::read_dir(workspace.path().join(".proof/data/editions"))
            .unwrap()
            .count(),
        edition_count
    );
}

#[test]
fn session_tool_calls_append_steps_without_completing_the_run() {
    let workspace = TestWorkspace::new();
    let (server, identity) = server(&workspace);
    let run_store = Arc::new(RecordingAgentRunStore::default());
    let mut server = server.with_run_store(run_store.clone());
    let mut run = AgentRun::new(
        identity.principal_id,
        AgentRunMode::Session,
        "Complete a multi-step task",
        chrono::Utc::now(),
    )
    .unwrap();
    run_store.save_agent_run(&run).unwrap();
    run.start(chrono::Utc::now()).unwrap();
    run_store.save_agent_run(&run).unwrap();

    let mut request = modern_request(
        1,
        "tools/call",
        json!({
            "name": "proof_test_v1_test_echo",
            "arguments": {"message": "session step"}
        }),
    );
    request["params"]["_meta"][RUN_META_KEY] = json!({"runId": run.id});
    let response = server.handle_message(request).unwrap();

    assert_eq!(response["result"]["isError"], false);
    assert_eq!(
        response["result"]["_meta"][RUN_META_KEY]["run"]["status"],
        "running"
    );
    assert_eq!(
        run_store.load_agent_run(&run.id).unwrap().unwrap().status,
        AgentRunStatus::Running
    );
    let steps = run_store.list_agent_run_steps(&run.id).unwrap();
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].status, AgentRunStepStatus::Succeeded);
}

#[test]
fn pending_retry_attempt_resumes_with_exact_recorded_input() {
    let workspace = TestWorkspace::new();
    let (server, identity) = server(&workspace);
    let run_store = Arc::new(RecordingAgentRunStore::default());
    let mut server = server.with_run_store(run_store.clone());
    let input = json!({"message": "retry me"});
    let now = chrono::Utc::now();
    let mut run = AgentRun::new(
        identity.principal_id,
        AgentRunMode::Session,
        "Recover a failed attempt",
        now,
    )
    .unwrap();
    run_store.save_agent_run(&run).unwrap();
    run.start(now).unwrap();
    run_store.save_agent_run(&run).unwrap();
    let mut failed = AgentRunStep::new(run.id, 0, "test.echo", "v1", &input, now).unwrap();
    run_store.save_agent_run_step(&failed).unwrap();
    failed.start(now).unwrap();
    run_store.save_agent_run_step(&failed).unwrap();
    failed.fail("temporary failure", now).unwrap();
    run_store.save_agent_run_step(&failed).unwrap();
    run.fail(now).unwrap();
    run_store.save_agent_run(&run).unwrap();
    let retry = failed.retry(now).unwrap();
    run_store.save_agent_run_step(&retry).unwrap();
    run.resume(now).unwrap();
    run_store.save_agent_run(&run).unwrap();

    let mut request = modern_request(
        1,
        "tools/call",
        json!({
            "name": "proof_test_v1_test_echo",
            "arguments": input
        }),
    );
    request["params"]["_meta"][RUN_META_KEY] = json!({"runId": run.id, "stepId": retry.id});
    let response = server.handle_message(request).unwrap();

    assert_eq!(response["result"]["isError"], false);
    let completed_retry = run_store.load_agent_run_step(&retry.id).unwrap().unwrap();
    assert_eq!(completed_retry.status, AgentRunStepStatus::Succeeded);
    assert_eq!(completed_retry.retry_of, Some(failed.id));
    let resumed = run_store.load_agent_run(&run.id).unwrap().unwrap();
    assert_eq!(resumed.status, AgentRunStatus::Running);
    assert_eq!(resumed.retry_count, 1);
}

#[test]
fn human_only_tools_require_signed_decisions_and_replay_once() {
    let workspace = TestWorkspace::new();
    let (mut server, identity, approval_store, run_store, calls) = approval_server(&workspace);
    let pending = server
        .handle_message(approval_call(1, None, "release"))
        .unwrap();
    let request_state = pending["result"]["requestState"].as_str().unwrap();
    let request: SignedApprovalRequest =
        serde_json::from_value(pending["result"]["_meta"][APPROVAL_META_KEY]["request"].clone())
            .unwrap();

    assert_eq!(pending["result"]["resultType"], "input_required");
    assert_eq!(
        pending["result"]["inputRequests"]["human_approval"]["method"],
        "elicitation/create"
    );
    assert_eq!(request.body.requested_by, identity.principal_id);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let run_id = uuid::Uuid::parse_str(
        pending["result"]["_meta"][RUN_META_KEY]["runId"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let step_id = uuid::Uuid::parse_str(
        pending["result"]["_meta"][RUN_META_KEY]["stepId"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        run_store.load_agent_run(&run_id).unwrap().unwrap().status,
        AgentRunStatus::WaitingForInput
    );
    assert_eq!(
        run_store
            .load_agent_run_step(&step_id)
            .unwrap()
            .unwrap()
            .status,
        AgentRunStepStatus::WaitingForApproval
    );
    assert_eq!(
        approval_store
            .load_approval_request(&request.body.id)
            .unwrap(),
        Some(request.clone())
    );

    let mut untrusted_retry = approval_call(2, Some(request_state), "release");
    untrusted_retry["params"]["inputResponses"] = json!({
        "human_approval": {"action": "accept", "content": {"approvalRequestId": request_state}}
    });
    let still_pending = server.handle_message(untrusted_retry).unwrap();
    assert_eq!(still_pending["result"]["resultType"], "input_required");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        still_pending["result"]["_meta"][RUN_META_KEY]["runId"],
        run_id.to_string()
    );

    let human = generate_keypair_for(PrincipalKind::Human);
    approval_store
        .trust_approver(principal_from_keypair(&human))
        .unwrap();
    let decision = SignedApprovalDecision::create(
        &request,
        ApprovalOutcome::Approved,
        Some("reviewed".to_string()),
        chrono::Utc::now(),
        &human,
    )
    .unwrap();
    approval_store.save_approval_decision(&decision).unwrap();

    let executed = server
        .handle_message(approval_call(3, Some(request_state), "release"))
        .unwrap();
    let replayed = server
        .handle_message(approval_call(4, Some(request_state), "release"))
        .unwrap();

    assert_eq!(executed["result"]["resultType"], "complete");
    assert_eq!(executed["result"]["isError"], false);
    assert_eq!(
        executed["result"]["structuredContent"]["published"],
        "release"
    );
    assert_eq!(
        executed["result"]["_meta"][APPROVAL_META_KEY]["status"],
        "executed"
    );
    assert_eq!(
        executed["result"]["_meta"][EVIDENCE_META_KEY]["proof"]["body"]["operation"],
        "test.publish::v1"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        executed["result"]["_meta"][EVIDENCE_META_KEY]["proof"]["body"]["id"],
        replayed["result"]["_meta"][EVIDENCE_META_KEY]["proof"]["body"]["id"]
    );
    let persisted_execution = approval_store
        .load_approval_execution(&request.body.id)
        .unwrap()
        .unwrap();
    assert_eq!(
        persisted_execution.executed_at,
        persisted_execution.proof.body.timestamp
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        executed["result"]["_meta"][RUN_META_KEY]["runId"],
        run_id.to_string()
    );
    assert_eq!(
        replayed["result"]["_meta"][RUN_META_KEY]["stepId"],
        step_id.to_string()
    );
    assert_eq!(replayed["result"]["_meta"][RUN_META_KEY]["replay"], true);
    assert_eq!(
        run_store.load_agent_run(&run_id).unwrap().unwrap().status,
        AgentRunStatus::Succeeded
    );
    assert_eq!(
        run_store
            .load_agent_run_step(&step_id)
            .unwrap()
            .unwrap()
            .status,
        AgentRunStepStatus::Succeeded
    );
}

#[test]
fn signed_denial_and_untrusted_approvers_never_dispatch() {
    let denied_workspace = TestWorkspace::new();
    let (mut denied_server, _, denied_store, _, denied_calls) = approval_server(&denied_workspace);
    let pending = denied_server
        .handle_message(approval_call(1, None, "release"))
        .unwrap();
    let request_state = pending["result"]["requestState"].as_str().unwrap();
    let request: SignedApprovalRequest =
        serde_json::from_value(pending["result"]["_meta"][APPROVAL_META_KEY]["request"].clone())
            .unwrap();
    let human = generate_keypair_for(PrincipalKind::Human);
    denied_store
        .trust_approver(principal_from_keypair(&human))
        .unwrap();
    denied_store
        .save_approval_decision(
            &SignedApprovalDecision::create(
                &request,
                ApprovalOutcome::Denied,
                Some("policy blocked".to_string()),
                chrono::Utc::now(),
                &human,
            )
            .unwrap(),
        )
        .unwrap();
    let denied = denied_server
        .handle_message(approval_call(2, Some(request_state), "release"))
        .unwrap();
    assert_eq!(denied["result"]["isError"], true);
    assert_eq!(
        denied["result"]["_meta"][APPROVAL_META_KEY]["status"],
        "denied"
    );
    assert!(denied["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("policy blocked"));
    assert_eq!(denied_calls.load(Ordering::SeqCst), 0);

    let untrusted_workspace = TestWorkspace::new();
    let (mut untrusted_server, _, untrusted_store, _, untrusted_calls) =
        approval_server(&untrusted_workspace);
    let pending = untrusted_server
        .handle_message(approval_call(3, None, "release"))
        .unwrap();
    let request_state = pending["result"]["requestState"].as_str().unwrap();
    let request: SignedApprovalRequest =
        serde_json::from_value(pending["result"]["_meta"][APPROVAL_META_KEY]["request"].clone())
            .unwrap();
    let stranger = generate_keypair_for(PrincipalKind::Human);
    untrusted_store
        .save_approval_decision(
            &SignedApprovalDecision::create(
                &request,
                ApprovalOutcome::Approved,
                None,
                chrono::Utc::now(),
                &stranger,
            )
            .unwrap(),
        )
        .unwrap();
    let untrusted = untrusted_server
        .handle_message(approval_call(4, Some(request_state), "release"))
        .unwrap();
    assert_eq!(untrusted["result"]["isError"], true);
    assert_eq!(
        untrusted["result"]["_meta"][APPROVAL_META_KEY]["status"],
        "untrusted"
    );
    assert_eq!(untrusted_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn approval_request_cannot_be_reused_for_changed_input() {
    let workspace = TestWorkspace::new();
    let (mut server, _, _, _, calls) = approval_server(&workspace);
    let pending = server
        .handle_message(approval_call(1, None, "release"))
        .unwrap();
    let request_state = pending["result"]["requestState"].as_str().unwrap();

    let changed = server
        .handle_message(approval_call(2, Some(request_state), "different"))
        .unwrap();

    assert_eq!(changed["result"]["isError"], true);
    assert!(changed["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("input does not match"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn legacy_initialize_flow_remains_supported() {
    let workspace = TestWorkspace::new();
    let (mut server, _) = server(&workspace);
    let initialize = server
        .handle_message(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": { "name": "legacy-client", "version": "1.0.0" }
            }
        }))
        .unwrap();
    assert_eq!(initialize["result"]["protocolVersion"], "2025-11-25");
    assert!(server
        .handle_message(json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }))
        .is_none());

    let tools = server
        .handle_message(json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {}
        }))
        .unwrap();
    assert_eq!(
        tools["result"]["tools"][0]["name"],
        "proof_test_v1_test_echo"
    );
    assert!(tools["result"].get("resultType").is_none());
}

#[test]
fn stdio_server_uses_one_json_response_per_line() {
    let workspace = TestWorkspace::new();
    let (mut server, _) = server(&workspace);
    let request = modern_request(1, "server/discover", json!({}));
    let notification = json!({
        "jsonrpc": "2.0",
        "method": "notifications/cancelled",
        "params": { "requestId": 99 }
    });
    let input = format!("{}\n{}\n", request, notification);
    let mut output = Vec::new();
    server
        .serve_stdio(Cursor::new(input.into_bytes()), &mut output)
        .unwrap();
    let output = String::from_utf8(output).unwrap();
    let lines = output.lines().collect::<Vec<_>>();

    assert_eq!(lines.len(), 1);
    let response: Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(response["id"], 1);
    assert_eq!(response["result"]["resultType"], "complete");
}

#[test]
fn workspace_loaders_restore_identity_and_inline_registry_schemas() {
    let workspace = TestWorkspace::new();
    let identity = generate_keypair();
    let proof_directory = workspace.path().join(".proof");
    let registry_directory = proof_directory.join("registry/test");
    std::fs::create_dir_all(&registry_directory).unwrap();
    std::fs::write(
        proof_directory.join("keypair.json"),
        serde_json::to_vec_pretty(&json!({
            "principal_id": identity.principal_id.as_uuid(),
            "kind": identity.kind,
            "created_at": identity.created_at,
            "public_key": identity.signing_key.verifying_key().to_bytes(),
            "signing_key": base64::engine::general_purpose::STANDARD
                .encode(identity.signing_key.to_bytes()),
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        registry_directory.join("echo.input.json"),
        registry_entry().input_schema,
    )
    .unwrap();
    std::fs::write(
        registry_directory.join("echo.output.json"),
        registry_entry().output_schema,
    )
    .unwrap();
    let mut manifest = registry_entry();
    manifest.input_schema = "test/echo.input.json".to_string();
    manifest.output_schema = "test/echo.output.json".to_string();
    std::fs::write(
        registry_directory.join("echo.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let restored = load_workspace_keypair(workspace.path()).unwrap();
    assert_eq!(restored.principal_id, identity.principal_id);
    assert_eq!(
        restored.signing_key.verifying_key(),
        identity.signing_key.verifying_key()
    );
    let registry = load_workspace_registry(workspace.path()).unwrap();
    let restored_entry = registry.find("test.echo", "v1").unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&restored_entry.input_schema).unwrap()["type"],
        "object"
    );
    assert_eq!(
        serde_json::from_str::<Value>(&restored_entry.output_schema).unwrap()["type"],
        "object"
    );
}

#[test]
fn workspace_registry_loader_accepts_the_full_platform_registry() {
    let workspace = TestWorkspace::new();
    let repository_registry = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("registry");
    let workspace_registry = workspace.path().join(".proof/registry");
    copy_directory(&repository_registry, &workspace_registry);

    let registry = load_workspace_registry(workspace.path()).unwrap();
    assert_eq!(registry.operations().len(), 21);
    for entry in registry.operations() {
        assert!(serde_json::from_str::<Value>(&entry.input_schema).is_ok());
        assert!(serde_json::from_str::<Value>(&entry.output_schema).is_ok());
    }
}
