//! Descriptor-pinned filesystem helpers for security-sensitive CLI material.

use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

#[cfg(target_os = "linux")]
use std::os::fd::AsRawFd;
#[cfg(target_os = "linux")]
use std::os::unix::fs::OpenOptionsExt;
#[cfg(target_os = "linux")]
use std::os::unix::fs::{MetadataExt, PermissionsExt};
#[cfg(target_os = "linux")]
use std::{ffi::CString, os::raw::c_char};

#[cfg(target_os = "linux")]
const O_DIRECTORY: i32 = 0o200000;
#[cfg(target_os = "linux")]
const O_NOFOLLOW: i32 = 0o400000;
#[cfg(target_os = "linux")]
const O_TMPFILE: i32 = 0o20200000;
#[cfg(target_os = "linux")]
const AT_FDCWD: i32 = -100;
#[cfg(target_os = "linux")]
const AT_SYMLINK_FOLLOW: i32 = 0x400;

#[cfg(target_os = "linux")]
unsafe extern "C" {
    fn linkat(
        old_directory: i32,
        old_path: *const c_char,
        new_directory: i32,
        new_path: *const c_char,
        flags: i32,
    ) -> i32;
    fn geteuid() -> u32;
}

pub(crate) struct SecureDirectory {
    handle: File,
}

impl SecureDirectory {
    pub(crate) fn open(path: &Path) -> Result<Self> {
        #[cfg(not(target_os = "linux"))]
        bail!("descriptor-relative secure filesystem operations require Linux");

        #[cfg(target_os = "linux")]
        {
            if std::fs::symlink_metadata(path)?.file_type().is_symlink() {
                bail!("secure directory path must not be a symbolic link");
            }
            let handle = OpenOptions::new()
                .read(true)
                .custom_flags(O_DIRECTORY | O_NOFOLLOW)
                .open(path)
                .with_context(|| {
                    format!("could not securely open directory: {}", path.display())
                })?;
            if !handle.metadata()?.is_dir() {
                bail!("secure filesystem path is not a directory");
            }
            Ok(Self { handle })
        }
    }

    pub(crate) fn open_child(&self, name: &str) -> Result<Self> {
        validate_name(name)?;
        Self::open(&self.child_path(name))
    }

    pub(crate) fn open_child_optional(&self, name: &str) -> Result<Option<Self>> {
        validate_name(name)?;
        let path = self.child_path(name);
        match std::fs::symlink_metadata(&path) {
            Ok(_) => Self::open(&path).map(Some),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub(crate) fn try_clone_handle(&self) -> Result<File> {
        Ok(self.handle.try_clone()?)
    }

    pub(crate) fn validate_private_current_user(&self, description: &str) -> Result<()> {
        #[cfg(not(target_os = "linux"))]
        bail!("private workspace validation requires Linux");

        #[cfg(target_os = "linux")]
        {
            let metadata = self.handle.metadata()?;
            // SAFETY: geteuid takes no arguments and has no memory-safety preconditions.
            let effective_uid = unsafe { geteuid() };
            if !metadata.is_dir()
                || metadata.uid() != effective_uid
                || metadata.permissions().mode() & 0o077 != 0
            {
                bail!(
                    "trusted workspace {description} must be a private current-user-owned directory"
                );
            }
            Ok(())
        }
    }

    pub(crate) fn ensure_child(&self, name: &str) -> Result<Self> {
        validate_name(name)?;
        let path = self.child_path(name);
        match std::fs::create_dir(&path) {
            Ok(()) => self.handle.sync_all()?,
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
        self.open_child(name)
    }

    pub(crate) fn read_optional(&self, name: &str) -> Result<Option<Vec<u8>>> {
        validate_name(name)?;
        let path = self.child_path(name);
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(target_os = "linux")]
        options.custom_flags(O_NOFOLLOW);
        let mut file = match options.open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        if !file.metadata()?.is_file() {
            bail!("secure filesystem leaf is not a regular file: {name}");
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        Ok(Some(bytes))
    }

    pub(crate) fn read_private_file(&self, name: &str) -> Result<Vec<u8>> {
        validate_name(name)?;
        let path = self.child_path(name);
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(target_os = "linux")]
        options.custom_flags(O_NOFOLLOW);
        let mut file = options
            .open(&path)
            .with_context(|| format!("trusted workspace file is missing: {name}"))?;
        let metadata = file.metadata()?;
        #[cfg(target_os = "linux")]
        {
            // SAFETY: geteuid takes no arguments and has no memory-safety preconditions.
            let effective_uid = unsafe { geteuid() };
            if !metadata.is_file()
                || metadata.uid() != effective_uid
                || metadata.nlink() != 1
                || metadata.permissions().mode() & 0o077 != 0
            {
                bail!(
                    "trusted workspace file must be a private current-user-owned single-link regular file: {name}"
                );
            }
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        Ok(bytes)
    }

    /// Durably publishes exact bytes without replacing an existing leaf.
    ///
    /// An unnamed same-directory file is synced before descriptor-based,
    /// no-replace publication. A process kill before publication leaves no
    /// partial pathname; a kill after publication is recovered by exact read.
    pub(crate) fn publish_exact(&self, name: &str, bytes: &[u8]) -> Result<bool> {
        validate_name(name)?;
        if let Some(existing) = self.read_optional(name)? {
            if existing != bytes {
                bail!("refusing to replace an existing preparation-bound file: {name}");
            }
            return Ok(false);
        }

        #[cfg(not(target_os = "linux"))]
        bail!("descriptor-based no-replace publication requires Linux");

        #[cfg(target_os = "linux")]
        {
            let mut options = OpenOptions::new();
            options.write(true).mode(0o600).custom_flags(O_TMPFILE);
            let mut temporary = options
                .open(self.descriptor_path())
                .context("could not create an unnamed preparation file")?;
            temporary.write_all(bytes)?;
            temporary.sync_all()?;
            let source = CString::new(format!("/proc/self/fd/{}", temporary.as_raw_fd()))?;
            let target = CString::new(name)?;
            // SAFETY: both C strings live through the call. The source is the
            // kernel-owned procfs link for the still-open unnamed inode, so no
            // user-controlled pathname can replace it between sync and link.
            let linked = unsafe {
                linkat(
                    AT_FDCWD,
                    source.as_ptr(),
                    self.handle.as_raw_fd(),
                    target.as_ptr(),
                    AT_SYMLINK_FOLLOW,
                )
            };
            if linked != 0 {
                let error = std::io::Error::last_os_error();
                if error.kind() == ErrorKind::AlreadyExists {
                    let existing = self
                        .read_optional(name)?
                        .context("preparation final file disappeared")?;
                    if existing != bytes {
                        bail!("preparation final file binding drift: {name}");
                    }
                    return Ok(false);
                }
                return Err(error.into());
            }
            self.handle.sync_all()?;
            let published = self
                .read_optional(name)?
                .context("published preparation file disappeared")?;
            if published != bytes {
                bail!("published preparation file failed exact-byte reread: {name}");
            }
            Ok(true)
        }
    }

    /// Atomically replaces one contained regular-file leaf without following
    /// an existing symbolic or hard link.
    pub(crate) fn replace_file(&self, name: &str, bytes: &[u8]) -> Result<()> {
        validate_name(name)?;

        #[cfg(not(target_os = "linux"))]
        bail!("descriptor-based replacement requires Linux");

        #[cfg(target_os = "linux")]
        {
            let temporary_name = format!(".replace-{}.tmp", uuid::Uuid::now_v7());
            let temporary_path = self.child_path(&temporary_name);
            let result = (|| -> Result<()> {
                let mut options = OpenOptions::new();
                options
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .custom_flags(O_NOFOLLOW);
                let mut temporary = options.open(&temporary_path)?;
                temporary.write_all(bytes)?;
                temporary.sync_all()?;
                std::fs::rename(&temporary_path, self.child_path(name))?;
                self.handle.sync_all()?;
                let published = self
                    .read_optional(name)?
                    .context("replacement file disappeared")?;
                if published != bytes {
                    bail!("replacement file failed exact-byte reread: {name}");
                }
                Ok(())
            })();
            if result.is_err() {
                match std::fs::remove_file(&temporary_path) {
                    Ok(()) => {}
                    Err(error) if error.kind() == ErrorKind::NotFound => {}
                    Err(_) => {}
                }
            }
            result
        }
    }

    pub(crate) fn exclusive_lock(&self, name: &str) -> Result<File> {
        validate_name(name)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(target_os = "linux")]
        options.mode(0o600).custom_flags(O_NOFOLLOW);
        let file = options.open(self.child_path(name))?;
        if !file.metadata()?.is_file() {
            bail!("preparation lock is not a regular file");
        }
        file.lock()?;
        Ok(file)
    }

    #[cfg(target_os = "linux")]
    fn child_path(&self, name: &str) -> PathBuf {
        self.descriptor_path().join(name)
    }

    #[cfg(target_os = "linux")]
    fn descriptor_path(&self) -> PathBuf {
        PathBuf::from(format!("/proc/self/fd/{}", self.handle.as_raw_fd()))
    }

    #[cfg(not(target_os = "linux"))]
    fn child_path(&self, _name: &str) -> PathBuf {
        unreachable!("SecureDirectory construction fails on unsupported platforms")
    }
}

pub(crate) fn open_trusted_absolute_directory(path: &Path) -> Result<(SecureDirectory, PathBuf)> {
    #[cfg(not(target_os = "linux"))]
    bail!("trusted workspace ancestry validation requires Linux");

    #[cfg(target_os = "linux")]
    {
        let absolute = lexical_absolute(path)?;
        let mut directory = SecureDirectory::open(Path::new("/"))?;
        validate_safe_ancestor(&directory, Path::new("/"))?;
        let mut walked = PathBuf::from("/");
        for component in absolute.components().skip(1) {
            let std::path::Component::Normal(name) = component else {
                bail!("trusted workspace path contains a non-ordinary component");
            };
            let name = name
                .to_str()
                .context("trusted workspace path is not valid UTF-8")?;
            directory = directory.open_child(name)?;
            walked.push(name);
            validate_safe_ancestor(&directory, &walked)?;
        }
        Ok((directory, absolute))
    }
}

pub(crate) fn open_descendant(root: &Path, components: &[&str]) -> Result<SecureDirectory> {
    let mut directory = SecureDirectory::open(root)?;
    for component in components {
        directory = directory.open_child(component)?;
    }
    Ok(directory)
}

fn validate_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || Path::new(name).components().count() != 1
        || name.contains('/')
        || name.contains('\\')
    {
        bail!("unsafe descriptor-relative filesystem name");
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn lexical_absolute(path: &Path) -> Result<PathBuf> {
    let input = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut normalized = PathBuf::from("/");
    for component in input.components() {
        match component {
            std::path::Component::RootDir | std::path::Component::CurDir => {}
            std::path::Component::Normal(component) => normalized.push(component),
            std::path::Component::ParentDir => {
                if !normalized.pop() {
                    bail!("trusted workspace path escapes the filesystem root");
                }
            }
            std::path::Component::Prefix(_) => {
                bail!("trusted workspace path contains an unsupported prefix")
            }
        }
    }
    Ok(normalized)
}

#[cfg(target_os = "linux")]
fn validate_safe_ancestor(directory: &SecureDirectory, path: &Path) -> Result<()> {
    let metadata = directory.handle.metadata()?;
    // SAFETY: geteuid takes no arguments and has no memory-safety preconditions.
    let effective_uid = unsafe { geteuid() };
    validate_safe_ancestor_metadata(
        metadata.is_dir(),
        metadata.uid(),
        metadata.permissions().mode(),
        effective_uid,
        path,
    )
}

#[cfg(target_os = "linux")]
fn validate_safe_ancestor_metadata(
    is_directory: bool,
    owner: u32,
    mode: u32,
    effective_uid: u32,
    path: &Path,
) -> Result<()> {
    let trusted_owner = owner == 0 || owner == effective_uid;
    let writable_by_other_uid = mode & 0o022 != 0;
    let safe_sticky_owner = mode & 0o1000 != 0 && trusted_owner;
    if !is_directory || !trusted_owner || (writable_by_other_uid && !safe_sticky_owner) {
        bail!(
            "trusted workspace ancestor is unsafe against other-user replacement: {}",
            path.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unnamed_publication_is_exact_idempotent_and_leaves_no_pending_path() {
        let temp = assert_fs::TempDir::new().unwrap();
        let directory = SecureDirectory::open(temp.path()).unwrap();
        let bytes = b"immutable preparation bytes";
        assert!(directory.publish_exact("final.json", bytes).unwrap());
        assert!(!directory.publish_exact("final.json", bytes).unwrap());
        assert_eq!(
            directory.read_optional("final.json").unwrap().unwrap(),
            bytes
        );
        let error = directory.publish_exact("final.json", b"drift").unwrap_err();
        assert!(error.to_string().contains("refusing to replace"));
        assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 1);
    }

    #[cfg(target_family = "unix")]
    #[test]
    fn descriptor_walk_rejects_symlink_component_and_leaf() {
        use std::os::unix::fs::symlink;

        let temp = assert_fs::TempDir::new().unwrap();
        std::fs::create_dir(temp.path().join("real")).unwrap();
        symlink(temp.path().join("real"), temp.path().join("linked")).unwrap();
        let root = SecureDirectory::open(temp.path()).unwrap();
        assert!(root.open_child("linked").is_err());

        std::fs::write(temp.path().join("real/target"), b"secret").unwrap();
        symlink(temp.path().join("real/target"), temp.path().join("leaf")).unwrap();
        assert!(root.read_optional("leaf").is_err());
    }

    #[cfg(target_family = "unix")]
    #[test]
    fn atomic_replacement_does_not_follow_existing_link_leaves() {
        use std::os::unix::fs::symlink;

        let directory = assert_fs::TempDir::new().unwrap();
        let outside = assert_fs::NamedTempFile::new("outside").unwrap();
        std::fs::write(outside.path(), b"outside").unwrap();
        symlink(outside.path(), directory.path().join("record.json")).unwrap();
        let secure = SecureDirectory::open(directory.path()).unwrap();

        secure.replace_file("record.json", b"imported").unwrap();

        assert_eq!(std::fs::read(outside.path()).unwrap(), b"outside");
        assert!(
            std::fs::symlink_metadata(directory.path().join("record.json"))
                .unwrap()
                .is_file()
        );
        assert_eq!(
            std::fs::read(directory.path().join("record.json")).unwrap(),
            b"imported"
        );
        secure.replace_file("record.json", b"updated").unwrap();
        assert_eq!(
            std::fs::read(directory.path().join("record.json")).unwrap(),
            b"updated"
        );

        let hardlink_target = assert_fs::NamedTempFile::new("hardlink-outside").unwrap();
        std::fs::write(hardlink_target.path(), b"hardlink-outside").unwrap();
        std::fs::hard_link(
            hardlink_target.path(),
            directory.path().join("hardlink.json"),
        )
        .unwrap();
        secure.replace_file("hardlink.json", b"contained").unwrap();
        assert_eq!(
            std::fs::read(hardlink_target.path()).unwrap(),
            b"hardlink-outside"
        );
        assert_eq!(
            std::fs::read(directory.path().join("hardlink.json")).unwrap(),
            b"contained"
        );
        assert!(secure.replace_file("../escape", b"blocked").is_err());
    }

    #[cfg(target_family = "unix")]
    #[test]
    fn trusted_ancestry_and_private_leaf_checks_fail_closed() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let temp = assert_fs::TempDir::new_in(env!("CARGO_MANIFEST_DIR")).unwrap();
        std::fs::set_permissions(temp.path(), PermissionsExt::from_mode(0o700)).unwrap();
        let child = temp.path().join("private-child");
        std::fs::create_dir(&child).unwrap();
        std::fs::set_permissions(&child, PermissionsExt::from_mode(0o700)).unwrap();
        let (directory, resolved) = open_trusted_absolute_directory(&child).unwrap();
        assert_eq!(resolved, child);
        directory
            .validate_private_current_user("test directory")
            .unwrap();

        let private = child.join("private.json");
        std::fs::write(&private, b"secret").unwrap();
        std::fs::set_permissions(&private, PermissionsExt::from_mode(0o600)).unwrap();
        assert_eq!(
            directory.read_private_file("private.json").unwrap(),
            b"secret"
        );
        std::fs::hard_link(&private, child.join("second-link")).unwrap();
        assert!(directory.read_private_file("private.json").is_err());
        std::fs::remove_file(child.join("second-link")).unwrap();
        std::fs::set_permissions(&private, PermissionsExt::from_mode(0o640)).unwrap();
        assert!(directory.read_private_file("private.json").is_err());

        let linked = temp.path().join("linked-child");
        symlink(&child, &linked).unwrap();
        assert!(open_trusted_absolute_directory(&linked).is_err());
        std::fs::set_permissions(temp.path(), PermissionsExt::from_mode(0o777)).unwrap();
        assert!(open_trusted_absolute_directory(&child).is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn ancestry_requires_a_trusted_owner_even_when_currently_read_only() {
        let path = Path::new("/synthetic-ancestor");
        let effective_uid = 1_000;
        validate_safe_ancestor_metadata(true, 0, 0o1777, effective_uid, path).unwrap();
        validate_safe_ancestor_metadata(true, effective_uid, 0o700, effective_uid, path).unwrap();
        let error =
            validate_safe_ancestor_metadata(true, 2_000, 0o555, effective_uid, path).unwrap_err();
        assert!(error.to_string().contains("other-user replacement"));
    }
}
