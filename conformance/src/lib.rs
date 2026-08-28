//! JSON-driven conformance cases for proof-kernel invariants.

use proof_kernel::ArtifactKind;
use serde::Deserialize;
use serde_json::Value;
use std::path::Path;

pub fn load_case(path: impl AsRef<Path>) -> Result<Value, String> {
    let bytes = std::fs::read(path.as_ref()).map_err(|error| error.to_string())?;
    serde_json::from_slice(&bytes).map_err(|error| format!("{}: {error}", path.as_ref().display()))
}

pub fn project_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("conformance crate has a parent")
}

/// Computes the expected digest for a JSON case value and artifact kind.
pub fn expected_digest(input: &Value, artifact_kind: &str) -> Result<String, String> {
    let kind = match artifact_kind {
        "operation-input" => ArtifactKind::OperationInput,
        "operation-output" => ArtifactKind::OperationOutput,
        "proof" => ArtifactKind::Proof,
        "delegation" => ArtifactKind::Delegation,
        "generic" => ArtifactKind::Generic,
        unknown => return Err(format!("unknown artifact kind: {unknown}")),
    };
    let canonical = proof_kernel::canonicalize(input).map_err(|error| error.to_string())?;
    Ok(proof_kernel::digest(kind, &canonical).hex())
}

#[derive(Debug, Deserialize)]
pub struct DigestCase {
    pub name: String,
    pub artifact_kind: String,
    pub input: Value,
    pub expected_digest: String,
}

#[derive(Debug, Deserialize)]
pub struct ScopeCase {
    pub name: String,
    pub allowed_operations: Option<Vec<String>>,
    pub allowed_domains: Option<Vec<String>>,
    pub resource_scope: Option<String>,
    pub operation: String,
    pub domain: String,
    pub resource: String,
    pub expected_allowed: bool,
}

impl From<&ScopeCase> for proof_kernel::delegation::DelegationScope {
    fn from(case: &ScopeCase) -> Self {
        Self {
            allowed_operations: case.allowed_operations.clone(),
            allowed_domains: case.allowed_domains.clone(),
            resource_scope: case.resource_scope.clone(),
        }
    }
}
