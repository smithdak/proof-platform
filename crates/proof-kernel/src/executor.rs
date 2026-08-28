//! Operation execution engine: routes operations from transports through the kernel.

use crate::delegation::{DelegationChain, DelegationError};
use crate::evidence::{Proof, ProofError};
use crate::identity::PrincipalId;
use crate::registry::{Governance, Registry};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use thiserror::Error;
use uuid::Uuid;

/// The context in which an operation executes.
#[derive(Clone, Debug)]
pub struct ExecutionContext {
    /// The Principal executing the operation.
    pub actor: PrincipalId,
    /// The delegation under which this operation is authorized (if any).
    pub delegation_id: Option<Uuid>,
    /// The delegation chain validating the actor's authority (if any).
    pub delegation_chain: Option<DelegationChain>,
    /// Path to the workspace.
    pub workspace_path: PathBuf,
    /// When the execution started.
    pub timestamp: DateTime<Utc>,
}

#[derive(Clone, Debug, Error, PartialEq)]
pub enum ExecutionError {
    #[error("operation not found: {operation} {version}")]
    OperationNotFound { operation: String, version: String },
    #[error("no handler registered for: {0}")]
    NoHandler(String),
    #[error("operation is human-only, agents cannot execute")]
    HumanOnly,
    #[error("delegation chain invalid: {0}")]
    Delegation(#[from] DelegationError),
    #[error("handler execution failed: {0}")]
    HandlerFailed(String),
    #[error("evidence generation failed: {0}")]
    EvidenceFailed(String),
    #[error("storage failed: {0}")]
    StorageFailed(String),
}

/// A storage backend for execution evidence and audit context.
pub trait ExecutionStore: Send + Sync {
    /// Persists a generated proof.
    fn save_proof(&self, proof: &Proof) -> Result<(), String>;

    /// Persists the execution context and returns its storage identifier.
    fn save_execution_context(&self, context: &ExecutionContext) -> Result<String, String>;
}

/// A handler that executes a specific operation.
pub trait OperationHandler: Send + Sync {
    /// The operation name this handler executes.
    fn operation(&self) -> &str;
    /// Executes the operation with the given input and context.
    fn execute(&self, input: &Value, context: &ExecutionContext) -> Result<Value, ExecutionError>;
}

/// The execution engine: routes operations to handlers through the kernel.
pub struct ExecutionEngine {
    registry: Registry,
    handlers: HashMap<String, Arc<dyn OperationHandler>>,
    storage: Option<Arc<dyn ExecutionStore>>,
    keypair: Arc<crate::identity::Keypair>,
}

impl ExecutionEngine {
    /// Creates a new execution engine with the given registry.
    pub fn new(registry: Registry) -> Self {
        Self {
            registry,
            handlers: HashMap::new(),
            storage: None,
            keypair: Arc::new(crate::identity::generate_keypair()),
        }
    }

    /// Creates an execution engine with a deterministic actor for transports.
    pub fn new_with_keypair(registry: Registry, keypair: crate::identity::Keypair) -> Self {
        Self {
            registry,
            handlers: HashMap::new(),
            storage: None,
            keypair: Arc::new(keypair),
        }
    }

    /// Sets the optional storage backend used to persist successful executions.
    pub fn with_storage(mut self, storage: Arc<dyn ExecutionStore>) -> Self {
        self.storage = Some(storage);
        self
    }

    /// Registers a handler for an operation.
    pub fn register_handler(&mut self, handler: Arc<dyn OperationHandler>) {
        self.handlers
            .insert(handler.operation().to_string(), handler);
    }

    /// Returns the number of registered handlers.
    pub fn handler_count(&self) -> usize {
        self.handlers.len()
    }

    /// Executes an operation through the kernel.
    ///
    /// 1. Looks up the operation in the registry.
    /// 2. Checks governance (agent-executable vs human-only).
    /// 3. Finds and executes the registered handler.
    /// 4. Returns the execution result.
    pub fn execute(
        &self,
        operation: &str,
        version: &str,
        input: &Value,
        context: &ExecutionContext,
    ) -> Result<Value, ExecutionError> {
        let entry = self.registry.find(operation, version).ok_or_else(|| {
            ExecutionError::OperationNotFound {
                operation: operation.to_string(),
                version: version.to_string(),
            }
        })?;

        if entry.governance == Governance::HumanOnly {
            return Err(ExecutionError::HumanOnly);
        }

        if let Some(chain) = &context.delegation_chain {
            chain.validate(context.actor, context.timestamp)?;
        }

        let handler = self
            .handlers
            .get(operation)
            .ok_or_else(|| ExecutionError::NoHandler(operation.to_string()))?;

        let result = handler.execute(input, context)?;

        if let Some(storage) = &self.storage {
            storage
                .save_execution_context(context)
                .map_err(ExecutionError::StorageFailed)?;
            let proof = self
                .create_operation_proof(operation, input, &result, context)
                .map_err(|error| ExecutionError::EvidenceFailed(error.to_string()))?;
            storage
                .save_proof(&proof)
                .map_err(ExecutionError::StorageFailed)?;
        }

        Ok(result)
    }

    pub(crate) fn create_operation_proof(
        &self,
        operation: &str,
        input: &Value,
        output: &Value,
        context: &ExecutionContext,
    ) -> Result<Proof, ProofError> {
        create_proof(
            context.actor,
            context.delegation_id,
            operation,
            input,
            output,
            context.timestamp,
            &self.keypair,
        )
    }

    /// Returns all registered operations.
    pub fn operations(&self) -> &[crate::registry::RegistryEntry] {
        self.registry.operations()
    }

    /// Returns whether an operation is agent-executable.
    pub fn is_agent_executable(
        &self,
        operation: &str,
        version: &str,
    ) -> Result<bool, ExecutionError> {
        let entry = self.registry.find(operation, version).ok_or_else(|| {
            ExecutionError::OperationNotFound {
                operation: operation.to_string(),
                version: version.to_string(),
            }
        })?;
        Ok(entry.governance == Governance::AgentExecutable)
    }
}

/// Creates a proof for an executed operation.
pub fn create_proof(
    actor: PrincipalId,
    delegation_id: Option<Uuid>,
    operation: &str,
    input: &Value,
    output: &Value,
    timestamp: DateTime<Utc>,
    keypair: &crate::identity::Keypair,
) -> Result<Proof, ProofError> {
    let input_canonical =
        crate::canonical::canonicalize(input).map_err(|_| ProofError::Canonicalization)?;
    let output_canonical =
        crate::canonical::canonicalize(output).map_err(|_| ProofError::Canonicalization)?;
    let input_digest = crate::canonical::digest(
        crate::canonical::ArtifactKind::OperationInput,
        &input_canonical,
    );
    let output_digest = crate::canonical::digest(
        crate::canonical::ArtifactKind::OperationOutput,
        &output_canonical,
    );

    let proof = Proof::new(
        Uuid::now_v7(),
        actor,
        delegation_id,
        operation,
        input_digest,
        output_digest,
        timestamp,
    );
    proof.sign(keypair)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::delegation::Delegation;
    use crate::identity::generate_keypair;
    use chrono::Duration;
    use serde_json::json;

    struct TestHandler {
        operation: String,
    }

    impl OperationHandler for TestHandler {
        fn operation(&self) -> &str {
            &self.operation
        }
        fn execute(
            &self,
            input: &Value,
            _context: &ExecutionContext,
        ) -> Result<Value, ExecutionError> {
            Ok(json!({"echo": input, "handled_by": self.operation}))
        }
    }

    fn test_registry_entry(
        operation: &str,
        governance: Governance,
    ) -> crate::registry::RegistryEntry {
        crate::registry::RegistryEntry {
            operation: operation.to_string(),
            domain: "test".to_string(),
            version: "v1".to_string(),
            action: format!("test:{}", operation.replace('.', "_")),
            description: format!("Test operation {}", operation),
            input_schema: "test.input.json".to_string(),
            output_schema: "test.output.json".to_string(),
            required_authority: "delegation-grant".to_string(),
            governance,
            idempotency: "required-uuidv7".to_string(),
            consequence: "test-mutation".to_string(),
            evidence_contract: "operation-effect-v1".to_string(),
            benchmark: None,
        }
    }

    fn test_engine(entries: Vec<crate::registry::RegistryEntry>) -> ExecutionEngine {
        let registry = Registry::new(entries).unwrap();
        let mut engine =
            ExecutionEngine::new_with_keypair(registry, crate::identity::generate_keypair());
        engine.register_handler(Arc::new(TestHandler {
            operation: "test.echo".to_string(),
        }));
        engine.register_handler(Arc::new(TestHandler {
            operation: "test.human_only".to_string(),
        }));
        engine
    }

    fn test_context() -> ExecutionContext {
        ExecutionContext {
            actor: PrincipalId::now(),
            delegation_id: None,
            delegation_chain: None,
            workspace_path: PathBuf::from("/tmp/test"),
            timestamp: Utc::now(),
        }
    }

    #[test]
    fn executes_registered_operation() {
        let entries = vec![test_registry_entry(
            "test.echo",
            Governance::AgentExecutable,
        )];
        let engine = test_engine(entries);
        let context = test_context();
        let result = engine
            .execute("test.echo", "v1", &json!({"msg": "hello"}), &context)
            .unwrap();
        assert_eq!(result["echo"]["msg"], "hello");
        assert_eq!(result["handled_by"], "test.echo");
    }

    #[test]
    fn rejects_unknown_operation() {
        let engine = test_engine(vec![]);
        let context = test_context();
        let result = engine.execute("nonexistent", "v1", &json!({}), &context);
        assert!(matches!(
            result,
            Err(ExecutionError::OperationNotFound { .. })
        ));
    }

    #[test]
    fn rejects_human_only_for_agents() {
        let entries = vec![test_registry_entry(
            "test.human_only",
            Governance::HumanOnly,
        )];
        let engine = test_engine(entries);
        let context = test_context();
        let result = engine.execute("test.human_only", "v1", &json!({}), &context);
        assert!(matches!(result, Err(ExecutionError::HumanOnly)));
    }

    #[test]
    fn rejects_invalid_delegation_chain() {
        let entries = vec![test_registry_entry(
            "test.echo",
            Governance::AgentExecutable,
        )];
        let engine = test_engine(entries);
        let actor = PrincipalId::now();
        let other_agent = PrincipalId::now();
        let mut context = test_context();
        context.actor = actor;
        context.delegation_chain = Some(DelegationChain {
            root: PrincipalId::now(),
            grants: vec![Delegation {
                id: Uuid::now_v7(),
                issuer: PrincipalId::now(),
                recipient: other_agent,
                allowed_actions: vec!["*".to_string()],
                resource_scope: vec!["*".to_string()],
                valid_from: context.timestamp - Duration::seconds(1),
                valid_until: context.timestamp + Duration::seconds(1),
                revoked: false,
            }],
        });

        let result = engine.execute("test.echo", "v1", &json!({}), &context);
        assert!(result.is_err());
    }

    #[test]
    fn executes_operation_with_valid_delegation_chain() {
        let entries = vec![test_registry_entry(
            "test.echo",
            Governance::AgentExecutable,
        )];
        let engine = test_engine(entries);
        let root = PrincipalId::now();
        let actor = PrincipalId::now();
        let mut context = test_context();
        context.actor = actor;
        context.delegation_chain = Some(DelegationChain {
            root,
            grants: vec![Delegation {
                id: Uuid::now_v7(),
                issuer: root,
                recipient: actor,
                allowed_actions: vec!["*".to_string()],
                resource_scope: vec!["*".to_string()],
                valid_from: context.timestamp - Duration::seconds(1),
                valid_until: context.timestamp + Duration::seconds(1),
                revoked: false,
            }],
        });

        engine
            .execute("test.echo", "v1", &json!({}), &context)
            .unwrap();
    }

    #[test]
    fn rejects_operation_without_handler() {
        let entries = vec![test_registry_entry(
            "test.no_handler",
            Governance::AgentExecutable,
        )];
        let engine = test_engine(entries);
        let context = test_context();
        let result = engine.execute("test.no_handler", "v1", &json!({}), &context);
        assert!(matches!(result, Err(ExecutionError::NoHandler(_))));
    }

    #[test]
    fn is_agent_executable_returns_true_for_agent_ops() {
        let entries = vec![test_registry_entry(
            "test.echo",
            Governance::AgentExecutable,
        )];
        let engine = test_engine(entries);
        assert!(engine.is_agent_executable("test.echo", "v1").unwrap());
    }

    #[test]
    fn is_agent_executable_returns_false_for_human_ops() {
        let entries = vec![test_registry_entry(
            "test.human_only",
            Governance::HumanOnly,
        )];
        let engine = test_engine(entries);
        assert!(!engine.is_agent_executable("test.human_only", "v1").unwrap());
    }

    #[test]
    fn engine_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ExecutionEngine>();
        assert_send_sync::<ExecutionContext>();
    }

    #[test]
    fn create_proof_signs_correctly() {
        let keypair = generate_keypair();
        let actor = keypair.principal_id;
        let input = json!({"test": true});
        let output = json!({"result": "ok"});
        let proof = create_proof(
            actor,
            None,
            "test.op",
            &input,
            &output,
            Utc::now(),
            &keypair,
        )
        .unwrap();
        assert!(proof.verify(&keypair.signing_key.verifying_key()).is_ok());
    }
}
