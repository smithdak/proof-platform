#![forbid(unsafe_code)]
//! Volatile, independently signed Human authentication for the operator plane.

mod authority;
mod error;
mod types;

pub use authority::{
    challenge_code, challenge_signed_bytes_digest, challenge_signing_bytes, client_nonce_digest,
    public_key_fingerprint, OperatorAuthAuthority,
};
pub use error::{AuthorizedCallError, OperatorAuthError};
pub use proof_kernel::{Capability, CapabilitySet, ControlDigest, SessionAuthorityBinding};
pub use types::{
    AuthPolicy, AuthorizedSession, ChallengeIssueRequest, ChallengeIssueResponse,
    SessionAttestation, SessionChallenge, SessionClaims, SessionExchangeRequest,
    SessionExchangeResponse, SessionHeaderValue, SessionToken, ALL_CAPABILITIES,
    CHALLENGE_TTL_SECONDS, SESSION_ABSOLUTE_TTL_SECONDS, SESSION_IDLE_TTL_SECONDS,
};

#[cfg(test)]
mod tests;
