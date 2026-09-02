//! Closed authentication failures used by the protected transport boundary.

use thiserror::Error;

/// Closed failure taxonomy for operator authentication.
///
/// The transport deliberately maps all credential/state failures to the same
/// `authentication_required` response. No variant contains attacker-controlled
/// text or secret material.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum OperatorAuthError {
    #[error("the authentication request is invalid")]
    InvalidRequest,
    #[error("operator authentication is required")]
    AuthenticationRequired,
    #[error("the session lacks the required capability")]
    CapabilityRequired,
    #[error("an authentication challenge is already pending")]
    ChallengePending,
    #[error("operator authentication is unavailable")]
    ControlUnavailable,
}

/// Separates an authentication rejection from a protected callback failure.
#[derive(Debug, PartialEq, Eq)]
pub enum AuthorizedCallError<E> {
    Auth(OperatorAuthError),
    Callback(E),
}

impl<E> From<OperatorAuthError> for AuthorizedCallError<E> {
    fn from(error: OperatorAuthError) -> Self {
        Self::Auth(error)
    }
}
