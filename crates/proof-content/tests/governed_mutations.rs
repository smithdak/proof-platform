use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use proof_content::{
    content_handlers, ChangeSet, ChangeSetEdit, ChangeSetStatus, FieldType, Object,
    ObjectCreateEdit, ObjectDeleteEdit, SchemaDefinition, SchemaField,
};
use proof_kernel::{
    generate_keypair_for, ExecutionContext, ExecutionEngine, ExecutionError, ExecutionStore,
    IdempotencyError, PrincipalKind, Proof, RecordingStore, Registry,
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

fn context(workspace_path: &Path, keypair: &proof_kernel::Keypair) -> ExecutionContext {
    ExecutionContext {
        actor: keypair.principal_id,
        principal_kind: Some(PrincipalKind::Agent),
        delegation_id: None,
        delegation_chain: None,
        workspace_path: workspace_path.to_path_buf(),
        timestamp: chrono::Utc::now(),
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

fn prepare_workspace() -> (TempDir, SchemaDefinition, ChangeSet, Object, Object) {
    let workspace = TempDir::new().unwrap();
    let registry_dir = workspace.path().join("registry/content");
    std::fs::create_dir_all(&registry_dir).unwrap();
    let root_registry = repository_root().join("registry/content");
    for file in ["changeset-commit.input.json", "edition-create.input.json"] {
        std::fs::copy(root_registry.join(file), registry_dir.join(file)).unwrap();
    }

    let schema = schema();
    let schema_dir = workspace.path().join(".proof/data/schemas");
    std::fs::create_dir_all(&schema_dir).unwrap();
    std::fs::write(
        schema_dir.join(format!("{}-{}.json", schema.id, schema.version)),
        serde_json::to_string(&schema).unwrap(),
    )
    .unwrap();

    let existing = Object::create(&schema, "en-US", json!({"title": "Existing"})).unwrap();
    let created = Object::create(&schema, "en-US", json!({"title": "Created"})).unwrap();
    let object_dir = workspace.path().join(".proof/data/objects");
    std::fs::create_dir_all(&object_dir).unwrap();
    std::fs::write(
        object_dir.join(format!("{}.json", existing.id)),
        serde_json::to_string(&existing).unwrap(),
    )
    .unwrap();

    let mut base_state = BTreeMap::new();
    base_state.insert(existing.id, existing.clone());
    let mut changeset = ChangeSet::new(
        "Replace object",
        &base_state,
        vec![
            ChangeSetEdit::ObjectCreate(ObjectCreateEdit {
                object: created.clone(),
            }),
            ChangeSetEdit::ObjectDelete(ObjectDeleteEdit {
                object_id: existing.id,
                expected_revision: existing.revision,
            }),
        ],
    );
    changeset.transition_to(ChangeSetStatus::Submitted).unwrap();
    changeset.transition_to(ChangeSetStatus::Approved).unwrap();
    let changeset_dir = workspace.path().join(".proof/data/changesets");
    std::fs::create_dir_all(&changeset_dir).unwrap();
    std::fs::write(
        changeset_dir.join(format!("{}.json", changeset.id)),
        serde_json::to_string(&changeset).unwrap(),
    )
    .unwrap();

    (workspace, schema, changeset, existing, created)
}

#[test]
fn root_content_registry_exposes_the_frozen_eight_v1_operations() {
    let registry =
        Registry::load_from_directory(repository_root().join("registry/content")).unwrap();
    let operations: BTreeSet<_> = registry
        .active_operations()
        .into_iter()
        .map(|entry| (entry.operation.as_str(), entry.version.as_str()))
        .collect();
    assert_eq!(
        operations,
        BTreeSet::from([
            ("schema.create", "v1"),
            ("object.create", "v1"),
            ("object.edit", "v1"),
            ("content.approve", "v1"),
            ("content.release", "v1"),
            ("changeset.commit", "v1"),
            ("release.publish", "v1"),
            ("edition.create", "v1"),
        ])
    );
    assert_eq!(
        registry.find("content.approve", "v1").unwrap().consequence,
        "content-approval"
    );
}

#[test]
fn target_wire_schemas_require_v7_keys_and_exact_output_envelopes() {
    let registry = repository_root().join("registry/content");
    for input_file in ["changeset-commit.input.json", "edition-create.input.json"] {
        let schema: Value =
            serde_json::from_str(&std::fs::read_to_string(registry.join(input_file)).unwrap())
                .unwrap();
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(
            schema["required"],
            json!(["idempotency_key", "changeset_id"])
        );
        assert_eq!(schema["properties"]["idempotency_key"]["format"], "uuid");
        assert!(schema["properties"]["idempotency_key"]["pattern"]
            .as_str()
            .unwrap()
            .contains("-7"));
    }
    for output_file in ["changeset-commit.output.json", "edition-create.output.json"] {
        let schema: Value =
            serde_json::from_str(&std::fs::read_to_string(registry.join(output_file)).unwrap())
                .unwrap();
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(schema["properties"]["data"]["additionalProperties"], false);
    }
}

#[test]
fn governed_commit_and_edition_replay_exactly_without_remutation() {
    let (workspace, _schema, changeset, deleted, created) = prepare_workspace();
    let keypair = generate_keypair_for(PrincipalKind::Agent);
    let store = Arc::new(RecordingStore::default());
    let engine = engine(store.clone(), keypair.clone());
    let context = context(workspace.path(), &keypair);

    let commit_key = Uuid::now_v7();
    let commit_input: Value = serde_json::from_str(&format!(
        r#"{{"notes":"first commit","changeset_id":"{}","idempotency_key":"{}"}}"#,
        changeset.id, commit_key
    ))
    .unwrap();
    let committed = engine
        .execute_evidenced("changeset.commit", "v1", &commit_input, &context)
        .unwrap();
    assert_eq!(committed.output["operation"], "changeset.commit");
    assert_eq!(committed.output["data"]["changeset"]["status"], "committed");
    assert_eq!(committed.output["data"]["objects_count"], 1);
    assert!(!workspace
        .path()
        .join(".proof/data/objects")
        .join(format!("{}.json", deleted.id))
        .exists());
    assert!(workspace
        .path()
        .join(".proof/data/objects")
        .join(format!("{}.json", created.id))
        .exists());
    let persisted_changeset: ChangeSet = serde_json::from_str(
        &std::fs::read_to_string(
            workspace
                .path()
                .join(".proof/data/changesets")
                .join(format!("{}.json", changeset.id)),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(persisted_changeset.status, ChangeSetStatus::Committed);

    let commit_retry: Value = serde_json::from_str(&format!(
        r#"{{"idempotency_key":"{}","changeset_id":"{}","notes":"first commit"}}"#,
        commit_key, changeset.id
    ))
    .unwrap();
    let replayed_commit = engine
        .execute_evidenced("changeset.commit", "v1", &commit_retry, &context)
        .unwrap();
    assert_eq!(replayed_commit.output, committed.output);
    assert_eq!(replayed_commit.proof, committed.proof);
    assert_eq!(replayed_commit.proof.body.id, committed.proof.body.id);
    assert_eq!(replayed_commit.proof.signature, committed.proof.signature);
    assert_eq!(store.proofs.lock().unwrap().len(), 1);
    assert_eq!(store.contexts.lock().unwrap().len(), 1);
    assert_eq!(
        engine
            .execute(
                "changeset.commit",
                "v1",
                &json!({
                    "idempotency_key": commit_key,
                    "changeset_id": changeset.id,
                    "notes": "changed input"
                }),
                &context,
            )
            .unwrap_err(),
        ExecutionError::Idempotency(IdempotencyError::Conflict)
    );

    let edition_key = Uuid::now_v7();
    let edition_input = json!({
        "idempotency_key": edition_key,
        "changeset_id": changeset.id,
    });
    let edition = engine
        .execute_evidenced("edition.create", "v1", &edition_input, &context)
        .unwrap();
    assert_eq!(edition.output["operation"], "edition.create");
    assert_eq!(
        edition.output["data"]["edition"]["changeset_id"],
        changeset.id.to_string()
    );
    assert_eq!(
        edition.output["data"]["edition"]["objects"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let edition_id = edition.output["data"]["edition"]["id"].as_str().unwrap();
    assert!(workspace
        .path()
        .join(".proof/data/editions")
        .join(format!("{edition_id}.json"))
        .exists());
    let replayed_edition = engine
        .execute_evidenced("edition.create", "v1", &edition_input, &context)
        .unwrap();
    assert_eq!(replayed_edition.output, edition.output);
    assert_eq!(replayed_edition.proof, edition.proof);
    assert_eq!(replayed_edition.proof.body.id, edition.proof.body.id);
    assert_eq!(replayed_edition.proof.signature, edition.proof.signature);
    assert_eq!(store.proofs.lock().unwrap().len(), 2);
    assert_eq!(store.contexts.lock().unwrap().len(), 2);
    assert_eq!(
        engine
            .execute(
                "edition.create",
                "v1",
                &json!({
                    "idempotency_key": edition_key,
                    "changeset_id": Uuid::now_v7(),
                }),
                &context,
            )
            .unwrap_err(),
        ExecutionError::Idempotency(IdempotencyError::Conflict)
    );
}

struct UnsupportedStore;

impl ExecutionStore for UnsupportedStore {
    fn save_proof(&self, _proof: &Proof) -> Result<(), String> {
        Ok(())
    }

    fn save_execution_context(&self, _context: &ExecutionContext) -> Result<String, String> {
        Ok("context".to_string())
    }
}

#[test]
fn governed_mutations_fail_closed_before_or_after_handler_entry() {
    let (workspace, _schema, changeset, deleted, _created) = prepare_workspace();
    let keypair = generate_keypair_for(PrincipalKind::Agent);
    let context = context(workspace.path(), &keypair);
    let valid_input = json!({
        "idempotency_key": Uuid::now_v7(),
        "changeset_id": changeset.id,
    });

    let without_storage = engine(Arc::new(RecordingStore::default()), keypair.clone());
    // The separate engine is deliberately created without a store below.
    let registry =
        Registry::load_from_directory(repository_root().join("registry/content")).unwrap();
    let mut no_storage = ExecutionEngine::new_with_keypair(registry, keypair.clone());
    for handler in content_handlers() {
        no_storage.register_handler(handler);
    }
    assert_eq!(
        no_storage
            .execute("changeset.commit", "v1", &valid_input, &context)
            .unwrap_err(),
        ExecutionError::Idempotency(IdempotencyError::StorageRequired)
    );
    assert_eq!(
        without_storage
            .execute(
                "changeset.commit",
                "v1",
                &json!({"idempotency_key": Uuid::nil(), "changeset_id": changeset.id}),
                &context,
            )
            .unwrap_err(),
        ExecutionError::Idempotency(IdempotencyError::InvalidUuidV7)
    );
    assert!(workspace
        .path()
        .join(".proof/data/objects")
        .join(format!("{}.json", deleted.id))
        .exists());

    let unsupported = engine(Arc::new(UnsupportedStore), keypair.clone());
    assert_eq!(
        unsupported
            .execute("changeset.commit", "v1", &valid_input, &context)
            .unwrap_err(),
        ExecutionError::Idempotency(IdempotencyError::StorageRequired)
    );

    let failing_store = Arc::new(RecordingStore::default());
    let failing_engine = engine(failing_store, keypair);
    let failing_input = json!({
        "idempotency_key": Uuid::now_v7(),
        "changeset_id": changeset.id,
    });
    assert!(matches!(
        failing_engine.execute("edition.create", "v1", &failing_input, &context),
        Err(ExecutionError::HandlerFailed(_))
    ));
    assert_eq!(
        failing_engine
            .execute("edition.create", "v1", &failing_input, &context)
            .unwrap_err(),
        ExecutionError::Idempotency(IdempotencyError::Indeterminate)
    );
}
