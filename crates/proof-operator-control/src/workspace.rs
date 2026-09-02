use std::{
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use proof_kernel::{
    OperatorAuthorityAuditStore, OperatorControlEnvironment, OperatorDirectoryStore,
};

use crate::ControlShellError;

/// Validated, non-forgeable input delivered to the trusted existing-only opener.
pub struct TrustedStoreOpenRequest {
    workspace_root: PathBuf,
    authoritative_database: PathBuf,
    forbidden_proof_directories: Vec<PathBuf>,
}

impl TrustedStoreOpenRequest {
    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    pub fn authoritative_database(&self) -> &Path {
        &self.authoritative_database
    }

    pub fn forbidden_proof_directories(&self) -> &[PathBuf] {
        &self.forbidden_proof_directories
    }
}

/// Injected existing-only trusted workspace opener.
pub trait OperatorStoreOpener: Send + Sync {
    /// The trusted store remains owned by the prepared control plane. Its
    /// implementation must close the database and release its retained
    /// workspace lock when dropped on a failed preflight.
    type Store: OperatorDirectoryStore + OperatorAuthorityAuditStore + Send + Sync + 'static;

    fn open_existing(
        &self,
        request: &TrustedStoreOpenRequest,
        environment: Arc<dyn OperatorControlEnvironment>,
    ) -> Result<Self::Store, ControlShellError>;
}

/// Constructs the frozen path set and delegates all descriptor trust checks to the opener.
pub fn open_authoritative_store<O: OperatorStoreOpener>(
    workspace_root: &Path,
    environment: Arc<dyn OperatorControlEnvironment>,
    opener: &O,
) -> Result<O::Store, ControlShellError> {
    #[cfg(not(target_os = "linux"))]
    return Err(ControlShellError::UnsupportedPlatform);

    #[cfg(target_os = "linux")]
    {
        let request = trusted_store_open_request(workspace_root)?;
        opener.open_existing(&request, environment)
    }
}

pub(crate) fn trusted_store_open_request(
    workspace_root: &Path,
) -> Result<TrustedStoreOpenRequest, ControlShellError> {
    if !is_absolute_ordinary(workspace_root) {
        return Err(ControlShellError::UnsafeWorkspace);
    }
    let forbidden = mandatory_repository_proof_directory()?;
    if !is_absolute_ordinary(&forbidden) {
        return Err(ControlShellError::UnsafeWorkspace);
    }
    let selected_proof = workspace_root.join(".proof");
    if selected_proof == forbidden {
        return Err(ControlShellError::UnsafeWorkspace);
    }
    Ok(TrustedStoreOpenRequest {
        workspace_root: workspace_root.to_owned(),
        authoritative_database: selected_proof.join("storage").join("storage.db"),
        forbidden_proof_directories: vec![forbidden],
    })
}

/// Returns the one build-anchored repository `.proof` exclusion. No caller,
/// environment variable, current directory, argument, or configuration value
/// participates in this path.
pub fn mandatory_repository_proof_directory() -> Result<PathBuf, ControlShellError> {
    let manifest_directory = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repository_root = manifest_directory
        .parent()
        .and_then(Path::parent)
        .ok_or(ControlShellError::UnsafeWorkspace)?;
    let path = repository_root.join(".proof");
    if !is_absolute_ordinary(&path) {
        return Err(ControlShellError::UnsafeWorkspace);
    }
    Ok(path)
}

fn is_absolute_ordinary(path: &Path) -> bool {
    path.is_absolute()
        && path
            .components()
            .all(|part| matches!(part, Component::RootDir | Component::Normal(_)))
}
