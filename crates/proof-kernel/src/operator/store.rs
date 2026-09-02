use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};
use thiserror::Error;
use uuid::Uuid;

use super::{
    ApprovalBinding, AuditEvent, AuditEventKind, BudgetAccountState, BudgetAmounts,
    BudgetReservation, Capability, CapabilitySet, CommandEnvelope, CommandKind, CommandOutcome,
    CommandReceipt, ControlDigest, ControlTransitionOutcome, DecisionOutcome, DispatchIntent,
    DispatchPermit, DispatchTokenCustody, DispatchTokenProof, LeaseAuthority, LeaseTokenCustody,
    LeaseTokenProof, OperatorCommand, OperatorWorkspace, PendingConsequence,
    PreparedGovernedExecution, ProofReference, RecoveryDirective, ReplayClaimBinding, ReviewField,
    RunControl, RunLease, RunProjection, SessionAuthorityBinding,
};
use crate::{
    canonicalize, digest, AgentRunMode, AgentRunStatus, AgentRunStepStatus, ArtifactKind,
    ContentDigest, ExecutionReplayClaim, ExecutionReplayClaimResult, Proof,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Error)]
#[serde(rename_all = "snake_case")]
pub enum OperatorStoreError {
    #[error("conflict")]
    Conflict,
    #[error("corrupt")]
    Corrupt,
    #[error("invalid")]
    Invalid,
    #[error("not actionable")]
    NotActionable,
    #[error("not found")]
    NotFound,
    #[error("signer failed")]
    SignerFailed,
    #[error("stale fence")]
    StaleFence,
    #[error("stale revision")]
    StaleRevision,
    #[error("unavailable")]
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Error)]
#[serde(rename_all = "snake_case")]
pub enum OperatorProvisioningError {
    #[error("catalog mismatch")]
    CatalogMismatch,
    #[error("close failed")]
    CloseFailed,
    #[error("environment unavailable")]
    EnvironmentUnavailable,
    #[error("invalid arguments")]
    InvalidArguments,
    #[error("lock unavailable")]
    LockUnavailable,
    #[error("migration failed")]
    MigrationFailed,
    #[error("movement detected")]
    MovementDetected,
    #[error("policy mismatch")]
    PolicyMismatch,
    #[error("schema mismatch")]
    SchemaMismatch,
    #[error("storage unavailable")]
    StorageUnavailable,
    #[error("unsafe provision")]
    UnsafeProvision,
    #[error("unsafe workspace")]
    UnsafeWorkspace,
    #[error("unsupported platform")]
    UnsupportedPlatform,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorProvisioningDocument {
    pub schema: String,
    #[serde(with = "super::strict_uuid_v7")]
    pub agent_id: Uuid,
    pub agent_public_key_fingerprint: ControlDigest,
    #[serde(with = "super::strict_uuid_v7")]
    pub human_id: Uuid,
    pub human_public_key_fingerprint: ControlDigest,
    pub capabilities: CapabilitySet,
    pub budget_limits: BudgetAmounts,
    #[serde(with = "super::strict_utc")]
    pub budget_deadline_at: DateTime<Utc>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProvisionOutcome {
    Created,
    ExactExisting,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProvisionOperatorWorkspaceResult {
    pub schema: String,
    pub outcome: ProvisionOutcome,
    #[serde(with = "super::strict_uuid_v7")]
    pub workspace_id: Uuid,
    #[serde(with = "super::strict_safe_integer")]
    pub schema_version: u64,
    pub workspace_binding_digest: ControlDigest,
    pub schema_catalog_digest: ControlDigest,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InitializeWorkspaceRequest {
    pub schema: String,
    pub provision: OperatorProvisioningDocument,
    pub schema_catalog: super::SchemaCatalogBinding,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlAuthorityEventKind {
    ControlShutdown,
    SessionChallengeIssued,
    SessionExpired,
    SessionIssued,
    SessionReplaced,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlAuditAppendRequest {
    pub schema: String,
    #[serde(with = "super::strict_uuid_v7")]
    pub workspace_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub server_instance_id: Uuid,
    pub kind: ControlAuthorityEventKind,
    #[serde(with = "super::strict_optional_uuid_v7")]
    pub human_id: Option<Uuid>,
    #[serde(with = "super::strict_optional_uuid_v7")]
    pub session_id: Option<Uuid>,
    #[serde(with = "super::strict_optional_uuid_v7")]
    pub challenge_id: Option<Uuid>,
    pub challenge_digest: Option<ControlDigest>,
    pub session_authority_digest: Option<ControlDigest>,
    #[serde(with = "super::strict_optional_uuid_v7")]
    pub related_session_id: Option<Uuid>,
    #[serde(with = "super::strict_optional_safe_integer")]
    pub auth_epoch: Option<u64>,
    #[serde(with = "super::strict_optional_safe_integer")]
    pub policy_revision: Option<u64>,
}
impl ControlAuditAppendRequest {
    pub const SCHEMA: &'static str = "proof.operator.control-audit-append-request/v1";

    pub fn validate(&self) -> Result<(), OperatorStoreError> {
        if self.schema != Self::SCHEMA
            || !super::uuid_is_v7(self.workspace_id)
            || !super::uuid_is_v7(self.server_instance_id)
            || [
                self.human_id,
                self.session_id,
                self.challenge_id,
                self.related_session_id,
            ]
            .into_iter()
            .flatten()
            .any(|id| !super::uuid_is_v7(id))
            || [self.auth_epoch, self.policy_revision]
                .into_iter()
                .flatten()
                .any(|value| value == 0 || value > super::MAX_SAFE_INTEGER)
        {
            return Err(OperatorStoreError::Invalid);
        }
        let valid = match self.kind {
            ControlAuthorityEventKind::SessionChallengeIssued => {
                self.human_id.is_some()
                    && self.session_id.is_none()
                    && self.challenge_id.is_some()
                    && self.challenge_digest.is_some()
                    && self.session_authority_digest.is_none()
                    && self.related_session_id.is_none()
                    && self.auth_epoch.is_some()
                    && self.policy_revision.is_some()
            }
            ControlAuthorityEventKind::SessionIssued => {
                self.human_id.is_some()
                    && self.session_id.is_some()
                    && self.challenge_id.is_some()
                    && self.challenge_digest.is_some()
                    && self.session_authority_digest.is_some()
                    && self.related_session_id.is_none()
                    && self.auth_epoch.is_some()
                    && self.policy_revision.is_some()
            }
            ControlAuthorityEventKind::SessionReplaced => {
                self.human_id.is_some()
                    && self.session_id.is_some()
                    && self.challenge_id.is_some()
                    && self.challenge_digest.is_some()
                    && self.session_authority_digest.is_some()
                    && self.related_session_id.is_some()
                    && self.auth_epoch.is_some()
                    && self.policy_revision.is_some()
            }
            ControlAuthorityEventKind::SessionExpired => {
                self.human_id.is_some()
                    && self.session_id.is_some()
                    && self.challenge_id.is_none()
                    && self.challenge_digest.is_none()
                    && self.session_authority_digest.is_some()
                    && self.related_session_id.is_none()
                    && self.auth_epoch.is_some()
                    && self.policy_revision.is_some()
            }
            ControlAuthorityEventKind::ControlShutdown => {
                self.human_id.is_none()
                    && self.session_id.is_none()
                    && self.challenge_id.is_none()
                    && self.challenge_digest.is_none()
                    && self.session_authority_digest.is_none()
                    && self.related_session_id.is_none()
                    && self.auth_epoch.is_none()
                    && self.policy_revision.is_none()
            }
        };
        if !valid {
            return Err(OperatorStoreError::Invalid);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlAuditAppendResult {
    pub schema: String,
    pub event: AuditEvent,
}
impl ControlAuditAppendResult {
    pub const SCHEMA: &'static str = "proof.operator.control-audit-append-result/v1";

    pub fn validate(&self) -> Result<(), OperatorStoreError> {
        if self.schema != Self::SCHEMA {
            return Err(OperatorStoreError::Invalid);
        }
        self.event
            .validate_chain_link(self.event.sequence, self.event.previous_digest)
            .map_err(|_| OperatorStoreError::Invalid)
    }
}

pub trait OperatorDirectoryStore: Send + Sync {
    fn load_operator_workspace(&self) -> Result<OperatorWorkspace, OperatorStoreError>;
    fn register_governed_run(
        &self,
        request: RegisterGovernedRunRequest,
    ) -> Result<RegisterGovernedRunResult, OperatorStoreError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperatorReadRoute {
    ApprovalDetail,
    Approvals,
    Attention,
    Audit,
    CommandDetail,
    Commands,
    RunDetail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CursorRoute {
    Approvals,
    Attention,
    Audit,
    Commands,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CursorSort {
    SequenceDescIdDesc,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CursorClaims {
    pub schema: String,
    pub route: CursorRoute,
    #[serde(with = "super::strict_uuid_v7")]
    pub workspace_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub server_instance_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub session_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub human_id: Uuid,
    #[serde(with = "super::strict_safe_integer")]
    pub auth_epoch: u64,
    pub required_capabilities: CapabilitySet,
    pub filter_digest: ControlDigest,
    pub sort: CursorSort,
    #[serde(with = "super::strict_safe_integer")]
    pub page_size: u64,
    #[serde(with = "super::strict_safe_integer")]
    pub high_water_sequence: u64,
    #[serde(with = "super::strict_safe_integer")]
    pub last_sequence: u64,
    #[serde(with = "super::strict_uuid_v7")]
    pub last_id: Uuid,
    #[serde(with = "super::strict_utc")]
    pub issued_at: DateTime<Utc>,
    #[serde(with = "super::strict_utc")]
    pub expires_at: DateTime<Utc>,
}

impl CursorClaims {
    pub const SCHEMA: &'static str = "proof.operator.cursor-claims/v1";

    pub fn validate(&self) -> Result<(), OperatorStoreError> {
        if self.schema != Self::SCHEMA
            || self.auth_epoch != 1
            || self.sort != CursorSort::SequenceDescIdDesc
            || !(1..=100).contains(&self.page_size)
            || self.high_water_sequence > super::MAX_SAFE_INTEGER
            || self.last_sequence > self.high_water_sequence
            || ![
                self.workspace_id,
                self.server_instance_id,
                self.session_id,
                self.human_id,
                self.last_id,
            ]
            .into_iter()
            .all(super::uuid_is_v7)
            || self.expires_at <= self.issued_at
            || self.expires_at - self.issued_at > chrono::Duration::seconds(300)
        {
            return Err(OperatorStoreError::Invalid);
        }
        Ok(())
    }

    pub fn validate_for_scope(
        &self,
        scope: &OperatorReadScope,
        page_size: u64,
        now: DateTime<Utc>,
    ) -> Result<(), OperatorStoreError> {
        self.validate()?;
        scope.validate()?;
        let route_matches = matches!(
            (self.route, scope.route),
            (CursorRoute::Approvals, OperatorReadRoute::Approvals)
                | (CursorRoute::Attention, OperatorReadRoute::Attention)
                | (CursorRoute::Audit, OperatorReadRoute::Audit)
                | (CursorRoute::Commands, OperatorReadRoute::Commands)
        );
        if !route_matches
            || self.workspace_id != scope.workspace_id
            || self.server_instance_id != scope.server_instance_id
            || self.session_id != scope.session_id
            || self.human_id != scope.human_id
            || self.auth_epoch != scope.auth_epoch
            || self.required_capabilities.as_slice() != scope.required_capabilities.as_slice()
            || scope.filter_digest != Some(self.filter_digest)
            || self.page_size != page_size
            || self.expires_at > scope.session_absolute_expires_at
            || now < self.issued_at
            || now >= self.expires_at
        {
            return Err(OperatorStoreError::Invalid);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorReadScope {
    pub schema: String,
    #[serde(with = "super::strict_uuid_v7")]
    pub workspace_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub server_instance_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub session_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub human_id: Uuid,
    #[serde(with = "super::strict_safe_integer")]
    pub auth_epoch: u64,
    #[serde(with = "super::strict_safe_integer")]
    pub policy_revision: u64,
    #[serde(with = "super::strict_utc")]
    pub session_absolute_expires_at: DateTime<Utc>,
    pub route: OperatorReadRoute,
    pub filter_digest: Option<ControlDigest>,
    pub granted_capabilities: CapabilitySet,
    pub required_capabilities: Vec<Capability>,
}
impl OperatorReadScope {
    pub const SCHEMA: &'static str = "proof.operator.read-scope/v1";

    pub fn validate(&self) -> Result<(), OperatorStoreError> {
        let paged = matches!(
            self.route,
            OperatorReadRoute::Approvals
                | OperatorReadRoute::Attention
                | OperatorReadRoute::Audit
                | OperatorReadRoute::Commands
        );
        let required_are_canonical = !self.required_capabilities.is_empty()
            && self.required_capabilities.len() <= 2
            && self
                .required_capabilities
                .windows(2)
                .all(|pair| pair[0] < pair[1]);
        let exact_route_capabilities = match self.route {
            OperatorReadRoute::Attention => matches!(
                self.required_capabilities.as_slice(),
                [Capability::ApprovalRead]
                    | [Capability::RunRead]
                    | [Capability::ApprovalRead, Capability::RunRead]
            ),
            OperatorReadRoute::ApprovalDetail | OperatorReadRoute::Approvals => {
                self.required_capabilities == [Capability::ApprovalRead]
            }
            OperatorReadRoute::RunDetail => self.required_capabilities == [Capability::RunRead],
            OperatorReadRoute::Audit
            | OperatorReadRoute::CommandDetail
            | OperatorReadRoute::Commands => self.required_capabilities == [Capability::AuditRead],
        };
        if self.schema != Self::SCHEMA
            || ![
                self.workspace_id,
                self.server_instance_id,
                self.session_id,
                self.human_id,
            ]
            .into_iter()
            .all(super::uuid_is_v7)
            || self.auth_epoch == 0
            || self.auth_epoch > super::MAX_SAFE_INTEGER
            || self.policy_revision == 0
            || self.policy_revision > super::MAX_SAFE_INTEGER
            || paged != self.filter_digest.is_some()
            || !required_are_canonical
            || !exact_route_capabilities
            || self
                .required_capabilities
                .iter()
                .any(|capability| !self.granted_capabilities.contains(*capability))
        {
            return Err(OperatorStoreError::Invalid);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PageWindowKind {
    Continuation,
    First,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifiedPageWindow {
    pub schema: String,
    pub kind: PageWindowKind,
    #[serde(with = "super::strict_optional_safe_integer")]
    pub high_water_sequence: Option<u64>,
    #[serde(with = "super::strict_optional_safe_integer")]
    pub last_sequence: Option<u64>,
    #[serde(with = "super::strict_optional_uuid_v7")]
    pub last_id: Option<Uuid>,
}
impl VerifiedPageWindow {
    pub fn first() -> Self {
        Self {
            schema: "proof.operator.verified-page-window/v1".into(),
            kind: PageWindowKind::First,
            high_water_sequence: None,
            last_sequence: None,
            last_id: None,
        }
    }
    pub fn continuation(
        high_water_sequence: u64,
        last_sequence: u64,
        last_id: Uuid,
    ) -> Result<Self, OperatorStoreError> {
        if !super::uuid_is_v7(last_id)
            || high_water_sequence > super::MAX_SAFE_INTEGER
            || last_sequence > high_water_sequence
        {
            return Err(OperatorStoreError::Invalid);
        }
        Ok(Self {
            schema: "proof.operator.verified-page-window/v1".into(),
            kind: PageWindowKind::Continuation,
            high_water_sequence: Some(high_water_sequence),
            last_sequence: Some(last_sequence),
            last_id: Some(last_id),
        })
    }
    pub fn validate(&self) -> Result<(), OperatorStoreError> {
        let all_none = self.high_water_sequence.is_none()
            && self.last_sequence.is_none()
            && self.last_id.is_none();
        let all_some = self.high_water_sequence.is_some()
            && self.last_sequence.is_some()
            && self.last_id.is_some();
        if self.schema != "proof.operator.verified-page-window/v1"
            || matches!(self.kind, PageWindowKind::First) != all_none
            || matches!(self.kind, PageWindowKind::Continuation) != all_some
            || self.high_water_sequence.unwrap_or(0) > super::MAX_SAFE_INTEGER
            || self.last_sequence.unwrap_or(0) > self.high_water_sequence.unwrap_or(0)
            || self.last_id.is_some_and(|id| !super::uuid_is_v7(id))
        {
            return Err(OperatorStoreError::Invalid);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PageInfo {
    #[serde(with = "super::strict_safe_integer")]
    pub page_size: u64,
    #[serde(with = "super::strict_safe_integer")]
    pub returned: u64,
    #[serde(with = "super::strict_safe_integer")]
    pub high_water_sequence: u64,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttentionKind {
    Approval,
    Run,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttentionState {
    AwaitingDecision,
    Recoverable,
    Running,
    Terminal,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalState {
    Approved,
    Denied,
    Expired,
    Pending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Urgency {
    Critical,
    High,
    Normal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttentionQuery {
    pub schema: String,
    pub kinds: Vec<AttentionKind>,
    pub states: Vec<AttentionState>,
    #[serde(with = "super::strict_safe_integer")]
    pub page_size: u64,
    pub cursor: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunAttentionItem {
    pub schema: String,
    pub kind: AttentionKind,
    #[serde(with = "super::strict_safe_integer")]
    pub projection_sequence: u64,
    #[serde(with = "super::strict_uuid_v7")]
    pub projection_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub run_id: Uuid,
    pub run_status: AgentRunStatus,
    pub attention: AttentionState,
    pub urgency: Urgency,
    pub goal_summary: String,
    #[serde(with = "super::strict_safe_integer")]
    pub run_revision: u64,
    #[serde(with = "super::strict_safe_integer")]
    pub control_revision: u64,
    #[serde(with = "super::strict_safe_integer")]
    pub fence_epoch: u64,
    #[serde(with = "super::strict_utc")]
    pub projected_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalAttentionItem {
    pub schema: String,
    pub kind: AttentionKind,
    #[serde(with = "super::strict_safe_integer")]
    pub projection_sequence: u64,
    #[serde(with = "super::strict_uuid_v7")]
    pub projection_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub run_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub approval_request_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub required_human_id: Uuid,
    pub operation: String,
    pub version: String,
    pub input_digest: ContentDigest,
    pub urgency: Urgency,
    #[serde(with = "super::strict_utc")]
    pub expires_at: DateTime<Utc>,
    #[serde(with = "super::strict_safe_integer")]
    pub run_revision: u64,
    #[serde(with = "super::strict_safe_integer")]
    pub control_revision: u64,
    #[serde(with = "super::strict_safe_integer")]
    pub fence_epoch: u64,
    #[serde(with = "super::strict_utc")]
    pub projected_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AttentionItem {
    Run(RunAttentionItem),
    Approval(ApprovalAttentionItem),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttentionPage {
    pub schema: String,
    pub items: Vec<AttentionItem>,
    pub page: PageInfo,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckpointTail {
    #[serde(with = "super::strict_uuid_v7")]
    pub checkpoint_id: Uuid,
    #[serde(with = "super::strict_safe_integer")]
    pub sequence: u64,
    pub state_digest: ContentDigest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetSnapshot {
    pub schema: String,
    #[serde(with = "super::strict_uuid_v7")]
    pub budget_id: Uuid,
    #[serde(with = "super::strict_safe_integer")]
    pub revision: u64,
    pub state: BudgetAccountState,
    pub limits: BudgetAmounts,
    pub reserved: BudgetAmounts,
    pub committed: BudgetAmounts,
    pub remaining: BudgetAmounts,
    #[serde(with = "super::strict_utc")]
    pub deadline_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PendingDecision {
    Approved,
    Denied,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingApprovalSummary {
    #[serde(with = "super::strict_uuid_v7")]
    pub approval_request_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub required_human_id: Uuid,
    pub operation: String,
    pub version: String,
    pub input_digest: ContentDigest,
    pub pending_consequence: PendingConsequence,
    #[serde(with = "super::strict_utc")]
    pub expires_at: DateTime<Utc>,
    pub decision: PendingDecision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthoritySummary {
    #[serde(with = "super::strict_uuid_v7")]
    pub agent_id: Uuid,
    #[serde(with = "super::strict_optional_uuid_v7")]
    pub human_id: Option<Uuid>,
    pub operation: String,
    pub version: String,
    #[serde(with = "super::strict_optional_uuid_v7")]
    pub delegation_id: Option<Uuid>,
    #[serde(with = "super::strict_safe_integer")]
    pub policy_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunAttemptSummary {
    #[serde(with = "super::strict_uuid_v7")]
    pub step_id: Uuid,
    #[serde(with = "super::strict_safe_integer")]
    pub ordinal: u64,
    #[serde(with = "super::strict_safe_integer")]
    pub attempt: u64,
    #[serde(with = "super::strict_optional_uuid_v7")]
    pub retry_of: Option<Uuid>,
    pub status: AgentRunStepStatus,
    pub operation: String,
    pub version: String,
    pub input_digest: ContentDigest,
    pub output_digest: Option<ContentDigest>,
    pub proof: Option<ProofReference>,
    pub error_class: Option<String>,
    #[serde(with = "super::strict_safe_integer")]
    pub revision: u64,
    #[serde(with = "super::strict_optional_utc")]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(with = "super::strict_optional_utc")]
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoverySummary {
    #[serde(with = "super::strict_uuid_v7")]
    pub directive_id: Uuid,
    pub classification: super::RecoveryClassification,
    #[serde(with = "super::strict_uuid_v7")]
    pub checkpoint_id: Uuid,
    #[serde(with = "super::strict_safe_integer")]
    pub checkpoint_sequence: u64,
    pub checkpoint_digest: ContentDigest,
    #[serde(with = "super::strict_uuid_v7")]
    pub source_lease_id: Uuid,
    #[serde(with = "super::strict_safe_integer")]
    pub source_fence_epoch: u64,
    #[serde(with = "super::strict_safe_integer")]
    pub source_control_revision: u64,
    pub intent_digest: ControlDigest,
    pub required_budget_disposition: super::RecoveryBudgetDisposition,
    pub directive_digest: ControlDigest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunDetail {
    pub schema: String,
    #[serde(with = "super::strict_uuid_v7")]
    pub run_id: Uuid,
    pub mode: AgentRunMode,
    pub status: AgentRunStatus,
    pub attention: AttentionState,
    pub goal_summary: String,
    pub authority: AuthoritySummary,
    pub attempts: Vec<RunAttemptSummary>,
    pub evidence: Vec<ProofReference>,
    #[serde(with = "super::strict_safe_integer")]
    pub run_revision: u64,
    #[serde(with = "super::strict_safe_integer")]
    pub control_revision: u64,
    #[serde(with = "super::strict_safe_integer")]
    pub fence_epoch: u64,
    pub checkpoint_tail: CheckpointTail,
    pub pending_approval: Option<PendingApprovalSummary>,
    pub recovery: Option<RecoverySummary>,
    pub budget: BudgetSnapshot,
    #[serde(with = "super::strict_utc")]
    pub created_at: DateTime<Utc>,
    #[serde(with = "super::strict_utc")]
    pub updated_at: DateTime<Utc>,
    #[serde(with = "super::strict_optional_utc")]
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalQuery {
    pub schema: String,
    pub states: Vec<ApprovalState>,
    #[serde(with = "super::strict_safe_integer")]
    pub page_size: u64,
    pub cursor: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalSummary {
    #[serde(with = "super::strict_uuid_v7")]
    pub approval_request_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub run_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub step_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub required_human_id: Uuid,
    pub operation: String,
    pub version: String,
    pub input_digest: ContentDigest,
    pub state: ApprovalState,
    #[serde(with = "super::strict_utc")]
    pub requested_at: DateTime<Utc>,
    #[serde(with = "super::strict_utc")]
    pub expires_at: DateTime<Utc>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalPage {
    pub schema: String,
    pub items: Vec<ApprovalSummary>,
    pub page: PageInfo,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalDecisionSummary {
    #[serde(with = "super::strict_uuid_v7")]
    pub decision_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub decided_by: Uuid,
    pub outcome: DecisionOutcome,
    pub decision_digest: ContentDigest,
    #[serde(with = "super::strict_utc")]
    pub decided_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalDetail {
    pub schema: String,
    pub summary: ApprovalSummary,
    pub request_digest: ContentDigest,
    pub checkpoint: CheckpointTail,
    #[serde(with = "super::strict_safe_integer")]
    pub run_revision: u64,
    #[serde(with = "super::strict_safe_integer")]
    pub step_revision: u64,
    #[serde(with = "super::strict_safe_integer")]
    pub control_revision: u64,
    #[serde(with = "super::strict_safe_integer")]
    pub fence_epoch: u64,
    pub argument_digest: ControlDigest,
    pub consequence_digest: ControlDigest,
    pub binding_digest: ControlDigest,
    pub pending_consequence: PendingConsequence,
    pub review_fields: Vec<ReviewField>,
    pub decision: Option<ApprovalDecisionSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandQuery {
    pub schema: String,
    pub kinds: Vec<CommandKind>,
    pub outcomes: Vec<CommandOutcome>,
    #[serde(with = "super::strict_optional_uuid_v7")]
    pub run_id: Option<Uuid>,
    #[serde(with = "super::strict_safe_integer")]
    pub page_size: u64,
    pub cursor: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandPage {
    pub schema: String,
    pub items: Vec<CommandReceipt>,
    pub page: PageInfo,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditQuery {
    pub schema: String,
    pub kinds: Vec<AuditEventKind>,
    #[serde(with = "super::strict_optional_uuid_v7")]
    pub run_id: Option<Uuid>,
    #[serde(with = "super::strict_optional_uuid_v7")]
    pub approval_request_id: Option<Uuid>,
    #[serde(with = "super::strict_safe_integer")]
    pub page_size: u64,
    pub cursor: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditPage {
    pub schema: String,
    pub items: Vec<AuditEvent>,
    pub page: PageInfo,
}

#[derive(Debug, Clone, Copy, Error, PartialEq, Eq)]
pub enum OperatorCursorError {
    #[error("cursor stale")]
    Stale,
    #[error("cursor unavailable")]
    Unavailable,
}

pub trait OperatorCursorCodec: Send + Sync {
    fn open_page(
        &self,
        scope: OperatorReadScope,
        cursor: Option<&str>,
        page_size: u64,
    ) -> Result<VerifiedPageWindow, OperatorCursorError>;
    fn seal_page(
        &self,
        scope: OperatorReadScope,
        page_size: u64,
        high_water_sequence: u64,
        last_sequence: u64,
        last_id: Uuid,
    ) -> Result<String, OperatorCursorError>;
}

pub trait OperatorReadStore: Send + Sync {
    fn page_attention(
        &self,
        query: AttentionQuery,
        scope: OperatorReadScope,
        cursor: &dyn OperatorCursorCodec,
    ) -> Result<AttentionPage, OperatorStoreError>;
    fn load_run_detail(
        &self,
        run_id: Uuid,
        scope: OperatorReadScope,
    ) -> Result<Option<RunDetail>, OperatorStoreError>;
    fn page_approvals(
        &self,
        query: ApprovalQuery,
        scope: OperatorReadScope,
        cursor: &dyn OperatorCursorCodec,
    ) -> Result<ApprovalPage, OperatorStoreError>;
    fn load_approval_detail(
        &self,
        request_id: Uuid,
        scope: OperatorReadScope,
    ) -> Result<Option<ApprovalDetail>, OperatorStoreError>;
    fn page_commands(
        &self,
        query: CommandQuery,
        scope: OperatorReadScope,
        cursor: &dyn OperatorCursorCodec,
    ) -> Result<CommandPage, OperatorStoreError>;
    fn load_command_receipt(
        &self,
        command_id: Uuid,
        scope: OperatorReadScope,
    ) -> Result<Option<CommandReceipt>, OperatorStoreError>;
    fn page_operator_audit(
        &self,
        query: AuditQuery,
        scope: OperatorReadScope,
        cursor: &dyn OperatorCursorCodec,
    ) -> Result<AuditPage, OperatorStoreError>;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InitialRunProjectionInput {
    pub schema: String,
    #[serde(with = "super::strict_uuid_v7")]
    pub workspace_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub run_id: Uuid,
    #[serde(with = "super::strict_safe_integer")]
    pub source_run_revision: u64,
    #[serde(with = "super::strict_uuid_v7")]
    pub checkpoint_id: Uuid,
    #[serde(with = "super::strict_safe_integer")]
    pub checkpoint_sequence: u64,
    pub checkpoint_digest: ContentDigest,
    pub run_status: crate::AgentRunStatus,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegisterGovernedRunRequest {
    pub schema: String,
    #[serde(with = "super::strict_uuid_v7")]
    pub workspace_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub run_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub budget_id: Uuid,
    pub initial_projection: InitialRunProjectionInput,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CreationOutcome {
    Created,
    ExactExisting,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegisterGovernedRunResult {
    pub schema: String,
    pub outcome: CreationOutcome,
    #[serde(with = "super::strict_uuid_v7")]
    pub run_id: Uuid,
    #[serde(with = "super::strict_safe_integer")]
    pub control_revision: u64,
    pub run_control: RunControl,
    pub initial_projection: RunProjection,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalSigningRequest {
    pub schema: String,
    #[serde(with = "super::strict_uuid_v7")]
    pub command_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub decision_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub authenticated_human_id: Uuid,
    pub approval_binding: ApprovalBinding,
    pub signed_request_digest: ContentDigest,
    pub outcome: DecisionOutcome,
    #[serde(with = "super::strict_utc")]
    pub validated_at: DateTime<Utc>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedDecisionResult {
    pub schema: String,
    pub decision_digest: ContentDigest,
    pub signature: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorProofSigningRequest {
    pub schema: String,
    #[serde(with = "super::strict_uuid_v7")]
    pub agent_id: Uuid,
    pub command: CommandEnvelope,
    pub command_digest: ControlDigest,
    pub input_digest: ContentDigest,
    pub output_digest: ContentDigest,
    #[serde(with = "super::strict_uuid_v7")]
    pub proof_id: Uuid,
    #[serde(with = "super::strict_utc")]
    pub timestamp: DateTime<Utc>,
    pub outcome: ControlTransitionOutcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperatorSignerError {
    #[error("identity mismatch")]
    IdentityMismatch,
    #[error("key load failed")]
    KeyLoadFailed,
    #[error("signing failed")]
    SigningFailed,
    #[error("verification failed")]
    VerificationFailed,
}
pub trait OperatorSigner: Send + Sync {
    fn sign_approval(
        &self,
        request: ApprovalSigningRequest,
    ) -> Result<SignedDecisionResult, OperatorSignerError>;
    fn sign_operator_proof(
        &self,
        request: OperatorProofSigningRequest,
    ) -> Result<Proof, OperatorSignerError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperatorMutationRoute {
    ApprovalDecide,
    RunCancel,
    RunResume,
    SessionRevoke,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorMutationScope {
    pub schema: String,
    pub route: OperatorMutationRoute,
    pub session_authority: SessionAuthorityBinding,
    pub session_authority_digest: ControlDigest,
    #[serde(with = "super::strict_safe_integer")]
    pub policy_revision: u64,
    pub required_capabilities: Vec<Capability>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandExecutionRequest {
    pub schema: String,
    pub scope: OperatorMutationScope,
    pub command: OperatorCommand,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandResultOutcome {
    AlreadyTerminal,
    Applied,
    ExactReplay,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandResult {
    pub schema: String,
    pub outcome: CommandResultOutcome,
    pub receipt: CommandReceipt,
}

pub trait OperatorCommandStore: Send + Sync {
    fn execute_operator_command(
        &self,
        request: CommandExecutionRequest,
        signer: &dyn OperatorSigner,
    ) -> Result<CommandResult, OperatorStoreError>;
}

pub struct LeaseClaimRequest<'a> {
    pub schema: String,
    pub workspace_id: Uuid,
    pub run_id: Uuid,
    pub lease_id: Uuid,
    pub owner_instance_id: Uuid,
    pub process_epoch_id: Uuid,
    lease_token: LeaseTokenProof<'a>,
    pub expected_fence_epoch: u64,
    pub expected_control_revision: u64,
}
impl LeaseClaimRequest<'_> {
    pub(crate) fn from_custody(
        binding: super::prepared::LeaseClaimBinding,
        lease_token: LeaseTokenProof<'_>,
    ) -> LeaseClaimRequest<'_> {
        LeaseClaimRequest {
            schema: "proof.operator.lease-claim-request/v1".into(),
            workspace_id: binding.workspace_id,
            run_id: binding.run_id,
            lease_id: binding.lease_id,
            owner_instance_id: binding.owner_instance_id,
            process_epoch_id: binding.process_epoch_id,
            lease_token,
            expected_fence_epoch: binding.expected_fence_epoch,
            expected_control_revision: binding.expected_control_revision,
        }
    }
    pub fn verifies_lease_token_digest(&self, expected: ControlDigest) -> bool {
        self.lease_token.verifies_digest(expected)
    }

    pub fn lease_token_digest(&self) -> ControlDigest {
        self.lease_token.digest()
    }
}
pub struct LeaseRenewRequest<'a> {
    pub schema: String,
    pub authority: LeaseAuthority<'a>,
}
pub struct LeaseReleaseRequest {
    pub schema: String,
    custody: LeaseTokenCustody,
    expected_control_revision: u64,
}
impl LeaseReleaseRequest {
    pub(crate) fn from_custody(custody: LeaseTokenCustody, expected_control_revision: u64) -> Self {
        Self {
            schema: "proof.operator.lease-release-request/v1".into(),
            custody,
            expected_control_revision,
        }
    }
    pub fn authority(&self) -> Result<LeaseAuthority<'_>, crate::ExecutionError> {
        self.custody.authority(self.expected_control_revision)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LeaseMutationOutcome {
    Acquired,
    Released,
    Renewed,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeaseMutationResult {
    pub schema: String,
    pub outcome: LeaseMutationOutcome,
    pub lease: RunLease,
    #[serde(with = "super::strict_safe_integer")]
    pub control_revision: u64,
}
impl LeaseMutationResult {
    pub const SCHEMA: &'static str = "proof.operator.lease-mutation-result/v1";

    pub fn validate(&self) -> Result<(), OperatorStoreError> {
        let state_matches = match self.outcome {
            LeaseMutationOutcome::Acquired | LeaseMutationOutcome::Renewed => {
                self.lease.state == super::RunLeaseState::Active
            }
            LeaseMutationOutcome::Released => self.lease.state == super::RunLeaseState::Released,
        };
        if self.schema != Self::SCHEMA
            || self.control_revision > super::MAX_SAFE_INTEGER
            || !state_matches
            || self.lease.validate().is_err()
        {
            return Err(OperatorStoreError::Invalid);
        }
        Ok(())
    }
}

pub struct BudgetReserveRequest<'a> {
    pub schema: String,
    pub authority: LeaseAuthority<'a>,
    pub reservation_id: Uuid,
    pub idempotency_key: Uuid,
    pub intent: DispatchIntent,
    pub intent_digest: ControlDigest,
    pub replay: Option<ReplayClaimBinding>,
    pub recovery: Option<RecoveryDirective>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetReserveOutcome {
    ExactExisting,
    Reserved,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetReserveResult {
    pub schema: String,
    pub outcome: BudgetReserveOutcome,
    pub reservation: BudgetReservation,
    #[serde(with = "super::strict_safe_integer")]
    pub budget_revision: u64,
    #[serde(with = "super::strict_safe_integer")]
    pub control_revision: u64,
}
impl BudgetReserveResult {
    pub const SCHEMA: &'static str = "proof.operator.budget-reserve-result/v1";

    pub fn validate(&self) -> Result<(), OperatorStoreError> {
        if self.schema != Self::SCHEMA
            || self.reservation.state != super::BudgetReservationState::Reserved
            || self.reservation.validate().is_err()
            || self.budget_revision > super::MAX_SAFE_INTEGER
            || self.control_revision > super::MAX_SAFE_INTEGER
        {
            return Err(OperatorStoreError::Invalid);
        }
        Ok(())
    }
}

pub struct BudgetSettlementRequest<'a> {
    pub schema: String,
    pub authority: LeaseAuthority<'a>,
    pub reservation_id: Uuid,
    pub disposition: BudgetSettlementDisposition,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetSettlementDisposition {
    ReleasePreDispatch,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetSettlementOutcome {
    Released,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetSettlementResult {
    pub schema: String,
    pub outcome: BudgetSettlementOutcome,
    pub reservation: BudgetReservation,
    #[serde(with = "super::strict_safe_integer")]
    pub budget_revision: u64,
    #[serde(with = "super::strict_safe_integer")]
    pub control_revision: u64,
}
impl BudgetSettlementResult {
    pub const SCHEMA: &'static str = "proof.operator.budget-settlement-result/v1";

    pub fn validate(&self) -> Result<(), OperatorStoreError> {
        if self.schema != Self::SCHEMA
            || self.outcome != BudgetSettlementOutcome::Released
            || self.reservation.state != super::BudgetReservationState::Released
            || self.reservation.validate().is_err()
            || self.budget_revision > super::MAX_SAFE_INTEGER
            || self.control_revision > super::MAX_SAFE_INTEGER
        {
            return Err(OperatorStoreError::Invalid);
        }
        Ok(())
    }
}

pub struct BeginDispatchRequest<'a> {
    pub schema: String,
    pub authority: LeaseAuthority<'a>,
    pub reservation_id: Uuid,
    dispatch_token: DispatchTokenProof<'a>,
    pub intent: DispatchIntent,
    pub intent_digest: ControlDigest,
    pub replay: Option<ReplayClaimBinding>,
    pub replay_claim_token: Option<Uuid>,
    pub call_digest: ControlDigest,
}
impl BeginDispatchRequest<'_> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn from_custody<'a>(
        authority: LeaseAuthority<'a>,
        reservation_id: Uuid,
        dispatch_token: DispatchTokenProof<'a>,
        intent: DispatchIntent,
        intent_digest: ControlDigest,
        replay: Option<ReplayClaimBinding>,
        replay_claim_token: Option<Uuid>,
        call_digest: ControlDigest,
    ) -> BeginDispatchRequest<'a> {
        BeginDispatchRequest {
            schema: "proof.operator.begin-dispatch-request/v1".into(),
            authority,
            reservation_id,
            dispatch_token,
            intent,
            intent_digest,
            replay,
            replay_claim_token,
            call_digest,
        }
    }
    pub fn verifies_dispatch_token_digest(&self, expected: ControlDigest) -> bool {
        self.dispatch_token.verifies_digest(expected)
    }

    pub fn dispatch_token_digest(&self) -> ControlDigest {
        self.dispatch_token.digest()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayProofEnvelope {
    pub schema: String,
    #[serde(with = "super::strict_uuid_v7")]
    pub proof_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub actor_id: Uuid,
    #[serde(with = "super::strict_optional_uuid_v7")]
    pub delegation_id: Option<Uuid>,
    pub operation: String,
    pub version: String,
    pub input_digest: ContentDigest,
    pub output_digest: ContentDigest,
    #[serde(with = "super::strict_utc")]
    pub timestamp: DateTime<Utc>,
    #[serde(with = "super::strict_optional_utc")]
    pub expires_at: Option<DateTime<Utc>>,
    pub signature: String,
}
impl ReplayProofEnvelope {
    pub const SCHEMA: &'static str = "proof.operator.replay-proof-envelope/v1";

    pub fn validate(&self) -> Result<(), OperatorStoreError> {
        if self.schema != Self::SCHEMA
            || !super::uuid_is_v7(self.proof_id)
            || !super::uuid_is_v7(self.actor_id)
            || self.delegation_id.is_some_and(|id| !super::uuid_is_v7(id))
            || !super::valid_operation_name(&self.operation)
            || !super::valid_operation_version(&self.version)
            || self
                .expires_at
                .is_some_and(|expires| expires <= self.timestamp)
            || !super::valid_fixed_base64url(&self.signature, 64)
        {
            return Err(OperatorStoreError::Invalid);
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayCompletionBinding {
    pub schema: String,
    pub replay_binding_digest: ControlDigest,
    #[serde(with = "super::strict_uuid_v7")]
    pub workspace_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub run_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub step_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub checkpoint_id: Uuid,
    #[serde(with = "super::strict_safe_integer")]
    pub checkpoint_sequence: u64,
    pub checkpoint_digest: ContentDigest,
    #[serde(with = "super::strict_safe_integer")]
    pub existing_run_revision: u64,
    #[serde(with = "super::strict_safe_integer")]
    pub existing_step_revision: u64,
    #[serde(with = "super::strict_safe_integer")]
    pub existing_control_revision: u64,
    pub canonical_output_json: String,
    pub input_digest: ContentDigest,
    pub output_digest: ContentDigest,
    pub proof: ReplayProofEnvelope,
}
impl ReplayCompletionBinding {
    pub const SCHEMA: &'static str = "proof.operator.replay-completion-binding/v1";

    pub fn validate(&self) -> Result<(), OperatorStoreError> {
        let output = serde_json::from_str::<serde_json::Value>(&self.canonical_output_json)
            .map_err(|_| OperatorStoreError::Invalid)?;
        let canonical = canonicalize(&output).map_err(|_| OperatorStoreError::Invalid)?;
        if self.schema != Self::SCHEMA
            || canonical.as_str() != self.canonical_output_json
            || self.canonical_output_json.len() < 2
            || self.canonical_output_json.len() > 1_048_576
            || ![
                self.workspace_id,
                self.run_id,
                self.step_id,
                self.checkpoint_id,
            ]
            .into_iter()
            .all(super::uuid_is_v7)
            || [
                self.checkpoint_sequence,
                self.existing_run_revision,
                self.existing_step_revision,
                self.existing_control_revision,
            ]
            .into_iter()
            .any(|value| value > super::MAX_SAFE_INTEGER)
            || self.proof.validate().is_err()
            || self.proof.input_digest != self.input_digest
            || self.proof.output_digest != self.output_digest
            || digest(ArtifactKind::OperationOutput, &canonical) != self.output_digest
        {
            return Err(OperatorStoreError::Invalid);
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayLookupRequest {
    pub schema: String,
    pub binding: ReplayClaimBinding,
}
impl ReplayLookupRequest {
    pub const SCHEMA: &'static str = "proof.operator.replay-lookup-request/v1";

    pub fn validate(&self) -> Result<(), OperatorStoreError> {
        if self.schema != Self::SCHEMA || self.binding.validate().is_err() {
            return Err(OperatorStoreError::Invalid);
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplayLookupOutcome {
    Completed,
    NotFound,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayLookupResult {
    pub schema: String,
    pub outcome: ReplayLookupOutcome,
    pub completion: Option<ReplayCompletionBinding>,
}
impl ReplayLookupResult {
    pub const SCHEMA: &'static str = "proof.operator.replay-lookup-result/v1";

    pub fn validate(&self) -> Result<(), OperatorStoreError> {
        let branch = match self.outcome {
            ReplayLookupOutcome::Completed => self
                .completion
                .as_ref()
                .is_some_and(|completion| completion.validate().is_ok()),
            ReplayLookupOutcome::NotFound => self.completion.is_none(),
        };
        if self.schema != Self::SCHEMA || !branch {
            return Err(OperatorStoreError::Invalid);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DispatchOutcome {
    DispatchAuthorized,
    ExactReplay,
    ReplayConflict,
    ReplayFailed,
    ReplayInProgress,
    ReplayUnsupported,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DispatchResult {
    pub schema: String,
    pub outcome: DispatchOutcome,
    pub permit: Option<DispatchPermit>,
    pub replay_completion: Option<ReplayCompletionBinding>,
    #[serde(with = "super::strict_safe_integer")]
    pub control_revision: u64,
}
impl DispatchResult {
    pub const SCHEMA: &'static str = "proof.operator.dispatch-result/v1";

    pub fn validate(&self) -> Result<(), OperatorStoreError> {
        if self.schema != Self::SCHEMA || self.control_revision > super::MAX_SAFE_INTEGER {
            return Err(OperatorStoreError::Invalid);
        }
        let branch = match self.outcome {
            DispatchOutcome::DispatchAuthorized => {
                self.permit
                    .as_ref()
                    .is_some_and(|permit| permit.validate().is_ok())
                    && self.replay_completion.is_none()
            }
            DispatchOutcome::ExactReplay => {
                self.permit.is_none()
                    && self
                        .replay_completion
                        .as_ref()
                        .is_some_and(|completion| completion.validate().is_ok())
            }
            DispatchOutcome::ReplayConflict
            | DispatchOutcome::ReplayFailed
            | DispatchOutcome::ReplayInProgress
            | DispatchOutcome::ReplayUnsupported => {
                self.permit.is_none() && self.replay_completion.is_none()
            }
        };
        if !branch {
            return Err(OperatorStoreError::Invalid);
        }
        Ok(())
    }
}

/// Persisted projection of sealed engine usage.
///
/// This record is not dispatch authority and cannot construct `PreparedUsage`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedUsageRecord {
    pub schema: String,
    pub boundary_kind: super::BoundaryKind,
    #[serde(with = "super::strict_safe_integer")]
    pub boundary_calls: u64,
    pub adapter: String,
    pub model: Option<String>,
    #[serde(with = "super::strict_safe_integer")]
    pub steps: u64,
    #[serde(with = "super::strict_safe_integer")]
    pub tokens: u64,
    #[serde(with = "super::strict_safe_integer")]
    pub cost_microusd: u64,
    #[serde(with = "super::strict_safe_integer")]
    pub tool_dispatches: u64,
    pub input_digest: ContentDigest,
    pub output_digest: ContentDigest,
}

impl PreparedUsageRecord {
    pub const SCHEMA: &'static str = "proof.operator.prepared-usage-body/v1";

    pub(crate) fn from_sealed(usage: &super::PreparedUsage) -> Self {
        Self {
            schema: Self::SCHEMA.into(),
            boundary_kind: usage.boundary_kind(),
            boundary_calls: usage.boundary_calls(),
            adapter: usage.adapter().into(),
            model: usage.model().map(str::to_string),
            steps: usage.steps(),
            tokens: usage.tokens(),
            cost_microusd: usage.cost_microusd(),
            tool_dispatches: usage.tool_dispatches(),
            input_digest: usage.input_digest(),
            output_digest: usage.output_digest(),
        }
    }

    pub fn validate(&self) -> Result<(), OperatorStoreError> {
        if self.schema != Self::SCHEMA
            || self.boundary_calls != 1
            || self.steps != 1
            || !super::valid_adapter_name(&self.adapter)
            || self
                .model
                .as_deref()
                .is_some_and(|value| !super::valid_model_name(value))
            || (self.boundary_kind == super::BoundaryKind::Provider) != self.model.is_some()
            || (self.boundary_kind == super::BoundaryKind::Provider && self.tool_dispatches != 0)
            || (self.boundary_kind == super::BoundaryKind::Tool
                && (self.tokens != 0 || self.cost_microusd != 0 || self.tool_dispatches != 1))
            || [self.tokens, self.cost_microusd, self.tool_dispatches]
                .into_iter()
                .any(|value| value > super::MAX_SAFE_INTEGER)
        {
            return Err(OperatorStoreError::Invalid);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedRuntimeResultBody {
    pub schema: String,
    pub usage: PreparedUsageRecord,
    pub output_digest: ContentDigest,
    pub proof: ProofReference,
    #[serde(with = "super::strict_safe_integer")]
    pub run_revision: u64,
    #[serde(with = "super::strict_safe_integer")]
    pub step_revision: u64,
    #[serde(with = "super::strict_optional_uuid_v7")]
    pub checkpoint_id: Option<Uuid>,
    #[serde(with = "super::strict_optional_safe_integer")]
    pub checkpoint_sequence: Option<u64>,
    pub checkpoint_digest: Option<ContentDigest>,
    #[serde(with = "super::strict_safe_integer")]
    pub first_event_sequence: u64,
    #[serde(with = "super::strict_safe_integer")]
    pub last_event_sequence: u64,
}

impl PreparedRuntimeResultBody {
    pub const SCHEMA: &'static str = "proof.operator.prepared-runtime-result-body/v1";

    pub fn validate(&self) -> Result<(), OperatorStoreError> {
        let checkpoint_all_none = self.checkpoint_id.is_none()
            && self.checkpoint_sequence.is_none()
            && self.checkpoint_digest.is_none();
        let checkpoint_all_some = self.checkpoint_id.is_some()
            && self.checkpoint_sequence.is_some()
            && self.checkpoint_digest.is_some();
        if self.schema != Self::SCHEMA
            || self.usage.validate().is_err()
            || !(checkpoint_all_none || checkpoint_all_some)
            || self
                .checkpoint_id
                .is_some_and(|value| !super::uuid_is_v7(value))
            || [
                self.run_revision,
                self.step_revision,
                self.checkpoint_sequence.unwrap_or(0),
                self.first_event_sequence,
                self.last_event_sequence,
            ]
            .into_iter()
            .any(|value| value > super::MAX_SAFE_INTEGER)
            || self.first_event_sequence > self.last_event_sequence
            || self.output_digest != self.usage.output_digest
            || self.proof.validate().is_err()
        {
            return Err(OperatorStoreError::Invalid);
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedExecutionBinding {
    pub schema: String,
    pub payload_digest: ControlDigest,
    pub replay_binding_digest: Option<ControlDigest>,
    #[serde(with = "super::strict_uuid_v7")]
    pub execution_context_id: Uuid,
    pub handler_mutation: super::PreparedHandlerMutation,
    pub result: PreparedRuntimeResultBody,
    pub result_digest: ControlDigest,
}

impl PreparedExecutionBinding {
    pub const SCHEMA: &'static str = "proof.operator.prepared-execution-binding/v1";

    pub fn from_prepared(
        prepared: &PreparedGovernedExecution,
        replay_binding_digest: Option<ControlDigest>,
    ) -> Result<Self, crate::ExecutionError> {
        if prepared.replay().claim().is_some() != replay_binding_digest.is_some() {
            return Err(crate::ExecutionError::EvidenceFailed(
                "prepared replay binding presence does not match replay transition".into(),
            ));
        }
        let proof = prepared.proof();
        let operation = proof
            .body
            .operation
            .split_once("::")
            .map(|(operation, _)| operation.to_string())
            .ok_or_else(|| {
                crate::ExecutionError::EvidenceFailed(
                    "prepared proof operation is not version-qualified".into(),
                )
            })?;
        let checkpoint = prepared.checkpoint();
        let first_event_sequence = prepared
            .events()
            .first()
            .map_or(0, |event| u64::from(event.sequence));
        let last_event_sequence = prepared
            .events()
            .last()
            .map_or(0, |event| u64::from(event.sequence));
        let result = PreparedRuntimeResultBody {
            schema: PreparedRuntimeResultBody::SCHEMA.into(),
            usage: PreparedUsageRecord::from_sealed(prepared.usage()),
            output_digest: proof.body.output_digest,
            proof: ProofReference {
                proof_id: proof.body.id,
                actor_id: proof.body.actor.as_uuid(),
                operation,
                proof_digest: proof
                    .proof_digest()
                    .map_err(|error| crate::ExecutionError::EvidenceFailed(error.to_string()))?,
            },
            run_revision: prepared.run_after().revision,
            step_revision: prepared.step_after().revision,
            checkpoint_id: checkpoint.map(|value| value.id),
            checkpoint_sequence: checkpoint.map(|value| u64::from(value.sequence)),
            checkpoint_digest: checkpoint.map(|value| value.state_digest),
            first_event_sequence,
            last_event_sequence,
        };
        let result_digest =
            super::control_digest_serialized("Proof-Operator-Runtime-Result-v1", &result)
                .map_err(|error| crate::ExecutionError::EvidenceFailed(error.to_string()))?;
        let binding = Self {
            schema: Self::SCHEMA.into(),
            payload_digest: prepared
                .payload_digest()
                .map_err(|error| crate::ExecutionError::EvidenceFailed(error.to_string()))?,
            replay_binding_digest,
            execution_context_id: prepared.execution_context_id(),
            handler_mutation: super::PreparedHandlerMutation::NoEffect,
            result,
            result_digest,
        };
        binding.validate()?;
        Ok(binding)
    }

    pub fn validate(&self) -> Result<(), crate::ExecutionError> {
        if self.schema != Self::SCHEMA
            || !super::uuid_is_v7(self.execution_context_id)
            || self.handler_mutation != super::PreparedHandlerMutation::NoEffect
            || self.result.validate().is_err()
        {
            return Err(crate::ExecutionError::EvidenceFailed(
                "prepared execution binding is invalid".into(),
            ));
        }
        let expected =
            super::control_digest_serialized("Proof-Operator-Runtime-Result-v1", &self.result)
                .map_err(|error| crate::ExecutionError::EvidenceFailed(error.to_string()))?;
        if expected != self.result_digest {
            return Err(crate::ExecutionError::EvidenceFailed(
                "prepared runtime result digest is invalid".into(),
            ));
        }
        Ok(())
    }
}

pub struct RuntimeCommitRequest<'a> {
    pub schema: String,
    pub authority: LeaseAuthority<'a>,
    pub permit: DispatchPermit,
    custody: DispatchTokenCustody,
    pub prepared: PreparedExecutionBinding,
}
impl RuntimeCommitRequest<'_> {
    pub(crate) fn from_custody(
        authority: LeaseAuthority<'_>,
        custody: DispatchTokenCustody,
        prepared: PreparedExecutionBinding,
    ) -> RuntimeCommitRequest<'_> {
        let permit = custody
            .permit()
            .expect("settlement custody is bound")
            .clone();
        RuntimeCommitRequest {
            schema: "proof.operator.runtime-commit-request/v1".into(),
            authority,
            permit,
            custody,
            prepared,
        }
    }
    pub fn verifies_dispatch_token_digest(&self, expected: ControlDigest) -> bool {
        self.custody.verifies_dispatch_token_digest(expected)
    }

    /// Returns whether the untrusted prepared projection is bound to the
    /// exact dispatch intent held inside custody.
    ///
    /// Storage calls this inside the commit barrier. A false result requires
    /// the barrier's atomic post-permit forfeit path; it is not a reason to
    /// discard this request before entering the barrier.
    pub fn prepared_matches_dispatch(&self) -> bool {
        self.custody.prepared_matches_dispatch(&self.prepared)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeCommitResult {
    pub schema: String,
    #[serde(with = "super::strict_safe_integer")]
    pub run_revision: u64,
    #[serde(with = "super::strict_safe_integer")]
    pub step_revision: u64,
    #[serde(with = "super::strict_safe_integer")]
    pub control_revision: u64,
    #[serde(with = "super::strict_safe_integer")]
    pub budget_revision: u64,
    pub charged: BudgetAmounts,
    pub proof: ProofReference,
}
impl RuntimeCommitResult {
    pub const SCHEMA: &'static str = "proof.operator.runtime-commit-result/v1";

    pub fn validate(&self) -> Result<(), OperatorStoreError> {
        if self.schema != Self::SCHEMA
            || [
                self.run_revision,
                self.step_revision,
                self.control_revision,
                self.budget_revision,
            ]
            .into_iter()
            .any(|value| value > super::MAX_SAFE_INTEGER)
            || !self.charged.is_safe()
            || self.proof.validate().is_err()
        {
            return Err(OperatorStoreError::Invalid);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeFailureCode {
    DispatchAmbiguous,
    HandlerFailed,
    ProcessShutdown,
    ResultInvalid,
}

/// Classifies an error returned by the governed execution boundary for its
/// durable runtime-failure settlement.
///
/// Governed evidence failures are sealed-result failures. Ordinary handler
/// failures remain handler failures regardless of their message text.
pub fn governed_runtime_failure_code(error: &crate::ExecutionError) -> RuntimeFailureCode {
    match error {
        crate::ExecutionError::EvidenceFailed(_) => RuntimeFailureCode::ResultInvalid,
        _ => RuntimeFailureCode::HandlerFailed,
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeFailureBody {
    pub schema: String,
    #[serde(with = "super::strict_uuid_v7")]
    pub reservation_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub permit_id: Uuid,
    pub classification: RuntimeFailureClassification,
    pub failure_code: RuntimeFailureCode,
    pub intent_digest: ControlDigest,
    pub call_digest: ControlDigest,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeFailureClassification {
    AmbiguousForfeitRequired,
}
pub struct RuntimeFailureRequest<'a> {
    pub schema: String,
    pub authority: LeaseAuthority<'a>,
    pub permit: DispatchPermit,
    custody: DispatchTokenCustody,
    pub failure: RuntimeFailureBody,
    pub error_digest: ControlDigest,
}
impl RuntimeFailureRequest<'_> {
    pub(crate) fn from_custody(
        authority: LeaseAuthority<'_>,
        custody: DispatchTokenCustody,
        failure: RuntimeFailureBody,
        error_digest: ControlDigest,
    ) -> RuntimeFailureRequest<'_> {
        let permit = custody
            .permit()
            .expect("settlement custody is bound")
            .clone();
        RuntimeFailureRequest {
            schema: "proof.operator.runtime-failure-request/v1".into(),
            authority,
            permit,
            custody,
            failure,
            error_digest,
        }
    }
    pub fn verifies_dispatch_token_digest(&self, expected: ControlDigest) -> bool {
        self.custody.verifies_dispatch_token_digest(expected)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeFailureResult {
    pub schema: String,
    #[serde(with = "super::strict_safe_integer")]
    pub run_revision: u64,
    #[serde(with = "super::strict_safe_integer")]
    pub control_revision: u64,
    #[serde(with = "super::strict_safe_integer")]
    pub budget_revision: u64,
    pub directive: Option<RecoveryDirective>,
}
impl RuntimeFailureResult {
    pub const SCHEMA: &'static str = "proof.operator.runtime-failure-result/v1";

    pub fn validate(&self) -> Result<(), OperatorStoreError> {
        if self.schema != Self::SCHEMA
            || [
                self.run_revision,
                self.control_revision,
                self.budget_revision,
            ]
            .into_iter()
            .any(|value| value > super::MAX_SAFE_INTEGER)
            || self.directive.is_some()
        {
            return Err(OperatorStoreError::Invalid);
        }
        Ok(())
    }
}

pub struct ReclaimRequest<'a> {
    pub schema: String,
    pub workspace_id: Uuid,
    pub run_id: Uuid,
    pub expired_lease_id: Uuid,
    pub expected_fence_epoch: u64,
    pub expected_control_revision: u64,
    pub new_lease_id: Uuid,
    pub owner_instance_id: Uuid,
    pub new_process_epoch_id: Uuid,
    new_lease_token: LeaseTokenProof<'a>,
    pub checkpoint_id: Uuid,
    pub checkpoint_sequence: u64,
    pub checkpoint_digest: ContentDigest,
}
impl ReclaimRequest<'_> {
    pub(crate) fn from_custody(
        claim: super::prepared::LeaseClaimBinding,
        expired_lease_id: Uuid,
        checkpoint_id: Uuid,
        checkpoint_sequence: u64,
        checkpoint_digest: ContentDigest,
        new_lease_token: LeaseTokenProof<'_>,
    ) -> ReclaimRequest<'_> {
        ReclaimRequest {
            schema: "proof.operator.reclaim-request/v1".into(),
            workspace_id: claim.workspace_id,
            run_id: claim.run_id,
            expired_lease_id,
            expected_fence_epoch: claim.expected_fence_epoch,
            expected_control_revision: claim.expected_control_revision,
            new_lease_id: claim.lease_id,
            owner_instance_id: claim.owner_instance_id,
            new_process_epoch_id: claim.process_epoch_id,
            new_lease_token,
            checkpoint_id,
            checkpoint_sequence,
            checkpoint_digest,
        }
    }
    pub fn verifies_new_lease_token_digest(&self, expected: ControlDigest) -> bool {
        self.new_lease_token.verifies_digest(expected)
    }

    pub fn new_lease_token_digest(&self) -> ControlDigest {
        self.new_lease_token.digest()
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReclaimOutcome {
    AmbiguousForfeited,
    IdleReclaimed,
    PreDispatchRecovered,
    RecoverableReclaimed,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReclaimResult {
    pub schema: String,
    pub outcome: ReclaimOutcome,
    pub lease: Option<RunLease>,
    pub directive: Option<RecoveryDirective>,
    #[serde(with = "super::strict_safe_integer")]
    pub control_revision: u64,
}
impl ReclaimResult {
    pub const SCHEMA: &'static str = "proof.operator.reclaim-result/v1";

    pub fn validate(&self) -> Result<(), OperatorStoreError> {
        let lease_valid = |lease: &RunLease| {
            lease.state == super::RunLeaseState::Active && lease.validate().is_ok()
        };
        let directive_valid = |directive: &RecoveryDirective| directive.validate().is_ok();
        let branch = match self.outcome {
            ReclaimOutcome::AmbiguousForfeited => self.lease.is_none() && self.directive.is_none(),
            ReclaimOutcome::IdleReclaimed => {
                self.lease.as_ref().is_some_and(lease_valid) && self.directive.is_none()
            }
            ReclaimOutcome::PreDispatchRecovered | ReclaimOutcome::RecoverableReclaimed => {
                self.lease.as_ref().is_some_and(lease_valid)
                    && self.directive.as_ref().is_some_and(directive_valid)
            }
        };
        if self.schema != Self::SCHEMA || self.control_revision > super::MAX_SAFE_INTEGER || !branch
        {
            return Err(OperatorStoreError::Invalid);
        }
        Ok(())
    }
}

pub trait ExecutionReplayTransaction {
    fn claim_execution_replay_in_transaction(
        &mut self,
        claim: &ExecutionReplayClaim,
    ) -> Result<ExecutionReplayClaimResult, String>;
}

pub trait OperatorRuntimeStore: Send + Sync {
    fn load_completed_replay(
        &self,
        request: ReplayLookupRequest,
    ) -> Result<ReplayLookupResult, OperatorStoreError>;
    fn claim_run_lease(
        &self,
        request: LeaseClaimRequest<'_>,
    ) -> Result<LeaseMutationResult, OperatorStoreError>;
    fn renew_run_lease(
        &self,
        request: LeaseRenewRequest<'_>,
    ) -> Result<LeaseMutationResult, OperatorStoreError>;
    fn release_run_lease(
        &self,
        request: LeaseReleaseRequest,
    ) -> Result<LeaseMutationResult, OperatorStoreError>;
    fn reserve_aggregate_budget(
        &self,
        request: BudgetReserveRequest<'_>,
    ) -> Result<BudgetReserveResult, OperatorStoreError>;
    fn settle_budget_reservation(
        &self,
        request: BudgetSettlementRequest<'_>,
    ) -> Result<BudgetSettlementResult, OperatorStoreError>;
    fn begin_dispatch(
        &self,
        request: BeginDispatchRequest<'_>,
    ) -> Result<DispatchResult, OperatorStoreError>;
    fn commit_runtime_barrier(
        &self,
        request: RuntimeCommitRequest<'_>,
        prepared: PreparedGovernedExecution,
    ) -> Result<RuntimeCommitResult, OperatorStoreError>;
    fn settle_runtime_failure(
        &self,
        request: RuntimeFailureRequest<'_>,
    ) -> Result<RuntimeFailureResult, OperatorStoreError>;
    fn reclaim_run(&self, request: ReclaimRequest<'_>)
        -> Result<ReclaimResult, OperatorStoreError>;
}

pub trait OperatorControlStore:
    OperatorDirectoryStore
    + OperatorAuthorityAuditStore
    + OperatorReadStore
    + OperatorCommandStore
    + OperatorRuntimeStore
{
}
impl<T> OperatorControlStore for T where
    T: OperatorDirectoryStore
        + OperatorAuthorityAuditStore
        + OperatorReadStore
        + OperatorCommandStore
        + OperatorRuntimeStore
{
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OperatorStoreBoundary {
    LoadOperatorWorkspace,
    RegisterGovernedRun,
    AppendAuthorityEvent,
    PageAttention,
    LoadRunDetail,
    PageApprovals,
    LoadApprovalDetail,
    PageCommands,
    LoadCommandReceipt,
    PageOperatorAudit,
    ExecuteOperatorCommand,
    LoadCompletedReplay,
    ClaimRunLease,
    RenewRunLease,
    ReleaseRunLease,
    ReserveAggregateBudget,
    SettleBudgetReservation,
    BeginDispatch,
    CommitRuntimeBarrier,
    SettleRuntimeFailure,
    ReclaimRun,
}

#[derive(Debug, Clone)]
pub enum RecordingOperatorResponse {
    Workspace(OperatorWorkspace),
    Register(RegisterGovernedRunResult),
    AuthorityAudit(ControlAuditAppendResult),
    Attention(AttentionPage),
    RunDetail(Option<RunDetail>),
    Approvals(ApprovalPage),
    ApprovalDetail(Option<ApprovalDetail>),
    Commands(CommandPage),
    CommandReceipt(Option<CommandReceipt>),
    Audit(AuditPage),
    Command(CommandResult),
    ReplayLookup(ReplayLookupResult),
    Lease(LeaseMutationResult),
    BudgetReserve(BudgetReserveResult),
    BudgetSettlement(BudgetSettlementResult),
    Dispatch(DispatchResult),
    RuntimeCommit(RuntimeCommitResult),
    RuntimeFailure(RuntimeFailureResult),
    Reclaim(ReclaimResult),
}

/// Nonsecret lease-authority fields captured by the recording store.
///
/// This diagnostic projection is intentionally not serializable and contains
/// only the derived token digest, never token custody or token bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordingLeaseAuthority {
    pub workspace_id: Uuid,
    pub run_id: Uuid,
    pub lease_id: Uuid,
    pub owner_instance_id: Uuid,
    pub process_epoch_id: Uuid,
    pub fence_epoch: u64,
    pub expected_control_revision: u64,
    pub lease_token_digest: ControlDigest,
}

impl RecordingLeaseAuthority {
    fn from_authority(authority: &LeaseAuthority<'_>) -> Self {
        Self {
            workspace_id: authority.workspace_id,
            run_id: authority.run_id,
            lease_id: authority.lease_id,
            owner_instance_id: authority.owner_instance_id,
            process_epoch_id: authority.process_epoch_id,
            fence_epoch: authority.fence_epoch,
            expected_control_revision: authority.expected_control_revision,
            lease_token_digest: authority.lease_token_digest(),
        }
    }
}

/// Nonsecret request projections captured by `RecordingOperatorControlStore`.
///
/// The enum deliberately has no serde implementation and cannot reveal the
/// secret-bearing request types consumed by the store boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordingOperatorRequest {
    Command {
        kind: CommandKind,
        command_id: Uuid,
        idempotency_key: Uuid,
        workspace_id: Uuid,
        server_instance_id: Uuid,
        session_id: Uuid,
        human_id: Uuid,
        run_id: Option<Uuid>,
        step_id: Option<Uuid>,
        expected_fence_epoch: Option<u64>,
        expected_control_revision: Option<u64>,
    },
    Claim {
        workspace_id: Uuid,
        run_id: Uuid,
        lease_id: Uuid,
        owner_instance_id: Uuid,
        process_epoch_id: Uuid,
        expected_fence_epoch: u64,
        expected_control_revision: u64,
        lease_token_digest: ControlDigest,
    },
    Reclaim {
        workspace_id: Uuid,
        run_id: Uuid,
        expired_lease_id: Uuid,
        new_lease_id: Uuid,
        owner_instance_id: Uuid,
        new_process_epoch_id: Uuid,
        expected_fence_epoch: u64,
        expected_control_revision: u64,
        new_lease_token_digest: ControlDigest,
        checkpoint_id: Uuid,
        checkpoint_sequence: u64,
        checkpoint_digest: ContentDigest,
    },
    Reserve {
        authority: RecordingLeaseAuthority,
        reservation_id: Uuid,
        idempotency_key: Uuid,
        intent_digest: ControlDigest,
        intent_ceiling: BudgetAmounts,
    },
    Begin {
        authority: RecordingLeaseAuthority,
        reservation_id: Uuid,
        dispatch_token_digest: ControlDigest,
        intent_digest: ControlDigest,
        call_digest: ControlDigest,
        replay_claim_token: Option<Uuid>,
        intent_ceiling: BudgetAmounts,
    },
    Commit {
        authority: RecordingLeaseAuthority,
        reservation_id: Uuid,
        permit_id: Uuid,
        dispatch_token_digest: ControlDigest,
        intent_ceiling: BudgetAmounts,
        prepared_matches_dispatch: bool,
    },
    Failure {
        authority: RecordingLeaseAuthority,
        reservation_id: Uuid,
        permit_id: Uuid,
        dispatch_token_digest: ControlDigest,
        intent_ceiling: BudgetAmounts,
        failure_code: RuntimeFailureCode,
        error_digest: ControlDigest,
    },
}

impl RecordingOperatorRequest {
    fn command(request: &CommandExecutionRequest) -> Self {
        let binding = request.command.binding();
        let (kind, run_id, step_id, expected_fence_epoch, expected_control_revision) =
            match &request.command {
                OperatorCommand::ApprovalDecision(command) => (
                    CommandKind::ApprovalDecide,
                    Some(command.run_id),
                    Some(command.step_id),
                    Some(command.expected_fence_epoch),
                    Some(command.expected_control_revision),
                ),
                OperatorCommand::RunCancel(command) => (
                    CommandKind::RunCancel,
                    Some(command.run_id),
                    None,
                    Some(command.expected_fence_epoch),
                    Some(command.expected_control_revision),
                ),
                OperatorCommand::RunResume(command) => (
                    CommandKind::RunResume,
                    Some(command.run_id),
                    Some(command.step_id),
                    Some(command.expected_fence_epoch),
                    Some(command.expected_control_revision),
                ),
                OperatorCommand::SessionRevoke(_) => {
                    (CommandKind::SessionRevoke, None, None, None, None)
                }
            };
        Self::Command {
            kind,
            command_id: binding.command_id,
            idempotency_key: binding.idempotency_key,
            workspace_id: binding.workspace_id,
            server_instance_id: binding.server_instance_id,
            session_id: binding.session_id,
            human_id: binding.human_id,
            run_id,
            step_id,
            expected_fence_epoch,
            expected_control_revision,
        }
    }
}

type RecordingResponder = Arc<
    dyn Fn(&RecordingOperatorRequest) -> Result<RecordingOperatorResponse, OperatorStoreError>
        + Send
        + Sync,
>;

#[derive(Default)]
pub struct RecordingOperatorControlStore {
    calls: Mutex<Vec<OperatorStoreBoundary>>,
    requests: Mutex<Vec<RecordingOperatorRequest>>,
    errors: Mutex<BTreeMap<OperatorStoreBoundary, VecDeque<OperatorStoreError>>>,
    responses: Mutex<VecDeque<RecordingOperatorResponse>>,
    responders: Mutex<BTreeMap<OperatorStoreBoundary, RecordingResponder>>,
}

impl RecordingOperatorControlStore {
    pub fn calls(&self) -> Vec<OperatorStoreBoundary> {
        self.calls
            .lock()
            .expect("operator call lock poisoned")
            .clone()
    }
    pub fn requests(&self) -> Vec<RecordingOperatorRequest> {
        self.requests
            .lock()
            .expect("operator request lock poisoned")
            .clone()
    }
    pub fn set_responder<F>(&self, boundary: OperatorStoreBoundary, responder: F)
    where
        F: Fn(&RecordingOperatorRequest) -> Result<RecordingOperatorResponse, OperatorStoreError>
            + Send
            + Sync
            + 'static,
    {
        self.responders
            .lock()
            .expect("operator responder lock poisoned")
            .insert(boundary, Arc::new(responder));
    }
    pub fn inject_error(&self, boundary: OperatorStoreBoundary, error: OperatorStoreError) {
        self.errors
            .lock()
            .expect("operator error lock poisoned")
            .entry(boundary)
            .or_default()
            .push_back(error);
    }
    pub fn push_response(&self, response: RecordingOperatorResponse) {
        self.responses
            .lock()
            .expect("operator response lock poisoned")
            .push_back(response);
    }
    fn enter(&self, boundary: OperatorStoreBoundary) -> Result<(), OperatorStoreError> {
        self.calls
            .lock()
            .map_err(|_| OperatorStoreError::Unavailable)?
            .push(boundary);
        if let Some(error) = self
            .errors
            .lock()
            .map_err(|_| OperatorStoreError::Unavailable)?
            .get_mut(&boundary)
            .and_then(VecDeque::pop_front)
        {
            return Err(error);
        }
        Ok(())
    }
    fn response(&self) -> Result<RecordingOperatorResponse, OperatorStoreError> {
        self.responses
            .lock()
            .map_err(|_| OperatorStoreError::Unavailable)?
            .pop_front()
            .ok_or(OperatorStoreError::Unavailable)
    }
    fn request_response(
        &self,
        boundary: OperatorStoreBoundary,
        request: RecordingOperatorRequest,
    ) -> Result<RecordingOperatorResponse, OperatorStoreError> {
        self.requests
            .lock()
            .map_err(|_| OperatorStoreError::Unavailable)?
            .push(request.clone());
        self.enter(boundary)?;
        let responder = self
            .responders
            .lock()
            .map_err(|_| OperatorStoreError::Unavailable)?
            .get(&boundary)
            .cloned();
        match responder {
            Some(responder) => responder(&request),
            None => self.response(),
        }
    }
}

impl OperatorDirectoryStore for RecordingOperatorControlStore {
    fn load_operator_workspace(&self) -> Result<OperatorWorkspace, OperatorStoreError> {
        self.enter(OperatorStoreBoundary::LoadOperatorWorkspace)?;
        match self.response()? {
            RecordingOperatorResponse::Workspace(v) => Ok(v),
            _ => Err(OperatorStoreError::Corrupt),
        }
    }
    fn register_governed_run(
        &self,
        _: RegisterGovernedRunRequest,
    ) -> Result<RegisterGovernedRunResult, OperatorStoreError> {
        self.enter(OperatorStoreBoundary::RegisterGovernedRun)?;
        match self.response()? {
            RecordingOperatorResponse::Register(v) => Ok(v),
            _ => Err(OperatorStoreError::Corrupt),
        }
    }
}
impl OperatorAuthorityAuditStore for RecordingOperatorControlStore {
    fn append_authority_event(
        &self,
        _: ControlAuditAppendRequest,
    ) -> Result<ControlAuditAppendResult, OperatorStoreError> {
        self.enter(OperatorStoreBoundary::AppendAuthorityEvent)?;
        match self.response()? {
            RecordingOperatorResponse::AuthorityAudit(v) => Ok(v),
            _ => Err(OperatorStoreError::Corrupt),
        }
    }
}
impl OperatorReadStore for RecordingOperatorControlStore {
    fn page_attention(
        &self,
        _: AttentionQuery,
        _: OperatorReadScope,
        _: &dyn OperatorCursorCodec,
    ) -> Result<AttentionPage, OperatorStoreError> {
        self.enter(OperatorStoreBoundary::PageAttention)?;
        match self.response()? {
            RecordingOperatorResponse::Attention(v) => Ok(v),
            _ => Err(OperatorStoreError::Corrupt),
        }
    }
    fn load_run_detail(
        &self,
        _: Uuid,
        _: OperatorReadScope,
    ) -> Result<Option<RunDetail>, OperatorStoreError> {
        self.enter(OperatorStoreBoundary::LoadRunDetail)?;
        match self.response()? {
            RecordingOperatorResponse::RunDetail(v) => Ok(v),
            _ => Err(OperatorStoreError::Corrupt),
        }
    }
    fn page_approvals(
        &self,
        _: ApprovalQuery,
        _: OperatorReadScope,
        _: &dyn OperatorCursorCodec,
    ) -> Result<ApprovalPage, OperatorStoreError> {
        self.enter(OperatorStoreBoundary::PageApprovals)?;
        match self.response()? {
            RecordingOperatorResponse::Approvals(v) => Ok(v),
            _ => Err(OperatorStoreError::Corrupt),
        }
    }
    fn load_approval_detail(
        &self,
        _: Uuid,
        _: OperatorReadScope,
    ) -> Result<Option<ApprovalDetail>, OperatorStoreError> {
        self.enter(OperatorStoreBoundary::LoadApprovalDetail)?;
        match self.response()? {
            RecordingOperatorResponse::ApprovalDetail(v) => Ok(v),
            _ => Err(OperatorStoreError::Corrupt),
        }
    }
    fn page_commands(
        &self,
        _: CommandQuery,
        _: OperatorReadScope,
        _: &dyn OperatorCursorCodec,
    ) -> Result<CommandPage, OperatorStoreError> {
        self.enter(OperatorStoreBoundary::PageCommands)?;
        match self.response()? {
            RecordingOperatorResponse::Commands(v) => Ok(v),
            _ => Err(OperatorStoreError::Corrupt),
        }
    }
    fn load_command_receipt(
        &self,
        _: Uuid,
        _: OperatorReadScope,
    ) -> Result<Option<CommandReceipt>, OperatorStoreError> {
        self.enter(OperatorStoreBoundary::LoadCommandReceipt)?;
        match self.response()? {
            RecordingOperatorResponse::CommandReceipt(v) => Ok(v),
            _ => Err(OperatorStoreError::Corrupt),
        }
    }
    fn page_operator_audit(
        &self,
        _: AuditQuery,
        _: OperatorReadScope,
        _: &dyn OperatorCursorCodec,
    ) -> Result<AuditPage, OperatorStoreError> {
        self.enter(OperatorStoreBoundary::PageOperatorAudit)?;
        match self.response()? {
            RecordingOperatorResponse::Audit(v) => Ok(v),
            _ => Err(OperatorStoreError::Corrupt),
        }
    }
}
impl OperatorCommandStore for RecordingOperatorControlStore {
    fn execute_operator_command(
        &self,
        request: CommandExecutionRequest,
        _: &dyn OperatorSigner,
    ) -> Result<CommandResult, OperatorStoreError> {
        let projection = RecordingOperatorRequest::command(&request);
        match self.request_response(OperatorStoreBoundary::ExecuteOperatorCommand, projection)? {
            RecordingOperatorResponse::Command(v) => Ok(v),
            _ => Err(OperatorStoreError::Corrupt),
        }
    }
}
impl OperatorRuntimeStore for RecordingOperatorControlStore {
    fn load_completed_replay(
        &self,
        _: ReplayLookupRequest,
    ) -> Result<ReplayLookupResult, OperatorStoreError> {
        self.enter(OperatorStoreBoundary::LoadCompletedReplay)?;
        match self.response()? {
            RecordingOperatorResponse::ReplayLookup(v) => Ok(v),
            _ => Err(OperatorStoreError::Corrupt),
        }
    }
    fn claim_run_lease(
        &self,
        request: LeaseClaimRequest<'_>,
    ) -> Result<LeaseMutationResult, OperatorStoreError> {
        let projection = RecordingOperatorRequest::Claim {
            workspace_id: request.workspace_id,
            run_id: request.run_id,
            lease_id: request.lease_id,
            owner_instance_id: request.owner_instance_id,
            process_epoch_id: request.process_epoch_id,
            expected_fence_epoch: request.expected_fence_epoch,
            expected_control_revision: request.expected_control_revision,
            lease_token_digest: request.lease_token_digest(),
        };
        match self.request_response(OperatorStoreBoundary::ClaimRunLease, projection)? {
            RecordingOperatorResponse::Lease(v) => Ok(v),
            _ => Err(OperatorStoreError::Corrupt),
        }
    }
    fn renew_run_lease(
        &self,
        _: LeaseRenewRequest<'_>,
    ) -> Result<LeaseMutationResult, OperatorStoreError> {
        self.enter(OperatorStoreBoundary::RenewRunLease)?;
        match self.response()? {
            RecordingOperatorResponse::Lease(v) => Ok(v),
            _ => Err(OperatorStoreError::Corrupt),
        }
    }
    fn release_run_lease(
        &self,
        _: LeaseReleaseRequest,
    ) -> Result<LeaseMutationResult, OperatorStoreError> {
        self.enter(OperatorStoreBoundary::ReleaseRunLease)?;
        match self.response()? {
            RecordingOperatorResponse::Lease(v) => Ok(v),
            _ => Err(OperatorStoreError::Corrupt),
        }
    }
    fn reserve_aggregate_budget(
        &self,
        request: BudgetReserveRequest<'_>,
    ) -> Result<BudgetReserveResult, OperatorStoreError> {
        let projection = RecordingOperatorRequest::Reserve {
            authority: RecordingLeaseAuthority::from_authority(&request.authority),
            reservation_id: request.reservation_id,
            idempotency_key: request.idempotency_key,
            intent_digest: request.intent_digest,
            intent_ceiling: request.intent.ceiling,
        };
        match self.request_response(OperatorStoreBoundary::ReserveAggregateBudget, projection)? {
            RecordingOperatorResponse::BudgetReserve(v) => Ok(v),
            _ => Err(OperatorStoreError::Corrupt),
        }
    }
    fn settle_budget_reservation(
        &self,
        _: BudgetSettlementRequest<'_>,
    ) -> Result<BudgetSettlementResult, OperatorStoreError> {
        self.enter(OperatorStoreBoundary::SettleBudgetReservation)?;
        match self.response()? {
            RecordingOperatorResponse::BudgetSettlement(v) => Ok(v),
            _ => Err(OperatorStoreError::Corrupt),
        }
    }
    fn begin_dispatch(
        &self,
        request: BeginDispatchRequest<'_>,
    ) -> Result<DispatchResult, OperatorStoreError> {
        let projection = RecordingOperatorRequest::Begin {
            authority: RecordingLeaseAuthority::from_authority(&request.authority),
            reservation_id: request.reservation_id,
            dispatch_token_digest: request.dispatch_token_digest(),
            intent_digest: request.intent_digest,
            call_digest: request.call_digest,
            replay_claim_token: request.replay_claim_token,
            intent_ceiling: request.intent.ceiling,
        };
        match self.request_response(OperatorStoreBoundary::BeginDispatch, projection)? {
            RecordingOperatorResponse::Dispatch(v) => Ok(v),
            _ => Err(OperatorStoreError::Corrupt),
        }
    }
    fn commit_runtime_barrier(
        &self,
        request: RuntimeCommitRequest<'_>,
        _: PreparedGovernedExecution,
    ) -> Result<RuntimeCommitResult, OperatorStoreError> {
        let projection = RecordingOperatorRequest::Commit {
            authority: RecordingLeaseAuthority::from_authority(&request.authority),
            reservation_id: request.permit.reservation_id,
            permit_id: request.permit.permit_id,
            dispatch_token_digest: request.custody.dispatch_token_digest(),
            intent_ceiling: request.custody.intent_ceiling(),
            prepared_matches_dispatch: request.prepared_matches_dispatch(),
        };
        match self.request_response(OperatorStoreBoundary::CommitRuntimeBarrier, projection)? {
            RecordingOperatorResponse::RuntimeCommit(v) => Ok(v),
            _ => Err(OperatorStoreError::Corrupt),
        }
    }
    fn settle_runtime_failure(
        &self,
        request: RuntimeFailureRequest<'_>,
    ) -> Result<RuntimeFailureResult, OperatorStoreError> {
        let projection = RecordingOperatorRequest::Failure {
            authority: RecordingLeaseAuthority::from_authority(&request.authority),
            reservation_id: request.permit.reservation_id,
            permit_id: request.permit.permit_id,
            dispatch_token_digest: request.custody.dispatch_token_digest(),
            intent_ceiling: request.custody.intent_ceiling(),
            failure_code: request.failure.failure_code,
            error_digest: request.error_digest,
        };
        match self.request_response(OperatorStoreBoundary::SettleRuntimeFailure, projection)? {
            RecordingOperatorResponse::RuntimeFailure(v) => Ok(v),
            _ => Err(OperatorStoreError::Corrupt),
        }
    }
    fn reclaim_run(
        &self,
        request: ReclaimRequest<'_>,
    ) -> Result<ReclaimResult, OperatorStoreError> {
        let projection = RecordingOperatorRequest::Reclaim {
            workspace_id: request.workspace_id,
            run_id: request.run_id,
            expired_lease_id: request.expired_lease_id,
            new_lease_id: request.new_lease_id,
            owner_instance_id: request.owner_instance_id,
            new_process_epoch_id: request.new_process_epoch_id,
            expected_fence_epoch: request.expected_fence_epoch,
            expected_control_revision: request.expected_control_revision,
            new_lease_token_digest: request.new_lease_token_digest(),
            checkpoint_id: request.checkpoint_id,
            checkpoint_sequence: request.checkpoint_sequence,
            checkpoint_digest: request.checkpoint_digest,
        };
        match self.request_response(OperatorStoreBoundary::ReclaimRun, projection)? {
            RecordingOperatorResponse::Reclaim(v) => Ok(v),
            _ => Err(OperatorStoreError::Corrupt),
        }
    }
}

pub trait OperatorAuthorityAuditStore: Send + Sync {
    fn append_authority_event(
        &self,
        request: ControlAuditAppendRequest,
    ) -> Result<ControlAuditAppendResult, OperatorStoreError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier};

    #[derive(Default)]
    struct RecordingCursorCodec {
        opened: Mutex<Vec<OperatorReadScope>>,
        sealed: Mutex<Vec<OperatorReadScope>>,
    }

    impl OperatorCursorCodec for RecordingCursorCodec {
        fn open_page(
            &self,
            scope: OperatorReadScope,
            _: Option<&str>,
            _: u64,
        ) -> Result<VerifiedPageWindow, OperatorCursorError> {
            self.opened.lock().unwrap().push(scope);
            Ok(VerifiedPageWindow::first())
        }

        fn seal_page(
            &self,
            scope: OperatorReadScope,
            _: u64,
            _: u64,
            _: u64,
            _: Uuid,
        ) -> Result<String, OperatorCursorError> {
            self.sealed.lock().unwrap().push(scope);
            Ok("sealed".into())
        }
    }

    fn replay_completion() -> ReplayCompletionBinding {
        let output = canonicalize(&serde_json::json!({"value": "out"})).unwrap();
        let output_digest = digest(ArtifactKind::OperationOutput, &output);
        ReplayCompletionBinding {
            schema: ReplayCompletionBinding::SCHEMA.into(),
            replay_binding_digest: ControlDigest::from_bytes([1; 32]),
            workspace_id: Uuid::now_v7(),
            run_id: Uuid::now_v7(),
            step_id: Uuid::now_v7(),
            checkpoint_id: Uuid::now_v7(),
            checkpoint_sequence: 0,
            checkpoint_digest: ContentDigest::from_bytes([2; 32]),
            existing_run_revision: 1,
            existing_step_revision: 1,
            existing_control_revision: 1,
            canonical_output_json: output.to_string(),
            input_digest: ContentDigest::from_bytes([3; 32]),
            output_digest,
            proof: ReplayProofEnvelope {
                schema: ReplayProofEnvelope::SCHEMA.into(),
                proof_id: Uuid::now_v7(),
                actor_id: Uuid::now_v7(),
                delegation_id: None,
                operation: "test.echo".into(),
                version: "v1".into(),
                input_digest: ContentDigest::from_bytes([3; 32]),
                output_digest,
                timestamp: "2030-01-01T00:00:00Z".parse().unwrap(),
                expires_at: None,
                signature: URL_SAFE_NO_PAD.encode([4_u8; 64]),
            },
        }
    }

    fn reserved_budget_reservation() -> BudgetReservation {
        let intent = DispatchIntent {
            schema: DispatchIntent::SCHEMA.into(),
            kind: crate::operator::BoundaryKind::Provider,
            adapter: "synthetic".into(),
            model: Some("fixed-v1".into()),
            operation: "test.echo".into(),
            version: "v1".into(),
            argument_digest: ControlDigest::from_bytes([5; 32]),
            ceiling: BudgetAmounts {
                steps: 1,
                tokens: 1,
                duration_ms: 1,
                cost_microusd: 1,
                tool_dispatches: 0,
            },
        };
        let intent_digest = crate::operator::control_digest_serialized(
            "Proof-Operator-Dispatch-Intent-v1",
            &intent,
        )
        .unwrap();
        BudgetReservation {
            schema: BudgetReservation::SCHEMA.into(),
            reservation_id: Uuid::now_v7(),
            budget_id: Uuid::now_v7(),
            run_id: Uuid::now_v7(),
            lease_id: Uuid::now_v7(),
            fence_epoch: 1,
            idempotency_key: Uuid::now_v7(),
            request_digest: ControlDigest::from_bytes([6; 32]),
            kind: crate::operator::BoundaryKind::Provider,
            intent,
            intent_digest,
            replay: None,
            recovery: None,
            state: crate::operator::BudgetReservationState::Reserved,
            reserved: BudgetAmounts {
                steps: 1,
                tokens: 1,
                duration_ms: 1,
                cost_microusd: 1,
                tool_dispatches: 0,
            },
            charged: BudgetAmounts::default(),
            created_at: "2030-01-01T00:00:00Z".parse().unwrap(),
            permit_id: None,
            dispatch_token_digest: None,
            call_digest: None,
            prepared_execution_digest: None,
            result_digest: None,
            prepared_binding: None,
            runtime_commit: None,
            dispatch_started_at: None,
            settled_at: None,
        }
    }

    fn control_shutdown_event() -> AuditEvent {
        let mut event = AuditEvent {
            schema: AuditEvent::SCHEMA.into(),
            workspace_id: Uuid::now_v7(),
            event_id: Uuid::now_v7(),
            sequence: 1,
            kind: AuditEventKind::ControlShutdown,
            outcome: crate::operator::AuditOutcome::Accepted,
            previous_digest: None,
            event_digest: ControlDigest::from_bytes([0; 32]),
            human_id: None,
            session_id: None,
            challenge_id: None,
            challenge_digest: None,
            session_authority_digest: None,
            related_session_id: None,
            server_instance_id: Some(Uuid::now_v7()),
            run_id: None,
            approval_request_id: None,
            command_id: None,
            command_kind: None,
            budget_id: None,
            reservation_id: None,
            lease_id: None,
            source_lease_id: None,
            process_epoch_id: None,
            permit_id: None,
            recovery_directive_id: None,
            fence_epoch: None,
            auth_epoch: None,
            policy_revision: None,
            intent_digest: None,
            call_digest: None,
            decision_digest: None,
            recovery_directive_digest: None,
            failure_scope: None,
            proof: None,
            occurred_at: "2030-01-01T00:00:00Z".parse().unwrap(),
        };
        let mut value = serde_json::to_value(&event).unwrap();
        value.as_object_mut().unwrap().remove("event_digest");
        event.event_digest =
            crate::operator::control_digest_serialized("Proof-Operator-Audit-Event-v1", &value)
                .unwrap();
        event
    }

    struct RejectingSigner;

    impl OperatorSigner for RejectingSigner {
        fn sign_approval(
            &self,
            _: ApprovalSigningRequest,
        ) -> Result<SignedDecisionResult, OperatorSignerError> {
            Err(OperatorSignerError::SigningFailed)
        }

        fn sign_operator_proof(
            &self,
            _: OperatorProofSigningRequest,
        ) -> Result<Proof, OperatorSignerError> {
            Err(OperatorSignerError::SigningFailed)
        }
    }

    fn command_request(command_id: Uuid, run_id: Uuid) -> CommandExecutionRequest {
        let at = "2030-01-01T00:00:00Z".parse().unwrap();
        let workspace_id = Uuid::now_v7();
        let server_instance_id = Uuid::now_v7();
        let session_id = Uuid::now_v7();
        let human_id = Uuid::now_v7();
        let binding = super::super::CommandBinding {
            command_id,
            idempotency_key: Uuid::now_v7(),
            workspace_id,
            server_instance_id,
            session_id,
            human_id,
            auth_epoch: 1,
            session_authority_digest: ControlDigest::from_bytes([9; 32]),
            policy_revision: 1,
        };
        CommandExecutionRequest {
            schema: "proof.operator.command-execution-request/v1".into(),
            scope: OperatorMutationScope {
                schema: "proof.operator.mutation-scope/v1".into(),
                route: OperatorMutationRoute::RunCancel,
                session_authority: SessionAuthorityBinding {
                    schema: SessionAuthorityBinding::SCHEMA.into(),
                    session_id,
                    workspace_id,
                    server_instance_id,
                    human_id,
                    auth_epoch: 1,
                    policy_revision: 1,
                    origin: "http://127.0.0.1".into(),
                    granted_capabilities: CapabilitySet::all(),
                    issued_at: at,
                    absolute_expires_at: at + chrono::Duration::minutes(1),
                },
                session_authority_digest: ControlDigest::from_bytes([9; 32]),
                policy_revision: 1,
                required_capabilities: vec![Capability::RunCancel],
            },
            command: OperatorCommand::RunCancel(super::super::RunCancelCommand {
                schema: super::super::RunCancelCommand::SCHEMA.into(),
                binding,
                run_id,
                expected_run_revision: 3,
                expected_control_revision: 4,
                expected_fence_epoch: 5,
            }),
        }
    }

    #[test]
    fn verified_windows_reject_partial_continuations() {
        assert!(VerifiedPageWindow::first().validate().is_ok());
        let partial = VerifiedPageWindow {
            schema: "proof.operator.verified-page-window/v1".into(),
            kind: PageWindowKind::Continuation,
            high_water_sequence: Some(2),
            last_sequence: None,
            last_id: None,
        };
        assert_eq!(partial.validate(), Err(OperatorStoreError::Invalid));
        assert_eq!(
            VerifiedPageWindow::continuation(super::super::MAX_SAFE_INTEGER + 1, 1, Uuid::now_v7()),
            Err(OperatorStoreError::Invalid)
        );
    }

    #[test]
    fn cursor_claims_bind_uuidv7_descending_sequence_and_deadline() {
        let issued_at = "2030-01-01T00:00:00Z".parse().unwrap();
        let mut claims = CursorClaims {
            schema: CursorClaims::SCHEMA.into(),
            route: CursorRoute::Audit,
            workspace_id: Uuid::now_v7(),
            server_instance_id: Uuid::now_v7(),
            session_id: Uuid::now_v7(),
            human_id: Uuid::now_v7(),
            auth_epoch: 1,
            required_capabilities: CapabilitySet::new(vec![Capability::AuditRead]).unwrap(),
            filter_digest: ControlDigest::from_bytes([1; 32]),
            sort: CursorSort::SequenceDescIdDesc,
            page_size: 25,
            high_water_sequence: 10,
            last_sequence: 8,
            last_id: Uuid::now_v7(),
            issued_at,
            expires_at: issued_at + chrono::Duration::minutes(1),
        };
        assert!(claims.validate().is_ok());
        let scope = OperatorReadScope {
            schema: OperatorReadScope::SCHEMA.into(),
            workspace_id: claims.workspace_id,
            server_instance_id: claims.server_instance_id,
            session_id: claims.session_id,
            human_id: claims.human_id,
            auth_epoch: 1,
            policy_revision: 1,
            session_absolute_expires_at: issued_at + chrono::Duration::minutes(10),
            route: OperatorReadRoute::Audit,
            filter_digest: Some(claims.filter_digest),
            granted_capabilities: CapabilitySet::all(),
            required_capabilities: vec![Capability::AuditRead],
        };
        assert!(scope.validate().is_ok());
        assert!(claims
            .validate_for_scope(&scope, 25, issued_at + chrono::Duration::seconds(1))
            .is_ok());
        assert_eq!(
            claims.validate_for_scope(&scope, 100, issued_at + chrono::Duration::seconds(1)),
            Err(OperatorStoreError::Invalid)
        );
        let mut uppercase = serde_json::to_value(&claims).unwrap();
        uppercase["last_id"] =
            serde_json::Value::String(claims.last_id.to_string().to_ascii_uppercase());
        assert!(serde_json::from_value::<CursorClaims>(uppercase).is_err());
        claims.last_sequence = 11;
        assert_eq!(claims.validate(), Err(OperatorStoreError::Invalid));
        claims.last_sequence = 8;
        claims.last_id = Uuid::nil();
        assert_eq!(claims.validate(), Err(OperatorStoreError::Invalid));
        assert!(serde_json::to_value(&claims).is_err());
        claims.last_id = Uuid::now_v7();
        claims.expires_at = issued_at + chrono::Duration::seconds(301);
        assert_eq!(claims.validate(), Err(OperatorStoreError::Invalid));
    }

    #[test]
    fn cursor_codec_consumes_the_exact_read_scope_by_value() {
        let codec = RecordingCursorCodec::default();
        let scope = OperatorReadScope {
            schema: OperatorReadScope::SCHEMA.into(),
            workspace_id: Uuid::now_v7(),
            server_instance_id: Uuid::now_v7(),
            session_id: Uuid::now_v7(),
            human_id: Uuid::now_v7(),
            auth_epoch: 1,
            policy_revision: 1,
            session_absolute_expires_at: "2030-01-01T00:10:00Z".parse().unwrap(),
            route: OperatorReadRoute::Audit,
            filter_digest: Some(ControlDigest::from_bytes([3; 32])),
            granted_capabilities: CapabilitySet::all(),
            required_capabilities: vec![Capability::AuditRead],
        };
        assert_eq!(
            codec.open_page(scope.clone(), None, 25).unwrap(),
            VerifiedPageWindow::first()
        );
        assert_eq!(
            codec
                .seal_page(scope.clone(), 25, 9, 7, Uuid::now_v7())
                .unwrap(),
            "sealed"
        );
        assert_eq!(codec.opened.lock().unwrap().as_slice(), &[scope.clone()]);
        assert_eq!(codec.sealed.lock().unwrap().as_slice(), &[scope]);
    }

    #[test]
    fn dispatch_result_enforces_every_replay_branch() {
        let at = "2030-01-01T00:00:00Z".parse().unwrap();
        let permit = DispatchPermit {
            schema: DispatchPermit::SCHEMA.into(),
            permit_id: Uuid::now_v7(),
            run_id: Uuid::now_v7(),
            reservation_id: Uuid::now_v7(),
            lease_id: Uuid::now_v7(),
            process_epoch_id: Uuid::now_v7(),
            fence_epoch: 1,
            expected_control_revision: 1,
            intent_digest: ControlDigest::from_bytes([1; 32]),
            replay_binding_digest: None,
            dispatch_token_digest: ControlDigest::from_bytes([2; 32]),
            call_digest: ControlDigest::from_bytes([3; 32]),
            authorized_at: at,
            budget_deadline_at: at + chrono::Duration::minutes(1),
        };
        let authorized = DispatchResult {
            schema: DispatchResult::SCHEMA.into(),
            outcome: DispatchOutcome::DispatchAuthorized,
            permit: Some(permit),
            replay_completion: None,
            control_revision: 2,
        };
        assert!(authorized.validate().is_ok());
        let mut nested_uppercase = serde_json::to_value(&authorized).unwrap();
        let permit_id = authorized.permit.as_ref().unwrap().permit_id;
        nested_uppercase["permit"]["permit_id"] =
            serde_json::Value::String(permit_id.to_string().to_ascii_uppercase());
        assert!(serde_json::from_value::<DispatchResult>(nested_uppercase).is_err());
        let mut nested_offset = serde_json::to_value(&authorized).unwrap();
        nested_offset["permit"]["authorized_at"] =
            serde_json::Value::String("2030-01-01T01:00:00+01:00".into());
        assert!(serde_json::from_value::<DispatchResult>(nested_offset).is_err());
        let mut nested_variant = serde_json::to_value(&authorized).unwrap();
        nested_variant["permit"]["permit_id"] =
            serde_json::Value::String("01890f47-9bcd-7def-c123-456789ab0001".into());
        assert!(serde_json::from_value::<DispatchResult>(nested_variant).is_err());
        let mut nested_leap = serde_json::to_value(&authorized).unwrap();
        nested_leap["permit"]["authorized_at"] =
            serde_json::Value::String("2030-01-01T00:00:60Z".into());
        assert!(serde_json::from_value::<DispatchResult>(nested_leap).is_err());
        let mut invalid_typed = authorized.clone();
        invalid_typed.permit.as_mut().unwrap().permit_id = Uuid::nil();
        assert!(serde_json::to_value(&invalid_typed).is_err());
        for outcome in [
            DispatchOutcome::ReplayConflict,
            DispatchOutcome::ReplayFailed,
            DispatchOutcome::ReplayInProgress,
            DispatchOutcome::ReplayUnsupported,
        ] {
            assert!(DispatchResult {
                schema: DispatchResult::SCHEMA.into(),
                outcome,
                permit: None,
                replay_completion: None,
                control_revision: 2,
            }
            .validate()
            .is_ok());
        }
        let mut invalid = authorized;
        invalid.outcome = DispatchOutcome::ReplayConflict;
        assert_eq!(invalid.validate(), Err(OperatorStoreError::Invalid));

        let completion = replay_completion();
        assert!(completion.validate().is_ok());
        let completed = ReplayLookupResult {
            schema: ReplayLookupResult::SCHEMA.into(),
            outcome: ReplayLookupOutcome::Completed,
            completion: Some(completion.clone()),
        };
        assert!(completed.validate().is_ok());
        assert!(DispatchResult {
            schema: DispatchResult::SCHEMA.into(),
            outcome: DispatchOutcome::ExactReplay,
            permit: None,
            replay_completion: Some(completion),
            control_revision: 2,
        }
        .validate()
        .is_ok());
        let invalid_lookup = ReplayLookupResult {
            schema: ReplayLookupResult::SCHEMA.into(),
            outcome: ReplayLookupOutcome::NotFound,
            completion: completed.completion,
        };
        assert_eq!(invalid_lookup.validate(), Err(OperatorStoreError::Invalid));
    }

    #[test]
    fn authority_audit_request_enforces_exact_transition_profiles() {
        let base = ControlAuditAppendRequest {
            schema: ControlAuditAppendRequest::SCHEMA.into(),
            workspace_id: Uuid::now_v7(),
            server_instance_id: Uuid::now_v7(),
            kind: ControlAuthorityEventKind::SessionIssued,
            human_id: Some(Uuid::now_v7()),
            session_id: Some(Uuid::now_v7()),
            challenge_id: Some(Uuid::now_v7()),
            challenge_digest: Some(ControlDigest::from_bytes([1; 32])),
            session_authority_digest: Some(ControlDigest::from_bytes([2; 32])),
            related_session_id: None,
            auth_epoch: Some(1),
            policy_revision: Some(1),
        };
        for kind in [
            ControlAuthorityEventKind::SessionChallengeIssued,
            ControlAuthorityEventKind::SessionIssued,
            ControlAuthorityEventKind::SessionReplaced,
            ControlAuthorityEventKind::SessionExpired,
            ControlAuthorityEventKind::ControlShutdown,
        ] {
            let mut request = base.clone();
            request.kind = kind;
            match kind {
                ControlAuthorityEventKind::SessionChallengeIssued => {
                    request.session_id = None;
                    request.session_authority_digest = None;
                }
                ControlAuthorityEventKind::SessionIssued => {}
                ControlAuthorityEventKind::SessionReplaced => {
                    request.related_session_id = Some(Uuid::now_v7());
                }
                ControlAuthorityEventKind::SessionExpired => {
                    request.challenge_id = None;
                    request.challenge_digest = None;
                }
                ControlAuthorityEventKind::ControlShutdown => {
                    request.human_id = None;
                    request.session_id = None;
                    request.challenge_id = None;
                    request.challenge_digest = None;
                    request.session_authority_digest = None;
                    request.auth_epoch = None;
                    request.policy_revision = None;
                }
            }
            assert!(request.validate().is_ok(), "{kind:?}");
        }
        let mut request = base;
        request.related_session_id = Some(Uuid::now_v7());
        assert_eq!(request.validate(), Err(OperatorStoreError::Invalid));
        let result = ControlAuditAppendResult {
            schema: ControlAuditAppendResult::SCHEMA.into(),
            event: control_shutdown_event(),
        };
        assert!(result.validate().is_ok());
    }

    #[test]
    fn runtime_and_reclaim_results_enforce_frozen_nullability() {
        let proof = ProofReference {
            proof_id: Uuid::now_v7(),
            actor_id: Uuid::now_v7(),
            operation: "test.echo".into(),
            proof_digest: ContentDigest::from_bytes([1; 32]),
        };
        assert!(proof.validate().is_ok());
        let commit = RuntimeCommitResult {
            schema: RuntimeCommitResult::SCHEMA.into(),
            run_revision: 1,
            step_revision: 1,
            control_revision: 1,
            budget_revision: 1,
            charged: BudgetAmounts::default(),
            proof,
        };
        assert!(commit.validate().is_ok());

        let failure = RuntimeFailureResult {
            schema: RuntimeFailureResult::SCHEMA.into(),
            run_revision: 1,
            control_revision: 1,
            budget_revision: 1,
            directive: None,
        };
        assert!(failure.validate().is_ok());
        let reclaim = ReclaimResult {
            schema: ReclaimResult::SCHEMA.into(),
            outcome: ReclaimOutcome::AmbiguousForfeited,
            lease: None,
            directive: None,
            control_revision: 1,
        };
        assert!(reclaim.validate().is_ok());
        assert_eq!(
            ReclaimResult {
                lease: Some(RunLease {
                    schema: RunLease::SCHEMA.into(),
                    run_id: Uuid::now_v7(),
                    workspace_id: Uuid::now_v7(),
                    lease_id: Uuid::now_v7(),
                    owner_instance_id: Uuid::now_v7(),
                    process_epoch_id: Uuid::now_v7(),
                    lease_token_digest: ControlDigest::from_bytes([2; 32]),
                    fence_epoch: 1,
                    revision: 0,
                    state: crate::operator::RunLeaseState::Active,
                    acquired_at: "2030-01-01T00:00:00Z".parse().unwrap(),
                    renewed_at: "2030-01-01T00:00:00Z".parse().unwrap(),
                    expires_at: "2030-01-01T00:00:30Z".parse().unwrap(),
                    released_at: None,
                    lease_digest: ControlDigest::from_bytes([3; 32]),
                }),
                ..reclaim
            }
            .validate(),
            Err(OperatorStoreError::Invalid)
        );
    }

    #[test]
    fn budget_store_results_require_exact_reservation_state() {
        let reservation = reserved_budget_reservation();
        let reserved = BudgetReserveResult {
            schema: BudgetReserveResult::SCHEMA.into(),
            outcome: BudgetReserveOutcome::Reserved,
            reservation: reservation.clone(),
            budget_revision: 1,
            control_revision: 1,
        };
        assert!(reserved.validate().is_ok());

        let mut released_reservation = reservation;
        released_reservation.state = crate::operator::BudgetReservationState::Released;
        released_reservation.settled_at = Some("2030-01-01T00:00:01Z".parse().unwrap());
        let settled = BudgetSettlementResult {
            schema: BudgetSettlementResult::SCHEMA.into(),
            outcome: BudgetSettlementOutcome::Released,
            reservation: released_reservation,
            budget_revision: 2,
            control_revision: 2,
        };
        assert!(settled.validate().is_ok());
        assert_eq!(
            BudgetReserveResult {
                reservation: settled.reservation,
                ..reserved
            }
            .validate(),
            Err(OperatorStoreError::Invalid)
        );
    }

    #[test]
    fn recording_store_records_and_injects_typed_failures() {
        let store = RecordingOperatorControlStore::default();
        store.inject_error(
            OperatorStoreBoundary::LoadCompletedReplay,
            OperatorStoreError::Conflict,
        );
        let mut binding = ReplayClaimBinding {
            schema: ReplayClaimBinding::SCHEMA.into(),
            policy: crate::operator::ReplayPolicy::RequiredUuidv7ExactReplay,
            workspace_id: Uuid::now_v7(),
            run_id: Uuid::now_v7(),
            step_id: Uuid::now_v7(),
            checkpoint_id: Uuid::now_v7(),
            checkpoint_sequence: 0,
            checkpoint_digest: ContentDigest::from_bytes([1; 32]),
            operation: "test.echo".into(),
            version: "v1".into(),
            idempotency_key: Uuid::now_v7(),
            input_digest: ContentDigest::from_bytes([2; 32]),
            claimed_by: crate::PrincipalId::now(),
            binding_digest: ControlDigest::from_bytes([3; 32]),
        };
        binding.binding_digest = binding.recomputed_binding_digest().unwrap();
        let request = ReplayLookupRequest {
            schema: ReplayLookupRequest::SCHEMA.into(),
            binding,
        };
        assert!(request.validate().is_ok());
        assert_eq!(
            store.load_completed_replay(request.clone()),
            Err(OperatorStoreError::Conflict)
        );
        store.push_response(RecordingOperatorResponse::ReplayLookup(
            ReplayLookupResult {
                schema: "proof.operator.replay-lookup-result/v1".into(),
                outcome: ReplayLookupOutcome::NotFound,
                completion: None,
            },
        ));
        assert_eq!(
            store.load_completed_replay(request).unwrap().outcome,
            ReplayLookupOutcome::NotFound
        );
        assert_eq!(
            store.calls(),
            vec![OperatorStoreBoundary::LoadCompletedReplay; 2]
        );
    }

    #[test]
    fn recording_store_responder_inspects_commands_concurrently() {
        let store = Arc::new(RecordingOperatorControlStore::default());
        let barrier = Arc::new(Barrier::new(2));
        let responder_calls = Arc::new(AtomicUsize::new(0));
        store.set_responder(OperatorStoreBoundary::ExecuteOperatorCommand, {
            let barrier = barrier.clone();
            let responder_calls = responder_calls.clone();
            move |request| {
                assert!(matches!(
                    request,
                    RecordingOperatorRequest::Command {
                        kind: CommandKind::RunCancel,
                        run_id: Some(_),
                        step_id: None,
                        expected_fence_epoch: Some(5),
                        expected_control_revision: Some(4),
                        ..
                    }
                ));
                responder_calls.fetch_add(1, Ordering::SeqCst);
                barrier.wait();
                Err(OperatorStoreError::Conflict)
            }
        });
        let first_command_id = Uuid::now_v7();
        let second_command_id = Uuid::now_v7();
        let first_run_id = Uuid::now_v7();
        let second_run_id = Uuid::now_v7();
        std::thread::scope(|scope| {
            let first = {
                let store = store.clone();
                scope.spawn(move || {
                    store.execute_operator_command(
                        command_request(first_command_id, first_run_id),
                        &RejectingSigner,
                    )
                })
            };
            let second = {
                let store = store.clone();
                scope.spawn(move || {
                    store.execute_operator_command(
                        command_request(second_command_id, second_run_id),
                        &RejectingSigner,
                    )
                })
            };
            assert_eq!(first.join().unwrap(), Err(OperatorStoreError::Conflict));
            assert_eq!(second.join().unwrap(), Err(OperatorStoreError::Conflict));
        });
        assert_eq!(responder_calls.load(Ordering::SeqCst), 2);
        assert_eq!(
            store.calls(),
            vec![OperatorStoreBoundary::ExecuteOperatorCommand; 2]
        );
        let requests = store.requests();
        assert_eq!(requests.len(), 2);
        let mut observed = requests
            .iter()
            .map(|request| match request {
                RecordingOperatorRequest::Command {
                    kind: CommandKind::RunCancel,
                    command_id,
                    workspace_id,
                    run_id: Some(run_id),
                    expected_fence_epoch: Some(5),
                    expected_control_revision: Some(4),
                    ..
                } => {
                    assert!(!workspace_id.is_nil());
                    (*command_id, *run_id)
                }
                other => panic!("unexpected command projection: {other:?}"),
            })
            .collect::<Vec<_>>();
        observed.sort();
        let mut expected = vec![
            (first_command_id, first_run_id),
            (second_command_id, second_run_id),
        ];
        expected.sort();
        assert_eq!(observed, expected);
    }
}
