use chrono::{Duration, Utc};
use proof_kernel::{
    create_proof, digest, generate_keypair, generate_keypair_for, principal_from_keypair,
    ApprovalGrant, ApprovalOutcome, ArtifactKind, Delegation, DelegationChain, ExecutionContext,
    ExecutionEngine, ExecutionError, ExecutionOutcome, ExecutionReplayClaim,
    ExecutionReplayClaimResult, ExecutionStore, Governance, IdempotencyError, IdempotencyPolicy,
    OperationHandler, Principal, PrincipalId, PrincipalKind, Proof, RecordingStore, Registry,
    RegistryEntry, SignedApprovalDecision, SignedApprovalRequest, VersionStatus,
};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration as StdDuration;
use uuid::Uuid;

#[derive(Clone, Copy)]
enum HandlerBehavior {
    Succeed,
    SucceedSlowly,
    Fail,
}

struct CountingHandler {
    operation: &'static str,
    calls: Arc<AtomicUsize>,
    policy: IdempotencyPolicy,
    behavior: HandlerBehavior,
}

impl OperationHandler for CountingHandler {
    fn operation(&self) -> &str {
        self.operation
    }

    fn idempotency_policy(&self) -> IdempotencyPolicy {
        self.policy
    }

    fn execute(&self, input: &Value, _context: &ExecutionContext) -> Result<Value, ExecutionError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match self.behavior {
            HandlerBehavior::Succeed => Ok(json!({"echo": input})),
            HandlerBehavior::SucceedSlowly => {
                thread::sleep(StdDuration::from_millis(75));
                Ok(json!({"echo": input}))
            }
            HandlerBehavior::Fail => Err(ExecutionError::HandlerFailed("mutation failed".into())),
        }
    }
}

fn registry_entry(operation: &str, governance: Governance) -> RegistryEntry {
    RegistryEntry {
        operation: operation.to_string(),
        domain: "test".to_string(),
        version: "v1".to_string(),
        action: format!("test:{}", operation.replace('.', "_")),
        description: "Replay test operation".to_string(),
        input_schema: "test.input.json".to_string(),
        output_schema: "test.output.json".to_string(),
        required_authority: "delegation-grant".to_string(),
        governance,
        idempotency: "required-uuidv7".to_string(),
        consequence: "test-mutation".to_string(),
        evidence_contract: "operation-effect-v1".to_string(),
        benchmark: None,
        status: VersionStatus::Active,
        deprecated_since: None,
        replacement_operation: None,
    }
}

fn context_for(keypair: &proof_kernel::Keypair) -> ExecutionContext {
    ExecutionContext {
        actor: keypair.principal_id,
        principal_kind: Some(PrincipalKind::Agent),
        delegation_id: None,
        delegation_chain: None,
        workspace_path: PathBuf::from("/tmp/exact-replay"),
        timestamp: Utc::now(),
    }
}

fn input(key: Uuid, value: impl Into<Value>) -> Value {
    json!({"idempotency_key": key, "value": value.into()})
}

fn engine_with_store(
    operation: &'static str,
    governance: Governance,
    keypair: &proof_kernel::Keypair,
    store: Arc<dyn ExecutionStore>,
    calls: Arc<AtomicUsize>,
    behavior: HandlerBehavior,
) -> ExecutionEngine {
    let registry = Registry::new(vec![registry_entry(operation, governance)]).unwrap();
    let mut engine =
        ExecutionEngine::new_with_keypair(registry, keypair.clone()).with_storage(store);
    engine.register_handler(Arc::new(CountingHandler {
        operation,
        calls,
        policy: IdempotencyPolicy::RequiredUuidV7ExactReplay,
        behavior,
    }));
    engine
}

#[test]
fn opted_out_handler_retains_legacy_execution_without_a_key_or_storage() {
    let keypair = generate_keypair();
    let calls = Arc::new(AtomicUsize::new(0));
    let registry = Registry::new(vec![registry_entry(
        "test.legacy",
        Governance::AgentExecutable,
    )])
    .unwrap();
    let mut engine = ExecutionEngine::new_with_keypair(registry, keypair.clone());
    engine.register_handler(Arc::new(CountingHandler {
        operation: "test.legacy",
        calls: calls.clone(),
        policy: IdempotencyPolicy::None,
        behavior: HandlerBehavior::Succeed,
    }));

    let output = engine
        .execute(
            "test.legacy",
            "v1",
            &json!({"value": 1}),
            &context_for(&keypair),
        )
        .unwrap();

    assert_eq!(output["echo"]["value"], 1);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn invalid_required_keys_fail_before_handler_entry() {
    let keypair = generate_keypair();
    let calls = Arc::new(AtomicUsize::new(0));
    let store = Arc::new(RecordingStore::default());
    let engine = engine_with_store(
        "test.replay",
        Governance::AgentExecutable,
        &keypair,
        store,
        calls.clone(),
        HandlerBehavior::Succeed,
    );
    let context = context_for(&keypair);

    let cases = [
        (json!({"value": 1}), IdempotencyError::MissingKey),
        (
            json!({"idempotency_key": 7, "value": 1}),
            IdempotencyError::InvalidUuidV7,
        ),
        (
            json!({"idempotency_key": "not-a-uuid", "value": 1}),
            IdempotencyError::InvalidUuidV7,
        ),
        (
            json!({"idempotency_key": Uuid::nil(), "value": 1}),
            IdempotencyError::InvalidUuidV7,
        ),
    ];
    for (input, expected) in cases {
        assert_eq!(
            engine
                .execute("test.replay", "v1", &input, &context)
                .unwrap_err(),
            ExecutionError::Idempotency(expected)
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

struct UnsupportedStore;

impl ExecutionStore for UnsupportedStore {
    fn save_proof(&self, _proof: &Proof) -> Result<(), String> {
        Ok(())
    }

    fn save_execution_context(&self, _context: &ExecutionContext) -> Result<String, String> {
        Ok("context".to_string())
    }
}

#[test]
fn absent_or_unsupported_storage_fails_before_handler_entry() {
    let keypair = generate_keypair();
    let calls = Arc::new(AtomicUsize::new(0));
    let registry = Registry::new(vec![registry_entry(
        "test.replay",
        Governance::AgentExecutable,
    )])
    .unwrap();
    let mut no_store = ExecutionEngine::new_with_keypair(registry, keypair.clone());
    no_store.register_handler(Arc::new(CountingHandler {
        operation: "test.replay",
        calls: calls.clone(),
        policy: IdempotencyPolicy::RequiredUuidV7ExactReplay,
        behavior: HandlerBehavior::Succeed,
    }));
    let input = input(Uuid::now_v7(), 1);
    let context = context_for(&keypair);
    assert_eq!(
        no_store
            .execute("test.replay", "v1", &input, &context)
            .unwrap_err(),
        ExecutionError::Idempotency(IdempotencyError::StorageRequired)
    );

    let unsupported = engine_with_store(
        "test.replay",
        Governance::AgentExecutable,
        &keypair,
        Arc::new(UnsupportedStore),
        calls.clone(),
        HandlerBehavior::Succeed,
    );
    assert_eq!(
        unsupported
            .execute("test.replay", "v1", &input, &context)
            .unwrap_err(),
        ExecutionError::Idempotency(IdempotencyError::StorageRequired)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn value_and_evidenced_entry_points_return_the_exact_completed_replay() {
    let keypair = generate_keypair();
    let calls = Arc::new(AtomicUsize::new(0));
    let store = Arc::new(RecordingStore::default());
    let engine = engine_with_store(
        "test.replay",
        Governance::AgentExecutable,
        &keypair,
        store.clone(),
        calls.clone(),
        HandlerBehavior::Succeed,
    );
    let key = Uuid::now_v7();
    let first_input: Value = serde_json::from_str(&format!(
        r#"{{"value":{{"b":2,"a":1}},"idempotency_key":"{key}"}}"#
    ))
    .unwrap();
    let retry_input: Value = serde_json::from_str(&format!(
        r#"{{"idempotency_key":"{key}","value":{{"a":1,"b":2}}}}"#
    ))
    .unwrap();
    let context = context_for(&keypair);

    let first_output = engine
        .execute("test.replay", "v1", &first_input, &context)
        .unwrap();
    let original_proof = store.proofs.lock().unwrap()[0].clone();
    let replay = engine
        .execute_evidenced("test.replay", "v1", &retry_input, &context)
        .unwrap();

    assert_eq!(replay.output, first_output);
    assert_eq!(replay.proof, original_proof);
    assert_eq!(replay.proof.body.id, original_proof.body.id);
    assert_eq!(replay.proof.signature, original_proof.signature);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(store.proofs.lock().unwrap().len(), 1);
    assert_eq!(store.contexts.lock().unwrap().len(), 1);
}

#[test]
fn changed_canonical_input_conflicts_before_handler_entry() {
    let keypair = generate_keypair();
    let calls = Arc::new(AtomicUsize::new(0));
    let engine = engine_with_store(
        "test.replay",
        Governance::AgentExecutable,
        &keypair,
        Arc::new(RecordingStore::default()),
        calls.clone(),
        HandlerBehavior::Succeed,
    );
    let key = Uuid::now_v7();
    let context = context_for(&keypair);
    engine
        .execute("test.replay", "v1", &input(key, 1), &context)
        .unwrap();

    assert_eq!(
        engine
            .execute("test.replay", "v1", &input(key, 2), &context)
            .unwrap_err(),
        ExecutionError::Idempotency(IdempotencyError::Conflict)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

fn approval_for(
    input: &Value,
    context: &ExecutionContext,
    requester: &proof_kernel::Keypair,
) -> (ApprovalGrant, Principal) {
    let approver = generate_keypair_for(PrincipalKind::Human);
    let principal = principal_from_keypair(&approver);
    let request = SignedApprovalRequest::create(
        "test.human_replay",
        "v1",
        input,
        context.timestamp - Duration::seconds(1),
        context.timestamp + Duration::minutes(5),
        requester,
    )
    .unwrap();
    let decision = SignedApprovalDecision::create(
        &request,
        ApprovalOutcome::Approved,
        None,
        context.timestamp,
        &approver,
    )
    .unwrap();
    (
        ApprovalGrant {
            request,
            decision,
            approver: principal.clone(),
        },
        principal,
    )
}

#[test]
fn both_approval_entry_points_share_replay_and_authorize_before_disclosure() {
    let requester = generate_keypair();
    let calls = Arc::new(AtomicUsize::new(0));
    let store = Arc::new(RecordingStore::default());
    let engine = engine_with_store(
        "test.human_replay",
        Governance::HumanOnly,
        &requester,
        store.clone(),
        calls.clone(),
        HandlerBehavior::Succeed,
    );
    let input = input(Uuid::now_v7(), "publish");
    let context = context_for(&requester);
    let (grant, trusted_approver) = approval_for(&input, &context, &requester);

    let first = engine
        .execute_with_approval(
            "test.human_replay",
            "v1",
            &input,
            &context,
            &grant,
            &trusted_approver,
        )
        .unwrap();
    let replay = engine
        .execute_with_approval_evidenced(
            "test.human_replay",
            "v1",
            &input,
            &context,
            &grant,
            &trusted_approver,
        )
        .unwrap();
    assert_eq!(replay.output, first);
    assert_eq!(replay.proof, store.proofs.lock().unwrap()[0]);
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    assert_eq!(
        engine
            .execute("test.human_replay", "v1", &input, &context)
            .unwrap_err(),
        ExecutionError::HumanOnly
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn delegation_is_revalidated_before_completed_replay_disclosure() {
    let keypair = generate_keypair();
    let calls = Arc::new(AtomicUsize::new(0));
    let engine = engine_with_store(
        "test.replay",
        Governance::AgentExecutable,
        &keypair,
        Arc::new(RecordingStore::default()),
        calls.clone(),
        HandlerBehavior::Succeed,
    );
    let input = input(Uuid::now_v7(), 1);
    let context = context_for(&keypair);
    engine
        .execute("test.replay", "v1", &input, &context)
        .unwrap();

    let mut unauthorized = context.clone();
    let root = PrincipalId::now();
    unauthorized.delegation_chain = Some(DelegationChain {
        root,
        grants: vec![Delegation {
            id: Uuid::now_v7(),
            issuer: root,
            recipient: PrincipalId::now(),
            allowed_actions: vec!["*".to_string()],
            resource_scope: vec!["*".to_string()],
            scope: proof_kernel::delegation::DelegationScope::default(),
            valid_from: context.timestamp - Duration::seconds(1),
            valid_until: context.timestamp + Duration::minutes(1),
            revoked: false,
        }],
    });
    assert!(matches!(
        engine
            .execute("test.replay", "v1", &input, &unauthorized)
            .unwrap_err(),
        ExecutionError::Delegation(_)
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn concurrent_callers_enter_mutating_handler_once_then_replay() {
    let keypair = generate_keypair();
    let calls = Arc::new(AtomicUsize::new(0));
    let store = Arc::new(RecordingStore::default());
    let engine = Arc::new(engine_with_store(
        "test.replay",
        Governance::AgentExecutable,
        &keypair,
        store,
        calls.clone(),
        HandlerBehavior::SucceedSlowly,
    ));
    let input = input(Uuid::now_v7(), 1);
    let context = context_for(&keypair);
    let barrier = Arc::new(Barrier::new(9));
    let mut threads = Vec::new();
    for _ in 0..8 {
        let engine = engine.clone();
        let input = input.clone();
        let context = context.clone();
        let barrier = barrier.clone();
        threads.push(thread::spawn(move || {
            barrier.wait();
            engine.execute_evidenced("test.replay", "v1", &input, &context)
        }));
    }
    barrier.wait();
    let results: Vec<_> = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect();

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(results.iter().any(Result::is_ok));
    assert!(results.iter().all(|result| {
        result.is_ok() || *result == Err(ExecutionError::Idempotency(IdempotencyError::InProgress))
    }));
    let replay = engine
        .execute_evidenced("test.replay", "v1", &input, &context)
        .unwrap();
    let original = results.into_iter().find_map(Result::ok).unwrap();
    assert_eq!(replay, original);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn handler_and_evidence_failures_become_indeterminate_without_reentry() {
    let keypair = generate_keypair();
    for (behavior, mismatched_actor, expected) in [
        (
            HandlerBehavior::Fail,
            false,
            ExecutionError::HandlerFailed("mutation failed".into()),
        ),
        (
            HandlerBehavior::Succeed,
            true,
            ExecutionError::EvidenceFailed("proof actor does not match signing key".into()),
        ),
    ] {
        let calls = Arc::new(AtomicUsize::new(0));
        let store = Arc::new(RecordingStore::default());
        let engine = engine_with_store(
            "test.replay",
            Governance::AgentExecutable,
            &keypair,
            store.clone(),
            calls.clone(),
            behavior,
        );
        let input = input(Uuid::now_v7(), 1);
        let mut context = context_for(&keypair);
        if mismatched_actor {
            context.actor = generate_keypair().principal_id;
        }
        assert_eq!(
            engine
                .execute("test.replay", "v1", &input, &context)
                .unwrap_err(),
            expected
        );
        assert_eq!(
            engine
                .execute("test.replay", "v1", &input, &context)
                .unwrap_err(),
            ExecutionError::Idempotency(IdempotencyError::Indeterminate)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(store.proofs.lock().unwrap().is_empty());
        assert!(store.contexts.lock().unwrap().is_empty());
    }
}

#[test]
fn benchmark_failure_marks_claim_indeterminate_before_handler_entry() {
    let keypair = generate_keypair();
    let calls = Arc::new(AtomicUsize::new(0));
    let store = Arc::new(RecordingStore::default());
    let mut entry = registry_entry("test.replay", Governance::AgentExecutable);
    entry.benchmark = Some("B1".to_string());
    let registry = Registry::new(vec![entry]).unwrap();
    let mut engine =
        ExecutionEngine::new_with_keypair(registry, keypair.clone()).with_storage(store.clone());
    engine.register_handler(Arc::new(CountingHandler {
        operation: "test.replay",
        calls: calls.clone(),
        policy: IdempotencyPolicy::RequiredUuidV7ExactReplay,
        behavior: HandlerBehavior::Succeed,
    }));
    let context = context_for(&keypair);
    let mut expired = create_proof(
        context.actor,
        None,
        "test.replay::v1",
        &json!({}),
        &json!({}),
        context.timestamp - Duration::hours(2),
        &keypair,
    )
    .unwrap();
    expired.body.expires_at = Some(context.timestamp - Duration::hours(1));
    store.proofs.lock().unwrap().push(expired);
    let input = input(Uuid::now_v7(), 1);

    assert!(matches!(
        engine
            .execute("test.replay", "v1", &input, &context)
            .unwrap_err(),
        ExecutionError::BenchmarkExpired { .. }
    ));
    assert_eq!(
        engine
            .execute("test.replay", "v1", &input, &context)
            .unwrap_err(),
        ExecutionError::Idempotency(IdempotencyError::Indeterminate)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

struct CompletionFailStore {
    claimed: AtomicUsize,
}

impl ExecutionStore for CompletionFailStore {
    fn save_proof(&self, _proof: &Proof) -> Result<(), String> {
        Ok(())
    }

    fn save_execution_context(&self, _context: &ExecutionContext) -> Result<String, String> {
        Ok("context".to_string())
    }

    fn claim_execution_replay(
        &self,
        _claim: &ExecutionReplayClaim,
    ) -> Result<ExecutionReplayClaimResult, String> {
        if self.claimed.fetch_add(1, Ordering::SeqCst) == 0 {
            Ok(ExecutionReplayClaimResult::Acquired)
        } else {
            Ok(ExecutionReplayClaimResult::InProgress)
        }
    }

    fn complete_execution_replay(
        &self,
        _claim: &ExecutionReplayClaim,
        _context: &ExecutionContext,
        _outcome: &ExecutionOutcome,
    ) -> Result<(), String> {
        Err("completion transaction failed".to_string())
    }

    fn fail_execution_replay(
        &self,
        _claim: &ExecutionReplayClaim,
        _failed_at: chrono::DateTime<Utc>,
        _failure: &str,
    ) -> Result<(), String> {
        Ok(())
    }
}

#[test]
fn completion_failure_leaves_claim_blocked_and_does_not_reenter_handler() {
    let keypair = generate_keypair();
    let calls = Arc::new(AtomicUsize::new(0));
    let engine = engine_with_store(
        "test.replay",
        Governance::AgentExecutable,
        &keypair,
        Arc::new(CompletionFailStore {
            claimed: AtomicUsize::new(0),
        }),
        calls.clone(),
        HandlerBehavior::Succeed,
    );
    let input = input(Uuid::now_v7(), 1);
    let context = context_for(&keypair);

    assert_eq!(
        engine
            .execute("test.replay", "v1", &input, &context)
            .unwrap_err(),
        ExecutionError::StorageFailed("completion transaction failed".to_string())
    );
    assert_eq!(
        engine
            .execute("test.replay", "v1", &input, &context)
            .unwrap_err(),
        ExecutionError::Idempotency(IdempotencyError::InProgress)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[derive(Clone, Copy)]
enum Corruption {
    Operation,
    InputDigest,
    OutputDigest,
}

struct CorruptCompletedStore {
    keypair: proof_kernel::Keypair,
    corruption: Corruption,
}

impl ExecutionStore for CorruptCompletedStore {
    fn save_proof(&self, _proof: &Proof) -> Result<(), String> {
        Ok(())
    }

    fn save_execution_context(&self, _context: &ExecutionContext) -> Result<String, String> {
        Ok("context".to_string())
    }

    fn claim_execution_replay(
        &self,
        claim: &ExecutionReplayClaim,
    ) -> Result<ExecutionReplayClaimResult, String> {
        let output = json!({"ok": true});
        let canonical_output = proof_kernel::canonicalize(&output).unwrap();
        let mut proof = Proof::new(
            Uuid::now_v7(),
            claim.claimed_by,
            None,
            format!("{}::{}", claim.key.operation, claim.key.version),
            claim.input_digest,
            digest(ArtifactKind::OperationOutput, &canonical_output),
            claim.claimed_at,
        );
        match self.corruption {
            Corruption::Operation => proof.body.operation = "test.other::v1".to_string(),
            Corruption::InputDigest => {
                proof.body.input_digest = proof_kernel::ContentDigest::from_bytes([1; 32])
            }
            Corruption::OutputDigest => {
                proof.body.output_digest = proof_kernel::ContentDigest::from_bytes([2; 32])
            }
        }
        let proof = proof.sign(&self.keypair).unwrap();
        Ok(ExecutionReplayClaimResult::Completed(ExecutionOutcome {
            output,
            proof,
        }))
    }
}

#[test]
fn corrupted_completed_replays_fail_closed_before_handler_entry() {
    for corruption in [
        Corruption::Operation,
        Corruption::InputDigest,
        Corruption::OutputDigest,
    ] {
        let keypair = generate_keypair();
        let calls = Arc::new(AtomicUsize::new(0));
        let engine = engine_with_store(
            "test.replay",
            Governance::AgentExecutable,
            &keypair,
            Arc::new(CorruptCompletedStore {
                keypair: keypair.clone(),
                corruption,
            }),
            calls.clone(),
            HandlerBehavior::Succeed,
        );
        let error = engine
            .execute(
                "test.replay",
                "v1",
                &input(Uuid::now_v7(), 1),
                &context_for(&keypair),
            )
            .unwrap_err();
        assert!(matches!(error, ExecutionError::StorageFailed(_)));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
}
