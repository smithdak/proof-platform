use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{Duration, Utc};
use proof_content::{
    content_handlers, verify_preview_approval_execution, verify_preview_publication, Edition,
    FieldType, Object, SchemaDefinition, SchemaField,
};
use proof_kernel::{
    generate_keypair_for, principal_from_keypair, ApprovalGrant, ApprovalOutcome, ExecutionContext,
    ExecutionEngine, ExecutionError, ExecutionReplayClaim, ExecutionReplayClaimResult,
    ExecutionStore, IdempotencyError, PrincipalKind, RecordingStore, Registry,
    SignedApprovalDecision, SignedApprovalRequest,
};
use serde_json::{json, Value};
use tempfile::TempDir;
use uuid::Uuid;

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn engine(store: Arc<dyn ExecutionStore>, keypair: proof_kernel::Keypair) -> ExecutionEngine {
    let registry =
        Registry::load_from_directory(repository_root().join("registry/content")).unwrap();
    let mut engine = ExecutionEngine::new_with_keypair(registry, keypair).with_storage(store);
    for handler in content_handlers() {
        engine.register_handler(handler);
    }
    engine
}

fn context(workspace: &Path, keypair: &proof_kernel::Keypair) -> ExecutionContext {
    ExecutionContext {
        actor: keypair.principal_id,
        principal_kind: Some(PrincipalKind::Agent),
        delegation_id: None,
        delegation_chain: None,
        workspace_path: workspace.to_path_buf(),
        timestamp: Utc::now(),
    }
}

fn schema() -> SchemaDefinition {
    SchemaDefinition::new(
        "Article",
        1,
        vec![SchemaField {
            name: "title".to_string(),
            field_type: FieldType::Text,
            required: true,
            localized: false,
            default_value: None,
        }],
    )
}

fn preview_workspace() -> (TempDir, Edition, String) {
    let workspace = TempDir::new().unwrap();
    let registry = workspace.path().join("registry/content");
    std::fs::create_dir_all(&registry).unwrap();
    std::fs::copy(
        repository_root().join("registry/content/release-publish-v2.input.json"),
        registry.join("release-publish-v2.input.json"),
    )
    .unwrap();
    let object = Object::create(&schema(), "en-US", json!({"title": "Synthetic"})).unwrap();
    let edition = Edition::new(Uuid::now_v7(), vec![object]);
    let edition_dir = workspace.path().join(".proof/data/editions");
    std::fs::create_dir_all(&edition_dir).unwrap();
    std::fs::write(
        edition_dir.join(format!("{}.json", edition.id)),
        serde_json::to_string(&edition).unwrap(),
    )
    .unwrap();
    let objects = edition.objects.clone();
    let manifest = json!({
        "schema": "proof-content-preview-manifest/v1",
        "edition_id": edition.id,
        "edition_content_digest": proof_content::digest::canonical_digest(&objects),
        "objects": objects.iter().map(|object| json!({
            "object_id": object.id,
            "locale": object.locale,
            "content_digest": proof_content::digest::canonical_digest(object),
        })).collect::<Vec<_>>(),
    });
    (
        workspace,
        edition,
        proof_content::digest::canonical_digest(&manifest),
    )
}

fn input(edition: &Edition, manifest_digest: &str, idempotency_key: Uuid) -> Value {
    json!({
        "idempotency_key": idempotency_key,
        "edition_id": edition.id,
        "environment": "preview",
        "version_label": "2026.08.30-rc1",
        "manifest_digest": manifest_digest,
    })
}

fn approval_for(
    input: &Value,
    context: &ExecutionContext,
    agent: &proof_kernel::Keypair,
) -> (ApprovalGrant, proof_kernel::Principal) {
    let human = generate_keypair_for(PrincipalKind::Human);
    let request = SignedApprovalRequest::create(
        "release.publish",
        "v2",
        input,
        context.timestamp - Duration::seconds(1),
        context.timestamp + Duration::minutes(1),
        agent,
    )
    .unwrap();
    let decision = SignedApprovalDecision::create(
        &request,
        ApprovalOutcome::Approved,
        None,
        context.timestamp,
        &human,
    )
    .unwrap();
    let principal = principal_from_keypair(&human);
    (
        ApprovalGrant {
            request,
            decision,
            approver: principal.clone(),
        },
        principal,
    )
}

#[test]
fn v2_publishes_once_replays_original_proof_and_verifies_artifact() {
    let (workspace, edition, manifest_digest) = preview_workspace();
    let agent = generate_keypair_for(PrincipalKind::Agent);
    let store = Arc::new(RecordingStore::default());
    let engine = engine(store, agent.clone());
    let context = context(workspace.path(), &agent);
    let request = input(&edition, &manifest_digest, Uuid::now_v7());
    let (approval, human) = approval_for(&request, &context, &agent);

    let first = engine
        .execute_with_approval_evidenced(
            "release.publish",
            "v2",
            &request,
            &context,
            &approval,
            &human,
        )
        .unwrap();
    let replay = engine
        .execute_with_approval_evidenced(
            "release.publish",
            "v2",
            &request,
            &context,
            &approval,
            &human,
        )
        .unwrap();
    assert_eq!(replay.output, first.output);
    assert_eq!(replay.proof, first.proof);
    assert_eq!(replay.proof.signature, first.proof.signature);
    let artifact_path = workspace.path().join(
        first.output["data"]["artifact"]["relative_path"]
            .as_str()
            .unwrap(),
    );
    assert!(artifact_path.exists());
    assert_eq!(
        std::fs::read_dir(artifact_path.parent().unwrap())
            .unwrap()
            .count(),
        1
    );
    verify_preview_publication(
        workspace.path(),
        &request,
        &first,
        &principal_from_keypair(&agent),
    )
    .unwrap();
    let approval_execution = proof_kernel::ApprovalExecution {
        request_id: approval.request.body.id,
        executed_at: context.timestamp,
        output: first.output.clone(),
        proof: first.proof.clone(),
    };
    verify_preview_approval_execution(
        &request,
        &approval_execution,
        &first,
        &approval,
        &agent,
        &human,
    )
    .unwrap();

    let changed = json!({
        "idempotency_key": request["idempotency_key"],
        "edition_id": edition.id,
        "environment": "preview",
        "version_label": "changed",
        "manifest_digest": manifest_digest,
    });
    let (changed_approval, changed_human) = approval_for(&changed, &context, &agent);
    assert_eq!(
        engine
            .execute_with_approval_evidenced(
                "release.publish",
                "v2",
                &changed,
                &context,
                &changed_approval,
                &changed_human,
            )
            .unwrap_err(),
        ExecutionError::Idempotency(IdempotencyError::Conflict)
    );
    assert_eq!(
        std::fs::read_dir(artifact_path.parent().unwrap())
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn v2_rejects_unknown_input_and_tampered_artifact_or_output() {
    let (workspace, edition, manifest_digest) = preview_workspace();
    let agent = generate_keypair_for(PrincipalKind::Agent);
    let store = Arc::new(RecordingStore::default());
    let engine = engine(store, agent.clone());
    let context = context(workspace.path(), &agent);
    let mut request = input(&edition, &manifest_digest, Uuid::now_v7());
    request
        .as_object_mut()
        .unwrap()
        .insert("unknown".to_string(), json!(true));
    let (approval, human) = approval_for(&request, &context, &agent);
    assert!(engine
        .execute_with_approval_evidenced(
            "release.publish",
            "v2",
            &request,
            &context,
            &approval,
            &human
        )
        .is_err());

    let request = input(&edition, &manifest_digest, Uuid::now_v7());
    let (approval, human) = approval_for(&request, &context, &agent);
    let outcome = engine
        .execute_with_approval_evidenced(
            "release.publish",
            "v2",
            &request,
            &context,
            &approval,
            &human,
        )
        .unwrap();
    let path = workspace.path().join(
        outcome.output["data"]["artifact"]["relative_path"]
            .as_str()
            .unwrap(),
    );
    let original_bytes = std::fs::read(&path).unwrap();
    std::fs::write(&path, b"{}" as &[u8]).unwrap();
    assert!(verify_preview_publication(
        workspace.path(),
        &request,
        &outcome,
        &principal_from_keypair(&agent)
    )
    .is_err());
    std::fs::write(&path, original_bytes).unwrap();
    let mut changed_output = outcome.clone();
    changed_output.output["data"]["environment"] = json!("production");
    assert!(verify_preview_publication(
        workspace.path(),
        &request,
        &changed_output,
        &principal_from_keypair(&agent)
    )
    .is_err());
    let mut version_tamper = outcome.clone();
    version_tamper.output["data"]["version_label"] = json!("2026.08.30-rc2");
    assert!(verify_preview_publication(
        workspace.path(),
        &request,
        &version_tamper,
        &principal_from_keypair(&agent)
    )
    .is_err());
    let mut manifest_tamper = outcome.clone();
    manifest_tamper.output["data"]["manifest_digest"] =
        json!("sha256:0000000000000000000000000000000000000000000000000000000000000000");
    assert!(verify_preview_publication(
        workspace.path(),
        &request,
        &manifest_tamper,
        &principal_from_keypair(&agent)
    )
    .is_err());
    let mut changed_proof = outcome.clone();
    changed_proof.proof.body.operation = "release.publish::v1".to_string();
    assert!(verify_preview_publication(
        workspace.path(),
        &request,
        &changed_proof,
        &principal_from_keypair(&agent)
    )
    .is_err());
}

#[test]
fn v2_registry_row_and_schemas_match_the_frozen_contract() {
    let registry =
        Registry::load_from_directory(repository_root().join("registry/content")).unwrap();
    let entry = registry.find("release.publish", "v2").unwrap();
    assert_eq!(entry.operation, "release.publish");
    assert_eq!(entry.domain, "content");
    assert_eq!(entry.version, "v2");
    assert_eq!(entry.action, "content:release_publish");
    assert_eq!(
        entry.description,
        "Publish one existing content edition as an immutable local preview artifact"
    );
    assert_eq!(entry.input_schema, "content/release-publish-v2.input.json");
    assert_eq!(
        entry.output_schema,
        "content/release-publish-v2.output.json"
    );
    assert_eq!(entry.required_authority, "delegation-grant");
    assert_eq!(entry.governance, proof_kernel::Governance::HumanOnly);
    assert_eq!(entry.idempotency, "required-uuidv7");
    assert_eq!(entry.consequence, "content-release");
    assert_eq!(entry.evidence_contract, "operation-effect-v1");
    assert_eq!(entry.benchmark.as_deref(), Some("B1"));
    assert_eq!(entry.status, proof_kernel::VersionStatus::Active);
    assert_eq!(entry.deprecated_since, None);
    assert_eq!(entry.replacement_operation, None);

    for file in [
        "release-publish-v2.input.json",
        "release-publish-v2.output.json",
    ] {
        let schema: Value = serde_json::from_str(
            &std::fs::read_to_string(repository_root().join("registry/content").join(file))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(schema["additionalProperties"], false);
    }
    let output: Value = serde_json::from_str(
        &std::fs::read_to_string(
            repository_root().join("registry/content/release-publish-v2.output.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(output["properties"]["data"]["additionalProperties"], false);
    assert_eq!(
        output["properties"]["data"]["properties"]["artifact"]["additionalProperties"],
        false
    );
}

#[test]
fn v2_failed_claim_is_indeterminate_and_creates_no_artifact() {
    let (workspace, edition, _manifest_digest) = preview_workspace();
    let agent = generate_keypair_for(PrincipalKind::Agent);
    let engine = engine(Arc::new(RecordingStore::default()), agent.clone());
    let context = context(workspace.path(), &agent);
    let request = input(
        &edition,
        "sha256:0000000000000000000000000000000000000000000000000000000000000000",
        Uuid::now_v7(),
    );
    let (approval, human) = approval_for(&request, &context, &agent);
    assert!(matches!(
        engine.execute_with_approval_evidenced(
            "release.publish",
            "v2",
            &request,
            &context,
            &approval,
            &human
        ),
        Err(ExecutionError::HandlerFailed(_))
    ));
    assert_eq!(
        engine
            .execute_with_approval_evidenced(
                "release.publish",
                "v2",
                &request,
                &context,
                &approval,
                &human
            )
            .unwrap_err(),
        ExecutionError::Idempotency(IdempotencyError::Indeterminate)
    );
    assert!(!workspace.path().join(".proof/data/previews").exists());
}

#[test]
fn v2_rejects_noncanonical_uuid_spellings_before_preview_write() {
    let (workspace, edition, manifest_digest) = preview_workspace();
    let agent = generate_keypair_for(PrincipalKind::Agent);
    let engine = engine(Arc::new(RecordingStore::default()), agent.clone());
    let context = context(workspace.path(), &agent);

    let mut uppercase = input(&edition, &manifest_digest, Uuid::now_v7());
    uppercase["idempotency_key"] = json!(uppercase["idempotency_key"]
        .as_str()
        .unwrap()
        .to_uppercase());
    let (approval, human) = approval_for(&uppercase, &context, &agent);
    assert!(matches!(
        engine.execute_with_approval_evidenced(
            "release.publish",
            "v2",
            &uppercase,
            &context,
            &approval,
            &human
        ),
        Err(ExecutionError::HandlerFailed(_))
    ));

    let mut simple = input(&edition, &manifest_digest, Uuid::now_v7());
    simple["idempotency_key"] = json!(Uuid::parse_str(simple["idempotency_key"].as_str().unwrap())
        .unwrap()
        .simple()
        .to_string());
    let (approval, human) = approval_for(&simple, &context, &agent);
    assert!(matches!(
        engine.execute_with_approval_evidenced(
            "release.publish",
            "v2",
            &simple,
            &context,
            &approval,
            &human
        ),
        Err(ExecutionError::HandlerFailed(_))
    ));
    assert!(!workspace.path().join(".proof/data/previews").exists());
}

#[test]
fn v2_rejects_unknown_persisted_edition_and_object_fields() {
    let (workspace, edition, manifest_digest) = preview_workspace();
    let agent = generate_keypair_for(PrincipalKind::Agent);
    let engine = engine(Arc::new(RecordingStore::default()), agent.clone());
    let context = context(workspace.path(), &agent);
    let edition_path = workspace
        .path()
        .join(".proof/data/editions")
        .join(format!("{}.json", edition.id));

    let mut raw: Value =
        serde_json::from_str(&std::fs::read_to_string(&edition_path).unwrap()).unwrap();
    raw["unexpected"] = json!(true);
    std::fs::write(&edition_path, serde_json::to_vec(&raw).unwrap()).unwrap();
    let request = input(&edition, &manifest_digest, Uuid::now_v7());
    let (approval, human) = approval_for(&request, &context, &agent);
    assert!(matches!(
        engine.execute_with_approval_evidenced(
            "release.publish",
            "v2",
            &request,
            &context,
            &approval,
            &human
        ),
        Err(ExecutionError::HandlerFailed(_))
    ));

    raw.as_object_mut().unwrap().remove("unexpected");
    raw["objects"][0]["unexpected"] = json!(true);
    std::fs::write(&edition_path, serde_json::to_vec(&raw).unwrap()).unwrap();
    let request = input(&edition, &manifest_digest, Uuid::now_v7());
    let (approval, human) = approval_for(&request, &context, &agent);
    assert!(matches!(
        engine.execute_with_approval_evidenced(
            "release.publish",
            "v2",
            &request,
            &context,
            &approval,
            &human
        ),
        Err(ExecutionError::HandlerFailed(_))
    ));
    assert!(!workspace.path().join(".proof/data/previews").exists());
}

#[test]
fn verifier_requires_one_expected_final_artifact_and_ignores_regular_temp_evidence() {
    let (workspace, edition, manifest_digest) = preview_workspace();
    let agent = generate_keypair_for(PrincipalKind::Agent);
    let engine = engine(Arc::new(RecordingStore::default()), agent.clone());
    let context = context(workspace.path(), &agent);
    let request = input(&edition, &manifest_digest, Uuid::now_v7());
    let (approval, human) = approval_for(&request, &context, &agent);
    let outcome = engine
        .execute_with_approval_evidenced(
            "release.publish",
            "v2",
            &request,
            &context,
            &approval,
            &human,
        )
        .unwrap();
    let directory = workspace
        .path()
        .join(".proof/data/previews")
        .join(edition.id.to_string());
    let expected_final = format!("{}.json", request["idempotency_key"].as_str().unwrap());
    let valid_temp = format!(".{expected_final}.{}.tmp", Uuid::now_v7());
    std::fs::write(directory.join(&valid_temp), b"failed temporary evidence").unwrap();
    verify_preview_publication(
        workspace.path(),
        &request,
        &outcome,
        &principal_from_keypair(&agent),
    )
    .unwrap();

    for rejected_temp in [
        ".preserved.tmp".to_string(),
        format!(".{}.json.{}.tmp", Uuid::now_v7(), Uuid::now_v7()),
        format!(".{expected_final}.{}.tmp", Uuid::nil()),
        format!(
            ".{expected_final}.{}.tmp",
            Uuid::now_v7().to_string().to_uppercase()
        ),
    ] {
        let path = directory.join(&rejected_temp);
        std::fs::write(&path, b"malformed temporary evidence").unwrap();
        assert!(verify_preview_publication(
            workspace.path(),
            &request,
            &outcome,
            &principal_from_keypair(&agent),
        )
        .is_err());
        std::fs::remove_file(path).unwrap();
    }

    std::fs::write(directory.join("extra.json"), b"extra final artifact").unwrap();
    assert!(verify_preview_publication(
        workspace.path(),
        &request,
        &outcome,
        &principal_from_keypair(&agent),
    )
    .is_err());
    std::fs::remove_file(directory.join("extra.json")).unwrap();
    std::fs::create_dir(directory.join(".non-regular.tmp")).unwrap();
    assert!(verify_preview_publication(
        workspace.path(),
        &request,
        &outcome,
        &principal_from_keypair(&agent),
    )
    .is_err());
}

#[cfg(unix)]
#[test]
fn verifier_rejects_symlink_entry_when_counting_preview_artifacts() {
    use std::os::unix::fs::symlink;

    let (workspace, edition, manifest_digest) = preview_workspace();
    let agent = generate_keypair_for(PrincipalKind::Agent);
    let engine = engine(Arc::new(RecordingStore::default()), agent.clone());
    let context = context(workspace.path(), &agent);
    let request = input(&edition, &manifest_digest, Uuid::now_v7());
    let (approval, human) = approval_for(&request, &context, &agent);
    let outcome = engine
        .execute_with_approval_evidenced(
            "release.publish",
            "v2",
            &request,
            &context,
            &approval,
            &human,
        )
        .unwrap();
    let directory = workspace
        .path()
        .join(".proof/data/previews")
        .join(edition.id.to_string());
    symlink("missing-target", directory.join(".symlink.tmp")).unwrap();
    assert!(verify_preview_publication(
        workspace.path(),
        &request,
        &outcome,
        &principal_from_keypair(&agent),
    )
    .is_err());
}

#[test]
fn v2_rejects_unequal_existing_artifact_without_overwrite() {
    let (workspace, edition, manifest_digest) = preview_workspace();
    let agent = generate_keypair_for(PrincipalKind::Agent);
    let engine = engine(Arc::new(RecordingStore::default()), agent.clone());
    let context = context(workspace.path(), &agent);
    let request = input(&edition, &manifest_digest, Uuid::now_v7());
    let path = workspace
        .path()
        .join(".proof/data/previews")
        .join(edition.id.to_string())
        .join(format!(
            "{}.json",
            request["idempotency_key"].as_str().unwrap()
        ));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"unequal existing artifact").unwrap();
    let (approval, human) = approval_for(&request, &context, &agent);
    assert!(matches!(
        engine.execute_with_approval_evidenced(
            "release.publish",
            "v2",
            &request,
            &context,
            &approval,
            &human
        ),
        Err(ExecutionError::HandlerFailed(_))
    ));
    assert_eq!(std::fs::read(&path).unwrap(), b"unequal existing artifact");
}

#[cfg(unix)]
#[test]
fn v2_rejects_preview_directory_and_final_file_symlink_escapes() {
    use std::os::unix::fs::symlink;

    let (workspace, edition, manifest_digest) = preview_workspace();
    let outside = TempDir::new().unwrap();
    let agent = generate_keypair_for(PrincipalKind::Agent);
    let publish_engine = engine(Arc::new(RecordingStore::default()), agent.clone());
    let publish_context = context(workspace.path(), &agent);
    let preview_root = workspace.path().join(".proof/data/previews");
    std::fs::create_dir_all(&preview_root).unwrap();
    symlink(outside.path(), preview_root.join(edition.id.to_string())).unwrap();
    let request = input(&edition, &manifest_digest, Uuid::now_v7());
    let (approval, human) = approval_for(&request, &publish_context, &agent);
    assert!(publish_engine
        .execute_with_approval_evidenced(
            "release.publish",
            "v2",
            &request,
            &publish_context,
            &approval,
            &human
        )
        .is_err());
    assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);

    std::fs::remove_file(preview_root.join(edition.id.to_string())).unwrap();
    let request = input(&edition, &manifest_digest, Uuid::now_v7());
    let final_dir = preview_root.join(edition.id.to_string());
    std::fs::create_dir_all(&final_dir).unwrap();
    let outside_file = outside.path().join("outside.json");
    std::fs::write(&outside_file, b"outside").unwrap();
    symlink(
        &outside_file,
        final_dir.join(format!(
            "{}.json",
            request["idempotency_key"].as_str().unwrap()
        )),
    )
    .unwrap();
    let (approval, human) = approval_for(&request, &publish_context, &agent);
    assert!(publish_engine
        .execute_with_approval_evidenced(
            "release.publish",
            "v2",
            &request,
            &publish_context,
            &approval,
            &human
        )
        .is_err());
    assert_eq!(std::fs::read(&outside_file).unwrap(), b"outside");

    let (verify_workspace, verify_edition, verify_manifest) = preview_workspace();
    let verify_agent = generate_keypair_for(PrincipalKind::Agent);
    let verify_engine = engine(Arc::new(RecordingStore::default()), verify_agent.clone());
    let verify_context = context(verify_workspace.path(), &verify_agent);
    let verify_request = input(&verify_edition, &verify_manifest, Uuid::now_v7());
    let (verify_approval, verify_human) =
        approval_for(&verify_request, &verify_context, &verify_agent);
    let outcome = verify_engine
        .execute_with_approval_evidenced(
            "release.publish",
            "v2",
            &verify_request,
            &verify_context,
            &verify_approval,
            &verify_human,
        )
        .unwrap();
    let artifact = verify_workspace.path().join(
        outcome.output["data"]["artifact"]["relative_path"]
            .as_str()
            .unwrap(),
    );
    std::fs::remove_file(&artifact).unwrap();
    symlink(&outside_file, &artifact).unwrap();
    assert!(verify_preview_publication(
        verify_workspace.path(),
        &verify_request,
        &outcome,
        &principal_from_keypair(&verify_agent)
    )
    .is_err());
}

#[cfg(unix)]
#[test]
fn v2_rejects_symlinked_edition_for_publication_and_verification() {
    use std::os::unix::fs::symlink;

    let (workspace, edition, manifest_digest) = preview_workspace();
    let outside = TempDir::new().unwrap();
    let outside_edition = outside.path().join("edition.json");
    std::fs::write(&outside_edition, b"outside edition").unwrap();
    let edition_path = workspace
        .path()
        .join(".proof/data/editions")
        .join(format!("{}.json", edition.id));
    std::fs::remove_file(&edition_path).unwrap();
    symlink(&outside_edition, &edition_path).unwrap();
    let agent = generate_keypair_for(PrincipalKind::Agent);
    let publish_engine = engine(Arc::new(RecordingStore::default()), agent.clone());
    let publish_context = context(workspace.path(), &agent);
    let request = input(&edition, &manifest_digest, Uuid::now_v7());
    let (approval, human) = approval_for(&request, &publish_context, &agent);
    assert!(publish_engine
        .execute_with_approval_evidenced(
            "release.publish",
            "v2",
            &request,
            &publish_context,
            &approval,
            &human
        )
        .is_err());
    assert_eq!(std::fs::read(&outside_edition).unwrap(), b"outside edition");
    assert!(!workspace.path().join(".proof/data/previews").exists());

    let (verify_workspace, verify_edition, verify_manifest) = preview_workspace();
    let verify_agent = generate_keypair_for(PrincipalKind::Agent);
    let verify_engine = engine(Arc::new(RecordingStore::default()), verify_agent.clone());
    let verify_context = context(verify_workspace.path(), &verify_agent);
    let verify_request = input(&verify_edition, &verify_manifest, Uuid::now_v7());
    let (verify_approval, verify_human) =
        approval_for(&verify_request, &verify_context, &verify_agent);
    let outcome = verify_engine
        .execute_with_approval_evidenced(
            "release.publish",
            "v2",
            &verify_request,
            &verify_context,
            &verify_approval,
            &verify_human,
        )
        .unwrap();
    let verify_edition_path = verify_workspace
        .path()
        .join(".proof/data/editions")
        .join(format!("{}.json", verify_edition.id));
    std::fs::remove_file(&verify_edition_path).unwrap();
    symlink(&outside_edition, &verify_edition_path).unwrap();
    assert!(verify_preview_publication(
        verify_workspace.path(),
        &verify_request,
        &outcome,
        &principal_from_keypair(&verify_agent)
    )
    .is_err());
    assert_eq!(std::fs::read(&outside_edition).unwrap(), b"outside edition");
}

struct CompletionFailStore(RecordingStore);

impl ExecutionStore for CompletionFailStore {
    fn save_proof(&self, proof: &proof_kernel::Proof) -> Result<(), String> {
        self.0.save_proof(proof)
    }

    fn save_execution_context(&self, context: &ExecutionContext) -> Result<String, String> {
        self.0.save_execution_context(context)
    }

    fn claim_execution_replay(
        &self,
        claim: &ExecutionReplayClaim,
    ) -> Result<ExecutionReplayClaimResult, String> {
        self.0.claim_execution_replay(claim)
    }

    fn complete_execution_replay(
        &self,
        _claim: &ExecutionReplayClaim,
        _context: &ExecutionContext,
        _outcome: &proof_kernel::ExecutionOutcome,
    ) -> Result<(), String> {
        Err("simulated crash before replay completion".to_string())
    }

    fn fail_execution_replay(
        &self,
        claim: &ExecutionReplayClaim,
        failed_at: chrono::DateTime<Utc>,
        failure: &str,
    ) -> Result<(), String> {
        self.0.fail_execution_replay(claim, failed_at, failure)
    }
}

#[test]
fn v2_completion_crash_leaves_one_artifact_and_an_indeterminate_claim() {
    let (workspace, edition, manifest_digest) = preview_workspace();
    let agent = generate_keypair_for(PrincipalKind::Agent);
    let engine = engine(
        Arc::new(CompletionFailStore(RecordingStore::default())),
        agent.clone(),
    );
    let context = context(workspace.path(), &agent);
    let request = input(&edition, &manifest_digest, Uuid::now_v7());
    let (approval, human) = approval_for(&request, &context, &agent);
    assert!(matches!(
        engine.execute_with_approval_evidenced(
            "release.publish",
            "v2",
            &request,
            &context,
            &approval,
            &human
        ),
        Err(ExecutionError::StorageFailed(_))
    ));
    let artifact_dir = workspace
        .path()
        .join(".proof/data/previews")
        .join(edition.id.to_string());
    assert_eq!(std::fs::read_dir(&artifact_dir).unwrap().count(), 1);
    assert_eq!(
        engine
            .execute_with_approval_evidenced(
                "release.publish",
                "v2",
                &request,
                &context,
                &approval,
                &human
            )
            .unwrap_err(),
        ExecutionError::Idempotency(IdempotencyError::InProgress)
    );
}
