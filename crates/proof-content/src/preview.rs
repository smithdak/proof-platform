use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

use chrono::{DateTime, Utc};
use proof_kernel::{
    canonicalize, digest, ApprovalExecution, ApprovalGrant, ArtifactKind, ContentDigest,
    ExecutionContext, ExecutionError, ExecutionOutcome, Keypair, Principal, Proof,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;
use uuid::Uuid;

use crate::{digest::canonical_digest, edition::Edition, object::Object};

const MANIFEST_SCHEMA: &str = "proof-content-preview-manifest/v1";
const ARTIFACT_SCHEMA: &str = "proof-content-preview-artifact/v1";
const ARTIFACT_DIGEST_SCHEMA: &str = "proof-content-preview-artifact-digest/v1";

#[cfg(test)]
static TEST_SYNC_FAILURE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[derive(Debug, Error)]
pub enum PreviewArtifactError {
    #[error("preview request is invalid: {0}")]
    Request(String),
    #[error("preview artifact I/O failed: {0}")]
    Io(String),
    #[error("preview artifact is invalid: {0}")]
    Invalid(String),
    #[error("preview proof is invalid: {0}")]
    Proof(String),
}

impl From<PreviewArtifactError> for ExecutionError {
    fn from(error: PreviewArtifactError) -> Self {
        ExecutionError::HandlerFailed(error.to_string())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PreviewRequest {
    pub idempotency_key: Uuid,
    pub edition_id: Uuid,
    pub environment: String,
    pub version_label: String,
    pub manifest_digest: String,
}

impl PreviewRequest {
    pub(crate) fn parse(input: &Value) -> Result<Self, PreviewArtifactError> {
        validate_request_text(input)?;
        let request: Self = serde_json::from_value(input.clone())
            .map_err(|error| PreviewArtifactError::Request(error.to_string()))?;
        if request.idempotency_key.get_version_num() != 7 {
            return Err(PreviewArtifactError::Request(
                "idempotency_key must be UUIDv7".to_string(),
            ));
        }
        if request.environment != "preview" {
            return Err(PreviewArtifactError::Request(
                "environment must be preview".to_string(),
            ));
        }
        if !valid_version_label(&request.version_label) {
            return Err(PreviewArtifactError::Request(
                "version_label must be 1-64 ASCII characters matching ^[0-9A-Za-z][0-9A-Za-z._-]{0,63}$"
                    .to_string(),
            ));
        }
        if !valid_sha256_digest(&request.manifest_digest) {
            return Err(PreviewArtifactError::Request(
                "manifest_digest must be sha256:<64 lowercase hex>".to_string(),
            ));
        }
        Ok(request)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PreviewManifestObject {
    object_id: Uuid,
    locale: String,
    content_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PreviewManifest {
    schema: String,
    edition_id: Uuid,
    edition_content_digest: String,
    objects: Vec<PreviewManifestObject>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PreviewArtifact {
    schema: String,
    publication_id: Uuid,
    request: PreviewArtifactRequest,
    request_digest: String,
    manifest: PreviewManifest,
    created_at: DateTime<Utc>,
    created_by: proof_kernel::PrincipalId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PreviewArtifactRequest {
    idempotency_key: Uuid,
    edition_id: Uuid,
    environment: String,
    version_label: String,
    manifest_digest: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PreviewOutput {
    operation: String,
    data: PreviewOutputData,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PreviewOutputData {
    publication_id: Uuid,
    edition_id: Uuid,
    environment: String,
    version_label: String,
    manifest_digest: String,
    artifact: PreviewOutputArtifact,
    published_at: DateTime<Utc>,
    published_by: proof_kernel::PrincipalId,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PreviewOutputArtifact {
    schema: String,
    relative_path: String,
    digest: String,
}

pub(crate) fn publish_preview(
    input: &Value,
    context: &ExecutionContext,
) -> Result<Value, PreviewArtifactError> {
    let request = PreviewRequest::parse(input)?;
    let manifest = load_manifest(&context.workspace_path, &request)?;
    let request_digest = operation_input_digest(input)?;
    let artifact = PreviewArtifact {
        schema: ARTIFACT_SCHEMA.to_string(),
        publication_id: request.idempotency_key,
        request: artifact_request(&request),
        request_digest: request_digest.to_string(),
        manifest,
        created_at: context.timestamp,
        created_by: context.actor,
    };
    let artifact_bytes = canonical_bytes(&artifact)?;
    let relative_path = preview_relative_path(&request);
    publish_immutable(&context.workspace_path, &request, &artifact_bytes)?;
    let artifact_digest = artifact_digest(&artifact)?;
    Ok(json!({
        "operation": "release.publish",
        "data": {
            "publication_id": request.idempotency_key,
            "edition_id": request.edition_id,
            "environment": "preview",
            "version_label": request.version_label,
            "manifest_digest": request.manifest_digest,
            "artifact": {
                "schema": ARTIFACT_SCHEMA,
                "relative_path": relative_path,
                "digest": artifact_digest.to_string(),
            },
            "published_at": context.timestamp,
            "published_by": context.actor,
        }
    }))
}

/// Independently verifies the persisted artifact and its binding to an engine outcome.
pub fn verify_preview_publication(
    workspace_path: &Path,
    input: &Value,
    outcome: &ExecutionOutcome,
    signing_principal: &Principal,
) -> Result<(), PreviewArtifactError> {
    let request = PreviewRequest::parse(input)?;
    let output: PreviewOutput = serde_json::from_value(outcome.output.clone())
        .map_err(|error| PreviewArtifactError::Invalid(error.to_string()))?;
    if output.operation != "release.publish" {
        return Err(PreviewArtifactError::Invalid(
            "wrong output operation".to_string(),
        ));
    }
    let expected_path = preview_relative_path(&request);
    if output.data.publication_id != request.idempotency_key
        || output.data.edition_id != request.edition_id
        || output.data.environment != "preview"
        || output.data.version_label != request.version_label
        || output.data.manifest_digest != request.manifest_digest
        || output.data.artifact.schema != ARTIFACT_SCHEMA
        || output.data.artifact.relative_path != expected_path
    {
        return Err(PreviewArtifactError::Invalid(
            "output does not bind the preview request".to_string(),
        ));
    }
    validate_preview_relative_path(&output.data.artifact.relative_path, &request)?;
    let bytes = read_preview_artifact(workspace_path, &request)?;
    verify_preview_directory_integrity(workspace_path, &request)?;
    let artifact: PreviewArtifact = serde_json::from_slice(&bytes)
        .map_err(|error| PreviewArtifactError::Invalid(error.to_string()))?;
    if canonical_bytes(&artifact)? != bytes {
        return Err(PreviewArtifactError::Invalid(
            "artifact bytes are not canonical".to_string(),
        ));
    }
    let manifest = load_manifest(workspace_path, &request)?;
    if artifact.schema != ARTIFACT_SCHEMA
        || artifact.publication_id != request.idempotency_key
        || artifact.request != artifact_request(&request)
        || artifact.request_digest != operation_input_digest(input)?.to_string()
        || artifact.manifest != manifest
        || artifact.created_at != output.data.published_at
        || artifact.created_by != output.data.published_by
        || artifact_digest(&artifact)?.to_string() != output.data.artifact.digest
    {
        return Err(PreviewArtifactError::Invalid(
            "artifact does not bind request, manifest, or output".to_string(),
        ));
    }
    verify_proof(
        input,
        &outcome.output,
        &outcome.proof,
        signing_principal,
        &output.data,
    )?;
    Ok(())
}

/// Verifies the signed approval execution's binding to the original proof and output.
pub fn verify_preview_approval_execution(
    input: &Value,
    execution: &ApprovalExecution,
    outcome: &ExecutionOutcome,
    approval: &ApprovalGrant,
    requester: &Keypair,
    trusted_approver: &Principal,
) -> Result<(), PreviewArtifactError> {
    approval
        .verify_for_execution(
            requester,
            trusted_approver,
            "release.publish",
            "v2",
            input,
            outcome.proof.body.actor,
            execution.executed_at,
        )
        .map_err(|error| PreviewArtifactError::Proof(error.to_string()))?;
    if execution.request_id != approval.request.body.id
        || execution.output != outcome.output
        || execution.proof != outcome.proof
        || execution.executed_at != outcome.proof.body.timestamp
    {
        return Err(PreviewArtifactError::Invalid(
            "approval execution does not bind the original outcome".to_string(),
        ));
    }
    Ok(())
}

fn verify_proof(
    input: &Value,
    output: &Value,
    proof: &Proof,
    signing_principal: &Principal,
    data: &PreviewOutputData,
) -> Result<(), PreviewArtifactError> {
    proof
        .verify(&signing_principal.public_key)
        .map_err(|error| PreviewArtifactError::Proof(error.to_string()))?;
    let input_digest = operation_input_digest(input)?;
    let output_digest = operation_output_digest(output)?;
    if proof.body.operation != "release.publish::v2"
        || proof.body.actor != data.published_by
        || proof.body.input_digest != input_digest
        || proof.body.output_digest != output_digest
        || proof.body.timestamp != data.published_at
    {
        return Err(PreviewArtifactError::Proof(
            "proof does not bind the preview output".to_string(),
        ));
    }
    Ok(())
}

fn load_manifest(
    workspace_path: &Path,
    request: &PreviewRequest,
) -> Result<PreviewManifest, PreviewArtifactError> {
    let contents = read_edition_file(workspace_path, request.edition_id)?;
    let raw_edition: Value = serde_json::from_str(&contents)
        .map_err(|error| PreviewArtifactError::Invalid(format!("invalid edition: {error}")))?;
    validate_persisted_edition(&raw_edition)?;
    let edition: Edition = serde_json::from_value(raw_edition)
        .map_err(|error| PreviewArtifactError::Invalid(format!("invalid edition: {error}")))?;
    if edition.id != request.edition_id {
        return Err(PreviewArtifactError::Invalid(
            "edition ID does not match request".to_string(),
        ));
    }
    let mut objects: Vec<Object> = edition.objects.clone();
    objects.sort_by(|left, right| left.id.cmp(&right.id));
    let edition_content_digest = canonical_digest(&objects);
    if edition.content_digest != edition_content_digest {
        return Err(PreviewArtifactError::Invalid(
            "edition content digest does not match persisted objects".to_string(),
        ));
    }
    let manifest = PreviewManifest {
        schema: MANIFEST_SCHEMA.to_string(),
        edition_id: edition.id,
        edition_content_digest,
        objects: objects
            .iter()
            .map(|object| PreviewManifestObject {
                object_id: object.id,
                locale: object.locale.clone(),
                content_digest: canonical_digest(object),
            })
            .collect(),
    };
    if canonical_digest(&manifest) != request.manifest_digest {
        return Err(PreviewArtifactError::Invalid(
            "manifest digest does not match requested edition".to_string(),
        ));
    }
    Ok(manifest)
}

fn publish_immutable(
    workspace_path: &Path,
    request: &PreviewRequest,
    bytes: &[u8],
) -> Result<(), PreviewArtifactError> {
    validate_preview_relative_path(&preview_relative_path(request), request)?;
    let directory = open_preview_directory(workspace_path, request.edition_id, true)?;
    let final_name = format!("{}.json", request.idempotency_key);
    if let Some(existing) = open_existing_file(&directory, &final_name)? {
        return equal_existing(existing, bytes);
    }
    let temporary_name = format!(".{}.{}.tmp", final_name, Uuid::now_v7());
    let mut file = create_new_file(&directory, &temporary_name)?;
    write_and_sync(&mut file, bytes)
        .map_err(|error| PreviewArtifactError::Io(error.to_string()))?;
    match hard_link_open_file_at(&file, &directory, &final_name) {
        Ok(()) => remove_file_at(&directory, &temporary_name),
        Err(error) if error.raw_os_error() == Some(17) => {
            match open_existing_file(&directory, &final_name)? {
                Some(existing) => equal_existing(existing, bytes),
                None => Err(PreviewArtifactError::Io(
                    "artifact appeared without a readable file".to_string(),
                )),
            }
        }
        Err(error) => Err(PreviewArtifactError::Io(format!(
            "atomic publish failed: {error}"
        ))),
    }
}

fn equal_existing(mut file: File, bytes: &[u8]) -> Result<(), PreviewArtifactError> {
    let mut existing = Vec::new();
    file.read_to_end(&mut existing)
        .map_err(|error| PreviewArtifactError::Io(error.to_string()))?;
    if existing == bytes {
        Ok(())
    } else {
        Err(PreviewArtifactError::Invalid(
            "preview artifact path already contains unequal bytes".to_string(),
        ))
    }
}

fn validate_preview_relative_path(
    relative_path: &str,
    request: &PreviewRequest,
) -> Result<(), PreviewArtifactError> {
    if relative_path != preview_relative_path(request)
        || relative_path.contains("..")
        || Path::new(relative_path).is_absolute()
    {
        return Err(PreviewArtifactError::Invalid(
            "artifact path escapes preview boundary".to_string(),
        ));
    }
    Ok(())
}

fn read_preview_artifact(
    workspace_path: &Path,
    request: &PreviewRequest,
) -> Result<Vec<u8>, PreviewArtifactError> {
    let directory = open_preview_directory(workspace_path, request.edition_id, false)?;
    let file_name = format!("{}.json", request.idempotency_key);
    let mut file = open_existing_file(&directory, &file_name)?
        .ok_or_else(|| PreviewArtifactError::Io("preview artifact does not exist".to_string()))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| PreviewArtifactError::Io(error.to_string()))?;
    Ok(bytes)
}

fn verify_preview_directory_integrity(
    workspace_path: &Path,
    request: &PreviewRequest,
) -> Result<(), PreviewArtifactError> {
    let directory = open_preview_directory(workspace_path, request.edition_id, false)?;
    let expected = format!("{}.json", request.idempotency_key);
    let entries = list_preview_entries(&directory)?;
    let mut final_count = 0_u8;
    for entry in entries {
        if entry.name == expected {
            if !entry.is_regular {
                return Err(PreviewArtifactError::Invalid(
                    "expected preview artifact is not a regular file".to_string(),
                ));
            }
            open_existing_file(&directory, &entry.name)?.ok_or_else(|| {
                PreviewArtifactError::Invalid("expected preview artifact disappeared".to_string())
            })?;
            final_count = final_count.saturating_add(1);
        } else if is_expected_temporary_name(&entry.name, &expected) {
            if !entry.is_regular {
                return Err(PreviewArtifactError::Invalid(
                    "preview temporary evidence is not a regular file".to_string(),
                ));
            }
        } else {
            return Err(PreviewArtifactError::Invalid(format!(
                "unexpected preview directory entry: {}",
                entry.name
            )));
        }
    }
    if final_count != 1 {
        return Err(PreviewArtifactError::Invalid(
            "preview directory must contain exactly one final artifact".to_string(),
        ));
    }
    Ok(())
}

fn is_expected_temporary_name(name: &str, expected_final_name: &str) -> bool {
    let Some(uuid_text) = name
        .strip_prefix(&format!(".{expected_final_name}."))
        .and_then(|value| value.strip_suffix(".tmp"))
    else {
        return false;
    };
    let Ok(uuid) = Uuid::parse_str(uuid_text) else {
        return false;
    };
    uuid.get_version_num() == 7 && uuid.to_string() == uuid_text
}

struct PreviewDirectoryEntry {
    name: String,
    is_regular: bool,
}

fn read_edition_file(
    workspace_path: &Path,
    edition_id: Uuid,
) -> Result<String, PreviewArtifactError> {
    let mut file = open_edition_file(workspace_path, edition_id)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .map_err(|error| PreviewArtifactError::Io(error.to_string()))?;
    Ok(contents)
}

fn write_and_sync(file: &mut File, bytes: &[u8]) -> std::io::Result<()> {
    file.write_all(bytes)?;
    #[cfg(test)]
    if TEST_SYNC_FAILURE.load(std::sync::atomic::Ordering::SeqCst) {
        return Err(std::io::Error::other("injected preview sync failure"));
    }
    file.sync_all()
}

#[cfg(target_os = "linux")]
fn open_preview_directory(
    workspace_path: &Path,
    edition_id: Uuid,
    create: bool,
) -> Result<File, PreviewArtifactError> {
    secure_linux::open_preview_directory(workspace_path, edition_id, create)
        .map_err(|error| PreviewArtifactError::Io(error.to_string()))
}

#[cfg(target_os = "linux")]
fn open_edition_file(
    workspace_path: &Path,
    edition_id: Uuid,
) -> Result<File, PreviewArtifactError> {
    secure_linux::open_edition_file(workspace_path, edition_id)
        .map_err(|error| PreviewArtifactError::Io(error.to_string()))
}

#[cfg(not(target_os = "linux"))]
fn open_edition_file(
    _workspace_path: &Path,
    _edition_id: Uuid,
) -> Result<File, PreviewArtifactError> {
    Err(PreviewArtifactError::Io(
        "symlink-safe edition I/O is unavailable on this platform".to_string(),
    ))
}

#[cfg(not(target_os = "linux"))]
fn open_preview_directory(
    _workspace_path: &Path,
    _edition_id: Uuid,
    _create: bool,
) -> Result<File, PreviewArtifactError> {
    Err(PreviewArtifactError::Io(
        "symlink-safe preview I/O is unavailable on this platform".to_string(),
    ))
}

#[cfg(target_os = "linux")]
fn open_existing_file(
    directory: &File,
    file_name: &str,
) -> Result<Option<File>, PreviewArtifactError> {
    secure_linux::open_existing_file(directory, file_name)
        .map_err(|error| PreviewArtifactError::Io(error.to_string()))
}

#[cfg(target_os = "linux")]
fn list_preview_entries(
    directory: &File,
) -> Result<Vec<PreviewDirectoryEntry>, PreviewArtifactError> {
    secure_linux::list_preview_entries(directory)
        .map_err(|error| PreviewArtifactError::Io(error.to_string()))
}

#[cfg(not(target_os = "linux"))]
fn list_preview_entries(
    _directory: &File,
) -> Result<Vec<PreviewDirectoryEntry>, PreviewArtifactError> {
    Err(PreviewArtifactError::Io(
        "symlink-safe preview I/O is unavailable on this platform".to_string(),
    ))
}

#[cfg(not(target_os = "linux"))]
fn open_existing_file(
    _directory: &File,
    _file_name: &str,
) -> Result<Option<File>, PreviewArtifactError> {
    Err(PreviewArtifactError::Io(
        "symlink-safe preview I/O is unavailable on this platform".to_string(),
    ))
}

#[cfg(target_os = "linux")]
fn create_new_file(directory: &File, file_name: &str) -> Result<File, PreviewArtifactError> {
    secure_linux::create_new_file(directory, file_name)
        .map_err(|error| PreviewArtifactError::Io(error.to_string()))
}

#[cfg(not(target_os = "linux"))]
fn create_new_file(_directory: &File, _file_name: &str) -> Result<File, PreviewArtifactError> {
    Err(PreviewArtifactError::Io(
        "symlink-safe preview I/O is unavailable on this platform".to_string(),
    ))
}

#[cfg(target_os = "linux")]
fn hard_link_open_file_at(source: &File, directory: &File, target: &str) -> std::io::Result<()> {
    secure_linux::hard_link_open_file_at(source, directory, target)
}

#[cfg(not(target_os = "linux"))]
fn hard_link_open_file_at(_source: &File, _directory: &File, _target: &str) -> std::io::Result<()> {
    Err(std::io::Error::other(
        "symlink-safe preview I/O is unavailable on this platform",
    ))
}

#[cfg(target_os = "linux")]
fn remove_file_at(directory: &File, file_name: &str) -> Result<(), PreviewArtifactError> {
    secure_linux::remove_file_at(directory, file_name)
        .map_err(|error| PreviewArtifactError::Io(error.to_string()))
}

#[cfg(not(target_os = "linux"))]
fn remove_file_at(_directory: &File, _file_name: &str) -> Result<(), PreviewArtifactError> {
    Err(PreviewArtifactError::Io(
        "symlink-safe preview I/O is unavailable on this platform".to_string(),
    ))
}

#[cfg(target_os = "linux")]
mod secure_linux {
    use std::ffi::CString;
    use std::fs::File;
    use std::io;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::path::Path;

    use uuid::Uuid;

    const O_RDONLY: i32 = 0;
    const O_WRONLY: i32 = 1;
    const O_CREAT: i32 = 0o100;
    const O_EXCL: i32 = 0o200;
    const O_DIRECTORY: i32 = 0o200000;
    const O_NOFOLLOW: i32 = 0o400000;
    const O_CLOEXEC: i32 = 0o2000000;
    const AT_FDCWD: i32 = -100;
    const AT_SYMLINK_FOLLOW: i32 = 0x400;

    unsafe extern "C" {
        fn openat(dirfd: i32, pathname: *const std::ffi::c_char, flags: i32, ...) -> i32;
        fn mkdirat(dirfd: i32, pathname: *const std::ffi::c_char, mode: u32) -> i32;
        fn linkat(
            olddirfd: i32,
            oldpath: *const std::ffi::c_char,
            newdirfd: i32,
            newpath: *const std::ffi::c_char,
            flags: i32,
        ) -> i32;
        fn unlinkat(dirfd: i32, pathname: *const std::ffi::c_char, flags: i32) -> i32;
    }

    pub(super) fn open_preview_directory(
        workspace_path: &Path,
        edition_id: Uuid,
        create: bool,
    ) -> io::Result<File> {
        let root = File::open(workspace_path)?;
        if !root.metadata()?.is_dir() {
            return Err(io::Error::other("workspace path is not a directory"));
        }
        let proof = open_directory(&root, ".proof", create)?;
        let data = open_directory(&proof, "data", create)?;
        let previews = open_directory(&data, "previews", create)?;
        open_directory(&previews, &edition_id.to_string(), create)
    }

    pub(super) fn open_edition_file(workspace_path: &Path, edition_id: Uuid) -> io::Result<File> {
        let root = File::open(workspace_path)?;
        if !root.metadata()?.is_dir() {
            return Err(io::Error::other("workspace path is not a directory"));
        }
        let proof = open_directory(&root, ".proof", false)?;
        let data = open_directory(&proof, "data", false)?;
        let editions = open_directory(&data, "editions", false)?;
        open_existing_file(&editions, &format!("{edition_id}.json"))?
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "edition does not exist"))
    }

    pub(super) fn open_existing_file(directory: &File, name: &str) -> io::Result<Option<File>> {
        let name = c_name(name)?;
        let fd = unsafe {
            openat(
                directory.as_raw_fd(),
                name.as_ptr(),
                O_RDONLY | O_NOFOLLOW | O_CLOEXEC,
            )
        };
        if fd < 0 {
            let error = io::Error::last_os_error();
            return if error.raw_os_error() == Some(2) {
                Ok(None)
            } else {
                Err(error)
            };
        }
        let file = unsafe { File::from_raw_fd(fd) };
        if !file.metadata()?.is_file() {
            return Err(io::Error::other("preview artifact is not a regular file"));
        }
        Ok(Some(file))
    }

    pub(super) fn list_preview_entries(
        directory: &File,
    ) -> io::Result<Vec<super::PreviewDirectoryEntry>> {
        let mut entries = Vec::new();
        let path = format!("/proc/self/fd/{}", directory.as_raw_fd());
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let name = entry.file_name().into_string().map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "non-UTF-8 preview entry")
            })?;
            let file_type = entry.file_type()?;
            if file_type.is_symlink() {
                return Err(io::Error::other("preview directory contains a symlink"));
            }
            entries.push(super::PreviewDirectoryEntry {
                name,
                is_regular: file_type.is_file(),
            });
        }
        Ok(entries)
    }

    pub(super) fn create_new_file(directory: &File, name: &str) -> io::Result<File> {
        let name = c_name(name)?;
        let fd = unsafe {
            openat(
                directory.as_raw_fd(),
                name.as_ptr(),
                O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC,
                0o600,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    pub(super) fn hard_link_open_file_at(
        source: &File,
        directory: &File,
        target: &str,
    ) -> io::Result<()> {
        let source_path = c_name(&format!("/proc/self/fd/{}", source.as_raw_fd()))?;
        let target = c_name(target)?;
        let result = unsafe {
            linkat(
                AT_FDCWD,
                source_path.as_ptr(),
                directory.as_raw_fd(),
                target.as_ptr(),
                AT_SYMLINK_FOLLOW,
            )
        };
        if result == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    pub(super) fn remove_file_at(directory: &File, name: &str) -> io::Result<()> {
        let name = c_name(name)?;
        let result = unsafe { unlinkat(directory.as_raw_fd(), name.as_ptr(), 0) };
        if result == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    fn open_directory(parent: &File, name: &str, create: bool) -> io::Result<File> {
        let name = c_name(name)?;
        let flags = O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC;
        let mut fd = unsafe { openat(parent.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 && io::Error::last_os_error().raw_os_error() == Some(2) && create {
            let result = unsafe { mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) };
            if result != 0 && io::Error::last_os_error().raw_os_error() != Some(17) {
                return Err(io::Error::last_os_error());
            }
            fd = unsafe { openat(parent.as_raw_fd(), name.as_ptr(), flags) };
        }
        if fd < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(unsafe { File::from_raw_fd(fd) })
        }
    }

    fn c_name(name: &str) -> io::Result<CString> {
        CString::new(name)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL path component"))
    }
}

fn preview_relative_path(request: &PreviewRequest) -> String {
    format!(
        ".proof/data/previews/{}/{}.json",
        request.edition_id, request.idempotency_key
    )
}

fn artifact_request(request: &PreviewRequest) -> PreviewArtifactRequest {
    PreviewArtifactRequest {
        idempotency_key: request.idempotency_key,
        edition_id: request.edition_id,
        environment: request.environment.clone(),
        version_label: request.version_label.clone(),
        manifest_digest: request.manifest_digest.clone(),
    }
}

fn canonical_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, PreviewArtifactError> {
    let value = serde_json::to_value(value)
        .map_err(|error| PreviewArtifactError::Invalid(error.to_string()))?;
    canonicalize(&value)
        .map(|value| value.as_bytes().to_vec())
        .map_err(|error| PreviewArtifactError::Invalid(error.to_string()))
}

fn operation_input_digest(input: &Value) -> Result<ContentDigest, PreviewArtifactError> {
    canonicalize(input)
        .map(|value| digest(ArtifactKind::OperationInput, &value))
        .map_err(|error| PreviewArtifactError::Invalid(error.to_string()))
}

fn operation_output_digest(output: &Value) -> Result<ContentDigest, PreviewArtifactError> {
    canonicalize(output)
        .map(|value| digest(ArtifactKind::OperationOutput, &value))
        .map_err(|error| PreviewArtifactError::Invalid(error.to_string()))
}

fn artifact_digest(artifact: &PreviewArtifact) -> Result<ContentDigest, PreviewArtifactError> {
    let envelope = json!({"schema": ARTIFACT_DIGEST_SCHEMA, "artifact": artifact});
    canonicalize(&envelope)
        .map(|value| digest(ArtifactKind::Generic, &value))
        .map_err(|error| PreviewArtifactError::Invalid(error.to_string()))
}

fn valid_version_label(label: &str) -> bool {
    let bytes = label.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && bytes[0].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn valid_sha256_digest(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value.as_bytes()[7..]
            .iter()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn validate_request_text(input: &Value) -> Result<(), PreviewArtifactError> {
    let object = input.as_object().ok_or_else(|| {
        PreviewArtifactError::Request("request must be a JSON object".to_string())
    })?;
    let idempotency_key = required_string(object, "idempotency_key")?;
    let edition_id = required_string(object, "edition_id")?;
    validate_canonical_uuid(idempotency_key, true, "idempotency_key")?;
    validate_canonical_uuid(edition_id, false, "edition_id")?;
    Ok(())
}

fn validate_canonical_uuid(
    value: &str,
    require_v7: bool,
    field: &str,
) -> Result<(), PreviewArtifactError> {
    let parsed = Uuid::parse_str(value).map_err(|_| {
        PreviewArtifactError::Request(format!("{field} must be a lowercase hyphenated UUID"))
    })?;
    if parsed.to_string() != value || (require_v7 && parsed.get_version_num() != 7) {
        return Err(PreviewArtifactError::Request(format!(
            "{field} must be a lowercase hyphenated {}UUID",
            if require_v7 { "v7 " } else { "" }
        )));
    }
    Ok(())
}

fn required_string<'a>(
    object: &'a serde_json::Map<String, Value>,
    field: &str,
) -> Result<&'a str, PreviewArtifactError> {
    object
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| PreviewArtifactError::Request(format!("{field} must be a string")))
}

fn validate_persisted_edition(value: &Value) -> Result<(), PreviewArtifactError> {
    let edition = value.as_object().ok_or_else(|| {
        PreviewArtifactError::Invalid("persisted edition must be an object".to_string())
    })?;
    reject_unknown_keys(
        edition,
        &[
            "id",
            "changeset_id",
            "objects",
            "created_at",
            "content_digest",
        ],
        "persisted edition",
    )?;
    let objects = edition
        .get("objects")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            PreviewArtifactError::Invalid("persisted edition objects must be an array".to_string())
        })?;
    for object in objects {
        let object = object.as_object().ok_or_else(|| {
            PreviewArtifactError::Invalid("persisted edition object must be an object".to_string())
        })?;
        reject_unknown_keys(
            object,
            &[
                "id",
                "schema_id",
                "schema_version",
                "locale",
                "content",
                "revision",
                "created_at",
                "updated_at",
                "status",
            ],
            "persisted edition object",
        )?;
    }
    Ok(())
}

fn reject_unknown_keys(
    object: &serde_json::Map<String, Value>,
    allowed: &[&str],
    name: &str,
) -> Result<(), PreviewArtifactError> {
    if let Some(key) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(PreviewArtifactError::Invalid(format!(
            "{name} contains unknown field: {key}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, OnceLock};

    use super::*;

    fn test_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn sync_failure_preserves_temporary_evidence_and_retry_does_not_delete_it() {
        let _guard = test_lock().lock().unwrap();
        let workspace = tempfile::TempDir::new().unwrap();
        let request = PreviewRequest {
            idempotency_key: Uuid::now_v7(),
            edition_id: Uuid::now_v7(),
            environment: "preview".to_string(),
            version_label: "test".to_string(),
            manifest_digest:
                "sha256:0000000000000000000000000000000000000000000000000000000000000000"
                    .to_string(),
        };
        TEST_SYNC_FAILURE.store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(
            publish_immutable(workspace.path(), &request, b"partial temporary evidence").is_err()
        );
        TEST_SYNC_FAILURE.store(false, std::sync::atomic::Ordering::SeqCst);

        let directory = workspace
            .path()
            .join(".proof/data/previews")
            .join(request.edition_id.to_string());
        let temporary_count = std::fs::read_dir(&directory)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
            .count();
        assert_eq!(temporary_count, 1);
        assert!(!directory
            .join(format!("{}.json", request.idempotency_key))
            .exists());

        publish_immutable(workspace.path(), &request, b"partial temporary evidence").unwrap();
        verify_preview_directory_integrity(workspace.path(), &request).unwrap();
        assert_eq!(
            std::fs::read_dir(&directory)
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
                .count(),
            1
        );
        assert_eq!(
            std::fs::read(directory.join(format!("{}.json", request.idempotency_key))).unwrap(),
            b"partial temporary evidence"
        );
    }
}
