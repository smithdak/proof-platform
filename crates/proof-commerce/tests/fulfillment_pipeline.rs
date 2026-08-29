use proof_commerce::{commerce_handlers, verify_fulfillment, FulfillmentPipeline, OrderLine};
use proof_kernel::{
    Delegation, DelegationChain, ExecutionContext, ExecutionEngine, Registry, RegistryEntry,
};
use serde_json::json;
use std::{fs, path::PathBuf};

fn engine() -> ExecutionEngine {
    let commerce_registry =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../registry/commerce");
    let mut entries = Vec::new();
    for path in fs::read_dir(commerce_registry).unwrap() {
        let path = path.unwrap().path();
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("catalog-") || name.starts_with("order-"))
            && path.extension().and_then(|extension| extension.to_str()) == Some("json")
            && !path.to_string_lossy().contains("input")
            && !path.to_string_lossy().contains("output")
        {
            let contents = fs::read_to_string(&path).unwrap();
            let entry: RegistryEntry = serde_json::from_str(&contents).unwrap();
            entries.push(entry);
        }
    }
    let registry = Registry::new(entries).unwrap();
    let mut engine = ExecutionEngine::new(registry);
    for handler in commerce_handlers() {
        engine.register_handler(handler);
    }
    engine
}

fn delegation_grant(
    keypair: &proof_kernel::Keypair,
    now: chrono::DateTime<chrono::Utc>,
) -> Delegation {
    Delegation {
        id: uuid::Uuid::now_v7(),
        issuer: keypair.principal_id,
        recipient: keypair.principal_id,
        allowed_actions: vec!["*".to_string()],
        resource_scope: vec!["*".to_string()],
        scope: Default::default(),
        valid_from: now - chrono::Duration::seconds(1),
        valid_until: now + chrono::Duration::seconds(1),
        revoked: false,
    }
}

fn context() -> (proof_kernel::Keypair, ExecutionContext) {
    let keypair = proof_kernel::generate_keypair_for(proof_kernel::PrincipalKind::Human);
    let grant = delegation_grant(&keypair, chrono::Utc::now());
    let execution_context = ExecutionContext {
        actor: keypair.principal_id,
        principal_kind: Some(proof_kernel::PrincipalKind::Human),
        delegation_id: Some(grant.id),
        delegation_chain: Some(DelegationChain {
            root: keypair.principal_id,
            grants: vec![grant],
        }),
        workspace_path: PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        timestamp: chrono::Utc::now(),
    };
    (keypair, execution_context)
}

fn line() -> OrderLine {
    OrderLine::new(uuid::Uuid::now_v7(), "Widget", 2).unwrap()
}

#[test]
fn governance_blocks_agent_execution() {
    let (keypair, execution_context) = context();
    let mut agent_context = execution_context.clone();
    agent_context.principal_kind = Some(proof_kernel::PrincipalKind::Agent);
    let registry_engine = engine();
    let pipeline = FulfillmentPipeline::new_with_keypair(&registry_engine, keypair.clone());
    let output = pipeline.fulfill(vec![line()], &agent_context);
    assert!(matches!(
        output,
        Err(proof_kernel::ExecutionError::HumanOnly)
    ));
}

#[test]
fn fulfills_order_with_governed_step_proofs() {
    let (keypair, execution_context) = context();
    let registry_engine = engine();
    let pipeline = FulfillmentPipeline::new_with_keypair(&registry_engine, keypair.clone());
    let output = pipeline.fulfill(vec![line()], &execution_context).unwrap();

    assert_eq!(output.order.status, proof_commerce::OrderStatus::Fulfilled);
    assert_eq!(output.manifest.evidence.len(), 3);
    assert_eq!(output.evidence_proofs.len(), 3);
    let operations: Vec<_> = output
        .manifest
        .evidence
        .iter()
        .map(|evidence| evidence.operation.as_str())
        .collect();
    assert_eq!(
        operations,
        ["order.create", "order.approve", "order.fulfill"]
    );
    for evidence in &output.manifest.evidence {
        assert!(evidence.content_digest.starts_with("sha256:"));
        assert_eq!(
            evidence.record_digest,
            Some(evidence.content_digest.clone())
        );
    }
    assert_eq!(
        output.manifest.fulfillment_digest,
        proof_commerce::canonical_digest(&output.manifest.evidence)
    );
    verify_fulfillment(&output.manifest, &output.order).unwrap();
    assert!(registry_engine
        .is_agent_executable("order.approve", "v1")
        .is_ok());
    assert!(!registry_engine
        .is_agent_executable("order.approve", "v1")
        .unwrap());
}

#[test]
fn catches_modified_orders_and_invalid_manifests_when_human() {
    let (keypair, execution_context) = context();
    let registry_engine = engine();
    let pipeline = FulfillmentPipeline::new_with_keypair(&registry_engine, keypair.clone());
    let mut output = pipeline.fulfill(vec![line()], &execution_context).unwrap();

    output.order.lines[0].name = "Tampered".to_string();
    assert!(verify_fulfillment(&output.manifest, &output.order).is_err());

    let mut duplicate_manifest = output.manifest.clone();
    duplicate_manifest
        .evidence
        .push(output.manifest.evidence[0].clone());
    let mut original_order = output.order.clone();
    original_order.lines[0].name = "Widget".to_string();
    assert!(verify_fulfillment(&duplicate_manifest, &original_order).is_err());

    let mut wrong_id_manifest = output.manifest.clone();
    wrong_id_manifest.order_id = uuid::Uuid::now_v7();
    let mut matching_order = output.order.clone();
    matching_order.lines[0].name = "Widget".to_string();
    assert!(verify_fulfillment(&wrong_id_manifest, &matching_order).is_err());
}

#[test]
fn canonical_digest_is_stable_and_key_order_insensitive() {
    let first = json!({"b": 2, "a": {"d": 4, "c": 3}});
    let second = json!({"a": {"c": 3, "d": 4}, "b": 2});
    assert_eq!(
        proof_commerce::canonical_digest(&first),
        proof_commerce::canonical_digest(&second)
    );
}
