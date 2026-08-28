use chrono::{DateTime, TimeZone, Utc};
use proof_kernel::{
    create_proof, generate_keypair_for, AuditFilter, ExecutionContext, ExecutionStore,
    PrincipalKind,
};
use proof_storage::SqliteStore;
use serde_json::json;
use std::path::PathBuf;

const OPERATION: &str = "schema.create";

fn timestamp(seconds: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, seconds).unwrap()
}

fn assert_context(actual: &ExecutionContext, expected: &ExecutionContext) {
    assert_eq!(actual.actor, expected.actor);
    assert_eq!(actual.delegation_id, expected.delegation_id);
    assert_eq!(actual.workspace_path, expected.workspace_path);
    assert_eq!(actual.timestamp, expected.timestamp);
    assert!(actual.delegation_chain.is_none());
}

fn context(
    actor: proof_kernel::PrincipalId,
    at: DateTime<Utc>,
    delegation_id: Option<uuid::Uuid>,
) -> ExecutionContext {
    ExecutionContext {
        actor,
        delegation_id,
        delegation_chain: None,
        workspace_path: PathBuf::from("/tmp/audit-workspace"),
        timestamp: at,
    }
}

#[test]
fn audit_contexts_round_trip_with_operation_filter() {
    let store = SqliteStore::in_memory().unwrap();
    let keypair = generate_keypair_for(PrincipalKind::Agent);
    let proof = create_proof(
        keypair.principal_id,
        None,
        OPERATION,
        &json!({"name": "example"}),
        &json!({"created": true}),
        timestamp(1),
        &keypair,
    )
    .unwrap();
    let saved_context = context(keypair.principal_id, timestamp(1), None);

    store.save_proof(&proof).unwrap();
    store.save_execution_context(&saved_context).unwrap();

    let mut filter = AuditFilter::new();
    filter.operation = Some(OPERATION.to_string());
    let loaded = store.load_audit_contexts(&filter).unwrap();

    assert_eq!(loaded.len(), 1);
    assert_context(&loaded[0], &saved_context);
}

#[test]
fn audit_contexts_filter_by_actor_since_limit_and_offset() {
    let store = SqliteStore::in_memory().unwrap();
    let actor_keypair = generate_keypair_for(PrincipalKind::Agent);
    let other_keypair = generate_keypair_for(PrincipalKind::Human);
    let actor_proof = create_proof(
        actor_keypair.principal_id,
        None,
        OPERATION,
        &json!({"sequence": 1}),
        &json!({}),
        timestamp(1),
        &actor_keypair,
    )
    .unwrap();
    let other_proof = create_proof(
        other_keypair.principal_id,
        None,
        "object.create",
        &json!({"sequence": 2}),
        &json!({}),
        timestamp(4),
        &other_keypair,
    )
    .unwrap();
    let first = context(actor_keypair.principal_id, timestamp(1), None);
    let second = context(actor_keypair.principal_id, timestamp(2), None);
    let third = context(actor_keypair.principal_id, timestamp(3), None);
    let other_actor = context(other_keypair.principal_id, timestamp(4), None);

    store.save_proof(&actor_proof).unwrap();
    store.save_proof(&other_proof).unwrap();
    store.save_execution_context(&first).unwrap();
    store.save_execution_context(&second).unwrap();
    store.save_execution_context(&third).unwrap();
    store.save_execution_context(&other_actor).unwrap();

    let mut filter = AuditFilter::new();
    filter.actor = Some(actor_keypair.principal_id);
    filter.since = Some(timestamp(2));
    filter.limit = 2;
    filter.offset = 0;
    let loaded = store.load_audit_contexts(&filter).unwrap();

    assert_eq!(loaded.len(), 2);
    assert_context(&loaded[0], &third);
    assert_context(&loaded[1], &second);

    filter.offset = 1;
    let loaded = store.load_audit_contexts(&filter).unwrap();

    assert_eq!(loaded.len(), 1);
    assert_context(&loaded[0], &second);
}
