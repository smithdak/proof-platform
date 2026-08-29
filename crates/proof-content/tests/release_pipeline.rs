use proof_content::{content_handlers, verify_release, ContentChange, ReleasePipeline};
use proof_kernel::{ExecutionContext, ExecutionEngine, Governance, PrincipalKind, Registry};
use serde_json::{json, Value};
use std::path::PathBuf;

fn schema_json() -> Value {
    json!({
        "id": uuid::Uuid::now_v7().to_string(),
        "name": "Article",
        "version": 1,
        "fields": [{"name": "title", "field_type": "text", "required": true, "localized": true}],
        "created_at": "2026-01-01T00:00:00Z"
    })
}

fn engine() -> ExecutionEngine {
    let registry =
        Registry::load_from_directory(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("registry"))
            .unwrap();
    let mut engine = ExecutionEngine::new(registry);
    for handler in content_handlers() {
        engine.register_handler(handler);
    }
    engine
}

fn context() -> (proof_kernel::Keypair, ExecutionContext) {
    let keypair = proof_kernel::generate_keypair_for(PrincipalKind::Human);
    let execution_context = ExecutionContext {
        actor: keypair.principal_id,
        principal_kind: Some(PrincipalKind::Human),
        delegation_id: None,
        delegation_chain: None,
        workspace_path: PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        timestamp: chrono::Utc::now(),
    };
    (keypair, execution_context)
}

#[test]
fn release_publish_registry_is_human_only() {
    let registry =
        Registry::load_from_directory(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("registry"))
            .unwrap();

    assert_eq!(
        registry.find("release.publish", "v1").unwrap().governance,
        Governance::HumanOnly
    );
}

#[test]
fn publishes_and_verifies_manifest_with_governed_proofs() {
    let (keypair, execution_context) = context();
    let registry_engine = engine();
    let pipeline = ReleasePipeline::new_with_keypair(&registry_engine, keypair.clone());
    let output = pipeline
        .publish(
            "production",
            vec![ContentChange::create(
                schema_json(),
                "en-US",
                json!({"title": "First"}),
            )],
            &execution_context,
        )
        .unwrap();

    assert_eq!(output.objects.len(), 1);
    assert_eq!(output.manifest.entries.len(), 1);
    assert_eq!(output.change_proofs.len(), 1);
    assert_eq!(output.release_proof.body.operation, "release.publish::v1");
    verify_release(&output.manifest, &output.objects).unwrap();

    let manifest_json = serde_json::to_value(&output.manifest).unwrap();
    assert_eq!(manifest_json["entries"].as_array().unwrap().len(), 1);
    let content_digest = output.manifest.entries[0].content_digest.clone();
    assert!(content_digest.starts_with("sha256:"));
}

#[test]
fn catches_missing_objects_and_modified_content() {
    let (keypair, execution_context) = context();
    let registry_engine = engine();
    let pipeline = ReleasePipeline::new_with_keypair(&registry_engine, keypair.clone());
    let mut output = pipeline
        .publish(
            "preview",
            vec![ContentChange::create(
                schema_json(),
                "en-US",
                json!({"title": "Stable"}),
            )],
            &execution_context,
        )
        .unwrap();

    verify_release(&output.manifest, &output.objects).unwrap();
    output.objects.clear();
    assert!(verify_release(&output.manifest, &output.objects).is_err());

    let (keypair, execution_context) = context();
    let registry_engine = engine();
    let pipeline = ReleasePipeline::new_with_keypair(&registry_engine, keypair.clone());
    let mut output = pipeline
        .publish(
            "preview",
            vec![ContentChange::create(
                schema_json(),
                "en-US",
                json!({"title": "Stable"}),
            )],
            &execution_context,
        )
        .unwrap();
    output.objects[0].content = json!({"title": "Tampered"});
    assert!(verify_release(&output.manifest, &output.objects).is_err());

    let duplicate_manifest = output.manifest.clone();
    let mut duplicated_manifest = duplicate_manifest;
    duplicated_manifest
        .entries
        .push(output.manifest.entries[0].clone());
    assert!(verify_release(&duplicated_manifest, &output.objects).is_err());
}
