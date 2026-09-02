//! Proof kernel: canonical data, identity, delegation, evidence, and registry.

pub mod agent;
pub mod agent_run;
pub mod approval;
pub mod benchmark;
pub mod canonical;
pub mod delegation;
pub mod evidence;
pub mod executor;
pub mod identity;
pub mod operator;
pub mod registry;

pub use agent::{
    AgentDefinition, AgentDefinitionError, AgentLimits, AgentRunEvent, AgentRunEventKind,
    AgentStore, AgentTool, RecordingAgentStore,
};
pub use agent_run::{
    AgentCheckpoint, AgentCheckpointAppendResult, AgentCheckpointTail, AgentEvaluationOutcome,
    AgentRun, AgentRunError, AgentRunEvaluation, AgentRunMode, AgentRunStatus, AgentRunStep,
    AgentRunStepStatus, AgentRunStore, LiveRunStartClaim, LiveRunStartClaimResult,
    RecordingAgentRunStore, LIVE_RUN_START_CLAIM_SCHEMA,
};
pub use approval::{
    ApprovalDecision, ApprovalError, ApprovalExecution, ApprovalGrant, ApprovalOutcome,
    ApprovalRequest, ApprovalStore, RecordingApprovalStore, SignedApprovalDecision,
    SignedApprovalRequest,
};
pub use benchmark::{Benchmark, BenchmarkError, BenchmarkResult, BenchmarkRunner};
pub use canonical::{
    canonicalize, canonicalize_serialized, derive_key_material, digest, ArtifactKind,
    CanonicalJson, CanonicalizationError, ContentDigest, DeriveKeyContext,
};
pub use delegation::{validate_chain, Delegation, DelegationChain, DelegationError};
pub use evidence::{Proof, ProofBody, ProofError};
pub use executor::{
    create_proof, AuditFilter, ExecutionContext, ExecutionEngine, ExecutionError, ExecutionOutcome,
    ExecutionReplayClaim, ExecutionReplayClaimResult, ExecutionReplayKey, ExecutionStore,
    IdempotencyError, IdempotencyPolicy, OperationHandler, RecordingStore,
};
pub use identity::{
    generate_keypair, generate_keypair_for, principal_from_keypair, sign, verify, IdentityError,
    Keypair, Principal, PrincipalId, PrincipalKind,
};
pub use operator::*;
pub use registry::{Governance, Registry, RegistryEntry, RegistryError, VersionStatus};
