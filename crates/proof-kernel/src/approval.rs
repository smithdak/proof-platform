//! Signed human approval contracts for governed agent execution.

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use ed25519_dalek::Signature;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use uuid::Uuid;

use crate::canonical::{
    canonicalize, canonicalize_serialized, digest, ArtifactKind, ContentDigest,
};
use crate::evidence::Proof;
use crate::identity::{
    principal_from_keypair, sign, verify, Keypair, Principal, PrincipalId, PrincipalKind,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApprovalOutcome {
    Approved,
    Denied,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub id: Uuid,
    pub operation: String,
    pub version: String,
    pub input_digest: ContentDigest,
    pub requested_by: PrincipalId,
    pub requested_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedApprovalRequest {
    pub body: ApprovalRequest,
    pub signature: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalDecision {
    pub id: Uuid,
    pub request_id: Uuid,
    pub request_digest: ContentDigest,
    pub outcome: ApprovalOutcome,
    pub decided_by: PrincipalId,
    pub decided_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedApprovalDecision {
    pub body: ApprovalDecision,
    pub signature: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalGrant {
    pub request: SignedApprovalRequest,
    pub decision: SignedApprovalDecision,
    pub approver: Principal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalExecution {
    pub request_id: Uuid,
    pub executed_at: DateTime<Utc>,
    pub output: Value,
    pub proof: Proof,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ApprovalError {
    #[error("approval operation must not be empty")]
    EmptyOperation,
    #[error("approval version must not be empty")]
    EmptyVersion,
    #[error("approval expiration must be after its request time")]
    InvalidWindow,
    #[error("approval requester must use an agent identity")]
    RequesterMustBeAgent,
    #[error("approval requester does not match the signing identity")]
    RequesterMismatch,
    #[error("approval decision must use a human identity")]
    ApproverMustBeHuman,
    #[error("approval decision does not match the signing identity")]
    ApproverMismatch,
    #[error("approval signature is invalid")]
    InvalidSignature,
    #[error("approval payload could not be canonicalized")]
    Canonicalization,
    #[error("approval request does not match the decision")]
    RequestMismatch,
    #[error("approval request was denied")]
    Denied,
    #[error("approval request has expired")]
    Expired,
    #[error("approval decision falls outside the request validity window")]
    DecisionOutOfWindow,
    #[error("approval operation does not match the requested execution")]
    OperationMismatch,
    #[error("approval version does not match the requested execution")]
    VersionMismatch,
    #[error("approval input does not match the requested execution")]
    InputMismatch,
    #[error("approval actor does not match the requesting agent")]
    ActorMismatch,
    #[error("approval was signed by an untrusted human principal")]
    UntrustedApprover,
}

impl SignedApprovalRequest {
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        operation: impl Into<String>,
        version: impl Into<String>,
        input: &Value,
        requested_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
        requester: &Keypair,
    ) -> Result<Self, ApprovalError> {
        let operation = operation.into();
        if operation.trim().is_empty() {
            return Err(ApprovalError::EmptyOperation);
        }
        let version = version.into();
        if version.trim().is_empty() {
            return Err(ApprovalError::EmptyVersion);
        }
        if expires_at <= requested_at {
            return Err(ApprovalError::InvalidWindow);
        }
        if requester.kind != PrincipalKind::Agent {
            return Err(ApprovalError::RequesterMustBeAgent);
        }
        let input = canonicalize(input).map_err(|_| ApprovalError::Canonicalization)?;
        let body = ApprovalRequest {
            id: Uuid::now_v7(),
            operation,
            version,
            input_digest: digest(ArtifactKind::OperationInput, &input),
            requested_by: requester.principal_id,
            requested_at,
            expires_at,
        };
        let payload = canonical_payload(&body)?;
        Ok(Self {
            body,
            signature: sign(requester, &payload).to_bytes().to_vec(),
        })
    }

    pub fn verify(&self, requester: &Principal) -> Result<(), ApprovalError> {
        if requester.kind != PrincipalKind::Agent {
            return Err(ApprovalError::RequesterMustBeAgent);
        }
        if requester.id != self.body.requested_by {
            return Err(ApprovalError::RequesterMismatch);
        }
        verify_bytes(
            &requester.public_key,
            &canonical_payload(&self.body)?,
            &self.signature,
        )
    }

    /// Verifies that this request authorizes the exact pending agent call.
    pub fn verify_for_call(
        &self,
        requester: &Principal,
        operation: &str,
        version: &str,
        input: &Value,
        actor: PrincipalId,
        now: DateTime<Utc>,
    ) -> Result<(), ApprovalError> {
        self.verify(requester)?;
        if now > self.body.expires_at {
            return Err(ApprovalError::Expired);
        }
        if actor != requester.id || actor != self.body.requested_by {
            return Err(ApprovalError::ActorMismatch);
        }
        if operation != self.body.operation {
            return Err(ApprovalError::OperationMismatch);
        }
        if version != self.body.version {
            return Err(ApprovalError::VersionMismatch);
        }
        let input = canonicalize(input).map_err(|_| ApprovalError::Canonicalization)?;
        if digest(ArtifactKind::OperationInput, &input) != self.body.input_digest {
            return Err(ApprovalError::InputMismatch);
        }
        Ok(())
    }

    pub fn digest(&self) -> Result<ContentDigest, ApprovalError> {
        let canonical =
            canonicalize_serialized(self).map_err(|_| ApprovalError::Canonicalization)?;
        Ok(digest(ArtifactKind::ApprovalRequest, &canonical))
    }
}

impl SignedApprovalDecision {
    pub fn create(
        request: &SignedApprovalRequest,
        outcome: ApprovalOutcome,
        reason: Option<String>,
        decided_at: DateTime<Utc>,
        approver: &Keypair,
    ) -> Result<Self, ApprovalError> {
        if approver.kind != PrincipalKind::Human {
            return Err(ApprovalError::ApproverMustBeHuman);
        }
        if decided_at < request.body.requested_at || decided_at > request.body.expires_at {
            return Err(ApprovalError::DecisionOutOfWindow);
        }
        let body = ApprovalDecision {
            id: Uuid::now_v7(),
            request_id: request.body.id,
            request_digest: request.digest()?,
            outcome,
            decided_by: approver.principal_id,
            decided_at,
            reason: reason.filter(|reason| !reason.trim().is_empty()),
        };
        let payload = canonical_payload(&body)?;
        Ok(Self {
            body,
            signature: sign(approver, &payload).to_bytes().to_vec(),
        })
    }

    pub fn verify(&self, approver: &Principal) -> Result<(), ApprovalError> {
        if approver.kind != PrincipalKind::Human {
            return Err(ApprovalError::ApproverMustBeHuman);
        }
        if approver.id != self.body.decided_by {
            return Err(ApprovalError::ApproverMismatch);
        }
        verify_bytes(
            &approver.public_key,
            &canonical_payload(&self.body)?,
            &self.signature,
        )
    }

    pub fn digest(&self) -> Result<ContentDigest, ApprovalError> {
        let canonical =
            canonicalize_serialized(self).map_err(|_| ApprovalError::Canonicalization)?;
        Ok(digest(ArtifactKind::ApprovalDecision, &canonical))
    }
}

impl ApprovalGrant {
    /// Verifies the linked agent request and trusted human decision signatures.
    pub fn verify_decision(
        &self,
        requester: &Principal,
        trusted_approver: &Principal,
    ) -> Result<(), ApprovalError> {
        if !same_principal(&self.approver, trusted_approver) {
            return Err(ApprovalError::UntrustedApprover);
        }
        self.request.verify(requester)?;
        self.decision.verify(&self.approver)?;
        if self.decision.body.request_id != self.request.body.id
            || self.decision.body.request_digest != self.request.digest()?
        {
            return Err(ApprovalError::RequestMismatch);
        }
        if self.decision.body.decided_at < self.request.body.requested_at
            || self.decision.body.decided_at > self.request.body.expires_at
        {
            return Err(ApprovalError::DecisionOutOfWindow);
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn verify_for_execution(
        &self,
        requester: &Keypair,
        trusted_approver: &Principal,
        operation: &str,
        version: &str,
        input: &Value,
        actor: PrincipalId,
        now: DateTime<Utc>,
    ) -> Result<(), ApprovalError> {
        let requester_principal = principal_from_keypair(requester);
        self.verify_decision(&requester_principal, trusted_approver)?;
        if self.decision.body.outcome != ApprovalOutcome::Approved {
            return Err(ApprovalError::Denied);
        }
        self.request
            .verify_for_call(&requester_principal, operation, version, input, actor, now)
    }
}

pub trait ApprovalStore: Send + Sync {
    fn save_approval_request(&self, request: &SignedApprovalRequest) -> Result<(), String>;
    fn load_approval_request(
        &self,
        request_id: &Uuid,
    ) -> Result<Option<SignedApprovalRequest>, String>;
    fn list_approval_requests(&self) -> Result<Vec<SignedApprovalRequest>, String>;
    fn save_approval_decision(&self, decision: &SignedApprovalDecision) -> Result<(), String>;
    fn load_approval_decision(
        &self,
        request_id: &Uuid,
    ) -> Result<Option<SignedApprovalDecision>, String>;
    fn save_approval_execution(&self, execution: &ApprovalExecution) -> Result<(), String>;
    fn load_approval_execution(
        &self,
        request_id: &Uuid,
    ) -> Result<Option<ApprovalExecution>, String>;
    fn load_trusted_approver(&self, approver: &PrincipalId) -> Result<Option<Principal>, String>;
}

#[derive(Default)]
pub struct RecordingApprovalStore {
    requests: Mutex<BTreeMap<Uuid, SignedApprovalRequest>>,
    decisions: Mutex<BTreeMap<Uuid, SignedApprovalDecision>>,
    executions: Mutex<BTreeMap<Uuid, ApprovalExecution>>,
    approvers: Mutex<HashMap<PrincipalId, Principal>>,
}

impl RecordingApprovalStore {
    pub fn trust_approver(&self, approver: Principal) -> Result<(), String> {
        self.approvers
            .lock()
            .map_err(|_| "approval approver lock poisoned".to_string())?
            .insert(approver.id, approver);
        Ok(())
    }
}

impl ApprovalStore for RecordingApprovalStore {
    fn save_approval_request(&self, request: &SignedApprovalRequest) -> Result<(), String> {
        save_once(&self.requests, request.body.id, request, "approval request")
    }

    fn load_approval_request(
        &self,
        request_id: &Uuid,
    ) -> Result<Option<SignedApprovalRequest>, String> {
        Ok(self
            .requests
            .lock()
            .map_err(|_| "approval request lock poisoned".to_string())?
            .get(request_id)
            .cloned())
    }

    fn list_approval_requests(&self) -> Result<Vec<SignedApprovalRequest>, String> {
        Ok(self
            .requests
            .lock()
            .map_err(|_| "approval request lock poisoned".to_string())?
            .values()
            .cloned()
            .collect())
    }

    fn save_approval_decision(&self, decision: &SignedApprovalDecision) -> Result<(), String> {
        save_once(
            &self.decisions,
            decision.body.request_id,
            decision,
            "approval decision",
        )
    }

    fn load_approval_decision(
        &self,
        request_id: &Uuid,
    ) -> Result<Option<SignedApprovalDecision>, String> {
        Ok(self
            .decisions
            .lock()
            .map_err(|_| "approval decision lock poisoned".to_string())?
            .get(request_id)
            .cloned())
    }

    fn save_approval_execution(&self, execution: &ApprovalExecution) -> Result<(), String> {
        save_once(
            &self.executions,
            execution.request_id,
            execution,
            "approval execution",
        )
    }

    fn load_approval_execution(
        &self,
        request_id: &Uuid,
    ) -> Result<Option<ApprovalExecution>, String> {
        Ok(self
            .executions
            .lock()
            .map_err(|_| "approval execution lock poisoned".to_string())?
            .get(request_id)
            .cloned())
    }

    fn load_trusted_approver(&self, approver: &PrincipalId) -> Result<Option<Principal>, String> {
        Ok(self
            .approvers
            .lock()
            .map_err(|_| "approval approver lock poisoned".to_string())?
            .get(approver)
            .cloned())
    }
}

fn save_once<T: Clone + PartialEq>(
    records: &Mutex<BTreeMap<Uuid, T>>,
    key: Uuid,
    value: &T,
    record_name: &str,
) -> Result<(), String> {
    let mut records = records
        .lock()
        .map_err(|_| format!("{record_name} lock poisoned"))?;
    if let Some(existing) = records.get(&key) {
        if existing == value {
            return Ok(());
        }
        return Err(format!("conflicting {record_name}: {key}"));
    }
    records.insert(key, value.clone());
    Ok(())
}

fn canonical_payload<T: Serialize>(value: &T) -> Result<Vec<u8>, ApprovalError> {
    canonicalize_serialized(value)
        .map(|canonical| canonical.as_bytes().to_vec())
        .map_err(|_| ApprovalError::Canonicalization)
}

fn verify_bytes(
    public_key: &ed25519_dalek::VerifyingKey,
    payload: &[u8],
    signature: &[u8],
) -> Result<(), ApprovalError> {
    let signature: [u8; 64] = signature
        .try_into()
        .map_err(|_| ApprovalError::InvalidSignature)?;
    verify(public_key, payload, &Signature::from_bytes(&signature))
        .map_err(|_| ApprovalError::InvalidSignature)
}

fn same_principal(left: &Principal, right: &Principal) -> bool {
    left.id == right.id && left.kind == right.kind && left.public_key == right.public_key
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::generate_keypair_for;
    use serde_json::json;

    fn signed_request(requester: &Keypair) -> SignedApprovalRequest {
        let requested_at = Utc::now();
        SignedApprovalRequest::create(
            "content.release",
            "v1",
            &json!({"object_id": "article-1"}),
            requested_at,
            requested_at + chrono::Duration::minutes(15),
            requester,
        )
        .unwrap()
    }

    #[test]
    fn request_signature_binds_actor_operation_and_input() {
        let requester = generate_keypair_for(PrincipalKind::Agent);
        let request = signed_request(&requester);
        request.verify(&principal_from_keypair(&requester)).unwrap();

        let mut changed = request.clone();
        changed.body.operation = "content.approve".to_string();
        assert_eq!(
            changed.verify(&principal_from_keypair(&requester)),
            Err(ApprovalError::InvalidSignature)
        );

        assert_eq!(
            request.verify_for_call(
                &principal_from_keypair(&requester),
                "content.release",
                "v1",
                &json!({"object_id": "different"}),
                requester.principal_id,
                Utc::now(),
            ),
            Err(ApprovalError::InputMismatch)
        );
    }

    #[test]
    fn only_human_identities_can_sign_decisions() {
        let requester = generate_keypair_for(PrincipalKind::Agent);
        let request = signed_request(&requester);
        assert_eq!(
            SignedApprovalDecision::create(
                &request,
                ApprovalOutcome::Approved,
                None,
                Utc::now(),
                &requester,
            ),
            Err(ApprovalError::ApproverMustBeHuman)
        );

        let human = generate_keypair_for(PrincipalKind::Human);
        let decision = SignedApprovalDecision::create(
            &request,
            ApprovalOutcome::Approved,
            Some("Reviewed".to_string()),
            Utc::now(),
            &human,
        )
        .unwrap();
        decision.verify(&principal_from_keypair(&human)).unwrap();
        assert!(decision.digest().is_ok());
    }

    #[test]
    fn grant_requires_exact_execution_and_trusted_approver() {
        let requester = generate_keypair_for(PrincipalKind::Agent);
        let human = generate_keypair_for(PrincipalKind::Human);
        let request = signed_request(&requester);
        let decision = SignedApprovalDecision::create(
            &request,
            ApprovalOutcome::Approved,
            None,
            Utc::now(),
            &human,
        )
        .unwrap();
        let approver = principal_from_keypair(&human);
        let grant = ApprovalGrant {
            request,
            decision,
            approver: approver.clone(),
        };
        grant
            .verify_decision(&principal_from_keypair(&requester), &approver)
            .unwrap();
        grant
            .verify_for_execution(
                &requester,
                &approver,
                "content.release",
                "v1",
                &json!({"object_id": "article-1"}),
                requester.principal_id,
                Utc::now(),
            )
            .unwrap();
        assert_eq!(
            grant.verify_for_execution(
                &requester,
                &approver,
                "content.release",
                "v1",
                &json!({"object_id": "different"}),
                requester.principal_id,
                Utc::now(),
            ),
            Err(ApprovalError::InputMismatch)
        );
        let stranger = principal_from_keypair(&generate_keypair_for(PrincipalKind::Human));
        assert_eq!(
            grant.verify_for_execution(
                &requester,
                &stranger,
                "content.release",
                "v1",
                &json!({"object_id": "article-1"}),
                requester.principal_id,
                Utc::now(),
            ),
            Err(ApprovalError::UntrustedApprover)
        );
    }

    #[test]
    fn recording_store_round_trips_approval_lifecycle() {
        let requester = generate_keypair_for(PrincipalKind::Agent);
        let human = generate_keypair_for(PrincipalKind::Human);
        let request = signed_request(&requester);
        let decision = SignedApprovalDecision::create(
            &request,
            ApprovalOutcome::Denied,
            Some("Needs revision".to_string()),
            Utc::now(),
            &human,
        )
        .unwrap();
        let store = RecordingApprovalStore::default();
        store
            .trust_approver(principal_from_keypair(&human))
            .unwrap();
        store.save_approval_request(&request).unwrap();
        store.save_approval_decision(&decision).unwrap();

        assert_eq!(
            store.load_approval_request(&request.body.id).unwrap(),
            Some(request.clone())
        );
        assert_eq!(
            store.load_approval_decision(&request.body.id).unwrap(),
            Some(decision)
        );
        assert_eq!(store.list_approval_requests().unwrap(), vec![request]);
        assert!(store
            .load_trusted_approver(&human.principal_id)
            .unwrap()
            .is_some());
    }
}
