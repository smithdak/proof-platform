use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use proof_content::{changeset::BaseState, edition::Edition, object::Object, release::Release, schema::{FieldType, SchemaDefinition, SchemaField}};
use proof_kernel::{
    canonicalize, digest, generate_keypair, ArtifactKind, CanonicalJson, DeriveKeyContext, PrincipalId, Proof, Registry,
};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(name = "proof", about = "Proof Platform CLI", version)]
struct Cli {
    #[arg(short, long)]
    verbose: bool,

    /// Path to the workspace directory
    #[arg(short = 'w', long, default_value = ".")]
    workspace: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Initialize a new Workspace
    Init,
    /// Create a content Schema
    SchemaCreate {
        #[arg(short, long)]
        name: String,
        #[arg(short, long)]
        fields: String,
    },
    /// Create a content Object
    ObjectCreate {
        #[arg(short, long)]
        schema_id: String,
        #[arg(short, long, default_value = "en-US")]
        locale: String,
        #[arg(short, long)]
        data: String,
    },
    /// Show workspace status
    Status,
    /// List available operations from the registry
    Capabilities,
}

struct Workspace {
    root: PathBuf,
    keypair: proof_kernel::Keypair,
    actor: PrincipalId,
}

impl Workspace {
    fn init(root: &Path) -> Result<Self> {
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
            "created_at": chrono::Utc::now().to_rfc3339(),
        });
        std::fs::write(proof_dir.join("config.json"), serde_json::to_string_pretty(&config)?)?;

        Ok(Self { root: root.to_path_buf(), keypair, actor })
    }

    fn open(root: &Path) -> Result<Self> {
        let config_path = root.join(".proof/config.json");
        let config: Value = serde_json::from_str(
            &std::fs::read_to_string(&config_path).context("workspace not initialized — run `proof init` first")?,
        )?;
        let _ = config; // read config for future use
        let keypair = generate_keypair();
        let actor = keypair.principal_id;
        Ok(Self { root: root.to_path_buf(), keypair, actor })
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
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        Command::Init => cmd_init(&cli, cli.verbose)?,
        Command::SchemaCreate { name, fields } => cmd_schema_create(&cli, name, fields, cli.verbose)?,
        Command::ObjectCreate { schema_id, locale, data } => cmd_object_create(&cli, schema_id, locale, data, cli.verbose)?,
        Command::Status => cmd_status(&cli)?,
        Command::Capabilities => cmd_capabilities(&cli)?,
    }
    Ok(())
}

fn cmd_init(cli: &Cli, verbose: bool) -> Result<()> {
    let ws = Workspace::init(&cli.workspace)?;
    let _ = ws;
    if verbose {
        eprintln!("Workspace initialized at {}", cli.workspace.display());
    }
    println!("{}", serde_json::json!({"status": "initialized", "path": cli.workspace.display().to_string()}));
    Ok(())
}

fn cmd_schema_create(cli: &Cli, name: &str, fields_json: &str, verbose: bool) -> Result<()> {
    let ws = Workspace::open(&cli.workspace)?;
    let fields_value: Value = serde_json::from_str(fields_json).context("fields must be valid JSON")?;

    let mut schema_fields = vec![];
    if let Value::Array(arr) = &fields_value {
        for field in arr {
            let field_name = field["name"].as_str().context("field missing name")?;
            let field_type_str = field["field_type"].as_str().unwrap_or("text");
            let field_type = match field_type_str {
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
                name: field_name.to_string(),
                field_type,
                required: field["required"].as_bool().unwrap_or(false),
                localized: field["localized"].as_bool().unwrap_or(false),
                default_value: field.get("default").cloned(),
            });
        }
    }

    let schema = SchemaDefinition {
        id: uuid::Uuid::now_v7(),
        name: name.to_string(),
        version: 1,
        fields: schema_fields,
        created_at: chrono::Utc::now(),
    };

    let schema_json = serde_json::to_value(&schema)?;
    ws.save_json("schemas", &schema.id.to_string(), &schema_json)?;

    let input_canonical = canonicalize(&serde_json::json!({"name": name}))?;
    let output_canonical = canonicalize(&serde_json::json!({"schema_id": schema.id.to_string()}))?;
    let input_digest = digest(ArtifactKind::OperationInput, &input_canonical);
    let output_digest = digest(ArtifactKind::OperationOutput, &output_canonical);

    let mut proof = Proof::new(
        uuid::Uuid::now_v7(),
        ws.actor,
        None,
        "schema.create",
        input_digest,
        output_digest,
        chrono::Utc::now(),
    );
    let signed_proof = proof.sign(&ws.keypair)?;
    ws.save_proof(&signed_proof)?;

    if verbose {
        eprintln!("Schema created: {}", schema.id);
    }
    println!("{}", serde_json::json!({
        "status": "created",
        "type": "schema",
        "id": schema.id.to_string(),
        "name": schema.name,
        "proof_id": signed_proof.body.id.to_string(),
    }));
    Ok(())
}

fn cmd_object_create(cli: &Cli, schema_id: &str, locale: &str, data: &str, verbose: bool) -> Result<()> {
    let ws = Workspace::open(&cli.workspace)?;
    let content: Value = serde_json::from_str(data).context("data must be valid JSON")?;
    let schema_uuid = uuid::Uuid::parse_str(schema_id).context("invalid schema_id")?;

    let schema_json = ws.load_json("schemas", schema_id)?;
    let schema: SchemaDefinition = serde_json::from_value(schema_json)?;

    let object = Object::create(&schema, locale, content)?;
    let object_json = serde_json::to_value(&object)?;
    ws.save_json("objects", &object.id.to_string(), &object_json)?;

    let input_canonical = canonicalize(&serde_json::json!({"schema_id": schema_id, "locale": locale}))?;
    let output_canonical = canonicalize(&serde_json::json!({"object_id": object.id.to_string()}))?;
    let input_digest = digest(ArtifactKind::OperationInput, &input_canonical);
    let output_digest = digest(ArtifactKind::OperationOutput, &output_canonical);

    let mut proof = Proof::new(
        uuid::Uuid::now_v7(),
        ws.actor,
        None,
        "object.create",
        input_digest,
        output_digest,
        chrono::Utc::now(),
    );
    let signed_proof = proof.sign(&ws.keypair)?;
    ws.save_proof(&signed_proof)?;

    if verbose {
        eprintln!("Object created: {}", object.id);
    }
    println!("{}", serde_json::json!({
        "status": "created",
        "type": "object",
        "id": object.id.to_string(),
        "proof_id": signed_proof.body.id.to_string(),
    }));
    Ok(())
}

fn cmd_status(cli: &Cli) -> Result<()> {
    let data_dir = cli.workspace.join(".proof/data");
    let count = |subdir: &str| -> usize {
        let dir = data_dir.join(subdir);
        std::fs::read_dir(&dir).map(|entries| entries.filter_map(|e| e.ok()).filter(|e| {
            e.path().extension().map_or(false, |ext| ext == "json")
        }).count()).unwrap_or(0)
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
        serde_json::json!({
            "operation": op.operation,
            "domain": op.domain,
            "version": op.version,
            "governance": format!("{:?}", op.governance).to_lowercase(),
        })
    }).collect();
    println!("{}", serde_json::json!({"count": ops.len(), "operations": ops}));
    Ok(())
}
