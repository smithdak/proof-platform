//! Round-trip tests for agent definitions and runtime events.

use chrono::{Duration, TimeZone, Utc};
use proof_kernel::{
    AgentCheckpoint, AgentDefinition, AgentEvaluationOutcome, AgentLimits, AgentRun,
    AgentRunEvaluation, AgentRunEvent, AgentRunEventKind, AgentRunMode, AgentRunStep, AgentTool,
};
use serde_json::json;
use std::sync::{Arc, Barrier};
use std::thread;

use super::store::SqliteStore;
use crate::StorageError;

fn now() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 8, 29, 18, 0, 0).unwrap()
}

fn definition(name: &str) -> AgentDefinition {
    AgentDefinition::new(
        name,
        "Create and approve an order.",
        "openai",
        "test-model",
        vec![
            AgentTool::new("order.create", "v1").unwrap(),
            AgentTool::new("order.approve", "v1").unwrap(),
        ],
        AgentLimits::default(),
        now(),
    )
    .unwrap()
}

#[test]
fn agent_definitions_round_trip_and_enforce_unique_names() {
    let store = SqliteStore::in_memory().unwrap();
    let agent = definition("order-manager");
    store.save_agent_definition(&agent).unwrap();
    store.save_agent_definition(&agent).unwrap();

    assert_eq!(
        store.load_agent_definition(&agent.id).unwrap(),
        Some(agent.clone())
    );
    assert_eq!(store.list_agent_definitions().unwrap(), vec![agent]);
    assert_eq!(
        store.load_agent_definition(&uuid::Uuid::now_v7()).unwrap(),
        None
    );

    let conflict = definition("order-manager");
    assert!(matches!(
        store.save_agent_definition(&conflict),
        Err(StorageError::Conflict(_))
    ));
}

#[test]
fn bound_runs_and_events_round_trip_in_sequence() {
    let store = SqliteStore::in_memory().unwrap();
    let agent = definition("release-manager");
    store.save_agent_definition(&agent).unwrap();
    let run = AgentRun::new_for_agent(
        proof_kernel::PrincipalId::now(),
        agent.id,
        AgentRunMode::Session,
        "Ship the release",
        now(),
    )
    .unwrap();
    store.save_agent_run(&run).unwrap();
    assert_eq!(
        store.load_agent_run(&run.id).unwrap().unwrap().agent_id,
        Some(agent.id)
    );

    let started = AgentRunEvent::create(
        run.id,
        0,
        AgentRunEventKind::Started,
        json!({"agent_id": agent.id}),
        now(),
    )
    .unwrap();
    let requested = AgentRunEvent::create(
        run.id,
        1,
        AgentRunEventKind::ModelRequested,
        json!({"turn": 1}),
        now() + Duration::seconds(1),
    )
    .unwrap();
    assert!(matches!(
        store.save_agent_run_event(&requested),
        Err(StorageError::Conflict(message)) if message.contains("sequence must be contiguous")
    ));
    store.save_agent_run_event(&started).unwrap();
    store.save_agent_run_event(&requested).unwrap();
    store.save_agent_run_event(&started).unwrap();

    assert_eq!(
        store.list_agent_run_events(&run.id).unwrap(),
        vec![started.clone(), requested]
    );
    let conflict = AgentRunEvent::create(
        run.id,
        0,
        AgentRunEventKind::Failed,
        json!({"error": "conflict"}),
        now(),
    )
    .unwrap();
    assert!(matches!(
        store.save_agent_run_event(&conflict),
        Err(StorageError::Conflict(_))
    ));
}

#[test]
fn terminal_events_require_the_matching_terminal_run_status() {
    let store = SqliteStore::in_memory().unwrap();
    let mut run = AgentRun::new(
        proof_kernel::PrincipalId::now(),
        AgentRunMode::Session,
        "Finish safely",
        now(),
    )
    .unwrap();
    store.save_agent_run(&run).unwrap();
    run.start(now()).unwrap();
    store.save_agent_run(&run).unwrap();
    let started =
        AgentRunEvent::create(run.id, 0, AgentRunEventKind::Started, json!({}), now()).unwrap();
    store.save_agent_run_event(&started).unwrap();
    let completed = AgentRunEvent::create(
        run.id,
        1,
        AgentRunEventKind::Completed,
        json!({"output": "done"}),
        now() + Duration::seconds(1),
    )
    .unwrap();
    assert!(matches!(
        store.save_agent_run_event(&completed),
        Err(StorageError::Conflict(message)) if message.contains("does not match stored run status running")
    ));

    run.fail(now() + Duration::seconds(2)).unwrap();
    store.save_agent_run(&run).unwrap();
    assert!(matches!(
        store.save_agent_run_event(&completed),
        Err(StorageError::Conflict(message)) if message.contains("does not match stored run status failed")
    ));
    let budget_exceeded = AgentRunEvent::create(
        run.id,
        1,
        AgentRunEventKind::BudgetExceeded,
        json!({"error": "token budget exceeded"}),
        now() + Duration::seconds(2),
    )
    .unwrap();
    store.save_agent_run_event(&budget_exceeded).unwrap();
}

#[test]
fn terminal_event_seals_run_trace_across_store_connections() {
    let directory = tempfile::TempDir::new().unwrap();
    let database = directory.path().join("proof.db");
    let writer = SqliteStore::open(&database).unwrap();
    let contender = SqliteStore::open(&database).unwrap();
    let mut run = AgentRun::new(
        proof_kernel::PrincipalId::now(),
        AgentRunMode::Session,
        "Seal the release trace",
        now(),
    )
    .unwrap();
    writer.save_agent_run(&run).unwrap();
    run.start(now()).unwrap();
    writer.save_agent_run(&run).unwrap();
    let step = AgentRunStep::new(
        run.id,
        0,
        "release.publish",
        "v1",
        &json!({"environment": "preview"}),
        now(),
    )
    .unwrap();
    writer.save_agent_run_step(&step).unwrap();
    let checkpoint =
        AgentCheckpoint::create(run.id, 0, json!({"cursor": "resp_1"}), now()).unwrap();
    writer.save_agent_checkpoint(&checkpoint).unwrap();
    let started =
        AgentRunEvent::create(run.id, 0, AgentRunEventKind::Started, json!({}), now()).unwrap();
    writer.save_agent_run_event(&started).unwrap();
    run.succeed(now() + Duration::seconds(1)).unwrap();
    writer.save_agent_run(&run).unwrap();
    let completed = AgentRunEvent::create(
        run.id,
        1,
        AgentRunEventKind::Completed,
        json!({"output": "published"}),
        now() + Duration::seconds(1),
    )
    .unwrap();
    writer.save_agent_run_event(&completed).unwrap();

    contender.save_agent_run(&run).unwrap();
    contender.save_agent_run_step(&step).unwrap();
    contender.save_agent_checkpoint(&checkpoint).unwrap();
    contender.save_agent_run_event(&started).unwrap();
    contender.save_agent_run_event(&completed).unwrap();

    let mut changed_run = run.clone();
    changed_run.goal = "Rewrite sealed intent".to_string();
    changed_run.revision += 1;
    assert_sealed(contender.save_agent_run(&changed_run));
    let mut changed_step = step.clone();
    changed_step.start(now() + Duration::seconds(2)).unwrap();
    assert_sealed(contender.save_agent_run_step(&changed_step));
    let new_step = AgentRunStep::new(
        run.id,
        1,
        "audit.record",
        "v1",
        &json!({}),
        now() + Duration::seconds(2),
    )
    .unwrap();
    assert_sealed(contender.save_agent_run_step(&new_step));
    let new_checkpoint = AgentCheckpoint::create(
        run.id,
        1,
        json!({"cursor": "resp_2"}),
        now() + Duration::seconds(2),
    )
    .unwrap();
    assert_sealed(contender.save_agent_checkpoint(&new_checkpoint));
    let further_event = AgentRunEvent::create(
        run.id,
        2,
        AgentRunEventKind::ModelRequested,
        json!({"turn": 2}),
        now() + Duration::seconds(2),
    )
    .unwrap();
    assert_sealed(contender.save_agent_run_event(&further_event));

    let evaluation = AgentRunEvaluation::create(
        &run,
        "terminal-trace-v1",
        AgentEvaluationOutcome::Passed,
        Some(10_000),
        json!({"sealed": true}),
        None,
        now() + Duration::seconds(3),
    )
    .unwrap();
    contender.save_agent_run_evaluation(&evaluation).unwrap();
    contender.save_agent_run_evaluation(&evaluation).unwrap();
    assert_eq!(
        contender.list_agent_run_events(&run.id).unwrap(),
        vec![started, completed]
    );
    assert_eq!(
        contender.list_agent_run_evaluations(&run.id).unwrap(),
        vec![evaluation]
    );
}

#[test]
fn terminal_event_serializes_against_a_competing_append() {
    let directory = tempfile::TempDir::new().unwrap();
    let database = directory.path().join("proof.db");
    let terminal_writer = SqliteStore::open(&database).unwrap();
    let competing_writer = SqliteStore::open(&database).unwrap();
    let mut run = AgentRun::new(
        proof_kernel::PrincipalId::now(),
        AgentRunMode::Session,
        "Fail once",
        now(),
    )
    .unwrap();
    terminal_writer.save_agent_run(&run).unwrap();
    run.start(now()).unwrap();
    terminal_writer.save_agent_run(&run).unwrap();
    let started =
        AgentRunEvent::create(run.id, 0, AgentRunEventKind::Started, json!({}), now()).unwrap();
    terminal_writer.save_agent_run_event(&started).unwrap();
    run.fail(now() + Duration::seconds(1)).unwrap();
    terminal_writer.save_agent_run(&run).unwrap();
    let failed = AgentRunEvent::create(
        run.id,
        1,
        AgentRunEventKind::Failed,
        json!({"error": "provider failed"}),
        now() + Duration::seconds(1),
    )
    .unwrap();
    let competing = AgentRunEvent::create(
        run.id,
        1,
        AgentRunEventKind::ModelRequested,
        json!({"turn": 2}),
        now() + Duration::seconds(1),
    )
    .unwrap();
    let barrier = Arc::new(Barrier::new(3));
    let terminal_barrier = Arc::clone(&barrier);
    let terminal = thread::spawn(move || {
        terminal_barrier.wait();
        terminal_writer.save_agent_run_event(&failed)
    });
    let competing_barrier = Arc::clone(&barrier);
    let competing = thread::spawn(move || {
        competing_barrier.wait();
        competing_writer.save_agent_run_event(&competing)
    });
    barrier.wait();

    terminal.join().unwrap().unwrap();
    assert!(matches!(
        competing.join().unwrap(),
        Err(StorageError::Conflict(_))
    ));
    let observer = SqliteStore::open(&database).unwrap();
    let events = observer.list_agent_run_events(&run.id).unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0], started);
    assert_eq!(events[1].kind, AgentRunEventKind::Failed);
}

#[test]
fn cancelled_run_is_sealed_without_an_event_kind() {
    let store = SqliteStore::in_memory().unwrap();
    let mut run = AgentRun::new(
        proof_kernel::PrincipalId::now(),
        AgentRunMode::Session,
        "Cancel safely",
        now(),
    )
    .unwrap();
    store.save_agent_run(&run).unwrap();
    run.start(now()).unwrap();
    store.save_agent_run(&run).unwrap();
    let step = AgentRunStep::new(run.id, 0, "audit.record", "v1", &json!({}), now()).unwrap();
    store.save_agent_run_step(&step).unwrap();
    let checkpoint = AgentCheckpoint::create(run.id, 0, json!({"cursor": 0}), now()).unwrap();
    store.save_agent_checkpoint(&checkpoint).unwrap();
    let started =
        AgentRunEvent::create(run.id, 0, AgentRunEventKind::Started, json!({}), now()).unwrap();
    store.save_agent_run_event(&started).unwrap();
    run.cancel(now() + Duration::seconds(1)).unwrap();
    store.save_agent_run(&run).unwrap();

    store.save_agent_run(&run).unwrap();
    store.save_agent_run_step(&step).unwrap();
    store.save_agent_checkpoint(&checkpoint).unwrap();
    store.save_agent_run_event(&started).unwrap();

    let mut changed_run = run.clone();
    changed_run.goal = "Change cancelled intent".to_string();
    changed_run.revision += 1;
    assert_sealed(store.save_agent_run(&changed_run));
    let mut changed_step = step;
    changed_step.start(now() + Duration::seconds(2)).unwrap();
    assert_sealed(store.save_agent_run_step(&changed_step));
    let next_checkpoint = AgentCheckpoint::create(
        run.id,
        1,
        json!({"cursor": 1}),
        now() + Duration::seconds(2),
    )
    .unwrap();
    assert_sealed(store.save_agent_checkpoint(&next_checkpoint));
    let next_event = AgentRunEvent::create(
        run.id,
        1,
        AgentRunEventKind::ModelRequested,
        json!({"turn": 1}),
        now() + Duration::seconds(2),
    )
    .unwrap();
    assert_sealed(store.save_agent_run_event(&next_event));
}

fn assert_sealed(result: Result<(), StorageError>) {
    assert!(matches!(
        result,
        Err(StorageError::Conflict(message)) if message.contains("trace is sealed")
    ));
}
