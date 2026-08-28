//! The ExecutionEngine implementation.

use super::context::{AuditFilter, ExecutionContext};
use super::error::ExecutionError;
use super::store::{ExecutionStore, OperationHandler};
use crate::delegation::DelegationError;
use crate::evidence::{Proof, ProofError};
use crate::identity::PrincipalId;
use crate::registry::{Governance, Registry, RegistryEntry, VersionStatus};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

pub struct ExecutionEngine {
    registry: Registry,
    handlers: HashMap<String, Arc<dyn OperationHandler>>,
    storage: Option<Arc<dyn ExecutionStore>>,
    keypair: Arc<crate::identity::Keypair>,
}

impl ExecutionEngine {
    /// Creates a new execution engine with the given registry.
    pub fn new(registry: Registry) -> Self {
        Self::new_with_keypair(registry, crate::identity::generate_keypair())
    }

    /// Creates an execution engine with a deterministic actor for transports.
    pub fn new_with_keypair(registry: Registry, keypair: crate::identity::Keypair) -> Self {
        let keypair = Arc::new(keypair);
        Self {
            registry,
            handlers: HashMap::new(),
            storage: None,
            keypair,
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
        #[cfg(feature = "tracing")]
        let mut operation_span =
            proof_observability::OperationSpan::new(operation, version, context.actor.to_string());
        #[cfg(feature = "tracing")]
        let result = self.execute_inner(operation, version, input, context, &mut operation_span);
        #[cfg(not(feature = "tracing"))]
        let result = self.execute_inner(operation, version, input, context);
        #[cfg(feature = "tracing")]
        match &result {
            Ok(_) => operation_span.record_success(),
            Err(_) => operation_span.record_failure(),
        }
        result
    }

    fn execute_inner(
        &self,
        operation: &str,
        version: &str,
        input: &Value,
        context: &ExecutionContext,
        #[cfg(feature = "tracing")] operation_span: &mut proof_observability::OperationSpan,
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

        if entry.status == VersionStatus::Deprecated {
            #[cfg(feature = "tracing")]
            tracing::warn!(
                operation,
                version,
                deprecated_since = ?entry.deprecated_since,
                replacement_operation = entry.replacement_operation,
                "executing deprecated operation"
            );
        } else if entry.status == VersionStatus::Sunset {
            return Err(ExecutionError::Sunset);
        }

        if context.delegation_id.is_some() {
            self.enforce_delegation(operation, entry.domain.as_str(), context)?;
        } else if let Some(chain) = &context.delegation_chain {
            chain.validate(context.actor, context.timestamp)?;
        }

        if let Some(benchmark_id) = &entry.benchmark {
            if let Some(storage) = &self.storage {
                let latest_proof = storage
                    .latest_proof_for_operation(operation, version)
                    .map_err(ExecutionError::StorageFailed)?;
                if latest_proof
                    .as_ref()
                    .is_some_and(|proof| proof.is_expired(context.timestamp))
                {
                    return Err(ExecutionError::BenchmarkExpired {
                        benchmark: benchmark_id.clone(),
                        proof_id: latest_proof
                            .expect("expired proof checked above")
                            .body
                            .id
                            .to_string(),
                    });
                }
            }
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
            #[cfg(feature = "tracing")]
            operation_span.set_proof_id(proof.body.id.to_string());
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

    fn enforce_delegation(
        &self,
        operation: &str,
        domain: &str,
        context: &ExecutionContext,
    ) -> Result<(), ExecutionError> {
        let delegation_id = context
            .delegation_id
            .expect("caller checks delegation presence");

        let chain = context
            .delegation_chain
            .as_ref()
            .ok_or(ExecutionError::Delegation(DelegationError::EmptyChain))?;
        chain.validate(context.actor, context.timestamp)?;

        let delegation = if let Some(storage) = &self.storage {
            storage
                .load_delegation(&delegation_id)
                .map_err(ExecutionError::StorageFailed)?
                .ok_or(ExecutionError::Delegation(DelegationError::EmptyChain))?
        } else {
            chain
                .grants
                .iter()
                .find(|grant| grant.id == delegation_id)
                .cloned()
                .ok_or(ExecutionError::Delegation(DelegationError::EmptyChain))?
        };

        if !delegation.scope.scope_allows_operation(operation, domain) {
            return Err(ExecutionError::ScopeViolation);
        }

        Ok(())
    }
}

impl ExecutionEngine {
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

mod tests {
    use super::super::store::RecordingStore;
    use super::*;
    use crate::delegation::{Delegation, DelegationChain, DelegationScope};
    use crate::identity::generate_keypair;
    use chrono::Duration;
    use serde_json::json;

    #[test]
    fn audit_filter_uses_default_limit() {
        let filter = AuditFilter::new();
        assert_eq!(filter.limit, 20);
        assert_eq!(filter.offset, 0);
        assert_eq!(filter.operation, None);
        assert_eq!(filter.actor, None);
        assert_eq!(filter.since, None);
    }

    #[test]
    fn audit_filter_clamps_limit() {
        let mut filter = AuditFilter::new();
        filter.limit = 0;
        filter.clamp_limit();
        assert_eq!(filter.limit, 1);

        filter.limit = 101;
        filter.clamp_limit();
        assert_eq!(filter.limit, 100);

        filter.limit = 42;
        filter.clamp_limit();
        assert_eq!(filter.limit, 42);
    }

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
            status: crate::registry::VersionStatus::Active,
            deprecated_since: None,
            replacement_operation: None,
        }
    }

    #[test]
    fn executes_deprecated_operation() {
        let mut entry = test_registry_entry("test.echo", Governance::AgentExecutable);
        entry.status = crate::registry::VersionStatus::Deprecated;
        entry.deprecated_since = Some(Utc::now().date_naive());
        entry.replacement_operation = Some("test.echo:v2".to_string());
        let engine = test_engine(vec![entry]);
        let result = engine
            .execute("test.echo", "v1", &json!({}), &test_context())
            .unwrap();
        assert_eq!(result["handled_by"], "test.echo");
    }

    #[test]
    fn rejects_sunset_operation() {
        let mut entry = test_registry_entry("test.echo", Governance::AgentExecutable);
        entry.status = crate::registry::VersionStatus::Sunset;
        let engine = test_engine(vec![entry]);
        let error = engine
            .execute("test.echo", "v1", &json!({}), &test_context())
            .unwrap_err();
        assert_eq!(error, ExecutionError::Sunset);
    }

    #[test]
    fn rejects_execution_when_latest_benchmark_proof_is_expired() {
        let store = Arc::new(RecordingStore::default());
        let engine_keypair = crate::identity::generate_keypair();
        let mut entry = test_registry_entry("test.echo", Governance::AgentExecutable);
        entry.benchmark = Some("B1".to_string());
        let registry = Registry::new(vec![entry]).unwrap();
        let mut engine = ExecutionEngine::new_with_keypair(registry, engine_keypair.clone())
            .with_storage(store.clone());
        engine.register_handler(Arc::new(TestHandler {
            operation: "test.echo".to_string(),
        }));

        let mut proof = create_proof(
            engine_keypair.principal_id,
            None,
            "test.echo::v1",
            &json!({}),
            &json!({}),
            Utc::now() - chrono::Duration::hours(2),
            &engine_keypair,
        )
        .unwrap();
        proof.body.expires_at = Some(Utc::now() - chrono::Duration::hours(1));
        store.proofs.lock().unwrap().push(proof.clone());

        let error = engine
            .execute("test.echo", "v1", &json!({}), &test_context())
            .unwrap_err();

        assert_eq!(
            error,
            ExecutionError::BenchmarkExpired {
                benchmark: "B1".to_string(),
                proof_id: proof.body.id.to_string(),
            }
        );
    }

    #[test]
    fn allows_execution_when_latest_benchmark_proof_is_not_expired() {
        let store = Arc::new(RecordingStore::default());
        let mut entry = test_registry_entry("test.echo", Governance::AgentExecutable);
        entry.benchmark = Some("B1".to_string());
        let registry = Registry::new(vec![entry]).unwrap();
        let engine_keypair = crate::identity::generate_keypair();
        let mut engine = ExecutionEngine::new_with_keypair(registry, engine_keypair.clone())
            .with_storage(store.clone());
        engine.register_handler(Arc::new(TestHandler {
            operation: "test.echo".to_string(),
        }));

        let mut proof = create_proof(
            engine_keypair.principal_id,
            None,
            "test.echo::v1",
            &json!({}),
            &json!({}),
            Utc::now(),
            &engine_keypair,
        )
        .unwrap();
        proof.body.expires_at = Some(Utc::now() + chrono::Duration::hours(1));
        store.proofs.lock().unwrap().push(proof);

        let context = ExecutionContext {
            actor: engine_keypair.principal_id,
            ..test_context()
        };
        let result = engine
            .execute("test.echo", "v1", &json!({}), &context)
            .unwrap();

        assert_eq!(result["handled_by"], "test.echo");
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

    fn valid_chain(context: &ExecutionContext, grant: Delegation) -> DelegationChain {
        let recipient = context.actor;
        DelegationChain {
            root: grant.issuer,
            grants: vec![Delegation { recipient, ..grant }],
        }
    }

    fn grant_with_scope(context: &ExecutionContext, scope: DelegationScope) -> Delegation {
        Delegation {
            id: Uuid::now_v7(),
            issuer: PrincipalId::now(),
            recipient: context.actor,
            allowed_actions: vec!["*".to_string()],
            resource_scope: vec!["*".to_string()],
            scope,
            valid_from: context.timestamp - Duration::seconds(1),
            valid_until: context.timestamp + Duration::seconds(1),
            revoked: false,
        }
    }

    #[test]
    fn executes_operation_without_delegation() {
        let engine = test_engine(vec![test_registry_entry(
            "test.echo",
            Governance::AgentExecutable,
        )]);

        engine
            .execute("test.echo", "v1", &json!({}), &test_context())
            .unwrap();
    }

    #[test]
    fn executes_operation_with_valid_delegation_scope() {
        let store = Arc::new(RecordingStore::default());
        let keypair = crate::identity::generate_keypair();
        let registry = Registry::new(vec![test_registry_entry(
            "test.echo",
            Governance::AgentExecutable,
        )])
        .unwrap();
        let mut engine = ExecutionEngine::new_with_keypair(registry, keypair.clone())
            .with_storage(store.clone());
        engine.register_handler(Arc::new(TestHandler {
            operation: "test.echo".to_string(),
        }));
        let mut context = test_context();
        context.actor = keypair.principal_id;
        let grant = grant_with_scope(
            &context,
            DelegationScope {
                allowed_operations: Some(vec!["test.echo".to_string()]),
                allowed_domains: Some(vec!["test".to_string()]),
                resource_scope: None,
            },
        );
        context.delegation_id = Some(grant.id);
        context.delegation_chain = Some(valid_chain(&context, grant.clone()));
        store.delegations.lock().unwrap().push(grant);

        engine
            .execute("test.echo", "v1", &json!({}), &context)
            .unwrap();
    }

    #[test]
    fn rejects_operation_outside_delegation_scope() {
        let store = Arc::new(RecordingStore::default());
        let keypair = crate::identity::generate_keypair();
        let registry = Registry::new(vec![test_registry_entry(
            "test.echo",
            Governance::AgentExecutable,
        )])
        .unwrap();
        let mut engine = ExecutionEngine::new_with_keypair(registry, keypair.clone())
            .with_storage(store.clone());
        engine.register_handler(Arc::new(TestHandler {
            operation: "test.echo".to_string(),
        }));
        let context = test_context();
        let grant = grant_with_scope(
            &context,
            DelegationScope {
                allowed_operations: Some(vec!["test.other".to_string()]),
                allowed_domains: Some(vec!["other".to_string()]),
                resource_scope: None,
            },
        );
        let mut context = context;
        context.delegation_id = Some(grant.id);
        context.delegation_chain = Some(valid_chain(&context, grant.clone()));
        store.delegations.lock().unwrap().push(grant);

        let error = engine
            .execute("test.echo", "v1", &json!({}), &context)
            .unwrap_err();

        assert_eq!(error, ExecutionError::ScopeViolation);
    }

    #[test]
    fn rejects_missing_delegation() {
        let store = Arc::new(RecordingStore::default());
        let keypair = crate::identity::generate_keypair();
        let registry = Registry::new(vec![test_registry_entry(
            "test.echo",
            Governance::AgentExecutable,
        )])
        .unwrap();
        let mut engine = ExecutionEngine::new_with_keypair(registry, keypair).with_storage(store);
        engine.register_handler(Arc::new(TestHandler {
            operation: "test.echo".to_string(),
        }));
        let context = test_context();
        let grant = grant_with_scope(&context, DelegationScope::default());
        let mut context = context;
        context.delegation_id = Some(grant.id);
        context.delegation_chain = Some(valid_chain(&context, grant));

        let error = engine
            .execute("test.echo", "v1", &json!({}), &context)
            .unwrap_err();

        assert_eq!(
            error,
            ExecutionError::Delegation(DelegationError::EmptyChain)
        );
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
                scope: crate::delegation::DelegationScope::default(),
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
                scope: crate::delegation::DelegationScope::default(),
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
