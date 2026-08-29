use crate::workspace::{save_workspace_json, Workspace};
use crate::{build_engine, load_registry, Cli};
use anyhow::{bail, Context, Result};
use proof_content::{changeset::BaseState, edition::Edition, object::Object};
use proof_kernel::{create_proof, ExecutionContext};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;

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
    let proof = ws.make_proof(
        "changeset.create",
        "v1",
        &serde_json::json!({"intent": intent}),
        &serde_json::json!({"changeset_id": changeset.id.to_string()}),
    )?;
    ws.save_proof(&proof)?;
    println!(
        "{}",
        serde_json::json!({"status": "created", "type": "changeset", "id": changeset.id.to_string(), "proof_id": proof.body.id.to_string()})
    );
    Ok(())
}

pub fn cmd_edition_create(cli: &Cli, changeset_id: &str) -> Result<()> {
    let ws = Workspace::open(&cli.workspace)?;
    let mut objects = vec![];
    let objects_dir = cli.workspace.join(".proof/data/objects");
    if objects_dir.exists() {
        for entry in std::fs::read_dir(&objects_dir)? {
            let entry = entry?;
            if entry.path().extension().map_or(false, |ext| ext == "json") {
                let content = std::fs::read_to_string(entry.path())?;
                let obj: Object = serde_json::from_str(&content)?;
                objects.push(obj);
            }
        }
    }
    let cs_uuid = uuid::Uuid::parse_str(changeset_id)?;
    let edition = Edition::new(cs_uuid, objects);
    ws.save_json(
        "editions",
        &edition.id.to_string(),
        &serde_json::to_value(&edition)?,
    )?;
    let proof = ws.make_proof("edition.create", "v1",
        &serde_json::json!({"changeset_id": changeset_id}),
        &serde_json::json!({"edition_id": edition.id.to_string(), "content_digest": edition.content_digest}))?;
    ws.save_proof(&proof)?;
    println!(
        "{}",
        serde_json::json!({"status": "created", "type": "edition", "id": edition.id.to_string(), "content_digest": edition.content_digest, "proof_id": proof.body.id.to_string()})
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
    let ws = Workspace::open(&cli.workspace)?;
    let input_value: Value = serde_json::from_str(input).context("invalid input JSON")?;
    let context = ExecutionContext {
        actor: ws.actor,
        principal_kind: Some(proof_kernel::PrincipalKind::Agent),
        delegation_id: None,
        delegation_chain: None,
        workspace_path: ws.root.clone(),
        timestamp: chrono::Utc::now(),
    };
    let registry = load_registry(&ws.root)?;
    let engine = build_engine(registry)?;
    let output = engine
        .execute(operation, version, &input_value, &context)
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    let proof_operation = format!("{operation}::{version}");
    let proof = create_proof(
        ws.actor,
        context.delegation_id,
        &proof_operation,
        &input_value,
        &output,
        context.timestamp,
        &ws.keypair,
    )
    .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    ws.save_proof(&proof)?;
    println!(
        "{}",
        serde_json::json!({
            "status": "executed",
            "operation": operation,
            "version": version,
            "result": output,
            "proof_id": proof.body.id.to_string(),
        })
    );
    Ok(())
}
