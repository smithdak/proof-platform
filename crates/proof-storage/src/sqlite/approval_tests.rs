//! Round-trip tests for durable approval workflows.

use chrono::{Duration, TimeZone, Utc};
use proof_kernel::{
    canonicalize, create_proof, digest, generate_keypair, generate_keypair_for,
    principal_from_keypair, AgentRun, AgentRunEvent, AgentRunEventKind, AgentRunMode, AgentRunStep,
    ApprovalExecution, ApprovalOutcome, ApprovalStore, ArtifactKind, PrincipalKind,
    SignedApprovalDecision, SignedApprovalRequest,
};
use serde_json::json;

use super::store::SqliteStore;
use crate::StorageError;

fn approval_fixture() -> (
    proof_kernel::Keypair,
    proof_kernel::Keypair,
    SignedApprovalRequest,
    SignedApprovalDecision,
) {
    let requested_at = Utc.with_ymd_and_hms(2026, 8, 29, 12, 0, 0).unwrap();
    let requester = generate_keypair();
    let approver = generate_keypair_for(PrincipalKind::Human);
    let request = SignedApprovalRequest::create(
        "release.publish",
        "v1",
        &json!({"release_id": "release-1"}),
        requested_at,
        requested_at + Duration::minutes(15),
        &requester,
    )
    .unwrap();
    let decision = SignedApprovalDecision::create(
        &request,
        ApprovalOutcome::Approved,
        Some("ship it".to_string()),
        requested_at + Duration::minutes(1),
        &approver,
    )
    .unwrap();
    (requester, approver, request, decision)
}

fn execution_for(
    requester: &proof_kernel::Keypair,
    request: &SignedApprovalRequest,
    input: serde_json::Value,
) -> ApprovalExecution {
    let output = json!({"published": true});
    let executed_at = request.body.requested_at + Duration::minutes(2);
    let proof = create_proof(
        requester.principal_id,
        None,
        "release.publish::v1",
        &input,
        &output,
        executed_at,
        requester,
    )
    .unwrap();
    ApprovalExecution {
        request_id: request.body.id,
        executed_at,
        output,
        proof,
    }
}

#[test]
fn approval_records_round_trip_and_are_idempotent() {
    let store = SqliteStore::in_memory().unwrap();
    let (requester, approver, request, decision) = approval_fixture();
    let approver_principal = principal_from_keypair(&approver);
    store.save_principal(&approver_principal).unwrap();

    store.save_approval_request(&request).unwrap();
    store.save_approval_request(&request).unwrap();
    assert_eq!(
        store.load_approval_request(&request.body.id).unwrap(),
        Some(request.clone())
    );
    assert_eq!(
        store.list_approval_requests().unwrap(),
        vec![request.clone()]
    );

    store.save_approval_decision(&decision).unwrap();
    store.save_approval_decision(&decision).unwrap();
    assert_eq!(
        store.load_approval_decision(&request.body.id).unwrap(),
        Some(decision)
    );

    let input = json!({"release_id": "release-1"});
    let output = json!({"published": true});
    let proof = create_proof(
        requester.principal_id,
        None,
        "release.publish::v1",
        &input,
        &output,
        request.body.requested_at + Duration::minutes(2),
        &requester,
    )
    .unwrap();
    let execution = ApprovalExecution {
        request_id: request.body.id,
        executed_at: request.body.requested_at + Duration::minutes(2),
        output,
        proof,
    };
    store.save_approval_execution(&execution).unwrap();
    store.save_approval_execution(&execution).unwrap();
    assert_eq!(
        store.load_approval_execution(&request.body.id).unwrap(),
        Some(execution)
    );

    let trusted = ApprovalStore::load_trusted_approver(&store, &approver_principal.id)
        .unwrap()
        .unwrap();
    assert_eq!(trusted.id, approver_principal.id);
    assert_eq!(trusted.kind, PrincipalKind::Human);
    assert_eq!(
        trusted.public_key.as_bytes(),
        approver_principal.public_key.as_bytes()
    );
}

#[test]
fn approval_records_reject_conflicting_replacements() {
    let store = SqliteStore::in_memory().unwrap();
    let (_, approver, request, decision) = approval_fixture();
    store
        .save_principal(&principal_from_keypair(&approver))
        .unwrap();
    store.save_approval_request(&request).unwrap();

    let mut conflicting_request = request.clone();
    conflicting_request.signature[0] ^= 1;
    assert!(matches!(
        store.save_approval_request(&conflicting_request),
        Err(StorageError::Conflict(_))
    ));

    store.save_approval_decision(&decision).unwrap();
    let mut conflicting_decision = decision;
    conflicting_decision.signature[0] ^= 1;
    assert!(matches!(
        store.save_approval_decision(&conflicting_decision),
        Err(StorageError::Conflict(_))
    ));
}

#[test]
fn approval_store_returns_none_for_missing_records_and_approvers() {
    let store = SqliteStore::in_memory().unwrap();
    let request_id = uuid::Uuid::now_v7();

    assert_eq!(store.load_approval_request(&request_id).unwrap(), None);
    assert_eq!(store.load_approval_decision(&request_id).unwrap(), None);
    assert_eq!(store.load_approval_execution(&request_id).unwrap(), None);
    assert_eq!(store.list_approval_requests().unwrap(), Vec::new());
    assert!(
        ApprovalStore::load_trusted_approver(&store, &proof_kernel::PrincipalId::now())
            .unwrap()
            .is_none()
    );
}

#[test]
fn list_approval_requests_uses_request_time_order() {
    let store = SqliteStore::in_memory().unwrap();
    let requester = generate_keypair();
    let later = Utc.with_ymd_and_hms(2026, 8, 29, 13, 0, 0).unwrap();
    let first = SignedApprovalRequest::create(
        "release.publish",
        "v1",
        &json!({"position": 1}),
        later - Duration::hours(1),
        later + Duration::minutes(15),
        &requester,
    )
    .unwrap();
    let second = SignedApprovalRequest::create(
        "release.publish",
        "v1",
        &json!({"position": 2}),
        later,
        later + Duration::minutes(15),
        &requester,
    )
    .unwrap();

    store.save_approval_request(&second).unwrap();
    store.save_approval_request(&first).unwrap();

    assert_eq!(store.list_approval_requests().unwrap(), vec![first, second]);
}

#[test]
fn approval_execution_proof_is_valid_after_round_trip() {
    let store = SqliteStore::in_memory().unwrap();
    let (requester, _, request, _) = approval_fixture();
    store.save_approval_request(&request).unwrap();
    let input = json!({"release_id": "release-1"});
    let output = json!({"published": true});
    let timestamp = request.body.requested_at + Duration::minutes(2);
    let proof = create_proof(
        requester.principal_id,
        None,
        "release.publish::v1",
        &input,
        &output,
        timestamp,
        &requester,
    )
    .unwrap();
    let execution = ApprovalExecution {
        request_id: request.body.id,
        executed_at: timestamp,
        output,
        proof,
    };
    store.save_approval_execution(&execution).unwrap();

    let loaded = store
        .load_approval_execution(&request.body.id)
        .unwrap()
        .unwrap();
    loaded
        .proof
        .verify(&requester.signing_key.verifying_key())
        .unwrap();
    let canonical = canonicalize(&loaded.output).unwrap();
    assert_eq!(
        loaded.proof.body.output_digest,
        digest(ArtifactKind::OperationOutput, &canonical)
    );
}

#[test]
fn principals_are_immutable_and_idempotent() {
    let store = SqliteStore::in_memory().unwrap();
    let keypair = generate_keypair_for(PrincipalKind::Human);
    let principal = principal_from_keypair(&keypair);
    store.save_principal(&principal).unwrap();

    let mut same_identity = principal.clone();
    same_identity.created_at += Duration::days(1);
    store.save_principal(&same_identity).unwrap();

    let mut different_kind = principal.clone();
    different_kind.kind = PrincipalKind::Agent;
    assert!(matches!(
        store.save_principal(&different_kind),
        Err(StorageError::Conflict(message)) if message.contains("different kind or public key")
    ));
    let mut different_key = principal.clone();
    different_key.public_key = generate_keypair_for(PrincipalKind::Human)
        .signing_key
        .verifying_key();
    assert!(matches!(
        store.save_principal(&different_key),
        Err(StorageError::Conflict(message)) if message.contains("different kind or public key")
    ));

    let loaded = store.load_principal(&principal.id).unwrap();
    assert_eq!(loaded.kind, principal.kind);
    assert_eq!(
        loaded.public_key.as_bytes(),
        principal.public_key.as_bytes()
    );
}

#[test]
fn sealed_run_approval_evidence_is_exact_retry_only() {
    let store = SqliteStore::in_memory().unwrap();
    store
        .connection()
        .pragma_update(None, "foreign_keys", "OFF")
        .unwrap();
    let (requester, approver, request, decision) = approval_fixture();
    store
        .save_principal(&principal_from_keypair(&approver))
        .unwrap();
    let execution = execution_for(&requester, &request, json!({"release_id": "release-1"}));
    store.save_approval_request(&request).unwrap();
    store.save_approval_decision(&decision).unwrap();
    store.save_approval_execution(&execution).unwrap();

    let missing_request = SignedApprovalRequest::create(
        "release.publish",
        "v1",
        &json!({"release_id": "missing"}),
        request.body.requested_at,
        request.body.expires_at,
        &requester,
    )
    .unwrap();
    let missing_decision = SignedApprovalDecision::create(
        &missing_request,
        ApprovalOutcome::Approved,
        None,
        request.body.requested_at + Duration::minutes(1),
        &approver,
    )
    .unwrap();
    let missing_execution = execution_for(
        &requester,
        &missing_request,
        json!({"release_id": "missing"}),
    );
    let mut run = AgentRun::new(
        requester.principal_id,
        AgentRunMode::Session,
        "Seal approval evidence",
        request.body.requested_at,
    )
    .unwrap();
    store.save_agent_run(&run).unwrap();
    run.start(request.body.requested_at).unwrap();
    store.save_agent_run(&run).unwrap();
    bind_approval_step(
        &store,
        run.id,
        0,
        request.body.id,
        request.body.requested_at,
    );
    bind_approval_step(
        &store,
        run.id,
        1,
        missing_request.body.id,
        request.body.requested_at,
    );
    let started = AgentRunEvent::create(
        run.id,
        0,
        AgentRunEventKind::Started,
        json!({}),
        request.body.requested_at,
    )
    .unwrap();
    store.save_agent_run_event(&started).unwrap();
    run.succeed(request.body.requested_at + Duration::minutes(3))
        .unwrap();
    store.save_agent_run(&run).unwrap();
    let completed = AgentRunEvent::create(
        run.id,
        1,
        AgentRunEventKind::Completed,
        json!({"output": "published"}),
        request.body.requested_at + Duration::minutes(3),
    )
    .unwrap();
    store.save_agent_run_event(&completed).unwrap();

    store.save_approval_request(&request).unwrap();
    store.save_approval_decision(&decision).unwrap();
    store.save_approval_execution(&execution).unwrap();
    assert_sealed_evidence(store.save_approval_request(&missing_request));
    assert_sealed_evidence(store.save_approval_decision(&missing_decision));
    assert_sealed_evidence(store.save_approval_execution(&missing_execution));
    assert_eq!(
        store
            .load_approval_request(&missing_request.body.id)
            .unwrap(),
        None
    );

    let unrelated_request = SignedApprovalRequest::create(
        "release.publish",
        "v1",
        &json!({"release_id": "unrelated"}),
        request.body.requested_at,
        request.body.expires_at,
        &requester,
    )
    .unwrap();
    let unrelated_decision = SignedApprovalDecision::create(
        &unrelated_request,
        ApprovalOutcome::Approved,
        None,
        request.body.requested_at + Duration::minutes(1),
        &approver,
    )
    .unwrap();
    let unrelated_execution = execution_for(
        &requester,
        &unrelated_request,
        json!({"release_id": "unrelated"}),
    );
    store.save_approval_request(&unrelated_request).unwrap();
    store.save_approval_decision(&unrelated_decision).unwrap();
    store.save_approval_execution(&unrelated_execution).unwrap();
}

#[test]
fn cancelled_run_rejects_missing_bound_approval_evidence() {
    let store = SqliteStore::in_memory().unwrap();
    store
        .connection()
        .pragma_update(None, "foreign_keys", "OFF")
        .unwrap();
    let (requester, approver, request, decision) = approval_fixture();
    store
        .save_principal(&principal_from_keypair(&approver))
        .unwrap();
    let execution = execution_for(&requester, &request, json!({"release_id": "release-1"}));
    let mut run = AgentRun::new(
        requester.principal_id,
        AgentRunMode::Session,
        "Cancel approval",
        request.body.requested_at,
    )
    .unwrap();
    store.save_agent_run(&run).unwrap();
    run.start(request.body.requested_at).unwrap();
    store.save_agent_run(&run).unwrap();
    bind_approval_step(
        &store,
        run.id,
        0,
        request.body.id,
        request.body.requested_at,
    );
    run.cancel(request.body.requested_at + Duration::minutes(1))
        .unwrap();
    store.save_agent_run(&run).unwrap();

    assert_sealed_evidence(store.save_approval_request(&request));
    assert_sealed_evidence(store.save_approval_decision(&decision));
    assert_sealed_evidence(store.save_approval_execution(&execution));
}

fn bind_approval_step(
    store: &SqliteStore,
    run_id: uuid::Uuid,
    ordinal: u32,
    request_id: uuid::Uuid,
    at: chrono::DateTime<Utc>,
) {
    let mut step = AgentRunStep::new(
        run_id,
        ordinal,
        "release.publish",
        "v1",
        &json!({"ordinal": ordinal}),
        at,
    )
    .unwrap();
    store.save_agent_run_step(&step).unwrap();
    step.start(at).unwrap();
    store.save_agent_run_step(&step).unwrap();
    step.wait_for_approval(request_id, at).unwrap();
    store.save_agent_run_step(&step).unwrap();
}

fn assert_sealed_evidence(result: Result<(), StorageError>) {
    assert!(matches!(
        result,
        Err(StorageError::Conflict(message)) if message.contains("trace is sealed")
    ));
}
