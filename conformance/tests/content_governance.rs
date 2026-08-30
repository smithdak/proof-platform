use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{Duration, Utc};
use proof_conformance::{load_case, project_root};
use proof_content::{
    content_handlers, BaseState, ChangeSet, ChangeSetEdit, ChangeSetStatus, Object,
    ObjectCreateEdit, ObjectStatus, SchemaDefinition,
};
use proof_kernel::delegation::DelegationScope;
use proof_kernel::{
    canonicalize, digest, generate_keypair_for, principal_from_keypair, ApprovalGrant,
    ApprovalOutcome, ArtifactKind, Delegation, DelegationChain, ExecutionContext, ExecutionEngine,
    ExecutionError, ExecutionOutcome, Governance, IdempotencyError, IdempotencyPolicy, Keypair,
    OperationHandler, PrincipalId, PrincipalKind, RecordingStore, Registry, SignedApprovalDecision,
    SignedApprovalRequest, VersionStatus,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tempfile::TempDir;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
struct ContentCaseFile {
    name: String,
    cases: Vec<ContentOperationCase>,
}

#[derive(Debug, Deserialize)]
struct ContentOperationCase {
    id: String,
    operation: String,
    version: String,
    domain: String,
    governance: Governance,
    idempotency: String,
    authority: AuthorityVector,
    input: Value,
    approval: ApprovalMode,
    replay: ReplayMode,
    #[serde(default)]
    retry_input: Option<Value>,
    #[serde(default)]
    conflict_input: Option<Value>,
    expected: Vec<ExpectedValue>,
}

#[derive(Debug, Deserialize)]
struct AuthorityVector {
    kind: String,
    allowed_operations: Vec<String>,
    allowed_domains: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum ApprovalMode {
    None,
    SignedHuman,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum ReplayMode {
    None,
    ExactOriginal,
}

#[derive(Debug, Deserialize)]
struct ExpectedValue {
    pointer: String,
    value: Value,
}

struct Fixture {
    workspace: TempDir,
    store: Arc<RecordingStore>,
    engine: ExecutionEngine,
    agent: Keypair,
    approver: Keypair,
    delegation_root: PrincipalId,
    bindings: BTreeMap<String, Value>,
    schema: Option<SchemaDefinition>,
    primary_object: Option<Object>,
}

impl Fixture {
    fn new(registry: Registry, handlers: Vec<Arc<dyn OperationHandler>>) -> Self {
        let workspace = TempDir::new().unwrap();
        for entry in registry.operations() {
            let source = project_root().join("registry").join(&entry.input_schema);
            let destination = workspace.path().join("registry").join(&entry.input_schema);
            std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
            std::fs::copy(source, destination).unwrap();
        }
        for directory in ["schemas", "objects", "changesets", "editions"] {
            std::fs::create_dir_all(workspace.path().join(".proof/data").join(directory)).unwrap();
        }

        let store = Arc::new(RecordingStore::default());
        let agent = generate_keypair_for(PrincipalKind::Agent);
        let mut engine =
            ExecutionEngine::new_with_keypair(registry, agent.clone()).with_storage(store.clone());
        for handler in handlers {
            engine.register_handler(handler);
        }

        let mut bindings = BTreeMap::new();
        bindings.insert("commit_key".to_string(), json!(Uuid::now_v7()));
        bindings.insert("edition_key".to_string(), json!(Uuid::now_v7()));
        bindings.insert("different_changeset_id".to_string(), json!(Uuid::now_v7()));

        Self {
            workspace,
            store,
            engine,
            agent,
            approver: generate_keypair_for(PrincipalKind::Human),
            delegation_root: PrincipalId::now(),
            bindings,
            schema: None,
            primary_object: None,
        }
    }

    fn context_for(
        &self,
        operation: &str,
        domain: &str,
        authority: &AuthorityVector,
    ) -> ExecutionContext {
        assert_eq!(authority.kind, "delegation-grant");
        assert_eq!(authority.allowed_operations, vec![operation.to_string()]);
        assert_eq!(authority.allowed_domains, vec![domain.to_string()]);

        let now = Utc::now();
        let grant = Delegation {
            id: Uuid::now_v7(),
            issuer: self.delegation_root,
            recipient: self.agent.principal_id,
            allowed_actions: vec!["content:*".to_string()],
            resource_scope: vec!["*".to_string()],
            scope: DelegationScope {
                allowed_operations: Some(authority.allowed_operations.clone()),
                allowed_domains: Some(authority.allowed_domains.clone()),
                resource_scope: Some("conformance-workspace".to_string()),
            },
            valid_from: now - Duration::minutes(1),
            valid_until: now + Duration::minutes(5),
            revoked: false,
        };
        self.store.delegations.lock().unwrap().push(grant.clone());

        ExecutionContext {
            actor: self.agent.principal_id,
            principal_kind: Some(PrincipalKind::Agent),
            delegation_id: Some(grant.id),
            delegation_chain: Some(DelegationChain {
                root: self.delegation_root,
                grants: vec![grant],
            }),
            workspace_path: self.workspace.path().to_path_buf(),
            timestamp: now,
        }
    }

    fn prepare_for(&mut self, operation: &str) {
        match operation {
            "content.approve" => {
                let object = self.primary_object.as_mut().expect("primary object");
                assert_eq!(object.status(), ObjectStatus::Draft);
                object.transition_to(ObjectStatus::Submitted).unwrap();
                save_json(
                    &self
                        .workspace
                        .path()
                        .join(".proof/data/objects")
                        .join(format!("{}.json", object.id)),
                    object,
                );
            }
            "changeset.commit" => self.prepare_changeset(),
            _ => {}
        }
    }

    fn prepare_changeset(&mut self) {
        assert!(!self.bindings.contains_key("changeset_id"));
        let schema = self.schema.as_ref().expect("schema");
        let existing = self
            .primary_object
            .as_ref()
            .expect("primary object")
            .clone();
        assert_eq!(existing.status(), ObjectStatus::Approved);

        let created = Object::create(
            schema,
            "en-US",
            json!({"title": "Committed through the conformance ChangeSet"}),
        )
        .unwrap();
        let mut base_state = BaseState::new();
        base_state.insert(existing.id, existing);
        let mut changeset = ChangeSet::new(
            "Create a committed conformance object",
            &base_state,
            vec![ChangeSetEdit::ObjectCreate(ObjectCreateEdit {
                object: created.clone(),
            })],
        );
        changeset.transition_to(ChangeSetStatus::Submitted).unwrap();
        changeset.transition_to(ChangeSetStatus::Approved).unwrap();
        save_json(
            &self
                .workspace
                .path()
                .join(".proof/data/changesets")
                .join(format!("{}.json", changeset.id)),
            &changeset,
        );

        self.bindings
            .insert("changeset_id".to_string(), json!(changeset.id));
        self.bindings
            .insert("committed_object_id".to_string(), json!(created.id));
    }

    fn materialize(&mut self, operation: &str, input: &Value, outcome: &ExecutionOutcome) {
        match operation {
            "schema.create" => {
                let schema: SchemaDefinition = serde_json::from_value(input.clone()).unwrap();
                assert_eq!(outcome.output["data"]["schema_id"], json!(schema.id));
                save_json(
                    &self
                        .workspace
                        .path()
                        .join(".proof/data/schemas")
                        .join(format!("{}-{}.json", schema.id, schema.version)),
                    &schema,
                );
                self.bindings
                    .insert("schema".to_string(), serde_json::to_value(&schema).unwrap());
                self.schema = Some(schema);
            }
            "object.create" => {
                let object = object_from_outcome(outcome);
                save_json(
                    &self
                        .workspace
                        .path()
                        .join(".proof/data/objects")
                        .join(format!("{}.json", object.id)),
                    &object,
                );
                self.bindings
                    .insert("primary_object_id".to_string(), json!(object.id));
                self.primary_object = Some(object);
            }
            "object.edit" | "content.approve" => {
                self.primary_object = Some(object_from_outcome(outcome));
            }
            "changeset.commit" => {
                let object_id = binding_uuid(&self.bindings, "committed_object_id");
                let object: Object = serde_json::from_slice(
                    &std::fs::read(
                        self.workspace
                            .path()
                            .join(".proof/data/objects")
                            .join(format!("{object_id}.json")),
                    )
                    .unwrap(),
                )
                .unwrap();
                assert_eq!(object.status(), ObjectStatus::Committed);
            }
            "edition.create" => {
                let edition_id = outcome.output["data"]["edition"]["id"]
                    .as_str()
                    .expect("edition id");
                self.bindings
                    .insert("edition_id".to_string(), json!(edition_id));
            }
            _ => {}
        }
    }
}

fn frozen_operations() -> BTreeSet<(String, String)> {
    [
        "schema.create",
        "object.create",
        "object.edit",
        "content.approve",
        "content.release",
        "changeset.commit",
        "release.publish",
        "edition.create",
    ]
    .into_iter()
    .map(|operation| (operation.to_string(), "v1".to_string()))
    .collect()
}

fn resolve(value: &Value, bindings: &BTreeMap<String, Value>) -> Value {
    match value {
        Value::String(value) if value.starts_with('$') => bindings
            .get(&value[1..])
            .cloned()
            .unwrap_or_else(|| panic!("missing fixture binding `{value}`")),
        Value::Array(values) => Value::Array(
            values
                .iter()
                .map(|value| resolve(value, bindings))
                .collect(),
        ),
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, value)| (key.clone(), resolve(value, bindings)))
                .collect(),
        ),
        _ => value.clone(),
    }
}

fn binding_uuid(bindings: &BTreeMap<String, Value>, name: &str) -> Uuid {
    Uuid::parse_str(
        bindings[name]
            .as_str()
            .unwrap_or_else(|| panic!("fixture binding `{name}` is not a string")),
    )
    .unwrap()
}

fn save_json(path: &Path, value: &impl serde::Serialize) {
    std::fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
}

fn object_from_outcome(outcome: &ExecutionOutcome) -> Object {
    serde_json::from_value(outcome.output["data"]["object"].clone()).unwrap()
}

fn fixture_state(workspace: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut state = BTreeMap::new();
    for directory in ["schemas", "objects", "changesets", "editions"] {
        let path = workspace.join(".proof/data").join(directory);
        if !path.exists() {
            continue;
        }
        for entry in std::fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_file() {
                state.insert(
                    PathBuf::from(directory).join(path.file_name().unwrap()),
                    std::fs::read(path).unwrap(),
                );
            }
        }
    }
    state
}

fn execute_case(
    fixture: &Fixture,
    case: &ContentOperationCase,
    input: &Value,
    context: &ExecutionContext,
) -> ExecutionOutcome {
    match (case.governance, case.approval) {
        (Governance::AgentExecutable, ApprovalMode::None) => fixture
            .engine
            .execute_evidenced(&case.operation, &case.version, input, context)
            .unwrap_or_else(|error| panic!("{} failed: {error}", case.id)),
        (Governance::HumanOnly, ApprovalMode::SignedHuman) => {
            let proof_count = fixture.store.proofs.lock().unwrap().len();
            let state = fixture_state(fixture.workspace.path());
            assert_eq!(
                fixture
                    .engine
                    .execute_evidenced(&case.operation, &case.version, input, context)
                    .unwrap_err(),
                ExecutionError::HumanOnly,
                "{} must reject an unapproved agent call",
                case.id
            );
            assert_eq!(fixture.store.proofs.lock().unwrap().len(), proof_count);
            assert_eq!(fixture_state(fixture.workspace.path()), state);

            let request = SignedApprovalRequest::create(
                case.operation.clone(),
                case.version.clone(),
                input,
                context.timestamp - Duration::seconds(1),
                context.timestamp + Duration::minutes(1),
                &fixture.agent,
            )
            .unwrap();
            let decision = SignedApprovalDecision::create(
                &request,
                ApprovalOutcome::Approved,
                Some(format!("approve conformance vector {}", case.id)),
                context.timestamp,
                &fixture.approver,
            )
            .unwrap();
            let trusted_approver = principal_from_keypair(&fixture.approver);
            let grant = ApprovalGrant {
                request,
                decision,
                approver: trusted_approver.clone(),
            };
            fixture
                .engine
                .execute_with_approval_evidenced(
                    &case.operation,
                    &case.version,
                    input,
                    context,
                    &grant,
                    &trusted_approver,
                )
                .unwrap_or_else(|error| panic!("approved {} failed: {error}", case.id))
        }
        pair => panic!("governance/approval mismatch for {}: {pair:?}", case.id),
    }
}

fn assert_engine_evidence(
    fixture: &Fixture,
    case: &ContentOperationCase,
    input: &Value,
    context: &ExecutionContext,
    outcome: &ExecutionOutcome,
) {
    assert_eq!(outcome.output["operation"], case.operation);
    assert_eq!(
        outcome.proof.body.operation,
        format!("{}::{}", case.operation, case.version)
    );
    assert_eq!(outcome.proof.body.actor, fixture.agent.principal_id);
    assert_eq!(outcome.proof.body.delegation_id, context.delegation_id);
    assert_eq!(outcome.proof.body.timestamp, context.timestamp);
    assert_eq!(outcome.proof.body.id.get_version_num(), 7);

    let verifying_key = fixture.agent.signing_key.verifying_key();
    outcome.proof.verify(&verifying_key).unwrap();
    let canonical_input = canonicalize(input).unwrap();
    let canonical_output = canonicalize(&outcome.output).unwrap();
    assert_eq!(
        outcome.proof.body.input_digest,
        digest(ArtifactKind::OperationInput, &canonical_input)
    );
    assert_eq!(
        outcome.proof.body.output_digest,
        digest(ArtifactKind::OperationOutput, &canonical_output)
    );

    let stored = fixture
        .store
        .proofs
        .lock()
        .unwrap()
        .iter()
        .filter(|proof| proof.body.id == outcome.proof.body.id)
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(stored, vec![outcome.proof.clone()]);
}

fn assert_expected_values(
    case: &ContentOperationCase,
    bindings: &BTreeMap<String, Value>,
    outcome: &ExecutionOutcome,
) {
    for expected in &case.expected {
        let expected_value = resolve(&expected.value, bindings);
        assert_eq!(
            outcome.output.pointer(&expected.pointer),
            Some(&expected_value),
            "output mismatch for {} at {}",
            case.id,
            expected.pointer
        );
    }
}

fn assert_exact_replay(
    fixture: &Fixture,
    case: &ContentOperationCase,
    input: &Value,
    context: &ExecutionContext,
    original: &ExecutionOutcome,
) {
    let retry = resolve(
        case.retry_input.as_ref().expect("exact retry input"),
        &fixture.bindings,
    );
    let conflict = resolve(
        case.conflict_input.as_ref().expect("conflict input"),
        &fixture.bindings,
    );
    assert_eq!(canonicalize(input).unwrap(), canonicalize(&retry).unwrap());
    assert_ne!(
        canonicalize(input).unwrap(),
        canonicalize(&conflict).unwrap()
    );
    let key = Uuid::parse_str(input["idempotency_key"].as_str().unwrap()).unwrap();
    assert_eq!(key.get_version_num(), 7);

    let state_after_first = fixture_state(fixture.workspace.path());
    let proof_count = fixture.store.proofs.lock().unwrap().len();
    let context_count = fixture.store.contexts.lock().unwrap().len();
    let replayed = fixture
        .engine
        .execute_evidenced(&case.operation, &case.version, &retry, context)
        .unwrap();
    assert_eq!(replayed.output, original.output);
    assert_eq!(replayed.proof.body.id, original.proof.body.id);
    assert_eq!(replayed.proof.body, original.proof.body);
    assert_eq!(replayed.proof.signature, original.proof.signature);
    assert_eq!(replayed.proof, original.proof);
    assert_eq!(fixture_state(fixture.workspace.path()), state_after_first);
    assert_eq!(fixture.store.proofs.lock().unwrap().len(), proof_count);
    assert_eq!(fixture.store.contexts.lock().unwrap().len(), context_count);

    assert_eq!(
        fixture
            .engine
            .execute_evidenced(&case.operation, &case.version, &conflict, context)
            .unwrap_err(),
        ExecutionError::Idempotency(IdempotencyError::Conflict)
    );
    assert_eq!(fixture_state(fixture.workspace.path()), state_after_first);
    assert_eq!(fixture.store.proofs.lock().unwrap().len(), proof_count);
    assert_eq!(fixture.store.contexts.lock().unwrap().len(), context_count);
}

#[test]
fn content_v1_json_vectors_execute_governed_engine_and_exact_replay() {
    let file: ContentCaseFile = serde_json::from_value(
        load_case(project_root().join("conformance/cases/content_governance.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(file.name, "content-v1-governed-walking-skeleton");
    assert_eq!(file.cases.len(), 8);

    let registry = Registry::load_from_directory(project_root().join("registry/content")).unwrap();
    assert_eq!(registry.operations().len(), 8);
    assert!(registry
        .operations()
        .iter()
        .all(|entry| entry.status == VersionStatus::Active && entry.version == "v1"));
    let registry_operations = registry
        .operations()
        .iter()
        .map(|entry| (entry.operation.clone(), entry.version.clone()))
        .collect::<BTreeSet<_>>();
    let vector_operations = file
        .cases
        .iter()
        .map(|case| (case.operation.clone(), case.version.clone()))
        .collect::<BTreeSet<_>>();
    assert_eq!(registry_operations, frozen_operations());
    assert_eq!(vector_operations, frozen_operations());
    assert!(registry.find("changeset.create", "v1").is_none());
    assert!(!file
        .cases
        .iter()
        .any(|case| case.operation == "changeset.create"));

    let case_ids = file
        .cases
        .iter()
        .map(|case| case.id.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(case_ids.len(), 8);
    for case in &file.cases {
        let entry = registry
            .find(&case.operation, &case.version)
            .unwrap_or_else(|| panic!("registry entry missing for {}", case.id));
        assert_eq!(entry.domain, case.domain);
        assert_eq!(entry.governance, case.governance);
        assert_eq!(entry.required_authority, case.authority.kind);
        assert_eq!(entry.idempotency, case.idempotency);
    }

    let handlers = content_handlers();
    let handler_policies = handlers
        .iter()
        .map(|handler| {
            (
                handler.operation().to_string(),
                handler.idempotency_policy(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let exact_operations = file
        .cases
        .iter()
        .filter(|case| case.replay == ReplayMode::ExactOriginal)
        .map(|case| case.operation.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        exact_operations,
        BTreeSet::from(["changeset.commit", "edition.create"])
    );
    for case in &file.cases {
        let expected_policy = match case.replay {
            ReplayMode::None => IdempotencyPolicy::None,
            ReplayMode::ExactOriginal => IdempotencyPolicy::RequiredUuidV7ExactReplay,
        };
        assert_eq!(handler_policies[&case.operation], expected_policy);
        match case.replay {
            ReplayMode::None => {
                assert!(case.retry_input.is_none());
                assert!(case.conflict_input.is_none());
                assert!(case.input.get("idempotency_key").is_none());
            }
            ReplayMode::ExactOriginal => {
                assert!(case.retry_input.is_some());
                assert!(case.conflict_input.is_some());
            }
        }
    }

    let mut fixture = Fixture::new(registry, handlers);
    assert_eq!(fixture.engine.handler_count(), 8);
    for case in &file.cases {
        fixture.prepare_for(&case.operation);
        let input = resolve(&case.input, &fixture.bindings);
        let context = fixture.context_for(&case.operation, &case.domain, &case.authority);
        let proof_count = fixture.store.proofs.lock().unwrap().len();
        let outcome = execute_case(&fixture, case, &input, &context);
        assert_eq!(fixture.store.proofs.lock().unwrap().len(), proof_count + 1);
        assert_engine_evidence(&fixture, case, &input, &context, &outcome);
        assert_expected_values(case, &fixture.bindings, &outcome);
        if case.replay == ReplayMode::ExactOriginal {
            assert_exact_replay(&fixture, case, &input, &context, &outcome);
        }
        fixture.materialize(&case.operation, &input, &outcome);
    }

    assert_eq!(fixture.store.proofs.lock().unwrap().len(), 8);
    assert_eq!(fixture.store.contexts.lock().unwrap().len(), 8);
    assert_eq!(fixture.store.delegations.lock().unwrap().len(), 8);
}
