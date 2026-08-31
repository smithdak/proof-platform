//! Round-trip tests for the durable agent run control plane.

use chrono::{Duration, TimeZone, Utc};
use proof_kernel::{
    canonicalize, digest, generate_keypair, AgentCheckpoint, AgentCheckpointAppendResult,
    AgentCheckpointTail, AgentEvaluationOutcome, AgentRun, AgentRunEvaluation, AgentRunEvent,
    AgentRunEventKind, AgentRunMode, AgentRunStatus, AgentRunStep, AgentRunStepStatus,
    AgentRunStore, ApprovalOutcome, ArtifactKind, LiveRunStartClaim, LiveRunStartClaimResult,
    SignedApprovalDecision, SignedApprovalRequest,
};
use serde_json::json;
use std::sync::{Arc, Barrier};

use super::store::SqliteStore;
use crate::StorageError;

fn now() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 8, 29, 15, 0, 0).unwrap()
}

fn live_claim(seed: &str) -> LiveRunStartClaim {
    LiveRunStartClaim::create(
        digest(
            ArtifactKind::Generic,
            &canonicalize(&json!({"readiness": seed})).unwrap(),
        ),
        digest(
            ArtifactKind::Generic,
            &canonicalize(&json!({"setup": seed})).unwrap(),
        ),
    )
    .unwrap()
}

fn live_start_bundle(claim: &LiveRunStartClaim) -> (AgentRun, AgentCheckpoint, AgentRunEvent) {
    let started_at = now();
    let agent_id = uuid::Uuid::now_v7();
    let mut run = AgentRun::new_for_agent(
        proof_kernel::PrincipalId::now(),
        agent_id,
        AgentRunMode::Session,
        "Publish one exact preview",
        started_at,
    )
    .unwrap();
    run.start(started_at).unwrap();
    let process_epoch_id = uuid::Uuid::now_v7();
    let checkpoint = AgentCheckpoint::create(
        run.id,
        0,
        json!({
            "kind": "agent_runtime_v2",
            "runtime": {
                "schema": "proof-agent-runtime-state/v2",
                "agent_id": agent_id,
                "run_id": run.id,
                "started_at": started_at,
                "process_epoch_id": process_epoch_id,
                "policy_binding": {"setup_digest": claim.setup_digest},
                "authority": {"delegation_id": uuid::Uuid::now_v7()},
            }
        }),
        started_at,
    )
    .unwrap();
    let event = AgentRunEvent::create(
        run.id,
        0,
        AgentRunEventKind::Started,
        json!({
            "live": true,
            "schema": "proof-agent-runtime-state/v2",
            "process_epoch_id": process_epoch_id,
            "policy_binding": checkpoint.state["runtime"]["policy_binding"].clone(),
            "authority": checkpoint.state["runtime"]["authority"].clone(),
        }),
        started_at,
    )
    .unwrap();
    (run, checkpoint, event)
}

#[test]
fn live_start_claim_round_trips_reopens_and_returns_original_run() {
    let directory = tempfile::TempDir::new().unwrap();
    let database = directory.path().join("proof.db");
    let claim = live_claim("round-trip");
    let (run, checkpoint, event) = live_start_bundle(&claim);
    {
        let store = SqliteStore::open(&database).unwrap();
        assert_eq!(
            store
                .claim_live_run_start(&claim, &run, &checkpoint, &event)
                .unwrap(),
            LiveRunStartClaimResult::Acquired
        );
        assert_eq!(store.load_agent_run(&run.id).unwrap(), Some(run.clone()));
        assert_eq!(
            store.list_agent_checkpoints(&run.id).unwrap(),
            vec![checkpoint.clone()]
        );
        assert_eq!(
            store.list_agent_run_events(&run.id).unwrap(),
            vec![event.clone()]
        );
    }

    let reopened = SqliteStore::open(&database).unwrap();
    let (proposed_run, proposed_checkpoint, proposed_event) = live_start_bundle(&claim);
    assert_eq!(
        reopened
            .claim_live_run_start(&claim, &proposed_run, &proposed_checkpoint, &proposed_event,)
            .unwrap(),
        LiveRunStartClaimResult::Existing(run.id)
    );
    assert_eq!(reopened.list_agent_runs().unwrap(), vec![run]);
}

#[test]
fn live_start_claim_replay_rejects_tampered_indexed_bundle_evidence() {
    let store = SqliteStore::in_memory().unwrap();
    let claim = live_claim("tampered-indexed-evidence");
    let (run, checkpoint, event) = live_start_bundle(&claim);
    assert_eq!(
        store
            .claim_live_run_start(&claim, &run, &checkpoint, &event)
            .unwrap(),
        LiveRunStartClaimResult::Acquired
    );

    store
        .conn
        .lock()
        .unwrap()
        .execute(
            "UPDATE agent_run_events SET data_digest = ?1 WHERE id = ?2",
            rusqlite::params!["0".repeat(64), event.id.to_string()],
        )
        .unwrap();

    let (proposed_run, proposed_checkpoint, proposed_event) = live_start_bundle(&claim);
    assert!(matches!(
        store.claim_live_run_start(&claim, &proposed_run, &proposed_checkpoint, &proposed_event,),
        Err(StorageError::Conflict(_))
    ));
}

#[test]
fn live_start_claim_conflicts_on_either_unique_digest() {
    let store = SqliteStore::in_memory().unwrap();
    let claim = live_claim("original");
    let (run, checkpoint, event) = live_start_bundle(&claim);
    assert_eq!(
        store
            .claim_live_run_start(&claim, &run, &checkpoint, &event)
            .unwrap(),
        LiveRunStartClaimResult::Acquired
    );

    let mut changed_setup = live_claim("changed-setup");
    changed_setup.readiness_binding_digest = claim.readiness_binding_digest;
    let (other_run, other_checkpoint, other_event) = live_start_bundle(&changed_setup);
    assert_eq!(
        store
            .claim_live_run_start(&changed_setup, &other_run, &other_checkpoint, &other_event,)
            .unwrap(),
        LiveRunStartClaimResult::Conflict
    );

    let mut changed_binding = live_claim("changed-binding");
    changed_binding.setup_digest = claim.setup_digest;
    let (other_run, other_checkpoint, other_event) = live_start_bundle(&changed_binding);
    assert_eq!(
        store
            .claim_live_run_start(
                &changed_binding,
                &other_run,
                &other_checkpoint,
                &other_event,
            )
            .unwrap(),
        LiveRunStartClaimResult::Conflict
    );
    assert_eq!(store.list_agent_runs().unwrap(), vec![run]);
}

#[test]
fn live_start_claim_rolls_back_every_new_row_on_bundle_insert_failure() {
    let store = SqliteStore::in_memory().unwrap();
    let mut unrelated = AgentRun::new(
        proof_kernel::PrincipalId::now(),
        AgentRunMode::Session,
        "Unrelated run",
        now(),
    )
    .unwrap();
    store.save_agent_run(&unrelated).unwrap();
    unrelated.start(now()).unwrap();
    store.save_agent_run(&unrelated).unwrap();
    let collision = AgentCheckpoint::create(
        unrelated.id,
        0,
        json!({"kind": "unrelated", "runtime": {}}),
        now(),
    )
    .unwrap();
    store.save_agent_checkpoint(&collision).unwrap();

    let claim = live_claim("rollback");
    let (proposed_run, mut proposed_checkpoint, proposed_event) = live_start_bundle(&claim);
    proposed_checkpoint.id = collision.id;
    assert!(store
        .claim_live_run_start(&claim, &proposed_run, &proposed_checkpoint, &proposed_event,)
        .is_err());
    assert_eq!(store.load_agent_run(&proposed_run.id).unwrap(), None);
    assert!(store
        .list_agent_run_events(&proposed_run.id)
        .unwrap()
        .is_empty());
    assert!(store
        .list_agent_checkpoints(&proposed_run.id)
        .unwrap()
        .is_empty());
    let claim_count: i64 = store
        .conn
        .lock()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM live_run_start_claims", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(claim_count, 0);
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
fn expected_tail_checkpoint_append_round_trips_through_sqlite_trait() {
    let store = SqliteStore::in_memory().unwrap();
    let run = AgentRun::new(
        proof_kernel::PrincipalId::now(),
        AgentRunMode::Session,
        "Append checkpoints",
        now(),
    )
    .unwrap();
    store.save_agent_run(&run).unwrap();
    let first = AgentCheckpoint::create(run.id, 0, json!({"cursor": 0}), now()).unwrap();
    let second = AgentCheckpoint::create(
        run.id,
        1,
        json!({"cursor": 1}),
        now() + Duration::seconds(1),
    )
    .unwrap();

    assert_eq!(
        AgentRunStore::append_agent_checkpoint(&store, None, &first).unwrap(),
        AgentCheckpointAppendResult::Appended
    );
    assert_eq!(
        AgentRunStore::append_agent_checkpoint(
            &store,
            Some(&AgentCheckpointTail::from(&first)),
            &second,
        )
        .unwrap(),
        AgentCheckpointAppendResult::Appended
    );
    assert_eq!(
        store.list_agent_checkpoints(&run.id).unwrap(),
        vec![first, second]
    );
}

#[test]
fn expected_tail_checkpoint_append_rejects_stale_writer_without_poisoning_history() {
    let store = SqliteStore::in_memory().unwrap();
    let run = AgentRun::new(
        proof_kernel::PrincipalId::now(),
        AgentRunMode::Session,
        "Reject a stale writer",
        now(),
    )
    .unwrap();
    store.save_agent_run(&run).unwrap();
    let first = AgentCheckpoint::create(run.id, 0, json!({"cursor": 0}), now()).unwrap();
    store.append_agent_checkpoint(None, &first).unwrap();
    let expected = AgentCheckpointTail::from(&first);
    let winner = AgentCheckpoint::create(
        run.id,
        1,
        json!({"writer": "winner"}),
        now() + Duration::seconds(1),
    )
    .unwrap();
    let stale = AgentCheckpoint::create(
        run.id,
        1,
        json!({"writer": "stale"}),
        now() + Duration::seconds(1),
    )
    .unwrap();

    assert_eq!(
        store
            .append_agent_checkpoint(Some(&expected), &winner)
            .unwrap(),
        AgentCheckpointAppendResult::Appended
    );
    assert_eq!(
        store
            .append_agent_checkpoint(Some(&expected), &stale)
            .unwrap(),
        AgentCheckpointAppendResult::Stale
    );
    let third = AgentCheckpoint::create(
        run.id,
        2,
        json!({"cursor": 2}),
        now() + Duration::seconds(2),
    )
    .unwrap();
    assert_eq!(
        store
            .append_agent_checkpoint(Some(&AgentCheckpointTail::from(&winner)), &third)
            .unwrap(),
        AgentCheckpointAppendResult::Appended
    );
    assert_eq!(
        store.list_agent_checkpoints(&run.id).unwrap(),
        vec![first, winner, third]
    );
}

#[test]
fn expected_tail_checkpoint_append_exact_retry_requires_current_candidate_and_predecessor() {
    let store = SqliteStore::in_memory().unwrap();
    let run = AgentRun::new(
        proof_kernel::PrincipalId::now(),
        AgentRunMode::Session,
        "Retry checkpoint append",
        now(),
    )
    .unwrap();
    store.save_agent_run(&run).unwrap();
    let first = AgentCheckpoint::create(run.id, 0, json!({"cursor": 0}), now()).unwrap();
    let first_tail = AgentCheckpointTail::from(&first);
    let second = AgentCheckpoint::create(
        run.id,
        1,
        json!({"cursor": 1}),
        now() + Duration::seconds(1),
    )
    .unwrap();
    store.append_agent_checkpoint(None, &first).unwrap();
    store
        .append_agent_checkpoint(Some(&first_tail), &second)
        .unwrap();

    assert_eq!(
        store
            .append_agent_checkpoint(Some(&first_tail), &second)
            .unwrap(),
        AgentCheckpointAppendResult::Appended
    );
    let wrong_predecessor = AgentCheckpointTail {
        checkpoint_id: uuid::Uuid::now_v7(),
        ..first_tail
    };
    assert_eq!(
        store
            .append_agent_checkpoint(Some(&wrong_predecessor), &second)
            .unwrap(),
        AgentCheckpointAppendResult::Stale
    );

    let third = AgentCheckpoint::create(
        run.id,
        2,
        json!({"cursor": 2}),
        now() + Duration::seconds(2),
    )
    .unwrap();
    store
        .append_agent_checkpoint(Some(&AgentCheckpointTail::from(&second)), &third)
        .unwrap();
    assert_eq!(
        store
            .append_agent_checkpoint(Some(&first_tail), &second)
            .unwrap(),
        AgentCheckpointAppendResult::Stale
    );
    assert_eq!(store.list_agent_checkpoints(&run.id).unwrap().len(), 3);
}

#[test]
fn expected_tail_checkpoint_append_serializes_two_sqlite_connections() {
    let directory = tempfile::TempDir::new().unwrap();
    let database = directory.path().join("proof.db");
    let seed = SqliteStore::open(&database).unwrap();
    let run = AgentRun::new(
        proof_kernel::PrincipalId::now(),
        AgentRunMode::Session,
        "Serialize checkpoint writers",
        now(),
    )
    .unwrap();
    seed.save_agent_run(&run).unwrap();
    let first = AgentCheckpoint::create(run.id, 0, json!({"cursor": 0}), now()).unwrap();
    seed.append_agent_checkpoint(None, &first).unwrap();
    drop(seed);

    let expected = AgentCheckpointTail::from(&first);
    let left = AgentCheckpoint::create(
        run.id,
        1,
        json!({"writer": "left"}),
        now() + Duration::seconds(1),
    )
    .unwrap();
    let right = AgentCheckpoint::create(
        run.id,
        1,
        json!({"writer": "right"}),
        now() + Duration::seconds(1),
    )
    .unwrap();
    let barrier = Arc::new(Barrier::new(3));
    let left_store = SqliteStore::open(&database).unwrap();
    let left_barrier = Arc::clone(&barrier);
    let left_handle = std::thread::spawn(move || {
        left_barrier.wait();
        let result = left_store
            .append_agent_checkpoint(Some(&expected), &left)
            .unwrap();
        (left.id, result)
    });
    let right_store = SqliteStore::open(&database).unwrap();
    let right_barrier = Arc::clone(&barrier);
    let right_handle = std::thread::spawn(move || {
        right_barrier.wait();
        let result = right_store
            .append_agent_checkpoint(Some(&expected), &right)
            .unwrap();
        (right.id, result)
    });
    barrier.wait();
    let results = [left_handle.join().unwrap(), right_handle.join().unwrap()];
    assert_eq!(
        results
            .iter()
            .filter(|(_, result)| *result == AgentCheckpointAppendResult::Appended)
            .count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|(_, result)| *result == AgentCheckpointAppendResult::Stale)
            .count(),
        1
    );

    let store = SqliteStore::open(&database).unwrap();
    let history = store.list_agent_checkpoints(&run.id).unwrap();
    assert_eq!(history.len(), 2);
    let winner_id = results
        .iter()
        .find_map(|(id, result)| (*result == AgentCheckpointAppendResult::Appended).then_some(*id))
        .unwrap();
    assert_eq!(history[1].id, winner_id);
}

#[test]
fn expected_tail_checkpoint_append_rejects_malformed_evidence_without_inserting() {
    let store = SqliteStore::in_memory().unwrap();
    let run = AgentRun::new(
        proof_kernel::PrincipalId::now(),
        AgentRunMode::Session,
        "Reject malformed checkpoints",
        now(),
    )
    .unwrap();
    store.save_agent_run(&run).unwrap();
    let first = AgentCheckpoint::create(run.id, 0, json!({"cursor": 0}), now()).unwrap();
    store.append_agent_checkpoint(None, &first).unwrap();
    let valid = AgentCheckpoint::create(
        run.id,
        1,
        json!({"cursor": 1}),
        now() + Duration::seconds(1),
    )
    .unwrap();
    let mut malformed = valid.clone();
    malformed.state = json!({"cursor": "tampered-after-digest"});
    let wrong_sequence =
        AgentCheckpoint::create(run.id, 0, json!({"cursor": "wrong-sequence"}), now()).unwrap();
    let mut reused_id = valid.clone();
    reused_id.id = first.id;

    assert!(matches!(
        store.append_agent_checkpoint(Some(&AgentCheckpointTail::from(&first)), &malformed),
        Err(StorageError::Conflict(message)) if message.contains("state digest")
    ));
    assert!(matches!(
        store.append_agent_checkpoint(Some(&AgentCheckpointTail::from(&first)), &wrong_sequence),
        Err(StorageError::Conflict(message)) if message.contains("does not follow")
    ));
    assert!(matches!(
        store.append_agent_checkpoint(Some(&AgentCheckpointTail::from(&first)), &reused_id),
        Err(StorageError::Conflict(message)) if message.contains("existing immutable checkpoint")
    ));

    let other_run = AgentRun::new(
        proof_kernel::PrincipalId::now(),
        AgentRunMode::Session,
        "Reject another run's tail",
        now(),
    )
    .unwrap();
    store.save_agent_run(&other_run).unwrap();
    let cross_run_candidate = AgentCheckpoint::create(
        other_run.id,
        1,
        json!({"cursor": "cross-run"}),
        now() + Duration::seconds(1),
    )
    .unwrap();
    assert!(matches!(
        store.append_agent_checkpoint(
            Some(&AgentCheckpointTail::from(&first)),
            &cross_run_candidate,
        ),
        Err(StorageError::Conflict(message)) if message.contains("belongs to run")
    ));
    assert_eq!(
        store.list_agent_checkpoints(&run.id).unwrap(),
        vec![first.clone()]
    );
    assert!(store
        .list_agent_checkpoints(&other_run.id)
        .unwrap()
        .is_empty());
    assert_eq!(
        store
            .append_agent_checkpoint(Some(&AgentCheckpointTail::from(&first)), &valid)
            .unwrap(),
        AgentCheckpointAppendResult::Appended
    );

    {
        let connection = store.connection();
        connection
            .execute(
                "UPDATE agent_checkpoints SET state_digest = ?1 WHERE id = ?2",
                rusqlite::params!["0".repeat(64), valid.id.to_string()],
            )
            .unwrap();
    }
    let third = AgentCheckpoint::create(
        run.id,
        2,
        json!({"cursor": 2}),
        now() + Duration::seconds(2),
    )
    .unwrap();
    assert!(matches!(
        store.append_agent_checkpoint(Some(&AgentCheckpointTail::from(&valid)), &third),
        Err(StorageError::Conflict(message)) if message.contains("indexed columns")
    ));
    let row_count: i64 = store
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM agent_checkpoints WHERE run_id = ?1",
            [run.id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(row_count, 2);
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
