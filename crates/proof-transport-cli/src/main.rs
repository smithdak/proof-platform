use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use proof_content::{
    changeset::BaseState,
    edition::Edition,
    object::Object,
    release::Release,
    schema::{FieldType, SchemaDefinition, SchemaField},
};
use proof_kernel::{
    canonicalize, digest, generate_keypair, ArtifactKind, PrincipalId, Proof, Registry,
};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;

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
        for subdir in ["schemas", "objects", "changesets", "editions", "releases", "proofs"] {
            std::fs::create_dir_all(proof_dir.join("data").join(subdir))?;
        }
        let keypair = generate_keypair();
        let actor = keypair.principal_id;
        let config = serde_json::json!({
            "actor_id": actor.to_string(),
            "version": "0.1.0",
        });
        std::fs::write(proof_dir.join("config.json"), serde_json::to_string_pretty(&config)?)?;
        Ok(Self { root: root.clone(), keypair, actor })
    }

    fn open(root: &PathBuf) -> Result<Self> {
        let config_path = root.join(".proof/config.json");
        let config: Value = serde_json::from_str(
            &std::fs::read_to_string(&config_path)
                .context("workspace not initialized — run `proof init` first")?,
        )?;
        let _ = config;
        let keypair = generate_keypair();
        let actor = keypair.principal_id;
        Ok(Self { root: root.clone(), keypair, actor })
    }

    fn save_json(&self, subdir: &str, id: &str, value: &Value) -> Result<()> {
        let dir = self.root.join(".proof/data").join(subdir);
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join(format!("{id}.json")), serde_json::to_string_pretty(value)?)?;
        Ok(())
    }

    fn load_json(&self, subdir: &str, id: &str) -> Result<Value> {
        let path = self.root.join(".proof/data").join(subdir).join(format!("{id}.json"));
        Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
    }

    fn save_proof(&self, proof: &Proof) -> Result<()> {
        let dir = self.root.join(".proof/data/proofs");
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join(format!("{}.json", proof.body.id)), serde_json::to_string_pretty(proof)?)?;
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
        proof.sign(&self.keypair).map_err(|e| anyhow::anyhow!("{e}"))
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match &cli.command {
        Command::Init => cmd_init(&cli)?,
        Command::SchemaCreate { name, fields } => cmd_schema_create(&cli, name, fields)?,
        Command::ObjectCreate { schema_id, locale, data } => cmd_object_create(&cli, schema_id, locale, data)?,
        Command::ChangesetCreate { intent } => cmd_changeset_create(&cli, intent)?,
        Command::EditionCreate { changeset_id } => cmd_edition_create(&cli, changeset_id)?,
        Command::ReleasePublish { edition_id, environment } => cmd_release_publish(&cli, edition_id, environment)?,
        Command::Status => cmd_status(&cli)?,
        Command::Capabilities => cmd_capabilities(&cli)?,
    }
    Ok(())
}

fn cmd_init(cli: &Cli) -> Result<()> {
    let ws = Workspace::init(&cli.workspace)?;
    println!("{}", serde_json::json!({"status": "initialized", "actor_id": ws.actor.to_string()}));
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
    let proof = ws.make_proof("schema.create",
        &serde_json::json!({"name": name}),
        &serde_json::json!({"schema_id": schema.id.to_string()}))?;
    ws.save_proof(&proof)?;
    println!("{}", serde_json::json!({"status": "created", "type": "schema", "id": schema.id.to_string(), "proof_id": proof.body.id.to_string()}));
    Ok(())
}

fn cmd_object_create(cli: &Cli, schema_id: &str, locale: &str, data: &str) -> Result<()> {
    let ws = Workspace::open(&cli.workspace)?;
    let content: Value = serde_json::from_str(data)?;
    let schema_uuid = uuid::Uuid::parse_str(schema_id)?;
    let schema_json = ws.load_json("schemas", schema_id)?;
    let schema: SchemaDefinition = serde_json::from_value(schema_json)?;
    schema.validate_object(&content)?;
    let object = Object::create(&schema, locale, content)?;
    let object_json = serde_json::to_value(&object)?;
    ws.save_json("objects", &object.id.to_string(), &object_json)?;
    let proof = ws.make_proof("object.create",
        &serde_json::json!({"schema_id": schema_id}),
        &serde_json::json!({"object_id": object.id.to_string()}))?;
    ws.save_proof(&proof)?;
    println!("{}", serde_json::json!({"status": "created", "type": "object", "id": object.id.to_string(), "proof_id": proof.body.id.to_string()}));
    Ok(())
}

fn cmd_changeset_create(cli: &Cli, intent: &str) -> Result<()> {
    let ws = Workspace::open(&cli.workspace)?;
    let base_state: BaseState = BTreeMap::new();
    let changeset = proof_content::ChangeSet::new(intent, &base_state, vec![]);
    let cs_json = serde_json::to_value(&changeset)?;
    ws.save_json("changesets", &changeset.id.to_string(), &cs_json)?;
    let proof = ws.make_proof("changeset.create",
        &serde_json::json!({"intent": intent}),
        &serde_json::json!({"changeset_id": changeset.id.to_string()}))?;
    ws.save_proof(&proof)?;
    println!("{}", serde_json::json!({"status": "created", "type": "changeset", "id": changeset.id.to_string(), "proof_id": proof.body.id.to_string()}));
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
    ws.save_json("editions", &edition.id.to_string(), &serde_json::to_value(&edition)?)?;
    let proof = ws.make_proof("edition.create",
        &serde_json::json!({"changeset_id": changeset_id}),
        &serde_json::json!({"edition_id": edition.id.to_string(), "content_digest": edition.content_digest}))?;
    ws.save_proof(&proof)?;
    println!("{}", serde_json::json!({"status": "created", "type": "edition", "id": edition.id.to_string(), "content_digest": edition.content_digest, "proof_id": proof.body.id.to_string()}));
    Ok(())
}

fn cmd_release_publish(cli: &Cli, edition_id: &str, environment: &str) -> Result<()> {
    let ws = Workspace::open(&cli.workspace)?;
    let edition_uuid = uuid::Uuid::parse_str(edition_id)?;
    let release = Release::new(edition_uuid, environment.to_string(), proof_content::PrincipalId(uuid::Uuid::parse_str(&ws.actor.to_string()).unwrap()));
    ws.save_json("releases", &release.id.to_string(), &serde_json::to_value(&release)?)?;
    let proof = ws.make_proof("release.publish",
        &serde_json::json!({"edition_id": edition_id, "environment": environment}),
        &serde_json::json!({"release_id": release.id.to_string()}))?;
    ws.save_proof(&proof)?;
    println!("{}", serde_json::json!({"status": "published", "type": "release", "id": release.id.to_string(), "edition_id": edition_id, "environment": environment, "proof_id": proof.body.id.to_string()}));
    Ok(())
}

fn cmd_status(cli: &Cli) -> Result<()> {
    let data_dir = cli.workspace.join(".proof/data");
    let count = |subdir: &str| -> usize {
        std::fs::read_dir(data_dir.join(subdir))
            .map(|entries| entries.filter_map(|e| e.ok()).filter(|e| e.path().extension().map_or(false, |ext| ext == "json")).count())
            .unwrap_or(0)
    };
    println!("{}", serde_json::json!({
        "schemas": count("schemas"),
        "objects": count("objects"),
        "changesets": count("changesets"),
        "editions": count("editions"),
        "releases": count("releases"),
        "proofs": count("proofs"),
    }));
    Ok(())
}

fn cmd_capabilities(cli: &Cli) -> Result<()> {
    let registry_dir = cli.workspace.join(".proof/registry");
    let registry = Registry::load_from_directory(&registry_dir)?;
    let ops: Vec<Value> = registry.operations().iter().map(|op| {
        serde_json::json!({"operation": op.operation, "domain": op.domain, "version": op.version, "governance": format!("{:?}", op.governance).to_lowercase()})
    }).collect();
    println!("{}", serde_json::json!({"count": ops.len(), "operations": ops}));
    Ok(())
}
