use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;

use serde::{ser::SerializeStruct, Deserialize, Serialize, Serializer};
use serde_json::Value;
use uuid::Uuid;
use zeroize::Zeroizing;

use super::{
    constant_time_eq_32, control_digest, valid_adapter_name, valid_model_name,
    valid_operation_name, valid_operation_version, ApprovalBinding, BeginDispatchRequest,
    BoundaryKind, ControlDigest, DispatchIntent, DispatchOutcome, DispatchPermit, DispatchResult,
    LeaseClaimRequest, LeaseMutationOutcome, LeaseMutationResult, LeaseReleaseRequest,
    OperatorControlEnvironment, ReclaimOutcome, ReclaimRequest, ReclaimResult, ReplayClaimBinding,
    RuntimeCommitRequest, RuntimeFailureBody, RuntimeFailureRequest,
};
use crate::{
    AgentCheckpoint, AgentCheckpointTail, AgentRun, AgentRunEvaluation, AgentRunEvent,
    AgentRunStep, ContentDigest, ExecutionContext, ExecutionError, ExecutionReplayClaim, Proof,
    SignedApprovalRequest,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GovernedEffectPolicy {
    Ineligible,
    NoDurableOrExternalEffect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreparedHandlerMutation {
    NoEffect,
}

pub struct PreparedBoundaryUsage {
    seal: Arc<GovernedReporterSeal>,
    boundary_kind: BoundaryKind,
    tokens: u64,
    cost_microusd: u64,
    tool_dispatches: u64,
}

pub struct PreparedHandlerOutput {
    output: Value,
    mutation: PreparedHandlerMutation,
    boundary_usage: PreparedBoundaryUsage,
}

impl PreparedHandlerOutput {
    pub fn output(&self) -> &Value {
        &self.output
    }
    pub fn mutation(&self) -> PreparedHandlerMutation {
        self.mutation
    }
    pub(crate) fn into_parts(self) -> (Value, PreparedHandlerMutation, PreparedBoundaryUsage) {
        (self.output, self.mutation, self.boundary_usage)
    }
}

struct GovernedReporterSeal {
    operation: String,
    version: String,
    kind: BoundaryKind,
    adapter: String,
    model: Option<String>,
}

/// Opaque capability minted only by governed handler registration.
///
/// It deliberately implements neither cloning, debugging, nor serialization.
pub struct GovernedAdapterReporter {
    seal: Arc<GovernedReporterSeal>,
}

impl GovernedAdapterReporter {
    /// Returns the typed governed-result error for an adapter response that
    /// omitted its mandatory usage report.
    pub fn missing_usage_error(&self) -> ExecutionError {
        ExecutionError::EvidenceFailed("prepared boundary usage is missing".into())
    }

    pub fn provider_output(
        &self,
        output: Value,
        tokens: u64,
        cost_microusd: u64,
    ) -> Result<PreparedHandlerOutput, ExecutionError> {
        if self.seal.kind != BoundaryKind::Provider {
            return Err(ExecutionError::EvidenceFailed(
                "governed reporter branch mismatch".into(),
            ));
        }
        Ok(PreparedHandlerOutput {
            output,
            mutation: PreparedHandlerMutation::NoEffect,
            boundary_usage: PreparedBoundaryUsage {
                seal: self.seal.clone(),
                boundary_kind: BoundaryKind::Provider,
                tokens,
                cost_microusd,
                tool_dispatches: 0,
            },
        })
    }

    pub fn tool_output(&self, output: Value) -> Result<PreparedHandlerOutput, ExecutionError> {
        if self.seal.kind != BoundaryKind::Tool {
            return Err(ExecutionError::EvidenceFailed(
                "governed reporter branch mismatch".into(),
            ));
        }
        Ok(PreparedHandlerOutput {
            output,
            mutation: PreparedHandlerMutation::NoEffect,
            boundary_usage: PreparedBoundaryUsage {
                seal: self.seal.clone(),
                boundary_kind: BoundaryKind::Tool,
                tokens: 0,
                cost_microusd: 0,
                tool_dispatches: 1,
            },
        })
    }
}

pub struct GovernedAdapterRegistration {
    operation: String,
    version: String,
    kind: BoundaryKind,
    adapter: String,
    model: Option<String>,
}

impl GovernedAdapterRegistration {
    pub fn new(
        operation: impl Into<String>,
        version: impl Into<String>,
        kind: BoundaryKind,
        adapter: impl Into<String>,
        model: Option<String>,
    ) -> Result<Self, ExecutionError> {
        let value = Self {
            operation: operation.into(),
            version: version.into(),
            kind,
            adapter: adapter.into(),
            model,
        };
        if !valid_operation_name(&value.operation)
            || !valid_operation_version(&value.version)
            || !valid_adapter_name(&value.adapter)
            || value
                .model
                .as_deref()
                .is_some_and(|model| !valid_model_name(model))
            || (value.kind == BoundaryKind::Provider) != value.model.is_some()
        {
            return Err(ExecutionError::HandlerFailed(
                "governed adapter registration is invalid".into(),
            ));
        }
        Ok(value)
    }

    pub(crate) fn mint(self) -> (GovernedAdapterReporter, RegisteredGovernedAdapter) {
        let seal = Arc::new(GovernedReporterSeal {
            operation: self.operation,
            version: self.version,
            kind: self.kind,
            adapter: self.adapter,
            model: self.model,
        });
        (
            GovernedAdapterReporter { seal: seal.clone() },
            RegisteredGovernedAdapter { seal },
        )
    }
}

pub(crate) struct RegisteredGovernedAdapter {
    seal: Arc<GovernedReporterSeal>,
}

impl RegisteredGovernedAdapter {
    pub(crate) fn operation(&self) -> &str {
        &self.seal.operation
    }
    pub(crate) fn version(&self) -> &str {
        &self.seal.version
    }
    pub(crate) fn matches_intent(&self, intent: &DispatchIntent) -> bool {
        self.seal.operation == intent.operation
            && self.seal.version == intent.version
            && self.seal.kind == intent.kind
            && self.seal.adapter == intent.adapter
            && self.seal.model == intent.model
    }
    fn owns(&self, usage: &PreparedBoundaryUsage) -> bool {
        Arc::ptr_eq(&self.seal, &usage.seal)
    }
}

pub struct DispatchTokenCustody {
    token: Zeroizing<[u8; 32]>,
    begin: Option<DispatchBeginBinding>,
    permit: Option<DispatchPermit>,
    monotonic_deadline: Option<u64>,
    authorization_state: AtomicU8,
    effect_state: AtomicU8,
    settlement_state: AtomicU8,
}

/// A failed settlement conversion that retains the sole dispatch custody.
///
/// The error deliberately omits custody from its debug representation. Callers
/// can correct a pre-barrier request error and recover the original custody.
pub struct DispatchSettlementConversionError {
    error: ExecutionError,
    custody: DispatchTokenCustody,
}

impl DispatchSettlementConversionError {
    pub fn error(&self) -> &ExecutionError {
        &self.error
    }

    pub fn into_custody(self) -> DispatchTokenCustody {
        self.custody
    }
}

impl fmt::Debug for DispatchSettlementConversionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DispatchSettlementConversionError")
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}

impl fmt::Display for DispatchSettlementConversionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.error.fmt(formatter)
    }
}

impl std::error::Error for DispatchSettlementConversionError {}

pub struct LeaseTokenCustody {
    token: Zeroizing<[u8; 32]>,
    establishment: Option<LeaseEstablishment>,
    bound: Option<BoundLease>,
    proof_state: AtomicU8,
}

#[derive(Clone)]
pub(crate) struct LeaseClaimBinding {
    pub(crate) workspace_id: Uuid,
    pub(crate) run_id: Uuid,
    pub(crate) lease_id: Uuid,
    pub(crate) owner_instance_id: Uuid,
    pub(crate) process_epoch_id: Uuid,
    pub(crate) expected_fence_epoch: u64,
    pub(crate) expected_control_revision: u64,
}

#[derive(Clone)]
enum LeaseEstablishment {
    Claim(LeaseClaimBinding),
    Reclaim {
        claim: LeaseClaimBinding,
        expired_lease_id: Uuid,
        checkpoint_id: Uuid,
        checkpoint_sequence: u64,
        checkpoint_digest: ContentDigest,
    },
}

#[derive(Clone)]
struct BoundLease {
    workspace_id: Uuid,
    run_id: Uuid,
    lease_id: Uuid,
    owner_instance_id: Uuid,
    process_epoch_id: Uuid,
    fence_epoch: u64,
}

#[derive(Clone)]
struct DispatchBeginBinding {
    workspace_id: Uuid,
    run_id: Uuid,
    lease_id: Uuid,
    process_epoch_id: Uuid,
    fence_epoch: u64,
    expected_control_revision: u64,
    reservation_id: Uuid,
    intent: DispatchIntent,
    replay: Option<ReplayClaimBinding>,
    replay_claim_token: Option<Uuid>,
    call_digest: ControlDigest,
}

impl LeaseTokenCustody {
    pub fn new(token: [u8; 32]) -> Self {
        Self {
            token: Zeroizing::new(token),
            establishment: None,
            bound: None,
            proof_state: AtomicU8::new(0),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn claim_request(
        &mut self,
        workspace_id: Uuid,
        run_id: Uuid,
        lease_id: Uuid,
        owner_instance_id: Uuid,
        process_epoch_id: Uuid,
        expected_fence_epoch: u64,
        expected_control_revision: u64,
    ) -> Result<LeaseClaimRequest<'_>, ExecutionError> {
        let claim = LeaseClaimBinding {
            workspace_id,
            run_id,
            lease_id,
            owner_instance_id,
            process_epoch_id,
            expected_fence_epoch,
            expected_control_revision,
        };
        validate_lease_claim_binding(&claim)?;
        self.begin_establishment(LeaseEstablishment::Claim(claim.clone()))?;
        Ok(LeaseClaimRequest::from_custody(
            claim,
            LeaseTokenProof { token: &self.token },
        ))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn reclaim_request(
        &mut self,
        workspace_id: Uuid,
        run_id: Uuid,
        expired_lease_id: Uuid,
        expected_fence_epoch: u64,
        expected_control_revision: u64,
        new_lease_id: Uuid,
        owner_instance_id: Uuid,
        new_process_epoch_id: Uuid,
        checkpoint_id: Uuid,
        checkpoint_sequence: u64,
        checkpoint_digest: ContentDigest,
    ) -> Result<ReclaimRequest<'_>, ExecutionError> {
        let claim = LeaseClaimBinding {
            workspace_id,
            run_id,
            lease_id: new_lease_id,
            owner_instance_id,
            process_epoch_id: new_process_epoch_id,
            expected_fence_epoch,
            expected_control_revision,
        };
        validate_lease_claim_binding(&claim)?;
        if !super::uuid_is_v7(expired_lease_id)
            || !super::uuid_is_v7(checkpoint_id)
            || checkpoint_sequence > super::MAX_SAFE_INTEGER
        {
            return Err(ExecutionError::HandlerFailed(
                "lease reclaim binding is invalid".into(),
            ));
        }
        self.begin_establishment(LeaseEstablishment::Reclaim {
            claim: claim.clone(),
            expired_lease_id,
            checkpoint_id,
            checkpoint_sequence,
            checkpoint_digest,
        })?;
        Ok(ReclaimRequest::from_custody(
            claim,
            expired_lease_id,
            checkpoint_id,
            checkpoint_sequence,
            checkpoint_digest,
            LeaseTokenProof { token: &self.token },
        ))
    }

    fn begin_establishment(
        &mut self,
        establishment: LeaseEstablishment,
    ) -> Result<(), ExecutionError> {
        if self.bound.is_some()
            || self.establishment.is_some()
            || self
                .proof_state
                .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return Err(ExecutionError::HandlerFailed(
                "lease establishment proof is unavailable".into(),
            ));
        }
        self.establishment = Some(establishment);
        Ok(())
    }

    pub fn bind_claim_result(
        &mut self,
        result: &LeaseMutationResult,
    ) -> Result<(), ExecutionError> {
        let Some(LeaseEstablishment::Claim(claim)) = self.establishment.as_ref() else {
            return Err(ExecutionError::HandlerFailed(
                "lease claim custody is not pending".into(),
            ));
        };
        if result.validate().is_err() || result.outcome != LeaseMutationOutcome::Acquired {
            return Err(ExecutionError::HandlerFailed(
                "lease claim did not acquire authority".into(),
            ));
        }
        let bound =
            validate_bound_lease(claim, &result.lease, result.control_revision, &self.token)?;
        self.bound = Some(bound);
        self.establishment = None;
        Ok(())
    }

    pub fn bind_reclaim_result(&mut self, result: &ReclaimResult) -> Result<(), ExecutionError> {
        let Some(LeaseEstablishment::Reclaim {
            claim,
            expired_lease_id,
            checkpoint_id,
            checkpoint_sequence,
            checkpoint_digest,
        }) = self.establishment.as_ref()
        else {
            return Err(ExecutionError::HandlerFailed(
                "lease reclaim custody is not pending".into(),
            ));
        };
        if result.validate().is_err()
            || !matches!(
                result.outcome,
                ReclaimOutcome::IdleReclaimed
                    | ReclaimOutcome::PreDispatchRecovered
                    | ReclaimOutcome::RecoverableReclaimed
            )
        {
            return Err(ExecutionError::HandlerFailed(
                "lease reclaim did not return authority".into(),
            ));
        }
        let lease = result.lease.as_ref().ok_or_else(|| {
            ExecutionError::HandlerFailed("lease reclaim result omitted authority".into())
        })?;
        let directive_branch = match result.outcome {
            ReclaimOutcome::IdleReclaimed => result.directive.is_none(),
            ReclaimOutcome::PreDispatchRecovered | ReclaimOutcome::RecoverableReclaimed => {
                result.directive.as_ref().is_some_and(|directive| {
                    directive.validate().is_ok()
                        && directive.workspace_id == claim.workspace_id
                        && directive.run_id == claim.run_id
                        && directive.source_lease_id == *expired_lease_id
                        && directive.checkpoint_id == *checkpoint_id
                        && directive.checkpoint_sequence == *checkpoint_sequence
                        && directive.checkpoint_digest == *checkpoint_digest
                        && directive.source_fence_epoch == claim.expected_fence_epoch
                        && directive.source_control_revision == claim.expected_control_revision
                })
            }
            ReclaimOutcome::AmbiguousForfeited => false,
        };
        if !directive_branch {
            return Err(ExecutionError::HandlerFailed(
                "lease reclaim result does not match its recovery branch".into(),
            ));
        }
        let bound = validate_bound_lease(claim, lease, result.control_revision, &self.token)?;
        self.bound = Some(bound);
        self.establishment = None;
        Ok(())
    }

    pub fn authority(
        &self,
        expected_control_revision: u64,
    ) -> Result<LeaseAuthority<'_>, ExecutionError> {
        let bound = self
            .bound
            .as_ref()
            .ok_or_else(|| ExecutionError::HandlerFailed("lease custody is not bound".into()))?;
        if expected_control_revision > super::MAX_SAFE_INTEGER {
            return Err(ExecutionError::HandlerFailed(
                "lease control revision is invalid".into(),
            ));
        }
        Ok(LeaseAuthority {
            schema: LeaseAuthority::SCHEMA.into(),
            workspace_id: bound.workspace_id,
            run_id: bound.run_id,
            lease_id: bound.lease_id,
            owner_instance_id: bound.owner_instance_id,
            process_epoch_id: bound.process_epoch_id,
            fence_epoch: bound.fence_epoch,
            expected_control_revision,
            lease_token: &self.token,
        })
    }

    pub fn into_release_request(
        self,
        expected_control_revision: u64,
    ) -> Result<LeaseReleaseRequest, ExecutionError> {
        if self.bound.is_none() || expected_control_revision > super::MAX_SAFE_INTEGER {
            return Err(ExecutionError::HandlerFailed(
                "lease release custody is unavailable".into(),
            ));
        }
        Ok(LeaseReleaseRequest::from_custody(
            self,
            expected_control_revision,
        ))
    }
}

pub struct LeaseTokenProof<'a> {
    token: &'a [u8; 32],
}
impl LeaseTokenProof<'_> {
    pub(super) fn digest(&self) -> ControlDigest {
        control_digest("Proof-Operator-Lease-Token-v1", self.token)
    }

    pub fn verifies_digest(&self, expected: ControlDigest) -> bool {
        let actual = self.digest();
        constant_time_eq_32(actual.as_bytes(), expected.as_bytes())
    }
}

pub struct LeaseAuthority<'a> {
    pub schema: String,
    pub workspace_id: Uuid,
    pub run_id: Uuid,
    pub lease_id: Uuid,
    pub owner_instance_id: Uuid,
    pub process_epoch_id: Uuid,
    pub fence_epoch: u64,
    pub expected_control_revision: u64,
    lease_token: &'a [u8; 32],
}
impl LeaseAuthority<'_> {
    pub const SCHEMA: &'static str = "proof.operator.lease-authority/v1";
    pub fn verifies_lease_token_digest(&self, expected: ControlDigest) -> bool {
        let actual = control_digest("Proof-Operator-Lease-Token-v1", self.lease_token);
        constant_time_eq_32(actual.as_bytes(), expected.as_bytes())
    }

    pub(super) fn lease_token_digest(&self) -> ControlDigest {
        control_digest("Proof-Operator-Lease-Token-v1", self.lease_token)
    }
}

impl DispatchTokenCustody {
    pub fn new(token: [u8; 32]) -> Self {
        Self {
            token: Zeroizing::new(token),
            begin: None,
            permit: None,
            monotonic_deadline: None,
            authorization_state: AtomicU8::new(0),
            effect_state: AtomicU8::new(0),
            settlement_state: AtomicU8::new(0),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn begin_request<'a>(
        &'a mut self,
        authority: LeaseAuthority<'a>,
        reservation_id: Uuid,
        intent: DispatchIntent,
        intent_digest: ControlDigest,
        replay: Option<ReplayClaimBinding>,
        replay_claim_token: Option<Uuid>,
        call_digest: ControlDigest,
    ) -> Result<BeginDispatchRequest<'a>, ExecutionError> {
        if self.begin.is_some()
            || self.permit.is_some()
            || !super::uuid_is_v7(reservation_id)
            || intent.validate().is_err()
            || super::control_digest_serialized("Proof-Operator-Dispatch-Intent-v1", &intent)
                .map_err(|error| ExecutionError::EvidenceFailed(error.to_string()))?
                != intent_digest
            || super::control_digest_serialized("Proof-Operator-Dispatch-Call-v1", &intent)
                .map_err(|error| ExecutionError::EvidenceFailed(error.to_string()))?
                != call_digest
            || replay.is_some() != replay_claim_token.is_some()
            || replay_claim_token.is_some_and(|id| !super::uuid_is_v7(id))
            || (intent.kind == BoundaryKind::Provider && replay.is_some())
        {
            return Err(ExecutionError::HandlerFailed(
                "dispatch begin binding is invalid".into(),
            ));
        }
        let binding = DispatchBeginBinding {
            workspace_id: authority.workspace_id,
            run_id: authority.run_id,
            lease_id: authority.lease_id,
            process_epoch_id: authority.process_epoch_id,
            fence_epoch: authority.fence_epoch,
            expected_control_revision: authority.expected_control_revision,
            reservation_id,
            intent: intent.clone(),
            replay: replay.clone(),
            replay_claim_token,
            call_digest,
        };
        self.begin = Some(binding);
        Ok(BeginDispatchRequest::from_custody(
            authority,
            reservation_id,
            DispatchTokenProof { token: &self.token },
            intent,
            intent_digest,
            replay,
            replay_claim_token,
            call_digest,
        ))
    }

    pub fn bind_permit(
        &mut self,
        result: &DispatchResult,
        environment: &dyn OperatorControlEnvironment,
    ) -> Result<(), ExecutionError> {
        let begin = self.begin.as_ref().ok_or_else(|| {
            ExecutionError::HandlerFailed("dispatch custody is not pending".into())
        })?;
        result
            .validate()
            .map_err(|_| ExecutionError::HandlerFailed("dispatch result is invalid".into()))?;
        if result.outcome != DispatchOutcome::DispatchAuthorized {
            return Err(ExecutionError::HandlerFailed(
                "dispatch result did not authorize an effect".into(),
            ));
        }
        let permit = result.permit.as_ref().ok_or_else(|| {
            ExecutionError::HandlerFailed("dispatch result omitted permit".into())
        })?;
        let replay_binding_digest = begin.replay.as_ref().map(|binding| binding.binding_digest);
        let token_digest =
            control_digest("Proof-Operator-Dispatch-Token-v1", self.token.as_slice());
        if permit.validate().is_err()
            || permit.run_id != begin.run_id
            || permit.reservation_id != begin.reservation_id
            || permit.lease_id != begin.lease_id
            || permit.process_epoch_id != begin.process_epoch_id
            || permit.fence_epoch != begin.fence_epoch
            || permit.expected_control_revision != begin.expected_control_revision
            || permit.intent_digest
                != super::control_digest_serialized(
                    "Proof-Operator-Dispatch-Intent-v1",
                    &begin.intent,
                )
                .map_err(|error| ExecutionError::EvidenceFailed(error.to_string()))?
            || permit.call_digest != begin.call_digest
            || permit.replay_binding_digest != replay_binding_digest
            || result.control_revision
                != begin
                    .expected_control_revision
                    .checked_add(1)
                    .ok_or_else(|| {
                        ExecutionError::HandlerFailed("dispatch control revision overflow".into())
                    })?
            || !constant_time_eq_32(
                permit.dispatch_token_digest.as_bytes(),
                token_digest.as_bytes(),
            )
        {
            return Err(ExecutionError::HandlerFailed(
                "dispatch permit did not bind custody".into(),
            ));
        }
        let utc_now = environment
            .trusted_utc_now()
            .map_err(|error| ExecutionError::HandlerFailed(error.to_string()))?;
        let monotonic_now = environment
            .monotonic_millis()
            .map_err(|error| ExecutionError::HandlerFailed(error.to_string()))?;
        let remaining = permit.budget_deadline_at.signed_duration_since(utc_now);
        let nanos = remaining.num_nanoseconds().ok_or_else(|| {
            ExecutionError::HandlerFailed("dispatch deadline is out of range".into())
        })?;
        if nanos <= 0 {
            return Err(ExecutionError::HandlerFailed(
                "dispatch permit expired before binding".into(),
            ));
        }
        let remaining_millis = u64::try_from((nanos + 999_999) / 1_000_000).map_err(|_| {
            ExecutionError::HandlerFailed("dispatch deadline is out of range".into())
        })?;
        self.monotonic_deadline =
            Some(monotonic_now.checked_add(remaining_millis).ok_or_else(|| {
                ExecutionError::HandlerFailed("dispatch deadline overflow".into())
            })?);
        self.permit = Some(permit.clone());
        Ok(())
    }

    pub fn authorization<'a>(
        &'a mut self,
        lease_liveness: &'a AtomicBool,
    ) -> Result<DispatchAuthorization<'a>, ExecutionError> {
        if self.permit.is_none()
            || self.monotonic_deadline.is_none()
            || self
                .authorization_state
                .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
            || self.effect_state.load(Ordering::Acquire) != 0
        {
            return Err(ExecutionError::HandlerFailed(
                "dispatch authority is unavailable".into(),
            ));
        }
        Ok(DispatchAuthorization {
            custody: self,
            lease_liveness,
        })
    }

    pub fn into_commit_request(
        self,
        authority: LeaseAuthority<'_>,
        prepared: super::PreparedExecutionBinding,
    ) -> Result<RuntimeCommitRequest<'_>, DispatchSettlementConversionError> {
        if let Err(error) = self.consume_settlement(true, &authority) {
            return Err(DispatchSettlementConversionError {
                error,
                custody: self,
            });
        }
        Ok(RuntimeCommitRequest::from_custody(
            authority, self, prepared,
        ))
    }

    pub fn into_failure_request(
        self,
        authority: LeaseAuthority<'_>,
        failure: RuntimeFailureBody,
        error_digest: ControlDigest,
    ) -> Result<RuntimeFailureRequest<'_>, DispatchSettlementConversionError> {
        let failure_matches = self.permit.as_ref().is_some_and(|permit| {
            failure.schema == "proof.operator.runtime-failure-body/v1"
                && failure.reservation_id == permit.reservation_id
                && failure.permit_id == permit.permit_id
                && failure.classification
                    == super::RuntimeFailureClassification::AmbiguousForfeitRequired
                && failure.intent_digest == permit.intent_digest
                && failure.call_digest == permit.call_digest
                && super::control_digest_serialized("Proof-Operator-Runtime-Failure-v1", &failure)
                    .is_ok_and(|actual| actual == error_digest)
        });
        if !failure_matches {
            return Err(DispatchSettlementConversionError {
                error: ExecutionError::HandlerFailed(
                    "runtime failure does not match dispatch custody".into(),
                ),
                custody: self,
            });
        }
        if let Err(error) = self.consume_settlement(false, &authority) {
            return Err(DispatchSettlementConversionError {
                error,
                custody: self,
            });
        }
        Ok(RuntimeFailureRequest::from_custody(
            authority,
            self,
            failure,
            error_digest,
        ))
    }

    pub(crate) fn prepared_matches_dispatch(
        &self,
        prepared: &super::PreparedExecutionBinding,
    ) -> bool {
        let Some(permit) = self.permit.as_ref() else {
            return false;
        };
        let Some(begin) = self.begin.as_ref() else {
            return false;
        };
        prepared.validate().is_ok()
            && prepared.replay_binding_digest == permit.replay_binding_digest
            && prepared.result.proof.operation == begin.intent.operation
            && prepared.result.usage.boundary_kind == begin.intent.kind
            && prepared.result.usage.adapter == begin.intent.adapter
            && prepared.result.usage.model.as_deref() == begin.intent.model.as_deref()
            && prepared.result.usage.steps <= begin.intent.ceiling.steps
            && prepared.result.usage.tokens <= begin.intent.ceiling.tokens
            && prepared.result.usage.cost_microusd <= begin.intent.ceiling.cost_microusd
            && prepared.result.usage.tool_dispatches <= begin.intent.ceiling.tool_dispatches
    }

    fn consume_settlement(
        &self,
        commit: bool,
        authority: &LeaseAuthority<'_>,
    ) -> Result<(), ExecutionError> {
        let permit = self
            .permit
            .as_ref()
            .ok_or_else(|| ExecutionError::HandlerFailed("dispatch custody is not bound".into()))?;
        if commit && self.effect_state.load(Ordering::Acquire) != 1
            || authority.run_id != permit.run_id
            || authority.lease_id != permit.lease_id
            || authority.process_epoch_id != permit.process_epoch_id
            || authority.fence_epoch != permit.fence_epoch
            || authority.expected_control_revision
                != permit
                    .expected_control_revision
                    .checked_add(1)
                    .ok_or_else(|| {
                        ExecutionError::HandlerFailed("dispatch control revision overflow".into())
                    })?
            || self
                .settlement_state
                .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return Err(ExecutionError::HandlerFailed(
                "dispatch settlement custody is invalid".into(),
            ));
        }
        Ok(())
    }

    pub(crate) fn permit(&self) -> Option<&DispatchPermit> {
        self.permit.as_ref()
    }
    pub(crate) fn verifies_dispatch_token_digest(&self, expected: ControlDigest) -> bool {
        let actual = control_digest("Proof-Operator-Dispatch-Token-v1", self.token.as_slice());
        constant_time_eq_32(actual.as_bytes(), expected.as_bytes())
    }

    pub(super) fn dispatch_token_digest(&self) -> ControlDigest {
        control_digest("Proof-Operator-Dispatch-Token-v1", self.token.as_slice())
    }

    pub(super) fn intent_ceiling(&self) -> super::BudgetAmounts {
        self.begin
            .as_ref()
            .expect("settlement custody requires a begin binding")
            .intent
            .ceiling
    }
}

pub struct DispatchTokenProof<'a> {
    token: &'a [u8; 32],
}
impl DispatchTokenProof<'_> {
    pub(super) fn digest(&self) -> ControlDigest {
        control_digest("Proof-Operator-Dispatch-Token-v1", self.token)
    }

    pub fn verifies_digest(&self, expected: ControlDigest) -> bool {
        let actual = self.digest();
        constant_time_eq_32(actual.as_bytes(), expected.as_bytes())
    }
}

pub struct DispatchAuthorization<'a> {
    custody: &'a mut DispatchTokenCustody,
    lease_liveness: &'a AtomicBool,
}

impl DispatchAuthorization<'_> {
    pub fn permit(&self) -> &DispatchPermit {
        self.custody
            .permit
            .as_ref()
            .expect("authorization requires a bound permit")
    }
    pub fn replay_binding(&self) -> Option<&ReplayClaimBinding> {
        self.custody
            .begin
            .as_ref()
            .and_then(|begin| begin.replay.as_ref())
    }
    pub fn replay_claim_token(&self) -> Option<Uuid> {
        self.custody
            .begin
            .as_ref()
            .and_then(|begin| begin.replay_claim_token)
    }
    pub fn intent(&self) -> &DispatchIntent {
        &self
            .custody
            .begin
            .as_ref()
            .expect("authorization requires a begin binding")
            .intent
    }
    pub(crate) fn workspace_id(&self) -> Uuid {
        self.custody
            .begin
            .as_ref()
            .expect("authorization requires a begin binding")
            .workspace_id
    }
    pub fn consume_effect(
        self,
        trusted_now: chrono::DateTime<chrono::Utc>,
        monotonic_now: u64,
    ) -> Result<(), ExecutionError> {
        let permit = self
            .custody
            .permit
            .as_ref()
            .expect("authorization requires a bound permit");
        let monotonic_deadline = self
            .custody
            .monotonic_deadline
            .expect("authorization requires a deadline");
        if !self.lease_liveness.load(Ordering::Acquire)
            || trusted_now >= permit.budget_deadline_at
            || monotonic_now >= monotonic_deadline
        {
            return Err(ExecutionError::HandlerFailed(
                "dispatch authority expired before effect".into(),
            ));
        }
        self.custody
            .effect_state
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|_| {
                ExecutionError::HandlerFailed("dispatch authority already consumed".into())
            })
    }
}

fn validate_lease_claim_binding(claim: &LeaseClaimBinding) -> Result<(), ExecutionError> {
    if ![
        claim.workspace_id,
        claim.run_id,
        claim.lease_id,
        claim.owner_instance_id,
        claim.process_epoch_id,
    ]
    .into_iter()
    .all(super::uuid_is_v7)
        || claim.expected_fence_epoch > super::MAX_SAFE_INTEGER
        || claim.expected_control_revision > super::MAX_SAFE_INTEGER
    {
        return Err(ExecutionError::HandlerFailed(
            "lease claim binding is invalid".into(),
        ));
    }
    Ok(())
}

fn validate_bound_lease(
    claim: &LeaseClaimBinding,
    lease: &super::RunLease,
    control_revision: u64,
    token: &[u8; 32],
) -> Result<BoundLease, ExecutionError> {
    let token_digest = control_digest("Proof-Operator-Lease-Token-v1", token);
    if lease.validate().is_err()
        || lease.state != super::RunLeaseState::Active
        || lease.workspace_id != claim.workspace_id
        || lease.run_id != claim.run_id
        || lease.lease_id != claim.lease_id
        || lease.owner_instance_id != claim.owner_instance_id
        || lease.process_epoch_id != claim.process_epoch_id
        || lease.fence_epoch
            != claim
                .expected_fence_epoch
                .checked_add(1)
                .ok_or_else(|| ExecutionError::HandlerFailed("lease fence overflow".into()))?
        || control_revision
            != claim
                .expected_control_revision
                .checked_add(1)
                .ok_or_else(|| {
                    ExecutionError::HandlerFailed("lease control revision overflow".into())
                })?
        || !constant_time_eq_32(lease.lease_token_digest.as_bytes(), token_digest.as_bytes())
        || lease.acquired_at != lease.renewed_at
        || lease.expires_at.signed_duration_since(lease.renewed_at) != chrono::Duration::seconds(30)
    {
        return Err(ExecutionError::HandlerFailed(
            "lease result did not bind custody".into(),
        ));
    }
    Ok(BoundLease {
        workspace_id: lease.workspace_id,
        run_id: lease.run_id,
        lease_id: lease.lease_id,
        owner_instance_id: lease.owner_instance_id,
        process_epoch_id: lease.process_epoch_id,
        fence_epoch: lease.fence_epoch,
    })
}

pub struct GovernedExecutionPlan<'a> {
    pub authorization: DispatchAuthorization<'a>,
    pub intent: DispatchIntent,
    pub run_before: AgentRun,
    pub step_before: AgentRunStep,
    pub checkpoint_tail: Option<AgentCheckpointTail>,
    pub replay_claim: Option<ExecutionReplayClaim>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedApprovalBundle {
    pub request: SignedApprovalRequest,
    pub binding: ApprovalBinding,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreparedReplayTransition {
    None,
    Complete(ExecutionReplayClaim),
}

impl PreparedReplayTransition {
    pub fn claim(&self) -> Option<&ExecutionReplayClaim> {
        match self {
            Self::None => None,
            Self::Complete(claim) => Some(claim),
        }
    }
}

impl Serialize for PreparedReplayTransition {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::None => serializer.serialize_none(),
            Self::Complete(claim) => {
                let mut state = serializer.serialize_struct("PreparedReplayCompletion", 2)?;
                state.serialize_field("schema", "proof.operator.prepared-replay-completion/v1")?;
                state.serialize_field("claim", claim)?;
                state.end()
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedUsage {
    schema: String,
    boundary_kind: BoundaryKind,
    boundary_calls: u64,
    adapter: String,
    model: Option<String>,
    steps: u64,
    tokens: u64,
    cost_microusd: u64,
    tool_dispatches: u64,
    input_digest: ContentDigest,
    output_digest: ContentDigest,
}

impl PreparedUsage {
    pub const SCHEMA: &'static str = "proof.operator.prepared-usage-body/v1";
    pub fn boundary_kind(&self) -> BoundaryKind {
        self.boundary_kind
    }
    pub fn boundary_calls(&self) -> u64 {
        self.boundary_calls
    }
    pub fn adapter(&self) -> &str {
        &self.adapter
    }
    pub fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }
    pub fn steps(&self) -> u64 {
        self.steps
    }
    pub fn tokens(&self) -> u64 {
        self.tokens
    }
    pub fn cost_microusd(&self) -> u64 {
        self.cost_microusd
    }
    pub fn tool_dispatches(&self) -> u64 {
        self.tool_dispatches
    }
    pub fn input_digest(&self) -> ContentDigest {
        self.input_digest
    }
    pub fn output_digest(&self) -> ContentDigest {
        self.output_digest
    }
    pub(crate) fn from_report(
        intent: &DispatchIntent,
        registered: &RegisteredGovernedAdapter,
        report: PreparedBoundaryUsage,
        input_digest: ContentDigest,
        output_digest: ContentDigest,
    ) -> Result<Self, ExecutionError> {
        if !registered.matches_intent(intent)
            || !registered.owns(&report)
            || report.boundary_kind != intent.kind
            || report.tokens > intent.ceiling.tokens
            || report.cost_microusd > intent.ceiling.cost_microusd
            || report.tool_dispatches > intent.ceiling.tool_dispatches
            || intent.ceiling.steps < 1
            || matches!(report.boundary_kind, BoundaryKind::Provider) && report.tool_dispatches != 0
            || matches!(report.boundary_kind, BoundaryKind::Tool)
                && (report.tokens != 0 || report.cost_microusd != 0 || report.tool_dispatches != 1)
        {
            return Err(ExecutionError::EvidenceFailed(
                "prepared boundary usage is invalid".into(),
            ));
        }
        Ok(Self {
            schema: Self::SCHEMA.into(),
            boundary_kind: report.boundary_kind,
            boundary_calls: 1,
            adapter: intent.adapter.clone(),
            model: intent.model.clone(),
            steps: 1,
            tokens: report.tokens,
            cost_microusd: report.cost_microusd,
            tool_dispatches: report.tool_dispatches,
            input_digest,
            output_digest,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PreparedGovernedExecution {
    output: Value,
    #[serde(serialize_with = "super::strict_uuid_v7::serialize")]
    execution_context_id: Uuid,
    context: ExecutionContext,
    proof: Proof,
    run_after: AgentRun,
    step_after: AgentRunStep,
    checkpoint: Option<AgentCheckpoint>,
    events: Vec<AgentRunEvent>,
    evaluation: Option<AgentRunEvaluation>,
    approval: Option<PreparedApprovalBundle>,
    handler_mutation: PreparedHandlerMutation,
    replay: PreparedReplayTransition,
    usage: PreparedUsage,
}

impl PreparedGovernedExecution {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        output: Value,
        execution_context_id: Uuid,
        context: ExecutionContext,
        proof: Proof,
        run_after: AgentRun,
        step_after: AgentRunStep,
        checkpoint: Option<AgentCheckpoint>,
        events: Vec<AgentRunEvent>,
        evaluation: Option<AgentRunEvaluation>,
        approval: Option<PreparedApprovalBundle>,
        replay: PreparedReplayTransition,
        usage: PreparedUsage,
    ) -> Self {
        Self {
            output,
            execution_context_id,
            context,
            proof,
            run_after,
            step_after,
            checkpoint,
            events,
            evaluation,
            approval,
            handler_mutation: PreparedHandlerMutation::NoEffect,
            replay,
            usage,
        }
    }
    pub fn output(&self) -> &Value {
        &self.output
    }
    pub fn execution_context_id(&self) -> Uuid {
        self.execution_context_id
    }
    pub fn context(&self) -> &ExecutionContext {
        &self.context
    }
    pub fn proof(&self) -> &Proof {
        &self.proof
    }
    pub fn run_after(&self) -> &AgentRun {
        &self.run_after
    }
    pub fn step_after(&self) -> &AgentRunStep {
        &self.step_after
    }
    pub fn checkpoint(&self) -> Option<&AgentCheckpoint> {
        self.checkpoint.as_ref()
    }
    pub fn events(&self) -> &[AgentRunEvent] {
        &self.events
    }
    pub fn evaluation(&self) -> Option<&AgentRunEvaluation> {
        self.evaluation.as_ref()
    }
    pub fn approval(&self) -> Option<&PreparedApprovalBundle> {
        self.approval.as_ref()
    }
    pub fn handler_mutation(&self) -> PreparedHandlerMutation {
        self.handler_mutation
    }
    pub fn replay(&self) -> &PreparedReplayTransition {
        &self.replay
    }
    pub fn usage(&self) -> &PreparedUsage {
        &self.usage
    }
    pub fn payload_digest(&self) -> Result<super::ControlDigest, crate::CanonicalizationError> {
        super::control_digest_serialized("Proof-Operator-Prepared-Execution-v1", self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Utc};
    use ed25519_dalek::SigningKey;
    use serde_json::json;
    use std::path::PathBuf;

    use crate::{
        canonicalize, canonicalize_serialized, digest, AgentRunMode, AgentRunStatus,
        AgentRunStepStatus, ArtifactKind, Keypair, PrincipalId, PrincipalKind,
    };

    fn id(suffix: u16) -> Uuid {
        Uuid::parse_str(&format!("01890f47-9bcd-7def-8123-456789ab{suffix:04x}"))
            .expect("fixed UUIDv7")
    }

    fn seal_lease(lease: &mut super::super::RunLease) {
        let mut value = serde_json::to_value(&*lease).unwrap();
        value.as_object_mut().unwrap().remove("lease_digest");
        lease.lease_digest =
            super::super::control_digest_serialized("Proof-Operator-Lease-v1", &value).unwrap();
    }

    fn fixture(replay: bool) -> PreparedGovernedExecution {
        let at: DateTime<Utc> = "2030-01-02T03:04:05Z".parse().unwrap();
        let actor = PrincipalId::new(id(1));
        let keypair = Keypair {
            principal_id: actor,
            kind: PrincipalKind::Agent,
            created_at: at,
            signing_key: SigningKey::from_bytes(&[3; 32]),
        };
        let input = json!({"value":"in"});
        let output = json!({"value":"out"});
        let input_digest = digest(ArtifactKind::OperationInput, &canonicalize(&input).unwrap());
        let output_digest = digest(
            ArtifactKind::OperationOutput,
            &canonicalize(&output).unwrap(),
        );
        let proof = Proof::new(
            id(2),
            actor,
            None,
            "test.echo::v1",
            input_digest,
            output_digest,
            at,
        )
        .sign(&keypair)
        .unwrap();
        let run_after = AgentRun {
            id: id(3),
            actor,
            agent_id: Some(id(4)),
            mode: AgentRunMode::OneShot,
            goal: "echo once".into(),
            status: AgentRunStatus::Succeeded,
            retry_count: 0,
            revision: 2,
            created_at: at,
            updated_at: at,
            completed_at: Some(at),
        };
        let step_after = AgentRunStep {
            id: id(5),
            run_id: run_after.id,
            ordinal: 0,
            attempt: 1,
            retry_of: None,
            operation: "test.echo".into(),
            version: "v1".into(),
            input_digest,
            status: AgentRunStepStatus::Succeeded,
            approval_request_id: None,
            output: Some(output.clone()),
            proof: Some(proof.clone()),
            error: None,
            revision: 2,
            created_at: at,
            updated_at: at,
            started_at: Some(at),
            completed_at: Some(at),
        };
        let intent = DispatchIntent {
            schema: DispatchIntent::SCHEMA.into(),
            kind: BoundaryKind::Provider,
            adapter: "synthetic".into(),
            model: Some("fixed-v1".into()),
            operation: "test.echo".into(),
            version: "v1".into(),
            argument_digest: control_digest("Proof-Operator-Dispatch-Argument-v1", b"input"),
            ceiling: super::super::BudgetAmounts {
                steps: 1,
                tokens: 9,
                duration_ms: 10,
                cost_microusd: 7,
                tool_dispatches: 0,
            },
        };
        let registration = GovernedAdapterRegistration::new(
            "test.echo",
            "v1",
            BoundaryKind::Provider,
            "synthetic",
            Some("fixed-v1".into()),
        )
        .unwrap();
        let (reporter, registered) = registration.mint();
        let report = reporter.provider_output(output.clone(), 9, 7).unwrap();
        let (_, _, boundary_usage) = report.into_parts();
        let usage = PreparedUsage::from_report(
            &intent,
            &registered,
            boundary_usage,
            input_digest,
            output_digest,
        )
        .unwrap();
        let replay = if replay {
            PreparedReplayTransition::Complete(ExecutionReplayClaim {
                key: crate::ExecutionReplayKey {
                    operation: "test.echo".into(),
                    version: "v1".into(),
                    idempotency_key: id(6),
                },
                input_digest,
                claim_token: id(7),
                claimed_by: actor,
                claimed_at: at,
            })
        } else {
            PreparedReplayTransition::None
        };
        PreparedGovernedExecution::new(
            output,
            id(8),
            ExecutionContext {
                actor,
                principal_kind: Some(PrincipalKind::Agent),
                delegation_id: None,
                delegation_chain: None,
                workspace_path: PathBuf::from("/workspace"),
                timestamp: at,
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
        )
    }

    #[test]
    fn golden_non_replay_serialization_and_digest() {
        let prepared = fixture(false);
        let value = serde_json::to_value(&prepared).unwrap();
        assert_eq!(value["replay"], Value::Null);
        assert_eq!(value["handler_mutation"], "no_effect");
        assert_eq!(value["checkpoint"], Value::Null);
        assert_eq!(value["approval"], Value::Null);
        assert_eq!(
            canonicalize_serialized(&prepared).unwrap().as_str(),
            canonicalize_serialized(&fixture(false)).unwrap().as_str()
        );
        assert_eq!(
            prepared.payload_digest().unwrap().to_string(),
            "blake3-256:8938361aa5c2c66910650f44b4285ebd84266aea4a84637751b1050af9d62d9d"
        );
    }

    #[test]
    fn golden_replay_serialization_and_digest() {
        let prepared = fixture(true);
        let value = serde_json::to_value(&prepared).unwrap();
        assert_eq!(
            value["replay"]["schema"],
            "proof.operator.prepared-replay-completion/v1"
        );
        assert_eq!(value["replay"]["claim"]["key"]["operation"], "test.echo");
        assert_eq!(
            prepared.payload_digest().unwrap().to_string(),
            "blake3-256:ee0650887e81d44ce6d35ef65df7913641b6064f63260b70e2b7d2e2a7514e4a"
        );
        let binding_digest = control_digest("Proof-Operator-Replay-Binding-v1", b"binding");
        let binding =
            super::super::PreparedExecutionBinding::from_prepared(&prepared, Some(binding_digest))
                .unwrap();
        assert_eq!(binding.payload_digest, prepared.payload_digest().unwrap());
        assert_eq!(
            binding.result_digest,
            super::super::control_digest_serialized(
                "Proof-Operator-Runtime-Result-v1",
                &binding.result
            )
            .unwrap()
        );
    }

    #[test]
    fn reporter_seal_rejects_cross_registration_and_wrong_branch() {
        let provider = GovernedAdapterRegistration::new(
            "test.echo",
            "v1",
            BoundaryKind::Provider,
            "synthetic",
            Some("fixed-v1".into()),
        )
        .unwrap();
        let tool = GovernedAdapterRegistration::new(
            "test.echo",
            "v1",
            BoundaryKind::Tool,
            "tool_adapter",
            None,
        )
        .unwrap();
        let (provider_reporter, provider_registration) = provider.mint();
        let (tool_reporter, tool_registration) = tool.mint();
        let wrong_provider_branch = match provider_reporter.tool_output(json!({})) {
            Err(error) => error,
            Ok(_) => panic!("wrong provider branch unexpectedly succeeded"),
        };
        let wrong_tool_branch = match tool_reporter.provider_output(json!({}), 1, 1) {
            Err(error) => error,
            Ok(_) => panic!("wrong tool branch unexpectedly succeeded"),
        };
        assert!(matches!(
            &wrong_provider_branch,
            ExecutionError::EvidenceFailed(_)
        ));
        assert!(matches!(
            &wrong_tool_branch,
            ExecutionError::EvidenceFailed(_)
        ));
        assert_eq!(
            super::super::governed_runtime_failure_code(&wrong_provider_branch),
            super::super::RuntimeFailureCode::ResultInvalid
        );
        assert_eq!(
            super::super::governed_runtime_failure_code(&wrong_tool_branch),
            super::super::RuntimeFailureCode::ResultInvalid
        );
        let missing = provider_reporter.missing_usage_error();
        assert!(matches!(&missing, ExecutionError::EvidenceFailed(_)));
        assert_eq!(
            super::super::governed_runtime_failure_code(&missing),
            super::super::RuntimeFailureCode::ResultInvalid
        );
        for message in [
            "handler report was unavailable",
            "handler result was rejected",
        ] {
            assert_eq!(
                super::super::governed_runtime_failure_code(&ExecutionError::HandlerFailed(
                    message.into()
                )),
                super::super::RuntimeFailureCode::HandlerFailed
            );
        }
        let report = provider_reporter
            .provider_output(json!({"value":"out"}), 1, 1)
            .unwrap();
        let (_, _, usage) = report.into_parts();
        assert!(provider_registration.owns(&usage));
        assert!(!tool_registration.owns(&usage));
        let overage = provider_reporter
            .provider_output(json!({"value":"out"}), 2, 1)
            .unwrap();
        let (_, _, overage) = overage.into_parts();
        let intent = DispatchIntent {
            schema: DispatchIntent::SCHEMA.into(),
            kind: BoundaryKind::Provider,
            adapter: "synthetic".into(),
            model: Some("fixed-v1".into()),
            operation: "test.echo".into(),
            version: "v1".into(),
            argument_digest: ControlDigest::from_bytes([1; 32]),
            ceiling: super::super::BudgetAmounts {
                steps: 1,
                tokens: 1,
                duration_ms: 1,
                cost_microusd: 1,
                tool_dispatches: 0,
            },
        };
        let overage = PreparedUsage::from_report(
            &intent,
            &provider_registration,
            overage,
            ContentDigest::from_bytes([2; 32]),
            ContentDigest::from_bytes([3; 32]),
        );
        let overage = match overage {
            Err(error) => error,
            Ok(_) => panic!("over-ceiling usage unexpectedly succeeded"),
        };
        assert!(matches!(&overage, ExecutionError::EvidenceFailed(_)));
        assert_eq!(
            super::super::governed_runtime_failure_code(&overage),
            super::super::RuntimeFailureCode::ResultInvalid
        );
        assert!(GovernedAdapterRegistration::new(
            "test.echo",
            "v01",
            BoundaryKind::Provider,
            "synthetic",
            Some("fixed-v1".into()),
        )
        .is_err());
        assert!(GovernedAdapterRegistration::new(
            "test.echo",
            "v1",
            BoundaryKind::Tool,
            "synthetic",
            Some("forbidden-model".into()),
        )
        .is_err());
    }

    #[test]
    fn reclaim_custody_binds_only_the_exact_successful_fence_transition() {
        let at: DateTime<Utc> = "2030-01-02T03:04:05Z".parse().unwrap();
        let token = [7_u8; 32];
        let mut custody = LeaseTokenCustody::new(token);
        {
            let request = custody
                .reclaim_request(
                    id(1),
                    id(2),
                    id(3),
                    1,
                    4,
                    id(4),
                    id(5),
                    id(6),
                    id(7),
                    9,
                    ContentDigest::from_bytes([8; 32]),
                )
                .unwrap();
            let expected = control_digest("Proof-Operator-Lease-Token-v1", &token);
            assert_eq!(request.new_lease_token_digest(), expected);
            assert!(request.verifies_new_lease_token_digest(expected));
            assert_ne!(
                request.new_lease_token_digest(),
                control_digest("Proof-Operator-Lease-Token-v2", &token)
            );
        }
        let mut lease = super::super::RunLease {
            schema: super::super::RunLease::SCHEMA.into(),
            run_id: id(2),
            workspace_id: id(1),
            lease_id: id(4),
            owner_instance_id: id(5),
            process_epoch_id: id(6),
            lease_token_digest: control_digest("Proof-Operator-Lease-Token-v1", &token),
            fence_epoch: 2,
            revision: 0,
            state: super::super::RunLeaseState::Active,
            acquired_at: at,
            renewed_at: at,
            expires_at: at + chrono::Duration::seconds(30),
            released_at: None,
            lease_digest: ControlDigest::from_bytes([0; 32]),
        };
        seal_lease(&mut lease);
        let result = ReclaimResult {
            schema: ReclaimResult::SCHEMA.into(),
            outcome: ReclaimOutcome::IdleReclaimed,
            lease: Some(lease.clone()),
            directive: None,
            control_revision: 5,
        };
        let mut wrong = result.clone();
        wrong.control_revision = 6;
        assert!(custody.bind_reclaim_result(&wrong).is_err());
        custody.bind_reclaim_result(&result).unwrap();
        let authority = custody.authority(5).unwrap();
        assert_eq!(authority.lease_id, id(4));
        assert_eq!(authority.fence_epoch, 2);
    }

    #[test]
    fn custody_binds_exact_lease_fence_and_permit_then_consumes_failure_path() {
        let at: DateTime<Utc> = "2030-01-02T03:04:05Z".parse().unwrap();
        let lease_token = [8; 32];
        let recorder = super::super::RecordingOperatorControlStore::default();
        let mut lease_custody = LeaseTokenCustody::new(lease_token);
        {
            let proof = lease_custody
                .claim_request(id(1), id(2), id(3), id(4), id(5), 0, 0)
                .unwrap();
            let expected = control_digest("Proof-Operator-Lease-Token-v1", &lease_token);
            assert_eq!(proof.lease_token_digest(), expected);
            assert!(proof.verifies_lease_token_digest(expected));
            assert_ne!(
                proof.lease_token_digest(),
                control_digest("Proof-Operator-Lease-Token-v2", &lease_token)
            );
            recorder.inject_error(
                super::super::OperatorStoreBoundary::ClaimRunLease,
                super::super::OperatorStoreError::Conflict,
            );
            assert_eq!(
                super::super::OperatorRuntimeStore::claim_run_lease(&recorder, proof),
                Err(super::super::OperatorStoreError::Conflict)
            );
        }
        assert!(lease_custody
            .claim_request(id(1), id(2), id(3), id(4), id(5), 0, 0)
            .is_err());
        let mut lease = super::super::RunLease {
            schema: super::super::RunLease::SCHEMA.into(),
            run_id: id(2),
            workspace_id: id(1),
            lease_id: id(3),
            owner_instance_id: id(4),
            process_epoch_id: id(5),
            lease_token_digest: control_digest("Proof-Operator-Lease-Token-v1", &lease_token),
            fence_epoch: 2,
            revision: 0,
            state: super::super::RunLeaseState::Active,
            acquired_at: at,
            renewed_at: at,
            expires_at: at + chrono::Duration::seconds(30),
            released_at: None,
            lease_digest: ControlDigest::from_bytes([0; 32]),
        };
        seal_lease(&mut lease);
        let mut result = LeaseMutationResult {
            schema: "proof.operator.lease-mutation-result/v1".into(),
            outcome: LeaseMutationOutcome::Acquired,
            lease,
            control_revision: 1,
        };
        assert!(lease_custody.bind_claim_result(&result).is_err());
        result.lease.fence_epoch = 1;
        seal_lease(&mut result.lease);
        lease_custody.bind_claim_result(&result).unwrap();
        assert_eq!(lease_custody.authority(1).unwrap().fence_epoch, 1);

        let input = json!({"value":"in"});
        let canonical = canonicalize(&input).unwrap();
        let intent = DispatchIntent {
            schema: DispatchIntent::SCHEMA.into(),
            kind: BoundaryKind::Provider,
            adapter: "synthetic".into(),
            model: Some("fixed-v1".into()),
            operation: "test.echo".into(),
            version: "v1".into(),
            argument_digest: control_digest(
                "Proof-Operator-Dispatch-Argument-v1",
                canonical.as_bytes(),
            ),
            ceiling: super::super::BudgetAmounts {
                steps: 1,
                tokens: 10,
                duration_ms: 10,
                cost_microusd: 10,
                tool_dispatches: 0,
            },
        };
        let intent_digest =
            super::super::control_digest_serialized("Proof-Operator-Dispatch-Intent-v1", &intent)
                .unwrap();
        let call_digest =
            super::super::control_digest_serialized("Proof-Operator-Dispatch-Call-v1", &intent)
                .unwrap();
        let expected_intent_ceiling = intent.ceiling;
        recorder.inject_error(
            super::super::OperatorStoreBoundary::ReserveAggregateBudget,
            super::super::OperatorStoreError::NotActionable,
        );
        assert_eq!(
            super::super::OperatorRuntimeStore::reserve_aggregate_budget(
                &recorder,
                super::super::BudgetReserveRequest {
                    schema: "proof.operator.budget-reserve-request/v1".into(),
                    authority: lease_custody.authority(1).unwrap(),
                    reservation_id: id(6),
                    idempotency_key: id(60),
                    intent: intent.clone(),
                    intent_digest,
                    replay: None,
                    recovery: None,
                },
            ),
            Err(super::super::OperatorStoreError::NotActionable)
        );
        let dispatch_token = [9; 32];
        let mut dispatch = DispatchTokenCustody::new(dispatch_token);
        {
            let begin = dispatch
                .begin_request(
                    lease_custody.authority(1).unwrap(),
                    id(6),
                    intent,
                    intent_digest,
                    None,
                    None,
                    call_digest,
                )
                .unwrap();
            let expected = control_digest("Proof-Operator-Dispatch-Token-v1", &dispatch_token);
            assert_eq!(begin.dispatch_token_digest(), expected);
            assert!(begin.verifies_dispatch_token_digest(expected));
            assert_ne!(
                begin.dispatch_token_digest(),
                control_digest("Proof-Operator-Dispatch-Token-v2", &dispatch_token)
            );
            recorder.inject_error(
                super::super::OperatorStoreBoundary::BeginDispatch,
                super::super::OperatorStoreError::NotActionable,
            );
            assert_eq!(
                super::super::OperatorRuntimeStore::begin_dispatch(&recorder, begin),
                Err(super::super::OperatorStoreError::NotActionable)
            );
        }
        let permit = DispatchPermit {
            schema: DispatchPermit::SCHEMA.into(),
            permit_id: id(7),
            run_id: id(2),
            reservation_id: id(6),
            lease_id: id(3),
            process_epoch_id: id(5),
            fence_epoch: 1,
            expected_control_revision: 1,
            intent_digest,
            replay_binding_digest: None,
            dispatch_token_digest: control_digest(
                "Proof-Operator-Dispatch-Token-v1",
                &dispatch_token,
            ),
            call_digest,
            authorized_at: at,
            budget_deadline_at: at + chrono::Duration::minutes(1),
        };
        let environment = super::super::RecordingOperatorControlEnvironment::new(at, [1; 32]);
        let mut wrong = permit.clone();
        wrong.fence_epoch = 2;
        assert!(dispatch
            .bind_permit(
                &DispatchResult {
                    schema: DispatchResult::SCHEMA.into(),
                    outcome: DispatchOutcome::DispatchAuthorized,
                    permit: Some(wrong),
                    replay_completion: None,
                    control_revision: 2,
                },
                &environment,
            )
            .is_err());
        dispatch
            .bind_permit(
                &DispatchResult {
                    schema: DispatchResult::SCHEMA.into(),
                    outcome: DispatchOutcome::DispatchAuthorized,
                    permit: Some(permit.clone()),
                    replay_completion: None,
                    control_revision: 2,
                },
                &environment,
            )
            .unwrap();
        let live = AtomicBool::new(true);
        dispatch
            .authorization(&live)
            .unwrap()
            .consume_effect(at, 0)
            .unwrap();
        assert!(dispatch.authorization(&live).is_err());
        let failure = RuntimeFailureBody {
            schema: "proof.operator.runtime-failure-body/v1".into(),
            reservation_id: permit.reservation_id,
            permit_id: permit.permit_id,
            classification: super::super::RuntimeFailureClassification::AmbiguousForfeitRequired,
            failure_code: super::super::RuntimeFailureCode::ResultInvalid,
            intent_digest,
            call_digest,
        };
        let error_digest =
            super::super::control_digest_serialized("Proof-Operator-Runtime-Failure-v1", &failure)
                .unwrap();
        let mut malformed = failure.clone();
        malformed.permit_id = id(99);
        let malformed_digest = super::super::control_digest_serialized(
            "Proof-Operator-Runtime-Failure-v1",
            &malformed,
        )
        .unwrap();
        let conversion = match dispatch.into_failure_request(
            lease_custody.authority(2).unwrap(),
            malformed,
            malformed_digest,
        ) {
            Err(error) => error,
            Ok(_) => panic!("malformed failure request unexpectedly converted"),
        };
        assert!(matches!(
            conversion.error(),
            ExecutionError::HandlerFailed(message)
                if message == "runtime failure does not match dispatch custody"
        ));
        assert_eq!(
            format!("{conversion:?}"),
            "DispatchSettlementConversionError { error: HandlerFailed(\"runtime failure does not match dispatch custody\"), .. }"
        );
        let dispatch = conversion.into_custody();
        let request = dispatch
            .into_failure_request(lease_custody.authority(2).unwrap(), failure, error_digest)
            .unwrap();
        assert!(request.verifies_dispatch_token_digest(permit.dispatch_token_digest));
        recorder.inject_error(
            super::super::OperatorStoreBoundary::SettleRuntimeFailure,
            super::super::OperatorStoreError::Invalid,
        );
        assert_eq!(
            super::super::OperatorRuntimeStore::settle_runtime_failure(&recorder, request),
            Err(super::super::OperatorStoreError::Invalid)
        );
        let mut reclaim_custody = LeaseTokenCustody::new([10; 32]);
        let reclaim = reclaim_custody
            .reclaim_request(
                id(1),
                id(2),
                id(80),
                2,
                2,
                id(81),
                id(4),
                id(82),
                id(83),
                10,
                ContentDigest::from_bytes([11; 32]),
            )
            .unwrap();
        recorder.inject_error(
            super::super::OperatorStoreBoundary::ReclaimRun,
            super::super::OperatorStoreError::StaleFence,
        );
        assert_eq!(
            super::super::OperatorRuntimeStore::reclaim_run(&recorder, reclaim),
            Err(super::super::OperatorStoreError::StaleFence)
        );
        let requests = recorder.requests();
        assert_eq!(requests.len(), 5);
        assert!(matches!(
            &requests[0],
            super::super::RecordingOperatorRequest::Claim {
                workspace_id,
                run_id,
                lease_id,
                owner_instance_id,
                process_epoch_id,
                expected_fence_epoch: 0,
                expected_control_revision: 0,
                lease_token_digest,
            } if *workspace_id == id(1)
                && *run_id == id(2)
                && *lease_id == id(3)
                && *owner_instance_id == id(4)
                && *process_epoch_id == id(5)
                && *lease_token_digest
                    == control_digest("Proof-Operator-Lease-Token-v1", &lease_token)
        ));
        assert!(matches!(
            &requests[1],
            super::super::RecordingOperatorRequest::Reserve {
                authority,
                reservation_id,
                idempotency_key,
                intent_digest: recorded_intent,
                intent_ceiling,
            } if authority.workspace_id == id(1)
                && authority.run_id == id(2)
                && authority.lease_id == id(3)
                && authority.fence_epoch == 1
                && authority.expected_control_revision == 1
                && authority.lease_token_digest
                    == control_digest("Proof-Operator-Lease-Token-v1", &lease_token)
                && *reservation_id == id(6)
                && *idempotency_key == id(60)
                && *recorded_intent == intent_digest
                && *intent_ceiling == expected_intent_ceiling
        ));
        assert!(matches!(
            &requests[2],
            super::super::RecordingOperatorRequest::Begin {
                authority,
                reservation_id,
                dispatch_token_digest,
                intent_digest: recorded_intent,
                call_digest: recorded_call,
                replay_claim_token: None,
                intent_ceiling,
            } if authority.expected_control_revision == 1
                && *reservation_id == id(6)
                && *dispatch_token_digest == permit.dispatch_token_digest
                && *recorded_intent == intent_digest
                && *recorded_call == call_digest
                && *intent_ceiling == expected_intent_ceiling
        ));
        assert!(matches!(
            &requests[3],
            super::super::RecordingOperatorRequest::Failure {
                authority,
                reservation_id,
                permit_id,
                dispatch_token_digest,
                intent_ceiling,
                failure_code: super::super::RuntimeFailureCode::ResultInvalid,
                error_digest: recorded_error,
            } if authority.expected_control_revision == 2
                && *reservation_id == id(6)
                && *permit_id == id(7)
                && *dispatch_token_digest == permit.dispatch_token_digest
                && *intent_ceiling == expected_intent_ceiling
                && *recorded_error == error_digest
        ));
        assert!(matches!(
            &requests[4],
            super::super::RecordingOperatorRequest::Reclaim {
                workspace_id,
                run_id,
                expired_lease_id,
                new_lease_id,
                owner_instance_id,
                new_process_epoch_id,
                expected_fence_epoch: 2,
                expected_control_revision: 2,
                new_lease_token_digest,
                checkpoint_id,
                checkpoint_sequence: 10,
                checkpoint_digest,
            } if *workspace_id == id(1)
                && *run_id == id(2)
                && *expired_lease_id == id(80)
                && *new_lease_id == id(81)
                && *owner_instance_id == id(4)
                && *new_process_epoch_id == id(82)
                && *new_lease_token_digest
                    == control_digest("Proof-Operator-Lease-Token-v1", &[10; 32])
                && *checkpoint_id == id(83)
                && *checkpoint_digest == ContentDigest::from_bytes([11; 32])
        ));
        let debug = format!("{requests:?}");
        assert!(!debug.contains("lease_token:"));
        assert!(!debug.contains("new_lease_token:"));
        assert!(!debug.contains("dispatch_token:"));
        assert!(!debug.contains("[9, 9, 9, 9, 9, 9, 9, 9"));
        let release = lease_custody.into_release_request(2).unwrap();
        assert_eq!(release.authority().unwrap().lease_id, id(3));
    }
}
