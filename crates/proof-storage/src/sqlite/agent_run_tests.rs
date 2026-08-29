//! Round-trip tests for the durable agent run control plane.

use chrono::{Duration, TimeZone, Utc};
use proof_kernel::{
    generate_keypair, AgentCheckpoint, AgentEvaluationOutcome, AgentRun, AgentRunEvaluation,
    AgentRunMode, AgentRunStatus, AgentRunStep, AgentRunStepStatus, ApprovalOutcome,
    SignedApprovalDecision, SignedApprovalRequest,
};
use serde_json::json;

use super::store::SqliteStore;
use crate::StorageError;

fn now() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 8, 29, 15, 0, 0).unwrap()
}

#[test]
fn agent_runs_round_trip_with_optimistic_revisions() {
    let store = SqliteStore::in_memory().unwrap();
    let mut run = AgentRun::new(
        proof_kernel::PrincipalId::now(),
        AgentRunMode::Session,
        "Reconcile the release",
        now(),
    )
    .unwrap();
    store.save_agent_run(&run).unwrap();
    let stale = run.clone();
    run.start(now() + Duration::seconds(1)).unwrap();
    store.save_agent_run(&run).unwrap();
    store.save_agent_run(&run).unwrap();

    assert_eq!(store.load_agent_run(&run.id).unwrap(), Some(run.clone()));
    assert_eq!(store.list_agent_runs().unwrap(), vec![run]);
    assert!(matches!(
        store.save_agent_run(&stale),
        Err(StorageError::Conflict(_))
    ));
    assert_eq!(store.load_agent_run(&uuid::Uuid::now_v7()).unwrap(), None);
}

#[test]
fn agent_run_revision_claims_hold_across_store_connections() {
    let directory = tempfile::TempDir::new().unwrap();
    let database = directory.path().join("proof.db");
    let first = SqliteStore::open(&database).unwrap();
    let second = SqliteStore::open(&database).unwrap();
    let run = AgentRun::new(
        proof_kernel::PrincipalId::now(),
        AgentRunMode::Session,
        "Claim one driver",
        now(),
    )
    .unwrap();
    first.save_agent_run(&run).unwrap();

    let mut started = run.clone();
    started.start(now() + Duration::seconds(1)).unwrap();
    let mut cancelled = run;
    cancelled.cancel(now() + Duration::seconds(2)).unwrap();

    first.save_agent_run(&started).unwrap();
    assert!(matches!(
        second.save_agent_run(&cancelled),
        Err(StorageError::Conflict(message)) if message.contains("stale agent run revision")
    ));
    assert_eq!(second.load_agent_run(&started.id).unwrap(), Some(started));
}

#[test]
fn agent_steps_track_approval_and_retry_lineage() {
    let store = SqliteStore::in_memory().unwrap();
    let requester = generate_keypair();
    let mut run = AgentRun::new(
        requester.principal_id,
        AgentRunMode::OneShot,
        "Approve an order",
        now(),
    )
    .unwrap();
    store.save_agent_run(&run).unwrap();
    run.start(now()).unwrap();
    store.save_agent_run(&run).unwrap();
    let input = json!({"order_id": "018f0000-0000-7000-8000-000000000001"});
    let request = SignedApprovalRequest::create(
        "order.approve",
        "v1",
        &input,
        now(),
        now() + Duration::minutes(15),
        &requester,
    )
    .unwrap();
    store.save_approval_request(&request).unwrap();
    let mut step = AgentRunStep::new(run.id, 0, "order.approve", "v1", &input, now()).unwrap();
    store.save_agent_run_step(&step).unwrap();
    step.start(now()).unwrap();
    store.save_agent_run_step(&step).unwrap();
    step.wait_for_approval(request.body.id, now()).unwrap();
    store.save_agent_run_step(&step).unwrap();

    assert_eq!(
        store
            .find_agent_run_step_by_approval(&request.body.id)
            .unwrap(),
        Some(step.clone())
    );
    assert_eq!(store.list_agent_run_steps(&run.id).unwrap(), vec![step]);
    assert_eq!(
        store.load_agent_run_step(&uuid::Uuid::now_v7()).unwrap(),
        None
    );

    let mut failed = AgentRunStep::new(run.id, 1, "order.lookup", "v1", &json!({}), now()).unwrap();
    store.save_agent_run_step(&failed).unwrap();
    failed.start(now()).unwrap();
    store.save_agent_run_step(&failed).unwrap();
    failed.fail("temporary failure", now()).unwrap();
    store.save_agent_run_step(&failed).unwrap();
    let retry = failed.retry(now() + Duration::seconds(1)).unwrap();
    store.save_agent_run_step(&retry).unwrap();
    assert_eq!(
        store.list_agent_run_steps(&run.id).unwrap(),
        vec![
            store
                .find_agent_run_step_by_approval(&request.body.id)
                .unwrap()
                .unwrap(),
            failed,
            retry,
        ]
    );
}

#[test]
fn agent_steps_reject_duplicate_approval_bindings_and_ambiguous_lookup() {
    let store = SqliteStore::in_memory().unwrap();
    let requester = generate_keypair();
    let run = AgentRun::new(
        requester.principal_id,
        AgentRunMode::OneShot,
        "Publish a preview release",
        now(),
    )
    .unwrap();
    store.save_agent_run(&run).unwrap();
    let input = json!({"environment": "preview"});
    let request = SignedApprovalRequest::create(
        "release.publish",
        "v1",
        &input,
        now(),
        now() + Duration::minutes(15),
        &requester,
    )
    .unwrap();
    store.save_approval_request(&request).unwrap();

    let mut first = AgentRunStep::new(run.id, 0, "release.publish", "v1", &input, now()).unwrap();
    store.save_agent_run_step(&first).unwrap();
    first.start(now()).unwrap();
    store.save_agent_run_step(&first).unwrap();
    first.wait_for_approval(request.body.id, now()).unwrap();
    store.save_agent_run_step(&first).unwrap();

    let mut second = AgentRunStep::new(run.id, 1, "release.publish", "v1", &input, now()).unwrap();
    store.save_agent_run_step(&second).unwrap();
    second.start(now()).unwrap();
    store.save_agent_run_step(&second).unwrap();
    second.wait_for_approval(request.body.id, now()).unwrap();
    assert!(matches!(
        store.save_agent_run_step(&second),
        Err(StorageError::Conflict(message)) if message.contains("already bound")
    ));

    {
        let connection = store.connection();
        connection
            .execute("DROP INDEX idx_agent_run_steps_approval_unique", [])
            .unwrap();
        connection
            .execute(
                "UPDATE agent_run_steps
                 SET approval_request_id = ?1, step_json = ?2
                 WHERE id = ?3",
                rusqlite::params![
                    request.body.id.to_string(),
                    serde_json::to_string(&second).unwrap(),
                    second.id.to_string()
                ],
            )
            .unwrap();
    }
    assert!(matches!(
        store.find_agent_run_step_by_approval(&request.body.id),
        Err(StorageError::Conflict(message)) if message.contains("multiple agent run steps")
    ));
}

#[test]
fn approval_lookup_rejects_json_column_mismatch() {
    let store = SqliteStore::in_memory().unwrap();
    let requester = generate_keypair();
    let run = AgentRun::new(
        requester.principal_id,
        AgentRunMode::OneShot,
        "Publish a preview release",
        now(),
    )
    .unwrap();
    store.save_agent_run(&run).unwrap();
    let input = json!({"environment": "preview"});
    let request = SignedApprovalRequest::create(
        "release.publish",
        "v1",
        &input,
        now(),
        now() + Duration::minutes(15),
        &requester,
    )
    .unwrap();
    store.save_approval_request(&request).unwrap();
    let mut step = AgentRunStep::new(run.id, 0, "release.publish", "v1", &input, now()).unwrap();
    store.save_agent_run_step(&step).unwrap();
    step.start(now()).unwrap();
    store.save_agent_run_step(&step).unwrap();
    step.wait_for_approval(request.body.id, now()).unwrap();
    store.save_agent_run_step(&step).unwrap();

    let mut corrupted = step.clone();
    corrupted.approval_request_id = Some(uuid::Uuid::now_v7());
    store
        .connection()
        .execute(
            "UPDATE agent_run_steps SET step_json = ?1 WHERE id = ?2",
            rusqlite::params![
                serde_json::to_string(&corrupted).unwrap(),
                step.id.to_string()
            ],
        )
        .unwrap();

    assert!(matches!(
        store.find_agent_run_step_by_approval(&request.body.id),
        Err(StorageError::Conflict(message)) if message.contains("does not match indexed request")
    ));
}

#[test]
fn checkpoints_and_evaluations_are_immutable_and_ordered() {
    let store = SqliteStore::in_memory().unwrap();
    let mut run = AgentRun::new(
        proof_kernel::PrincipalId::now(),
        AgentRunMode::Session,
        "Evaluate a run",
        now(),
    )
    .unwrap();
    store.save_agent_run(&run).unwrap();
    run.start(now()).unwrap();
    store.save_agent_run(&run).unwrap();
    run.succeed(now() + Duration::seconds(1)).unwrap();
    store.save_agent_run(&run).unwrap();
    assert_eq!(run.status, AgentRunStatus::Succeeded);

    let first = AgentCheckpoint::create(run.id, 0, json!({"cursor": 1}), now()).unwrap();
    let second = AgentCheckpoint::create(run.id, 1, json!({"cursor": 2}), now()).unwrap();
    store.save_agent_checkpoint(&second).unwrap();
    store.save_agent_checkpoint(&first).unwrap();
    store.save_agent_checkpoint(&first).unwrap();
    assert_eq!(
        store.list_agent_checkpoints(&run.id).unwrap(),
        vec![first.clone(), second]
    );
    let conflict = AgentCheckpoint::create(run.id, 0, json!({"cursor": 9}), now()).unwrap();
    assert!(matches!(
        store.save_agent_checkpoint(&conflict),
        Err(StorageError::Conflict(_))
    ));

    let evaluation = AgentRunEvaluation::create(
        &run,
        "proof-policy-v1",
        AgentEvaluationOutcome::Passed,
        Some(9_800),
        json!({"proof_valid": true}),
        Some("healthy".to_string()),
        now() + Duration::seconds(2),
    )
    .unwrap();
    store.save_agent_run_evaluation(&evaluation).unwrap();
    store.save_agent_run_evaluation(&evaluation).unwrap();
    assert_eq!(
        store.list_agent_run_evaluations(&run.id).unwrap(),
        vec![evaluation]
    );
}

#[test]
fn approval_decisions_can_coexist_with_agent_run_records() {
    let store = SqliteStore::in_memory().unwrap();
    let requester = generate_keypair();
    let human = proof_kernel::generate_keypair_for(proof_kernel::PrincipalKind::Human);
    store
        .save_principal(&proof_kernel::principal_from_keypair(&human))
        .unwrap();
    let request = SignedApprovalRequest::create(
        "order.approve",
        "v1",
        &json!({"order_id": "018f0000-0000-7000-8000-000000000001"}),
        now(),
        now() + Duration::minutes(15),
        &requester,
    )
    .unwrap();
    store.save_approval_request(&request).unwrap();
    let decision = SignedApprovalDecision::create(
        &request,
        ApprovalOutcome::Approved,
        None,
        now() + Duration::seconds(1),
        &human,
    )
    .unwrap();
    store.save_approval_decision(&decision).unwrap();
    assert_eq!(
        store
            .load_approval_decision(&request.body.id)
            .unwrap()
            .unwrap()
            .body
            .outcome,
        ApprovalOutcome::Approved
    );
    assert_eq!(AgentRunStepStatus::WaitingForApproval.is_terminal(), false);
}
