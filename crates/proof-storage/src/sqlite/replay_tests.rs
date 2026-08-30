use super::store::SqliteStore;
use chrono::{Duration, TimeZone, Utc};
use proof_kernel::{
    canonicalize, create_proof, digest, generate_keypair, ArtifactKind, ExecutionContext,
    ExecutionOutcome, ExecutionReplayClaim, ExecutionReplayClaimResult, ExecutionReplayKey,
    Keypair,
};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Barrier};
use std::thread;
use tempfile::TempDir;
use uuid::Uuid;

struct ReplayFixture {
    keypair: Keypair,
    input: Value,
    output: Value,
    claim: ExecutionReplayClaim,
    context: ExecutionContext,
    outcome: ExecutionOutcome,
}

impl ReplayFixture {
    fn new() -> Self {
        let keypair = generate_keypair();
        let input = json!({
            "idempotency_key": Uuid::now_v7(),
            "title": "First edition",
        });
        let output = json!({"edition": {"id": "edition-1"}});
        let timestamp = Utc.with_ymd_and_hms(2026, 8, 29, 15, 0, 0).unwrap();
        let input_digest = digest(ArtifactKind::OperationInput, &canonicalize(&input).unwrap());
        let claim = ExecutionReplayClaim {
            key: ExecutionReplayKey {
                operation: "edition.create".to_string(),
                version: "v1".to_string(),
                idempotency_key: input["idempotency_key"].as_str().unwrap().parse().unwrap(),
            },
            input_digest,
            claim_token: Uuid::now_v7(),
            claimed_by: keypair.principal_id,
            claimed_at: timestamp - Duration::seconds(1),
        };
        let context = ExecutionContext {
            actor: keypair.principal_id,
            principal_kind: None,
            delegation_id: None,
            delegation_chain: None,
            workspace_path: PathBuf::from("/workspace/proof"),
            timestamp,
        };
        let proof = create_proof(
            keypair.principal_id,
            None,
            "edition.create::v1",
            &input,
            &output,
            timestamp,
            &keypair,
        )
        .unwrap();
        let outcome = ExecutionOutcome {
            output: output.clone(),
            proof,
        };
        Self {
            keypair,
            input,
            output,
            claim,
            context,
            outcome,
        }
    }

    fn retry_claim(&self) -> ExecutionReplayClaim {
        ExecutionReplayClaim {
            claim_token: Uuid::now_v7(),
            ..self.claim.clone()
        }
    }
}

#[test]
fn claim_complete_and_exact_replay_round_trip_across_reopen() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("proof.db");
    let fixture = ReplayFixture::new();
    let store = SqliteStore::open(&path).unwrap();

    assert_eq!(
        store.claim_execution_replay(&fixture.claim).unwrap(),
        ExecutionReplayClaimResult::Acquired
    );
    assert_eq!(
        store
            .claim_execution_replay(&fixture.retry_claim())
            .unwrap(),
        ExecutionReplayClaimResult::InProgress
    );

    let mut conflicting = fixture.retry_claim();
    conflicting.input_digest = digest(
        ArtifactKind::OperationInput,
        &canonicalize(&json!({"changed": true})).unwrap(),
    );
    assert_eq!(
        store.claim_execution_replay(&conflicting).unwrap(),
        ExecutionReplayClaimResult::Conflict
    );

    store
        .complete_execution_replay(&fixture.claim, &fixture.context, &fixture.outcome)
        .unwrap();
    store
        .complete_execution_replay(&fixture.claim, &fixture.context, &fixture.outcome)
        .unwrap();

    let (state, output_json, proof_json, proof_id, context_id): (
        String,
        String,
        String,
        String,
        String,
    ) = store
        .connection()
        .query_row(
            "SELECT state, output_json, proof_json, proof_id, execution_context_id
             FROM execution_replays",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(state, "completed");
    assert_eq!(output_json, canonicalize(&fixture.output).unwrap().as_str());
    assert_eq!(
        proof_json,
        serde_json::to_string(&fixture.outcome.proof).unwrap()
    );
    assert_eq!(proof_id, fixture.outcome.proof.body.id.to_string());
    assert!(Uuid::parse_str(&context_id).is_ok());
    assert_eq!(store.context_count().unwrap(), 1);
    assert_eq!(store.proof_count().unwrap(), 1);
    drop(store);

    let reopened = SqliteStore::open(&path).unwrap();
    let replay = reopened
        .claim_execution_replay(&fixture.retry_claim())
        .unwrap();
    assert_eq!(
        replay,
        ExecutionReplayClaimResult::Completed(fixture.outcome.clone())
    );
    assert_eq!(reopened.context_count().unwrap(), 1);
    assert_eq!(reopened.proof_count().unwrap(), 1);
}

#[test]
fn failed_claim_is_idempotent_and_permanently_fail_closed() {
    let fixture = ReplayFixture::new();
    let store = SqliteStore::in_memory().unwrap();
    let failed_at = fixture.context.timestamp + Duration::seconds(1);
    store.claim_execution_replay(&fixture.claim).unwrap();

    store
        .fail_execution_replay(
            &fixture.claim,
            failed_at,
            "handler outcome is indeterminate",
        )
        .unwrap();
    store
        .fail_execution_replay(
            &fixture.claim,
            failed_at,
            "handler outcome is indeterminate",
        )
        .unwrap();

    assert_eq!(
        store
            .claim_execution_replay(&fixture.retry_claim())
            .unwrap(),
        ExecutionReplayClaimResult::Failed
    );
    assert!(store
        .fail_execution_replay(&fixture.claim, failed_at, "different failure")
        .is_err());
    assert!(store
        .complete_execution_replay(&fixture.claim, &fixture.context, &fixture.outcome)
        .is_err());
    assert_eq!(store.context_count().unwrap(), 0);
    assert_eq!(store.proof_count().unwrap(), 0);
}

#[test]
fn claimed_row_survives_reopen_and_is_never_stolen() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("proof.db");
    let fixture = ReplayFixture::new();
    let store = SqliteStore::open(&path).unwrap();
    assert_eq!(
        store.claim_execution_replay(&fixture.claim).unwrap(),
        ExecutionReplayClaimResult::Acquired
    );
    drop(store);

    let reopened = SqliteStore::open(&path).unwrap();
    assert_eq!(
        reopened
            .claim_execution_replay(&fixture.retry_claim())
            .unwrap(),
        ExecutionReplayClaimResult::InProgress
    );
    let (state, token): (String, String) = reopened
        .connection()
        .query_row(
            "SELECT state, claim_token FROM execution_replays",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(state, "claimed");
    assert_eq!(token, fixture.claim.claim_token.to_string());
}

#[test]
fn completion_validation_and_transaction_failures_leave_no_partial_audit_rows() {
    let fixture = ReplayFixture::new();
    let store = SqliteStore::in_memory().unwrap();
    store.claim_execution_replay(&fixture.claim).unwrap();

    let mut invalid_outcome = fixture.outcome.clone();
    invalid_outcome.output = json!({"edition": {"id": "different"}});
    assert!(store
        .complete_execution_replay(&fixture.claim, &fixture.context, &invalid_outcome)
        .is_err());
    assert_eq!(store.context_count().unwrap(), 0);
    assert_eq!(store.proof_count().unwrap(), 0);

    store
        .connection()
        .execute_batch(
            "CREATE TABLE replay_completion_guard (
                 proof_id TEXT NOT NULL REFERENCES proofs(id)
             );
             CREATE TRIGGER reject_replay_completion
             BEFORE UPDATE OF state ON execution_replays
             WHEN NEW.state = 'completed'
             BEGIN
                 INSERT INTO replay_completion_guard (proof_id) VALUES ('missing-proof');
             END;",
        )
        .unwrap();
    assert!(store
        .complete_execution_replay(&fixture.claim, &fixture.context, &fixture.outcome)
        .is_err());
    assert_eq!(store.context_count().unwrap(), 0);
    assert_eq!(store.proof_count().unwrap(), 0);
    let state: String = store
        .connection()
        .query_row("SELECT state FROM execution_replays", [], |row| row.get(0))
        .unwrap();
    assert_eq!(state, "claimed");

    store
        .connection()
        .execute_batch(
            "DROP TRIGGER reject_replay_completion;
             DROP TABLE replay_completion_guard;",
        )
        .unwrap();
    store
        .complete_execution_replay(&fixture.claim, &fixture.context, &fixture.outcome)
        .unwrap();
    assert_eq!(store.context_count().unwrap(), 1);
    assert_eq!(store.proof_count().unwrap(), 1);
}

#[test]
fn completion_rejects_wrong_owner_token_and_different_exact_result() {
    let fixture = ReplayFixture::new();
    let store = SqliteStore::in_memory().unwrap();
    store.claim_execution_replay(&fixture.claim).unwrap();

    let wrong_token = fixture.retry_claim();
    assert!(store
        .complete_execution_replay(&wrong_token, &fixture.context, &fixture.outcome)
        .is_err());
    assert_eq!(store.context_count().unwrap(), 0);
    assert_eq!(store.proof_count().unwrap(), 0);

    store
        .complete_execution_replay(&fixture.claim, &fixture.context, &fixture.outcome)
        .unwrap();
    let different_output = json!({"edition": {"id": "edition-2"}});
    let different_outcome = ExecutionOutcome {
        proof: create_proof(
            fixture.keypair.principal_id,
            None,
            "edition.create::v1",
            &fixture.input,
            &different_output,
            fixture.context.timestamp,
            &fixture.keypair,
        )
        .unwrap(),
        output: different_output,
    };
    assert!(store
        .complete_execution_replay(&fixture.claim, &fixture.context, &different_outcome)
        .is_err());
    assert_eq!(store.context_count().unwrap(), 1);
    assert_eq!(store.proof_count().unwrap(), 1);
}

#[test]
fn two_connections_racing_one_tuple_acquire_exactly_once() {
    let directory = TempDir::new().unwrap();
    let path = directory.path().join("proof.db");
    SqliteStore::open(&path).unwrap();
    let fixture = ReplayFixture::new();
    let barrier = Arc::new(Barrier::new(2));
    let handles = (0..2)
        .map(|_| {
            let path = path.clone();
            let barrier = barrier.clone();
            let claim = fixture.retry_claim();
            thread::spawn(move || {
                let store = SqliteStore::open(&path).unwrap();
                barrier.wait();
                store.claim_execution_replay(&claim).unwrap()
            })
        })
        .collect::<Vec<_>>();
    let results = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect::<Vec<_>>();

    assert_eq!(
        results
            .iter()
            .filter(|result| **result == ExecutionReplayClaimResult::Acquired)
            .count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|result| **result == ExecutionReplayClaimResult::InProgress)
            .count(),
        1
    );
    let store = SqliteStore::open(&path).unwrap();
    let replay_count: i64 = store
        .connection()
        .query_row("SELECT COUNT(*) FROM execution_replays", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(replay_count, 1);
}

#[test]
fn uuidv7_and_nonempty_failure_are_validated_before_writes() {
    let fixture = ReplayFixture::new();
    let store = SqliteStore::in_memory().unwrap();
    let mut invalid = fixture.claim.clone();
    invalid.key.idempotency_key = Uuid::nil();
    assert!(store.claim_execution_replay(&invalid).is_err());
    let replay_count: i64 = store
        .connection()
        .query_row("SELECT COUNT(*) FROM execution_replays", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(replay_count, 0);

    store.claim_execution_replay(&fixture.claim).unwrap();
    assert!(store
        .fail_execution_replay(&fixture.claim, fixture.context.timestamp, "")
        .is_err());
    let state: String = store
        .connection()
        .query_row("SELECT state FROM execution_replays", [], |row| row.get(0))
        .unwrap();
    assert_eq!(state, "claimed");
}

#[test]
fn in_memory_store_enables_and_enforces_replay_foreign_keys() {
    let fixture = ReplayFixture::new();
    let store = SqliteStore::in_memory().unwrap();
    let foreign_keys: i64 = store
        .connection()
        .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
        .unwrap();
    assert_eq!(foreign_keys, 1);

    let context_id = store.save_execution_context(&fixture.context).unwrap();
    let digest = fixture.claim.input_digest.hex();
    let missing_proof = store.connection().execute(
        "INSERT INTO execution_replays (
             operation, version, idempotency_key, input_digest, state, claim_token,
             claimed_by, claimed_at, completed_at, output_json, proof_id, proof_json,
             execution_context_id
         ) VALUES ('edition.create', 'v1', 'missing-proof', ?1, 'completed',
                   'token-missing-proof', 'actor', '2026-08-29T15:00:00Z',
                   '2026-08-29T15:01:00Z', '{}', 'missing-proof', '{}', ?2)",
        rusqlite::params![digest, context_id.to_string()],
    );
    assert!(matches!(
        missing_proof,
        Err(rusqlite::Error::SqliteFailure(error, _))
            if error.code == rusqlite::ErrorCode::ConstraintViolation
    ));

    store.save_proof(&fixture.outcome.proof).unwrap();
    let missing_context = store.connection().execute(
        "INSERT INTO execution_replays (
             operation, version, idempotency_key, input_digest, state, claim_token,
             claimed_by, claimed_at, completed_at, output_json, proof_id, proof_json,
             execution_context_id
         ) VALUES ('edition.create', 'v1', 'missing-context', ?1, 'completed',
                   'token-missing-context', 'actor', '2026-08-29T15:00:00Z',
                   '2026-08-29T15:01:00Z', '{}', ?2, '{}', 'missing-context')",
        rusqlite::params![digest, fixture.outcome.proof.body.id.to_string()],
    );
    assert!(matches!(
        missing_context,
        Err(rusqlite::Error::SqliteFailure(error, _))
            if error.code == rusqlite::ErrorCode::ConstraintViolation
    ));
}
