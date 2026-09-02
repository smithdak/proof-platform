use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use super::{
    ArtifactDigest, BudgetAmounts, CapabilitySet, ControlDigest, PreparedExecutionBinding,
};
use crate::{AgentRunStatus, ContentDigest, PrincipalId, PrincipalKind};

macro_rules! schema_value {
    ($name:ident, $value:literal) => {
        impl $name {
            pub const SCHEMA: &'static str = $value;
        }
    };
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum OperatorValidationError {
    #[error("schema discriminator is invalid")]
    InvalidSchema,
    #[error("UUID must be UUIDv7")]
    InvalidUuid,
    #[error("revision, epoch, or safe integer is invalid")]
    InvalidRevision,
    #[error("nullable fields do not form the required transition branch")]
    InvalidBranch,
    #[error("transition chronology is invalid")]
    InvalidChronology,
    #[error("budget values exceed their ceiling")]
    BudgetExceeded,
}

fn uuid_v7(value: Uuid) -> bool {
    super::uuid_is_v7(value)
}

fn digest_without_field<T: Serialize>(
    domain: &str,
    value: &T,
    field: &str,
) -> Result<ControlDigest, OperatorValidationError> {
    let mut value =
        serde_json::to_value(value).map_err(|_| OperatorValidationError::InvalidSchema)?;
    value
        .as_object_mut()
        .ok_or(OperatorValidationError::InvalidSchema)?
        .remove(field);
    super::control_digest_serialized(domain, &value)
        .map_err(|_| OperatorValidationError::InvalidSchema)
}

fn decode_fixed_base64url<const N: usize>(value: &str) -> Option<[u8; N]> {
    let decoded = URL_SAFE_NO_PAD.decode(value).ok()?;
    decoded.try_into().ok()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrincipalBinding {
    #[serde(with = "super::strict_principal_id")]
    pub principal_id: PrincipalId,
    pub kind: PrincipalKind,
    pub public_key: String,
    pub public_key_fingerprint: ControlDigest,
}

impl PrincipalBinding {
    pub fn validate(&self) -> Result<(), OperatorValidationError> {
        let public_key = decode_fixed_base64url::<32>(&self.public_key)
            .ok_or(OperatorValidationError::InvalidSchema)?;
        if !uuid_v7(self.principal_id.as_uuid())
            || self.public_key_fingerprint
                != super::control_digest("Proof-Operator-Public-Key-v1", &public_key)
        {
            return Err(OperatorValidationError::InvalidSchema);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DescriptorIdentity {
    #[serde(with = "super::strict_safe_integer")]
    pub device: u64,
    #[serde(with = "super::strict_safe_integer")]
    pub inode: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceFingerprintInput {
    pub schema: String,
    #[serde(with = "super::strict_uuid_v7")]
    pub workspace_id: Uuid,
    pub proof_directory: DescriptorIdentity,
    pub control_lock: DescriptorIdentity,
    pub agent_key_file: DescriptorIdentity,
    pub human_key_file: DescriptorIdentity,
    #[serde(with = "super::strict_uuid_v7")]
    pub agent_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub human_id: Uuid,
    pub agent_public_key: String,
    pub human_public_key: String,
}
schema_value!(
    WorkspaceFingerprintInput,
    "proof.operator.workspace-fingerprint-input/v1"
);
impl WorkspaceFingerprintInput {
    pub fn validate(&self) -> Result<(), OperatorValidationError> {
        let descriptors = [
            self.proof_directory,
            self.control_lock,
            self.agent_key_file,
            self.human_key_file,
        ];
        if self.schema != Self::SCHEMA
            || ![self.workspace_id, self.agent_id, self.human_id]
                .into_iter()
                .all(uuid_v7)
            || descriptors.iter().any(|identity| {
                identity.device > super::MAX_SAFE_INTEGER
                    || identity.inode == 0
                    || identity.inode > super::MAX_SAFE_INTEGER
            })
            || decode_fixed_base64url::<32>(&self.agent_public_key).is_none()
            || decode_fixed_base64url::<32>(&self.human_public_key).is_none()
        {
            return Err(OperatorValidationError::InvalidSchema);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchemaCatalogEntryBinding {
    pub operation: String,
    pub version: String,
    pub registry_entry_path: String,
    pub registry_entry_sha256: ArtifactDigest,
    pub input_schema_path: String,
    pub input_schema_sha256: ArtifactDigest,
    pub output_schema_path: String,
    pub output_schema_sha256: ArtifactDigest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchemaCatalogBinding {
    pub schema: String,
    pub entries: Vec<SchemaCatalogEntryBinding>,
}
schema_value!(
    SchemaCatalogBinding,
    "proof.operator.schema-catalog-binding/v1"
);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorWorkspace {
    pub schema: String,
    #[serde(with = "super::strict_uuid_v7")]
    pub workspace_id: Uuid,
    pub database_name: String,
    pub fingerprint_input: WorkspaceFingerprintInput,
    pub workspace_fingerprint: ControlDigest,
    pub schema_catalog_digest: ControlDigest,
    pub agent: PrincipalBinding,
    pub human: PrincipalBinding,
    #[serde(with = "super::strict_safe_integer")]
    pub auth_epoch: u64,
    #[serde(with = "super::strict_safe_integer")]
    pub policy_revision: u64,
    pub capabilities: CapabilitySet,
    #[serde(with = "super::strict_utc")]
    pub created_at: DateTime<Utc>,
    #[serde(with = "super::strict_utc")]
    pub updated_at: DateTime<Utc>,
    pub binding_digest: ControlDigest,
}
schema_value!(OperatorWorkspace, "proof-operator-workspace/v1");
impl OperatorWorkspace {
    pub fn validate(&self) -> Result<(), OperatorValidationError> {
        self.fingerprint_input.validate()?;
        self.agent.validate()?;
        self.human.validate()?;
        if self.schema != Self::SCHEMA
            || self.database_name != "storage.db"
            || !uuid_v7(self.workspace_id)
            || self.fingerprint_input.workspace_id != self.workspace_id
            || self.agent.principal_id.as_uuid() != self.fingerprint_input.agent_id
            || self.human.principal_id.as_uuid() != self.fingerprint_input.human_id
            || self.agent.kind != PrincipalKind::Agent
            || self.human.kind != PrincipalKind::Human
            || self.agent.public_key != self.fingerprint_input.agent_public_key
            || self.human.public_key != self.fingerprint_input.human_public_key
            || self.auth_epoch != 1
            || self.policy_revision != 1
            || self.created_at > self.updated_at
            || self.workspace_fingerprint
                != super::control_digest_serialized(
                    "Proof-Operator-Workspace-v1",
                    &self.fingerprint_input,
                )
                .map_err(|_| OperatorValidationError::InvalidSchema)?
            || self.binding_digest
                != digest_without_field(
                    "Proof-Operator-Workspace-Binding-v1",
                    self,
                    "binding_digest",
                )?
        {
            return Err(OperatorValidationError::InvalidSchema);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanEnrollment {
    pub schema: String,
    #[serde(with = "super::strict_uuid_v7")]
    pub workspace_id: Uuid,
    pub human: PrincipalBinding,
    pub capabilities: CapabilitySet,
    pub capability_set_digest: ControlDigest,
    #[serde(with = "super::strict_utc")]
    pub enrolled_at: DateTime<Utc>,
}
schema_value!(HumanEnrollment, "proof-operator-human-enrollment/v1");
impl HumanEnrollment {
    pub fn validate(&self) -> Result<(), OperatorValidationError> {
        self.human.validate()?;
        if self.schema != Self::SCHEMA
            || !uuid_v7(self.workspace_id)
            || self.human.kind != PrincipalKind::Human
            || self.capability_set_digest
                != super::control_digest_serialized(
                    "Proof-Operator-Capability-Set-v1",
                    &self.capabilities,
                )
                .map_err(|_| OperatorValidationError::InvalidSchema)?
        {
            return Err(OperatorValidationError::InvalidSchema);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionAuthorityBinding {
    pub schema: String,
    #[serde(with = "super::strict_uuid_v7")]
    pub session_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub workspace_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub server_instance_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub human_id: Uuid,
    #[serde(with = "super::strict_safe_integer")]
    pub auth_epoch: u64,
    #[serde(with = "super::strict_safe_integer")]
    pub policy_revision: u64,
    pub origin: String,
    pub granted_capabilities: CapabilitySet,
    #[serde(with = "super::strict_utc")]
    pub issued_at: DateTime<Utc>,
    #[serde(with = "super::strict_utc")]
    pub absolute_expires_at: DateTime<Utc>,
}
schema_value!(
    SessionAuthorityBinding,
    "proof.operator.session.authority-binding/v1"
);
impl SessionAuthorityBinding {
    pub fn validate(&self) -> Result<(), OperatorValidationError> {
        if self.schema != Self::SCHEMA
            || ![
                self.session_id,
                self.workspace_id,
                self.server_instance_id,
                self.human_id,
            ]
            .into_iter()
            .all(uuid_v7)
            || self.auth_epoch == 0
            || self.auth_epoch > super::MAX_SAFE_INTEGER
            || self.policy_revision == 0
            || self.policy_revision > super::MAX_SAFE_INTEGER
            || self.origin.is_empty()
            || self.absolute_expires_at <= self.issued_at
        {
            return Err(OperatorValidationError::InvalidSchema);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetPolicy {
    pub schema: String,
    #[serde(with = "super::strict_uuid_v7")]
    pub budget_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub workspace_id: Uuid,
    pub limits: BudgetAmounts,
    #[serde(with = "super::strict_utc")]
    pub deadline_at: DateTime<Utc>,
    pub limits_digest: ControlDigest,
}
schema_value!(BudgetPolicy, "proof.operator.budget-policy/v1");
impl BudgetPolicy {
    pub fn validate(&self) -> Result<(), OperatorValidationError> {
        if self.schema != Self::SCHEMA
            || !uuid_v7(self.budget_id)
            || !uuid_v7(self.workspace_id)
            || !self.limits.is_safe()
            || self.limits_digest
                != digest_without_field("Proof-Operator-Budget-Limits-v1", self, "limits_digest")?
        {
            return Err(OperatorValidationError::InvalidSchema);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetAccountState {
    Active,
    Closed,
    Exhausted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetAccount {
    pub schema: String,
    pub policy: BudgetPolicy,
    #[serde(with = "super::strict_safe_integer")]
    pub revision: u64,
    pub state: BudgetAccountState,
    pub reserved: BudgetAmounts,
    pub committed: BudgetAmounts,
    #[serde(with = "super::strict_utc")]
    pub created_at: DateTime<Utc>,
    #[serde(with = "super::strict_utc")]
    pub updated_at: DateTime<Utc>,
}
schema_value!(BudgetAccount, "proof-operator-budget-account/v1");
impl BudgetAccount {
    pub fn validate(&self) -> Result<(), OperatorValidationError> {
        self.policy.validate()?;
        if self.schema != Self::SCHEMA
            || self.revision > super::MAX_SAFE_INTEGER
            || !self.reserved.is_safe()
            || !self.committed.is_safe()
        {
            return Err(OperatorValidationError::InvalidSchema);
        }
        if self.updated_at < self.created_at || self.policy.deadline_at <= self.created_at {
            return Err(OperatorValidationError::InvalidChronology);
        }
        let used = BudgetAmounts {
            steps: self
                .reserved
                .steps
                .checked_add(self.committed.steps)
                .ok_or(OperatorValidationError::BudgetExceeded)?,
            tokens: self
                .reserved
                .tokens
                .checked_add(self.committed.tokens)
                .ok_or(OperatorValidationError::BudgetExceeded)?,
            duration_ms: self
                .reserved
                .duration_ms
                .checked_add(self.committed.duration_ms)
                .ok_or(OperatorValidationError::BudgetExceeded)?,
            cost_microusd: self
                .reserved
                .cost_microusd
                .checked_add(self.committed.cost_microusd)
                .ok_or(OperatorValidationError::BudgetExceeded)?,
            tool_dispatches: self
                .reserved
                .tool_dispatches
                .checked_add(self.committed.tool_dispatches)
                .ok_or(OperatorValidationError::BudgetExceeded)?,
        };
        if !used.fits_within(&self.policy.limits) {
            return Err(OperatorValidationError::BudgetExceeded);
        }
        Ok(())
    }
    pub fn can_reserve(
        &self,
        requested: &BudgetAmounts,
        now: DateTime<Utc>,
    ) -> Result<(), OperatorValidationError> {
        self.validate()?;
        if !requested.is_safe()
            || !matches!(self.state, BudgetAccountState::Active)
            || now >= self.policy.deadline_at
        {
            return Err(OperatorValidationError::BudgetExceeded);
        }
        let next = BudgetAmounts {
            steps: self
                .reserved
                .steps
                .checked_add(requested.steps)
                .ok_or(OperatorValidationError::BudgetExceeded)?,
            tokens: self
                .reserved
                .tokens
                .checked_add(requested.tokens)
                .ok_or(OperatorValidationError::BudgetExceeded)?,
            duration_ms: self
                .reserved
                .duration_ms
                .checked_add(requested.duration_ms)
                .ok_or(OperatorValidationError::BudgetExceeded)?,
            cost_microusd: self
                .reserved
                .cost_microusd
                .checked_add(requested.cost_microusd)
                .ok_or(OperatorValidationError::BudgetExceeded)?,
            tool_dispatches: self
                .reserved
                .tool_dispatches
                .checked_add(requested.tool_dispatches)
                .ok_or(OperatorValidationError::BudgetExceeded)?,
        };
        let combined = BudgetAmounts {
            steps: next
                .steps
                .checked_add(self.committed.steps)
                .ok_or(OperatorValidationError::BudgetExceeded)?,
            tokens: next
                .tokens
                .checked_add(self.committed.tokens)
                .ok_or(OperatorValidationError::BudgetExceeded)?,
            duration_ms: next
                .duration_ms
                .checked_add(self.committed.duration_ms)
                .ok_or(OperatorValidationError::BudgetExceeded)?,
            cost_microusd: next
                .cost_microusd
                .checked_add(self.committed.cost_microusd)
                .ok_or(OperatorValidationError::BudgetExceeded)?,
            tool_dispatches: next
                .tool_dispatches
                .checked_add(self.committed.tool_dispatches)
                .ok_or(OperatorValidationError::BudgetExceeded)?,
        };
        if !combined.fits_within(&self.policy.limits) {
            return Err(OperatorValidationError::BudgetExceeded);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunControl {
    pub schema: String,
    #[serde(with = "super::strict_uuid_v7")]
    pub run_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub workspace_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub budget_id: Uuid,
    #[serde(with = "super::strict_safe_integer")]
    pub control_revision: u64,
    #[serde(with = "super::strict_optional_uuid_v7")]
    pub active_dispatch_reservation_id: Option<Uuid>,
    #[serde(with = "super::strict_optional_uuid_v7")]
    pub recovery_directive_id: Option<Uuid>,
    pub recovery_directive_digest: Option<ControlDigest>,
    #[serde(with = "super::strict_optional_uuid_v7")]
    pub last_command_id: Option<Uuid>,
    #[serde(with = "super::strict_utc")]
    pub created_at: DateTime<Utc>,
    #[serde(with = "super::strict_utc")]
    pub updated_at: DateTime<Utc>,
    pub binding_digest: ControlDigest,
}
schema_value!(RunControl, "proof-operator-run-control/v1");
impl RunControl {
    pub fn validate(&self) -> Result<(), OperatorValidationError> {
        if self.schema != Self::SCHEMA {
            return Err(OperatorValidationError::InvalidSchema);
        }
        if !uuid_v7(self.run_id)
            || !uuid_v7(self.workspace_id)
            || !uuid_v7(self.budget_id)
            || self.control_revision > super::MAX_SAFE_INTEGER
        {
            return Err(OperatorValidationError::InvalidRevision);
        }
        if self.recovery_directive_id.is_some() != self.recovery_directive_digest.is_some() {
            return Err(OperatorValidationError::InvalidBranch);
        }
        if [
            self.active_dispatch_reservation_id,
            self.recovery_directive_id,
            self.last_command_id,
        ]
        .into_iter()
        .flatten()
        .any(|id| !uuid_v7(id))
            || self.updated_at < self.created_at
            || self.binding_digest
                != digest_without_field("Proof-Operator-Run-Binding-v1", self, "binding_digest")?
        {
            return Err(OperatorValidationError::InvalidChronology);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunLeaseState {
    Active,
    Released,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunLease {
    pub schema: String,
    #[serde(with = "super::strict_uuid_v7")]
    pub run_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub workspace_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub lease_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub owner_instance_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub process_epoch_id: Uuid,
    pub lease_token_digest: ControlDigest,
    #[serde(with = "super::strict_safe_integer")]
    pub fence_epoch: u64,
    #[serde(with = "super::strict_safe_integer")]
    pub revision: u64,
    pub state: RunLeaseState,
    #[serde(with = "super::strict_utc")]
    pub acquired_at: DateTime<Utc>,
    #[serde(with = "super::strict_utc")]
    pub renewed_at: DateTime<Utc>,
    #[serde(with = "super::strict_utc")]
    pub expires_at: DateTime<Utc>,
    #[serde(with = "super::strict_optional_utc")]
    pub released_at: Option<DateTime<Utc>>,
    pub lease_digest: ControlDigest,
}
schema_value!(RunLease, "proof.operator.run-lease/v1");
impl RunLease {
    pub fn validate(&self) -> Result<(), OperatorValidationError> {
        if self.schema != Self::SCHEMA {
            return Err(OperatorValidationError::InvalidSchema);
        }
        if self.fence_epoch == 0
            || self.fence_epoch > super::MAX_SAFE_INTEGER
            || self.revision > super::MAX_SAFE_INTEGER
            || ![
                self.run_id,
                self.workspace_id,
                self.lease_id,
                self.owner_instance_id,
                self.process_epoch_id,
            ]
            .into_iter()
            .all(uuid_v7)
        {
            return Err(OperatorValidationError::InvalidRevision);
        }
        if self.renewed_at < self.acquired_at
            || self.expires_at <= self.renewed_at
            || matches!(self.state, RunLeaseState::Active) != self.released_at.is_none()
            || matches!(self.state, RunLeaseState::Active)
                && self.expires_at.signed_duration_since(self.renewed_at)
                    != chrono::Duration::seconds(30)
            || self
                .released_at
                .is_some_and(|released| released < self.renewed_at || released >= self.expires_at)
        {
            return Err(OperatorValidationError::InvalidChronology);
        }
        if self.lease_digest
            != digest_without_field("Proof-Operator-Lease-v1", self, "lease_digest")?
        {
            return Err(OperatorValidationError::InvalidSchema);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryKind {
    Provider,
    Tool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DispatchIntent {
    pub schema: String,
    pub kind: BoundaryKind,
    pub adapter: String,
    pub model: Option<String>,
    pub operation: String,
    pub version: String,
    pub argument_digest: ControlDigest,
    pub ceiling: BudgetAmounts,
}
schema_value!(DispatchIntent, "proof.operator.dispatch-intent/v1");
impl DispatchIntent {
    pub fn validate(&self) -> Result<(), OperatorValidationError> {
        if self.schema != Self::SCHEMA
            || !super::valid_operation_name(&self.operation)
            || !super::valid_operation_version(&self.version)
            || !super::valid_adapter_name(&self.adapter)
            || self
                .model
                .as_deref()
                .is_some_and(|model| !super::valid_model_name(model))
        {
            return Err(OperatorValidationError::InvalidSchema);
        }
        if !self.ceiling.is_safe()
            || matches!(self.kind, BoundaryKind::Provider) != self.model.is_some()
        {
            return Err(OperatorValidationError::InvalidBranch);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplayPolicy {
    RequiredUuidv7ExactReplay,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayClaimBinding {
    pub schema: String,
    pub policy: ReplayPolicy,
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
    pub operation: String,
    pub version: String,
    #[serde(with = "super::strict_uuid_v7")]
    pub idempotency_key: Uuid,
    pub input_digest: ContentDigest,
    #[serde(with = "super::strict_principal_id")]
    pub claimed_by: PrincipalId,
    pub binding_digest: ControlDigest,
}
schema_value!(ReplayClaimBinding, "proof.operator.replay-claim-binding/v1");
impl ReplayClaimBinding {
    pub fn recomputed_binding_digest(&self) -> Result<ControlDigest, OperatorValidationError> {
        let mut value =
            serde_json::to_value(self).map_err(|_| OperatorValidationError::InvalidSchema)?;
        value
            .as_object_mut()
            .ok_or(OperatorValidationError::InvalidSchema)?
            .remove("binding_digest");
        super::control_digest_serialized("Proof-Operator-Replay-Binding-v1", &value)
            .map_err(|_| OperatorValidationError::InvalidSchema)
    }

    pub fn validate(&self) -> Result<(), OperatorValidationError> {
        if self.schema != Self::SCHEMA
            || self.policy != ReplayPolicy::RequiredUuidv7ExactReplay
            || ![
                self.workspace_id,
                self.run_id,
                self.step_id,
                self.checkpoint_id,
                self.idempotency_key,
                self.claimed_by.as_uuid(),
            ]
            .into_iter()
            .all(super::uuid_is_v7)
            || self.checkpoint_sequence > super::MAX_SAFE_INTEGER
            || !super::valid_operation_name(&self.operation)
            || !super::valid_operation_version(&self.version)
        {
            return Err(OperatorValidationError::InvalidSchema);
        }
        if self.recomputed_binding_digest()? != self.binding_digest {
            return Err(OperatorValidationError::InvalidSchema);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DispatchPermit {
    pub schema: String,
    #[serde(with = "super::strict_uuid_v7")]
    pub permit_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub run_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub reservation_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub lease_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub process_epoch_id: Uuid,
    #[serde(with = "super::strict_safe_integer")]
    pub fence_epoch: u64,
    #[serde(with = "super::strict_safe_integer")]
    pub expected_control_revision: u64,
    pub intent_digest: ControlDigest,
    pub replay_binding_digest: Option<ControlDigest>,
    pub dispatch_token_digest: ControlDigest,
    pub call_digest: ControlDigest,
    #[serde(with = "super::strict_utc")]
    pub authorized_at: DateTime<Utc>,
    #[serde(with = "super::strict_utc")]
    pub budget_deadline_at: DateTime<Utc>,
}
schema_value!(DispatchPermit, "proof.operator.dispatch-permit/v1");
impl DispatchPermit {
    pub fn validate(&self) -> Result<(), OperatorValidationError> {
        if self.schema != Self::SCHEMA
            || ![
                self.permit_id,
                self.run_id,
                self.reservation_id,
                self.lease_id,
                self.process_epoch_id,
            ]
            .into_iter()
            .all(super::uuid_is_v7)
            || self.fence_epoch == 0
            || self.fence_epoch > super::MAX_SAFE_INTEGER
            || self.expected_control_revision > super::MAX_SAFE_INTEGER
        {
            return Err(OperatorValidationError::InvalidRevision);
        }
        if self.budget_deadline_at <= self.authorized_at {
            return Err(OperatorValidationError::InvalidChronology);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetReservationState {
    Committed,
    Dispatching,
    Forfeited,
    Released,
    Reserved,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetReservation {
    pub schema: String,
    #[serde(with = "super::strict_uuid_v7")]
    pub reservation_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub budget_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub run_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub lease_id: Uuid,
    #[serde(with = "super::strict_safe_integer")]
    pub fence_epoch: u64,
    #[serde(with = "super::strict_uuid_v7")]
    pub idempotency_key: Uuid,
    pub request_digest: ControlDigest,
    pub kind: BoundaryKind,
    pub intent: DispatchIntent,
    pub intent_digest: ControlDigest,
    pub replay: Option<ReplayClaimBinding>,
    pub recovery: Option<RecoveryDirective>,
    pub state: BudgetReservationState,
    pub reserved: BudgetAmounts,
    pub charged: BudgetAmounts,
    #[serde(with = "super::strict_utc")]
    pub created_at: DateTime<Utc>,
    #[serde(with = "super::strict_optional_uuid_v7")]
    pub permit_id: Option<Uuid>,
    pub dispatch_token_digest: Option<ControlDigest>,
    pub call_digest: Option<ControlDigest>,
    pub prepared_execution_digest: Option<ControlDigest>,
    pub result_digest: Option<ControlDigest>,
    pub prepared_binding: Option<PreparedExecutionBinding>,
    pub runtime_commit: Option<RuntimeCommit>,
    #[serde(with = "super::strict_optional_utc")]
    pub dispatch_started_at: Option<DateTime<Utc>>,
    #[serde(with = "super::strict_optional_utc")]
    pub settled_at: Option<DateTime<Utc>>,
}
schema_value!(BudgetReservation, "proof-operator-budget-reservation/v1");
impl BudgetReservation {
    pub fn validate(&self) -> Result<(), OperatorValidationError> {
        self.intent.validate()?;
        if self.schema != Self::SCHEMA
            || ![
                self.reservation_id,
                self.budget_id,
                self.run_id,
                self.lease_id,
                self.idempotency_key,
            ]
            .into_iter()
            .all(uuid_v7)
            || self.fence_epoch == 0
            || self.fence_epoch > super::MAX_SAFE_INTEGER
            || self.kind != self.intent.kind
            || (self.kind == BoundaryKind::Provider && self.replay.is_some())
            || !self.reserved.is_safe()
            || self.reserved != self.intent.ceiling
            || !self.charged.is_safe()
            || !self.charged.fits_within(&self.reserved)
            || self.permit_id.is_some_and(|id| !uuid_v7(id))
            || self
                .replay
                .as_ref()
                .is_some_and(|value| value.validate().is_err())
            || self
                .recovery
                .as_ref()
                .is_some_and(|value| value.validate().is_err())
        {
            return Err(OperatorValidationError::InvalidBranch);
        }
        let expected_intent_digest =
            super::control_digest_serialized("Proof-Operator-Dispatch-Intent-v1", &self.intent)
                .map_err(|_| OperatorValidationError::InvalidSchema)?;
        if self.intent_digest != expected_intent_digest
            || self.recovery.as_ref().is_some_and(|recovery| {
                recovery.run_id != self.run_id
                    || recovery.intent_digest != self.intent_digest
                    || recovery.replay != self.replay
            })
        {
            return Err(OperatorValidationError::InvalidSchema);
        }

        let zero = BudgetAmounts::default();
        let pre_dispatch_empty = self.permit_id.is_none()
            && self.dispatch_token_digest.is_none()
            && self.call_digest.is_none()
            && self.prepared_execution_digest.is_none()
            && self.result_digest.is_none()
            && self.prepared_binding.is_none()
            && self.runtime_commit.is_none()
            && self.dispatch_started_at.is_none();
        let dispatch_identity = self.permit_id.is_some()
            && self.dispatch_token_digest.is_some()
            && self.call_digest.is_some()
            && self.dispatch_started_at.is_some();
        let branch_valid = match self.state {
            BudgetReservationState::Reserved => {
                self.charged == zero && pre_dispatch_empty && self.settled_at.is_none()
            }
            BudgetReservationState::Released => {
                self.charged == zero && pre_dispatch_empty && self.settled_at.is_some()
            }
            BudgetReservationState::Dispatching => {
                self.charged == zero
                    && dispatch_identity
                    && self.prepared_execution_digest.is_none()
                    && self.result_digest.is_none()
                    && self.prepared_binding.is_none()
                    && self.runtime_commit.is_none()
                    && self.settled_at.is_none()
            }
            BudgetReservationState::Forfeited => {
                self.charged == self.reserved
                    && dispatch_identity
                    && self.prepared_execution_digest.is_none()
                    && self.result_digest.is_none()
                    && self.prepared_binding.is_none()
                    && self.runtime_commit.is_none()
                    && self.settled_at.is_some()
            }
            BudgetReservationState::Committed => {
                dispatch_identity
                    && self.prepared_execution_digest.is_some()
                    && self.result_digest.is_some()
                    && self.prepared_binding.is_some()
                    && self.runtime_commit.is_some()
                    && self.settled_at.is_some()
            }
        };
        if !branch_valid
            || self
                .dispatch_started_at
                .is_some_and(|at| at < self.created_at)
            || self.settled_at.is_some_and(|at| {
                at < self.created_at || self.dispatch_started_at.is_some_and(|start| at < start)
            })
        {
            return Err(OperatorValidationError::InvalidBranch);
        }
        if let Some(commit) = &self.runtime_commit {
            commit.validate()?;
            let prepared = self
                .prepared_binding
                .as_ref()
                .ok_or(OperatorValidationError::InvalidBranch)?;
            prepared
                .validate()
                .map_err(|_| OperatorValidationError::InvalidBranch)?;
            if commit.permit.permit_id != self.permit_id.expect("committed branch")
                || commit.permit.reservation_id != self.reservation_id
                || commit.permit.run_id != self.run_id
                || commit.permit.lease_id != self.lease_id
                || commit.permit.fence_epoch != self.fence_epoch
                || commit.permit.intent_digest != self.intent_digest
                || commit.permit.call_digest != self.call_digest.expect("committed branch")
                || commit.actual_charge != self.charged
                || Some(commit.prepared_execution_digest) != self.prepared_execution_digest
                || Some(commit.result_digest) != self.result_digest
                || commit.prepared_execution_digest != prepared.payload_digest
                || commit.result_digest != prepared.result_digest
            {
                return Err(OperatorValidationError::InvalidBranch);
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeCommit {
    pub schema: String,
    pub permit: DispatchPermit,
    #[serde(with = "super::strict_safe_integer")]
    pub expected_run_revision: u64,
    #[serde(with = "super::strict_safe_integer")]
    pub expected_step_revision: u64,
    #[serde(with = "super::strict_uuid_v7")]
    pub expected_checkpoint_id: Uuid,
    #[serde(with = "super::strict_safe_integer")]
    pub expected_checkpoint_sequence: u64,
    pub expected_checkpoint_digest: ContentDigest,
    pub actual_charge: BudgetAmounts,
    pub prepared_execution_digest: ControlDigest,
    pub result_digest: ControlDigest,
    #[serde(with = "super::strict_utc")]
    pub committed_at: DateTime<Utc>,
}
schema_value!(RuntimeCommit, "proof.operator.runtime-commit/v1");
impl RuntimeCommit {
    pub fn validate(&self) -> Result<(), OperatorValidationError> {
        if self.schema != Self::SCHEMA
            || self.permit.validate().is_err()
            || self.expected_run_revision > super::MAX_SAFE_INTEGER
            || self.expected_step_revision > super::MAX_SAFE_INTEGER
            || !uuid_v7(self.expected_checkpoint_id)
            || self.expected_checkpoint_sequence > super::MAX_SAFE_INTEGER
            || !self.actual_charge.is_safe()
            || self.committed_at < self.permit.authorized_at
        {
            return Err(OperatorValidationError::InvalidBranch);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalBinding {
    pub schema: String,
    #[serde(with = "super::strict_uuid_v7")]
    pub approval_request_id: Uuid,
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
    #[serde(with = "super::strict_uuid_v7")]
    pub required_human_id: Uuid,
    pub input_digest: ContentDigest,
    pub review_fields: Vec<ReviewField>,
    pub consequence: PendingConsequence,
    pub argument_digest: ControlDigest,
    pub consequence_digest: ControlDigest,
    #[serde(with = "super::strict_utc")]
    pub created_at: DateTime<Utc>,
    pub binding_digest: ControlDigest,
}
schema_value!(ApprovalBinding, "proof-operator-approval-binding/v1");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewFieldClassification {
    Identifier,
    Public,
    Secret,
    Sensitive,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewField {
    pub name: String,
    pub classification: ReviewFieldClassification,
    pub display_value: String,
    pub input_digest: ContentDigest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsequenceClassification {
    ExternalEffect,
    GovernedWrite,
    ProviderCall,
    ToolCall,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingConsequenceBody {
    pub classification: ConsequenceClassification,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingConsequence {
    pub classification: ConsequenceClassification,
    pub summary: String,
    pub consequence_digest: ControlDigest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryDirective {
    pub schema: String,
    #[serde(with = "super::strict_uuid_v7")]
    pub directive_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub workspace_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub run_id: Uuid,
    pub classification: RecoveryClassification,
    #[serde(with = "super::strict_uuid_v7")]
    pub source_lease_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub source_reservation_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub source_budget_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub source_idempotency_key: Uuid,
    pub source_request_digest: ControlDigest,
    #[serde(with = "super::strict_uuid_v7")]
    pub checkpoint_id: Uuid,
    #[serde(with = "super::strict_safe_integer")]
    pub checkpoint_sequence: u64,
    pub checkpoint_digest: ContentDigest,
    #[serde(with = "super::strict_safe_integer")]
    pub source_fence_epoch: u64,
    #[serde(with = "super::strict_safe_integer")]
    pub source_control_revision: u64,
    pub intent_digest: ControlDigest,
    pub replay: Option<ReplayClaimBinding>,
    pub required_budget_disposition: RecoveryBudgetDisposition,
    #[serde(with = "super::strict_utc")]
    pub created_at: DateTime<Utc>,
    pub directive_digest: ControlDigest,
}
schema_value!(RecoveryDirective, "proof.operator.recovery-directive/v1");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryClassification {
    PreDispatchRecoverable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryBudgetDisposition {
    None,
}

impl RecoveryDirective {
    pub fn validate(&self) -> Result<(), OperatorValidationError> {
        if self.schema != Self::SCHEMA
            || self.classification != RecoveryClassification::PreDispatchRecoverable
            || self.required_budget_disposition != RecoveryBudgetDisposition::None
            || ![
                self.directive_id,
                self.workspace_id,
                self.run_id,
                self.source_lease_id,
                self.source_reservation_id,
                self.source_budget_id,
                self.source_idempotency_key,
                self.checkpoint_id,
            ]
            .into_iter()
            .all(super::uuid_is_v7)
            || self.checkpoint_sequence > super::MAX_SAFE_INTEGER
            || self.source_fence_epoch == 0
            || self.source_fence_epoch > super::MAX_SAFE_INTEGER
            || self.source_control_revision > super::MAX_SAFE_INTEGER
            || self.replay.as_ref().is_some_and(|binding| {
                binding.validate().is_err()
                    || binding.workspace_id != self.workspace_id
                    || binding.run_id != self.run_id
                    || binding.checkpoint_id != self.checkpoint_id
                    || binding.checkpoint_sequence != self.checkpoint_sequence
                    || binding.checkpoint_digest != self.checkpoint_digest
            })
        {
            return Err(OperatorValidationError::InvalidBranch);
        }
        if self.directive_digest
            != digest_without_field(
                "Proof-Operator-Recovery-Directive-v1",
                self,
                "directive_digest",
            )?
        {
            return Err(OperatorValidationError::InvalidSchema);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunProjection {
    pub schema: String,
    #[serde(with = "super::strict_uuid_v7")]
    pub projection_id: Uuid,
    #[serde(with = "super::strict_safe_integer")]
    pub projection_sequence: u64,
    #[serde(with = "super::strict_safe_integer")]
    pub projection_revision: u64,
    #[serde(with = "super::strict_uuid_v7")]
    pub workspace_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub run_id: Uuid,
    #[serde(with = "super::strict_safe_integer")]
    pub source_run_revision: u64,
    #[serde(with = "super::strict_safe_integer")]
    pub source_control_revision: u64,
    #[serde(with = "super::strict_uuid_v7")]
    pub checkpoint_id: Uuid,
    #[serde(with = "super::strict_safe_integer")]
    pub checkpoint_sequence: u64,
    pub checkpoint_digest: ContentDigest,
    #[serde(with = "super::strict_safe_integer")]
    pub fence_epoch: u64,
    pub run_status: AgentRunStatus,
    pub attention: super::AttentionState,
    #[serde(with = "super::strict_optional_uuid_v7")]
    pub required_human_id: Option<Uuid>,
    #[serde(with = "super::strict_optional_uuid_v7")]
    pub approval_request_id: Option<Uuid>,
    #[serde(with = "super::strict_optional_uuid_v7")]
    pub recovery_directive_id: Option<Uuid>,
    pub recovery_directive_digest: Option<ControlDigest>,
    #[serde(with = "super::strict_utc")]
    pub projected_at: DateTime<Utc>,
    pub snapshot_digest: ControlDigest,
}
schema_value!(RunProjection, "proof-operator-run-projection/v1");
impl RunProjection {
    pub fn validate(&self) -> Result<(), OperatorValidationError> {
        if self.schema != Self::SCHEMA
            || self.projection_sequence == 0
            || [
                self.projection_sequence,
                self.projection_revision,
                self.source_run_revision,
                self.source_control_revision,
                self.checkpoint_sequence,
                self.fence_epoch,
            ]
            .into_iter()
            .any(|value| value > super::MAX_SAFE_INTEGER)
            || ![
                self.projection_id,
                self.workspace_id,
                self.run_id,
                self.checkpoint_id,
            ]
            .into_iter()
            .all(super::uuid_is_v7)
            || self
                .required_human_id
                .into_iter()
                .chain(self.approval_request_id)
                .chain(self.recovery_directive_id)
                .any(|id| !super::uuid_is_v7(id))
        {
            return Err(OperatorValidationError::InvalidRevision);
        }
        let approval = self.required_human_id.is_some()
            && self.approval_request_id.is_some()
            && self.recovery_directive_id.is_none()
            && self.recovery_directive_digest.is_none();
        let recovery = self.required_human_id.is_none()
            && self.approval_request_id.is_none()
            && self.recovery_directive_id.is_some()
            && self.recovery_directive_digest.is_some();
        let clear = self.required_human_id.is_none()
            && self.approval_request_id.is_none()
            && self.recovery_directive_id.is_none()
            && self.recovery_directive_digest.is_none();
        let valid = match self.attention {
            super::AttentionState::AwaitingDecision => {
                self.run_status == AgentRunStatus::WaitingForInput && approval
            }
            super::AttentionState::Recoverable => {
                self.run_status == AgentRunStatus::Failed && recovery
            }
            super::AttentionState::Running => {
                matches!(
                    self.run_status,
                    AgentRunStatus::Queued | AgentRunStatus::Running
                ) && clear
            }
            super::AttentionState::Terminal => {
                matches!(
                    self.run_status,
                    AgentRunStatus::Cancelled | AgentRunStatus::Failed | AgentRunStatus::Succeeded
                ) && clear
            }
        };
        if !valid {
            return Err(OperatorValidationError::InvalidBranch);
        }
        if self.snapshot_digest
            != digest_without_field("Proof-Operator-Run-Projection-v1", self, "snapshot_digest")?
        {
            return Err(OperatorValidationError::InvalidSchema);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditEventKind {
    ApprovalDecided,
    ApprovalExpired,
    BudgetCommitted,
    BudgetForfeited,
    BudgetRejected,
    BudgetReleased,
    BudgetReserved,
    CommandConflict,
    CommandRejected,
    ControlFailure,
    ControlShutdown,
    DispatchAuthorized,
    LeaseAcquired,
    LeaseReclaimed,
    LeaseReleased,
    LeaseRenewed,
    RecoveryCompleted,
    RecoveryStarted,
    RunCancelled,
    RunResumed,
    RuntimeResultCommitted,
    SessionChallengeIssued,
    SessionExpired,
    SessionIssued,
    SessionReplaced,
    SessionRevoked,
    StaleFenceRejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditOutcome {
    Accepted,
    Conflict,
    Expired,
    Failed,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditFailureScope {
    Command,
    Runtime,
    Storage,
    Workspace,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProofReference {
    #[serde(with = "super::strict_uuid_v7")]
    pub proof_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub actor_id: Uuid,
    pub operation: String,
    pub proof_digest: ContentDigest,
}
impl ProofReference {
    pub fn validate(&self) -> Result<(), OperatorValidationError> {
        if !uuid_v7(self.proof_id)
            || !uuid_v7(self.actor_id)
            || !super::valid_operation_name(&self.operation)
        {
            return Err(OperatorValidationError::InvalidSchema);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditEvent {
    pub schema: String,
    #[serde(with = "super::strict_uuid_v7")]
    pub workspace_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub event_id: Uuid,
    #[serde(with = "super::strict_safe_integer")]
    pub sequence: u64,
    pub kind: AuditEventKind,
    pub outcome: AuditOutcome,
    pub previous_digest: Option<ControlDigest>,
    pub event_digest: ControlDigest,
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
    #[serde(with = "super::strict_optional_uuid_v7")]
    pub server_instance_id: Option<Uuid>,
    #[serde(with = "super::strict_optional_uuid_v7")]
    pub run_id: Option<Uuid>,
    #[serde(with = "super::strict_optional_uuid_v7")]
    pub approval_request_id: Option<Uuid>,
    #[serde(with = "super::strict_optional_uuid_v7")]
    pub command_id: Option<Uuid>,
    pub command_kind: Option<CommandKind>,
    #[serde(with = "super::strict_optional_uuid_v7")]
    pub budget_id: Option<Uuid>,
    #[serde(with = "super::strict_optional_uuid_v7")]
    pub reservation_id: Option<Uuid>,
    #[serde(with = "super::strict_optional_uuid_v7")]
    pub lease_id: Option<Uuid>,
    #[serde(with = "super::strict_optional_uuid_v7")]
    pub source_lease_id: Option<Uuid>,
    #[serde(with = "super::strict_optional_uuid_v7")]
    pub process_epoch_id: Option<Uuid>,
    #[serde(with = "super::strict_optional_uuid_v7")]
    pub permit_id: Option<Uuid>,
    #[serde(with = "super::strict_optional_uuid_v7")]
    pub recovery_directive_id: Option<Uuid>,
    #[serde(with = "super::strict_optional_safe_integer")]
    pub fence_epoch: Option<u64>,
    #[serde(with = "super::strict_optional_safe_integer")]
    pub auth_epoch: Option<u64>,
    #[serde(with = "super::strict_optional_safe_integer")]
    pub policy_revision: Option<u64>,
    pub intent_digest: Option<ControlDigest>,
    pub call_digest: Option<ControlDigest>,
    pub decision_digest: Option<ContentDigest>,
    pub recovery_directive_digest: Option<ControlDigest>,
    pub failure_scope: Option<AuditFailureScope>,
    pub proof: Option<ProofReference>,
    #[serde(with = "super::strict_utc")]
    pub occurred_at: DateTime<Utc>,
}
schema_value!(AuditEvent, "proof.operator.audit-event/v1");
impl AuditEvent {
    pub fn validate_chain_link(
        &self,
        expected_sequence: u64,
        expected_previous: Option<ControlDigest>,
    ) -> Result<(), OperatorValidationError> {
        if self.schema != Self::SCHEMA
            || self.sequence != expected_sequence
            || self.sequence == 0
            || self.previous_digest != expected_previous
            || (self.sequence == 1) != self.previous_digest.is_none()
        {
            return Err(OperatorValidationError::InvalidChronology);
        }
        if !super::uuid_is_v7(self.workspace_id)
            || !super::uuid_is_v7(self.event_id)
            || [
                self.human_id,
                self.session_id,
                self.challenge_id,
                self.related_session_id,
                self.server_instance_id,
                self.run_id,
                self.approval_request_id,
                self.command_id,
                self.budget_id,
                self.reservation_id,
                self.lease_id,
                self.source_lease_id,
                self.process_epoch_id,
                self.permit_id,
                self.recovery_directive_id,
            ]
            .into_iter()
            .flatten()
            .any(|id| !super::uuid_is_v7(id))
            || [self.fence_epoch, self.auth_epoch, self.policy_revision]
                .into_iter()
                .flatten()
                .any(|value| value > super::MAX_SAFE_INTEGER)
            || [self.auth_epoch, self.policy_revision]
                .into_iter()
                .flatten()
                .any(|value| value == 0)
            || self
                .proof
                .as_ref()
                .is_some_and(|proof| proof.validate().is_err())
        {
            return Err(OperatorValidationError::InvalidUuid);
        }
        if self.session_id.is_some() != self.session_authority_digest.is_some() {
            return Err(OperatorValidationError::InvalidBranch);
        }
        let challenge_kind = matches!(
            self.kind,
            AuditEventKind::SessionChallengeIssued
                | AuditEventKind::SessionIssued
                | AuditEventKind::SessionReplaced
        );
        if challenge_kind != self.challenge_digest.is_some() {
            return Err(OperatorValidationError::InvalidBranch);
        }
        let mask = self.profile_mask();
        let profile = match self.kind {
            AuditEventKind::SessionChallengeIssued => {
                self.outcome == AuditOutcome::Accepted && mask == 0x60015
            }
            AuditEventKind::SessionIssued => {
                self.outcome == AuditOutcome::Accepted && mask == 0x60017
            }
            AuditEventKind::SessionReplaced => {
                self.outcome == AuditOutcome::Accepted && mask == 0x6001f
            }
            AuditEventKind::SessionRevoked => {
                self.outcome == AuditOutcome::Accepted
                    && mask == 0x1060193
                    && self.command_kind == Some(CommandKind::SessionRevoke)
                    && self.proof_operation_is("operator.session_revoke")
            }
            AuditEventKind::SessionExpired => {
                self.outcome == AuditOutcome::Expired && mask == 0x60013
            }
            AuditEventKind::ApprovalDecided => {
                self.outcome == AuditOutcome::Accepted
                    && mask == 0x12001e3
                    && self.command_kind == Some(CommandKind::ApprovalDecide)
                    && self.proof_operation_is("operator.approval_decide")
            }
            AuditEventKind::ApprovalExpired => {
                self.outcome == AuditOutcome::Expired && mask == 0x60
            }
            AuditEventKind::RunCancelled => {
                self.outcome == AuditOutcome::Accepted
                    && mask == 0x10001a3
                    && self.command_kind == Some(CommandKind::RunCancel)
                    && self.proof_operation_is("operator.run_cancel")
            }
            AuditEventKind::RunResumed => {
                self.outcome == AuditOutcome::Accepted
                    && matches!(mask, 0x12001e3 | 0x141a9a3)
                    && self.command_kind == Some(CommandKind::RunResume)
                    && self.proof_operation_is("operator.run_resume")
            }
            AuditEventKind::LeaseAcquired | AuditEventKind::LeaseRenewed => {
                self.outcome == AuditOutcome::Accepted && mask == 0x12830
            }
            AuditEventKind::LeaseReclaimed => {
                self.outcome == AuditOutcome::Accepted
                    && matches!(mask, 0x13830 | 0x49b830 | 0x49bc30)
            }
            AuditEventKind::LeaseReleased => {
                self.outcome == AuditOutcome::Accepted && mask == 0x12830
            }
            AuditEventKind::StaleFenceRejected => {
                self.outcome == AuditOutcome::Rejected && mask == 0x16c30
            }
            AuditEventKind::BudgetReserved | AuditEventKind::BudgetReleased => {
                self.outcome == AuditOutcome::Accepted && mask == 0x90e20
            }
            AuditEventKind::BudgetCommitted => {
                self.outcome == AuditOutcome::Accepted && mask == 0x194e20
            }
            AuditEventKind::BudgetForfeited => {
                self.outcome == AuditOutcome::Failed && mask == 0x194e20
            }
            AuditEventKind::BudgetRejected => {
                self.outcome == AuditOutcome::Rejected && mask == 0x90e20
            }
            AuditEventKind::DispatchAuthorized => {
                self.outcome == AuditOutcome::Accepted && mask == 0x196e30
            }
            AuditEventKind::RuntimeResultCommitted => {
                self.outcome == AuditOutcome::Accepted && mask == 0x1196e30
            }
            AuditEventKind::RecoveryStarted => {
                self.outcome == AuditOutcome::Accepted && mask == 0x499430
            }
            AuditEventKind::RecoveryCompleted => {
                self.outcome == AuditOutcome::Accepted && mask == 0x49bc30
            }
            AuditEventKind::ControlShutdown => {
                self.outcome == AuditOutcome::Accepted && mask == 0x10
            }
            AuditEventKind::CommandRejected => {
                self.outcome == AuditOutcome::Rejected && self.valid_command_profile(mask)
            }
            AuditEventKind::CommandConflict => {
                self.outcome == AuditOutcome::Conflict && self.valid_command_profile(mask)
            }
            AuditEventKind::ControlFailure => self.valid_failure_profile(mask),
        };
        if !profile {
            return Err(OperatorValidationError::InvalidBranch);
        }
        let mut digest_value =
            serde_json::to_value(self).map_err(|_| OperatorValidationError::InvalidSchema)?;
        digest_value
            .as_object_mut()
            .ok_or(OperatorValidationError::InvalidSchema)?
            .remove("event_digest");
        let expected =
            super::control_digest_serialized("Proof-Operator-Audit-Event-v1", &digest_value)
                .map_err(|_| OperatorValidationError::InvalidSchema)?;
        if expected != self.event_digest {
            return Err(OperatorValidationError::InvalidSchema);
        }
        Ok(())
    }

    fn profile_mask(&self) -> u32 {
        let present = [
            self.human_id.is_some(),
            self.session_id.is_some(),
            self.challenge_id.is_some(),
            self.related_session_id.is_some(),
            self.server_instance_id.is_some(),
            self.run_id.is_some(),
            self.approval_request_id.is_some(),
            self.command_id.is_some(),
            self.command_kind.is_some(),
            self.budget_id.is_some(),
            self.reservation_id.is_some(),
            self.lease_id.is_some(),
            self.source_lease_id.is_some(),
            self.process_epoch_id.is_some(),
            self.permit_id.is_some(),
            self.recovery_directive_id.is_some(),
            self.fence_epoch.is_some(),
            self.auth_epoch.is_some(),
            self.policy_revision.is_some(),
            self.intent_digest.is_some(),
            self.call_digest.is_some(),
            self.decision_digest.is_some(),
            self.recovery_directive_digest.is_some(),
            self.failure_scope.is_some(),
            self.proof.is_some(),
        ];
        present
            .iter()
            .enumerate()
            .fold(0_u32, |mask, (index, yes)| {
                mask | (u32::from(*yes) << index)
            })
    }

    fn proof_operation_is(&self, operation: &str) -> bool {
        self.proof
            .as_ref()
            .is_some_and(|proof| proof.operation == operation)
    }

    fn valid_command_profile(&self, mask: u32) -> bool {
        matches!(
            (self.command_kind, mask),
            (Some(CommandKind::ApprovalDecide), 0x1e3)
                | (Some(CommandKind::RunCancel), 0x1a3)
                | (Some(CommandKind::RunResume), 0x2001e3)
                | (Some(CommandKind::RunResume), 0x4081a3)
                | (Some(CommandKind::SessionRevoke), 0x183)
        )
    }

    fn valid_failure_profile(&self, mask: u32) -> bool {
        if self.outcome != AuditOutcome::Failed {
            return false;
        }
        matches!(
            (self.failure_scope, self.command_kind, mask),
            (Some(AuditFailureScope::Workspace), None, 0x800010)
                | (Some(AuditFailureScope::Storage), None, 0x800010)
                | (
                    Some(AuditFailureScope::Command),
                    Some(CommandKind::ApprovalDecide),
                    0x8001f3
                )
                | (
                    Some(AuditFailureScope::Command),
                    Some(CommandKind::RunCancel),
                    0x8001b3
                )
                | (
                    Some(AuditFailureScope::Command),
                    Some(CommandKind::RunResume),
                    0xe081f3
                )
                | (
                    Some(AuditFailureScope::Command),
                    Some(CommandKind::SessionRevoke),
                    0x800193
                )
                | (Some(AuditFailureScope::Runtime), None, 0x996e30)
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandBinding {
    #[serde(with = "super::strict_uuid_v7")]
    pub command_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub idempotency_key: Uuid,
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
    pub session_authority_digest: ControlDigest,
    #[serde(with = "super::strict_safe_integer")]
    pub policy_revision: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionOutcome {
    Approved,
    Denied,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalDecisionCommand {
    pub schema: String,
    pub binding: CommandBinding,
    #[serde(with = "super::strict_uuid_v7")]
    pub approval_request_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub run_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub step_id: Uuid,
    pub outcome: DecisionOutcome,
    pub expected_request_digest: ContentDigest,
    #[serde(with = "super::strict_safe_integer")]
    pub expected_run_revision: u64,
    #[serde(with = "super::strict_safe_integer")]
    pub expected_step_revision: u64,
    #[serde(with = "super::strict_safe_integer")]
    pub expected_control_revision: u64,
    #[serde(with = "super::strict_uuid_v7")]
    pub expected_checkpoint_id: Uuid,
    #[serde(with = "super::strict_safe_integer")]
    pub expected_checkpoint_sequence: u64,
    pub expected_checkpoint_digest: ContentDigest,
    #[serde(with = "super::strict_safe_integer")]
    pub expected_fence_epoch: u64,
}
schema_value!(
    ApprovalDecisionCommand,
    "proof.operator.command.approval-decision/v1"
);
impl ApprovalDecisionCommand {
    pub fn validate(&self) -> Result<(), OperatorValidationError> {
        if self.schema != Self::SCHEMA || self.expected_fence_epoch == 0 {
            return Err(OperatorValidationError::InvalidRevision);
        }
        self.binding.validate()?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunCancelCommand {
    pub schema: String,
    pub binding: CommandBinding,
    #[serde(with = "super::strict_uuid_v7")]
    pub run_id: Uuid,
    #[serde(with = "super::strict_safe_integer")]
    pub expected_run_revision: u64,
    #[serde(with = "super::strict_safe_integer")]
    pub expected_control_revision: u64,
    #[serde(with = "super::strict_safe_integer")]
    pub expected_fence_epoch: u64,
}
schema_value!(RunCancelCommand, "proof.operator.command.run-cancel/v1");
impl RunCancelCommand {
    pub fn validate(&self) -> Result<(), OperatorValidationError> {
        if self.schema != Self::SCHEMA || self.expected_fence_epoch == 0 {
            return Err(OperatorValidationError::InvalidRevision);
        }
        self.binding.validate()?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunResumeCommand {
    pub schema: String,
    pub binding: CommandBinding,
    #[serde(with = "super::strict_uuid_v7")]
    pub run_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub step_id: Uuid,
    #[serde(with = "super::strict_optional_uuid_v7")]
    pub approval_request_id: Option<Uuid>,
    pub decision_digest: Option<ContentDigest>,
    #[serde(with = "super::strict_optional_uuid_v7")]
    pub recovery_directive_id: Option<Uuid>,
    pub recovery_directive_digest: Option<ControlDigest>,
    #[serde(with = "super::strict_safe_integer")]
    pub expected_run_revision: u64,
    #[serde(with = "super::strict_safe_integer")]
    pub expected_step_revision: u64,
    #[serde(with = "super::strict_safe_integer")]
    pub expected_control_revision: u64,
    #[serde(with = "super::strict_uuid_v7")]
    pub expected_checkpoint_id: Uuid,
    #[serde(with = "super::strict_safe_integer")]
    pub expected_checkpoint_sequence: u64,
    pub expected_checkpoint_digest: ContentDigest,
    #[serde(with = "super::strict_safe_integer")]
    pub expected_fence_epoch: u64,
}
schema_value!(RunResumeCommand, "proof.operator.command.run-resume/v1");
impl RunResumeCommand {
    pub fn validate(&self) -> Result<(), OperatorValidationError> {
        if self.schema != Self::SCHEMA || self.expected_fence_epoch == 0 {
            return Err(OperatorValidationError::InvalidRevision);
        }
        self.binding.validate()?;
        let approval = self.approval_request_id.is_some()
            && self.decision_digest.is_some()
            && self.recovery_directive_id.is_none()
            && self.recovery_directive_digest.is_none();
        let recovery = self.approval_request_id.is_none()
            && self.decision_digest.is_none()
            && self.recovery_directive_id.is_some()
            && self.recovery_directive_digest.is_some();
        if approval == recovery {
            return Err(OperatorValidationError::InvalidBranch);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionRevokeRequest {
    pub schema: String,
    pub binding: CommandBinding,
}
schema_value!(
    SessionRevokeRequest,
    "proof.operator.command.session-revoke/v1"
);

impl CommandBinding {
    pub fn validate(&self) -> Result<(), OperatorValidationError> {
        if ![
            self.command_id,
            self.idempotency_key,
            self.workspace_id,
            self.server_instance_id,
            self.session_id,
            self.human_id,
        ]
        .into_iter()
        .all(uuid_v7)
        {
            return Err(OperatorValidationError::InvalidUuid);
        }
        if self.auth_epoch != 1 || self.policy_revision != 1 {
            return Err(OperatorValidationError::InvalidRevision);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OperatorCommand {
    ApprovalDecision(ApprovalDecisionCommand),
    RunCancel(RunCancelCommand),
    RunResume(RunResumeCommand),
    SessionRevoke(SessionRevokeRequest),
}

impl OperatorCommand {
    pub fn binding(&self) -> &CommandBinding {
        match self {
            Self::ApprovalDecision(v) => &v.binding,
            Self::RunCancel(v) => &v.binding,
            Self::RunResume(v) => &v.binding,
            Self::SessionRevoke(v) => &v.binding,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandKind {
    ApprovalDecide,
    RunCancel,
    RunResume,
    SessionRevoke,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandOutcome {
    AlreadyTerminal,
    Applied,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandEnvelope {
    pub schema: String,
    pub command: OperatorCommand,
    pub request_digest: ControlDigest,
    pub required_capabilities: Vec<super::Capability>,
    #[serde(with = "super::strict_utc")]
    pub requested_at: DateTime<Utc>,
}
schema_value!(CommandEnvelope, "proof.operator.command-envelope/v1");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandReceipt {
    pub schema: String,
    #[serde(with = "super::strict_uuid_v7")]
    pub receipt_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub command_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub idempotency_key: Uuid,
    pub kind: CommandKind,
    pub outcome: CommandOutcome,
    pub request_digest: ControlDigest,
    #[serde(with = "super::strict_uuid_v7")]
    pub workspace_id: Uuid,
    #[serde(with = "super::strict_uuid_v7")]
    pub human_id: Uuid,
    #[serde(with = "super::strict_optional_uuid_v7")]
    pub target_run_id: Option<Uuid>,
    #[serde(with = "super::strict_optional_uuid_v7")]
    pub approval_request_id: Option<Uuid>,
    #[serde(with = "super::strict_optional_safe_integer")]
    pub observed_run_revision: Option<u64>,
    #[serde(with = "super::strict_optional_safe_integer")]
    pub resulting_run_revision: Option<u64>,
    #[serde(with = "super::strict_optional_safe_integer")]
    pub resulting_step_revision: Option<u64>,
    #[serde(with = "super::strict_optional_safe_integer")]
    pub resulting_control_revision: Option<u64>,
    #[serde(with = "super::strict_optional_safe_integer")]
    pub resulting_fence_epoch: Option<u64>,
    #[serde(with = "super::strict_optional_uuid_v7")]
    pub decision_id: Option<Uuid>,
    pub decision_digest: Option<ContentDigest>,
    pub proof: Option<ProofReference>,
    #[serde(with = "super::strict_uuid_v7")]
    pub audit_event_id: Uuid,
    #[serde(with = "super::strict_safe_integer")]
    pub audit_sequence: u64,
    pub audit_digest: ControlDigest,
    #[serde(with = "super::strict_utc")]
    pub completed_at: DateTime<Utc>,
    pub receipt_digest: ControlDigest,
}
schema_value!(CommandReceipt, "proof.operator.command-receipt/v1");
impl CommandReceipt {
    pub fn validate(&self) -> Result<(), OperatorValidationError> {
        if self.schema != Self::SCHEMA
            || self.audit_sequence == 0
            || self.audit_sequence > super::MAX_SAFE_INTEGER
            || ![
                self.receipt_id,
                self.command_id,
                self.idempotency_key,
                self.workspace_id,
                self.human_id,
                self.audit_event_id,
            ]
            .into_iter()
            .all(uuid_v7)
            || [
                self.target_run_id,
                self.approval_request_id,
                self.decision_id,
            ]
            .into_iter()
            .flatten()
            .any(|id| !uuid_v7(id))
            || [
                self.observed_run_revision,
                self.resulting_run_revision,
                self.resulting_step_revision,
                self.resulting_control_revision,
                self.resulting_fence_epoch,
            ]
            .into_iter()
            .flatten()
            .any(|revision| revision > super::MAX_SAFE_INTEGER)
        {
            return Err(OperatorValidationError::InvalidRevision);
        }
        if matches!(self.outcome, CommandOutcome::Applied) != self.proof.is_some() {
            return Err(OperatorValidationError::InvalidBranch);
        }
        if self.decision_id.is_some() != self.decision_digest.is_some() {
            return Err(OperatorValidationError::InvalidBranch);
        }
        let proof_operation = self.proof.as_ref().map(|proof| proof.operation.as_str());
        let run_base = self.target_run_id.is_some()
            && self.observed_run_revision.is_some()
            && self.resulting_run_revision.is_some()
            && self.resulting_control_revision.is_some()
            && self.resulting_fence_epoch.is_some();
        let branch = match self.kind {
            CommandKind::ApprovalDecide => {
                self.outcome == CommandOutcome::Applied
                    && run_base
                    && self.approval_request_id.is_some()
                    && self.resulting_step_revision.is_some()
                    && self.decision_id.is_some()
                    && proof_operation == Some("operator.approval_decide")
            }
            CommandKind::RunCancel => {
                run_base
                    && self.approval_request_id.is_none()
                    && self.resulting_step_revision.is_none()
                    && self.decision_id.is_none()
                    && (self.outcome == CommandOutcome::AlreadyTerminal
                        || proof_operation == Some("operator.run_cancel"))
                    && (self.outcome != CommandOutcome::AlreadyTerminal
                        || self.resulting_run_revision == self.observed_run_revision)
            }
            CommandKind::RunResume => {
                self.outcome == CommandOutcome::Applied
                    && run_base
                    && self.resulting_step_revision.is_some()
                    && (self.approval_request_id.is_some() == self.decision_id.is_some())
                    && proof_operation == Some("operator.run_resume")
            }
            CommandKind::SessionRevoke => {
                self.outcome == CommandOutcome::Applied
                    && self.target_run_id.is_none()
                    && self.approval_request_id.is_none()
                    && self.observed_run_revision.is_none()
                    && self.resulting_run_revision.is_none()
                    && self.resulting_step_revision.is_none()
                    && self.resulting_control_revision.is_none()
                    && self.resulting_fence_epoch.is_none()
                    && self.decision_id.is_none()
                    && proof_operation == Some("operator.session_revoke")
            }
        };
        if !branch {
            return Err(OperatorValidationError::InvalidBranch);
        }
        if self.receipt_digest
            != digest_without_field("Proof-Operator-Command-Receipt-v1", self, "receipt_digest")?
        {
            return Err(OperatorValidationError::InvalidSchema);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlTransitionOutcome {
    pub schema: String,
    #[serde(with = "super::strict_uuid_v7")]
    pub command_id: Uuid,
    pub kind: CommandKind,
    pub outcome: AppliedCommandOutcome,
    pub proof_operation: OperatorProofOperation,
    #[serde(with = "super::strict_optional_uuid_v7")]
    pub target_run_id: Option<Uuid>,
    #[serde(with = "super::strict_optional_uuid_v7")]
    pub approval_request_id: Option<Uuid>,
    #[serde(with = "super::strict_optional_safe_integer")]
    pub resulting_run_revision: Option<u64>,
    #[serde(with = "super::strict_optional_safe_integer")]
    pub resulting_step_revision: Option<u64>,
    #[serde(with = "super::strict_optional_safe_integer")]
    pub resulting_control_revision: Option<u64>,
    #[serde(with = "super::strict_optional_safe_integer")]
    pub resulting_fence_epoch: Option<u64>,
    pub decision_digest: Option<ContentDigest>,
    #[serde(with = "super::strict_utc")]
    pub completed_at: DateTime<Utc>,
}
schema_value!(
    ControlTransitionOutcome,
    "proof.operator.control-transition-outcome/v1"
);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppliedCommandOutcome {
    Applied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperatorProofOperation {
    #[serde(rename = "operator.approval_decide")]
    ApprovalDecide,
    #[serde(rename = "operator.run_cancel")]
    RunCancel,
    #[serde(rename = "operator.run_resume")]
    RunResume,
    #[serde(rename = "operator.session_revoke")]
    SessionRevoke,
}

impl ControlTransitionOutcome {
    pub fn validate(&self) -> Result<(), OperatorValidationError> {
        if self.schema != Self::SCHEMA
            || !uuid_v7(self.command_id)
            || [self.target_run_id, self.approval_request_id]
                .into_iter()
                .flatten()
                .any(|id| !uuid_v7(id))
            || [
                self.resulting_run_revision,
                self.resulting_step_revision,
                self.resulting_control_revision,
                self.resulting_fence_epoch,
            ]
            .into_iter()
            .flatten()
            .any(|revision| revision > super::MAX_SAFE_INTEGER)
        {
            return Err(OperatorValidationError::InvalidRevision);
        }
        let all_run = self.target_run_id.is_some()
            && self.resulting_run_revision.is_some()
            && self.resulting_control_revision.is_some()
            && self.resulting_fence_epoch.is_some();
        let valid = match self.kind {
            CommandKind::ApprovalDecide => {
                self.proof_operation == OperatorProofOperation::ApprovalDecide
                    && all_run
                    && self.approval_request_id.is_some()
                    && self.resulting_step_revision.is_some()
                    && self.decision_digest.is_some()
            }
            CommandKind::RunCancel => {
                self.proof_operation == OperatorProofOperation::RunCancel
                    && all_run
                    && self.approval_request_id.is_none()
                    && self.resulting_step_revision.is_none()
                    && self.decision_digest.is_none()
            }
            CommandKind::RunResume => {
                self.proof_operation == OperatorProofOperation::RunResume
                    && all_run
                    && self.resulting_step_revision.is_some()
                    && (self.approval_request_id.is_some() == self.decision_digest.is_some())
            }
            CommandKind::SessionRevoke => {
                self.proof_operation == OperatorProofOperation::SessionRevoke
                    && self.target_run_id.is_none()
                    && self.approval_request_id.is_none()
                    && self.resulting_run_revision.is_none()
                    && self.resulting_step_revision.is_none()
                    && self.resulting_control_revision.is_none()
                    && self.resulting_fence_epoch.is_none()
                    && self.decision_digest.is_none()
            }
        };
        if !valid {
            return Err(OperatorValidationError::InvalidBranch);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u16) -> Uuid {
        Uuid::parse_str(&format!("01890f47-9bcd-7def-8123-456789ab{n:04x}")).unwrap()
    }
    fn control(n: u8) -> ControlDigest {
        ControlDigest::from_bytes([n; 32])
    }
    fn content(n: u8) -> ContentDigest {
        ContentDigest::from_bytes([n; 32])
    }
    fn amounts(value: u64) -> BudgetAmounts {
        BudgetAmounts {
            steps: value,
            tokens: value,
            duration_ms: value,
            cost_microusd: value,
            tool_dispatches: value,
        }
    }
    fn binding() -> CommandBinding {
        CommandBinding {
            command_id: id(1),
            idempotency_key: id(2),
            workspace_id: id(3),
            server_instance_id: id(4),
            session_id: id(5),
            human_id: id(6),
            auth_epoch: 1,
            session_authority_digest: control(1),
            policy_revision: 1,
        }
    }

    fn audit_for_profile(
        kind: AuditEventKind,
        outcome: AuditOutcome,
        mask: u32,
        command_kind: Option<CommandKind>,
        proof_operation: &str,
        failure_scope: Option<AuditFailureScope>,
    ) -> AuditEvent {
        let uuid_field = |bit: u32| ((mask >> bit) & 1 == 1).then(|| id(bit as u16 + 20));
        let digest_field = |bit: u32| ((mask >> bit) & 1 == 1).then(|| control(bit as u8));
        let content_field = |bit: u32| ((mask >> bit) & 1 == 1).then(|| content(bit as u8));
        let safe_field = |bit: u32| ((mask >> bit) & 1 == 1).then_some(1);
        let session_id = uuid_field(1);
        let challenge_kind = matches!(
            kind,
            AuditEventKind::SessionChallengeIssued
                | AuditEventKind::SessionIssued
                | AuditEventKind::SessionReplaced
        );
        let mut event = AuditEvent {
            schema: AuditEvent::SCHEMA.into(),
            workspace_id: id(10),
            event_id: id(11),
            sequence: 1,
            kind,
            outcome,
            previous_digest: None,
            event_digest: control(0),
            human_id: uuid_field(0),
            session_id,
            challenge_id: uuid_field(2),
            challenge_digest: challenge_kind.then(|| control(30)),
            session_authority_digest: session_id.map(|_| control(31)),
            related_session_id: uuid_field(3),
            server_instance_id: uuid_field(4),
            run_id: uuid_field(5),
            approval_request_id: uuid_field(6),
            command_id: uuid_field(7),
            command_kind,
            budget_id: uuid_field(9),
            reservation_id: uuid_field(10),
            lease_id: uuid_field(11),
            source_lease_id: uuid_field(12),
            process_epoch_id: uuid_field(13),
            permit_id: uuid_field(14),
            recovery_directive_id: uuid_field(15),
            fence_epoch: safe_field(16),
            auth_epoch: safe_field(17),
            policy_revision: safe_field(18),
            intent_digest: digest_field(19),
            call_digest: digest_field(20),
            decision_digest: content_field(21),
            recovery_directive_digest: digest_field(22),
            failure_scope,
            proof: ((mask >> 24) & 1 == 1).then(|| ProofReference {
                proof_id: id(60),
                actor_id: id(61),
                operation: proof_operation.into(),
                proof_digest: content(25),
            }),
            occurred_at: "2030-01-01T00:00:00Z".parse().unwrap(),
        };
        event.event_digest =
            digest_without_field("Proof-Operator-Audit-Event-v1", &event, "event_digest").unwrap();
        event
    }

    #[test]
    fn workspace_enrollment_and_session_validate_exact_identity_digests() {
        let now: DateTime<Utc> = "2030-01-01T00:00:00Z".parse().unwrap();
        let agent_key = URL_SAFE_NO_PAD.encode([1_u8; 32]);
        let human_key = URL_SAFE_NO_PAD.encode([2_u8; 32]);
        let agent = PrincipalBinding {
            principal_id: PrincipalId::new(id(2)),
            kind: PrincipalKind::Agent,
            public_key: agent_key.clone(),
            public_key_fingerprint: super::super::control_digest(
                "Proof-Operator-Public-Key-v1",
                &[1_u8; 32],
            ),
        };
        let human = PrincipalBinding {
            principal_id: PrincipalId::new(id(3)),
            kind: PrincipalKind::Human,
            public_key: human_key.clone(),
            public_key_fingerprint: super::super::control_digest(
                "Proof-Operator-Public-Key-v1",
                &[2_u8; 32],
            ),
        };
        let fingerprint_input = WorkspaceFingerprintInput {
            schema: WorkspaceFingerprintInput::SCHEMA.into(),
            workspace_id: id(1),
            proof_directory: DescriptorIdentity {
                device: 1,
                inode: 1,
            },
            control_lock: DescriptorIdentity {
                device: 1,
                inode: 2,
            },
            agent_key_file: DescriptorIdentity {
                device: 1,
                inode: 3,
            },
            human_key_file: DescriptorIdentity {
                device: 1,
                inode: 4,
            },
            agent_id: id(2),
            human_id: id(3),
            agent_public_key: agent_key,
            human_public_key: human_key,
        };
        let mut workspace = OperatorWorkspace {
            schema: OperatorWorkspace::SCHEMA.into(),
            workspace_id: id(1),
            database_name: "storage.db".into(),
            workspace_fingerprint: super::super::control_digest_serialized(
                "Proof-Operator-Workspace-v1",
                &fingerprint_input,
            )
            .unwrap(),
            fingerprint_input,
            schema_catalog_digest: control(9),
            agent,
            human: human.clone(),
            auth_epoch: 1,
            policy_revision: 1,
            capabilities: CapabilitySet::all(),
            created_at: now,
            updated_at: now,
            binding_digest: control(0),
        };
        workspace.binding_digest = digest_without_field(
            "Proof-Operator-Workspace-Binding-v1",
            &workspace,
            "binding_digest",
        )
        .unwrap();
        assert!(workspace.validate().is_ok());

        let capabilities = CapabilitySet::all();
        let enrollment = HumanEnrollment {
            schema: HumanEnrollment::SCHEMA.into(),
            workspace_id: id(1),
            human,
            capability_set_digest: super::super::control_digest_serialized(
                "Proof-Operator-Capability-Set-v1",
                &capabilities,
            )
            .unwrap(),
            capabilities: capabilities.clone(),
            enrolled_at: now,
        };
        assert!(enrollment.validate().is_ok());
        let session = SessionAuthorityBinding {
            schema: SessionAuthorityBinding::SCHEMA.into(),
            session_id: id(4),
            workspace_id: id(1),
            server_instance_id: id(5),
            human_id: id(3),
            auth_epoch: 1,
            policy_revision: 1,
            origin: "http://127.0.0.1:3000".into(),
            granted_capabilities: capabilities,
            issued_at: now,
            absolute_expires_at: now + chrono::Duration::seconds(900),
        };
        assert!(session.validate().is_ok());

        workspace.database_name = "other.db".into();
        assert_eq!(
            workspace.validate(),
            Err(OperatorValidationError::InvalidSchema)
        );
    }

    #[test]
    fn aggregate_budget_reservation_is_checked_against_committed_usage() {
        let now: DateTime<Utc> = "2030-01-01T00:00:00Z".parse().unwrap();
        let mut policy = BudgetPolicy {
            schema: BudgetPolicy::SCHEMA.into(),
            budget_id: id(1),
            workspace_id: id(2),
            limits: amounts(10),
            deadline_at: now + chrono::Duration::minutes(1),
            limits_digest: control(0),
        };
        policy.limits_digest =
            digest_without_field("Proof-Operator-Budget-Limits-v1", &policy, "limits_digest")
                .unwrap();
        let account = BudgetAccount {
            schema: BudgetAccount::SCHEMA.into(),
            policy,
            revision: 1,
            state: BudgetAccountState::Active,
            reserved: amounts(4),
            committed: amounts(5),
            created_at: now,
            updated_at: now,
        };
        assert!(account.can_reserve(&amounts(1), now).is_ok());
        assert_eq!(
            account.can_reserve(&amounts(2), now),
            Err(OperatorValidationError::BudgetExceeded)
        );
        assert_eq!(
            account.can_reserve(&amounts(super::super::MAX_SAFE_INTEGER + 1), now),
            Err(OperatorValidationError::BudgetExceeded)
        );
    }

    #[test]
    fn reservation_and_runtime_commit_enforce_closed_state_profiles() {
        let now: DateTime<Utc> = "2030-01-01T00:00:00Z".parse().unwrap();
        let intent = DispatchIntent {
            schema: DispatchIntent::SCHEMA.into(),
            kind: BoundaryKind::Provider,
            adapter: "synthetic".into(),
            model: Some("fixed-v1".into()),
            operation: "test.echo".into(),
            version: "v1".into(),
            argument_digest: control(1),
            ceiling: amounts(1),
        };
        let intent_digest =
            super::super::control_digest_serialized("Proof-Operator-Dispatch-Intent-v1", &intent)
                .unwrap();
        let mut reservation = BudgetReservation {
            schema: BudgetReservation::SCHEMA.into(),
            reservation_id: id(1),
            budget_id: id(2),
            run_id: id(3),
            lease_id: id(4),
            fence_epoch: 1,
            idempotency_key: id(5),
            request_digest: control(2),
            kind: BoundaryKind::Provider,
            intent,
            intent_digest,
            replay: None,
            recovery: None,
            state: BudgetReservationState::Reserved,
            reserved: amounts(1),
            charged: BudgetAmounts::default(),
            created_at: now,
            permit_id: None,
            dispatch_token_digest: None,
            call_digest: None,
            prepared_execution_digest: None,
            result_digest: None,
            prepared_binding: None,
            runtime_commit: None,
            dispatch_started_at: None,
            settled_at: None,
        };
        assert!(reservation.validate().is_ok());
        reservation.state = BudgetReservationState::Dispatching;
        assert_eq!(
            reservation.validate(),
            Err(OperatorValidationError::InvalidBranch)
        );

        let permit = DispatchPermit {
            schema: DispatchPermit::SCHEMA.into(),
            permit_id: id(6),
            run_id: id(3),
            reservation_id: id(1),
            lease_id: id(4),
            process_epoch_id: id(7),
            fence_epoch: 1,
            expected_control_revision: 1,
            intent_digest,
            replay_binding_digest: None,
            dispatch_token_digest: control(3),
            call_digest: control(4),
            authorized_at: now,
            budget_deadline_at: now + chrono::Duration::minutes(1),
        };
        let mut commit = RuntimeCommit {
            schema: RuntimeCommit::SCHEMA.into(),
            permit,
            expected_run_revision: 1,
            expected_step_revision: 1,
            expected_checkpoint_id: id(8),
            expected_checkpoint_sequence: 0,
            expected_checkpoint_digest: content(1),
            actual_charge: amounts(1),
            prepared_execution_digest: control(5),
            result_digest: control(6),
            committed_at: now,
        };
        assert!(commit.validate().is_ok());
        commit.expected_checkpoint_id = Uuid::nil();
        assert_eq!(
            commit.validate(),
            Err(OperatorValidationError::InvalidBranch)
        );
    }

    #[test]
    fn lease_and_audit_chronology_are_deterministic() {
        let now: DateTime<Utc> = "2030-01-01T00:00:00Z".parse().unwrap();
        let mut lease = RunLease {
            schema: RunLease::SCHEMA.into(),
            run_id: id(1),
            workspace_id: id(2),
            lease_id: id(3),
            owner_instance_id: id(4),
            process_epoch_id: id(5),
            lease_token_digest: control(1),
            fence_epoch: 1,
            revision: 0,
            state: RunLeaseState::Active,
            acquired_at: now,
            renewed_at: now,
            expires_at: now + chrono::Duration::seconds(30),
            released_at: None,
            lease_digest: control(0),
        };
        lease.lease_digest =
            digest_without_field("Proof-Operator-Lease-v1", &lease, "lease_digest").unwrap();
        assert!(lease.validate().is_ok());
        let mut event = AuditEvent {
            schema: AuditEvent::SCHEMA.into(),
            workspace_id: id(1),
            event_id: id(2),
            sequence: 1,
            kind: AuditEventKind::ControlShutdown,
            outcome: AuditOutcome::Accepted,
            previous_digest: None,
            event_digest: control(3),
            human_id: None,
            session_id: None,
            challenge_id: None,
            challenge_digest: None,
            session_authority_digest: None,
            related_session_id: None,
            server_instance_id: Some(id(3)),
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
            occurred_at: now,
        };
        let mut event_value = serde_json::to_value(&event).unwrap();
        event_value.as_object_mut().unwrap().remove("event_digest");
        event.event_digest = crate::operator::control_digest_serialized(
            "Proof-Operator-Audit-Event-v1",
            &event_value,
        )
        .unwrap();
        assert!(event.validate_chain_link(1, None).is_ok());
        assert_eq!(
            event.validate_chain_link(2, Some(control(3))),
            Err(OperatorValidationError::InvalidChronology)
        );
    }

    #[test]
    fn every_audit_transition_profile_accepts_only_its_frozen_presence_mask() {
        let cases = [
            (
                AuditEventKind::SessionChallengeIssued,
                AuditOutcome::Accepted,
                0x60015,
                None,
                "test.echo",
                None,
            ),
            (
                AuditEventKind::SessionIssued,
                AuditOutcome::Accepted,
                0x60017,
                None,
                "test.echo",
                None,
            ),
            (
                AuditEventKind::SessionReplaced,
                AuditOutcome::Accepted,
                0x6001f,
                None,
                "test.echo",
                None,
            ),
            (
                AuditEventKind::SessionRevoked,
                AuditOutcome::Accepted,
                0x1060193,
                Some(CommandKind::SessionRevoke),
                "operator.session_revoke",
                None,
            ),
            (
                AuditEventKind::SessionExpired,
                AuditOutcome::Expired,
                0x60013,
                None,
                "test.echo",
                None,
            ),
            (
                AuditEventKind::ApprovalDecided,
                AuditOutcome::Accepted,
                0x12001e3,
                Some(CommandKind::ApprovalDecide),
                "operator.approval_decide",
                None,
            ),
            (
                AuditEventKind::ApprovalExpired,
                AuditOutcome::Expired,
                0x60,
                None,
                "test.echo",
                None,
            ),
            (
                AuditEventKind::RunCancelled,
                AuditOutcome::Accepted,
                0x10001a3,
                Some(CommandKind::RunCancel),
                "operator.run_cancel",
                None,
            ),
            (
                AuditEventKind::RunResumed,
                AuditOutcome::Accepted,
                0x12001e3,
                Some(CommandKind::RunResume),
                "operator.run_resume",
                None,
            ),
            (
                AuditEventKind::LeaseAcquired,
                AuditOutcome::Accepted,
                0x12830,
                None,
                "test.echo",
                None,
            ),
            (
                AuditEventKind::LeaseRenewed,
                AuditOutcome::Accepted,
                0x12830,
                None,
                "test.echo",
                None,
            ),
            (
                AuditEventKind::LeaseReclaimed,
                AuditOutcome::Accepted,
                0x13830,
                None,
                "test.echo",
                None,
            ),
            (
                AuditEventKind::LeaseReleased,
                AuditOutcome::Accepted,
                0x12830,
                None,
                "test.echo",
                None,
            ),
            (
                AuditEventKind::StaleFenceRejected,
                AuditOutcome::Rejected,
                0x16c30,
                None,
                "test.echo",
                None,
            ),
            (
                AuditEventKind::BudgetReserved,
                AuditOutcome::Accepted,
                0x90e20,
                None,
                "test.echo",
                None,
            ),
            (
                AuditEventKind::BudgetReleased,
                AuditOutcome::Accepted,
                0x90e20,
                None,
                "test.echo",
                None,
            ),
            (
                AuditEventKind::BudgetCommitted,
                AuditOutcome::Accepted,
                0x194e20,
                None,
                "test.echo",
                None,
            ),
            (
                AuditEventKind::BudgetForfeited,
                AuditOutcome::Failed,
                0x194e20,
                None,
                "test.echo",
                None,
            ),
            (
                AuditEventKind::BudgetRejected,
                AuditOutcome::Rejected,
                0x90e20,
                None,
                "test.echo",
                None,
            ),
            (
                AuditEventKind::DispatchAuthorized,
                AuditOutcome::Accepted,
                0x196e30,
                None,
                "test.echo",
                None,
            ),
            (
                AuditEventKind::RuntimeResultCommitted,
                AuditOutcome::Accepted,
                0x1196e30,
                None,
                "test.echo",
                None,
            ),
            (
                AuditEventKind::RecoveryStarted,
                AuditOutcome::Accepted,
                0x499430,
                None,
                "test.echo",
                None,
            ),
            (
                AuditEventKind::RecoveryCompleted,
                AuditOutcome::Accepted,
                0x49bc30,
                None,
                "test.echo",
                None,
            ),
            (
                AuditEventKind::ControlShutdown,
                AuditOutcome::Accepted,
                0x10,
                None,
                "test.echo",
                None,
            ),
            (
                AuditEventKind::CommandRejected,
                AuditOutcome::Rejected,
                0x1a3,
                Some(CommandKind::RunCancel),
                "test.echo",
                None,
            ),
            (
                AuditEventKind::CommandConflict,
                AuditOutcome::Conflict,
                0x1a3,
                Some(CommandKind::RunCancel),
                "test.echo",
                None,
            ),
            (
                AuditEventKind::ControlFailure,
                AuditOutcome::Failed,
                0x800010,
                None,
                "test.echo",
                Some(AuditFailureScope::Workspace),
            ),
        ];
        for (kind, outcome, mask, command_kind, proof_operation, failure_scope) in cases {
            let event = audit_for_profile(
                kind,
                outcome,
                mask,
                command_kind,
                proof_operation,
                failure_scope,
            );
            assert_eq!(event.validate_chain_link(1, None), Ok(()), "{kind:?}");
        }

        let mut invalid = audit_for_profile(
            AuditEventKind::ControlShutdown,
            AuditOutcome::Accepted,
            0x10,
            None,
            "test.echo",
            None,
        );
        invalid.run_id = Some(id(90));
        invalid.event_digest =
            digest_without_field("Proof-Operator-Audit-Event-v1", &invalid, "event_digest")
                .unwrap();
        assert_eq!(
            invalid.validate_chain_link(1, None),
            Err(OperatorValidationError::InvalidBranch)
        );
    }

    #[test]
    fn resume_requires_exactly_one_approval_or_recovery_branch() {
        let command = RunResumeCommand {
            schema: RunResumeCommand::SCHEMA.into(),
            binding: binding(),
            run_id: id(7),
            step_id: id(8),
            approval_request_id: Some(id(9)),
            decision_digest: Some(content(1)),
            recovery_directive_id: None,
            recovery_directive_digest: None,
            expected_run_revision: 1,
            expected_step_revision: 1,
            expected_control_revision: 1,
            expected_checkpoint_id: id(10),
            expected_checkpoint_sequence: 0,
            expected_checkpoint_digest: content(2),
            expected_fence_epoch: 1,
        };
        assert!(command.validate().is_ok());
        let mut invalid = command.clone();
        invalid.recovery_directive_id = Some(id(11));
        invalid.recovery_directive_digest = Some(control(3));
        assert_eq!(
            invalid.validate(),
            Err(OperatorValidationError::InvalidBranch)
        );
    }

    #[test]
    fn strict_command_binding_rejects_non_v7_ids_and_unknown_fields() {
        let mut value = serde_json::to_value(binding()).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("extra".into(), serde_json::json!(true));
        assert!(serde_json::from_value::<CommandBinding>(value).is_err());
        let mut invalid = binding();
        invalid.command_id = Uuid::nil();
        assert_eq!(
            invalid.validate(),
            Err(OperatorValidationError::InvalidUuid)
        );
    }

    #[test]
    fn dispatch_intent_requires_exact_operation_version_and_branch_bounds() {
        let valid = DispatchIntent {
            schema: DispatchIntent::SCHEMA.into(),
            kind: BoundaryKind::Provider,
            adapter: "bounded_adapter".into(),
            model: Some("model/v1".into()),
            operation: "test.echo".into(),
            version: "v1".into(),
            argument_digest: control(1),
            ceiling: amounts(1),
        };
        assert!(valid.validate().is_ok());
        for version in ["1", "v0", "v01", "v", "v1x"] {
            let mut invalid = valid.clone();
            invalid.version = version.into();
            assert!(invalid.validate().is_err(), "accepted {version}");
        }
        let mut invalid = valid.clone();
        invalid.operation = "test.echo.more".into();
        assert!(invalid.validate().is_err());
        invalid = valid;
        invalid.ceiling.steps = super::super::MAX_SAFE_INTEGER + 1;
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn replay_binding_recomputes_digest_and_rejects_identity_mutation() {
        let mut replay = ReplayClaimBinding {
            schema: ReplayClaimBinding::SCHEMA.into(),
            policy: ReplayPolicy::RequiredUuidv7ExactReplay,
            workspace_id: id(1),
            run_id: id(2),
            step_id: id(3),
            checkpoint_id: id(4),
            checkpoint_sequence: 0,
            checkpoint_digest: content(1),
            operation: "test.echo".into(),
            version: "v1".into(),
            idempotency_key: id(5),
            input_digest: content(2),
            claimed_by: PrincipalId::new(id(6)),
            binding_digest: control(0),
        };
        replay.binding_digest = replay.recomputed_binding_digest().unwrap();
        assert!(replay.validate().is_ok());
        replay.step_id = id(7);
        assert!(replay.validate().is_err());
    }

    fn projection(
        status: AgentRunStatus,
        attention: crate::operator::AttentionState,
    ) -> RunProjection {
        let at = "2030-01-01T00:00:00Z".parse().unwrap();
        let (human, approval, recovery, recovery_digest) = match attention {
            crate::operator::AttentionState::AwaitingDecision => {
                (Some(id(5)), Some(id(6)), None, None)
            }
            crate::operator::AttentionState::Recoverable => {
                (None, None, Some(id(7)), Some(control(7)))
            }
            crate::operator::AttentionState::Running
            | crate::operator::AttentionState::Terminal => (None, None, None, None),
        };
        let mut value = RunProjection {
            schema: RunProjection::SCHEMA.into(),
            projection_id: id(1),
            projection_sequence: 1,
            projection_revision: 0,
            workspace_id: id(2),
            run_id: id(3),
            source_run_revision: 0,
            source_control_revision: 0,
            checkpoint_id: id(4),
            checkpoint_sequence: 0,
            checkpoint_digest: content(1),
            fence_epoch: 0,
            run_status: status,
            attention,
            required_human_id: human,
            approval_request_id: approval,
            recovery_directive_id: recovery,
            recovery_directive_digest: recovery_digest,
            projected_at: at,
            snapshot_digest: control(0),
        };
        value.snapshot_digest = digest_without_field(
            "Proof-Operator-Run-Projection-v1",
            &value,
            "snapshot_digest",
        )
        .unwrap();
        value
    }

    #[test]
    fn run_projection_accepts_exactly_the_four_frozen_status_branches() {
        for (status, attention) in [
            (
                AgentRunStatus::WaitingForInput,
                crate::operator::AttentionState::AwaitingDecision,
            ),
            (
                AgentRunStatus::Failed,
                crate::operator::AttentionState::Recoverable,
            ),
            (
                AgentRunStatus::Running,
                crate::operator::AttentionState::Running,
            ),
            (
                AgentRunStatus::Succeeded,
                crate::operator::AttentionState::Terminal,
            ),
        ] {
            assert!(projection(status, attention).validate().is_ok());
        }
        let mut invalid = projection(
            AgentRunStatus::Succeeded,
            crate::operator::AttentionState::Terminal,
        );
        invalid.required_human_id = Some(id(9));
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn control_transition_outcome_enforces_kind_specific_nullability() {
        let value = ControlTransitionOutcome {
            schema: ControlTransitionOutcome::SCHEMA.into(),
            command_id: id(1),
            kind: CommandKind::RunCancel,
            outcome: AppliedCommandOutcome::Applied,
            proof_operation: OperatorProofOperation::RunCancel,
            target_run_id: Some(id(2)),
            approval_request_id: None,
            resulting_run_revision: Some(2),
            resulting_step_revision: None,
            resulting_control_revision: Some(3),
            resulting_fence_epoch: Some(1),
            decision_digest: None,
            completed_at: "2030-01-01T00:00:00Z".parse().unwrap(),
        };
        assert!(value.validate().is_ok());
        assert_eq!(
            serde_json::to_value(value.proof_operation).unwrap(),
            serde_json::Value::String("operator.run_cancel".into())
        );
        let mut invalid = value;
        invalid.proof_operation = OperatorProofOperation::RunResume;
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn cancel_already_terminal_receipt_is_stable_and_rejects_applied_shape() {
        let mut receipt = CommandReceipt {
            schema: CommandReceipt::SCHEMA.into(),
            receipt_id: id(1),
            command_id: id(2),
            idempotency_key: id(3),
            kind: CommandKind::RunCancel,
            outcome: CommandOutcome::AlreadyTerminal,
            request_digest: control(1),
            workspace_id: id(4),
            human_id: id(5),
            target_run_id: Some(id(6)),
            approval_request_id: None,
            observed_run_revision: Some(2),
            resulting_run_revision: Some(2),
            resulting_step_revision: None,
            resulting_control_revision: Some(3),
            resulting_fence_epoch: Some(1),
            decision_id: None,
            decision_digest: None,
            proof: None,
            audit_event_id: id(7),
            audit_sequence: 1,
            audit_digest: control(2),
            completed_at: "2030-01-01T00:00:00Z".parse().unwrap(),
            receipt_digest: control(0),
        };
        receipt.receipt_digest = digest_without_field(
            "Proof-Operator-Command-Receipt-v1",
            &receipt,
            "receipt_digest",
        )
        .unwrap();
        assert!(receipt.validate().is_ok());
        let canonical = serde_json::to_string(&receipt).unwrap();
        assert_eq!(canonical, serde_json::to_string(&receipt).unwrap());
        receipt.resulting_run_revision = Some(3);
        receipt.receipt_digest = digest_without_field(
            "Proof-Operator-Command-Receipt-v1",
            &receipt,
            "receipt_digest",
        )
        .unwrap();
        assert!(receipt.validate().is_err());
        receipt.resulting_run_revision = Some(2);
        receipt.outcome = CommandOutcome::Applied;
        assert!(receipt.validate().is_err());
    }
}
