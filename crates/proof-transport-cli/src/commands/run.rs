use anyhow::{bail, Context, Result};
use chrono::Utc;
use proof_kernel::{
    AgentCheckpoint, AgentEvaluationOutcome, AgentRun, AgentRunEvaluation, AgentRunMode,
};
use serde_json::Value;
use uuid::Uuid;

use crate::{open_store, Cli, Workspace};

/// Starts a durable multi-step agent session.
pub fn cmd_run_start(cli: &Cli, goal: &str) -> Result<()> {
    let workspace = Workspace::open(&cli.workspace)?;
    let store = open_store(&workspace.root)?;
    let now = Utc::now();
    let mut run = AgentRun::new(workspace.actor, AgentRunMode::Session, goal, now)?;
    store.save_agent_run(&run)?;
    run.start(now)?;
    store.save_agent_run(&run)?;
    print_json(serde_json::json!({
        "status": "running",
        "run": run,
        "mcpMeta": {"com.proofplatform/run": {"runId": run.id}},
    }))
}

/// Lists durable agent runs in creation order.
pub fn cmd_run_list(cli: &Cli) -> Result<()> {
    let workspace = Workspace::open(&cli.workspace)?;
    let runs = open_store(&workspace.root)?.list_agent_runs()?;
    print_json(serde_json::json!({"count": runs.len(), "runs": runs}))
}

/// Shows a run with all attempts, checkpoints, and evaluations.
pub fn cmd_run_inspect(cli: &Cli, run_id: &str) -> Result<()> {
    let workspace = Workspace::open(&cli.workspace)?;
    let store = open_store(&workspace.root)?;
    let run_id = parse_id(run_id, "run")?;
    let run = load_run(&store, run_id)?;
    ensure_actor(&run, workspace.actor)?;
    print_json(serde_json::json!({
        "run": run,
        "steps": store.list_agent_run_steps(&run_id)?,
        "checkpoints": store.list_agent_checkpoints(&run_id)?,
        "evaluations": store.list_agent_run_evaluations(&run_id)?,
    }))
}

/// Appends a canonical, digest-addressed checkpoint to a run.
pub fn cmd_run_checkpoint(cli: &Cli, run_id: &str, state: &str) -> Result<()> {
    let workspace = Workspace::open(&cli.workspace)?;
    let store = open_store(&workspace.root)?;
    let run_id = parse_id(run_id, "run")?;
    let run = load_run(&store, run_id)?;
    ensure_actor(&run, workspace.actor)?;
    let state: Value = serde_json::from_str(state).context("invalid checkpoint state JSON")?;
    let checkpoints = store.list_agent_checkpoints(&run_id)?;
    let sequence = match checkpoints.last() {
        Some(checkpoint) => checkpoint
            .sequence
            .checked_add(1)
            .context("agent checkpoint sequence overflow")?,
        None => 0,
    };
    let checkpoint = AgentCheckpoint::create(run_id, sequence, state, Utc::now())?;
    store.save_agent_checkpoint(&checkpoint)?;
    print_json(serde_json::json!({
        "status": "checkpointed",
        "checkpoint": checkpoint,
    }))
}

/// Creates a new pending attempt for a failed or cancelled step.
pub fn cmd_run_retry(cli: &Cli, run_id: &str, step_id: &str) -> Result<()> {
    let workspace = Workspace::open(&cli.workspace)?;
    let store = open_store(&workspace.root)?;
    let run_id = parse_id(run_id, "run")?;
    let step_id = parse_id(step_id, "step")?;
    let mut run = load_run(&store, run_id)?;
    ensure_actor(&run, workspace.actor)?;
    let step = store
        .load_agent_run_step(&step_id)?
        .with_context(|| format!("agent run step not found: {step_id}"))?;
    if step.run_id != run.id {
        bail!("agent run step {step_id} does not belong to run {run_id}");
    }
    let retry = step.retry(Utc::now())?;
    store.save_agent_run_step(&retry)?;
    run.resume(Utc::now())?;
    store.save_agent_run(&run)?;
    print_json(serde_json::json!({
        "status": "retry_pending",
        "run": run,
        "step": retry,
        "mcpMeta": {
            "com.proofplatform/run": {
                "runId": run.id,
                "stepId": retry.id,
            }
        },
    }))
}

/// Marks a running session complete.
pub fn cmd_run_complete(cli: &Cli, run_id: &str) -> Result<()> {
    transition_run(cli, run_id, |run| run.succeed(Utc::now()), "succeeded")
}

/// Cancels a non-completed run.
pub fn cmd_run_cancel(cli: &Cli, run_id: &str) -> Result<()> {
    transition_run(cli, run_id, |run| run.cancel(Utc::now()), "cancelled")
}

/// Appends an immutable evaluation to a terminal run.
#[allow(clippy::too_many_arguments)]
pub fn cmd_run_evaluate(
    cli: &Cli,
    run_id: &str,
    evaluator: &str,
    outcome: &str,
    score_bps: Option<u16>,
    metrics: &str,
    summary: Option<&str>,
) -> Result<()> {
    let workspace = Workspace::open(&cli.workspace)?;
    let store = open_store(&workspace.root)?;
    let run_id = parse_id(run_id, "run")?;
    let run = load_run(&store, run_id)?;
    ensure_actor(&run, workspace.actor)?;
    let outcome = match outcome {
        "passed" => AgentEvaluationOutcome::Passed,
        "failed" => AgentEvaluationOutcome::Failed,
        _ => bail!("evaluation outcome must be `passed` or `failed`"),
    };
    let metrics = serde_json::from_str(metrics).context("invalid evaluation metrics JSON")?;
    let evaluation = AgentRunEvaluation::create(
        &run,
        evaluator,
        outcome,
        score_bps,
        metrics,
        summary.map(ToString::to_string),
        Utc::now(),
    )?;
    store.save_agent_run_evaluation(&evaluation)?;
    print_json(serde_json::json!({
        "status": "evaluated",
        "evaluation": evaluation,
    }))
}

fn transition_run(
    cli: &Cli,
    run_id: &str,
    transition: impl FnOnce(&mut AgentRun) -> Result<(), proof_kernel::AgentRunError>,
    status: &str,
) -> Result<()> {
    let workspace = Workspace::open(&cli.workspace)?;
    let store = open_store(&workspace.root)?;
    let run_id = parse_id(run_id, "run")?;
    let mut run = load_run(&store, run_id)?;
    ensure_actor(&run, workspace.actor)?;
    transition(&mut run)?;
    store.save_agent_run(&run)?;
    print_json(serde_json::json!({"status": status, "run": run}))
}

fn parse_id(value: &str, kind: &str) -> Result<Uuid> {
    Uuid::parse_str(value).with_context(|| format!("invalid agent {kind} ID"))
}

fn load_run(store: &proof_storage::SqliteStore, run_id: Uuid) -> Result<AgentRun> {
    store
        .load_agent_run(&run_id)?
        .with_context(|| format!("agent run not found: {run_id}"))
}

fn ensure_actor(run: &AgentRun, actor: proof_kernel::PrincipalId) -> Result<()> {
    if run.actor != actor {
        bail!("agent run belongs to a different workspace identity");
    }
    Ok(())
}

fn print_json(value: Value) -> Result<()> {
    println!("{}", serde_json::to_string(&value)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use proof_kernel::{AgentRunStatus, AgentRunStep, AgentRunStepStatus};

    fn initialized_cli(directory: &assert_fs::TempDir) -> Cli {
        let cli = Cli::parse_from(["proof", "-w", directory.path().to_str().unwrap(), "init"]);
        crate::commands::content::cmd_init(&cli).unwrap();
        cli
    }

    #[test]
    fn run_commands_cover_session_checkpoint_completion_and_evaluation() {
        let directory = assert_fs::TempDir::new().unwrap();
        let cli = initialized_cli(&directory);
        cmd_run_start(&cli, "Ship the release").unwrap();
        let store = open_store(&cli.workspace).unwrap();
        let run = store.list_agent_runs().unwrap().pop().unwrap();

        cmd_run_list(&cli).unwrap();
        cmd_run_inspect(&cli, &run.id.to_string()).unwrap();
        cmd_run_checkpoint(&cli, &run.id.to_string(), r#"{"cursor":1}"#).unwrap();
        cmd_run_complete(&cli, &run.id.to_string()).unwrap();
        cmd_run_evaluate(
            &cli,
            &run.id.to_string(),
            "policy-v1",
            "passed",
            Some(9_500),
            r#"{"proof_valid":true}"#,
            Some("healthy"),
        )
        .unwrap();

        let store = open_store(&cli.workspace).unwrap();
        assert_eq!(
            store.load_agent_run(&run.id).unwrap().unwrap().status,
            AgentRunStatus::Succeeded
        );
        assert_eq!(store.list_agent_checkpoints(&run.id).unwrap().len(), 1);
        assert_eq!(store.list_agent_run_evaluations(&run.id).unwrap().len(), 1);
    }

    #[test]
    fn retry_and_cancel_commands_preserve_attempt_lineage() {
        let directory = assert_fs::TempDir::new().unwrap();
        let cli = initialized_cli(&directory);
        let workspace = Workspace::open(&cli.workspace).unwrap();
        let store = open_store(&workspace.root).unwrap();
        let mut run = AgentRun::new(
            workspace.actor,
            AgentRunMode::Session,
            "Retry a call",
            Utc::now(),
        )
        .unwrap();
        store.save_agent_run(&run).unwrap();
        run.start(Utc::now()).unwrap();
        store.save_agent_run(&run).unwrap();
        let mut step = AgentRunStep::new(
            run.id,
            0,
            "test.echo",
            "v1",
            &serde_json::json!({}),
            Utc::now(),
        )
        .unwrap();
        store.save_agent_run_step(&step).unwrap();
        step.start(Utc::now()).unwrap();
        store.save_agent_run_step(&step).unwrap();
        step.fail("temporary", Utc::now()).unwrap();
        store.save_agent_run_step(&step).unwrap();
        run.fail(Utc::now()).unwrap();
        store.save_agent_run(&run).unwrap();

        cmd_run_retry(&cli, &run.id.to_string(), &step.id.to_string()).unwrap();
        let retried = store.list_agent_run_steps(&run.id).unwrap();
        assert_eq!(retried.len(), 2);
        assert_eq!(retried[1].retry_of, Some(step.id));
        assert_eq!(retried[1].status, AgentRunStepStatus::Pending);
        assert_eq!(
            store.load_agent_run(&run.id).unwrap().unwrap().status,
            AgentRunStatus::Running
        );
        cmd_run_cancel(&cli, &run.id.to_string()).unwrap();
        assert_eq!(
            store.load_agent_run(&run.id).unwrap().unwrap().status,
            AgentRunStatus::Cancelled
        );
    }
}
