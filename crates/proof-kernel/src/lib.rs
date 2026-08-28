//! Proof kernel: canonical data, identity, delegation, evidence, and registry.

pub mod canonical;
pub mod delegation;
pub mod evidence;
pub mod identity;
pub mod registry;

pub use canonical::{
    canonicalize, canonicalize_serialized, derive_key_material, digest, ArtifactKind,
    CanonicalJson, CanonicalizationError, ContentDigest, DeriveKeyContext,
};
pub use delegation::Delegation;
pub use evidence::{Proof, ProofBody, ProofError};
pub use identity::{
    generate_keypair, generate_keypair_for, principal_from_keypair, sign, verify, IdentityError,
    Keypair, Principal, PrincipalId, PrincipalKind,
};
pub use registry::{Governance, Registry, RegistryEntry, RegistryError};
