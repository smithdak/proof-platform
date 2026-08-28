//! Proof kernel: canonical data, identity, delegation, evidence, and registry.

pub mod benchmark;
pub mod canonical;
pub mod delegation;
pub mod evidence;
pub mod executor;
pub mod identity;
pub mod registry;

pub use benchmark::{Benchmark, BenchmarkError, BenchmarkResult, BenchmarkRunner};
pub use canonical::{
    canonicalize, canonicalize_serialized, derive_key_material, digest, ArtifactKind,
    CanonicalJson, CanonicalizationError, ContentDigest, DeriveKeyContext,
};
pub use delegation::{validate_chain, Delegation, DelegationChain, DelegationError};
pub use evidence::{Proof, ProofBody, ProofError};
pub use executor::{
    create_proof, ExecutionContext, ExecutionEngine, ExecutionError, ExecutionStore,
    OperationHandler, RecordingStore,
};
pub use identity::{
    generate_keypair, generate_keypair_for, principal_from_keypair, sign, verify, IdentityError,
    Keypair, Principal, PrincipalId, PrincipalKind,
};
pub use registry::{Governance, Registry, RegistryEntry, RegistryError, VersionStatus};
