//! The ExecutionEngine implementation.

use super::context::ExecutionContext;
use super::error::{ExecutionError, IdempotencyError};
use super::store::{
    ExecutionReplayClaim, ExecutionReplayClaimResult, ExecutionReplayKey, ExecutionStore,
    IdempotencyPolicy, OperationHandler,
};
use crate::agent_run::AgentRunMode;
use crate::approval::ApprovalGrant;
use crate::canonical::{canonicalize, digest, ArtifactKind};
use crate::delegation::DelegationError;
use crate::evidence::{Proof, ProofError};
use crate::identity::{Principal, PrincipalId, PrincipalKind};
use crate::operator::{
    control_digest, control_digest_serialized, GovernedAdapterRegistration,
    GovernedAdapterReporter, GovernedEffectPolicy, GovernedExecutionPlan,
    OperatorControlEnvironment, OperatorSchemaCatalog, PreparedGovernedExecution,
    PreparedReplayTransition, PreparedUsage, RegisteredGovernedAdapter,
};
use crate::registry::{Governance, Registry, VersionStatus};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

pub struct ExecutionEngine {
    registry: Registry,
    handlers: HashMap<String, Arc<dyn OperationHandler>>,
    governed_handlers:
        HashMap<(String, String), (RegisteredGovernedAdapter, Arc<dyn OperationHandler>)>,
    storage: Option<Arc<dyn ExecutionStore>>,
    keypair: Arc<crate::identity::Keypair>,
    operator_environment: Option<Arc<dyn OperatorControlEnvironment>>,
    operator_catalog: Option<Arc<OperatorSchemaCatalog>>,
    consumed_operator_permits: Mutex<HashSet<Uuid>>,
    consumed_operator_tokens: Mutex<HashSet<crate::operator::ControlDigest>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionOutcome {
    pub output: Value,
    pub proof: Proof,
}

struct InternalExecutionOutcome {
    output: Value,
    proof: Option<Proof>,
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
            governed_handlers: HashMap::new(),
            storage: None,
            keypair,
            operator_environment: None,
            operator_catalog: None,
            consumed_operator_permits: Mutex::new(HashSet::new()),
            consumed_operator_tokens: Mutex::new(HashSet::new()),
        }
    }

    /// Sets the optional storage backend used to persist successful executions.
    pub fn with_storage(mut self, storage: Arc<dyn ExecutionStore>) -> Self {
        self.storage = Some(storage);
        self
    }

    /// Installs the immutable catalog and process-authority environment used
    /// by prepared governed execution.
    pub fn with_operator_control(
        mut self,
        environment: Arc<dyn OperatorControlEnvironment>,
        catalog: Arc<OperatorSchemaCatalog>,
    ) -> Self {
        self.operator_environment = Some(environment);
        self.operator_catalog = Some(catalog);
        self
    }

    /// Registers a handler for an operation.
    pub fn register_handler(&mut self, handler: Arc<dyn OperationHandler>) {
        self.handlers
            .insert(handler.operation().to_string(), handler);
    }

    /// Registers a governed handler through one opaque metering capability.
    pub fn register_governed_handler<F>(
        &mut self,
        registration: GovernedAdapterRegistration,
        factory: F,
    ) -> Result<(), ExecutionError>
    where
        F: FnOnce(GovernedAdapterReporter) -> Arc<dyn OperationHandler>,
    {
        let (reporter, registered) = registration.mint();
        let handler = factory(reporter);
        if handler.operation() != registered.operation()
            || handler.governed_effect_policy_for(registered.version())
                != GovernedEffectPolicy::NoDurableOrExternalEffect
            || self
                .registry
                .find(registered.operation(), registered.version())
                .is_none()
        {
            return Err(ExecutionError::HandlerFailed(
                "governed handler registration does not match its adapter".into(),
            ));
        }
        self.handlers
            .insert(handler.operation().to_string(), handler.clone());
        self.governed_handlers.insert(
            (
                registered.operation().to_string(),
                registered.version().to_string(),
            ),
            (registered, handler),
        );
        Ok(())
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
        self.execute_operation(operation, version, input, context, None, false)
            .map(|outcome| outcome.output)
    }

    /// Executes an operation and returns its signed proof with the output.
    pub fn execute_evidenced(
        &self,
        operation: &str,
        version: &str,
        input: &Value,
        context: &ExecutionContext,
    ) -> Result<ExecutionOutcome, ExecutionError> {
        let outcome = self.execute_operation(operation, version, input, context, None, true)?;
        Ok(ExecutionOutcome {
            output: outcome.output,
            proof: outcome
                .proof
                .expect("evidenced execution always creates a proof"),
        })
    }

    /// Executes a human-only operation after verifying signed approval evidence.
    pub fn execute_with_approval(
        &self,
        operation: &str,
        version: &str,
        input: &Value,
        context: &ExecutionContext,
        approval: &ApprovalGrant,
        trusted_approver: &Principal,
    ) -> Result<Value, ExecutionError> {
        self.execute_operation(
            operation,
            version,
            input,
            context,
            Some((approval, trusted_approver)),
            false,
        )
        .map(|outcome| outcome.output)
    }

    /// Executes a human-only operation and returns signed execution evidence.
    pub fn execute_with_approval_evidenced(
        &self,
        operation: &str,
        version: &str,
        input: &Value,
        context: &ExecutionContext,
        approval: &ApprovalGrant,
        trusted_approver: &Principal,
    ) -> Result<ExecutionOutcome, ExecutionError> {
        let outcome = self.execute_operation(
            operation,
            version,
            input,
            context,
            Some((approval, trusted_approver)),
            true,
        )?;
        Ok(ExecutionOutcome {
            output: outcome.output,
            proof: outcome
                .proof
                .expect("evidenced execution always creates a proof"),
        })
    }

    /// Executes one permit-authorized bounded handler without any store call.
    pub fn execute_evidenced_unpersisted(
        &self,
        operation: &str,
        version: &str,
        input: &Value,
        context: &ExecutionContext,
        plan: GovernedExecutionPlan<'_>,
    ) -> Result<PreparedGovernedExecution, ExecutionError> {
        let entry = self.registry.find(operation, version).ok_or_else(|| {
            ExecutionError::OperationNotFound {
                operation: operation.to_string(),
                version: version.to_string(),
            }
        })?;
        let environment = self.operator_environment.as_ref().ok_or_else(|| {
            ExecutionError::EvidenceFailed("operator environment is not configured".into())
        })?;
        let catalog = self.operator_catalog.as_ref().ok_or_else(|| {
            ExecutionError::EvidenceFailed("operator schema catalog is not configured".into())
        })?;
        let trusted_now = environment
            .trusted_utc_now()
            .map_err(|error| ExecutionError::EvidenceFailed(error.to_string()))?;
        if entry.status == VersionStatus::Sunset {
            return Err(ExecutionError::Sunset);
        }
        if entry.governance == Governance::HumanOnly
            && context.principal_kind != Some(PrincipalKind::Human)
        {
            return Err(ExecutionError::HumanOnly);
        }
        if context.actor != plan.run_before.actor || context.actor != self.keypair.principal_id {
            return Err(ExecutionError::HandlerFailed(
                "governed actor mismatch".into(),
            ));
        }
        match (&context.delegation_id, &context.delegation_chain) {
            (None, None) => {}
            (Some(delegation_id), Some(chain)) => {
                chain.validate(context.actor, trusted_now)?;
                if chain.grants.last().map(|grant| grant.id) != Some(*delegation_id)
                    || chain
                        .grants
                        .iter()
                        .any(|grant| !grant.scope.scope_allows_operation(operation, &entry.domain))
                {
                    return Err(ExecutionError::ScopeViolation);
                }
            }
            _ => return Err(ExecutionError::Delegation(DelegationError::EmptyChain)),
        }
        catalog
            .validate_input(operation, version, input)
            .map_err(|_| {
                ExecutionError::EvidenceFailed("governed input failed schema validation".into())
            })?;
        let input_canonical =
            canonicalize(input).map_err(|e| ExecutionError::EvidenceFailed(e.to_string()))?;
        let input_digest = digest(ArtifactKind::OperationInput, &input_canonical);
        let argument_digest = control_digest(
            "Proof-Operator-Dispatch-Argument-v1",
            input_canonical.as_bytes(),
        );

        let (registered, handler) = self
            .governed_handlers
            .get(&(operation.into(), version.into()))
            .ok_or_else(|| {
                ExecutionError::HandlerFailed("handler has no governed adapter registration".into())
            })?;
        if handler.operation() != operation
            || handler.governed_effect_policy_for(version)
                != GovernedEffectPolicy::NoDurableOrExternalEffect
            || !registered.matches_intent(&plan.intent)
        {
            return Err(ExecutionError::HandlerFailed(
                "handler is ineligible for governed execution".into(),
            ));
        }
        plan.intent
            .validate()
            .map_err(|_| ExecutionError::HandlerFailed("dispatch intent is invalid".into()))?;
        let permit = plan.authorization.permit().clone();
        let authorization_intent = plan.authorization.intent().clone();
        let authorization_replay = plan.authorization.replay_binding().cloned();
        let authorization_claim_token = plan.authorization.replay_claim_token();
        let authorization_workspace_id = plan.authorization.workspace_id();
        let intent_digest =
            control_digest_serialized("Proof-Operator-Dispatch-Intent-v1", &plan.intent)
                .map_err(|e| ExecutionError::EvidenceFailed(e.to_string()))?;
        let call_digest =
            control_digest_serialized("Proof-Operator-Dispatch-Call-v1", &plan.intent)
                .map_err(|e| ExecutionError::EvidenceFailed(e.to_string()))?;
        if authorization_intent != plan.intent
            || plan.intent.operation != operation
            || plan.intent.version != version
            || plan.intent.argument_digest != argument_digest
            || permit.run_id != plan.run_before.id
            || plan.step_before.run_id != plan.run_before.id
            || plan.step_before.operation != operation
            || plan.step_before.version != version
            || plan.step_before.input_digest != input_digest
            || plan.run_before.status != crate::AgentRunStatus::Running
            || plan.step_before.status != crate::AgentRunStepStatus::Running
            || plan.run_before.completed_at.is_some()
            || plan.step_before.approval_request_id.is_some()
            || plan.step_before.output.is_some()
            || plan.step_before.proof.is_some()
            || plan.step_before.error.is_some()
            || plan.step_before.completed_at.is_some()
            || plan.run_before.revision > crate::operator::MAX_SAFE_INTEGER
            || plan.step_before.revision > crate::operator::MAX_SAFE_INTEGER
            || !crate::operator::uuid_is_v7(plan.run_before.id)
            || !crate::operator::uuid_is_v7(plan.step_before.id)
            || plan
                .step_before
                .retry_of
                .is_some_and(|id| !crate::operator::uuid_is_v7(id))
            || plan.run_before.created_at > plan.run_before.updated_at
            || plan.step_before.created_at > plan.step_before.updated_at
            || plan.run_before.updated_at > trusted_now
            || plan.step_before.updated_at > trusted_now
            || plan.step_before.started_at.is_none_or(|started| {
                started < plan.step_before.created_at || started > trusted_now
            })
            || permit.validate().is_err()
            || permit.intent_digest != intent_digest
            || permit.call_digest != call_digest
            || permit.authorized_at > trusted_now
            || permit.authorized_at < plan.run_before.updated_at
            || permit.authorized_at < plan.step_before.updated_at
        {
            return Err(ExecutionError::HandlerFailed(
                "governed execution plan does not match its permit".into(),
            ));
        }
        let checkpoint = plan.checkpoint_tail.ok_or_else(|| {
            ExecutionError::HandlerFailed("governed execution requires a checkpoint tail".into())
        })?;
        if !crate::operator::uuid_is_v7(checkpoint.checkpoint_id) {
            return Err(ExecutionError::HandlerFailed(
                "governed checkpoint is invalid".into(),
            ));
        }

        match handler.idempotency_policy_for(version) {
            IdempotencyPolicy::None
                if plan.replay_claim.is_some()
                    || authorization_replay.is_some()
                    || authorization_claim_token.is_some()
                    || permit.replay_binding_digest.is_some() =>
            {
                return Err(ExecutionError::HandlerFailed(
                    "non-replay execution carried replay authority".into(),
                ))
            }
            IdempotencyPolicy::RequiredUuidV7ExactReplay => {
                let claim = plan.replay_claim.as_ref().ok_or_else(|| {
                    ExecutionError::HandlerFailed("required replay claim is missing".into())
                })?;
                let binding = authorization_replay.as_ref().ok_or_else(|| {
                    ExecutionError::HandlerFailed("required replay binding is missing".into())
                })?;
                binding.validate().map_err(|_| {
                    ExecutionError::HandlerFailed("replay binding is invalid".into())
                })?;
                if binding.workspace_id != authorization_workspace_id
                    || binding.run_id != plan.run_before.id
                    || binding.step_id != plan.step_before.id
                    || binding.checkpoint_id != checkpoint.checkpoint_id
                    || binding.checkpoint_sequence != u64::from(checkpoint.sequence)
                    || binding.checkpoint_digest != checkpoint.state_digest
                    || binding.operation != operation
                    || binding.version != version
                    || binding.idempotency_key != claim.key.idempotency_key
                    || binding.input_digest != input_digest
                    || binding.claimed_by != context.actor
                    || binding.recomputed_binding_digest().map_err(|_| {
                        ExecutionError::HandlerFailed("replay binding digest is invalid".into())
                    })? != binding.binding_digest
                    || claim.key.operation != operation
                    || claim.key.version != version
                    || claim.input_digest != input_digest
                    || claim.claimed_by != context.actor
                    || authorization_claim_token != Some(claim.claim_token)
                    || !crate::operator::uuid_is_v7(claim.key.idempotency_key)
                    || !crate::operator::uuid_is_v7(claim.claim_token)
                    || claim.claimed_at > trusted_now
                    || permit.replay_binding_digest != Some(binding.binding_digest)
                {
                    return Err(ExecutionError::HandlerFailed(
                        "replay claim does not match governed execution".into(),
                    ));
                }
            }
            IdempotencyPolicy::None => {}
        }
        let execution_context_id = environment
            .new_uuid_v7()
            .map_err(|e| ExecutionError::EvidenceFailed(e.to_string()))?;
        let proof_id = environment
            .new_uuid_v7()
            .map_err(|e| ExecutionError::EvidenceFailed(e.to_string()))?;
        if !crate::operator::uuid_is_v7(execution_context_id)
            || !crate::operator::uuid_is_v7(proof_id)
        {
            return Err(ExecutionError::EvidenceFailed(
                "operator environment returned a non-UUIDv7 identifier".into(),
            ));
        }
        let boundary_now = environment
            .trusted_utc_now()
            .map_err(|e| ExecutionError::HandlerFailed(e.to_string()))?;
        let monotonic_now = environment
            .monotonic_millis()
            .map_err(|e| ExecutionError::HandlerFailed(e.to_string()))?;
        if boundary_now < trusted_now || context.timestamp > boundary_now {
            return Err(ExecutionError::HandlerFailed(
                "governed execution chronology is invalid".into(),
            ));
        }
        let permit_id = permit.permit_id;
        let permit_token_digest = permit.dispatch_token_digest;
        if handler.operation() != operation
            || handler.governed_effect_policy_for(version)
                != GovernedEffectPolicy::NoDurableOrExternalEffect
            || !registered.matches_intent(&plan.intent)
        {
            return Err(ExecutionError::HandlerFailed(
                "governed handler registration changed before boundary entry".into(),
            ));
        }
        {
            let mut consumed = self.consumed_operator_permits.lock().map_err(|_| {
                ExecutionError::HandlerFailed("permit registry is unavailable".into())
            })?;
            let mut consumed_tokens = self.consumed_operator_tokens.lock().map_err(|_| {
                ExecutionError::HandlerFailed("permit registry is unavailable".into())
            })?;
            if consumed.contains(&permit_id) || consumed_tokens.contains(&permit_token_digest) {
                return Err(ExecutionError::HandlerFailed(
                    "dispatch permit was already consumed".into(),
                ));
            }
            consumed.insert(permit_id);
            consumed_tokens.insert(permit_token_digest);
        }
        plan.authorization
            .consume_effect(boundary_now, monotonic_now)?;
        let prepared_output = handler.execute_governed_versioned(version, input, context)?;
        let (prepared_value, mutation, boundary_usage) = prepared_output.into_parts();
        if mutation != crate::operator::PreparedHandlerMutation::NoEffect {
            return Err(ExecutionError::HandlerFailed(
                "governed handler returned an effectful mutation".into(),
            ));
        }
        catalog
            .validate_output(operation, version, &prepared_value)
            .map_err(|_| {
                ExecutionError::EvidenceFailed("governed output failed schema validation".into())
            })?;
        let output_canonical = canonicalize(&prepared_value)
            .map_err(|e| ExecutionError::EvidenceFailed(e.to_string()))?;
        let output_digest = digest(ArtifactKind::OperationOutput, &output_canonical);
        let usage = PreparedUsage::from_report(
            &plan.intent,
            registered,
            boundary_usage,
            input_digest,
            output_digest,
        )?;

        let proof = Proof::new(
            proof_id,
            context.actor,
            context.delegation_id,
            format!("{operation}::{version}"),
            input_digest,
            output_digest,
            boundary_now,
        )
        .sign(&self.keypair)
        .map_err(|e| ExecutionError::EvidenceFailed(e.to_string()))?;
        let mut step_after = plan.step_before;
        step_after
            .succeed(prepared_value.clone(), proof.clone(), boundary_now)
            .map_err(|e| ExecutionError::HandlerFailed(e.to_string()))?;
        let mut run_after = plan.run_before;
        if run_after.mode == AgentRunMode::OneShot {
            run_after
                .succeed(boundary_now)
                .map_err(|e| ExecutionError::HandlerFailed(e.to_string()))?;
        }
        let replay = match plan.replay_claim {
            Some(claim) => PreparedReplayTransition::Complete(claim),
            None => PreparedReplayTransition::None,
        };
        Ok(PreparedGovernedExecution::new(
            prepared_value,
            execution_context_id,
            ExecutionContext {
                timestamp: boundary_now,
                ..context.clone()
            },
            proof,
            run_after,
            step_after,
            None,
            Vec::new(),
            None,
            None,
            replay,
            usage,
        ))
    }

    fn execute_operation(
        &self,
        operation: &str,
        version: &str,
        input: &Value,
        context: &ExecutionContext,
        approval: Option<(&ApprovalGrant, &Principal)>,
        evidence_required: bool,
    ) -> Result<InternalExecutionOutcome, ExecutionError> {
        #[cfg(feature = "tracing")]
        let mut operation_span =
            proof_observability::OperationSpan::new(operation, version, context.actor.to_string());
        #[cfg(feature = "tracing")]
        let result = self.execute_inner(
            operation,
            version,
            input,
            context,
            approval,
            evidence_required,
            &mut operation_span,
        );
        #[cfg(not(feature = "tracing"))]
        let result = self.execute_inner(
            operation,
            version,
            input,
            context,
            approval,
            evidence_required,
        );
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
        approval: Option<(&ApprovalGrant, &Principal)>,
        evidence_required: bool,
        #[cfg(feature = "tracing")] operation_span: &mut proof_observability::OperationSpan,
    ) -> Result<InternalExecutionOutcome, ExecutionError> {
        let entry = self.registry.find(operation, version).ok_or_else(|| {
            ExecutionError::OperationNotFound {
                operation: operation.to_string(),
                version: version.to_string(),
            }
        })?;

        if entry.governance == Governance::HumanOnly
            && context.principal_kind != Some(PrincipalKind::Human)
        {
            let Some((approval, trusted_approver)) = approval else {
                return Err(ExecutionError::HumanOnly);
            };
            approval.verify_for_execution(
                &self.keypair,
                trusted_approver,
                operation,
                version,
                input,
                context.actor,
                context.timestamp,
            )?;
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

        let handler = self
            .handlers
            .get(operation)
            .ok_or_else(|| ExecutionError::NoHandler(operation.to_string()))?;

        let replay_claim = match handler.idempotency_policy_for(version) {
            IdempotencyPolicy::None => None,
            IdempotencyPolicy::RequiredUuidV7ExactReplay => {
                let claim = self.execution_replay_claim(operation, version, input, context)?;
                let storage = self
                    .storage
                    .as_ref()
                    .ok_or(IdempotencyError::StorageRequired)?;
                match storage
                    .claim_execution_replay(&claim)
                    .map_err(ExecutionError::StorageFailed)?
                {
                    ExecutionReplayClaimResult::Acquired => Some(claim),
                    ExecutionReplayClaimResult::Completed(outcome) => {
                        self.validate_replayed_outcome(&claim, &outcome)?;
                        return Ok(InternalExecutionOutcome {
                            output: outcome.output,
                            proof: Some(outcome.proof),
                        });
                    }
                    ExecutionReplayClaimResult::Conflict => {
                        return Err(IdempotencyError::Conflict.into())
                    }
                    ExecutionReplayClaimResult::InProgress => {
                        return Err(IdempotencyError::InProgress.into())
                    }
                    ExecutionReplayClaimResult::Failed => {
                        return Err(IdempotencyError::Indeterminate.into())
                    }
                    ExecutionReplayClaimResult::Unsupported => {
                        return Err(IdempotencyError::StorageRequired.into())
                    }
                }
            }
        };

        if let Some(benchmark_id) = &entry.benchmark {
            if let Some(storage) = &self.storage {
                let latest_proof = match storage.latest_proof_for_operation(operation, version) {
                    Ok(proof) => proof,
                    Err(error) => {
                        let error = ExecutionError::StorageFailed(error);
                        return Err(self.fail_acquired_claim(
                            replay_claim.as_ref(),
                            context.timestamp,
                            error,
                        ));
                    }
                };
                if latest_proof
                    .as_ref()
                    .is_some_and(|proof| proof.is_expired(context.timestamp))
                {
                    let error = ExecutionError::BenchmarkExpired {
                        benchmark: benchmark_id.clone(),
                        proof_id: latest_proof
                            .expect("expired proof checked above")
                            .body
                            .id
                            .to_string(),
                    };
                    return Err(self.fail_acquired_claim(
                        replay_claim.as_ref(),
                        context.timestamp,
                        error,
                    ));
                }
            }
        }

        let output = match handler.execute_versioned(version, input, context) {
            Ok(output) => output,
            Err(error) => {
                return Err(self.fail_acquired_claim(
                    replay_claim.as_ref(),
                    context.timestamp,
                    error,
                ))
            }
        };
        let proof = if evidence_required || self.storage.is_some() || replay_claim.is_some() {
            match self.create_operation_proof(operation, version, input, &output, context) {
                Ok(proof) => Some(proof),
                Err(error) => {
                    let error = ExecutionError::EvidenceFailed(error.to_string());
                    return Err(self.fail_acquired_claim(
                        replay_claim.as_ref(),
                        context.timestamp,
                        error,
                    ));
                }
            }
        } else {
            None
        };
        #[cfg(feature = "tracing")]
        if let Some(proof) = &proof {
            operation_span.set_proof_id(proof.body.id.to_string());
        }

        if let Some(claim) = replay_claim.as_ref() {
            let outcome = ExecutionOutcome {
                output: output.clone(),
                proof: proof
                    .as_ref()
                    .expect("exact replay always creates a proof")
                    .clone(),
            };
            self.storage
                .as_ref()
                .expect("exact replay requires storage")
                .complete_execution_replay(claim, context, &outcome)
                .map_err(ExecutionError::StorageFailed)?;
        } else if let Some(storage) = &self.storage {
            storage
                .save_execution_context(context)
                .map_err(ExecutionError::StorageFailed)?;
            storage
                .save_proof(proof.as_ref().expect("storage execution creates a proof"))
                .map_err(ExecutionError::StorageFailed)?;
        }

        Ok(InternalExecutionOutcome { output, proof })
    }

    fn execution_replay_claim(
        &self,
        operation: &str,
        version: &str,
        input: &Value,
        context: &ExecutionContext,
    ) -> Result<ExecutionReplayClaim, ExecutionError> {
        let idempotency_key = input
            .get("idempotency_key")
            .ok_or(IdempotencyError::MissingKey)?
            .as_str()
            .ok_or(IdempotencyError::InvalidUuidV7)?;
        let idempotency_key =
            Uuid::parse_str(idempotency_key).map_err(|_| IdempotencyError::InvalidUuidV7)?;
        if !crate::operator::uuid_is_v7(idempotency_key) {
            return Err(IdempotencyError::InvalidUuidV7.into());
        }
        let input = canonicalize(input)
            .map_err(|error| ExecutionError::EvidenceFailed(error.to_string()))?;
        Ok(ExecutionReplayClaim {
            key: ExecutionReplayKey {
                operation: operation.to_string(),
                version: version.to_string(),
                idempotency_key,
            },
            input_digest: digest(ArtifactKind::OperationInput, &input),
            claim_token: Uuid::now_v7(),
            claimed_by: context.actor,
            claimed_at: context.timestamp,
        })
    }

    fn validate_replayed_outcome(
        &self,
        claim: &ExecutionReplayClaim,
        outcome: &ExecutionOutcome,
    ) -> Result<(), ExecutionError> {
        let expected_operation = format!("{}::{}", claim.key.operation, claim.key.version);
        if outcome.proof.body.operation != expected_operation {
            return Err(ExecutionError::StorageFailed(
                "stored replay proof operation does not match the claim".to_string(),
            ));
        }
        if outcome.proof.body.input_digest != claim.input_digest {
            return Err(ExecutionError::StorageFailed(
                "stored replay proof input digest does not match the claim".to_string(),
            ));
        }
        let output = canonicalize(&outcome.output).map_err(|_| {
            ExecutionError::StorageFailed(
                "stored replay output could not be canonicalized".to_string(),
            )
        })?;
        if outcome.proof.body.output_digest != digest(ArtifactKind::OperationOutput, &output) {
            return Err(ExecutionError::StorageFailed(
                "stored replay proof output digest does not match the output".to_string(),
            ));
        }
        Ok(())
    }

    fn fail_acquired_claim(
        &self,
        claim: Option<&ExecutionReplayClaim>,
        failed_at: DateTime<Utc>,
        error: ExecutionError,
    ) -> ExecutionError {
        let Some(claim) = claim else {
            return error;
        };
        let storage = self
            .storage
            .as_ref()
            .expect("an acquired replay claim always has storage");
        match storage.fail_execution_replay(claim, failed_at, &error.to_string()) {
            Ok(()) => error,
            Err(failure) => ExecutionError::StorageFailed(format!(
                "failed to record indeterminate execution after {error}: {failure}"
            )),
        }
    }

    pub(crate) fn create_operation_proof(
        &self,
        operation: &str,
        version: &str,
        input: &Value,
        output: &Value,
        context: &ExecutionContext,
    ) -> Result<Proof, ProofError> {
        let proof_operation = format!("{operation}::{version}");
        create_proof(
            context.actor,
            context.delegation_id,
            &proof_operation,
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
            let delegation = storage
                .load_delegation(&delegation_id)
                .map_err(ExecutionError::StorageFailed)?
                .ok_or(ExecutionError::Delegation(DelegationError::EmptyChain))?;
            let chain_index = chain
                .grants
                .iter()
                .position(|grant| grant.id == delegation_id)
                .ok_or(ExecutionError::Delegation(DelegationError::EmptyChain))?;
            if chain.grants[chain_index] != delegation {
                return Err(ExecutionError::Delegation(DelegationError::EmptyChain));
            }
            if delegation.recipient != context.actor {
                return Err(ExecutionError::Delegation(
                    DelegationError::InvalidTerminalAgent { index: chain_index },
                ));
            }
            delegation
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

#[cfg(test)]
mod tests {
    use super::super::store::RecordingStore;
    use super::*;
    use crate::approval::{
        ApprovalError, ApprovalGrant, ApprovalOutcome, SignedApprovalDecision,
        SignedApprovalRequest,
    };
    use crate::delegation::{Delegation, DelegationChain, DelegationScope};
    use crate::identity::{generate_keypair, generate_keypair_for, principal_from_keypair};
    use chrono::Duration;
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use super::super::context::AuditFilter;

    struct BoundaryExpiryEnvironment {
        utc_calls: AtomicUsize,
        base: DateTime<Utc>,
    }

    impl crate::operator::OperatorControlEnvironment for BoundaryExpiryEnvironment {
        fn trusted_utc_now(
            &self,
        ) -> Result<DateTime<Utc>, crate::operator::OperatorEnvironmentError> {
            let call = self.utc_calls.fetch_add(1, Ordering::SeqCst);
            Ok(if call >= 2 {
                self.base + Duration::minutes(1)
            } else {
                self.base
            })
        }

        fn monotonic_millis(&self) -> Result<u64, crate::operator::OperatorEnvironmentError> {
            Ok(0)
        }

        fn fill_random(
            &self,
            _: crate::operator::OperatorRandomPurpose,
            output: &mut [u8],
        ) -> Result<(), crate::operator::OperatorEnvironmentError> {
            output.fill(7);
            Ok(())
        }

        fn new_uuid_v7(&self) -> Result<Uuid, crate::operator::OperatorEnvironmentError> {
            Ok(Uuid::now_v7())
        }
    }

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

    struct GovernedTestHandler {
        reporter: GovernedAdapterReporter,
        calls: Arc<AtomicUsize>,
        tokens: u64,
        report_tool: bool,
        replay_required: bool,
    }

    impl OperationHandler for GovernedTestHandler {
        fn operation(&self) -> &str {
            "test.echo"
        }
        fn execute(&self, _: &Value, _: &ExecutionContext) -> Result<Value, ExecutionError> {
            panic!("legacy execution must not be entered by governed execution")
        }
        fn governed_effect_policy_for(&self, _: &str) -> GovernedEffectPolicy {
            GovernedEffectPolicy::NoDurableOrExternalEffect
        }
        fn idempotency_policy_for(&self, _: &str) -> IdempotencyPolicy {
            if self.replay_required {
                IdempotencyPolicy::RequiredUuidV7ExactReplay
            } else {
                IdempotencyPolicy::None
            }
        }
        fn execute_governed_versioned(
            &self,
            _: &str,
            _: &Value,
            _: &ExecutionContext,
        ) -> Result<crate::operator::PreparedHandlerOutput, ExecutionError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.report_tool {
                self.reporter.tool_output(json!({"value":"out"}))
            } else {
                self.reporter
                    .provider_output(json!({"value":"out"}), self.tokens, 3)
            }
        }
    }

    fn governed_catalog(
        entry: &crate::registry::RegistryEntry,
    ) -> Arc<crate::operator::OperatorSchemaCatalog> {
        let schema = br#"{"$schema":"https://json-schema.org/draft/2020-12/schema","type":"object","additionalProperties":false,"required":["value"],"properties":{"value":{"type":"string"}}}"#;
        Arc::new(
            crate::operator::OperatorSchemaCatalog::from_source_inventory(
                crate::operator::OperatorSchemaSourceInventory {
                    entries: vec![crate::operator::OperatorSchemaSource {
                        registry_entry_path: "test/echo.json".into(),
                        registry_entry: serde_json::to_vec(entry).unwrap(),
                        input_schema_path: "test.input.json".into(),
                        input_schema: schema.to_vec(),
                        output_schema_path: "test.output.json".into(),
                        output_schema: schema.to_vec(),
                    }],
                },
            )
            .unwrap(),
        )
    }

    fn execute_governed_fixture(
        engine: &ExecutionEngine,
        keypair: &crate::identity::Keypair,
        environment: &dyn crate::operator::OperatorControlEnvironment,
        token: [u8; 32],
        permit_id: Uuid,
    ) -> Result<PreparedGovernedExecution, ExecutionError> {
        execute_governed_fixture_with_intent(engine, keypair, environment, token, permit_id, |_| {})
    }

    fn execute_governed_fixture_with_intent<F>(
        engine: &ExecutionEngine,
        keypair: &crate::identity::Keypair,
        environment: &dyn crate::operator::OperatorControlEnvironment,
        token: [u8; 32],
        permit_id: Uuid,
        mutate_intent: F,
    ) -> Result<PreparedGovernedExecution, ExecutionError>
    where
        F: FnOnce(&mut crate::operator::DispatchIntent),
    {
        execute_governed_fixture_config(
            engine,
            keypair,
            environment,
            token,
            permit_id,
            false,
            mutate_intent,
        )
    }

    fn execute_governed_replay_fixture(
        engine: &ExecutionEngine,
        keypair: &crate::identity::Keypair,
        environment: &dyn crate::operator::OperatorControlEnvironment,
        token: [u8; 32],
        permit_id: Uuid,
    ) -> Result<PreparedGovernedExecution, ExecutionError> {
        execute_governed_fixture_config(
            engine,
            keypair,
            environment,
            token,
            permit_id,
            true,
            |_| {},
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_governed_fixture_config<F>(
        engine: &ExecutionEngine,
        keypair: &crate::identity::Keypair,
        environment: &dyn crate::operator::OperatorControlEnvironment,
        token: [u8; 32],
        permit_id: Uuid,
        replay_required: bool,
        mutate_intent: F,
    ) -> Result<PreparedGovernedExecution, ExecutionError>
    where
        F: FnOnce(&mut crate::operator::DispatchIntent),
    {
        execute_governed_fixture_config_with_claim(
            engine,
            keypair,
            environment,
            token,
            permit_id,
            replay_required,
            mutate_intent,
            |_| {},
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_governed_fixture_config_with_claim<F, G>(
        engine: &ExecutionEngine,
        keypair: &crate::identity::Keypair,
        environment: &dyn crate::operator::OperatorControlEnvironment,
        token: [u8; 32],
        permit_id: Uuid,
        replay_required: bool,
        mutate_intent: F,
        mutate_claim: G,
    ) -> Result<PreparedGovernedExecution, ExecutionError>
    where
        F: FnOnce(&mut crate::operator::DispatchIntent),
        G: FnOnce(&mut Option<ExecutionReplayClaim>),
    {
        execute_governed_fixture_config_with_claim_and_custody(
            engine,
            keypair,
            environment,
            token,
            permit_id,
            replay_required,
            mutate_intent,
            mutate_claim,
        )
        .map(|fixture| fixture.prepared)
    }

    struct GovernedFixtureWithCustody {
        prepared: PreparedGovernedExecution,
        dispatch: crate::operator::DispatchTokenCustody,
        lease: crate::operator::LeaseTokenCustody,
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_governed_fixture_config_with_claim_and_custody<F, G>(
        engine: &ExecutionEngine,
        keypair: &crate::identity::Keypair,
        environment: &dyn crate::operator::OperatorControlEnvironment,
        token: [u8; 32],
        permit_id: Uuid,
        replay_required: bool,
        mutate_intent: F,
        mutate_claim: G,
    ) -> Result<GovernedFixtureWithCustody, ExecutionError>
    where
        F: FnOnce(&mut crate::operator::DispatchIntent),
        G: FnOnce(&mut Option<ExecutionReplayClaim>),
    {
        let at: DateTime<Utc> = "2031-02-03T04:05:06Z".parse().unwrap();
        let input = json!({"value":"in"});
        let input_digest = digest(ArtifactKind::OperationInput, &canonicalize(&input).unwrap());
        let run_id = Uuid::now_v7();
        let run = crate::AgentRun {
            id: run_id,
            actor: keypair.principal_id,
            agent_id: Some(Uuid::now_v7()),
            mode: AgentRunMode::OneShot,
            goal: "test".into(),
            status: crate::AgentRunStatus::Running,
            retry_count: 0,
            revision: 1,
            created_at: at,
            updated_at: at,
            completed_at: None,
        };
        let step = crate::AgentRunStep {
            id: Uuid::now_v7(),
            run_id,
            ordinal: 0,
            attempt: 1,
            retry_of: None,
            operation: "test.echo".into(),
            version: "v1".into(),
            input_digest,
            status: crate::AgentRunStepStatus::Running,
            approval_request_id: None,
            output: None,
            proof: None,
            error: None,
            revision: 1,
            created_at: at,
            updated_at: at,
            started_at: Some(at),
            completed_at: None,
        };
        let mut intent = crate::operator::DispatchIntent {
            schema: crate::operator::DispatchIntent::SCHEMA.into(),
            kind: if replay_required {
                crate::operator::BoundaryKind::Tool
            } else {
                crate::operator::BoundaryKind::Provider
            },
            adapter: if replay_required {
                "synthetic_tool".into()
            } else {
                "synthetic".into()
            },
            model: (!replay_required).then(|| "fixed-v1".into()),
            operation: "test.echo".into(),
            version: "v1".into(),
            argument_digest: crate::operator::control_digest(
                "Proof-Operator-Dispatch-Argument-v1",
                canonicalize(&input).unwrap().as_bytes(),
            ),
            ceiling: crate::operator::BudgetAmounts {
                steps: 1,
                tokens: if replay_required { 0 } else { 10 },
                duration_ms: 1,
                cost_microusd: if replay_required { 0 } else { 3 },
                tool_dispatches: u64::from(replay_required),
            },
        };
        mutate_intent(&mut intent);
        let intent_digest = crate::operator::control_digest_serialized(
            "Proof-Operator-Dispatch-Intent-v1",
            &intent,
        )
        .unwrap();
        let call_digest =
            crate::operator::control_digest_serialized("Proof-Operator-Dispatch-Call-v1", &intent)
                .unwrap();
        let reservation_id = Uuid::now_v7();
        let lease_id = Uuid::now_v7();
        let workspace_id = Uuid::now_v7();
        let owner_instance_id = Uuid::now_v7();
        let process_epoch_id = Uuid::now_v7();
        let checkpoint = crate::AgentCheckpointTail {
            checkpoint_id: Uuid::now_v7(),
            sequence: 0,
            state_digest: digest(
                ArtifactKind::AgentCheckpoint,
                &canonicalize(&json!({"state":"before"})).unwrap(),
            ),
        };
        let (replay, mut replay_claim) = if replay_required {
            let claim_token = Uuid::now_v7();
            let mut binding = crate::operator::ReplayClaimBinding {
                schema: crate::operator::ReplayClaimBinding::SCHEMA.into(),
                policy: crate::operator::ReplayPolicy::RequiredUuidv7ExactReplay,
                workspace_id,
                run_id,
                step_id: step.id,
                checkpoint_id: checkpoint.checkpoint_id,
                checkpoint_sequence: u64::from(checkpoint.sequence),
                checkpoint_digest: checkpoint.state_digest,
                operation: "test.echo".into(),
                version: "v1".into(),
                idempotency_key: Uuid::now_v7(),
                input_digest,
                claimed_by: keypair.principal_id,
                binding_digest: crate::operator::ControlDigest::from_bytes([0; 32]),
            };
            binding.binding_digest = binding.recomputed_binding_digest().unwrap();
            let claim = ExecutionReplayClaim {
                key: ExecutionReplayKey {
                    operation: binding.operation.clone(),
                    version: binding.version.clone(),
                    idempotency_key: binding.idempotency_key,
                },
                input_digest,
                claim_token,
                claimed_by: keypair.principal_id,
                claimed_at: at,
            };
            (Some(binding), Some(claim))
        } else {
            (None, None)
        };
        let lease_token = [8; 32];
        let mut lease_custody = crate::operator::LeaseTokenCustody::new(lease_token);
        {
            let request = lease_custody.claim_request(
                workspace_id,
                run_id,
                lease_id,
                owner_instance_id,
                process_epoch_id,
                0,
                0,
            )?;
            assert!(
                request.verifies_lease_token_digest(crate::operator::control_digest(
                    "Proof-Operator-Lease-Token-v1",
                    &lease_token,
                ))
            );
        }
        let mut bound_lease = crate::operator::RunLease {
            schema: crate::operator::RunLease::SCHEMA.into(),
            run_id,
            workspace_id,
            lease_id,
            owner_instance_id,
            process_epoch_id,
            lease_token_digest: crate::operator::control_digest(
                "Proof-Operator-Lease-Token-v1",
                &lease_token,
            ),
            fence_epoch: 1,
            revision: 0,
            state: crate::operator::RunLeaseState::Active,
            acquired_at: at,
            renewed_at: at,
            expires_at: at + Duration::seconds(30),
            released_at: None,
            lease_digest: crate::operator::ControlDigest::from_bytes([0; 32]),
        };
        let mut lease_value = serde_json::to_value(&bound_lease).unwrap();
        lease_value.as_object_mut().unwrap().remove("lease_digest");
        bound_lease.lease_digest =
            crate::operator::control_digest_serialized("Proof-Operator-Lease-v1", &lease_value)
                .unwrap();
        lease_custody.bind_claim_result(&crate::operator::LeaseMutationResult {
            schema: "proof.operator.lease-mutation-result/v1".into(),
            outcome: crate::operator::LeaseMutationOutcome::Acquired,
            lease: bound_lease,
            control_revision: 1,
        })?;
        let permit = crate::operator::DispatchPermit {
            schema: crate::operator::DispatchPermit::SCHEMA.into(),
            permit_id,
            run_id,
            reservation_id,
            lease_id,
            process_epoch_id,
            fence_epoch: 1,
            expected_control_revision: 1,
            intent_digest,
            replay_binding_digest: replay.as_ref().map(|binding| binding.binding_digest),
            dispatch_token_digest: crate::operator::control_digest(
                "Proof-Operator-Dispatch-Token-v1",
                &token,
            ),
            call_digest,
            authorized_at: at,
            budget_deadline_at: at + Duration::minutes(1),
        };
        let live = AtomicBool::new(true);
        let mut custody = crate::operator::DispatchTokenCustody::new(token);
        {
            let authority = lease_custody.authority(1)?;
            let request = custody.begin_request(
                authority,
                reservation_id,
                intent.clone(),
                intent_digest,
                replay.clone(),
                replay_claim.as_ref().map(|claim| claim.claim_token),
                call_digest,
            )?;
            assert!(request.verifies_dispatch_token_digest(permit.dispatch_token_digest));
        }
        custody.bind_permit(
            &crate::operator::DispatchResult {
                schema: crate::operator::DispatchResult::SCHEMA.into(),
                outcome: crate::operator::DispatchOutcome::DispatchAuthorized,
                permit: Some(permit),
                replay_completion: None,
                control_revision: 2,
            },
            environment,
        )?;
        mutate_claim(&mut replay_claim);
        let authorization = custody.authorization(&live)?;
        let prepared = engine.execute_evidenced_unpersisted(
            "test.echo",
            "v1",
            &input,
            &ExecutionContext {
                actor: keypair.principal_id,
                principal_kind: Some(PrincipalKind::Agent),
                delegation_id: None,
                delegation_chain: None,
                workspace_path: PathBuf::from("/workspace"),
                timestamp: at,
            },
            GovernedExecutionPlan {
                authorization,
                intent,
                run_before: run,
                step_before: step,
                checkpoint_tail: Some(checkpoint),
                replay_claim,
            },
        )?;
        Ok(GovernedFixtureWithCustody {
            prepared,
            dispatch: custody,
            lease: lease_custody,
        })
    }

    #[test]
    fn governed_execution_is_unpersisted_and_consumes_authority_once() {
        let entry = test_registry_entry("test.echo", Governance::AgentExecutable);
        let registry = Registry::new(vec![entry.clone()]).unwrap();
        let keypair = generate_keypair();
        let store = Arc::new(RecordingStore::default());
        let environment = Arc::new(crate::operator::RecordingOperatorControlEnvironment::new(
            "2031-02-03T04:05:06Z".parse().unwrap(),
            [4; 32],
        ));
        let calls = Arc::new(AtomicUsize::new(0));
        let mut engine = ExecutionEngine::new_with_keypair(registry, keypair.clone())
            .with_storage(store.clone())
            .with_operator_control(environment.clone(), governed_catalog(&entry));
        engine
            .register_governed_handler(
                GovernedAdapterRegistration::new(
                    "test.echo",
                    "v1",
                    crate::operator::BoundaryKind::Provider,
                    "synthetic",
                    Some("fixed-v1".into()),
                )
                .unwrap(),
                {
                    let calls = calls.clone();
                    move |reporter| {
                        Arc::new(GovernedTestHandler {
                            reporter,
                            calls,
                            tokens: 10,
                            report_tool: false,
                            replay_required: false,
                        })
                    }
                },
            )
            .unwrap();
        let token = [5; 32];
        let permit_id = Uuid::now_v7();
        let prepared =
            execute_governed_fixture(&engine, &keypair, environment.as_ref(), token, permit_id)
                .unwrap();
        assert_eq!(prepared.output(), &json!({"value":"out"}));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(store.proofs.lock().unwrap().is_empty());
        assert!(store.contexts.lock().unwrap().is_empty());
        assert!(execute_governed_fixture(
            &engine,
            &keypair,
            environment.as_ref(),
            token,
            permit_id
        )
        .is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn invalid_prepared_results_reach_commit_barrier_for_atomic_forfeit() {
        let entry = test_registry_entry("test.echo", Governance::AgentExecutable);
        let registry = Registry::new(vec![entry.clone()]).unwrap();
        let keypair = generate_keypair();
        let environment = Arc::new(crate::operator::RecordingOperatorControlEnvironment::new(
            "2031-02-03T04:05:06Z".parse().unwrap(),
            [4; 32],
        ));
        let calls = Arc::new(AtomicUsize::new(0));
        let mut engine = ExecutionEngine::new_with_keypair(registry, keypair.clone())
            .with_operator_control(environment.clone(), governed_catalog(&entry));
        engine
            .register_governed_handler(
                GovernedAdapterRegistration::new(
                    "test.echo",
                    "v1",
                    crate::operator::BoundaryKind::Provider,
                    "synthetic",
                    Some("fixed-v1".into()),
                )
                .unwrap(),
                {
                    let calls = calls.clone();
                    move |reporter| {
                        Arc::new(GovernedTestHandler {
                            reporter,
                            calls,
                            tokens: 2,
                            report_tool: false,
                            replay_required: false,
                        })
                    }
                },
            )
            .unwrap();

        let store = crate::operator::RecordingOperatorControlStore::default();
        for case in 0_u8..3 {
            let fixture = execute_governed_fixture_config_with_claim_and_custody(
                &engine,
                &keypair,
                environment.as_ref(),
                [50 + case; 32],
                Uuid::now_v7(),
                false,
                |_| {},
                |_| {},
            )
            .unwrap();
            let GovernedFixtureWithCustody {
                prepared,
                dispatch,
                lease,
            } = fixture;
            let mut binding =
                crate::operator::PreparedExecutionBinding::from_prepared(&prepared, None).unwrap();
            match case {
                0 => binding.result_digest = crate::operator::ControlDigest::from_bytes([0; 32]),
                1 => {
                    binding.result.proof.operation = "test.other".into();
                    binding.result_digest = crate::operator::control_digest_serialized(
                        "Proof-Operator-Runtime-Result-v1",
                        &binding.result,
                    )
                    .unwrap();
                }
                2 => {
                    binding.result.usage.tokens = 11;
                    binding.result_digest = crate::operator::control_digest_serialized(
                        "Proof-Operator-Runtime-Result-v1",
                        &binding.result,
                    )
                    .unwrap();
                }
                _ => unreachable!(),
            }
            let request = dispatch
                .into_commit_request(lease.authority(2).unwrap(), binding)
                .unwrap();
            assert!(!request.prepared_matches_dispatch());
            store.inject_error(
                crate::operator::OperatorStoreBoundary::CommitRuntimeBarrier,
                crate::operator::OperatorStoreError::Invalid,
            );
            assert_eq!(
                crate::operator::OperatorRuntimeStore::commit_runtime_barrier(
                    &store, request, prepared,
                ),
                Err(crate::operator::OperatorStoreError::Invalid)
            );
        }
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert_eq!(
            store.calls(),
            vec![crate::operator::OperatorStoreBoundary::CommitRuntimeBarrier; 3]
        );
        let requests = store.requests();
        assert_eq!(requests.len(), 3);
        assert!(requests.iter().enumerate().all(|(case, request)| matches!(
            request,
            crate::operator::RecordingOperatorRequest::Commit {
                authority,
                reservation_id,
                permit_id,
                dispatch_token_digest,
                intent_ceiling: crate::operator::BudgetAmounts {
                    steps: 1,
                    tokens: 10,
                    duration_ms: 1,
                    cost_microusd: 3,
                    tool_dispatches: 0,
                },
                prepared_matches_dispatch: false,
            } if authority.fence_epoch == 1
                && authority.expected_control_revision == 2
                && !reservation_id.is_nil()
                && !permit_id.is_nil()
                && *dispatch_token_digest == crate::operator::control_digest(
                    "Proof-Operator-Dispatch-Token-v1",
                    &[50 + case as u8; 32],
                )
        )));
    }

    #[test]
    fn governed_execution_rejects_ineligible_and_over_ceiling_before_publish() {
        for (ordinary, tokens, expected_calls) in [(true, 1, 0), (false, 11, 1)] {
            let entry = test_registry_entry("test.echo", Governance::AgentExecutable);
            let registry = Registry::new(vec![entry.clone()]).unwrap();
            let keypair = generate_keypair();
            let calls = Arc::new(AtomicUsize::new(0));
            let environment = Arc::new(crate::operator::RecordingOperatorControlEnvironment::new(
                "2031-02-03T04:05:06Z".parse().unwrap(),
                [4; 32],
            ));
            let mut engine = ExecutionEngine::new_with_keypair(registry, keypair.clone())
                .with_operator_control(environment.clone(), governed_catalog(&entry));
            if ordinary {
                engine.register_handler(Arc::new(TestHandler {
                    operation: "test.echo".into(),
                }));
            } else {
                engine
                    .register_governed_handler(
                        GovernedAdapterRegistration::new(
                            "test.echo",
                            "v1",
                            crate::operator::BoundaryKind::Provider,
                            "synthetic",
                            Some("fixed-v1".into()),
                        )
                        .unwrap(),
                        {
                            let calls = calls.clone();
                            move |reporter| {
                                Arc::new(GovernedTestHandler {
                                    reporter,
                                    calls,
                                    tokens,
                                    report_tool: false,
                                    replay_required: false,
                                })
                            }
                        },
                    )
                    .unwrap();
            }
            let error = execute_governed_fixture(
                &engine,
                &keypair,
                environment.as_ref(),
                [6; 32],
                Uuid::now_v7(),
            )
            .unwrap_err();
            if !ordinary {
                assert!(matches!(&error, ExecutionError::EvidenceFailed(_)));
                assert_eq!(
                    crate::operator::governed_runtime_failure_code(&error),
                    crate::operator::RuntimeFailureCode::ResultInvalid
                );
            }
            assert_eq!(calls.load(Ordering::SeqCst), expected_calls);
        }
    }

    #[test]
    fn governed_registration_metadata_mismatches_are_zero_entry_rejections() {
        let entry = test_registry_entry("test.echo", Governance::AgentExecutable);
        let registry = Registry::new(vec![entry.clone()]).unwrap();
        let keypair = generate_keypair();
        let calls = Arc::new(AtomicUsize::new(0));
        let environment = Arc::new(crate::operator::RecordingOperatorControlEnvironment::new(
            "2031-02-03T04:05:06Z".parse().unwrap(),
            [4; 32],
        ));
        let mut engine = ExecutionEngine::new_with_keypair(registry, keypair.clone())
            .with_operator_control(environment.clone(), governed_catalog(&entry));
        engine
            .register_governed_handler(
                GovernedAdapterRegistration::new(
                    "test.echo",
                    "v1",
                    crate::operator::BoundaryKind::Provider,
                    "synthetic",
                    Some("fixed-v1".into()),
                )
                .unwrap(),
                {
                    let calls = calls.clone();
                    move |reporter| {
                        Arc::new(GovernedTestHandler {
                            reporter,
                            calls,
                            tokens: 1,
                            report_tool: false,
                            replay_required: false,
                        })
                    }
                },
            )
            .unwrap();
        for case in 0..5 {
            let result = execute_governed_fixture_with_intent(
                &engine,
                &keypair,
                environment.as_ref(),
                [case + 20; 32],
                Uuid::now_v7(),
                |intent| match case {
                    0 => intent.adapter = "different_adapter".into(),
                    1 => intent.model = Some("different-model".into()),
                    2 => {
                        intent.kind = crate::operator::BoundaryKind::Tool;
                        intent.model = None;
                        intent.ceiling.tokens = 0;
                        intent.ceiling.cost_microusd = 0;
                        intent.ceiling.tool_dispatches = 1;
                    }
                    3 => intent.version = "v2".into(),
                    _ => {
                        intent.argument_digest = crate::operator::ControlDigest::from_bytes([7; 32])
                    }
                },
            );
            assert!(result.is_err());
            assert_eq!(calls.load(Ordering::SeqCst), 0);
        }
    }

    #[test]
    fn governed_reporter_wrong_branch_is_rejected_after_one_boundary_entry() {
        let entry = test_registry_entry("test.echo", Governance::AgentExecutable);
        let registry = Registry::new(vec![entry.clone()]).unwrap();
        let keypair = generate_keypair();
        let calls = Arc::new(AtomicUsize::new(0));
        let environment = Arc::new(crate::operator::RecordingOperatorControlEnvironment::new(
            "2031-02-03T04:05:06Z".parse().unwrap(),
            [4; 32],
        ));
        let mut engine = ExecutionEngine::new_with_keypair(registry, keypair.clone())
            .with_operator_control(environment.clone(), governed_catalog(&entry));
        engine
            .register_governed_handler(
                GovernedAdapterRegistration::new(
                    "test.echo",
                    "v1",
                    crate::operator::BoundaryKind::Provider,
                    "synthetic",
                    Some("fixed-v1".into()),
                )
                .unwrap(),
                {
                    let calls = calls.clone();
                    move |reporter| {
                        Arc::new(GovernedTestHandler {
                            reporter,
                            calls,
                            tokens: 0,
                            report_tool: true,
                            replay_required: false,
                        })
                    }
                },
            )
            .unwrap();
        let error = execute_governed_fixture(
            &engine,
            &keypair,
            environment.as_ref(),
            [33; 32],
            Uuid::now_v7(),
        )
        .unwrap_err();
        assert!(matches!(&error, ExecutionError::EvidenceFailed(_)));
        assert_eq!(
            crate::operator::governed_runtime_failure_code(&error),
            crate::operator::RuntimeFailureCode::ResultInvalid
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn governed_invalid_output_is_a_typed_result_failure() {
        struct InvalidOutputHandler {
            reporter: GovernedAdapterReporter,
            calls: Arc<AtomicUsize>,
        }

        impl OperationHandler for InvalidOutputHandler {
            fn operation(&self) -> &str {
                "test.echo"
            }
            fn execute(&self, _: &Value, _: &ExecutionContext) -> Result<Value, ExecutionError> {
                panic!("legacy execution must not be entered by governed execution")
            }
            fn governed_effect_policy_for(&self, _: &str) -> GovernedEffectPolicy {
                GovernedEffectPolicy::NoDurableOrExternalEffect
            }
            fn execute_governed_versioned(
                &self,
                _: &str,
                _: &Value,
                _: &ExecutionContext,
            ) -> Result<crate::operator::PreparedHandlerOutput, ExecutionError> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                self.reporter
                    .provider_output(json!({"unexpected": true}), 1, 1)
            }
        }

        let entry = test_registry_entry("test.echo", Governance::AgentExecutable);
        let registry = Registry::new(vec![entry.clone()]).unwrap();
        let keypair = generate_keypair();
        let calls = Arc::new(AtomicUsize::new(0));
        let environment = Arc::new(crate::operator::RecordingOperatorControlEnvironment::new(
            "2031-02-03T04:05:06Z".parse().unwrap(),
            [4; 32],
        ));
        let mut engine = ExecutionEngine::new_with_keypair(registry, keypair.clone())
            .with_operator_control(environment.clone(), governed_catalog(&entry));
        engine
            .register_governed_handler(
                GovernedAdapterRegistration::new(
                    "test.echo",
                    "v1",
                    crate::operator::BoundaryKind::Provider,
                    "synthetic",
                    Some("fixed-v1".into()),
                )
                .unwrap(),
                {
                    let calls = calls.clone();
                    move |reporter| Arc::new(InvalidOutputHandler { reporter, calls })
                },
            )
            .unwrap();
        let error = execute_governed_fixture(
            &engine,
            &keypair,
            environment.as_ref(),
            [34; 32],
            Uuid::now_v7(),
        )
        .unwrap_err();
        assert!(matches!(&error, ExecutionError::EvidenceFailed(_)));
        assert_eq!(
            crate::operator::governed_runtime_failure_code(&error),
            crate::operator::RuntimeFailureCode::ResultInvalid
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn governed_actor_mismatch_is_zero_entry_rejection() {
        let entry = test_registry_entry("test.echo", Governance::AgentExecutable);
        let registry = Registry::new(vec![entry.clone()]).unwrap();
        let engine_keypair = generate_keypair();
        let request_keypair = generate_keypair();
        let calls = Arc::new(AtomicUsize::new(0));
        let environment = Arc::new(crate::operator::RecordingOperatorControlEnvironment::new(
            "2031-02-03T04:05:06Z".parse().unwrap(),
            [4; 32],
        ));
        let mut engine = ExecutionEngine::new_with_keypair(registry, engine_keypair)
            .with_operator_control(environment.clone(), governed_catalog(&entry));
        engine
            .register_governed_handler(
                GovernedAdapterRegistration::new(
                    "test.echo",
                    "v1",
                    crate::operator::BoundaryKind::Provider,
                    "synthetic",
                    Some("fixed-v1".into()),
                )
                .unwrap(),
                {
                    let calls = calls.clone();
                    move |reporter| {
                        Arc::new(GovernedTestHandler {
                            reporter,
                            calls,
                            tokens: 1,
                            report_tool: false,
                            replay_required: false,
                        })
                    }
                },
            )
            .unwrap();
        assert!(execute_governed_fixture(
            &engine,
            &request_keypair,
            environment.as_ref(),
            [34; 32],
            Uuid::now_v7(),
        )
        .is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn governed_deadline_equality_is_rechecked_at_boundary_with_zero_entry() {
        let entry = test_registry_entry("test.echo", Governance::AgentExecutable);
        let registry = Registry::new(vec![entry.clone()]).unwrap();
        let keypair = generate_keypair();
        let calls = Arc::new(AtomicUsize::new(0));
        let environment = Arc::new(BoundaryExpiryEnvironment {
            utc_calls: AtomicUsize::new(0),
            base: "2031-02-03T04:05:06Z".parse().unwrap(),
        });
        let mut engine = ExecutionEngine::new_with_keypair(registry, keypair.clone())
            .with_operator_control(environment.clone(), governed_catalog(&entry));
        engine
            .register_governed_handler(
                GovernedAdapterRegistration::new(
                    "test.echo",
                    "v1",
                    crate::operator::BoundaryKind::Provider,
                    "synthetic",
                    Some("fixed-v1".into()),
                )
                .unwrap(),
                {
                    let calls = calls.clone();
                    move |reporter| {
                        Arc::new(GovernedTestHandler {
                            reporter,
                            calls,
                            tokens: 1,
                            report_tool: false,
                            replay_required: false,
                        })
                    }
                },
            )
            .unwrap();
        assert!(execute_governed_fixture(
            &engine,
            &keypair,
            environment.as_ref(),
            [35; 32],
            Uuid::now_v7(),
        )
        .is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn governed_required_exact_replay_completes_the_bound_claim() {
        let entry = test_registry_entry("test.echo", Governance::AgentExecutable);
        let registry = Registry::new(vec![entry.clone()]).unwrap();
        let keypair = generate_keypair();
        let calls = Arc::new(AtomicUsize::new(0));
        let environment = Arc::new(crate::operator::RecordingOperatorControlEnvironment::new(
            "2031-02-03T04:05:06Z".parse().unwrap(),
            [4; 32],
        ));
        let mut engine = ExecutionEngine::new_with_keypair(registry, keypair.clone())
            .with_operator_control(environment.clone(), governed_catalog(&entry));
        engine
            .register_governed_handler(
                GovernedAdapterRegistration::new(
                    "test.echo",
                    "v1",
                    crate::operator::BoundaryKind::Tool,
                    "synthetic_tool",
                    None,
                )
                .unwrap(),
                {
                    let calls = calls.clone();
                    move |reporter| {
                        Arc::new(GovernedTestHandler {
                            reporter,
                            calls,
                            tokens: 0,
                            report_tool: true,
                            replay_required: true,
                        })
                    }
                },
            )
            .unwrap();
        let prepared = execute_governed_replay_fixture(
            &engine,
            &keypair,
            environment.as_ref(),
            [36; 32],
            Uuid::now_v7(),
        )
        .unwrap();
        assert!(matches!(
            prepared.replay(),
            crate::operator::PreparedReplayTransition::Complete(_)
        ));
        assert_eq!(prepared.usage().tool_dispatches(), 1);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn governed_replay_policy_rejects_missing_mismatched_and_unexpected_authority_zero_entry() {
        fn engine_for_policy(
            replay_required: bool,
        ) -> (
            ExecutionEngine,
            crate::identity::Keypair,
            Arc<crate::operator::RecordingOperatorControlEnvironment>,
            Arc<AtomicUsize>,
        ) {
            let entry = test_registry_entry("test.echo", Governance::AgentExecutable);
            let registry = Registry::new(vec![entry.clone()]).unwrap();
            let keypair = generate_keypair();
            let calls = Arc::new(AtomicUsize::new(0));
            let environment = Arc::new(crate::operator::RecordingOperatorControlEnvironment::new(
                "2031-02-03T04:05:06Z".parse().unwrap(),
                [4; 32],
            ));
            let mut engine = ExecutionEngine::new_with_keypair(registry, keypair.clone())
                .with_operator_control(environment.clone(), governed_catalog(&entry));
            engine
                .register_governed_handler(
                    GovernedAdapterRegistration::new(
                        "test.echo",
                        "v1",
                        crate::operator::BoundaryKind::Tool,
                        "synthetic_tool",
                        None,
                    )
                    .unwrap(),
                    {
                        let calls = calls.clone();
                        move |reporter| {
                            Arc::new(GovernedTestHandler {
                                reporter,
                                calls,
                                tokens: 0,
                                report_tool: true,
                                replay_required,
                            })
                        }
                    },
                )
                .unwrap();
            (engine, keypair, environment, calls)
        }

        let (required_engine, keypair, environment, calls) = engine_for_policy(true);
        assert!(execute_governed_fixture_config_with_claim(
            &required_engine,
            &keypair,
            environment.as_ref(),
            [41; 32],
            Uuid::now_v7(),
            true,
            |_| {},
            |claim| *claim = None,
        )
        .is_err());
        assert!(execute_governed_fixture_config_with_claim(
            &required_engine,
            &keypair,
            environment.as_ref(),
            [42; 32],
            Uuid::now_v7(),
            true,
            |_| {},
            |claim| claim.as_mut().unwrap().claimed_by = crate::PrincipalId::now(),
        )
        .is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 0);

        let (none_engine, keypair, environment, calls) = engine_for_policy(false);
        assert!(execute_governed_replay_fixture(
            &none_engine,
            &keypair,
            environment.as_ref(),
            [43; 32],
            Uuid::now_v7(),
        )
        .is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
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
            principal_kind: Some(PrincipalKind::Agent),
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
    fn evidenced_execution_returns_matching_signed_proof() {
        let keypair = generate_keypair();
        let registry = Registry::new(vec![test_registry_entry(
            "test.echo",
            Governance::AgentExecutable,
        )])
        .unwrap();
        let mut engine = ExecutionEngine::new_with_keypair(registry, keypair.clone());
        engine.register_handler(Arc::new(TestHandler {
            operation: "test.echo".to_string(),
        }));
        let input = json!({"msg": "hello"});
        let context = ExecutionContext {
            actor: keypair.principal_id,
            ..test_context()
        };

        let outcome = engine
            .execute_evidenced("test.echo", "v1", &input, &context)
            .unwrap();

        assert_eq!(outcome.output["echo"], input);
        assert_eq!(outcome.proof.body.operation, "test.echo::v1");
        assert_eq!(outcome.proof.body.actor, keypair.principal_id);
        outcome
            .proof
            .verify(&keypair.signing_key.verifying_key())
            .unwrap();
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
    fn allows_human_only_for_human_principals() {
        let store = Arc::new(RecordingStore::default());
        let entry = test_registry_entry("test.human_only", Governance::HumanOnly);
        let registry = Registry::new(vec![entry]).unwrap();
        let human_keypair = crate::identity::generate_keypair_for(PrincipalKind::Human);
        let mut engine = ExecutionEngine::new_with_keypair(registry, human_keypair.clone())
            .with_storage(store.clone());
        engine.register_handler(Arc::new(TestHandler {
            operation: "test.human_only".to_string(),
        }));
        let mut context = test_context();
        context.actor = human_keypair.principal_id;
        context.principal_kind = Some(PrincipalKind::Human);
        let result = engine
            .execute("test.human_only", "v1", &json!({}), &context)
            .unwrap();
        assert_eq!(result["handled_by"], "test.human_only");
    }

    #[test]
    fn executes_human_only_with_exact_signed_approval() {
        let requester = generate_keypair();
        let approver = generate_keypair_for(PrincipalKind::Human);
        let trusted_approver = principal_from_keypair(&approver);
        let registry = Registry::new(vec![test_registry_entry(
            "test.human_only",
            Governance::HumanOnly,
        )])
        .unwrap();
        let mut engine = ExecutionEngine::new_with_keypair(registry, requester.clone());
        engine.register_handler(Arc::new(TestHandler {
            operation: "test.human_only".to_string(),
        }));
        let input = json!({"change": "publish"});
        let context = ExecutionContext {
            actor: requester.principal_id,
            ..test_context()
        };
        let request = SignedApprovalRequest::create(
            "test.human_only",
            "v1",
            &input,
            context.timestamp - Duration::seconds(1),
            context.timestamp + Duration::minutes(15),
            &requester,
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
        let grant = ApprovalGrant {
            request,
            decision,
            approver: trusted_approver.clone(),
        };

        let result = engine
            .execute_with_approval(
                "test.human_only",
                "v1",
                &input,
                &context,
                &grant,
                &trusted_approver,
            )
            .unwrap();
        assert_eq!(result["handled_by"], "test.human_only");

        let error = engine
            .execute_with_approval(
                "test.human_only",
                "v1",
                &json!({"change": "different"}),
                &context,
                &grant,
                &trusted_approver,
            )
            .unwrap_err();
        assert_eq!(
            error,
            ExecutionError::Approval(ApprovalError::InputMismatch)
        );
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
