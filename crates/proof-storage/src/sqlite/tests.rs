//! Unit tests for the SQLite storage adapter.

use super::migrations::{rollback_to, schema_version, MIGRATIONS};
use super::store::{ProofFilter, SqliteStore};
use crate::StorageError;
use chrono::{DateTime, TimeZone, Utc};
use proof_kernel::ExecutionContext;
use proof_kernel::{
    canonicalize, digest, generate_keypair_for, ArtifactKind, Delegation, PrincipalId, Proof,
    RegistryEntry,
};
use rusqlite::params;
use serde_json::json;
use std::path::PathBuf;
use uuid::Uuid;

use proof_kernel::Governance;

fn test_proof() -> Proof {
    let keypair = generate_keypair_for(proof_kernel::PrincipalKind::Agent);
    let input_json = canonicalize(&json!({"input": "test"})).unwrap();
    let output_json = canonicalize(&json!({"output": "ok"})).unwrap();
    let input = digest(ArtifactKind::OperationInput, &input_json);
    let output = digest(ArtifactKind::OperationOutput, &output_json);
    Proof::new(
        Uuid::now_v7(),
        keypair.principal_id,
        None,
        "test.operation",
        input,
        output,
        Utc.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap(),
    )
    .sign(&keypair)
    .unwrap()
}

fn signed_proof(
    keypair: &proof_kernel::Keypair,
    operation: &str,
    input_digest: proof_kernel::ContentDigest,
    output_digest: proof_kernel::ContentDigest,
    timestamp: chrono::DateTime<Utc>,
) -> Proof {
    Proof::new(
        Uuid::now_v7(),
        keypair.principal_id,
        None,
        operation,
        input_digest,
        output_digest,
        timestamp,
    )
    .sign(keypair)
    .unwrap()
}

fn json_digest(
    kind: proof_kernel::ArtifactKind,
    value: serde_json::Value,
) -> proof_kernel::ContentDigest {
    let canonical = proof_kernel::canonicalize(&value).unwrap();
    proof_kernel::digest(kind, &canonical)
}

fn expired_proof(
    keypair: &proof_kernel::Keypair,
    operation: &str,
    timestamp: chrono::DateTime<Utc>,
    expires_at: chrono::DateTime<Utc>,
) -> Proof {
    let input = json_digest(
        proof_kernel::ArtifactKind::OperationInput,
        json!({"expired": timestamp.to_rfc3339()}),
    );
    let output = json_digest(
        proof_kernel::ArtifactKind::OperationOutput,
        json!({"expired": expires_at.to_rfc3339()}),
    );
    let mut proof = Proof::new(
        Uuid::now_v7(),
        keypair.principal_id,
        None,
        operation,
        input,
        output,
        timestamp,
    );
    proof.body.expires_at = Some(expires_at);
    proof.sign(keypair).unwrap()
}

fn test_registry_entry(operation: &str) -> RegistryEntry {
    RegistryEntry {
        operation: operation.to_string(),
        domain: "test".to_string(),
        version: "v1".to_string(),
        action: "test:action".to_string(),
        description: "test entry".to_string(),
        input_schema: "input.json".to_string(),
        output_schema: "output.json".to_string(),
        required_authority: "delegation-grant".to_string(),
        governance: Governance::AgentExecutable,
        idempotency: "required".to_string(),
        consequence: "test-consequence".to_string(),
        evidence_contract: "test-contract".to_string(),
        benchmark: None,
        status: proof_kernel::VersionStatus::Active,
        deprecated_since: None,
        replacement_operation: None,
    }
}

#[test]
fn proof_round_trips() {
    let store = SqliteStore::in_memory().unwrap();
    let proof = test_proof();
    store.save_proof(&proof).unwrap();
    let loaded = store.load_proof(&proof.body.id).unwrap();
    assert_eq!(loaded, proof);
}

#[test]
fn registry_entries_round_trip() {
    let store = SqliteStore::in_memory().unwrap();
    let entries = vec![
        test_registry_entry("alpha.operation"),
        test_registry_entry("zulu.operation"),
    ];
    store.save_registry(&entries).unwrap();
    let loaded = store.load_registry().unwrap();
    assert_eq!(loaded, entries);
}

#[test]
fn execution_context_is_persisted() {
    let store = SqliteStore::in_memory().unwrap();
    let keypair = generate_keypair_for(proof_kernel::PrincipalKind::Human);
    let context = ExecutionContext {
        actor: keypair.principal_id,
        delegation_id: None,
        delegation_chain: None,
        workspace_path: PathBuf::from("/tmp/workspace"),
        timestamp: Utc::now(),
    };
    let context_id = store.save_execution_context(&context).unwrap();
    let count: i64 = store
        .connection()
        .query_row("SELECT COUNT(*) FROM execution_contexts", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(count, 1);
    assert!(Uuid::parse_str(&context_id.to_string()).is_ok());
}

#[test]
fn principals_round_trip() {
    let store = SqliteStore::in_memory().unwrap();
    let keypair = generate_keypair_for(proof_kernel::PrincipalKind::Agent);
    let principal = proof_kernel::principal_from_keypair(&keypair);

    store.save_principal(&principal).unwrap();
    let loaded = store.load_principal(&principal.id).unwrap();

    assert_eq!(loaded.id, principal.id);
    assert_eq!(loaded.kind, principal.kind);
    assert_eq!(
        loaded.public_key.as_bytes(),
        principal.public_key.as_bytes()
    );
    assert!(matches!(
        store.load_principal(&proof_kernel::PrincipalId::now()),
        Err(StorageError::NotFound(_))
    ));
}

#[test]
fn lists_proofs_by_operation_and_version() {
    let store = SqliteStore::in_memory().unwrap();
    let keypair = generate_keypair_for(proof_kernel::PrincipalKind::Agent);
    let principal = proof_kernel::principal_from_keypair(&keypair);
    store.save_principal(&principal).unwrap();

    let first = signed_proof(
        &keypair,
        "chain.operation::v1",
        json_digest(
            proof_kernel::ArtifactKind::OperationInput,
            json!({"step": 0}),
        ),
        json_digest(
            proof_kernel::ArtifactKind::OperationOutput,
            json!({"step": 1}),
        ),
        Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 1).unwrap(),
    );
    let second = signed_proof(
        &keypair,
        "chain.operation::v2",
        json_digest(
            proof_kernel::ArtifactKind::OperationInput,
            json!({"step": 2}),
        ),
        json_digest(
            proof_kernel::ArtifactKind::OperationOutput,
            json!({"step": 3}),
        ),
        Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 2).unwrap(),
    );
    store.save_proof(&first).unwrap();
    store.save_proof(&second).unwrap();

    let all = store
        .list_proofs_for_operation("chain.operation", None)
        .unwrap();
    let v1 = store
        .list_proofs_for_operation("chain.operation", Some("v1"))
        .unwrap();
    let v2 = store
        .list_proofs_for_operation("chain.operation", Some("v2"))
        .unwrap();

    assert_eq!(all, vec![first.clone(), second.clone()]);
    assert_eq!(v1, vec![first]);
    assert_eq!(v2, vec![second]);
}

#[test]
fn lists_proofs_by_actor() {
    let store = SqliteStore::in_memory().unwrap();
    let first_keypair = generate_keypair_for(proof_kernel::PrincipalKind::Agent);
    let second_keypair = generate_keypair_for(proof_kernel::PrincipalKind::Agent);
    for keypair in [&first_keypair, &second_keypair] {
        store
            .save_principal(&proof_kernel::principal_from_keypair(keypair))
            .unwrap();
    }

    let first = signed_proof(
        &first_keypair,
        "actor.operation::v1",
        json_digest(proof_kernel::ArtifactKind::OperationInput, json!({"a": 1})),
        json_digest(proof_kernel::ArtifactKind::OperationOutput, json!({"a": 2})),
        Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 1).unwrap(),
    );
    let second = signed_proof(
        &first_keypair,
        "actor.operation::v1",
        json_digest(proof_kernel::ArtifactKind::OperationInput, json!({"a": 2})),
        json_digest(proof_kernel::ArtifactKind::OperationOutput, json!({"a": 3})),
        Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 2).unwrap(),
    );
    let other = signed_proof(
        &second_keypair,
        "actor.operation::v1",
        json_digest(proof_kernel::ArtifactKind::OperationInput, json!({"b": 1})),
        json_digest(proof_kernel::ArtifactKind::OperationOutput, json!({"b": 2})),
        Utc::now(),
    );
    for proof in [&first, &second, &other] {
        store.save_proof(proof).unwrap();
    }

    let actor_proofs = store
        .list_proofs_for_actor(&first_keypair.principal_id)
        .unwrap();

    assert_eq!(actor_proofs, vec![first, second]);
}

#[test]
fn verifies_a_valid_proof_chain() {
    let store = SqliteStore::in_memory().unwrap();
    let keypair = generate_keypair_for(proof_kernel::PrincipalKind::Agent);
    store
        .save_principal(&proof_kernel::principal_from_keypair(&keypair))
        .unwrap();

    let first_input = json_digest(
        proof_kernel::ArtifactKind::OperationInput,
        json!({"state": 0}),
    );
    let first_output = json_digest(
        proof_kernel::ArtifactKind::OperationOutput,
        json!({"state": 1}),
    );
    let second_input = first_output;
    let second_output = json_digest(
        proof_kernel::ArtifactKind::OperationOutput,
        json!({"state": 2}),
    );
    let first = signed_proof(
        &keypair,
        "chain.operation::v1",
        first_input,
        first_output,
        Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 1).unwrap(),
    );
    let second = signed_proof(
        &keypair,
        "chain.operation::v1",
        second_input,
        second_output,
        Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 2).unwrap(),
    );
    store.save_proof(&first).unwrap();
    store.save_proof(&second).unwrap();

    store
        .verify_proof_chain(&[first.body.id, second.body.id])
        .unwrap();
}

#[test]
fn rejects_signature_without_principal() {
    let store = SqliteStore::in_memory().unwrap();
    let proof = test_proof();
    store.save_proof(&proof).unwrap();

    let result = store.verify_proof_chain(&[proof.body.id]);

    assert!(matches!(result, Err(StorageError::Conflict(_))));
}

#[test]
fn rejects_disconnected_proof_chain() {
    let store = SqliteStore::in_memory().unwrap();
    let keypair = generate_keypair_for(proof_kernel::PrincipalKind::Agent);
    store
        .save_principal(&proof_kernel::principal_from_keypair(&keypair))
        .unwrap();

    let first = signed_proof(
        &keypair,
        "chain.operation::v1",
        json_digest(
            proof_kernel::ArtifactKind::OperationInput,
            json!({"state": 0}),
        ),
        json_digest(
            proof_kernel::ArtifactKind::OperationOutput,
            json!({"state": 1}),
        ),
        Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 1).unwrap(),
    );
    let second = signed_proof(
        &keypair,
        "chain.operation::v1",
        json_digest(
            proof_kernel::ArtifactKind::OperationInput,
            json!({"unrelated": 1}),
        ),
        json_digest(
            proof_kernel::ArtifactKind::OperationOutput,
            json!({"unrelated": 2}),
        ),
        Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 2).unwrap(),
    );
    store.save_proof(&first).unwrap();
    store.save_proof(&second).unwrap();

    let result = store.verify_proof_chain(&[first.body.id, second.body.id]);

    assert!(matches!(result, Err(StorageError::Conflict(_))));
}

#[test]
fn deletes_expired_contexts() {
    let store = SqliteStore::in_memory().unwrap();
    let keypair = generate_keypair_for(proof_kernel::PrincipalKind::Human);
    let make_context = |timestamp| ExecutionContext {
        actor: keypair.principal_id,
        delegation_id: None,
        delegation_chain: None,
        workspace_path: PathBuf::from("/tmp/workspace"),
        timestamp,
    };
    let expired_id = store
        .save_execution_context(&make_context(
            Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
        ))
        .unwrap();
    let keep_id = store
        .save_execution_context(&make_context(
            Utc.with_ymd_and_hms(2026, 1, 2, 0, 0, 0).unwrap(),
        ))
        .unwrap();

    let deleted = store
        .delete_expired_contexts(Utc.with_ymd_and_hms(2026, 1, 2, 0, 0, 0).unwrap())
        .unwrap();
    let count = store.context_count().unwrap();

    assert_eq!(deleted, 1);
    assert_eq!(count, 1);
    assert_eq!(store.proof_count().unwrap(), 0);
    let remaining: Vec<String> = store
        .connection()
        .prepare("SELECT id FROM execution_contexts")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(remaining, vec![keep_id.to_string()]);
    assert_ne!(expired_id, keep_id);
}

#[test]
fn load_and_list_exclude_expired_proofs_by_default() {
    let store = SqliteStore::in_memory().unwrap();
    let keypair = generate_keypair_for(proof_kernel::PrincipalKind::Agent);
    let principal = proof_kernel::principal_from_keypair(&keypair);
    store.save_principal(&principal).unwrap();

    let expired = expired_proof(
        &keypair,
        "expiry.operation::v1",
        Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
        Utc.with_ymd_and_hms(2026, 1, 2, 0, 0, 0).unwrap(),
    );
    let active = expired_proof(
        &keypair,
        "expiry.operation::v1",
        Utc.with_ymd_and_hms(2026, 1, 2, 0, 0, 1).unwrap(),
        Utc::now() + chrono::Duration::hours(1),
    );
    store.save_proof(&expired).unwrap();
    store.save_proof(&active).unwrap();

    assert!(matches!(
        store.load_proof(&expired.body.id),
        Err(StorageError::NotFound(_))
    ));
    assert_eq!(
        store
            .load_proof_with_options(&expired.body.id, true)
            .unwrap()
            .body
            .id,
        expired.body.id
    );
    assert_eq!(
        store
            .list_proofs_for_operation("expiry.operation", Some("v1"))
            .unwrap(),
        vec![active.clone()]
    );
    assert_eq!(
        store
            .list_proofs_for_operation_with_options("expiry.operation", Some("v1"), true)
            .unwrap(),
        vec![expired.clone(), active.clone()]
    );
    assert_eq!(
        store.list_proofs_for_actor(&keypair.principal_id).unwrap(),
        vec![active.clone()]
    );
    assert_eq!(
        store
            .list_proofs_for_actor_with_options(&keypair.principal_id, true)
            .unwrap(),
        vec![expired, active]
    );
}

#[test]
fn purges_only_proofs_expired_at_or_before_now() {
    let store = SqliteStore::in_memory().unwrap();
    let keypair = generate_keypair_for(proof_kernel::PrincipalKind::Agent);
    let principal = proof_kernel::principal_from_keypair(&keypair);
    store.save_principal(&principal).unwrap();

    let past = expired_proof(
        &keypair,
        "purge.operation::v1",
        Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
        Utc.with_ymd_and_hms(2026, 1, 2, 0, 0, 0).unwrap(),
    );
    let boundary = expired_proof(
        &keypair,
        "purge.operation::v1",
        Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 1).unwrap(),
        Utc.with_ymd_and_hms(2026, 1, 2, 0, 0, 1).unwrap(),
    );
    let future = expired_proof(
        &keypair,
        "purge.operation::v1",
        Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 2).unwrap(),
        Utc.with_ymd_and_hms(2026, 1, 2, 0, 0, 2).unwrap(),
    );
    store.save_proof(&past).unwrap();
    store.save_proof(&boundary).unwrap();
    store.save_proof(&future).unwrap();

    let deleted = store
        .purge_expired_proofs(Utc.with_ymd_and_hms(2026, 1, 2, 0, 0, 1).unwrap())
        .unwrap();

    assert_eq!(deleted, 2);
    assert_eq!(store.proof_count().unwrap(), 1);
    assert_eq!(
        store
            .load_proof_with_options(&future.body.id, true)
            .unwrap()
            .body
            .id,
        future.body.id
    );
}
