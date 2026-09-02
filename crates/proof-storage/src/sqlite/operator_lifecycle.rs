//! Guarded schema-14 workspace lifecycle.

use super::migrations::run_migrations_through;
use super::store::SqliteStore;
#[cfg(target_os = "linux")]
use super::trusted_open::{open_existing_no_migration_with_hook, validate_existing_operator_files};
use proof_kernel::{
    control_digest, control_digest_serialized, AuditEvent, BudgetAccount, BudgetAccountState,
    BudgetAmounts, BudgetPolicy, CapabilitySet, ControlDigest, DescriptorIdentity, HumanEnrollment,
    InitializeWorkspaceRequest, OperatorControlEnvironment, OperatorProvisioningDocument,
    OperatorProvisioningError, OperatorSchemaCatalog, OperatorWorkspace, PrincipalBinding,
    PrincipalId, PrincipalKind, ProvisionOperatorWorkspaceResult, ProvisionOutcome,
    WorkspaceFingerprintInput,
};
use rusqlite::{params, Connection, TransactionBehavior};
use rustix::fd::OwnedFd;
use rustix::fs::{flock, fstat, openat, FileType, FlockOperation, Mode, OFlags};
use serde::{de::DeserializeOwned, Deserialize};
use std::collections::BTreeSet;
use std::fs::File;
use std::os::unix::fs::FileExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;

const DATABASE_NAME: &str = "storage.db";
const LOCK_NAME: &str = "operator-control.lock";
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperatorLockMode {
    Provisioning,
    ExistingOnly,
}

/// Nonforgeable ownership of the workspace-wide operator lock and descriptor set.
pub struct OwnedOperatorWorkspaceLock {
    workspace_path: PathBuf,
    proof_path: PathBuf,
    storage_path: PathBuf,
    workspace: File,
    proof: File,
    storage: File,
    database: File,
    lock_fd: Option<OwnedFd>,
    workspace_identity: FileIdentity,
    proof_identity: FileIdentity,
    storage_identity: FileIdentity,
    database_identity: FileIdentity,
    lock_identity: FileIdentity,
    provision: Option<ProvisionGuard>,
}

struct ProvisionGuard {
    path: PathBuf,
    file: File,
    identity: FileIdentity,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredWorkspaceConfig {
    actor_id: Uuid,
    version: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredAgentKeypair {
    principal_id: Uuid,
    kind: PrincipalKind,
    created_at: chrono::DateTime<chrono::Utc>,
    public_key: [u8; 32],
    signing_key: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct FileIdentity {
    device: u64,
    inode: u64,
}

impl FileIdentity {
    fn descriptor(self) -> Result<DescriptorIdentity, OperatorProvisioningError> {
        if self.device > MAX_SAFE_INTEGER || self.inode == 0 || self.inode > MAX_SAFE_INTEGER {
            return Err(OperatorProvisioningError::UnsafeWorkspace);
        }
        Ok(DescriptorIdentity {
            device: self.device,
            inode: self.inode,
        })
    }
}

pub fn acquire_operator_workspace_lock(
    workspace: &Path,
    provision: Option<&Path>,
    forbidden_proof_directories: &[&Path],
    mode: OperatorLockMode,
) -> Result<OwnedOperatorWorkspaceLock, OperatorProvisioningError> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (workspace, provision, forbidden_proof_directories, mode);
        return Err(OperatorProvisioningError::UnsupportedPlatform);
    }

    #[cfg(target_os = "linux")]
    {
        validate_lifecycle_arguments(workspace, provision, forbidden_proof_directories, mode)?;
        let workspace_file = open_absolute_directory(workspace, true)?;
        let proof_path = workspace.join(".proof");
        let proof = open_child_directory(&workspace_file, ".proof")?;
        let storage_path = proof_path.join("storage");
        let storage = open_child_directory(&proof, "storage")?;

        let workspace_identity = identity(&workspace_file)?;
        let proof_identity = identity(&proof)?;
        let storage_identity = identity(&storage)?;

        let mut forbidden = Vec::with_capacity(forbidden_proof_directories.len());
        for path in forbidden_proof_directories {
            let directory = open_absolute_directory(path, true)?;
            if identity(&directory)? == proof_identity {
                return Err(OperatorProvisioningError::UnsafeWorkspace);
            }
            forbidden.push(directory);
        }

        let lock_fd = match mode {
            OperatorLockMode::Provisioning => match openat(
                &proof,
                LOCK_NAME,
                OFlags::RDWR | OFlags::CREATE | OFlags::EXCL | OFlags::CLOEXEC | OFlags::NOFOLLOW,
                Mode::from_raw_mode(0o600),
            ) {
                Ok(fd) => fd,
                Err(error) if error == rustix::io::Errno::EXIST => open_lock_file(&proof)?,
                Err(_) => return Err(OperatorProvisioningError::UnsafeWorkspace),
            },
            OperatorLockMode::ExistingOnly => open_lock_file(&proof)?,
        };
        validate_regular_fd(&lock_fd, 0o600, true)?;
        flock(&lock_fd, FlockOperation::NonBlockingLockExclusive)
            .map_err(|_| OperatorProvisioningError::LockUnavailable)?;
        let lock_identity = identity_fd(&lock_fd)?;

        // Database and provisioning bytes are not touched until the lock is held.
        let database_fd = openat(
            &storage,
            DATABASE_NAME,
            OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        )
        .map_err(|_| OperatorProvisioningError::UnsafeWorkspace)?;
        validate_regular_fd(&database_fd, 0o600, false)?;
        let database = File::from(database_fd);
        let database_identity = identity(&database)?;

        let provision_guard = match provision {
            Some(path) => {
                let file = open_absolute_regular(path)?;
                let identity = identity(&file)?;
                Some(ProvisionGuard {
                    path: path.to_path_buf(),
                    file,
                    identity,
                })
            }
            None => None,
        };

        // Retain forbidden descriptors until all selected identities have been compared.
        drop(forbidden);
        Ok(OwnedOperatorWorkspaceLock {
            workspace_path: workspace.to_path_buf(),
            proof_path,
            storage_path,
            workspace: workspace_file,
            proof,
            storage,
            database,
            lock_fd: Some(lock_fd),
            workspace_identity,
            proof_identity,
            storage_identity,
            database_identity,
            lock_identity,
            provision: provision_guard,
        })
    }
}

pub fn upgrade_operator_schema14_offline(
    guard: &mut OwnedOperatorWorkspaceLock,
) -> Result<(), OperatorProvisioningError> {
    guard.verify_live()?;
    let (connection, directory, database) = guard.open_no_migration_connection()?;
    let version = schema_version_read_only(&connection)?;
    if version != 13 && version != 14 {
        return Err(OperatorProvisioningError::SchemaMismatch);
    }
    if version == 13 {
        run_migrations_through(&connection, 14, TransactionBehavior::Exclusive)
            .map_err(|_| OperatorProvisioningError::MigrationFailed)?;
    }
    if schema_version_read_only(&connection)? != 14 {
        return Err(OperatorProvisioningError::SchemaMismatch);
    }
    close_connection(connection)?;
    drop(database);
    drop(directory);
    guard.verify_live()
}

pub fn initialize_operator_workspace_guarded(
    guard: &mut OwnedOperatorWorkspaceLock,
    request: InitializeWorkspaceRequest,
    environment: Arc<dyn OperatorControlEnvironment>,
    catalog: Arc<OperatorSchemaCatalog>,
) -> Result<ProvisionOperatorWorkspaceResult, OperatorProvisioningError> {
    guard.verify_live()?;
    validate_initialize_request(&request, &catalog)?;
    verify_provision_document(guard, &request.provision)?;
    let (connection, directory, database) = guard.open_no_migration_connection()?;
    if schema_version_read_only(&connection)? != 14 {
        return Err(OperatorProvisioningError::SchemaMismatch);
    }

    let existing = load_workspace_row(&connection)?;
    let result = if let Some(existing) = existing {
        verify_existing_policy(&connection, guard, &request, &catalog, &existing)?;
        ProvisionOperatorWorkspaceResult {
            schema: "proof.operator.provision-workspace-result/v1".into(),
            outcome: ProvisionOutcome::ExactExisting,
            workspace_id: existing.workspace_id,
            schema_version: 14,
            workspace_binding_digest: existing.binding_digest,
            schema_catalog_digest: existing.schema_catalog_digest,
        }
    } else {
        create_policy(
            &connection,
            guard,
            request,
            environment.as_ref(),
            catalog.as_ref(),
        )?
    };
    close_connection(connection)?;
    drop(database);
    drop(directory);
    guard.verify_live()?;
    Ok(result)
}

pub fn open_operator_schema14_existing(
    guard: &mut OwnedOperatorWorkspaceLock,
    environment: Arc<dyn OperatorControlEnvironment>,
    catalog: Arc<OperatorSchemaCatalog>,
) -> Result<SqliteStore, OperatorProvisioningError> {
    guard.verify_live()?;
    let (connection, directory, database) = guard.open_no_migration_connection()?;
    if schema_version_read_only(&connection)? != 14 {
        return Err(OperatorProvisioningError::SchemaMismatch);
    }
    let workspace =
        load_workspace_row(&connection)?.ok_or(OperatorProvisioningError::PolicyMismatch)?;
    verify_persisted_policy(&connection, guard, &workspace, catalog.as_ref())?;
    Ok(SqliteStore::from_operator_existing_connection(
        connection,
        directory,
        database,
        environment,
        catalog,
    ))
}

pub fn release_operator_workspace_lock(
    mut guard: OwnedOperatorWorkspaceLock,
) -> Result<(), OperatorProvisioningError> {
    guard.verify_live()?;
    let lock = guard
        .lock_fd
        .take()
        .ok_or(OperatorProvisioningError::LockUnavailable)?;
    drop(lock);
    Ok(())
}

impl OwnedOperatorWorkspaceLock {
    fn open_no_migration_connection(
        &self,
    ) -> Result<(Connection, File, File), OperatorProvisioningError> {
        #[cfg(target_os = "linux")]
        {
            let directory = self
                .storage
                .try_clone()
                .map_err(|_| OperatorProvisioningError::StorageUnavailable)?;
            open_existing_no_migration_with_hook(
                directory,
                &self.storage_path,
                DATABASE_NAME,
                |_| Ok(()),
            )
            .map_err(|_| OperatorProvisioningError::StorageUnavailable)
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err(OperatorProvisioningError::UnsupportedPlatform)
        }
    }

    fn verify_live(&self) -> Result<(), OperatorProvisioningError> {
        let lock = self
            .lock_fd
            .as_ref()
            .ok_or(OperatorProvisioningError::LockUnavailable)?;
        if identity(&self.workspace)? != self.workspace_identity
            || identity(&self.proof)? != self.proof_identity
            || identity(&self.storage)? != self.storage_identity
            || identity(&self.database)? != self.database_identity
            || identity_fd(lock)? != self.lock_identity
        {
            return Err(OperatorProvisioningError::MovementDetected);
        }
        verify_path_identity(&self.workspace_path, self.workspace_identity, true)?;
        verify_path_identity(&self.proof_path, self.proof_identity, true)?;
        verify_path_identity(&self.storage_path, self.storage_identity, true)?;
        verify_path_identity(
            &self.storage_path.join(DATABASE_NAME),
            self.database_identity,
            false,
        )?;
        verify_path_identity(&self.proof_path.join(LOCK_NAME), self.lock_identity, false)?;
        if let Some(provision) = &self.provision {
            if identity(&provision.file)? != provision.identity {
                return Err(OperatorProvisioningError::MovementDetected);
            }
            verify_path_identity(&provision.path, provision.identity, false)?;
        }
        #[cfg(target_os = "linux")]
        validate_existing_operator_files(&self.storage, DATABASE_NAME)
            .map_err(|_| OperatorProvisioningError::UnsafeWorkspace)?;
        Ok(())
    }
}

fn validate_lifecycle_arguments(
    workspace: &Path,
    provision: Option<&Path>,
    forbidden: &[&Path],
    mode: OperatorLockMode,
) -> Result<(), OperatorProvisioningError> {
    if !ordinary_absolute(workspace)
        || forbidden.is_empty()
        || forbidden.iter().any(|path| !ordinary_absolute(path))
        || match mode {
            OperatorLockMode::Provisioning => provision.is_none(),
            OperatorLockMode::ExistingOnly => provision.is_some(),
        }
        || provision.is_some_and(|path| !ordinary_absolute(path))
    {
        return Err(OperatorProvisioningError::InvalidArguments);
    }
    Ok(())
}

fn ordinary_absolute(path: &Path) -> bool {
    let mut components = path.components();
    components.next() == Some(Component::RootDir)
        && components.all(|component| matches!(component, Component::Normal(_)))
}

fn open_absolute_directory(
    path: &Path,
    require_private: bool,
) -> Result<File, OperatorProvisioningError> {
    if !ordinary_absolute(path) {
        return Err(OperatorProvisioningError::InvalidArguments);
    }
    let mut directory = File::open("/").map_err(|_| OperatorProvisioningError::UnsafeWorkspace)?;
    for component in path.components().skip(1) {
        let Component::Normal(name) = component else {
            return Err(OperatorProvisioningError::InvalidArguments);
        };
        let fd = openat(
            &directory,
            name,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        )
        .map_err(|_| OperatorProvisioningError::UnsafeWorkspace)?;
        directory = File::from(fd);
    }
    if require_private {
        validate_directory(&directory)?;
    }
    Ok(directory)
}

fn open_child_directory(parent: &File, name: &str) -> Result<File, OperatorProvisioningError> {
    let fd = openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(|_| OperatorProvisioningError::UnsafeWorkspace)?;
    let file = File::from(fd);
    validate_directory(&file)?;
    Ok(file)
}

fn open_absolute_regular(path: &Path) -> Result<File, OperatorProvisioningError> {
    let parent = path
        .parent()
        .filter(|parent| ordinary_absolute(parent))
        .ok_or(OperatorProvisioningError::InvalidArguments)?;
    let name = path
        .file_name()
        .ok_or(OperatorProvisioningError::InvalidArguments)?;
    let directory = open_absolute_directory(parent, false)?;
    let fd = openat(
        &directory,
        name,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(|_| OperatorProvisioningError::UnsafeProvision)?;
    validate_regular_fd(&fd, 0o600, false)
        .map_err(|_| OperatorProvisioningError::UnsafeProvision)?;
    Ok(File::from(fd))
}

fn open_lock_file(proof: &File) -> Result<OwnedFd, OperatorProvisioningError> {
    openat(
        proof,
        LOCK_NAME,
        OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(|_| OperatorProvisioningError::UnsafeWorkspace)
}

fn validate_directory(file: &File) -> Result<(), OperatorProvisioningError> {
    let metadata = file
        .metadata()
        .map_err(|_| OperatorProvisioningError::UnsafeWorkspace)?;
    let uid = rustix::process::geteuid().as_raw();
    if !metadata.is_dir() || metadata.uid() != uid || metadata.mode() & 0o777 != 0o700 {
        return Err(OperatorProvisioningError::UnsafeWorkspace);
    }
    Ok(())
}

fn validate_regular_fd(
    fd: &OwnedFd,
    mode: u32,
    require_empty: bool,
) -> Result<(), OperatorProvisioningError> {
    let metadata = fstat(fd).map_err(|_| OperatorProvisioningError::UnsafeWorkspace)?;
    let uid = rustix::process::geteuid().as_raw();
    if FileType::from_raw_mode(metadata.st_mode) != FileType::RegularFile
        || metadata.st_uid != uid
        || metadata.st_nlink != 1
        || metadata.st_mode & 0o777 != mode
        || (require_empty && metadata.st_size != 0)
    {
        return Err(OperatorProvisioningError::UnsafeWorkspace);
    }
    Ok(())
}

fn identity(file: &File) -> Result<FileIdentity, OperatorProvisioningError> {
    let metadata = file
        .metadata()
        .map_err(|_| OperatorProvisioningError::MovementDetected)?;
    Ok(FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

fn identity_fd(fd: &OwnedFd) -> Result<FileIdentity, OperatorProvisioningError> {
    let metadata = fstat(fd).map_err(|_| OperatorProvisioningError::MovementDetected)?;
    Ok(FileIdentity {
        device: metadata.st_dev,
        inode: metadata.st_ino,
    })
}

fn verify_path_identity(
    path: &Path,
    expected: FileIdentity,
    directory: bool,
) -> Result<(), OperatorProvisioningError> {
    let actual = if directory {
        identity(&open_absolute_directory(path, true)?)?
    } else {
        identity(&open_absolute_regular(path)?)?
    };
    if actual != expected {
        return Err(OperatorProvisioningError::MovementDetected);
    }
    Ok(())
}

fn schema_version_read_only(connection: &Connection) -> Result<u32, OperatorProvisioningError> {
    connection
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )
        .map_err(|_| OperatorProvisioningError::SchemaMismatch)
}

fn close_connection(connection: Connection) -> Result<(), OperatorProvisioningError> {
    connection
        .close()
        .map_err(|_| OperatorProvisioningError::CloseFailed)
}

fn validate_initialize_request(
    request: &InitializeWorkspaceRequest,
    catalog: &OperatorSchemaCatalog,
) -> Result<(), OperatorProvisioningError> {
    if request.schema != "proof.operator.initialize-workspace-request/v1"
        || request.provision.schema != "proof.operator.provisioning-document/v1"
        || request.schema_catalog != *catalog.binding()
        || request.provision.capabilities != CapabilitySet::all()
    {
        return Err(OperatorProvisioningError::InvalidArguments);
    }
    Ok(())
}

fn verify_provision_document(
    guard: &OwnedOperatorWorkspaceLock,
    expected: &OperatorProvisioningDocument,
) -> Result<(), OperatorProvisioningError> {
    let provision = guard
        .provision
        .as_ref()
        .ok_or(OperatorProvisioningError::InvalidArguments)?;
    let actual: OperatorProvisioningDocument = strict_decode(&read_descriptor(&provision.file)?)?;
    if &actual != expected {
        return Err(OperatorProvisioningError::PolicyMismatch);
    }
    Ok(())
}

fn create_policy(
    connection: &Connection,
    guard: &OwnedOperatorWorkspaceLock,
    request: InitializeWorkspaceRequest,
    environment: &dyn OperatorControlEnvironment,
    catalog: &OperatorSchemaCatalog,
) -> Result<ProvisionOperatorWorkspaceResult, OperatorProvisioningError> {
    let now = environment
        .trusted_utc_now()
        .map_err(|_| OperatorProvisioningError::EnvironmentUnavailable)?;
    if request.provision.budget_deadline_at <= now {
        return Err(OperatorProvisioningError::InvalidArguments);
    }
    let workspace_id = environment
        .new_uuid_v7()
        .map_err(|_| OperatorProvisioningError::EnvironmentUnavailable)?;
    let budget_id = environment
        .new_uuid_v7()
        .map_err(|_| OperatorProvisioningError::EnvironmentUnavailable)?;
    let (agent, human) = load_and_validate_principals(connection, &request)?;
    let (agent_key, human_key) = validate_key_descriptors(
        guard,
        request.provision.agent_id,
        request.provision.human_id,
        &agent,
    )?;

    let fingerprint_input = WorkspaceFingerprintInput {
        schema: WorkspaceFingerprintInput::SCHEMA.into(),
        workspace_id,
        proof_directory: guard.proof_identity.descriptor()?,
        control_lock: guard.lock_identity.descriptor()?,
        agent_key_file: agent_key.descriptor()?,
        human_key_file: human_key.descriptor()?,
        agent_id: request.provision.agent_id,
        human_id: request.provision.human_id,
        agent_public_key: agent.public_key.clone(),
        human_public_key: human.public_key.clone(),
    };
    let workspace_fingerprint =
        control_digest_serialized("Proof-Operator-Workspace-v1", &fingerprint_input)
            .map_err(|_| OperatorProvisioningError::PolicyMismatch)?;
    let mut workspace = OperatorWorkspace {
        schema: OperatorWorkspace::SCHEMA.into(),
        workspace_id,
        database_name: DATABASE_NAME.into(),
        fingerprint_input,
        workspace_fingerprint,
        schema_catalog_digest: catalog.digest(),
        agent,
        human: human.clone(),
        auth_epoch: 1,
        policy_revision: 1,
        capabilities: request.provision.capabilities.clone(),
        created_at: now,
        updated_at: now,
        binding_digest: ControlDigest::from_bytes([0; 32]),
    };
    workspace.binding_digest = digest_without_field(
        "Proof-Operator-Workspace-Binding-v1",
        &workspace,
        "binding_digest",
    )?;
    workspace
        .validate()
        .map_err(|_| OperatorProvisioningError::PolicyMismatch)?;

    let enrollment = HumanEnrollment {
        schema: HumanEnrollment::SCHEMA.into(),
        workspace_id,
        human,
        capabilities: request.provision.capabilities.clone(),
        capability_set_digest: control_digest_serialized(
            "Proof-Operator-Capability-Set-v1",
            &request.provision.capabilities,
        )
        .map_err(|_| OperatorProvisioningError::PolicyMismatch)?,
        enrolled_at: now,
    };
    enrollment
        .validate()
        .map_err(|_| OperatorProvisioningError::PolicyMismatch)?;

    let mut policy = BudgetPolicy {
        schema: BudgetPolicy::SCHEMA.into(),
        budget_id,
        workspace_id,
        limits: request.provision.budget_limits,
        deadline_at: request.provision.budget_deadline_at,
        limits_digest: ControlDigest::from_bytes([0; 32]),
    };
    policy.limits_digest =
        digest_without_field("Proof-Operator-Budget-Limits-v1", &policy, "limits_digest")?;
    let account = BudgetAccount {
        schema: BudgetAccount::SCHEMA.into(),
        policy: policy.clone(),
        revision: 0,
        state: BudgetAccountState::Active,
        reserved: Default::default(),
        committed: Default::default(),
        created_at: now,
        updated_at: now,
    };
    account
        .validate()
        .map_err(|_| OperatorProvisioningError::PolicyMismatch)?;

    let transaction = connection
        .unchecked_transaction()
        .map_err(|_| OperatorProvisioningError::StorageUnavailable)?;
    transaction
        .execute(
            "INSERT INTO operator_workspaces
             (singleton, workspace_id, schema, database_name, fingerprint_json,
              workspace_fingerprint, schema_catalog_digest, binding_digest,
              agent_id, human_id, auth_epoch, policy_revision, capabilities_json,
              created_at, updated_at, binding_json)
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 1, 1, ?10, ?11, ?11, ?12)",
            params![
                workspace_id.to_string(),
                workspace.schema,
                workspace.database_name,
                strict_json(&workspace.fingerprint_input)?,
                workspace.workspace_fingerprint.to_string(),
                workspace.schema_catalog_digest.to_string(),
                workspace.binding_digest.to_string(),
                request.provision.agent_id.to_string(),
                request.provision.human_id.to_string(),
                strict_json(&workspace.capabilities)?,
                now.to_rfc3339(),
                strict_json(&workspace)?,
            ],
        )
        .map_err(|_| OperatorProvisioningError::StorageUnavailable)?;
    transaction
        .execute(
            "INSERT INTO operator_human_enrollments
             (workspace_id, human_id, schema, capability_set_digest, enrolled_at, enrollment_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                workspace_id.to_string(),
                request.provision.human_id.to_string(),
                enrollment.schema,
                enrollment.capability_set_digest.to_string(),
                now.to_rfc3339(),
                strict_json(&enrollment)?,
            ],
        )
        .map_err(|_| OperatorProvisioningError::StorageUnavailable)?;
    transaction
        .execute(
            "INSERT INTO operator_budget_accounts
             (budget_id, workspace_id, schema, revision, state,
              max_steps, max_tokens, max_duration_ms, max_cost_microusd, max_tool_dispatches,
              deadline_at, created_at, updated_at, limits_digest, limits_json)
             VALUES (?1, ?2, ?3, 0, 'active', ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10, ?11, ?12)",
            params![
                budget_id.to_string(),
                workspace_id.to_string(),
                account.schema,
                i64_safe(policy.limits.steps)?,
                i64_safe(policy.limits.tokens)?,
                i64_safe(policy.limits.duration_ms)?,
                i64_safe(policy.limits.cost_microusd)?,
                i64_safe(policy.limits.tool_dispatches)?,
                policy.deadline_at.to_rfc3339(),
                now.to_rfc3339(),
                policy.limits_digest.to_string(),
                strict_json(&policy)?,
            ],
        )
        .map_err(|_| OperatorProvisioningError::StorageUnavailable)?;
    transaction
        .execute(
            "INSERT INTO operator_audit_heads (workspace_id, last_sequence, last_digest)
             VALUES (?1, 0, NULL)",
            [workspace_id.to_string()],
        )
        .map_err(|_| OperatorProvisioningError::StorageUnavailable)?;
    transaction
        .commit()
        .map_err(|_| OperatorProvisioningError::StorageUnavailable)?;

    Ok(ProvisionOperatorWorkspaceResult {
        schema: "proof.operator.provision-workspace-result/v1".into(),
        outcome: ProvisionOutcome::Created,
        workspace_id,
        schema_version: 14,
        workspace_binding_digest: workspace.binding_digest,
        schema_catalog_digest: workspace.schema_catalog_digest,
    })
}

fn load_workspace_row(
    connection: &Connection,
) -> Result<Option<OperatorWorkspace>, OperatorProvisioningError> {
    struct RawWorkspace {
        workspace_id: String,
        schema: String,
        database_name: String,
        fingerprint_json: String,
        workspace_fingerprint: String,
        schema_catalog_digest: String,
        binding_digest: String,
        agent_id: String,
        human_id: String,
        auth_epoch: i64,
        policy_revision: i64,
        capabilities_json: String,
        created_at: String,
        updated_at: String,
        binding_json: String,
    }
    let row = connection
        .query_row(
            "SELECT workspace_id, schema, database_name, fingerprint_json,
                    workspace_fingerprint, schema_catalog_digest, binding_digest,
                    agent_id, human_id, auth_epoch, policy_revision, capabilities_json,
                    created_at, updated_at, binding_json
             FROM operator_workspaces WHERE singleton = 1",
            [],
            |row| {
                Ok(RawWorkspace {
                    workspace_id: row.get(0)?,
                    schema: row.get(1)?,
                    database_name: row.get(2)?,
                    fingerprint_json: row.get(3)?,
                    workspace_fingerprint: row.get(4)?,
                    schema_catalog_digest: row.get(5)?,
                    binding_digest: row.get(6)?,
                    agent_id: row.get(7)?,
                    human_id: row.get(8)?,
                    auth_epoch: row.get(9)?,
                    policy_revision: row.get(10)?,
                    capabilities_json: row.get(11)?,
                    created_at: row.get(12)?,
                    updated_at: row.get(13)?,
                    binding_json: row.get(14)?,
                })
            },
        )
        .optional()
        .map_err(|_| OperatorProvisioningError::StorageUnavailable)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let workspace: OperatorWorkspace = strict_decode(row.binding_json.as_bytes())?;
    workspace
        .validate()
        .map_err(|_| OperatorProvisioningError::PolicyMismatch)?;
    let duplicate_columns_match = row.workspace_id == workspace.workspace_id.to_string()
        && row.schema == workspace.schema
        && row.database_name == workspace.database_name
        && row.fingerprint_json == strict_json(&workspace.fingerprint_input)?
        && row.workspace_fingerprint == workspace.workspace_fingerprint.to_string()
        && row.schema_catalog_digest == workspace.schema_catalog_digest.to_string()
        && row.binding_digest == workspace.binding_digest.to_string()
        && row.agent_id == workspace.agent.principal_id.as_uuid().to_string()
        && row.human_id == workspace.human.principal_id.as_uuid().to_string()
        && row.auth_epoch == i64_safe(workspace.auth_epoch)?
        && row.policy_revision == i64_safe(workspace.policy_revision)?
        && row.capabilities_json == strict_json(&workspace.capabilities)?
        && row.created_at == workspace.created_at.to_rfc3339()
        && row.updated_at == workspace.updated_at.to_rfc3339();
    if !duplicate_columns_match {
        return Err(OperatorProvisioningError::PolicyMismatch);
    }
    Ok(Some(workspace))
}

fn verify_existing_policy(
    connection: &Connection,
    guard: &OwnedOperatorWorkspaceLock,
    request: &InitializeWorkspaceRequest,
    catalog: &OperatorSchemaCatalog,
    workspace: &OperatorWorkspace,
) -> Result<(), OperatorProvisioningError> {
    verify_persisted_policy(connection, guard, workspace, catalog)?;
    if workspace.schema_catalog_digest != catalog.digest()
        || workspace.agent.principal_id.as_uuid() != request.provision.agent_id
        || workspace.human.principal_id.as_uuid() != request.provision.human_id
        || workspace.agent.public_key_fingerprint != request.provision.agent_public_key_fingerprint
        || workspace.human.public_key_fingerprint != request.provision.human_public_key_fingerprint
        || workspace.capabilities != request.provision.capabilities
    {
        return Err(OperatorProvisioningError::PolicyMismatch);
    }
    let budget = load_budget_account(connection, workspace.workspace_id)?;
    if budget.policy.limits != request.provision.budget_limits
        || budget.policy.deadline_at != request.provision.budget_deadline_at
    {
        return Err(OperatorProvisioningError::PolicyMismatch);
    }
    Ok(())
}

fn verify_persisted_policy(
    connection: &Connection,
    guard: &OwnedOperatorWorkspaceLock,
    workspace: &OperatorWorkspace,
    catalog: &OperatorSchemaCatalog,
) -> Result<(), OperatorProvisioningError> {
    workspace
        .validate()
        .map_err(|_| OperatorProvisioningError::PolicyMismatch)?;
    if workspace.schema_catalog_digest != catalog.digest() {
        return Err(OperatorProvisioningError::CatalogMismatch);
    }
    for table in [
        "operator_workspaces",
        "operator_human_enrollments",
        "operator_budget_accounts",
        "operator_audit_heads",
    ] {
        let count: i64 = connection
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .map_err(|_| OperatorProvisioningError::StorageUnavailable)?;
        if count != 1 {
            return Err(OperatorProvisioningError::PolicyMismatch);
        }
    }
    verify_workspace_descriptors(connection, guard, workspace)?;
    let enrollment = load_human_enrollment(connection, workspace.workspace_id)?;
    if enrollment.workspace_id != workspace.workspace_id
        || enrollment.human != workspace.human
        || enrollment.capabilities != workspace.capabilities
        || enrollment.enrolled_at != workspace.created_at
    {
        return Err(OperatorProvisioningError::PolicyMismatch);
    }
    let budget = load_budget_account(connection, workspace.workspace_id)?;
    if budget.policy.workspace_id != workspace.workspace_id
        || budget.schema != BudgetAccount::SCHEMA
    {
        return Err(OperatorProvisioningError::PolicyMismatch);
    }
    verify_audit_head(connection, workspace.workspace_id)
}

fn load_human_enrollment(
    connection: &Connection,
    workspace_id: Uuid,
) -> Result<HumanEnrollment, OperatorProvisioningError> {
    let (raw_workspace, human_id, schema, capability_digest, enrolled_at, serialized): (
        String,
        String,
        String,
        String,
        String,
        String,
    ) = connection
        .query_row(
            "SELECT workspace_id, human_id, schema, capability_set_digest,
                    enrolled_at, enrollment_json
             FROM operator_human_enrollments WHERE workspace_id = ?1",
            [workspace_id.to_string()],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .map_err(|_| OperatorProvisioningError::PolicyMismatch)?;
    let enrollment: HumanEnrollment = strict_decode(serialized.as_bytes())?;
    enrollment
        .validate()
        .map_err(|_| OperatorProvisioningError::PolicyMismatch)?;
    if raw_workspace != enrollment.workspace_id.to_string()
        || human_id != enrollment.human.principal_id.as_uuid().to_string()
        || schema != enrollment.schema
        || capability_digest != enrollment.capability_set_digest.to_string()
        || enrolled_at != enrollment.enrolled_at.to_rfc3339()
        || serialized != strict_json(&enrollment)?
    {
        return Err(OperatorProvisioningError::PolicyMismatch);
    }
    Ok(enrollment)
}

fn load_budget_account(
    connection: &Connection,
    workspace_id: Uuid,
) -> Result<BudgetAccount, OperatorProvisioningError> {
    struct RawBudget {
        budget_id: String,
        workspace_id: String,
        schema: String,
        revision: i64,
        state: String,
        max: [i64; 5],
        reserved: [i64; 5],
        committed: [i64; 5],
        deadline_at: String,
        created_at: String,
        updated_at: String,
        limits_digest: String,
        limits_json: String,
    }
    let row = connection
        .query_row(
            "SELECT budget_id, workspace_id, schema, revision, state,
                    max_steps, max_tokens, max_duration_ms, max_cost_microusd,
                    max_tool_dispatches, reserved_steps, reserved_tokens,
                    reserved_duration_ms, reserved_cost_microusd,
                    reserved_tool_dispatches, committed_steps, committed_tokens,
                    committed_duration_ms, committed_cost_microusd,
                    committed_tool_dispatches, deadline_at, created_at, updated_at,
                    limits_digest, limits_json
             FROM operator_budget_accounts WHERE workspace_id = ?1",
            [workspace_id.to_string()],
            |row| {
                Ok(RawBudget {
                    budget_id: row.get(0)?,
                    workspace_id: row.get(1)?,
                    schema: row.get(2)?,
                    revision: row.get(3)?,
                    state: row.get(4)?,
                    max: [
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                        row.get(8)?,
                        row.get(9)?,
                    ],
                    reserved: [
                        row.get(10)?,
                        row.get(11)?,
                        row.get(12)?,
                        row.get(13)?,
                        row.get(14)?,
                    ],
                    committed: [
                        row.get(15)?,
                        row.get(16)?,
                        row.get(17)?,
                        row.get(18)?,
                        row.get(19)?,
                    ],
                    deadline_at: row.get(20)?,
                    created_at: row.get(21)?,
                    updated_at: row.get(22)?,
                    limits_digest: row.get(23)?,
                    limits_json: row.get(24)?,
                })
            },
        )
        .map_err(|_| OperatorProvisioningError::PolicyMismatch)?;
    let policy: BudgetPolicy = strict_decode(row.limits_json.as_bytes())?;
    let amounts = |values: [i64; 5]| -> Result<BudgetAmounts, OperatorProvisioningError> {
        Ok(BudgetAmounts {
            steps: u64_from_db(values[0])?,
            tokens: u64_from_db(values[1])?,
            duration_ms: u64_from_db(values[2])?,
            cost_microusd: u64_from_db(values[3])?,
            tool_dispatches: u64_from_db(values[4])?,
        })
    };
    let state = match row.state.as_str() {
        "active" => BudgetAccountState::Active,
        "closed" => BudgetAccountState::Closed,
        "exhausted" => BudgetAccountState::Exhausted,
        _ => return Err(OperatorProvisioningError::PolicyMismatch),
    };
    let account = BudgetAccount {
        schema: row.schema.clone(),
        policy,
        revision: u64_from_db(row.revision)?,
        state,
        reserved: amounts(row.reserved)?,
        committed: amounts(row.committed)?,
        created_at: row
            .created_at
            .parse()
            .map_err(|_| OperatorProvisioningError::PolicyMismatch)?,
        updated_at: row
            .updated_at
            .parse()
            .map_err(|_| OperatorProvisioningError::PolicyMismatch)?,
    };
    account
        .validate()
        .map_err(|_| OperatorProvisioningError::PolicyMismatch)?;
    if row.budget_id != account.policy.budget_id.to_string()
        || row.workspace_id != account.policy.workspace_id.to_string()
        || row.schema != account.schema
        || amounts(row.max)? != account.policy.limits
        || row.deadline_at != account.policy.deadline_at.to_rfc3339()
        || row.limits_digest != account.policy.limits_digest.to_string()
        || row.limits_json != strict_json(&account.policy)?
    {
        return Err(OperatorProvisioningError::PolicyMismatch);
    }
    Ok(account)
}

fn verify_audit_head(
    connection: &Connection,
    workspace_id: Uuid,
) -> Result<(), OperatorProvisioningError> {
    let (last_sequence, last_digest): (i64, Option<String>) = connection
        .query_row(
            "SELECT last_sequence, last_digest FROM operator_audit_heads
             WHERE workspace_id = ?1",
            [workspace_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| OperatorProvisioningError::PolicyMismatch)?;
    let mut statement = connection
        .prepare(
            "SELECT sequence, event_digest, event_json FROM operator_audit_events
             WHERE workspace_id = ?1 ORDER BY sequence ASC",
        )
        .map_err(|_| OperatorProvisioningError::StorageUnavailable)?;
    let mut rows = statement
        .query([workspace_id.to_string()])
        .map_err(|_| OperatorProvisioningError::StorageUnavailable)?;
    let mut expected_sequence = 1_u64;
    let mut previous_digest = None;
    while let Some(row) = rows
        .next()
        .map_err(|_| OperatorProvisioningError::StorageUnavailable)?
    {
        let raw_sequence: i64 = row
            .get(0)
            .map_err(|_| OperatorProvisioningError::PolicyMismatch)?;
        let raw_digest: String = row
            .get(1)
            .map_err(|_| OperatorProvisioningError::PolicyMismatch)?;
        let serialized: String = row
            .get(2)
            .map_err(|_| OperatorProvisioningError::PolicyMismatch)?;
        let event: AuditEvent = strict_decode(serialized.as_bytes())?;
        event
            .validate_chain_link(expected_sequence, previous_digest)
            .map_err(|_| OperatorProvisioningError::PolicyMismatch)?;
        if u64_from_db(raw_sequence)? != event.sequence
            || raw_digest != event.event_digest.to_string()
            || event.workspace_id != workspace_id
            || serialized != strict_json(&event)?
        {
            return Err(OperatorProvisioningError::PolicyMismatch);
        }
        previous_digest = Some(event.event_digest);
        expected_sequence = expected_sequence
            .checked_add(1)
            .ok_or(OperatorProvisioningError::PolicyMismatch)?;
    }
    let observed_last = expected_sequence - 1;
    if u64_from_db(last_sequence)? != observed_last
        || last_digest != previous_digest.map(|digest| digest.to_string())
    {
        return Err(OperatorProvisioningError::PolicyMismatch);
    }
    Ok(())
}

fn verify_workspace_descriptors(
    connection: &Connection,
    guard: &OwnedOperatorWorkspaceLock,
    workspace: &OperatorWorkspace,
) -> Result<(), OperatorProvisioningError> {
    let (agent_key, human_key) = validate_key_descriptors(
        guard,
        workspace.agent.principal_id.as_uuid(),
        workspace.human.principal_id.as_uuid(),
        &workspace.agent,
    )?;
    let expected = &workspace.fingerprint_input;
    if expected.proof_directory != guard.proof_identity.descriptor()?
        || expected.control_lock != guard.lock_identity.descriptor()?
        || expected.agent_key_file != agent_key.descriptor()?
        || expected.human_key_file != human_key.descriptor()?
    {
        return Err(OperatorProvisioningError::MovementDetected);
    }
    let agent = load_principal_binding(
        connection,
        workspace.agent.principal_id.as_uuid(),
        PrincipalKind::Agent,
    )?;
    let human = load_principal_binding(
        connection,
        workspace.human.principal_id.as_uuid(),
        PrincipalKind::Human,
    )?;
    if agent != workspace.agent || human != workspace.human {
        return Err(OperatorProvisioningError::PolicyMismatch);
    }
    Ok(())
}

fn load_and_validate_principals(
    connection: &Connection,
    request: &InitializeWorkspaceRequest,
) -> Result<(PrincipalBinding, PrincipalBinding), OperatorProvisioningError> {
    let agent =
        load_principal_binding(connection, request.provision.agent_id, PrincipalKind::Agent)?;
    let human =
        load_principal_binding(connection, request.provision.human_id, PrincipalKind::Human)?;
    if agent.public_key_fingerprint != request.provision.agent_public_key_fingerprint
        || human.public_key_fingerprint != request.provision.human_public_key_fingerprint
    {
        return Err(OperatorProvisioningError::PolicyMismatch);
    }
    Ok((agent, human))
}

fn load_principal_binding(
    connection: &Connection,
    id: Uuid,
    expected_kind: PrincipalKind,
) -> Result<PrincipalBinding, OperatorProvisioningError> {
    let (kind, bytes): (String, Vec<u8>) = connection
        .query_row(
            "SELECT kind, public_key FROM principals WHERE id = ?1",
            [id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| OperatorProvisioningError::PolicyMismatch)?;
    let actual_kind: PrincipalKind = strict_decode(kind.as_bytes())?;
    let bytes: [u8; 32] = bytes
        .try_into()
        .map_err(|_| OperatorProvisioningError::PolicyMismatch)?;
    if actual_kind != expected_kind {
        return Err(OperatorProvisioningError::PolicyMismatch);
    }
    let binding = PrincipalBinding {
        principal_id: PrincipalId::new(id),
        kind: actual_kind,
        public_key: base64url(&bytes),
        public_key_fingerprint: control_digest("Proof-Operator-Public-Key-v1", &bytes),
    };
    binding
        .validate()
        .map_err(|_| OperatorProvisioningError::PolicyMismatch)?;
    Ok(binding)
}

fn validate_key_descriptors(
    guard: &OwnedOperatorWorkspaceLock,
    agent_id: Uuid,
    human_id: Uuid,
    agent_binding: &PrincipalBinding,
) -> Result<(FileIdentity, FileIdentity), OperatorProvisioningError> {
    let config = open_regular_child(&guard.proof, "config.json")?;
    let agent = open_regular_child(&guard.proof, "keypair.json")?;
    let approvers = open_child_directory(&guard.proof, "approvers")?;
    let human_name = format!("{human_id}.json");
    let human = open_regular_child(&approvers, &human_name)?;
    let config: StoredWorkspaceConfig = strict_decode(&read_descriptor(&config)?)?;
    if config.actor_id != agent_id || config.version.is_empty() {
        return Err(OperatorProvisioningError::PolicyMismatch);
    }
    let stored_agent: StoredAgentKeypair = strict_decode(&read_descriptor(&agent)?)?;
    let expected_public: [u8; 32] = decode_base64url_32(&agent_binding.public_key)
        .ok_or(OperatorProvisioningError::PolicyMismatch)?;
    let signing_bytes = decode_standard_base64_32(&stored_agent.signing_key)
        .ok_or(OperatorProvisioningError::PolicyMismatch)?;
    let signing_key = ed25519_dalek::SigningKey::from_bytes(&signing_bytes);
    if stored_agent.principal_id != agent_id
        || stored_agent.kind != PrincipalKind::Agent
        || stored_agent.public_key != expected_public
        || signing_key.verifying_key().to_bytes() != expected_public
    {
        return Err(OperatorProvisioningError::PolicyMismatch);
    }
    let _ = stored_agent.created_at;
    Ok((identity(&agent)?, identity(&human)?))
}

fn open_regular_child(parent: &File, name: &str) -> Result<File, OperatorProvisioningError> {
    let fd = openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(|_| OperatorProvisioningError::UnsafeWorkspace)?;
    validate_regular_fd(&fd, 0o600, false)?;
    Ok(File::from(fd))
}

fn read_descriptor(file: &File) -> Result<Vec<u8>, OperatorProvisioningError> {
    const MAX_DOCUMENT_SIZE: u64 = 1_048_576;
    let length = file
        .metadata()
        .map_err(|_| OperatorProvisioningError::UnsafeWorkspace)?
        .len();
    if length == 0 || length > MAX_DOCUMENT_SIZE {
        return Err(OperatorProvisioningError::PolicyMismatch);
    }
    let mut bytes = vec![0; length as usize];
    let mut offset = 0;
    while offset < bytes.len() {
        let read = file
            .read_at(&mut bytes[offset..], offset as u64)
            .map_err(|_| OperatorProvisioningError::UnsafeWorkspace)?;
        if read == 0 {
            return Err(OperatorProvisioningError::MovementDetected);
        }
        offset += read;
    }
    let mut trailing = [0_u8; 1];
    if file
        .read_at(&mut trailing, length)
        .map_err(|_| OperatorProvisioningError::UnsafeWorkspace)?
        != 0
    {
        return Err(OperatorProvisioningError::MovementDetected);
    }
    Ok(bytes)
}

fn strict_decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, OperatorProvisioningError> {
    let mut duplicate_check = serde_json::Deserializer::from_slice(bytes);
    DuplicateCheckedValue::deserialize(&mut duplicate_check)
        .map_err(|_| OperatorProvisioningError::PolicyMismatch)?;
    duplicate_check
        .end()
        .map_err(|_| OperatorProvisioningError::PolicyMismatch)?;
    let mut typed = serde_json::Deserializer::from_slice(bytes);
    let result =
        T::deserialize(&mut typed).map_err(|_| OperatorProvisioningError::PolicyMismatch)?;
    typed
        .end()
        .map_err(|_| OperatorProvisioningError::PolicyMismatch)?;
    Ok(result)
}

struct DuplicateCheckedValue;

impl<'de> Deserialize<'de> for DuplicateCheckedValue {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = DuplicateCheckedValue;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("JSON without duplicate object names")
            }

            fn visit_bool<E>(self, _: bool) -> Result<Self::Value, E> {
                Ok(DuplicateCheckedValue)
            }
            fn visit_i64<E>(self, _: i64) -> Result<Self::Value, E> {
                Ok(DuplicateCheckedValue)
            }
            fn visit_u64<E>(self, _: u64) -> Result<Self::Value, E> {
                Ok(DuplicateCheckedValue)
            }
            fn visit_f64<E>(self, _: f64) -> Result<Self::Value, E> {
                Ok(DuplicateCheckedValue)
            }
            fn visit_str<E>(self, _: &str) -> Result<Self::Value, E> {
                Ok(DuplicateCheckedValue)
            }
            fn visit_string<E>(self, _: String) -> Result<Self::Value, E> {
                Ok(DuplicateCheckedValue)
            }
            fn visit_none<E>(self) -> Result<Self::Value, E> {
                Ok(DuplicateCheckedValue)
            }
            fn visit_unit<E>(self) -> Result<Self::Value, E> {
                Ok(DuplicateCheckedValue)
            }
            fn visit_some<D: serde::Deserializer<'de>>(
                self,
                deserializer: D,
            ) -> Result<Self::Value, D::Error> {
                DuplicateCheckedValue::deserialize(deserializer)
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> Result<Self::Value, A::Error> {
                while sequence.next_element::<DuplicateCheckedValue>()?.is_some() {}
                Ok(DuplicateCheckedValue)
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Self::Value, A::Error> {
                let mut names = BTreeSet::new();
                while let Some(name) = map.next_key::<String>()? {
                    if !names.insert(name.clone()) {
                        return Err(serde::de::Error::custom(format!(
                            "duplicate object name {name}"
                        )));
                    }
                    map.next_value::<DuplicateCheckedValue>()?;
                }
                Ok(DuplicateCheckedValue)
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

fn decode_base64url_32(value: &str) -> Option<[u8; 32]> {
    decode_base64_32(value, true)
}

fn decode_standard_base64_32(value: &str) -> Option<[u8; 32]> {
    decode_base64_32(value, false)
}

fn decode_base64_32(value: &str, url_safe: bool) -> Option<[u8; 32]> {
    let bytes = value.as_bytes();
    let expected_len = if url_safe { 43 } else { 44 };
    if bytes.len() != expected_len || (!url_safe && bytes[43] != b'=') {
        return None;
    }
    let mut output = [0_u8; 32];
    let mut bit_buffer = 0_u32;
    let mut bit_count = 0_u8;
    let mut written = 0_usize;
    for byte in bytes.iter().copied().filter(|byte| *byte != b'=') {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'-' if url_safe => 62,
            b'_' if url_safe => 63,
            b'+' if !url_safe => 62,
            b'/' if !url_safe => 63,
            _ => return None,
        };
        bit_buffer = (bit_buffer << 6) | u32::from(value);
        bit_count += 6;
        if bit_count >= 8 {
            bit_count -= 8;
            if written >= output.len() {
                return None;
            }
            output[written] = (bit_buffer >> bit_count) as u8;
            written += 1;
            bit_buffer &= (1_u32 << bit_count) - 1;
        }
    }
    if written != output.len() || bit_buffer != 0 {
        return None;
    }
    if url_safe {
        (base64url(&output) == value).then_some(output)
    } else {
        // For 32 bytes canonical standard base64 always has one trailing '='.
        Some(output)
    }
}

fn digest_without_field<T: serde::Serialize>(
    domain: &str,
    value: &T,
    field: &str,
) -> Result<ControlDigest, OperatorProvisioningError> {
    let mut value =
        serde_json::to_value(value).map_err(|_| OperatorProvisioningError::PolicyMismatch)?;
    value
        .as_object_mut()
        .ok_or(OperatorProvisioningError::PolicyMismatch)?
        .remove(field);
    control_digest_serialized(domain, &value).map_err(|_| OperatorProvisioningError::PolicyMismatch)
}

fn strict_json<T: serde::Serialize>(value: &T) -> Result<String, OperatorProvisioningError> {
    proof_kernel::canonicalize_serialized(value)
        .map(|canonical| canonical.to_string())
        .map_err(|_| OperatorProvisioningError::PolicyMismatch)
}

fn i64_safe(value: u64) -> Result<i64, OperatorProvisioningError> {
    if value > MAX_SAFE_INTEGER {
        return Err(OperatorProvisioningError::PolicyMismatch);
    }
    Ok(value as i64)
}

fn u64_from_db(value: i64) -> Result<u64, OperatorProvisioningError> {
    if value < 0 || value as u64 > MAX_SAFE_INTEGER {
        return Err(OperatorProvisioningError::PolicyMismatch);
    }
    Ok(value as u64)
}

fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut output = String::with_capacity((bytes.len() * 4).div_ceil(3));
    let mut index = 0;
    while index + 3 <= bytes.len() {
        let chunk = (u32::from(bytes[index]) << 16)
            | (u32::from(bytes[index + 1]) << 8)
            | u32::from(bytes[index + 2]);
        output.push(ALPHABET[((chunk >> 18) & 63) as usize] as char);
        output.push(ALPHABET[((chunk >> 12) & 63) as usize] as char);
        output.push(ALPHABET[((chunk >> 6) & 63) as usize] as char);
        output.push(ALPHABET[(chunk & 63) as usize] as char);
        index += 3;
    }
    let remaining = bytes.len() - index;
    if remaining == 1 {
        let chunk = u32::from(bytes[index]) << 16;
        output.push(ALPHABET[((chunk >> 18) & 63) as usize] as char);
        output.push(ALPHABET[((chunk >> 12) & 63) as usize] as char);
    } else if remaining == 2 {
        let chunk = (u32::from(bytes[index]) << 16) | (u32::from(bytes[index + 1]) << 8);
        output.push(ALPHABET[((chunk >> 18) & 63) as usize] as char);
        output.push(ALPHABET[((chunk >> 12) & 63) as usize] as char);
        output.push(ALPHABET[((chunk >> 6) & 63) as usize] as char);
    }
    output
}

use rusqlite::OptionalExtension;

#[cfg(test)]
mod tests {
    use super::super::migrations::MIGRATIONS;
    use super::*;

    #[test]
    fn base64url_is_unpadded_and_canonical() {
        assert_eq!(
            base64url(&[0; 32]),
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
        );
        assert_eq!(
            base64url(&[255; 32]),
            "__________________________________________8"
        );
    }

    #[test]
    fn migration_fourteen_is_the_unique_tail() {
        assert_eq!(
            MIGRATIONS.last().map(|migration| migration.version),
            Some(14)
        );
        assert_eq!(
            MIGRATIONS
                .iter()
                .filter(|migration| migration.version == 14)
                .count(),
            1
        );
    }
}
