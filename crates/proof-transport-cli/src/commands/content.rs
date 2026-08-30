use crate::workspace::{save_workspace_json, Workspace};
use crate::{build_engine, load_registry, open_store, Cli};
use anyhow::{bail, Context, Result};
use proof_content::{changeset::BaseState, object::Object};
use proof_kernel::{ExecutionContext, ExecutionOutcome};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::Arc;

pub fn cmd_init(cli: &Cli) -> Result<()> {
    let ws = Workspace::init(&cli.workspace)?;
    println!(
        "{}",
        serde_json::json!({"status": "initialized", "actor_id": ws.actor.to_string()})
    );
    Ok(())
}

pub fn cmd_schema_create(cli: &Cli, name: &str, fields_json: &str) -> Result<()> {
    let ws = Workspace::open(&cli.workspace)?;
    let fields_value: Value = serde_json::from_str(fields_json)?;
    let mut schema_fields = vec![];
    if let Value::Array(arr) = &fields_value {
        for field in arr {
            let fname = field["name"].as_str().context("field missing name")?;
            let ftype = match field["field_type"].as_str().unwrap_or("text") {
                "text" => proof_content::schema::FieldType::Text,
                "rich_text" => proof_content::schema::FieldType::RichText,
                "number" => proof_content::schema::FieldType::Number,
                "boolean" => proof_content::schema::FieldType::Boolean,
                "date" => proof_content::schema::FieldType::Date,
                "date_time" => proof_content::schema::FieldType::DateTime,
                "json" => proof_content::schema::FieldType::Json,
                "reference" => proof_content::schema::FieldType::Reference,
                _ => proof_content::schema::FieldType::Text,
            };
            schema_fields.push(proof_content::schema::SchemaField {
                name: fname.to_string(),
                field_type: ftype,
                required: field["required"].as_bool().unwrap_or(false),
                localized: field["localized"].as_bool().unwrap_or(false),
                default_value: field.get("default").cloned(),
            });
        }
    }
    let schema = proof_content::schema::SchemaDefinition::new(name.to_string(), 1, schema_fields);
    schema.validate()?;
    let schema_json = serde_json::to_value(&schema)?;
    ws.save_json("schemas", &schema.id.to_string(), &schema_json)?;
    let proof = ws.make_proof(
        "schema.create",
        "v1",
        &serde_json::json!({"name": name}),
        &serde_json::json!({"schema_id": schema.id.to_string()}),
    )?;
    ws.save_proof(&proof)?;
    println!(
        "{}",
        serde_json::json!({"status": "created", "type": "schema", "id": schema.id.to_string(), "proof_id": proof.body.id.to_string()})
    );
    Ok(())
}

pub fn cmd_object_create(cli: &Cli, schema_id: &str, locale: &str, data: &str) -> Result<()> {
    let ws = Workspace::open(&cli.workspace)?;
    let content: Value = serde_json::from_str(data)?;
    let _schema_uuid = uuid::Uuid::parse_str(schema_id)?;
    let schema_json = ws.load_json("schemas", schema_id)?;
    let schema: proof_content::schema::SchemaDefinition = serde_json::from_value(schema_json)?;
    schema.validate_object(&content)?;
    let object = Object::create(&schema, locale, content)?;
    let object_json = serde_json::to_value(&object)?;
    ws.save_json("objects", &object.id.to_string(), &object_json)?;
    let proof = ws.make_proof(
        "object.create",
        "v1",
        &serde_json::json!({"schema_id": schema_id}),
        &serde_json::json!({"object_id": object.id.to_string()}),
    )?;
    ws.save_proof(&proof)?;
    println!(
        "{}",
        serde_json::json!({"status": "created", "type": "object", "id": object.id.to_string(), "proof_id": proof.body.id.to_string()})
    );
    Ok(())
}

pub fn cmd_changeset_create(cli: &Cli, intent: &str) -> Result<()> {
    let ws = Workspace::open(&cli.workspace)?;
    let base_state: BaseState = BTreeMap::new();
    let changeset = proof_content::ChangeSet::new(intent, &base_state, vec![]);
    let cs_json = serde_json::to_value(&changeset)?;
    ws.save_json("changesets", &changeset.id.to_string(), &cs_json)?;
    println!(
        "{}",
        serde_json::json!({"status": "created", "type": "changeset", "id": changeset.id.to_string()})
    );
    Ok(())
}

pub fn cmd_edition_create(cli: &Cli, changeset_id: &str, idempotency_key: &str) -> Result<()> {
    let outcome = execute_governed(
        cli,
        "edition.create",
        "v1",
        &serde_json::json!({
            "changeset_id": changeset_id,
            "idempotency_key": idempotency_key,
        }),
    )?;
    println!(
        "{}",
        serde_json::json!({
            "status": "executed",
            "operation": "edition.create",
            "version": "v1",
            "result": outcome.output,
            "proof_id": outcome.proof.body.id.to_string(),
        })
    );
    Ok(())
}

pub fn cmd_changeset_commit(
    cli: &Cli,
    changeset_id: &str,
    idempotency_key: &str,
    notes: Option<&str>,
) -> Result<()> {
    let mut input = serde_json::json!({
        "changeset_id": changeset_id,
        "idempotency_key": idempotency_key,
    });
    if let Some(notes) = notes {
        input["notes"] = Value::String(notes.to_string());
    }
    let outcome = execute_governed(cli, "changeset.commit", "v1", &input)?;
    println!(
        "{}",
        serde_json::json!({
            "status": "executed",
            "operation": "changeset.commit",
            "version": "v1",
            "result": outcome.output,
            "proof_id": outcome.proof.body.id.to_string(),
        })
    );
    Ok(())
}

pub fn cmd_release_publish(_cli: &Cli, _edition_id: &str, _environment: &str) -> Result<()> {
    bail!(
        "release.publish is human-only; use `proof agent start` to create a signed approval \
         request, `proof approval approve` to record the human decision, then `proof agent \
         resume`"
    )
}

pub fn cmd_status(cli: &Cli) -> Result<()> {
    let data_dir = cli.workspace.join(".proof/data");
    let count = |subdir: &str| -> usize {
        std::fs::read_dir(data_dir.join(subdir))
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .filter(|e| e.path().extension().map_or(false, |ext| ext == "json"))
                    .count()
            })
            .unwrap_or(0)
    };
    println!(
        "{}",
        serde_json::json!({
            "schemas": count("schemas"),
            "objects": count("objects"),
            "changesets": count("changesets"),
            "editions": count("editions"),
            "releases": count("releases"),
            "proofs": count("proofs"),
        })
    );
    Ok(())
}

pub fn cmd_execute(cli: &Cli, operation: &str, version: &str, input: &str) -> Result<()> {
    let input_value: Value = serde_json::from_str(input).context("invalid input JSON")?;
    let outcome = execute_governed(cli, operation, version, &input_value)?;
    println!(
        "{}",
        serde_json::json!({
            "status": "executed",
            "operation": operation,
            "version": version,
            "result": outcome.output,
            "proof_id": outcome.proof.body.id.to_string(),
        })
    );
    Ok(())
}

pub(crate) fn execute_governed(
    cli: &Cli,
    operation: &str,
    version: &str,
    input: &Value,
) -> Result<ExecutionOutcome> {
    let ws = Workspace::open(&cli.workspace)?;
    let context = ExecutionContext {
        actor: ws.actor,
        principal_kind: Some(proof_kernel::PrincipalKind::Agent),
        delegation_id: None,
        delegation_chain: None,
        workspace_path: ws.root.clone(),
        timestamp: chrono::Utc::now(),
    };
    let registry = load_registry(&ws.root)?;
    let store = Arc::new(open_store(&ws.root)?);
    let engine = build_engine(registry, ws.keypair.clone(), store)?;
    let outcome = engine
        .execute_evidenced(operation, version, input, &context)
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    // The engine is the evidence authority. This file is only the legacy CLI
    // proof view used by `status` and `verify`, so retries overwrite identical
    // serialized original evidence instead of creating a replacement proof.
    ws.save_proof(&outcome.proof)?;
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use proof_content::{
        ChangeSet, ChangeSetEdit, ChangeSetStatus, FieldType, Object, ObjectCreateEdit,
        ObjectDeleteEdit, SchemaDefinition, SchemaField,
    };
    use proof_kernel::IdempotencyError;
    use std::path::Path;

    fn repository_root() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf()
    }

    fn install_governed_content_registry(root: &Path) {
        let source = repository_root().join("registry/content");
        let workspace_registry = root.join(".proof/registry/content");
        let handler_registry = root.join("registry/content");
        std::fs::create_dir_all(&workspace_registry).unwrap();
        std::fs::create_dir_all(&handler_registry).unwrap();
        for file in [
            "changeset-commit.json",
            "changeset-commit.input.json",
            "edition-create.json",
            "edition-create.input.json",
        ] {
            std::fs::copy(source.join(file), workspace_registry.join(file)).unwrap();
            std::fs::copy(source.join(file), handler_registry.join(file)).unwrap();
        }
    }

    fn cli_for_workspace(root: &Path) -> Cli {
        Cli::parse_from(["proof", "-w", root.to_str().unwrap(), "status"])
    }

    fn prepare_workspace(root: &Path) -> ChangeSet {
        let init = Cli::parse_from(["proof", "-w", root.to_str().unwrap(), "init"]);
        cmd_init(&init).unwrap();
        install_governed_content_registry(root);

        let schema = SchemaDefinition::new(
            "Article".to_string(),
            1,
            vec![SchemaField {
                name: "title".to_string(),
                field_type: FieldType::Text,
                required: true,
                localized: false,
                default_value: None,
            }],
        );
        let existing = Object::create(&schema, "en-US", json!({"title": "Existing"})).unwrap();
        let created = Object::create(&schema, "en-US", json!({"title": "Created"})).unwrap();
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

        let workspace = Workspace::open(&root.to_path_buf()).unwrap();
        workspace
            .save_json(
                "schemas",
                &schema.id.to_string(),
                &serde_json::to_value(schema).unwrap(),
            )
            .unwrap();
        workspace
            .save_json(
                "objects",
                &existing.id.to_string(),
                &serde_json::to_value(existing).unwrap(),
            )
            .unwrap();
        workspace
            .save_json(
                "changesets",
                &changeset.id.to_string(),
                &serde_json::to_value(&changeset).unwrap(),
            )
            .unwrap();
        changeset
    }

    #[test]
    fn direct_governed_commands_require_caller_supplied_idempotency_keys() {
        let key = uuid::Uuid::now_v7().to_string();
        let changeset_id = uuid::Uuid::now_v7().to_string();
        let edition = Cli::parse_from([
            "proof",
            "edition-create",
            "--changeset-id",
            &changeset_id,
            "--idempotency-key",
            &key,
        ]);
        assert!(matches!(
            edition.command,
            crate::Command::EditionCreate { idempotency_key, .. } if idempotency_key == key
        ));

        let commit = Cli::parse_from([
            "proof",
            "changeset-commit",
            "--changeset-id",
            &changeset_id,
            "--idempotency-key",
            &key,
        ]);
        assert!(matches!(
            commit.command,
            crate::Command::ChangesetCommit { idempotency_key, .. } if idempotency_key == key
        ));
    }

    #[test]
    fn governed_commit_and_edition_replay_original_evidence_without_remutation() {
        let directory = assert_fs::TempDir::new().unwrap();
        let changeset = prepare_workspace(directory.path());
        let cli = cli_for_workspace(directory.path());
        cmd_changeset_create(&cli, "local drafting helper").unwrap();
        assert_eq!(
            std::fs::read_dir(directory.path().join(".proof/data/proofs"))
                .unwrap()
                .count(),
            0
        );
        let commit_key = uuid::Uuid::now_v7();
        let first_commit: Value = serde_json::from_str(&format!(
            r#"{{"notes":"first commit","changeset_id":"{}","idempotency_key":"{}"}}"#,
            changeset.id, commit_key
        ))
        .unwrap();
        let committed = execute_governed(&cli, "changeset.commit", "v1", &first_commit).unwrap();
        assert_eq!(committed.output["data"]["changeset"]["status"], "committed");
        assert_eq!(committed.proof.body.operation, "changeset.commit::v1");
        let workspace_keypair = Workspace::open(&directory.path().to_path_buf())
            .unwrap()
            .keypair;
        committed
            .proof
            .verify(&workspace_keypair.signing_key.verifying_key())
            .unwrap();

        let changeset_file = directory
            .path()
            .join(".proof/data/changesets")
            .join(format!("{}.json", changeset.id));
        let committed_bytes = std::fs::read(&changeset_file).unwrap();
        let reordered_commit: Value = serde_json::from_str(&format!(
            r#"{{"idempotency_key":"{}","changeset_id":"{}","notes":"first commit"}}"#,
            commit_key, changeset.id
        ))
        .unwrap();
        let replayed_commit =
            execute_governed(&cli, "changeset.commit", "v1", &reordered_commit).unwrap();
        assert_eq!(replayed_commit.output, committed.output);
        assert_eq!(replayed_commit.proof, committed.proof);
        assert_eq!(std::fs::read(&changeset_file).unwrap(), committed_bytes);

        let changed_commit = json!({
            "idempotency_key": commit_key,
            "changeset_id": changeset.id,
            "notes": "different input",
        });
        let error = execute_governed(&cli, "changeset.commit", "v1", &changed_commit).unwrap_err();
        assert!(error
            .to_string()
            .contains(&IdempotencyError::Conflict.to_string()));
        assert_eq!(std::fs::read(&changeset_file).unwrap(), committed_bytes);

        let edition_key = uuid::Uuid::now_v7();
        let first_edition: Value = serde_json::from_str(&format!(
            r#"{{"changeset_id":"{}","idempotency_key":"{}"}}"#,
            changeset.id, edition_key
        ))
        .unwrap();
        let edition = execute_governed(&cli, "edition.create", "v1", &first_edition).unwrap();
        assert_eq!(edition.output["operation"], "edition.create");
        assert_eq!(edition.proof.body.operation, "edition.create::v1");
        edition
            .proof
            .verify(&workspace_keypair.signing_key.verifying_key())
            .unwrap();
        let editions_dir = directory.path().join(".proof/data/editions");
        assert_eq!(std::fs::read_dir(&editions_dir).unwrap().count(), 1);

        let reordered_edition: Value = serde_json::from_str(&format!(
            r#"{{"idempotency_key":"{}","changeset_id":"{}"}}"#,
            edition_key, changeset.id
        ))
        .unwrap();
        let replayed_edition =
            execute_governed(&cli, "edition.create", "v1", &reordered_edition).unwrap();
        assert_eq!(replayed_edition.output, edition.output);
        assert_eq!(replayed_edition.proof, edition.proof);
        assert_eq!(std::fs::read_dir(&editions_dir).unwrap().count(), 1);

        let changed_edition = json!({
            "idempotency_key": edition_key,
            "changeset_id": uuid::Uuid::now_v7(),
        });
        let error = execute_governed(&cli, "edition.create", "v1", &changed_edition).unwrap_err();
        assert!(error
            .to_string()
            .contains(&IdempotencyError::Conflict.to_string()));
        assert_eq!(std::fs::read_dir(&editions_dir).unwrap().count(), 1);

        let proof_file = directory
            .path()
            .join(".proof/data/proofs")
            .join(format!("{}.json", edition.proof.body.id));
        let persisted: proof_kernel::Proof =
            serde_json::from_slice(&std::fs::read(proof_file).unwrap()).unwrap();
        assert_eq!(persisted, edition.proof);
    }
}
