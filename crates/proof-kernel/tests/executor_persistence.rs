use proof_kernel::{
    ExecutionContext, ExecutionEngine, Governance, OperationHandler, RecordingStore, Registry,
    RegistryEntry,
};
use serde_json::{json, Value};
use std::{path::PathBuf, sync::Arc};

struct EchoHandler;

impl OperationHandler for EchoHandler {
    fn operation(&self) -> &str {
        "test.echo"
    }

    fn execute(
        &self,
        input: &Value,
        _context: &ExecutionContext,
    ) -> Result<Value, proof_kernel::ExecutionError> {
        Ok(json!({"echo": input}))
    }
}

fn registry_entry() -> RegistryEntry {
    RegistryEntry {
        operation: "test.echo".to_string(),
        domain: "test".to_string(),
        version: "v1".to_string(),
        action: "test:echo".to_string(),
        description: "Test operation".to_string(),
        input_schema: "test.input.json".to_string(),
        output_schema: "test.output.json".to_string(),
        required_authority: "delegation-grant".to_string(),
        governance: Governance::AgentExecutable,
        idempotency: "required-uuidv7".to_string(),
        consequence: "test-mutation".to_string(),
        evidence_contract: "operation-effect-v1".to_string(),
        benchmark: None,
    }
}

fn context() -> (proof_kernel::Keypair, ExecutionContext) {
    let keypair = proof_kernel::generate_keypair();
    (
        keypair.clone(),
        ExecutionContext {
            actor: keypair.principal_id,
            delegation_id: None,
            delegation_chain: None,
            workspace_path: PathBuf::from("/tmp/test"),
            timestamp: chrono::Utc::now(),
        },
    )
}

fn assert_contexts_match(recorded: &ExecutionContext, expected: &ExecutionContext) {
    assert_eq!(recorded.actor, expected.actor);
    assert_eq!(recorded.delegation_id, expected.delegation_id);
    assert_eq!(recorded.workspace_path, expected.workspace_path);
    assert_eq!(recorded.timestamp, expected.timestamp);
}

#[test]
fn records_proof_and_context_for_storage_enabled_engine() {
    let store = Arc::new(RecordingStore::default());
    let registry = Registry::new(vec![registry_entry()]).unwrap();
    let (actor_keypair, execution_context) = context();
    let mut engine = ExecutionEngine::new_with_keypair(registry, actor_keypair.clone())
        .with_storage(store.clone());
    engine.register_handler(Arc::new(EchoHandler));

    engine
        .execute(
            "test.echo",
            "v1",
            &json!({"message": "persisted"}),
            &execution_context,
        )
        .unwrap();

    let mut proofs = store.proofs.lock().unwrap();
    assert_eq!(proofs.len(), 1);
    let proof = proofs.pop().unwrap();
    assert_eq!(proof.body.operation, "test.echo");
    assert_eq!(proof.body.actor, execution_context.actor);
    assert_eq!(
        proof.verify(&actor_keypair.signing_key.verifying_key()),
        Ok(())
    );

    let contexts = store.contexts.lock().unwrap();
    assert_eq!(contexts.len(), 1);
    assert_contexts_match(&contexts[0], &execution_context);
}

#[test]
fn does_not_record_for_storage_disabled_engine() {
    let store = Arc::new(RecordingStore::default());
    let registry = Registry::new(vec![registry_entry()]).unwrap();
    let mut engine = ExecutionEngine::new(registry);
    engine.register_handler(Arc::new(EchoHandler));

    engine
        .execute(
            "test.echo",
            "v1",
            &json!({"message": "not persisted"}),
            &context().1,
        )
        .unwrap();

    assert!(store.proofs.lock().unwrap().is_empty());
    assert!(store.contexts.lock().unwrap().is_empty());
}
