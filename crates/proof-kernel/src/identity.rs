//! Ed25519 identities for humans, agents, and services.

use std::fmt;

use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PrincipalId(Uuid);

impl PrincipalId {
    pub const fn new(id: Uuid) -> Self {
        Self(id)
    }

    pub const fn as_uuid(&self) -> Uuid {
        self.0
    }

    pub fn now() -> Self {
        Self(Uuid::now_v7())
    }
}

impl fmt::Display for PrincipalId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PrincipalKind {
    Human,
    Agent,
    Service,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Principal {
    pub id: PrincipalId,
    pub kind: PrincipalKind,
    pub public_key: VerifyingKey,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keypair {
    pub principal_id: PrincipalId,
    pub kind: PrincipalKind,
    pub created_at: DateTime<Utc>,
    pub signing_key: SigningKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("invalid Ed25519 signature")]
pub struct IdentityError;

pub fn generate_keypair() -> Keypair {
    generate_keypair_for(PrincipalKind::Agent)
}

pub fn generate_keypair_for(kind: PrincipalKind) -> Keypair {
    Keypair {
        principal_id: PrincipalId::now(),
        kind,
        created_at: Utc::now(),
        signing_key: SigningKey::generate(&mut OsRng),
    }
}

pub fn principal_from_keypair(keypair: &Keypair) -> Principal {
    Principal {
        id: keypair.principal_id,
        kind: keypair.kind,
        public_key: keypair.signing_key.verifying_key(),
        created_at: keypair.created_at,
    }
}

pub fn sign(keypair: &Keypair, message: &[u8]) -> Signature {
    keypair.signing_key.sign(message)
}

pub fn verify(
    public_key: &VerifyingKey,
    message: &[u8],
    signature: &Signature,
) -> Result<(), IdentityError> {
    public_key
        .verify(message, signature)
        .map_err(|_| IdentityError)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_distinct_uuidv7_principals() {
        let first = generate_keypair_for(PrincipalKind::Human);
        let second = generate_keypair_for(PrincipalKind::Agent);
        assert_ne!(first.principal_id, second.principal_id);
        assert_ne!(first.signing_key, second.signing_key);
    }

    #[test]
    fn signs_and_verifies_messages() {
        for kind in [
            PrincipalKind::Human,
            PrincipalKind::Agent,
            PrincipalKind::Service,
        ] {
            let keypair = generate_keypair_for(kind);
            let principal = principal_from_keypair(&keypair);
            assert_eq!(principal.kind, kind);
            let signature = sign(&keypair, b"message");
            assert!(verify(&principal.public_key, b"message", &signature).is_ok());
            assert!(verify(&principal.public_key, b"changed", &signature).is_err());
        }
    }
}
