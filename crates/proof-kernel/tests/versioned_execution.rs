use chrono::{Duration, Utc};
use proof_kernel::delegation::DelegationScope;
use proof_kernel::{
    generate_keypair, Delegation, DelegationChain, ExecutionContext, ExecutionEngine,
    ExecutionError, Governance, IdempotencyPolicy, OperationHandler, PrincipalId, PrincipalKind,
    RecordingStore, Registry, RegistryEntry, VersionStatus,
};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

#[derive(Default)]
struct VersionAwareHandler {
    policy_versions: Mutex<Vec<String>>,
    executed_versions: Mutex<Vec<String>>,
}

impl OperationHandler for VersionAwareHandler {
    fn operation(&self) -> &str {
        "release.publish"
    }

    fn idempotency_policy_for(&self, version: &str) -> IdempotencyPolicy {
        self.policy_versions
            .lock()
            .unwrap()
            .push(version.to_string());
        match version {
            "v2" => IdempotencyPolicy::RequiredUuidV7ExactReplay,
            _ => self.idempotency_policy(),
        }
    }

    fn execute(&self, input: &Value, _context: &ExecutionContext) -> Result<Value, ExecutionError> {
        Ok(json!({"version": "legacy", "input": input}))
    }

    fn execute_versioned(
        &self,
        version: &str,
        input: &Value,
        _context: &ExecutionContext,
    ) -> Result<Value, ExecutionError> {
        self.executed_versions
            .lock()
            .unwrap()
            .push(version.to_string());
        Ok(json!({"version": version, "input": input}))
    }
}

fn registry_entry(version: &str, domain: &str) -> RegistryEntry {
    RegistryEntry {
        operation: "release.publish".to_string(),
        domain: domain.to_string(),
        version: version.to_string(),
        action: "content:release_publish".to_string(),
        description: "Publish a release".to_string(),
        input_schema: "release.input.json".to_string(),
        output_schema: "release.output.json".to_string(),
        required_authority: "delegation-grant".to_string(),
        governance: Governance::AgentExecutable,
        idempotency: "required-uuidv7".to_string(),
        consequence: "content-release".to_string(),
        evidence_contract: "operation-effect-v1".to_string(),
        benchmark: None,
        status: VersionStatus::Active,
        deprecated_since: None,
        replacement_operation: None,
    }
}

fn context(actor: PrincipalId) -> ExecutionContext {
    ExecutionContext {
        actor,
        principal_kind: Some(PrincipalKind::Agent),
        delegation_id: None,
        delegation_chain: None,
        workspace_path: PathBuf::from("/tmp/version-aware-execution"),
        timestamp: Utc::now(),
    }
}

fn scoped_grant(context: &ExecutionContext) -> Delegation {
    Delegation {
        id: Uuid::now_v7(),
        issuer: PrincipalId::now(),
        recipient: context.actor,
        allowed_actions: vec!["*".to_string()],
        resource_scope: vec!["*".to_string()],
        scope: DelegationScope {
            allowed_operations: Some(vec!["release.publish".to_string()]),
            allowed_domains: Some(vec!["content".to_string()]),
            resource_scope: None,
        },
        valid_from: context.timestamp - Duration::minutes(1),
        valid_until: context.timestamp + Duration::minutes(1),
        revoked: false,
    }
}

fn chain_for(grant: Delegation) -> DelegationChain {
    DelegationChain {
        root: grant.issuer,
        grants: vec![grant],
    }
}

fn engine(
    entries: Vec<RegistryEntry>,
    keypair: &proof_kernel::Keypair,
    store: Option<Arc<RecordingStore>>,
    handler: Arc<VersionAwareHandler>,
) -> ExecutionEngine {
    let mut engine =
        ExecutionEngine::new_with_keypair(Registry::new(entries).unwrap(), keypair.clone());
    if let Some(store) = store {
        engine = engine.with_storage(store);
    }
    engine.register_handler(handler);
    engine
}

#[test]
fn requested_version_selects_distinct_policy_execution_and_replay_paths() {
    let keypair = generate_keypair();
    let store = Arc::new(RecordingStore::default());
    let handler = Arc::new(VersionAwareHandler::default());
    let engine = engine(
        vec![
            registry_entry("v1", "content"),
            registry_entry("v2", "content"),
        ],
        &keypair,
        Some(store),
        handler.clone(),
    );
    let context = context(keypair.principal_id);

    let first_v1 = engine
        .execute_evidenced("release.publish", "v1", &json!({"legacy": true}), &context)
        .unwrap();
    let second_v1 = engine
        .execute_evidenced("release.publish", "v1", &json!({"legacy": true}), &context)
        .unwrap();
    assert_eq!(first_v1.output["version"], "v1");
    assert_eq!(second_v1.output["version"], "v1");
    assert_ne!(first_v1.proof.body.id, second_v1.proof.body.id);

    let v2_input = json!({"idempotency_key": Uuid::now_v7(), "edition_id": Uuid::now_v7()});
    let first_v2 = engine
        .execute_evidenced("release.publish", "v2", &v2_input, &context)
        .unwrap();
    let replayed_v2 = engine
        .execute_evidenced("release.publish", "v2", &v2_input, &context)
        .unwrap();
    assert_eq!(replayed_v2, first_v2);
    assert_eq!(first_v2.output["version"], "v2");
    assert_eq!(first_v2.proof.body.operation, "release.publish::v2");

    assert_eq!(
        *handler.executed_versions.lock().unwrap(),
        ["v1", "v1", "v2"]
    );
    assert_eq!(
        *handler.policy_versions.lock().unwrap(),
        ["v1", "v1", "v2", "v2"]
    );
}

#[test]
fn explicit_loaded_delegation_enforces_operation_and_domain_before_handler_hooks() {
    for scope in [
        DelegationScope {
            allowed_operations: Some(vec!["release.other".to_string()]),
            allowed_domains: Some(vec!["content".to_string()]),
            resource_scope: None,
        },
        DelegationScope {
            allowed_operations: Some(vec!["release.publish".to_string()]),
            allowed_domains: Some(vec!["other".to_string()]),
            resource_scope: None,
        },
    ] {
        let keypair = generate_keypair();
        let store = Arc::new(RecordingStore::default());
        let handler = Arc::new(VersionAwareHandler::default());
        let engine = engine(
            vec![registry_entry("v1", "content")],
            &keypair,
            Some(store.clone()),
            handler.clone(),
        );
        let mut context = context(keypair.principal_id);
        let mut grant = scoped_grant(&context);
        grant.scope = scope;
        context.delegation_id = Some(grant.id);
        context.delegation_chain = Some(chain_for(grant.clone()));
        store.delegations.lock().unwrap().push(grant);

        assert_eq!(
            engine
                .execute("release.publish", "v1", &json!({}), &context)
                .unwrap_err(),
            ExecutionError::ScopeViolation
        );
        assert!(handler.policy_versions.lock().unwrap().is_empty());
        assert!(handler.executed_versions.lock().unwrap().is_empty());
    }
}

#[test]
fn explicit_loaded_delegation_must_match_the_chain_and_executing_actor() {
    let mutations: [fn(&mut Delegation, &ExecutionContext); 4] = [
        |grant, _| grant.scope.allowed_operations = Some(vec!["release.other".to_string()]),
        |grant, _| grant.recipient = PrincipalId::now(),
        |grant, _| grant.revoked = true,
        |grant, context| grant.valid_until = context.timestamp - Duration::seconds(1),
    ];

    for mutate in mutations {
        let keypair = generate_keypair();
        let store = Arc::new(RecordingStore::default());
        let handler = Arc::new(VersionAwareHandler::default());
        let engine = engine(
            vec![registry_entry("v1", "content")],
            &keypair,
            Some(store.clone()),
            handler.clone(),
        );
        let mut context = context(keypair.principal_id);
        let chain_grant = scoped_grant(&context);
        let mut stored_grant = chain_grant.clone();
        mutate(&mut stored_grant, &context);
        context.delegation_id = Some(chain_grant.id);
        context.delegation_chain = Some(chain_for(chain_grant));
        store.delegations.lock().unwrap().push(stored_grant);

        assert!(matches!(
            engine.execute("release.publish", "v1", &json!({}), &context),
            Err(ExecutionError::Delegation(_))
        ));
        assert!(handler.policy_versions.lock().unwrap().is_empty());
        assert!(handler.executed_versions.lock().unwrap().is_empty());
    }
}

#[test]
fn missing_stored_delegation_fails_before_handler_hooks() {
    let keypair = generate_keypair();
    let store = Arc::new(RecordingStore::default());
    let handler = Arc::new(VersionAwareHandler::default());
    let engine = engine(
        vec![registry_entry("v1", "content")],
        &keypair,
        Some(store),
        handler.clone(),
    );
    let mut context = context(keypair.principal_id);
    let grant = scoped_grant(&context);
    context.delegation_id = Some(grant.id);
    context.delegation_chain = Some(chain_for(grant));

    assert!(matches!(
        engine.execute("release.publish", "v1", &json!({}), &context),
        Err(ExecutionError::Delegation(_))
    ));
    assert!(handler.policy_versions.lock().unwrap().is_empty());
    assert!(handler.executed_versions.lock().unwrap().is_empty());
}

#[test]
fn legacy_chain_only_and_no_storage_explicit_id_callers_remain_supported() {
    for explicit_id in [false, true] {
        let keypair = generate_keypair();
        let handler = Arc::new(VersionAwareHandler::default());
        let engine = engine(
            vec![registry_entry("v1", "content")],
            &keypair,
            None,
            handler,
        );
        let mut context = context(keypair.principal_id);
        let grant = scoped_grant(&context);
        if explicit_id {
            context.delegation_id = Some(grant.id);
        }
        context.delegation_chain = Some(chain_for(grant));

        let output = engine
            .execute("release.publish", "v1", &json!({}), &context)
            .unwrap();
        assert_eq!(output["version"], "v1");
    }
}
