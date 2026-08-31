pub mod commands;
pub mod workspace;

pub use workspace::{load_workspace_json, save_workspace_json, Workspace};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use proof_kernel::{principal_from_keypair, ExecutionEngine, Registry};
use proof_storage::SqliteStore;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "proof", about = "Proof Platform CLI", version)]
pub(crate) struct Cli {
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
    ChangesetCommit {
        #[arg(short, long)]
        changeset_id: String,
        #[arg(long)]
        idempotency_key: String,
        #[arg(long)]
        notes: Option<String>,
    },
    EditionCreate {
        #[arg(short, long)]
        changeset_id: String,
        #[arg(long)]
        idempotency_key: String,
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
    #[command(subcommand)]
    Approval(ApprovalCommand),
    #[command(subcommand)]
    Agent(AgentCommand),
    #[command(subcommand)]
    Run(RunCommand),
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
enum ApprovalCommand {
    ApproverInit,
    List,
    /// Start the local operator approval console.
    Ui {
        #[arg(long, default_value_t = 0)]
        port: u16,
    },
    Approve {
        request_id: String,
        #[arg(long)]
        approver: String,
        #[arg(long)]
        reason: Option<String>,
    },
    Deny {
        request_id: String,
        #[arg(long)]
        approver: String,
        #[arg(long)]
        reason: Option<String>,
    },
}

#[derive(Subcommand)]
enum AgentCommand {
    Create {
        #[arg(long)]
        name: String,
        #[arg(long)]
        instructions: String,
        #[arg(long, default_value = "openai")]
        provider: String,
        #[arg(long)]
        model: String,
        #[arg(long = "tool", required = true)]
        tools: Vec<String>,
        #[arg(long, default_value_t = 16)]
        max_steps: u32,
        #[arg(long, default_value_t = 24)]
        max_model_calls: u32,
        #[arg(long, default_value_t = 100_000)]
        max_total_tokens: u64,
        #[arg(long, default_value_t = 900)]
        max_duration_seconds: u64,
        #[arg(long, default_value_t = 4_096)]
        max_output_tokens_per_call: u32,
        #[arg(long)]
        max_cost_microusd: Option<u64>,
    },
    List,
    Inspect {
        agent_id: String,
    },
    Start {
        agent_id: String,
        #[arg(long)]
        goal: String,
    },
    Resume {
        run_id: String,
    },
    /// Start the frozen Release Manager live journey after deterministic preflight.
    LiveStart {
        agent_id: String,
        #[arg(long)]
        goal: String,
        #[arg(long)]
        policy_file: PathBuf,
        #[arg(long)]
        preflight_evaluation_id: String,
        #[arg(long)]
        delegation_id: String,
    },
    /// Resume the exact sealed Release Manager live run and authority binding.
    LiveResume {
        run_id: String,
        #[arg(long)]
        policy_file: PathBuf,
    },
    /// Build fresh deterministic evidence without touching a provider boundary.
    #[command(subcommand)]
    LivePrepare(LivePrepareCommand),
    Watch {
        run_id: String,
    },
    /// Evaluate a terminal agent trace against a deterministic task policy.
    Evaluate {
        run_id: String,
        #[arg(long)]
        evaluator: String,
        #[arg(long)]
        policy_file: PathBuf,
    },
}

#[derive(Subcommand)]
enum LivePrepareCommand {
    /// Start the deterministic v1 rehearsal and stop at signed approval.
    Start { preparation_id: String },
    /// Resume the approved rehearsal and produce a checked readiness packet.
    Finish {
        preparation_id: String,
        #[arg(long)]
        agent_id: String,
        #[arg(long)]
        delegation_id: String,
        #[arg(long)]
        policy_file: PathBuf,
    },
}

#[derive(Subcommand)]
enum RunCommand {
    Start {
        #[arg(long)]
        goal: String,
    },
    List,
    Inspect {
        run_id: String,
    },
    Checkpoint {
        run_id: String,
        #[arg(long)]
        state: String,
    },
    Retry {
        run_id: String,
        step_id: String,
    },
    Complete {
        run_id: String,
    },
    Cancel {
        run_id: String,
    },
    Evaluate {
        run_id: String,
        #[arg(long)]
        evaluator: String,
        #[arg(long)]
        outcome: String,
        #[arg(long)]
        score_bps: Option<u16>,
        #[arg(long, default_value = "{}")]
        metrics: String,
        #[arg(long)]
        summary: Option<String>,
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

pub(crate) fn load_registry(root: &PathBuf) -> Result<Registry> {
    Registry::load_from_directory(root.join(".proof/registry"))
        .map_err(|error| anyhow::anyhow!(error.to_string()))
}

pub(crate) fn open_store(root: &PathBuf) -> Result<SqliteStore> {
    let database_path = root.join(".proof/storage/storage.db");
    if let Some(parent) = database_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    SqliteStore::open(&database_path).map_err(anyhow::Error::from)
}

pub(crate) fn open_content_store(root: &Path) -> Result<proof_storage::ContentAddressedStore> {
    let storage_directory = root.join(".proof/storage");
    std::fs::create_dir_all(&storage_directory)?;
    proof_storage::ContentAddressedStore::open(
        &storage_directory.join("content.db"),
        &storage_directory.join("blobs"),
    )
    .map_err(anyhow::Error::from)
}

pub(crate) fn build_engine(
    registry: Registry,
    keypair: proof_kernel::Keypair,
    store: Arc<SqliteStore>,
) -> Result<ExecutionEngine> {
    store
        .save_principal(&principal_from_keypair(&keypair))
        .map_err(anyhow::Error::from)?;
    let mut engine = ExecutionEngine::new_with_keypair(registry, keypair).with_storage(store);
    for handler in proof_content::content_handlers() {
        engine.register_handler(handler);
    }
    Ok(engine)
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match &cli.command {
        Command::Init => commands::content::cmd_init(&cli)?,
        Command::SchemaCreate { name, fields } => {
            commands::content::cmd_schema_create(&cli, name, fields)?
        }
        Command::ObjectCreate {
            schema_id,
            locale,
            data,
        } => commands::content::cmd_object_create(&cli, schema_id, locale, data)?,
        Command::ChangesetCreate { intent } => {
            commands::content::cmd_changeset_create(&cli, intent)?
        }
        Command::ChangesetCommit {
            changeset_id,
            idempotency_key,
            notes,
        } => commands::content::cmd_changeset_commit(
            &cli,
            changeset_id,
            idempotency_key,
            notes.as_deref(),
        )?,
        Command::EditionCreate {
            changeset_id,
            idempotency_key,
        } => commands::content::cmd_edition_create(&cli, changeset_id, idempotency_key)?,
        Command::ReleasePublish {
            edition_id,
            environment,
        } => commands::content::cmd_release_publish(&cli, edition_id, environment)?,
        Command::Status => commands::content::cmd_status(&cli)?,
        Command::Capabilities => commands::registry::cmd_capabilities(&cli)?,
        Command::Registry(command) => match command {
            RegistryCommand::List => commands::registry::cmd_registry_list(&cli)?,
            RegistryCommand::Inspect { operation } => {
                commands::registry::cmd_registry_inspect(&cli, operation)?
            }
        },
        Command::Workspace(command) => match command {
            WorkspaceCommand::Init { path } => commands::workspace::cmd_workspace_init(path)?,
            WorkspaceCommand::Status => commands::workspace::cmd_workspace_status(&cli)?,
        },
        Command::Keypair(command) => match command {
            KeypairCommand::Export => commands::workspace::cmd_keypair_export(&cli)?,
            KeypairCommand::Rotate => commands::workspace::cmd_keypair_rotate(&cli)?,
        },
        Command::Verify { proof_id } => commands::registry::cmd_verify(&cli, proof_id)?,
        Command::Execute {
            operation,
            version,
            input,
        } => commands::content::cmd_execute(&cli, operation, version, input)?,
        Command::Benchmark(command) => match command {
            BenchmarkCommand::Run {
                operation,
                version,
                threshold_ms,
                runs,
                input,
            } => commands::benchmark::cmd_benchmark_run(
                &cli,
                operation,
                version,
                *threshold_ms,
                *runs,
                input,
            )?,
            BenchmarkCommand::Report => commands::benchmark::cmd_benchmark_report(&cli)?,
        },
        Command::Delegation(command) => match command {
            DelegationCommand::Grant { agent_id, scope } => {
                commands::delegation::cmd_delegation_grant(&cli, agent_id, scope)?
            }
            DelegationCommand::List => commands::delegation::cmd_delegation_list(&cli)?,
            DelegationCommand::Revoke { delegation_id } => {
                commands::delegation::cmd_delegation_revoke(&cli, delegation_id)?
            }
            DelegationCommand::Validate { delegation_id } => {
                commands::delegation::cmd_delegation_validate(&cli, delegation_id)?
            }
        },
        Command::Approval(command) => match command {
            ApprovalCommand::ApproverInit => commands::approval::cmd_approver_init(&cli)?,
            ApprovalCommand::List => commands::approval::cmd_approval_list(&cli)?,
            ApprovalCommand::Ui { port } => commands::approval_ui::cmd_approval_ui(&cli, *port)?,
            ApprovalCommand::Approve {
                request_id,
                approver,
                reason,
            } => commands::approval::cmd_approval_approve(
                &cli,
                request_id,
                approver,
                reason.as_deref(),
            )?,
            ApprovalCommand::Deny {
                request_id,
                approver,
                reason,
            } => commands::approval::cmd_approval_deny(
                &cli,
                request_id,
                approver,
                reason.as_deref(),
            )?,
        },
        Command::Agent(command) => match command {
            AgentCommand::Create {
                name,
                instructions,
                provider,
                model,
                tools,
                max_steps,
                max_model_calls,
                max_total_tokens,
                max_duration_seconds,
                max_output_tokens_per_call,
                max_cost_microusd,
            } => commands::agent::cmd_agent_create(
                &cli,
                name,
                instructions,
                provider,
                model,
                tools,
                *max_steps,
                *max_model_calls,
                *max_total_tokens,
                *max_duration_seconds,
                *max_output_tokens_per_call,
                *max_cost_microusd,
            )?,
            AgentCommand::List => commands::agent::cmd_agent_list(&cli)?,
            AgentCommand::Inspect { agent_id } => {
                commands::agent::cmd_agent_inspect(&cli, agent_id)?
            }
            AgentCommand::Start { agent_id, goal } => {
                commands::agent::cmd_agent_start(&cli, agent_id, goal)?
            }
            AgentCommand::Resume { run_id } => commands::agent::cmd_agent_resume(&cli, run_id)?,
            AgentCommand::LiveStart {
                agent_id,
                goal,
                policy_file,
                preflight_evaluation_id,
                delegation_id,
            } => commands::live::cmd_agent_live_start(
                &cli,
                agent_id,
                goal,
                policy_file,
                preflight_evaluation_id,
                delegation_id,
            )?,
            AgentCommand::LiveResume {
                run_id,
                policy_file,
            } => commands::live::cmd_agent_live_resume(&cli, run_id, policy_file)?,
            AgentCommand::LivePrepare(command) => match command {
                LivePrepareCommand::Start { preparation_id } => {
                    commands::live_prepare::cmd_live_prepare_start(&cli, preparation_id)?
                }
                LivePrepareCommand::Finish {
                    preparation_id,
                    agent_id,
                    delegation_id,
                    policy_file,
                } => commands::live_prepare::cmd_live_prepare_finish(
                    &cli,
                    preparation_id,
                    agent_id,
                    delegation_id,
                    policy_file,
                )?,
            },
            AgentCommand::Watch { run_id } => commands::agent::cmd_agent_watch(&cli, run_id)?,
            AgentCommand::Evaluate {
                run_id,
                evaluator,
                policy_file,
            } => commands::agent::cmd_agent_evaluate(&cli, run_id, evaluator, policy_file)?,
        },
        Command::Run(command) => match command {
            RunCommand::Start { goal } => commands::run::cmd_run_start(&cli, goal)?,
            RunCommand::List => commands::run::cmd_run_list(&cli)?,
            RunCommand::Inspect { run_id } => commands::run::cmd_run_inspect(&cli, run_id)?,
            RunCommand::Checkpoint { run_id, state } => {
                commands::run::cmd_run_checkpoint(&cli, run_id, state)?
            }
            RunCommand::Retry { run_id, step_id } => {
                commands::run::cmd_run_retry(&cli, run_id, step_id)?
            }
            RunCommand::Complete { run_id } => commands::run::cmd_run_complete(&cli, run_id)?,
            RunCommand::Cancel { run_id } => commands::run::cmd_run_cancel(&cli, run_id)?,
            RunCommand::Evaluate {
                run_id,
                evaluator,
                outcome,
                score_bps,
                metrics,
                summary,
            } => commands::run::cmd_run_evaluate(
                &cli,
                run_id,
                evaluator,
                outcome,
                *score_bps,
                metrics,
                summary.as_deref(),
            )?,
        },
        Command::Export { output } => commands::transfer::cmd_export(&cli, output)?,
        Command::Import { input } => commands::transfer::cmd_import(&cli, input)?,
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_fs::prelude::*;
    use commands::transfer::{ExportedPrincipal, WorkspaceArchive};
    use proof_content::schema::SchemaDefinition;
    use proof_kernel::Proof;

    #[test]
    fn parses_frozen_live_start_and_resume_commands() {
        let start = Cli::try_parse_from([
            "proof",
            "agent",
            "live-start",
            "018f0000-0000-7000-8000-000000000001",
            "--goal",
            "Publish edition AXP-E0001",
            "--policy-file",
            "policy.json",
            "--preflight-evaluation-id",
            "018f0000-0000-7000-8000-000000000002",
            "--delegation-id",
            "018f0000-0000-7000-8000-000000000003",
        ])
        .unwrap();
        assert!(matches!(
            start.command,
            Command::Agent(AgentCommand::LiveStart {
                agent_id,
                goal,
                policy_file,
                preflight_evaluation_id,
                delegation_id,
            }) if agent_id == "018f0000-0000-7000-8000-000000000001"
                && goal == "Publish edition AXP-E0001"
                && policy_file == PathBuf::from("policy.json")
                && preflight_evaluation_id == "018f0000-0000-7000-8000-000000000002"
                && delegation_id == "018f0000-0000-7000-8000-000000000003"
        ));

        let resume = Cli::try_parse_from([
            "proof",
            "agent",
            "live-resume",
            "018f0000-0000-7000-8000-000000000004",
            "--policy-file",
            "policy.json",
        ])
        .unwrap();
        assert!(matches!(
            resume.command,
            Command::Agent(AgentCommand::LiveResume { run_id, policy_file })
                if run_id == "018f0000-0000-7000-8000-000000000004"
                    && policy_file == PathBuf::from("policy.json")
        ));
    }

    #[test]
    fn parses_credential_free_live_prepare_phases() {
        let start = Cli::try_parse_from([
            "proof",
            "agent",
            "live-prepare",
            "start",
            "018f0000-0000-7000-8000-000000000005",
        ])
        .unwrap();
        assert!(matches!(
            start.command,
            Command::Agent(AgentCommand::LivePrepare(LivePrepareCommand::Start {
                preparation_id
            })) if preparation_id == "018f0000-0000-7000-8000-000000000005"
        ));

        let finish = Cli::try_parse_from([
            "proof",
            "agent",
            "live-prepare",
            "finish",
            "018f0000-0000-7000-8000-000000000005",
            "--agent-id",
            "018f0000-0000-7000-8000-000000000006",
            "--delegation-id",
            "018f0000-0000-7000-8000-000000000007",
            "--policy-file",
            "policy.json",
        ])
        .unwrap();
        assert!(matches!(
            finish.command,
            Command::Agent(AgentCommand::LivePrepare(LivePrepareCommand::Finish {
                preparation_id,
                agent_id,
                delegation_id,
                policy_file,
            })) if preparation_id == "018f0000-0000-7000-8000-000000000005"
                && agent_id == "018f0000-0000-7000-8000-000000000006"
                && delegation_id == "018f0000-0000-7000-8000-000000000007"
                && policy_file == PathBuf::from("policy.json")
        ));
    }

    #[test]
    fn legacy_release_publish_fails_closed_without_mutation() {
        let workspace = assert_fs::TempDir::new().unwrap();
        let cli = Cli::parse_from(["proof", "-w", workspace.path().to_str().unwrap(), "init"]);
        commands::content::cmd_init(&cli).unwrap();

        let error = commands::content::cmd_release_publish(
            &cli,
            "018f0000-0000-7000-8000-000000000001",
            "preview",
        )
        .unwrap_err();

        assert!(error.to_string().contains("release.publish is human-only"));
        assert_eq!(
            std::fs::read_dir(workspace.path().join(".proof/data/releases"))
                .unwrap()
                .count(),
            0
        );
        assert_eq!(
            open_store(&workspace.path().to_path_buf())
                .unwrap()
                .proof_count()
                .unwrap(),
            0
        );
    }

    #[test]
    fn exports_and_imports_workspace_round_trip() {
        let source = assert_fs::TempDir::new().unwrap();
        let target = assert_fs::TempDir::new().unwrap();
        let archive = source.child("workspace.tar.gz");
        let source_args = Cli::parse_from(["proof", "-w", source.path().to_str().unwrap(), "init"]);
        commands::content::cmd_init(&source_args).unwrap();
        let source_workspace = Workspace::open(&source.path().to_path_buf()).unwrap();
        let proof = source_workspace
            .make_proof(
                "test.operation",
                "v1",
                &serde_json::json!({"roundtrip": true}),
                &serde_json::json!({"ok": true}),
            )
            .unwrap();
        let source_store = open_store(&source.path().to_path_buf()).unwrap();
        source_store.save_proof(&proof).unwrap();
        let delegation = proof_kernel::Delegation {
            id: uuid::Uuid::now_v7(),
            issuer: source_workspace.actor,
            recipient: source_workspace.actor,
            allowed_actions: vec!["content:release_publish".to_string()],
            resource_scope: vec!["preview".to_string()],
            scope: proof_kernel::delegation::DelegationScope {
                allowed_operations: Some(vec!["release.publish".to_string()]),
                allowed_domains: Some(vec!["content".to_string()]),
                resource_scope: None,
            },
            valid_from: chrono::Utc::now(),
            valid_until: chrono::Utc::now() + chrono::Duration::hours(1),
            revoked: false,
        };
        source_store.save_delegation(&delegation).unwrap();

        let export_args = Cli::parse_from([
            "proof",
            "-w",
            source.path().to_str().unwrap(),
            "export",
            "--output",
            archive.path().to_str().unwrap(),
        ]);
        commands::transfer::cmd_export(&export_args, archive.path()).unwrap();
        assert!(archive.path().is_file());

        let import_args = Cli::parse_from([
            "proof",
            "-w",
            target.path().to_str().unwrap(),
            "import",
            "--input",
            archive.path().to_str().unwrap(),
        ]);
        let error = commands::transfer::cmd_import(&import_args, archive.path()).unwrap_err();
        assert!(error.to_string().contains("workspace not initialized"));

        commands::content::cmd_init(&import_args).unwrap();
        commands::transfer::cmd_import(&import_args, archive.path()).unwrap();

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
        assert_eq!(
            store.load_delegation(&delegation.id).unwrap().unwrap(),
            delegation,
            "workspace transfer must preserve complete structured delegation scope"
        );
    }

    #[test]
    fn export_includes_workspace_data_and_import_restores_it() {
        let source = assert_fs::TempDir::new().unwrap();
        let target = assert_fs::TempDir::new().unwrap();
        let archive = source.child("workspace-data.tar.gz");
        let source_args = Cli::parse_from(["proof", "-w", source.path().to_str().unwrap(), "init"]);
        commands::content::cmd_init(&source_args).unwrap();
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
        commands::transfer::cmd_export(&source_args, archive.path()).unwrap();

        let import_args = Cli::parse_from(["proof", "-w", target.path().to_str().unwrap(), "init"]);
        commands::content::cmd_init(&import_args).unwrap();
        commands::transfer::cmd_import(&import_args, archive.path()).unwrap();
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
        commands::content::cmd_init(&init_args).unwrap();
        let registry_dir = workspace.path().join(".proof/registry/content");
        std::fs::create_dir_all(&registry_dir).unwrap();
        std::fs::write(
            registry_dir.join("schema-create.json"),
            r#"{"operation":"schema.create","domain":"content","version":"v1","action":"content:schema_create","description":"Create a content Schema definition","input_schema":"content/schema-create.input.json","output_schema":"content/schema-create.output.json","required_authority":"delegation-grant","governance":"agent-executable","idempotency":"required-uuidv7","consequence":"content-mutation","evidence_contract":"operation-effect-v1","benchmark":"B1"}"#,
        )
        .unwrap();
        std::fs::copy(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("registry/content/schema-create.input.json"),
            registry_dir.join("schema-create.input.json"),
        )
        .unwrap();
        let benchmark_input = serde_json::to_string(&SchemaDefinition::new(
            "test".to_string(),
            1,
            vec![proof_content::SchemaField {
                name: "title".to_string(),
                field_type: proof_content::FieldType::Text,
                required: true,
                localized: false,
                default_value: None,
            }],
        ))
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
            &benchmark_input,
        ]);
        commands::benchmark::cmd_benchmark_run(
            &run_args,
            "schema.create",
            "v1",
            1000,
            2,
            &benchmark_input,
        )
        .unwrap();

        let report_args = Cli::parse_from([
            "proof",
            "-w",
            workspace.path().to_str().unwrap(),
            "benchmark",
            "report",
        ]);
        commands::benchmark::cmd_benchmark_report(&report_args).unwrap();

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
        commands::content::cmd_init(&source_args).unwrap();
        let workspace_keypair = Workspace::open(&source.path().to_path_buf())
            .unwrap()
            .keypair;
        let mut workspace = Workspace::open(&source.path().to_path_buf()).unwrap();
        let proof = workspace
            .make_proof(
                "test.operation",
                "v1",
                &serde_json::json!({"a": 1}),
                &serde_json::json!({"b": 2}),
            )
            .unwrap();
        let exported_principal = commands::transfer::ExportedPrincipal {
            id: workspace_keypair.principal_id.to_string(),
            kind: workspace_keypair.kind,
            public_key: workspace_keypair.signing_key.verifying_key().to_bytes(),
            created_at: workspace_keypair.created_at,
        };
        let mut archive_manifest = commands::transfer::WorkspaceArchive {
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

        commands::content::cmd_init(&Cli::parse_from([
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
        let error = commands::transfer::cmd_import(&import_args, archive.path()).unwrap_err();
        println!("IMPORT ERROR: {}", error);
        assert!(error.to_string().contains("invalid proof signature"));
    }

    #[test]
    fn import_keeps_newer_existing_proof() {
        let source = assert_fs::TempDir::new().unwrap();
        let target = assert_fs::TempDir::new().unwrap();
        let archive = source.child("older.tar.gz");
        let source_args = Cli::parse_from(["proof", "-w", source.path().to_str().unwrap(), "init"]);
        commands::content::cmd_init(&source_args).unwrap();
        let mut workspace = Workspace::open(&source.path().to_path_buf()).unwrap();
        let source_keypair = workspace.keypair.clone();
        let proof = workspace
            .make_proof(
                "test.operation",
                "v1",
                &serde_json::json!({"a": 1}),
                &serde_json::json!({"b": 2}),
            )
            .unwrap();
        let archive_manifest = commands::transfer::WorkspaceArchive {
            format_version: 1,
            exported_at: chrono::Utc::now(),
            registry: serde_json::json!([]),
            proofs: vec![proof.clone()],
            principals: vec![commands::transfer::ExportedPrincipal {
                id: source_keypair.principal_id.to_string(),
                kind: source_keypair.kind,
                public_key: source_keypair.signing_key.verifying_key().to_bytes(),
                created_at: source_keypair.created_at,
            }],
            delegations: vec![],
            blobs: vec![],
        };
        tests::create_archive(archive.path(), &archive_manifest).unwrap();

        commands::content::cmd_init(&Cli::parse_from([
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
        commands::transfer::cmd_import(&import_args, archive.path()).unwrap();
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
