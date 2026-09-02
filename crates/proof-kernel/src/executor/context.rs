//! Execution context and audit filtering types.

use crate::delegation::DelegationChain;
use crate::identity::{PrincipalId, PrincipalKind};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionContext {
    /// The Principal executing the operation.
    pub actor: PrincipalId,
    /// The kind of principal executing the operation. `None` is treated as an
    /// agent so that transports without principal-kind information remain
    /// conservative for human-only operations.
    pub principal_kind: Option<PrincipalKind>,
    /// The delegation under which this operation is authorized (if any).
    pub delegation_id: Option<Uuid>,
    /// The delegation chain validating the actor's authority (if any).
    pub delegation_chain: Option<DelegationChain>,
    /// Path to the workspace.
    pub workspace_path: PathBuf,
    /// When the execution started.
    pub timestamp: DateTime<Utc>,
}

#[derive(Clone, Debug, Default)]
pub struct AuditFilter {
    /// Restrict results to this operation identifier.
    pub operation: Option<String>,
    /// Restrict results to this principal.
    pub actor: Option<PrincipalId>,
    /// Only return contexts recorded at or after this time.
    pub since: Option<DateTime<Utc>>,
    /// Maximum number of contexts to return.
    pub limit: usize,
    /// Number of contexts to skip before returning results.
    pub offset: usize,
}

impl AuditFilter {
    const DEFAULT_LIMIT: usize = 20;
    const MAX_LIMIT: usize = 100;

    /// Returns a filter with the default limit.
    pub fn new() -> Self {
        Self {
            limit: Self::DEFAULT_LIMIT,
            ..Self::default()
        }
    }

    /// Clamps the limit to the supported range of 1 through 100.
    pub fn clamp_limit(&mut self) {
        self.limit = self.limit.clamp(1, Self::MAX_LIMIT);
    }
}
