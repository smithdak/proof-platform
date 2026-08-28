use anyhow::{bail, Context, Result};
use base64::Engine;
use clap::{Parser, Subcommand};
use proof_content::{
    changeset::BaseState,
    edition::Edition,
    object::Object,
    release::Release,
    schema::{FieldType, SchemaDefinition, SchemaField},
};
use proof_kernel::{
    canonicalize, create_proof, digest, generate_keypair, principal_from_keypair, ArtifactKind,
    Benchmark, BenchmarkResult, BenchmarkRunner, ExecutionContext, ExecutionEngine, ExecutionError,
    OperationHandler, PrincipalId, Proof, Registry, RegistryEntry,
};
use proof_kernel::{Delegation, DelegationChain};
use proof_storage::SqliteStore;
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "proof", about = "Proof Platform CLI", version)]
struct Cli {
    #[arg(short, long)]
    verbose: bool,
    #[arg(short = 'w', long, default_value = ".")]
    workspace: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Init,
    SchemaCreate {
        #[arg(short, long)]
        name: String,
        #[arg(short, long)]
        fields: String,
    },
    ObjectCreate {
        #[arg(short, long)]
        schema_id: String,
        #[arg(short, long, default_value = "en-US")]
        locale: String,
        #[arg(short, long)]
        data: String,
    },
    ChangesetCreate {
        #[arg(short, long)]
        intent: String,
    },
    EditionCreate {
        #[arg(short, long)]
        changeset_id: String,
    },
    ReleasePublish {
        #[arg(short, long)]
        edition_id: String,
        #[arg(long)]
        environment: String,
    },
    Status,
    Capabilities,
    #[command(subcommand)]
    Registry(RegistryCommand),
    #[command(subcommand)]
    Workspace(WorkspaceCommand),
    #[command(subcommand)]
    Keypair(KeypairCommand),
    Verify {
        proof_id: String,
    },
    Execute {
        operation: String,
        version: String,
        #[arg(short, long)]
        input: String,
    },
    #[command(subcommand)]
    Benchmark(BenchmarkCommand),
    #[command(subcommand)]
    Delegation(DelegationCommand),
    Export {
        #[arg(short, long)]
        output: PathBuf,
    },
    Import {
        #[arg(short, long)]
        input: PathBuf,
    },
}

#[derive(Subcommand)]
enum BenchmarkCommand {
    Run {
        operation: String,
        version: String,
        #[arg(long)]
        threshold_ms: u64,
        #[arg(long, default_value_t = 10)]
        runs: u32,
        #[arg(short, long, default_value = "{}")]
        input: String,
    },
    Report,
}

#[derive(Subcommand)]
enum DelegationCommand {
    Grant {
        agent_id: String,
        #[arg(short, long)]
        scope: String,
    },
    List,
    Revoke {
        delegation_id: String,
    },
    Validate {
        delegation_id: String,
    },
}

#[derive(Subcommand)]
enum RegistryCommand {
    List,
    Inspect { operation: String },
}

#[derive(Subcommand)]
enum WorkspaceCommand {
    Init { path: String },
    Status,
}

#[derive(Subcommand)]
enum KeypairCommand {
    Export,
    Rotate,
}

struct Workspace {
    root: PathBuf,
    keypair: proof_kernel::Keypair,
    actor: PrincipalId,
}

impl Workspace {
    fn init(root: &PathBuf) -> Result<Self> {
        let proof_dir = root.join(".proof");
        std::fs::create_dir_all(proof_dir.join("registry"))?;
        std::fs::create_dir_all(proof_dir.join("storage"))?;
        for subdir in [
            "schemas",
            "objects",
            "changesets",
            "editions",
            "releases",
            "proofs",
        ] {
            std::fs::create_dir_all(proof_dir.join("data").join(subdir))?;
        }
        let keypair = generate_keypair();
        let actor = keypair.principal_id;
        let config = serde_json::json!({
            "actor_id": actor.to_string(),
            "version": "0.1.0",
        });
        std::fs::write(
            proof_dir.join("config.json"),
            serde_json::to_string_pretty(&config)?,
        )?;
        let keypair_json = serde_json::json!({
            "principal_id": keypair.principal_id.as_uuid(),
            "kind": keypair.kind,
            "created_at": keypair.created_at,
            "public_key": keypair.signing_key.verifying_key().to_bytes(),
            "signing_key": base64::engine::general_purpose::STANDARD
                .encode(keypair.signing_key.to_bytes()),
        });
        std::fs::write(
            proof_dir.join("keypair.json"),
            serde_json::to_string_pretty(&keypair_json)?,
        )?;
        let store = SqliteStore::open(&proof_dir.join("storage/storage.db"))
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        store
            .save_principal(&principal_from_keypair(&keypair))
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        Ok(Self {
            root: root.clone(),
            keypair,
            actor,
        })
    }

    fn open(root: &PathBuf) -> Result<Self> {
        let config_path = root.join(".proof/config.json");
        let config: Value = serde_json::from_str(
            &std::fs::read_to_string(&config_path)
                .context("workspace not initialized — run `proof init` first")?,
        )?;
        let actor_text = config["actor_id"]
            .as_str()
            .context("workspace config missing actor_id")?;
        let actor: PrincipalId =
            PrincipalId::new(uuid::Uuid::parse_str(actor_text).context("invalid actor_id")?);
        let keypair = Self::load_keypair(root)?;
        Ok(Self {
            root: root.clone(),
            keypair,
            actor,
        })
    }

    fn save_json(&self, subdir: &str, id: &str, value: &Value) -> Result<()> {
        let dir = self.root.join(".proof/data").join(subdir);
        std::fs::create_dir_all(&dir)?;
        std::fs::write(
            dir.join(format!("{id}.json")),
            serde_json::to_string_pretty(value)?,
        )?;
        Ok(())
    }

    fn load_json(&self, subdir: &str, id: &str) -> Result<Value> {
        let path = self
            .root
            .join(".proof/data")
            .join(subdir)
            .join(format!("{id}.json"));
        Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
    }

    fn save_proof(&self, proof: &Proof) -> Result<()> {
        let dir = self.root.join(".proof/data/proofs");
        std::fs::create_dir_all(&dir)?;
        std::fs::write(
            dir.join(format!("{}.json", proof.body.id)),
            serde_json::to_string_pretty(proof)?,
        )?;
        Ok(())
    }

    fn make_proof(&self, operation: &str, input: &Value, output: &Value) -> Result<Proof> {
        let input_c = canonicalize(input)?;
        let output_c = canonicalize(output)?;
        let input_digest = digest(ArtifactKind::OperationInput, &input_c);
        let output_digest = digest(ArtifactKind::OperationOutput, &output_c);
        let proof = Proof::new(
            uuid::Uuid::now_v7(),
            self.actor,
            None,
            operation,
            input_digest,
            output_digest,
            chrono::Utc::now(),
        );
        proof
            .sign(&self.keypair)
            .map_err(|e| anyhow::anyhow!("{e}"))
    }
}

#[derive(serde::Deserialize)]
struct StoredKeypair {
    principal_id: uuid::Uuid,
    kind: proof_kernel::PrincipalKind,
    created_at: chrono::DateTime<chrono::Utc>,
    public_key: [u8; 32],
    signing_key: String,
}

impl Workspace {
    fn load_keypair(root: &PathBuf) -> Result<proof_kernel::Keypair> {
        let path = root.join(".proof/keypair.json");
        let raw = std::fs::read_to_string(path)
            .context("workspace keypair missing — run `proof init` first")?;
        let stored: StoredKeypair = serde_json::from_str(&raw)?;
        let signing_key_bytes: Vec<u8> = base64::engine::general_purpose::STANDARD
            .decode(stored.signing_key)
            .context("invalid stored signing key")?;
        let signing_bytes: [u8; 32] = signing_key_bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("stored signing key must be 32 bytes"))?;
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&signing_bytes);
        if signing_key.verifying_key().to_bytes() != stored.public_key {
            bail!("stored keypair public key mismatch");
        }
        let actor = PrincipalId::new(stored.principal_id);
        Ok(proof_kernel::Keypair {
            principal_id: actor,
            kind: stored.kind,
            created_at: stored.created_at,
            signing_key,
        })
    }

    fn rotate(root: &PathBuf) -> Result<proof_kernel::Keypair> {
        let old_keypair = Self::load_keypair(root)?;
        let proof_dir = root.join(".proof");
        let rotated_dir = proof_dir.join("rotated");
        std::fs::create_dir_all(&rotated_dir)?;
        let rotated_at = chrono::Utc::now();
        let rotated_file_name = format!("keypair-{}.json", rotated_at.timestamp_millis());
        let old_keypair_json = serde_json::json!({
            "principal_id": old_keypair.principal_id.as_uuid(),
            "kind": old_keypair.kind,
            "created_at": old_keypair.created_at,
            "public_key": old_keypair.signing_key.verifying_key().to_bytes(),
            "signing_key": base64::engine::general_purpose::STANDARD
                .encode(old_keypair.signing_key.to_bytes()),
            "rotated_at": rotated_at,
        });
        std::fs::write(
            rotated_dir.join(rotated_file_name),
            serde_json::to_string_pretty(&old_keypair_json)?,
        )?;

        let new_keypair = generate_keypair();
        let actor = new_keypair.principal_id;
        let config_path = proof_dir.join("config.json");
        let mut config: Value = serde_json::from_str(&std::fs::read_to_string(&config_path)?)?;
        config["actor_id"] = serde_json::json!(actor.to_string());
        std::fs::write(&config_path, serde_json::to_string_pretty(&config)?)?;
        let keypair_json = serde_json::json!({
            "principal_id": new_keypair.principal_id.as_uuid(),
            "kind": new_keypair.kind,
            "created_at": new_keypair.created_at,
            "public_key": new_keypair.signing_key.verifying_key().to_bytes(),
            "signing_key": base64::engine::general_purpose::STANDARD
                .encode(new_keypair.signing_key.to_bytes()),
        });
        std::fs::write(
            proof_dir.join("keypair.json"),
            serde_json::to_string_pretty(&keypair_json)?,
        )?;

        let storage_dir = proof_dir.join("storage");
        std::fs::create_dir_all(&storage_dir)?;
        let store = SqliteStore::open(&storage_dir.join("storage.db"))
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        store
            .save_principal(&principal_from_keypair(&new_keypair))
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;

        Ok(new_keypair)
    }
}

#[derive(Clone)]
struct ContentOperationHandler {
    operation: String,
}

impl OperationHandler for ContentOperationHandler {
    fn operation(&self) -> &str {
        &self.operation
    }

    fn execute(&self, input: &Value, context: &ExecutionContext) -> Result<Value, ExecutionError> {
        execute_content_operation(&self.operation, input, context)
            .map_err(|error| ExecutionError::HandlerFailed(error.to_string()))
    }
}

fn execute_content_operation(
    operation: &str,
    input: &Value,
    context: &ExecutionContext,
) -> Result<Value, anyhow::Error> {
    let root = context.workspace_path.clone();
    match operation {
        "schema.create" => {
            let name = input["name"]
                .as_str()
                .context("input missing required string: name")?;
            let fields = input.get("fields").cloned().unwrap_or(Value::Array(vec![]));
            let schema = build_schema(name, &fields)?;
            let schema_json = serde_json::to_value(&schema)?;
            save_workspace_json(&root, "schemas", &schema.id.to_string(), &schema_json)?;
            Ok(serde_json::json!({"schema_id": schema.id.to_string()}))
        }
        "object.create" => {
            let schema_id = input["schema_id"]
                .as_str()
                .context("input missing required string: schema_id")?;
            let locale = input["locale"].as_str().unwrap_or("en-US");
            let content = input
                .get("data")
                .cloned()
                .context("input missing required field: data")?;
            let schema: proof_content::schema::SchemaDefinition =
                serde_json::from_value(load_workspace_json(&root, "schemas", schema_id)?)?;
            schema.validate_object(&content)?;
            let object = proof_content::object::Object::create(&schema, locale, content)?;
            let object_json = serde_json::to_value(&object)?;
            save_workspace_json(&root, "objects", &object.id.to_string(), &object_json)?;
            Ok(serde_json::json!({"object_id": object.id.to_string()}))
        }
        "changeset.create" => {
            let intent = input["intent"]
                .as_str()
                .context("input missing required string: intent")?;
            let base_state: BaseState = BTreeMap::new();
            let changeset = proof_content::ChangeSet::new(intent, &base_state, vec![]);
            let changeset_json = serde_json::to_value(&changeset)?;
            save_workspace_json(
                &root,
                "changesets",
                &changeset.id.to_string(),
                &changeset_json,
            )?;
            Ok(serde_json::json!({"changeset_id": changeset.id.to_string()}))
        }
        _ => bail!("no local implementation for operation: {operation}"),
    }
}

fn build_schema(name: &str, fields: &Value) -> Result<proof_content::schema::SchemaDefinition> {
    let mut schema_fields = vec![];
    if let Value::Array(entries) = fields {
        for field in entries {
            let field_name = field["name"]
                .as_str()
                .context("field missing required string: name")?;
            let field_type = match field["field_type"].as_str().unwrap_or("text") {
                "text" => proof_content::schema::FieldType::Text,
                "rich_text" => proof_content::schema::FieldType::RichText,
                "number" => proof_content::schema::FieldType::Number,
                "boolean" => proof_content::schema::FieldType::Boolean,
                "date" => proof_content::schema::FieldType::Date,
                "date_time" => proof_content::schema::FieldType::DateTime,
                "json" => proof_content::schema::FieldType::Json,
                "reference" => proof_content::schema::FieldType::Reference,
                unknown => bail!("unknown field_type: {unknown}"),
            };
            schema_fields.push(proof_content::schema::SchemaField {
                name: field_name.to_string(),
                field_type,
                required: field["required"].as_bool().unwrap_or(false),
                localized: field["localized"].as_bool().unwrap_or(false),
                default_value: field.get("default").cloned(),
            });
        }
    }
    let schema = proof_content::schema::SchemaDefinition::new(name.to_string(), 1, schema_fields);
    schema.validate()?;
    Ok(schema)
}

fn save_workspace_json(root: &PathBuf, subdir: &str, id: &str, value: &Value) -> Result<()> {
    let dir = root.join(".proof/data").join(subdir);
    std::fs::create_dir_all(&dir)?;
    std::fs::write(
        dir.join(format!("{id}.json")),
        serde_json::to_string_pretty(value)?,
    )?;
    Ok(())
}

fn load_workspace_json(root: &PathBuf, subdir: &str, id: &str) -> Result<Value> {
    let path = root
        .join(".proof/data")
        .join(subdir)
        .join(format!("{id}.json"));
    Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
}

fn load_registry(root: &PathBuf) -> Result<Registry> {
    Registry::load_from_directory(root.join(".proof/registry"))
        .map_err(|error| anyhow::anyhow!(error.to_string()))
}

fn build_engine(registry: Registry) -> Result<ExecutionEngine> {
    let mut engine = ExecutionEngine::new(registry);
    for operation in ["schema.create", "object.create", "changeset.create"] {
        engine.register_handler(Arc::new(ContentOperationHandler {
            operation: operation.to_string(),
        }));
    }
    Ok(engine)
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match &cli.command {
        Command::Init => cmd_init(&cli)?,
        Command::SchemaCreate { name, fields } => cmd_schema_create(&cli, name, fields)?,
        Command::ObjectCreate {
            schema_id,
            locale,
            data,
        } => cmd_object_create(&cli, schema_id, locale, data)?,
        Command::ChangesetCreate { intent } => cmd_changeset_create(&cli, intent)?,
        Command::EditionCreate { changeset_id } => cmd_edition_create(&cli, changeset_id)?,
        Command::ReleasePublish {
            edition_id,
            environment,
        } => cmd_release_publish(&cli, edition_id, environment)?,
        Command::Status => cmd_status(&cli)?,
        Command::Capabilities => cmd_capabilities(&cli)?,
        Command::Registry(command) => match command {
            RegistryCommand::List => cmd_registry_list(&cli)?,
            RegistryCommand::Inspect { operation } => cmd_registry_inspect(&cli, operation)?,
        },
        Command::Workspace(command) => match command {
            WorkspaceCommand::Init { path } => cmd_workspace_init(path)?,
            WorkspaceCommand::Status => cmd_workspace_status(&cli)?,
        },
        Command::Keypair(command) => match command {
            KeypairCommand::Export => cmd_keypair_export(&cli)?,
            KeypairCommand::Rotate => cmd_keypair_rotate(&cli)?,
        },
        Command::Verify { proof_id } => cmd_verify(&cli, proof_id)?,
        Command::Execute {
            operation,
            version,
            input,
        } => cmd_execute(&cli, operation, version, input)?,
        Command::Benchmark(command) => match command {
            BenchmarkCommand::Run {
                operation,
                version,
                threshold_ms,
                runs,
                input,
            } => cmd_benchmark_run(&cli, operation, version, *threshold_ms, *runs, input)?,
            BenchmarkCommand::Report => cmd_benchmark_report(&cli)?,
        },
        Command::Delegation(command) => match command {
            DelegationCommand::Grant { agent_id, scope } => {
                cmd_delegation_grant(&cli, agent_id, scope)?
            }
            DelegationCommand::List => cmd_delegation_list(&cli)?,
            DelegationCommand::Revoke { delegation_id } => {
                cmd_delegation_revoke(&cli, delegation_id)?
            }
            DelegationCommand::Validate { delegation_id } => {
                cmd_delegation_validate(&cli, delegation_id)?
            }
        },
        Command::Export { output } => cmd_export(&cli, output)?,
        Command::Import { input } => cmd_import(&cli, input)?,
    }
    Ok(())
}

fn cmd_init(cli: &Cli) -> Result<()> {
    let ws = Workspace::init(&cli.workspace)?;
    println!(
        "{}",
        serde_json::json!({"status": "initialized", "actor_id": ws.actor.to_string()})
    );
    Ok(())
}

fn cmd_schema_create(cli: &Cli, name: &str, fields_json: &str) -> Result<()> {
    let ws = Workspace::open(&cli.workspace)?;
    let fields_value: Value = serde_json::from_str(fields_json)?;
    let mut schema_fields = vec![];
    if let Value::Array(arr) = &fields_value {
        for field in arr {
            let fname = field["name"].as_str().context("field missing name")?;
            let ftype = match field["field_type"].as_str().unwrap_or("text") {
                "text" => FieldType::Text,
                "rich_text" => FieldType::RichText,
                "number" => FieldType::Number,
                "boolean" => FieldType::Boolean,
                "date" => FieldType::Date,
                "date_time" => FieldType::DateTime,
                "json" => FieldType::Json,
                "reference" => FieldType::Reference,
                _ => FieldType::Text,
            };
            schema_fields.push(SchemaField {
                name: fname.to_string(),
                field_type: ftype,
                required: field["required"].as_bool().unwrap_or(false),
                localized: field["localized"].as_bool().unwrap_or(false),
                default_value: field.get("default").cloned(),
            });
        }
    }
    let schema = SchemaDefinition::new(name.to_string(), 1, schema_fields);
    schema.validate()?;
    let schema_json = serde_json::to_value(&schema)?;
    ws.save_json("schemas", &schema.id.to_string(), &schema_json)?;
    let proof = ws.make_proof(
        "schema.create",
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

fn cmd_object_create(cli: &Cli, schema_id: &str, locale: &str, data: &str) -> Result<()> {
    let ws = Workspace::open(&cli.workspace)?;
    let content: Value = serde_json::from_str(data)?;
    let _schema_uuid = uuid::Uuid::parse_str(schema_id)?;
    let schema_json = ws.load_json("schemas", schema_id)?;
    let schema: SchemaDefinition = serde_json::from_value(schema_json)?;
    schema.validate_object(&content)?;
    let object = Object::create(&schema, locale, content)?;
    let object_json = serde_json::to_value(&object)?;
    ws.save_json("objects", &object.id.to_string(), &object_json)?;
    let proof = ws.make_proof(
        "object.create",
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

fn cmd_changeset_create(cli: &Cli, intent: &str) -> Result<()> {
    let ws = Workspace::open(&cli.workspace)?;
    let base_state: BaseState = BTreeMap::new();
    let changeset = proof_content::ChangeSet::new(intent, &base_state, vec![]);
    let cs_json = serde_json::to_value(&changeset)?;
    ws.save_json("changesets", &changeset.id.to_string(), &cs_json)?;
    let proof = ws.make_proof(
        "changeset.create",
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

fn cmd_edition_create(cli: &Cli, changeset_id: &str) -> Result<()> {
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
    let proof = ws.make_proof("edition.create",
        &serde_json::json!({"changeset_id": changeset_id}),
        &serde_json::json!({"edition_id": edition.id.to_string(), "content_digest": edition.content_digest}))?;
    ws.save_proof(&proof)?;
    println!(
        "{}",
        serde_json::json!({"status": "created", "type": "edition", "id": edition.id.to_string(), "content_digest": edition.content_digest, "proof_id": proof.body.id.to_string()})
    );
    Ok(())
}

fn cmd_release_publish(cli: &Cli, edition_id: &str, environment: &str) -> Result<()> {
    let ws = Workspace::open(&cli.workspace)?;
    let edition_uuid = uuid::Uuid::parse_str(edition_id)?;
    let release = Release::new(
        edition_uuid,
        environment.to_string(),
        proof_content::PrincipalId(uuid::Uuid::parse_str(&ws.actor.to_string()).unwrap()),
    );
    ws.save_json(
        "releases",
        &release.id.to_string(),
        &serde_json::to_value(&release)?,
    )?;
    let proof = ws.make_proof(
        "release.publish",
        &serde_json::json!({"edition_id": edition_id, "environment": environment}),
        &serde_json::json!({"release_id": release.id.to_string()}),
    )?;
    ws.save_proof(&proof)?;
    println!(
        "{}",
        serde_json::json!({"status": "published", "type": "release", "id": release.id.to_string(), "edition_id": edition_id, "environment": environment, "proof_id": proof.body.id.to_string()})
    );
    Ok(())
}

fn cmd_status(cli: &Cli) -> Result<()> {
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

fn cmd_workspace_init(path: &str) -> Result<()> {
    let root = PathBuf::from(path);
    let ws = Workspace::init(&root)?;
    println!(
        "{}",
        serde_json::json!({
            "status": "initialized",
            "workspace_path": root.display().to_string(),
            "actor_id": ws.actor.to_string(),
        })
    );
    Ok(())
}

fn cmd_workspace_status(cli: &Cli) -> Result<()> {
    let root = cli
        .workspace
        .canonicalize()
        .unwrap_or_else(|_| cli.workspace.clone());
    let registry_count = load_registry(&root)
        .map(|registry| registry.operations().len())
        .unwrap_or(0);
    let proofs_dir = root.join(".proof/data/proofs");
    let proof_count = std::fs::read_dir(&proofs_dir)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok())
                .filter(|entry| entry.path().extension().map_or(false, |ext| ext == "json"))
                .count()
        })
        .unwrap_or(0);
    let db_path = root.join(".proof/storage/storage.db");
    let principal_count = if db_path.exists() {
        let store =
            SqliteStore::open(&db_path).map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let count: u64 =
            store
                .connection()
                .query_row("SELECT COUNT(*) FROM principals", [], |row| row.get(0))?;
        count as usize
    } else {
        0
    };
    println!(
        "{}",
        serde_json::json!({
            "workspace_path": root.display().to_string(),
            "registered_operations": registry_count,
            "stored_proofs": proof_count,
            "stored_principals": principal_count,
        })
    );
    Ok(())
}

fn cmd_keypair_export(cli: &Cli) -> Result<()> {
    let keypair = Workspace::load_keypair(&cli.workspace)?;
    let public_key = keypair.signing_key.verifying_key().to_bytes();
    println!(
        "{}",
        serde_json::json!({
            "principal_id": keypair.principal_id.to_string(),
            "public_key": base64::engine::general_purpose::STANDARD.encode(public_key),
        })
    );
    Ok(())
}

fn cmd_keypair_rotate(cli: &Cli) -> Result<()> {
    let old_keypair = Workspace::load_keypair(&cli.workspace)?;
    let new_keypair = Workspace::rotate(&cli.workspace)?;
    println!(
        "{}",
        serde_json::json!({
            "status": "rotated",
            "old_principal_id": old_keypair.principal_id.to_string(),
            "new_principal_id": new_keypair.principal_id.to_string(),
        })
    );
    Ok(())
}

fn cmd_capabilities(cli: &Cli) -> Result<()> {
    let registry = load_registry(&cli.workspace)?;
    let ops: Vec<Value> = registry.operations().iter().map(|op| {
        serde_json::json!({"operation": op.operation, "domain": op.domain, "version": op.version, "governance": format!("{:?}", op.governance).to_lowercase()})
    }).collect();
    println!(
        "{}",
        serde_json::json!({"count": ops.len(), "operations": ops})
    );
    Ok(())
}

fn cmd_registry_list(cli: &Cli) -> Result<()> {
    let registry = load_registry(&cli.workspace)?;
    let operations: Vec<Value> = registry
        .operations()
        .iter()
        .map(|entry| {
            serde_json::json!({
                "operation": entry.operation,
                "version": entry.version,
                "domain": entry.domain,
                "action": entry.action,
                "governance": entry.governance,
            })
        })
        .collect();
    println!(
        "{}",
        serde_json::json!({"count": operations.len(), "operations": operations})
    );
    Ok(())
}

fn cmd_registry_inspect(cli: &Cli, operation: &str) -> Result<()> {
    let registry = load_registry(&cli.workspace)?;
    let entries: Vec<&proof_kernel::RegistryEntry> = registry
        .operations()
        .iter()
        .filter(|entry| entry.operation == operation)
        .collect();
    if entries.is_empty() {
        bail!("operation not found: {operation}");
    }
    let values: Vec<Value> = entries
        .iter()
        .map(|entry| serde_json::to_value(entry).map_err(anyhow::Error::from))
        .collect::<Result<_>>()?;
    if values.len() == 1 {
        println!("{}", values[0]);
    } else {
        println!(
            "{}",
            serde_json::json!({"count": values.len(), "versions": values})
        );
    }
    Ok(())
}

fn cmd_verify(cli: &Cli, proof_id: &str) -> Result<()> {
    let root = &cli.workspace;
    let proof_path = root
        .join(".proof/data/proofs")
        .join(format!("{proof_id}.json"));
    let raw = std::fs::read_to_string(&proof_path)
        .with_context(|| format!("proof not found: {proof_id}"))?;
    let proof: Proof = serde_json::from_str(&raw).context("invalid proof JSON")?;
    let keypair = Workspace::load_keypair(root)?;
    if proof.body.actor != keypair.principal_id {
        bail!(
            "proof actor {} does not match stored keypair actor {}",
            proof.body.actor,
            keypair.principal_id
        );
    }
    proof
        .verify(&keypair.signing_key.verifying_key())
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    println!(
        "{}",
        serde_json::json!({
            "proof_id": proof.body.id.to_string(),
            "operation": proof.body.operation,
            "actor_id": proof.body.actor.to_string(),
            "valid": true,
        })
    );
    Ok(())
}

fn cmd_execute(cli: &Cli, operation: &str, version: &str, input: &str) -> Result<()> {
    let ws = Workspace::open(&cli.workspace)?;
    let input_value: Value = serde_json::from_str(input).context("invalid input JSON")?;
    let context = ExecutionContext {
        actor: ws.actor,
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
    let proof = create_proof(
        ws.actor,
        context.delegation_id,
        operation,
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

fn benchmark_contract(entry: &RegistryEntry, threshold_ms: u64) -> Result<Benchmark> {
    let benchmark_id = entry.benchmark.as_deref().with_context(|| {
        format!(
            "operation {} {} does not declare a benchmark",
            entry.operation, entry.version
        )
    })?;
    if threshold_ms == 0 {
        bail!("--threshold-ms must be greater than zero");
    }
    BenchmarkRunner::benchmark(
        benchmark_id,
        "CLI operation benchmark",
        threshold_ms,
        serde_json::json!({"type": "object"}),
    )
    .map_err(|error| anyhow::anyhow!(error.to_string()))
}

fn benchmark_summary(results: &[BenchmarkResult]) -> Value {
    let mut durations: Vec<u64> = results.iter().map(|result| result.duration_ms).collect();
    durations.sort_unstable();
    let percentile = |percentile: f64| -> u64 {
        if durations.is_empty() {
            0
        } else {
            let index = ((durations.len() as f64 - 1.0) * percentile).ceil() as usize;
            durations[index.min(durations.len() - 1)]
        }
    };
    let count = durations.len() as u64;
    let total: u128 = durations.iter().map(|duration| *duration as u128).sum();
    serde_json::json!({
        "runs": count,
        "passed": results.iter().filter(|result| result.passed).count(),
        "failed": results.iter().filter(|result| !result.passed).count(),
        "avg_ms": if count == 0 { 0.0 } else { total as f64 / count as f64 },
        "p95_ms": percentile(0.95),
        "max_ms": durations.last().copied().unwrap_or(0),
    })
}

fn cmd_benchmark_run(
    cli: &Cli,
    operation: &str,
    version: &str,
    threshold_ms: u64,
    runs: u32,
    input: &str,
) -> Result<()> {
    if runs == 0 {
        bail!("--runs must be greater than zero");
    }
    let ws = Workspace::open(&cli.workspace)?;
    let input_value: Value = serde_json::from_str(input).context("invalid input JSON")?;
    let registry = load_registry(&ws.root)?;
    let entry = registry
        .find(operation, version)
        .with_context(|| format!("operation not found: {operation} {version}"))?
        .clone();
    let benchmark = benchmark_contract(&entry, threshold_ms)?;
    let engine = build_engine(registry)?;
    let context = ExecutionContext {
        actor: ws.actor,
        delegation_id: None,
        delegation_chain: None,
        workspace_path: ws.root.clone(),
        timestamp: chrono::Utc::now(),
    };
    let runner = BenchmarkRunner;
    let mut results = Vec::with_capacity(runs as usize);
    let store = open_store(&ws.root)?;
    for _ in 0..runs {
        let result = runner
            .run(
                &engine,
                &benchmark,
                operation,
                version,
                &input_value,
                &context,
            )
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        store
            .save_benchmark_result(&result)
            .map_err(anyhow::Error::from)?;
        results.push(result);
    }
    let summary = benchmark_summary(&results);
    let passed = summary["failed"] == 0;
    println!(
        "{}",
        serde_json::json!({
            "status": if passed { "passed" } else { "failed" },
            "operation": operation,
            "version": version,
            "benchmark": benchmark.name,
            "threshold_ms": threshold_ms,
            "summary": summary,
        })
    );
    if !passed {
        bail!("benchmark failed");
    }
    Ok(())
}

fn cmd_benchmark_report(cli: &Cli) -> Result<()> {
    let ws = Workspace::open(&cli.workspace)?;
    let store = open_store(&ws.root)?;
    let connection = store.connection();
    let mut statement = connection.prepare_cached(
        "
        SELECT
            operation,
            version,
            COUNT(*) AS runs,
            SUM(passed) AS passed,
            ROUND(AVG(duration_ms), 3) AS avg_ms,
            MAX(duration_ms) AS max_ms
        FROM benchmark_results
        GROUP BY operation, version
        ORDER BY operation, version
        ",
    )?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, f64>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    println!(
        "{:<28} {:<10} {:>6} {:>8} {:>12} {:>10}",
        "OPERATION", "VERSION", "RUNS", "PASSED", "AVG MS", "MAX MS"
    );
    for (operation, version, runs, passed, average, maximum) in rows {
        println!(
            "{:<28} {:<10} {:>6} {:>8} {:>12.3} {:>10}",
            operation, version, runs, passed, average, maximum
        );
    }
    Ok(())
}

fn open_store(root: &PathBuf) -> Result<SqliteStore> {
    let database_path = root.join(".proof/storage/storage.db");
    if let Some(parent) = database_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    SqliteStore::open(&database_path).map_err(anyhow::Error::from)
}

fn delegation_from_row(
    id: String,
    issuer: String,
    recipient: String,
    allowed_actions: String,
    resource_scope: String,
    valid_from: String,
    valid_until: String,
    revoked: i64,
) -> Result<(Delegation, serde_json::Value)> {
    let parse_json = |label: &str, value: &str| {
        serde_json::from_str::<Vec<String>>(value)
            .with_context(|| format!("invalid delegation {label}: {value}"))
    };
    let delegation = Delegation {
        id: uuid::Uuid::parse_str(&id)?,
        issuer: PrincipalId::new(uuid::Uuid::parse_str(&issuer)?),
        recipient: PrincipalId::new(uuid::Uuid::parse_str(&recipient)?),
        allowed_actions: parse_json("allowed_actions", &allowed_actions)?,
        resource_scope: parse_json("resource_scope", &resource_scope)?,
        scope: Default::default(),
        valid_from: chrono::DateTime::parse_from_rfc3339(&valid_from)?.with_timezone(&chrono::Utc),
        valid_until: chrono::DateTime::parse_from_rfc3339(&valid_until)?
            .with_timezone(&chrono::Utc),
        revoked: revoked != 0,
    };
    let summary = serde_json::json!({
        "delegation_id": delegation.id.to_string(),
        "issuer": delegation.issuer.to_string(),
        "recipient": delegation.recipient.to_string(),
        "allowed_actions": delegation.allowed_actions,
        "resource_scope": delegation.resource_scope,
        "valid_from": delegation.valid_from.to_rfc3339(),
        "valid_until": delegation.valid_until.to_rfc3339(),
        "revoked": delegation.revoked,
    });
    Ok((delegation, summary))
}

fn save_delegation(store: &SqliteStore, delegation: &Delegation) -> Result<()> {
    store.connection().execute(
        "
        INSERT INTO delegations (
            id, issuer, recipient, allowed_actions, resource_scope,
            valid_from, valid_until, revoked
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        ON CONFLICT(id) DO UPDATE SET
            issuer = excluded.issuer,
            recipient = excluded.recipient,
            allowed_actions = excluded.allowed_actions,
            resource_scope = excluded.resource_scope,
            valid_from = excluded.valid_from,
            valid_until = excluded.valid_until,
            revoked = excluded.revoked
        ",
        rusqlite::params![
            delegation.id.to_string(),
            delegation.issuer.to_string(),
            delegation.recipient.to_string(),
            serde_json::to_string(&delegation.allowed_actions)?,
            serde_json::to_string(&delegation.resource_scope)?,
            delegation.valid_from.to_rfc3339(),
            delegation.valid_until.to_rfc3339(),
            delegation.revoked,
        ],
    )?;
    Ok(())
}

fn save_delegation_principal(
    store: &SqliteStore,
    principal_id: PrincipalId,
    kind: proof_kernel::PrincipalKind,
    public_key: Option<&ed25519_dalek::VerifyingKey>,
    _created_at: chrono::DateTime<chrono::Utc>,
) -> Result<()> {
    let public_key = match public_key {
        Some(public_key) => public_key.as_bytes().to_vec(),
        None => vec![0u8; 32],
    };
    store.connection().execute(
        "
        INSERT INTO principals (id, kind, display_name, public_key)
        VALUES (?1, ?2, ?3, ?4)
        ON CONFLICT(id) DO UPDATE SET
            kind = excluded.kind,
            display_name = excluded.display_name
        ",
        rusqlite::params![
            principal_id.to_string(),
            serde_json::to_string(&kind)?,
            serde_json::to_string(&kind)?,
            public_key,
        ],
    )?;
    Ok(())
}

fn load_delegations(store: &SqliteStore) -> Result<Vec<Delegation>> {
    let connection = store.connection();
    let mut statement = connection.prepare(
        "
        SELECT id, issuer, recipient, allowed_actions, resource_scope,
               valid_from, valid_until, revoked
        FROM delegations
        ORDER BY valid_from, id
        ",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, i64>(7)?,
        ))
    })?;
    rows.map(|row| {
        let (
            id,
            issuer,
            recipient,
            allowed_actions,
            resource_scope,
            valid_from,
            valid_until,
            revoked,
        ) = row?;
        delegation_from_row(
            id,
            issuer,
            recipient,
            allowed_actions,
            resource_scope,
            valid_from,
            valid_until,
            revoked,
        )
        .map(|(delegation, _)| delegation)
    })
    .collect()
}

fn load_delegation(store: &SqliteStore, delegation_id: &str) -> Result<Delegation> {
    let id = uuid::Uuid::parse_str(delegation_id).context("invalid delegation ID")?;
    let delegation = store
        .connection()
        .query_row(
            "
            SELECT id, issuer, recipient, allowed_actions, resource_scope,
                   valid_from, valid_until, revoked
            FROM delegations
            WHERE id = ?1
            ",
            [id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, i64>(7)?,
                ))
            },
        )
        .map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => anyhow::anyhow!("delegation not found: {id}"),
            error => error.into(),
        })?;
    let (id, issuer, recipient, allowed_actions, resource_scope, valid_from, valid_until, revoked) =
        delegation;
    Ok(delegation_from_row(
        id,
        issuer,
        recipient,
        allowed_actions,
        resource_scope,
        valid_from,
        valid_until,
        revoked,
    )?
    .0)
}

fn revoke_delegation(
    store: &SqliteStore,
    delegation_id: &str,
    issuer: PrincipalId,
) -> Result<usize> {
    let id = uuid::Uuid::parse_str(delegation_id).context("invalid delegation ID")?;
    let changed = store.connection().execute(
        "UPDATE delegations SET revoked = TRUE WHERE id = ?1 AND issuer = ?2",
        rusqlite::params![id.to_string(), issuer.to_string()],
    )?;
    Ok(changed)
}

fn cmd_delegation_grant(cli: &Cli, agent_id: &str, scope_json: &str) -> Result<()> {
    let ws = Workspace::open(&cli.workspace)?;
    let scope: serde_json::Value =
        serde_json::from_str(scope_json).context("invalid scope JSON")?;
    let allowed_actions = scope["actions"]
        .as_array()
        .context("scope missing actions array")?
        .iter()
        .map(|action| {
            action
                .as_str()
                .context("scope action must be a string")
                .map(ToString::to_string)
        })
        .collect::<Result<Vec<_>>>()?;
    let resource_scope = scope["resources"]
        .as_array()
        .context("scope missing resources array")?
        .iter()
        .map(|resource| {
            resource
                .as_str()
                .context("scope resource must be a string")
                .map(ToString::to_string)
        })
        .collect::<Result<Vec<_>>>()?;
    let agent_uuid = uuid::Uuid::parse_str(agent_id).context("invalid agent ID")?;
    let delegation = Delegation {
        id: uuid::Uuid::now_v7(),
        issuer: ws.actor,
        recipient: PrincipalId::new(agent_uuid),
        allowed_actions,
        resource_scope,
        scope: Default::default(),
        valid_from: chrono::Utc::now(),
        valid_until: chrono::Utc::now() + chrono::Duration::hours(24),
        revoked: false,
    };
    let store = open_store(&ws.root)?;
    save_delegation_principal(
        &store,
        ws.actor,
        proof_kernel::PrincipalKind::Human,
        Some(&ws.keypair.signing_key.verifying_key()),
        ws.keypair.created_at,
    )?;
    save_delegation_principal(
        &store,
        delegation.recipient,
        proof_kernel::PrincipalKind::Agent,
        None,
        delegation.valid_from,
    )?;
    save_delegation(&store, &delegation)?;
    println!(
        "{}",
        serde_json::json!({
            "status": "granted",
            "delegation_id": delegation.id.to_string(),
            "agent_id": agent_id,
            "valid_until": delegation.valid_until.to_rfc3339(),
        })
    );
    Ok(())
}

fn cmd_delegation_list(cli: &Cli) -> Result<()> {
    let ws = Workspace::open(&cli.workspace)?;
    let store = open_store(&ws.root)?;
    let delegations = load_delegations(&store)?;
    let summaries: Vec<_> = delegations
        .iter()
        .map(|delegation| {
            serde_json::json!({
                "delegation_id": delegation.id.to_string(),
                "issuer": delegation.issuer.to_string(),
                "recipient": delegation.recipient.to_string(),
                "allowed_actions": delegation.allowed_actions,
                "resource_scope": delegation.resource_scope,
                "valid_from": delegation.valid_from.to_rfc3339(),
                "valid_until": delegation.valid_until.to_rfc3339(),
                "revoked": delegation.revoked,
            })
        })
        .collect();
    println!(
        "{}",
        serde_json::json!({"count": summaries.len(), "delegations": summaries})
    );
    Ok(())
}

fn cmd_delegation_revoke(cli: &Cli, delegation_id: &str) -> Result<()> {
    let ws = Workspace::open(&cli.workspace)?;
    let store = open_store(&ws.root)?;
    let changed = revoke_delegation(&store, delegation_id, ws.actor)?;
    if changed == 0 {
        bail!("delegation not found: {delegation_id}");
    }
    println!(
        "{}",
        serde_json::json!({"status": "revoked", "delegation_id": delegation_id})
    );
    Ok(())
}

fn cmd_delegation_validate(cli: &Cli, delegation_id: &str) -> Result<()> {
    let ws = Workspace::open(&cli.workspace)?;
    let store = open_store(&ws.root)?;
    let delegation = load_delegation(&store, delegation_id)?;
    let chain = DelegationChain {
        root: delegation.issuer,
        grants: vec![delegation.clone()],
    };
    let now = chrono::Utc::now();
    let result = chain
        .validate(delegation.recipient, now)
        .map_err(|error| anyhow::anyhow!(error.to_string()));
    let valid = result.is_ok();
    let reason = result.err().map(|error| error.to_string());
    println!(
        "{}",
        serde_json::json!({
            "delegation_id": delegation.id.to_string(),
            "issuer": delegation.issuer.to_string(),
            "recipient": delegation.recipient.to_string(),
            "valid": valid,
            "checked_at": now.to_rfc3339(),
            "reason": reason,
        })
    );
    Ok(())
}

#[derive(serde::Serialize, serde::Deserialize)]
struct WorkspaceArchive {
    format_version: u32,
    exported_at: chrono::DateTime<chrono::Utc>,
    registry: Value,
    proofs: Vec<Proof>,
    principals: Vec<ExportedPrincipal>,
    delegations: Vec<Delegation>,
    blobs: Vec<ExportedBlob>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ExportedPrincipal {
    id: String,
    kind: proof_kernel::PrincipalKind,
    public_key: [u8; 32],
    created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ExportedBlob {
    digest: String,
    size: u64,
    created_at: String,
    content: Vec<u8>,
    references: Vec<ExportedBlobReference>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ExportedBlobReference {
    artifact_kind: String,
    artifact_id: String,
    created_at: String,
}

fn load_exported_principal(row: (String, String, Vec<u8>, String)) -> Result<ExportedPrincipal> {
    let (id, kind, public_key, created_at) = row;
    let public_key_bytes: [u8; 32] = public_key
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid exported principal public key"))?;
    Ok(ExportedPrincipal {
        id,
        kind: serde_json::from_str(&kind)?,
        public_key: public_key_bytes,
        created_at: chrono::DateTime::parse_from_rfc3339(&created_at)?.with_timezone(&chrono::Utc),
    })
}

fn store_exported_principal(store: &SqliteStore, principal: &ExportedPrincipal) -> Result<()> {
    store.connection().execute(
        "
        INSERT INTO principals (id, kind, display_name, public_key)
        VALUES (?1, ?2, ?3, ?4)
        ON CONFLICT(id) DO UPDATE SET
            kind = excluded.kind,
            display_name = excluded.display_name,
            public_key = excluded.public_key
        ",
        rusqlite::params![
            principal.id,
            serde_json::to_string(&principal.kind)?,
            serde_json::to_string(&principal.kind)?,
            principal.public_key.as_slice(),
        ],
    )?;
    Ok(())
}

fn load_exported_delegations(store: &SqliteStore) -> Result<Vec<Delegation>> {
    let connection = store.connection();
    let mut statement = connection.prepare(
        "
        SELECT id, issuer, recipient, allowed_actions, resource_scope,
               valid_from, valid_until, revoked
        FROM delegations
        ORDER BY id
        ",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, i64>(7)?,
        ))
    })?;
    rows.map(|row| {
        let (
            id,
            issuer,
            recipient,
            allowed_actions,
            resource_scope,
            valid_from,
            valid_until,
            revoked,
        ) = row?;
        delegation_from_row(
            id,
            issuer,
            recipient,
            allowed_actions,
            resource_scope,
            valid_from,
            valid_until,
            revoked,
        )
        .map(|(delegation, _)| delegation)
    })
    .collect()
}

fn load_exported_blobs(store: &proof_storage::ContentAddressedStore) -> Result<Vec<ExportedBlob>> {
    let mut blobs = Vec::new();
    let rows: Vec<(String, u64, String)> = {
        let connection = store.connection();
        let mut statement = connection
            .prepare("SELECT digest, size_bytes, created_at FROM content_blobs ORDER BY digest")?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, u64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    for (digest_hex, size, created_at) in rows {
        let mut digest_bytes = [0_u8; 32];
        hex::decode_to_slice(&digest_hex, &mut digest_bytes)
            .map_err(|_| anyhow::anyhow!("invalid blob digest: {digest_hex}"))?;
        let digest = proof_kernel::ContentDigest::from_bytes(digest_bytes);
        let content = store
            .get(&digest)?
            .ok_or_else(|| anyhow::anyhow!("missing blob content: {digest_hex}"))?;
        let mut references = Vec::new();
        for (artifact_kind, artifact_id) in store.references(&digest)? {
            references.push(ExportedBlobReference {
                artifact_kind,
                artifact_id,
                created_at: chrono::Utc::now().to_rfc3339(),
            });
        }
        blobs.push(ExportedBlob {
            digest: digest_hex,
            size,
            created_at,
            content,
            references,
        });
    }
    Ok(blobs)
}

fn store_exported_blob(
    store: &proof_storage::ContentAddressedStore,
    blob: &ExportedBlob,
) -> Result<()> {
    let digest = store.put(&blob.content)?;
    if digest.hex() != blob.digest {
        bail!(
            "blob content digest mismatch: expected {}, got {}",
            blob.digest,
            digest.hex()
        );
    }
    for reference in &blob.references {
        store.add_reference(
            &digest,
            proof_storage::BlobReference {
                artifact_kind: &reference.artifact_kind,
                artifact_id: &reference.artifact_id,
            },
        )?;
    }
    Ok(())
}

fn add_directory_to_archive(
    builder: &mut tar::Builder<flate2::write::GzEncoder<std::fs::File>>,
    source: &Path,
    prefix: &str,
) -> Result<()> {
    if !source.exists() {
        return Ok(());
    }
    builder.append_dir_all(prefix, source)?;
    Ok(())
}

fn read_archive_files(
    archive: &mut tar::Archive<flate2::read::GzDecoder<std::fs::File>>,
) -> Result<Vec<(String, String, Value)>> {
    let mut files = Vec::new();
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry
            .path()?
            .components()
            .map(|component| component.as_os_str().to_string_lossy().to_string())
            .collect::<Vec<_>>();
        if path.len() != 3 || path[0] != "workspace-data" {
            continue;
        }
        let mut contents = String::new();
        entry.read_to_string(&mut contents)?;
        let value = serde_json::from_str(&contents)?;
        files.push((path[1].clone(), path[2].clone(), value));
    }
    Ok(files)
}

fn import_workspace_files(root: &Path, files: Vec<(String, String, Value)>) -> Result<()> {
    for (subdirectory, id, value) in files {
        let directory = root.join(".proof/data").join(subdirectory);
        std::fs::create_dir_all(&directory)?;
        std::fs::write(directory.join(id), serde_json::to_string_pretty(&value)?)?;
    }
    Ok(())
}

fn cmd_export(cli: &Cli, output: &Path) -> Result<()> {
    let workspace = Workspace::open(&cli.workspace)?;
    let store = open_store(&workspace.root)?;
    let cas = open_content_store(&workspace.root)?;

    let proofs_dir = workspace.root.join(".proof/data/proofs");
    if proofs_dir.exists() {
        for proof_path in std::fs::read_dir(&proofs_dir)? {
            let proof_path = proof_path?;
            let raw = std::fs::read_to_string(proof_path.path())?;
            let proof: Proof = serde_json::from_str(&raw)?;
            store.save_proof(&proof)?;
        }
    }

    let registry_entries = store.load_registry().map_err(anyhow::Error::from)?;
    let registry = if registry_entries.is_empty() {
        serde_json::to_value(load_registry(&workspace.root)?.operations())?
    } else {
        serde_json::to_value(&registry_entries)?
    };

    let mut proofs = Vec::new();
    let proof_ids = {
        let proof_connection = store.connection();
        let mut proof_statement =
            proof_connection.prepare("SELECT id FROM proofs ORDER BY timestamp, id")?;
        let rows = proof_statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    for proof_id_text in proof_ids {
        let proof_id = uuid::Uuid::parse_str(&proof_id_text)?;
        proofs.push(store.load_proof(&proof_id)?);
    }

    let principal_rows = {
        let principal_connection = store.connection();
        let mut principal_statement = principal_connection
            .prepare("SELECT id, kind, public_key FROM principals ORDER BY id")?;
        let rows = principal_statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                    chrono::Utc::now().to_rfc3339(),
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    let mut principals = Vec::new();
    for row in principal_rows {
        principals.push(load_exported_principal(row)?);
    }

    let archive = WorkspaceArchive {
        format_version: 1,
        exported_at: chrono::Utc::now(),
        registry,
        proofs,
        principals,
        delegations: load_exported_delegations(&store)?,
        blobs: load_exported_blobs(&cas)?,
    };

    let output_file = std::fs::File::create(output)
        .with_context(|| format!("cannot create export archive: {}", output.display()))?;
    let encoder = flate2::write::GzEncoder::new(output_file, flate2::Compression::default());
    let mut builder = tar::Builder::new(encoder);
    let manifest = serde_json::to_vec_pretty(&archive)?;
    let mut header = tar::Header::new_gnu();
    header.set_size(manifest.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder.append_data(&mut header, "manifest.json", manifest.as_slice())?;
    add_directory_to_archive(
        &mut builder,
        &workspace.root.join(".proof/registry"),
        "registry",
    )?;
    add_directory_to_archive(
        &mut builder,
        &workspace.root.join(".proof/data"),
        "workspace-data",
    )?;
    builder.finish()?;
    builder.into_inner()?.finish()?;
    Ok(())
}

fn cmd_import(cli: &Cli, input: &Path) -> Result<()> {
    let workspace = Workspace::open(&cli.workspace)?;
    let store = open_store(&workspace.root)?;
    let cas = open_content_store(&workspace.root)?;

    let input_file = std::fs::File::open(input)
        .with_context(|| format!("cannot open import archive: {}", input.display()))?;
    let decoder = flate2::read::GzDecoder::new(input_file);
    let mut manifest_bytes = Vec::new();
    {
        let mut archive = tar::Archive::new(decoder);
        for entry in archive.entries()? {
            let mut entry = entry?;
            let path = entry.path()?;
            if path == Path::new("manifest.json") {
                entry.read_to_end(&mut manifest_bytes)?;
                break;
            }
        }
    }
    if manifest_bytes.is_empty() {
        bail!("archive missing manifest.json");
    }
    let manifest: WorkspaceArchive = serde_json::from_slice(&manifest_bytes)?;
    if manifest.format_version != 1 {
        bail!(
            "unsupported archive format version: {}",
            manifest.format_version
        );
    }

    let mut imported_proofs = 0_usize;
    let mut newer_proofs = 0_usize;
    for proof in &manifest.proofs {
        let principal = store
            .load_principal(&proof.body.actor)
            .map_err(anyhow::Error::from);
        let public_key = match principal {
            Ok(principal) => principal.public_key,
            Err(_) => {
                let exported = manifest
                    .principals
                    .iter()
                    .find(|principal| principal.id == proof.body.actor.to_string())
                    .with_context(|| format!("missing principal for proof {}", proof.body.id))?;
                ed25519_dalek::VerifyingKey::from_bytes(&exported.public_key)?
            }
        };
        proof.verify(&public_key).map_err(anyhow::Error::from)?;

        let existing = store.load_proof(&proof.body.id).ok();
        if existing
            .as_ref()
            .map(|existing| existing.body.timestamp < proof.body.timestamp)
            .unwrap_or(true)
        {
            store.save_proof(proof)?;
            imported_proofs += 1;
            if existing.is_some() {
                newer_proofs += 1;
            }
        }
    }

    for principal in &manifest.principals {
        store_exported_principal(&store, principal)?;
    }
    for delegation in &manifest.delegations {
        save_delegation(&store, delegation)?;
    }
    for blob in &manifest.blobs {
        store_exported_blob(&cas, blob)?;
    }

    let registry: Vec<proof_kernel::RegistryEntry> = serde_json::from_value(manifest.registry)?;
    store.save_registry(&registry)?;

    let mut files_archive =
        tar::Archive::new(flate2::read::GzDecoder::new(std::fs::File::open(input)?));
    let files = read_archive_files(&mut files_archive)?;
    let workspace_file_count = files.len();
    import_workspace_files(&workspace.root, files)?;

    println!(
        "{}",
        serde_json::json!({
            "status": "imported",
            "format_version": manifest.format_version,
            "proofs": imported_proofs,
            "newer_proofs_replaced": newer_proofs,
            "principals": manifest.principals.len(),
            "delegations": manifest.delegations.len(),
            "blobs": manifest.blobs.len(),
            "workspace_files": workspace_file_count,
        })
    );
    Ok(())
}

fn open_content_store(root: &Path) -> Result<proof_storage::ContentAddressedStore> {
    let storage_directory = root.join(".proof/storage");
    std::fs::create_dir_all(&storage_directory)?;
    proof_storage::ContentAddressedStore::open(
        &storage_directory.join("content.db"),
        &storage_directory.join("blobs"),
    )
    .map_err(anyhow::Error::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_fs::prelude::*;

    #[test]
    fn exports_and_imports_workspace_round_trip() {
        let source = assert_fs::TempDir::new().unwrap();
        let target = assert_fs::TempDir::new().unwrap();
        let archive = source.child("workspace.tar.gz");
        let source_args = Cli::parse_from(["proof", "-w", source.path().to_str().unwrap(), "init"]);
        cmd_init(&source_args).unwrap();
        let source_workspace = Workspace::open(&source.path().to_path_buf()).unwrap();
        let proof = source_workspace
            .make_proof(
                "test.operation::v1",
                &serde_json::json!({"roundtrip": true}),
                &serde_json::json!({"ok": true}),
            )
            .unwrap();
        let source_store = open_store(&source.path().to_path_buf()).unwrap();
        source_store.save_proof(&proof).unwrap();

        let export_args = Cli::parse_from([
            "proof",
            "-w",
            source.path().to_str().unwrap(),
            "export",
            "--output",
            archive.path().to_str().unwrap(),
        ]);
        cmd_export(&export_args, archive.path()).unwrap();
        assert!(archive.path().is_file());

        let import_args = Cli::parse_from([
            "proof",
            "-w",
            target.path().to_str().unwrap(),
            "import",
            "--input",
            archive.path().to_str().unwrap(),
        ]);
        let error = cmd_import(&import_args, archive.path()).unwrap_err();
        assert!(error.to_string().contains("workspace not initialized"));

        cmd_init(&import_args).unwrap();
        cmd_import(&import_args, archive.path()).unwrap();

        let store = open_store(&target.path().to_path_buf()).unwrap();
        assert_eq!(store.proof_count().unwrap(), 1);
        let proof_ids: Vec<(String,)> = store
            .connection()
            .prepare("SELECT id FROM proofs")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?,)))
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(proof_ids.len(), 1);
        let proof = store
            .load_proof(&uuid::Uuid::parse_str(&proof_ids[0].0).unwrap())
            .unwrap();
        let principal = store.load_principal(&proof.body.actor).unwrap();
        proof.verify(&principal.public_key).unwrap();
    }

    #[test]
    fn export_includes_workspace_data_and_import_restores_it() {
        let source = assert_fs::TempDir::new().unwrap();
        let target = assert_fs::TempDir::new().unwrap();
        let archive = source.child("workspace-data.tar.gz");
        let source_args = Cli::parse_from(["proof", "-w", source.path().to_str().unwrap(), "init"]);
        cmd_init(&source_args).unwrap();
        let workspace = Workspace::open(&source.path().to_path_buf()).unwrap();
        let schema = SchemaDefinition::new(
            "post".to_string(),
            1,
            vec![proof_content::SchemaField {
                name: "title".to_string(),
                field_type: proof_content::FieldType::Text,
                required: true,
                localized: false,
                default_value: None,
            }],
        );
        let value = serde_json::to_value(&schema).unwrap();
        workspace
            .save_json("schemas", &schema.id.to_string(), &value)
            .unwrap();
        cmd_export(&source_args, archive.path()).unwrap();

        let import_args = Cli::parse_from(["proof", "-w", target.path().to_str().unwrap(), "init"]);
        cmd_init(&import_args).unwrap();
        cmd_import(&import_args, archive.path()).unwrap();
        assert!(target
            .child(".proof/data/schemas")
            .child(format!("{}.json", schema.id))
            .exists());
    }

    #[test]
    fn benchmark_run_persists_and_report_reads_results() {
        let workspace = assert_fs::TempDir::new().unwrap();
        let init_args =
            Cli::parse_from(["proof", "-w", workspace.path().to_str().unwrap(), "init"]);
        cmd_init(&init_args).unwrap();
        let registry_dir = workspace.path().join(".proof/registry/content");
        std::fs::create_dir_all(&registry_dir).unwrap();
        std::fs::write(
            registry_dir.join("schema-create.json"),
            r#"{"operation":"schema.create","domain":"content","version":"v1","action":"content:schema_create","description":"Create a content Schema definition","input_schema":"content/schema-create.input.json","output_schema":"content/schema-create.output.json","required_authority":"delegation-grant","governance":"agent-executable","idempotency":"required-uuidv7","consequence":"content-mutation","evidence_contract":"operation-effect-v1","benchmark":"B1"}"#,
        )
        .unwrap();

        let run_args = Cli::parse_from([
            "proof",
            "-w",
            workspace.path().to_str().unwrap(),
            "benchmark",
            "run",
            "schema.create",
            "v1",
            "--threshold-ms",
            "1000",
            "--runs",
            "2",
            "--input",
            r#"{"name":"test","version":1,"fields":[{"name":"title","field_type":"text","required":true,"localized":false}]}"#,
        ]);
        cmd_benchmark_run(
            &run_args,
            "schema.create",
            "v1",
            1000,
            2,
            r#"{"name":"test","version":1,"fields":[{"name":"title","field_type":"text","required":true,"localized":false}]}"#,
        )
        .unwrap();

        let report_args = Cli::parse_from([
            "proof",
            "-w",
            workspace.path().to_str().unwrap(),
            "benchmark",
            "report",
        ]);
        cmd_benchmark_report(&report_args).unwrap();

        let store = open_store(&workspace.path().to_path_buf()).unwrap();
        assert_eq!(
            store
                .list_benchmark_results("schema.create", "v1")
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn import_rejects_invalid_proof_signature() {
        let source = assert_fs::TempDir::new().unwrap();
        let target = assert_fs::TempDir::new().unwrap();
        let archive = source.child("invalid.tar.gz");
        let source_args = Cli::parse_from(["proof", "-w", source.path().to_str().unwrap(), "init"]);
        cmd_init(&source_args).unwrap();
        let workspace_keypair = Workspace::open(&source.path().to_path_buf())
            .unwrap()
            .keypair;
        let mut workspace = Workspace::open(&source.path().to_path_buf()).unwrap();
        let proof = workspace
            .make_proof(
                "test.operation::v1",
                &serde_json::json!({"a": 1}),
                &serde_json::json!({"b": 2}),
            )
            .unwrap();
        let exported_principal = ExportedPrincipal {
            id: workspace_keypair.principal_id.to_string(),
            kind: workspace_keypair.kind,
            public_key: workspace_keypair.signing_key.verifying_key().to_bytes(),
            created_at: workspace_keypair.created_at,
        };
        let mut archive_manifest = WorkspaceArchive {
            format_version: 1,
            exported_at: chrono::Utc::now(),
            registry: serde_json::json!([]),
            proofs: vec![proof],
            principals: vec![exported_principal],
            delegations: vec![],
            blobs: vec![],
        };
        archive_manifest.proofs[0].signature = ed25519_dalek::Signature::from_bytes(&[1; 64]);

        let output_file = std::fs::File::create(archive.path()).unwrap();
        let encoder = flate2::write::GzEncoder::new(output_file, flate2::Compression::default());
        let mut builder = tar::Builder::new(encoder);
        let manifest = serde_json::to_vec_pretty(&archive_manifest).unwrap();
        let mut header = tar::Header::new_gnu();
        header.set_size(manifest.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, "manifest.json", manifest.as_slice())
            .unwrap();
        builder.finish().unwrap();
        builder.into_inner().unwrap().finish().unwrap();

        cmd_init(&Cli::parse_from([
            "proof",
            "-w",
            target.path().to_str().unwrap(),
            "init",
        ]))
        .unwrap();
        let import_args = Cli::parse_from([
            "proof",
            "-w",
            target.path().to_str().unwrap(),
            "import",
            "--input",
            archive.path().to_str().unwrap(),
        ]);
        let error = cmd_import(&import_args, archive.path()).unwrap_err();
        println!("IMPORT ERROR: {}", error);
        assert!(error.to_string().contains("invalid proof signature"));
    }

    #[test]
    fn import_keeps_newer_existing_proof() {
        let source = assert_fs::TempDir::new().unwrap();
        let target = assert_fs::TempDir::new().unwrap();
        let archive = source.child("older.tar.gz");
        let source_args = Cli::parse_from(["proof", "-w", source.path().to_str().unwrap(), "init"]);
        cmd_init(&source_args).unwrap();
        let mut workspace = Workspace::open(&source.path().to_path_buf()).unwrap();
        let source_keypair = workspace.keypair.clone();
        let proof = workspace
            .make_proof(
                "test.operation::v1",
                &serde_json::json!({"a": 1}),
                &serde_json::json!({"b": 2}),
            )
            .unwrap();
        let archive_manifest = WorkspaceArchive {
            format_version: 1,
            exported_at: chrono::Utc::now(),
            registry: serde_json::json!([]),
            proofs: vec![proof.clone()],
            principals: vec![ExportedPrincipal {
                id: source_keypair.principal_id.to_string(),
                kind: source_keypair.kind,
                public_key: source_keypair.signing_key.verifying_key().to_bytes(),
                created_at: source_keypair.created_at,
            }],
            delegations: vec![],
            blobs: vec![],
        };
        create_archive(archive.path(), &archive_manifest).unwrap();

        cmd_init(&Cli::parse_from([
            "proof",
            "-w",
            target.path().to_str().unwrap(),
            "init",
        ]))
        .unwrap();
        let target_workspace = Workspace::open(&target.path().to_path_buf()).unwrap();
        let mut newer = Proof::new(
            proof.body.id,
            target_workspace.actor,
            None,
            proof.body.operation.clone(),
            proof.body.input_digest,
            proof.body.output_digest,
            proof.body.timestamp + chrono::Duration::seconds(1),
        );
        newer = newer.sign(&target_workspace.keypair).unwrap();
        {
            let target_store = open_store(&target.path().to_path_buf()).unwrap();
            target_store.save_proof(&newer).unwrap();
        }
        let import_args = Cli::parse_from([
            "proof",
            "-w",
            target.path().to_str().unwrap(),
            "import",
            "--input",
            archive.path().to_str().unwrap(),
        ]);
        cmd_import(&import_args, archive.path()).unwrap();
        let store = open_store(&target.path().to_path_buf()).unwrap();
        let stored = store.load_proof(&proof.body.id).unwrap();
        assert_eq!(stored.body.timestamp, newer.body.timestamp);
    }

    fn create_archive(path: &Path, manifest: &WorkspaceArchive) -> Result<()> {
        let output_file = std::fs::File::create(path)?;
        let encoder = flate2::write::GzEncoder::new(output_file, flate2::Compression::default());
        let mut builder = tar::Builder::new(encoder);
        let bytes = serde_json::to_vec_pretty(manifest)?;
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append_data(&mut header, "manifest.json", bytes.as_slice())?;
        builder.finish()?;
        builder.into_inner()?.finish()?;
        Ok(())
    }
}
