use std::path::Path;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use chrono::Utc;
use proof_agent_runtime::{
    AgentRuntime, AgentRuntimeOutcome, ApprovalEvidence, DeterministicTraceEvaluator,
    OpenAiResponsesGateway, TraceEvaluationPolicy, DEFAULT_OPENAI_BASE_URL,
};
use proof_kernel::{
    AgentDefinition, AgentEvaluationOutcome, AgentLimits, AgentRun, AgentRunEvent,
    AgentRunEventKind, AgentRunStatus, AgentTool, ExecutionEngine, Registry,
};
use proof_storage::SqliteStore;
use uuid::Uuid;

use crate::{load_registry, open_store, Cli, Workspace};

#[allow(clippy::too_many_arguments)]
pub(crate) fn cmd_agent_create(
    cli: &Cli,
    name: &str,
    instructions: &str,
    provider: &str,
    model: &str,
    tools: &[String],
    max_steps: u32,
    max_model_calls: u32,
    max_total_tokens: u64,
    max_duration_seconds: u64,
    max_output_tokens_per_call: u32,
    max_cost_microusd: Option<u64>,
) -> Result<()> {
    let workspace = Workspace::open(&cli.workspace)?;
    let registry = load_registry(&workspace.root)?;
    let tools = tools
        .iter()
        .map(|value| parse_tool(value, &registry))
        .collect::<Result<Vec<_>>>()?;
    let definition = AgentDefinition::new(
        name,
        instructions,
        provider,
        model,
        tools,
        AgentLimits {
            max_steps,
            max_model_calls,
            max_total_tokens,
            max_duration_seconds,
            max_output_tokens_per_call,
            max_cost_microusd,
        },
        Utc::now(),
    )?;
    open_store(&workspace.root)?.save_agent_definition(&definition)?;
    print_json(serde_json::json!({
        "status": "created",
        "agent": definition,
        "next": format!(
            "proof agent start {} --goal <goal>",
            definition.id
        ),
    }))
}

pub(crate) fn cmd_agent_list(cli: &Cli) -> Result<()> {
    let workspace = Workspace::open(&cli.workspace)?;
    let agents = open_store(&workspace.root)?.list_agent_definitions()?;
    print_json(serde_json::json!({"count": agents.len(), "agents": agents}))
}

pub(crate) fn cmd_agent_inspect(cli: &Cli, agent_id: &str) -> Result<()> {
    let workspace = Workspace::open(&cli.workspace)?;
    let store = open_store(&workspace.root)?;
    let agent_id = parse_id(agent_id, "agent")?;
    let agent = store
        .load_agent_definition(&agent_id)?
        .with_context(|| format!("agent definition not found: {agent_id}"))?;
    let runs = store
        .list_agent_runs()?
        .into_iter()
        .filter(|run| run.agent_id == Some(agent_id))
        .collect::<Vec<_>>();
    print_json(serde_json::json!({"agent": agent, "runs": runs}))
}

pub(crate) fn cmd_agent_start(cli: &Cli, agent_id: &str, goal: &str) -> Result<()> {
    let workspace = Workspace::open(&cli.workspace)?;
    let store = Arc::new(open_store(&workspace.root)?);
    let agent_id = parse_id(agent_id, "agent")?;
    let agent = store
        .load_agent_definition(&agent_id)?
        .with_context(|| format!("agent definition not found: {agent_id}"))?;
    ensure_openai_provider(&agent)?;
    let runtime = build_runtime(&workspace, store, load_registry(&workspace.root)?)?;
    print_outcome(runtime.start(agent_id, goal)?)
}

pub(crate) fn cmd_agent_resume(cli: &Cli, run_id: &str) -> Result<()> {
    let workspace = Workspace::open(&cli.workspace)?;
    let store = Arc::new(open_store(&workspace.root)?);
    let run_id = parse_id(run_id, "run")?;
    let run = store
        .load_agent_run(&run_id)?
        .with_context(|| format!("agent run not found: {run_id}"))?;
    let agent_id = run
        .agent_id
        .with_context(|| format!("agent run {run_id} is not bound to an agent definition"))?;
    let agent = store
        .load_agent_definition(&agent_id)?
        .with_context(|| format!("agent definition not found: {agent_id}"))?;
    ensure_openai_provider(&agent)?;
    let runtime = build_runtime(&workspace, store, load_registry(&workspace.root)?)?;
    print_outcome(runtime.resume(run_id)?)
}

pub(crate) fn cmd_agent_watch(cli: &Cli, run_id: &str) -> Result<()> {
    let workspace = Workspace::open(&cli.workspace)?;
    let store = open_store(&workspace.root)?;
    let run_id = parse_id(run_id, "run")?;
    let run = store
        .load_agent_run(&run_id)?
        .with_context(|| format!("agent run not found: {run_id}"))?;
    if run.actor != workspace.actor {
        bail!(
            "agent run {run_id} belongs to actor {}, not workspace actor {}",
            run.actor,
            workspace.actor
        );
    }
    let steps = store.list_agent_run_steps(&run_id)?;
    let checkpoints = store.list_agent_checkpoints(&run_id)?;
    let latest_runtime_state = checkpoints.iter().rev().find_map(|checkpoint| {
        (checkpoint
            .state
            .get("kind")
            .and_then(serde_json::Value::as_str)
            == Some("agent_runtime_v1"))
        .then(|| checkpoint.state.get("runtime").cloned())
        .flatten()
    });
    let mut approvals = Vec::new();
    for request_id in steps.iter().filter_map(|step| step.approval_request_id) {
        approvals.push(serde_json::json!({
            "request": store.load_approval_request(&request_id)?,
            "decision": store.load_approval_decision(&request_id)?,
            "execution": store.load_approval_execution(&request_id)?,
        }));
    }
    let agent = run
        .agent_id
        .map(|agent_id| store.load_agent_definition(&agent_id))
        .transpose()?
        .flatten();
    print_json(serde_json::json!({
        "run": run,
        "agent": agent,
        "state": latest_runtime_state,
        "steps": steps,
        "events": store.list_agent_run_events(&run_id)?,
        "approvals": approvals,
        "evaluations": store.list_agent_run_evaluations(&run_id)?,
    }))
}

pub(crate) fn cmd_agent_evaluate(
    cli: &Cli,
    run_id: &str,
    evaluator_id: &str,
    policy_file: &Path,
) -> Result<()> {
    let workspace = Workspace::open(&cli.workspace)?;
    let store = open_store(&workspace.root)?;
    let run_id = parse_id(run_id, "run")?;
    let initial_run = store
        .load_agent_run(&run_id)?
        .with_context(|| format!("agent run not found: {run_id}"))?;
    if !initial_run.status.is_terminal() {
        bail!(
            "agent run {run_id} is not terminal: {:?}",
            initial_run.status
        );
    }
    let events = store.list_agent_run_events(&run_id)?;
    require_sealed_terminal_trace(&initial_run, &events)?;
    let run = store
        .load_agent_run(&run_id)?
        .with_context(|| format!("agent run disappeared while evaluating: {run_id}"))?;
    require_sealed_terminal_trace(&run, &events)?;
    let agent_id = run
        .agent_id
        .with_context(|| format!("agent run {run_id} is not bound to an agent definition"))?;
    let agent = store
        .load_agent_definition(&agent_id)?
        .with_context(|| format!("agent definition not found: {agent_id}"))?;
    let run_actor = store
        .load_principal(&run.actor)
        .with_context(|| format!("agent run actor is not enrolled: {}", run.actor))?;

    let policy_json = std::fs::read_to_string(policy_file).with_context(|| {
        format!(
            "could not read evaluation policy: {}",
            policy_file.display()
        )
    })?;
    let policy: TraceEvaluationPolicy = serde_json::from_str(&policy_json)
        .with_context(|| format!("invalid evaluation policy: {}", policy_file.display()))?;
    let evaluator = DeterministicTraceEvaluator::new(policy)?;
    let steps = store.list_agent_run_steps(&run_id)?;

    let mut approvals = Vec::new();
    let mut trusted_approvers = Vec::new();
    for request_id in steps.iter().filter_map(|step| step.approval_request_id) {
        let Some(request) = store.load_approval_request(&request_id)? else {
            continue;
        };
        let Some(decision) = store.load_approval_decision(&request_id)? else {
            continue;
        };
        let Some(execution) = store.load_approval_execution(&request_id)? else {
            continue;
        };
        let Ok(approver) = store.load_principal(&decision.body.decided_by) else {
            continue;
        };
        if !trusted_approvers
            .iter()
            .any(|trusted: &proof_kernel::Principal| {
                trusted.id == approver.id
                    && trusted.kind == approver.kind
                    && trusted.public_key == approver.public_key
            })
        {
            trusted_approvers.push(approver.clone());
        }
        approvals.push(ApprovalEvidence::new(
            request, decision, approver, execution,
        ));
    }

    let evaluation = evaluator.evaluate(
        &run,
        &agent,
        &run_actor,
        &trusted_approvers,
        &steps,
        &events,
        &approvals,
        evaluator_id,
        Utc::now(),
    )?;
    store.save_agent_run_evaluation(&evaluation)?;
    let failed = evaluation.outcome == AgentEvaluationOutcome::Failed;
    let failure_summary = evaluation
        .summary
        .clone()
        .unwrap_or_else(|| "deterministic task checks failed".to_string());
    print_json(serde_json::json!({
        "status": if failed { "failed" } else { "passed" },
        "policy_file": policy_file,
        "evaluation": &evaluation,
    }))?;
    if failed {
        bail!("agent evaluation failed for run {run_id}: {failure_summary}");
    }
    Ok(())
}

fn require_sealed_terminal_trace(run: &AgentRun, events: &[AgentRunEvent]) -> Result<()> {
    // SQLite seals cancelled runs at the status transition because the event
    // contract has no cancellation variant. Their last event, if any, is
    // therefore intentionally non-terminal.
    if run.status == AgentRunStatus::Cancelled {
        return Ok(());
    }
    let Some(last) = events.last() else {
        bail!(
            "agent run {} has no terminal event; retry after the runtime seals the trace",
            run.id
        );
    };
    let matches_status = match run.status {
        AgentRunStatus::Succeeded => last.kind == AgentRunEventKind::Completed,
        AgentRunStatus::Failed => matches!(
            last.kind,
            AgentRunEventKind::Failed | AgentRunEventKind::BudgetExceeded
        ),
        _ => false,
    };
    if !matches_status {
        bail!(
            "agent run {} terminal ledger is not sealed: status {:?}, last event {:?}",
            run.id,
            run.status,
            last.kind
        );
    }
    Ok(())
}

fn build_runtime(
    workspace: &Workspace,
    store: Arc<SqliteStore>,
    registry: Registry,
) -> Result<AgentRuntime> {
    let mut engine = ExecutionEngine::new_with_keypair(registry.clone(), workspace.keypair.clone())
        .with_storage(store.clone());
    for handler in proof_content::content_handlers() {
        engine.register_handler(handler);
    }
    for handler in proof_commerce::commerce_handlers() {
        engine.register_handler(handler);
    }
    for handler in proof_workflow::workflow_handlers() {
        engine.register_handler(handler);
    }
    for handler in proof_analytics::analytics_handlers() {
        engine.register_handler(handler);
    }
    let api_key = std::env::var("OPENAI_API_KEY")
        .context("OPENAI_API_KEY is required to start or resume an OpenAI agent")?;
    let base_url =
        std::env::var("OPENAI_BASE_URL").unwrap_or_else(|_| DEFAULT_OPENAI_BASE_URL.to_string());
    let model = Arc::new(OpenAiResponsesGateway::new(api_key, base_url)?);
    AgentRuntime::new(
        registry,
        engine,
        workspace.keypair.clone(),
        workspace.root.clone(),
        store.clone(),
        store.clone(),
        store,
        model,
    )
    .map_err(anyhow::Error::from)
}

fn ensure_openai_provider(agent: &AgentDefinition) -> Result<()> {
    if !agent.provider.eq_ignore_ascii_case("openai") {
        bail!(
            "agent provider {} is not supported by this CLI; supported provider: openai",
            agent.provider
        );
    }
    Ok(())
}

fn parse_tool(value: &str, registry: &Registry) -> Result<AgentTool> {
    let (operation, version) = value
        .rsplit_once("::")
        .with_context(|| format!("agent tool must use operation::version format: {value}"))?;
    let tool = AgentTool::new(operation, version)?;
    registry
        .find(&tool.operation, &tool.version)
        .with_context(|| format!("registered operation not found: {}", tool.key()))?;
    Ok(tool)
}

fn parse_id(value: &str, kind: &str) -> Result<Uuid> {
    Uuid::parse_str(value).with_context(|| format!("invalid {kind} ID"))
}

fn print_outcome(outcome: AgentRuntimeOutcome) -> Result<()> {
    let next = match &outcome {
        AgentRuntimeOutcome::WaitingForApproval { run, request, .. } => Some(serde_json::json!({
            "approve": format!(
                "proof approval approve {} --approver <approver-id>",
                request.body.id
            ),
            "deny": format!(
                "proof approval deny {} --approver <approver-id>",
                request.body.id
            ),
            "resume": format!("proof agent resume {}", run.id),
        })),
        AgentRuntimeOutcome::Completed { run, .. } | AgentRuntimeOutcome::Failed { run, .. } => {
            Some(serde_json::json!({
                "watch": format!("proof agent watch {}", run.id),
            }))
        }
    };
    let mut value = serde_json::to_value(outcome)?;
    if let Some(next) = next {
        value["next"] = next;
    }
    print_json(value)
}

fn print_json(value: serde_json::Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Mutex, OnceLock};
    use std::thread;

    use clap::Parser;
    use proof_kernel::{AgentRun, AgentRunMode, AgentRunStatus};

    use super::*;

    fn initialized_cli(directory: &assert_fs::TempDir) -> Cli {
        let cli = Cli::parse_from(["proof", "-w", directory.path().to_str().unwrap(), "init"]);
        crate::commands::content::cmd_init(&cli).unwrap();
        cli
    }

    fn install_catalog_registry(directory: &assert_fs::TempDir) {
        let source = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("registry/commerce");
        let target = directory.path().join(".proof/registry/commerce");
        std::fs::create_dir_all(&target).unwrap();
        for file_name in [
            "catalog-create.json",
            "catalog-create.input.json",
            "catalog-create.output.json",
        ] {
            std::fs::copy(source.join(file_name), target.join(file_name)).unwrap();
        }
    }

    fn create_agent(cli: &Cli) -> AgentDefinition {
        cmd_agent_create(
            cli,
            "catalog-manager",
            "Create the requested catalog.",
            "openai",
            "test-model",
            &["catalog.create::v1".to_string()],
            4,
            6,
            10_000,
            60,
            512,
            None,
        )
        .unwrap();
        open_store(&cli.workspace)
            .unwrap()
            .list_agent_definitions()
            .unwrap()
            .pop()
            .unwrap()
    }

    #[test]
    fn create_list_inspect_and_watch_agent_records() {
        let directory = assert_fs::TempDir::new().unwrap();
        let cli = initialized_cli(&directory);
        install_catalog_registry(&directory);
        let agent = create_agent(&cli);

        cmd_agent_list(&cli).unwrap();
        cmd_agent_inspect(&cli, &agent.id.to_string()).unwrap();
        let store = open_store(&cli.workspace).unwrap();
        let now = Utc::now();
        let mut run = AgentRun::new_for_agent(
            Workspace::open(&cli.workspace).unwrap().actor,
            agent.id,
            AgentRunMode::Session,
            "Inspect me",
            now,
        )
        .unwrap();
        store.save_agent_run(&run).unwrap();
        run.start(now).unwrap();
        store.save_agent_run(&run).unwrap();
        cmd_agent_watch(&cli, &run.id.to_string()).unwrap();
        let error = cmd_agent_evaluate(
            &cli,
            &run.id.to_string(),
            "not-terminal/v1",
            Path::new("unused-policy.json"),
        )
        .unwrap_err();
        assert!(error.to_string().contains("is not terminal"));

        run.succeed(now + chrono::Duration::seconds(1)).unwrap();
        store.save_agent_run(&run).unwrap();
        let error = cmd_agent_evaluate(
            &cli,
            &run.id.to_string(),
            "unsealed/v1",
            Path::new("unused-policy.json"),
        )
        .unwrap_err();
        assert!(error.to_string().contains("has no terminal event"));
    }

    #[test]
    fn cancelled_run_is_immediately_sealed_and_can_be_evaluated() {
        let directory = assert_fs::TempDir::new().unwrap();
        let cli = initialized_cli(&directory);
        install_catalog_registry(&directory);
        let agent = create_agent(&cli);
        let workspace = Workspace::open(&cli.workspace).unwrap();
        let store = open_store(&cli.workspace).unwrap();
        let now = Utc::now();
        let mut run = AgentRun::new_for_agent(
            workspace.actor,
            agent.id,
            AgentRunMode::Session,
            "Cancelled before execution",
            now,
        )
        .unwrap();
        store.save_agent_run(&run).unwrap();
        run.cancel(now + chrono::Duration::seconds(1)).unwrap();
        store.save_agent_run(&run).unwrap();
        let policy = directory.path().join("cancelled-policy.json");
        std::fs::write(
            &policy,
            serde_json::to_vec(&serde_json::json!({
                "expected_calls": [],
                "allow_additional_calls": false
            }))
            .unwrap(),
        )
        .unwrap();

        let error =
            cmd_agent_evaluate(&cli, &run.id.to_string(), "cancelled/v1", &policy).unwrap_err();

        assert!(error.to_string().contains("agent evaluation failed"));
        let evaluations = store.list_agent_run_evaluations(&run.id).unwrap();
        assert_eq!(evaluations.len(), 1);
        assert_eq!(evaluations[0].outcome, AgentEvaluationOutcome::Failed);
        assert_eq!(evaluations[0].metrics["checks"][0]["name"], "run_succeeded");
        assert_eq!(evaluations[0].metrics["checks"][0]["passed"], false);
    }

    #[test]
    fn start_executes_an_openai_tool_loop_and_resume_is_idempotent() {
        let _environment = environment_lock().lock().unwrap();
        let directory = assert_fs::TempDir::new().unwrap();
        let cli = initialized_cli(&directory);
        install_catalog_registry(&directory);
        let agent = create_agent(&cli);
        let (base_url, server) = fake_openai_server(vec![
            serde_json::json!({
                "id": "resp_tool",
                "status": "completed",
                "output": [{
                    "type": "function_call",
                    "call_id": "call_catalog",
                    "name": "proof_commerce_v1_catalog_create",
                    "arguments": "{\"name\":\"Spring\"}"
                }],
                "usage": {"input_tokens": 10, "output_tokens": 4, "total_tokens": 14}
            }),
            serde_json::json!({
                "id": "resp_finish",
                "status": "completed",
                "output": [{
                    "type": "message",
                    "content": [{"type": "output_text", "text": "Catalog created."}]
                }],
                "usage": {"input_tokens": 8, "output_tokens": 3, "total_tokens": 11}
            }),
        ]);
        let old_key = std::env::var_os("OPENAI_API_KEY");
        let old_base = std::env::var_os("OPENAI_BASE_URL");
        std::env::set_var("OPENAI_API_KEY", "test-key");
        std::env::set_var("OPENAI_BASE_URL", &base_url);

        cmd_agent_start(&cli, &agent.id.to_string(), "Create Spring").unwrap();
        server.join().unwrap();
        let store = open_store(&cli.workspace).unwrap();
        let run = store.list_agent_runs().unwrap().pop().unwrap();
        assert_eq!(run.status, AgentRunStatus::Succeeded);
        assert_eq!(store.list_agent_run_steps(&run.id).unwrap().len(), 1);
        assert_eq!(store.list_agent_run_events(&run.id).unwrap().len(), 8);
        assert_eq!(store.list_agent_run_evaluations(&run.id).unwrap().len(), 1);

        let malformed_policy = directory.path().join("catalog-eval-malformed.json");
        std::fs::write(&malformed_policy, "{").unwrap();
        let error = cmd_agent_evaluate(
            &cli,
            &run.id.to_string(),
            "catalog-malformed/v1",
            &malformed_policy,
        )
        .unwrap_err();
        assert!(error.to_string().contains("invalid evaluation policy"));
        assert_eq!(store.list_agent_run_evaluations(&run.id).unwrap().len(), 1);

        let typo_policy = directory.path().join("catalog-eval-typo.json");
        std::fs::write(
            &typo_policy,
            serde_json::to_vec_pretty(&serde_json::json!({
                "expected_calls": [],
                "allow_additional_calls": false,
                "required_final_output_referencess": []
            }))
            .unwrap(),
        )
        .unwrap();
        let error = cmd_agent_evaluate(&cli, &run.id.to_string(), "catalog-typo/v1", &typo_policy)
            .unwrap_err();
        assert!(error.to_string().contains("invalid evaluation policy"));
        assert_eq!(store.list_agent_run_evaluations(&run.id).unwrap().len(), 1);

        let passing_policy = directory.path().join("catalog-eval-pass.json");
        std::fs::write(
            &passing_policy,
            serde_json::to_vec_pretty(&serde_json::json!({
                "expected_calls": [{
                    "operation": "catalog.create",
                    "version": "v1",
                    "arguments": {"name": "Spring"},
                    "requires_approved_execution": false
                }],
                "allow_additional_calls": false
            }))
            .unwrap(),
        )
        .unwrap();
        cmd_agent_evaluate(
            &cli,
            &run.id.to_string(),
            "catalog-create/v1",
            &passing_policy,
        )
        .unwrap();
        let evaluations = store.list_agent_run_evaluations(&run.id).unwrap();
        assert_eq!(evaluations.len(), 2);
        assert_eq!(
            evaluations.last().unwrap().outcome,
            AgentEvaluationOutcome::Passed
        );
        assert!(
            evaluations.last().unwrap().metrics["binding"]["policy_digest"]
                .as_str()
                .is_some()
        );
        assert!(
            evaluations.last().unwrap().metrics["binding"]["trace_digest"]
                .as_str()
                .is_some()
        );

        let failing_policy = directory.path().join("catalog-eval-fail.json");
        std::fs::write(
            &failing_policy,
            serde_json::to_vec_pretty(&serde_json::json!({
                "expected_calls": [{
                    "operation": "catalog.create",
                    "version": "v1",
                    "arguments": {"name": "Winter"},
                    "requires_approved_execution": false
                }],
                "allow_additional_calls": false
            }))
            .unwrap(),
        )
        .unwrap();
        let error = cmd_agent_evaluate(
            &cli,
            &run.id.to_string(),
            "catalog-create-wrong-target/v1",
            &failing_policy,
        )
        .unwrap_err();
        assert!(error.to_string().contains("agent evaluation failed"));
        let evaluations = store.list_agent_run_evaluations(&run.id).unwrap();
        assert_eq!(evaluations.len(), 3);
        assert_eq!(
            evaluations.last().unwrap().outcome,
            AgentEvaluationOutcome::Failed
        );

        cmd_agent_resume(&cli, &run.id.to_string()).unwrap();
        restore_env("OPENAI_API_KEY", old_key);
        restore_env("OPENAI_BASE_URL", old_base);
    }

    fn environment_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn restore_env(key: &str, value: Option<std::ffi::OsString>) {
        match value {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }

    fn fake_openai_server(responses: Vec<serde_json::Value>) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            for response in responses {
                let (mut stream, _) = listener.accept().unwrap();
                read_request(&mut stream);
                let body = serde_json::to_vec(&response).unwrap();
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .unwrap();
                stream.write_all(&body).unwrap();
                stream.flush().unwrap();
            }
        });
        (format!("http://{address}/v1"), handle)
    }

    fn read_request(stream: &mut std::net::TcpStream) {
        let mut request = Vec::new();
        let mut buffer = [0_u8; 4096];
        let mut expected_length = None;
        loop {
            let read = stream.read(&mut buffer).unwrap();
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..read]);
            if expected_length.is_none() {
                if let Some(header_end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..header_end]);
                    let content_length = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(str::trim)
                                .and_then(|value| value.parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    expected_length = Some(header_end + 4 + content_length);
                }
            }
            if expected_length.is_some_and(|length| request.len() >= length) {
                break;
            }
        }
    }
}
