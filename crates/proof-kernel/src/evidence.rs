//! Signed evidence envelopes.

use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::canonical::{canonicalize_serialized, digest, ArtifactKind, ContentDigest};
use crate::identity::{sign, verify, IdentityError, Keypair, PrincipalId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProofBody {
    pub id: Uuid,
    pub actor: PrincipalId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delegation_id: Option<Uuid>,
    pub operation: String,
    pub input_digest: ContentDigest,
    pub output_digest: ContentDigest,
    pub timestamp: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proof {
    pub body: ProofBody,
    pub signature: Signature,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ProofError {
    #[error("invalid proof signature")]
    InvalidSignature,
    #[error("proof actor does not match signing key")]
    ActorMismatch,
    #[error("canonicalization failed")]
    Canonicalization,
}

impl From<IdentityError> for ProofError {
    fn from(_: IdentityError) -> Self {
        Self::InvalidSignature
    }
}

impl Proof {
    /// Returns whether the proof was expired at the supplied instant.
    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        self.body
            .expires_at
            .is_some_and(|expires_at| expires_at <= now)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: Uuid,
        actor: PrincipalId,
        delegation_id: Option<Uuid>,
        operation: impl Into<String>,
        input_digest: ContentDigest,
        output_digest: ContentDigest,
        timestamp: DateTime<Utc>,
    ) -> Self {
        Self {
            body: ProofBody {
                id,
                actor,
                delegation_id,
                operation: operation.into(),
                input_digest,
                output_digest,
                timestamp,
                expires_at: None,
            },
            signature: Signature::from_bytes(&[0; 64]),
        }
    }

    pub fn signing_payload(&self) -> Result<Vec<u8>, ProofError> {
        canonicalize_serialized(&self.body)
            .map(|canonical| canonical.as_bytes().to_vec())
            .map_err(|_| ProofError::Canonicalization)
    }

    pub fn sign(mut self, keypair: &Keypair) -> Result<Self, ProofError> {
        if keypair.principal_id != self.body.actor {
            return Err(ProofError::ActorMismatch);
        }
        let payload = self.signing_payload()?;
        self.signature = sign(keypair, &payload);
        Ok(self)
    }

    pub fn verify(&self, public_key: &VerifyingKey) -> Result<(), ProofError> {
        let payload = self.signing_payload()?;
        verify(public_key, &payload, &self.signature)?;
        Ok(())
    }

    pub fn proof_digest(&self) -> Result<ContentDigest, ProofError> {
        let canonical = canonicalize_serialized(self).map_err(|_| ProofError::Canonicalization)?;
        Ok(digest(ArtifactKind::Proof, &canonical))
    }
}

impl Serialize for Proof {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        #[derive(Serialize)]
        struct Wire<'a> {
            body: &'a ProofBody,
            signature: Vec<u8>,
        }
        let wire = Wire {
            body: &self.body,
            signature: self.signature.to_bytes().to_vec(),
        };
        wire.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Proof {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Wire {
            body: ProofBody,
            signature: Vec<u8>,
        }
        let wire = Wire::deserialize(deserializer)?;
        if wire.signature.len() != 64 {
            return Err(serde::de::Error::invalid_length(
                wire.signature.len(),
                &"64 signature bytes",
            ));
        }
        let bytes: [u8; 64] = wire
            .signature
            .try_into()
            .map_err(|_| serde::de::Error::custom("invalid signature length"))?;
        Ok(Self {
            body: wire.body,
            signature: Signature::from_bytes(&bytes),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical::canonicalize;
    use crate::identity::{generate_keypair_for, principal_from_keypair, PrincipalKind};
    use serde_json::json;

    fn proof_for(keypair: &Keypair) -> Proof {
        let input = canonicalize(&json!({"hello": "world"})).unwrap();
        let output = canonicalize(&json!({"ok": true})).unwrap();
        Proof::new(
            Uuid::now_v7(),
            keypair.principal_id,
            None,
            "object.create",
            digest(ArtifactKind::OperationInput, &input),
            digest(ArtifactKind::OperationOutput, &output),
            Utc::now(),
        )
        .sign(keypair)
        .unwrap()
    }

    #[test]
    fn signs_and_verifies_actor() {
        let keypair = generate_keypair_for(PrincipalKind::Agent);
        let proof = proof_for(&keypair);
        let principal = principal_from_keypair(&keypair);
        assert!(proof.verify(&principal.public_key).is_ok());
    }

    #[test]
    fn rejects_modified_body() {
        let keypair = generate_keypair_for(PrincipalKind::Agent);
        let principal = principal_from_keypair(&keypair);
        let mut proof = proof_for(&keypair);
        proof.body.operation = "object.delete".to_string();
        assert!(proof.verify(&principal.public_key).is_err());
    }

    #[test]
    fn rejects_actor_mismatch_and_produces_stable_digest() {
        let keypair = generate_keypair_for(PrincipalKind::Agent);
        let proof = proof_for(&keypair);
        let other = generate_keypair_for(PrincipalKind::Service);
        assert_eq!(proof.clone().sign(&other), Err(ProofError::ActorMismatch));
        assert_eq!(proof.proof_digest(), proof.proof_digest());
    }

    #[test]
    fn round_trips_through_json() {
        let keypair = generate_keypair_for(PrincipalKind::Human);
        let proof = proof_for(&keypair);
        let json = serde_json::to_string(&proof).unwrap();
        let decoded: Proof = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, proof);
    }
}
