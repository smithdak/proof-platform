//! Native no-follow opening for an existing database in a trusted directory.

use super::store::SqliteStore;
use crate::StorageError;
use std::fs::File;
use std::io::{Error, ErrorKind};
use std::path::Path;

impl SqliteStore {
    /// Opens an existing SQLite database inside a validated private directory.
    ///
    /// This Linux-only seam is defense in depth for a fresh, current-user-owned
    /// workspace. SQLite retains ordinary pathname semantics, so callers must
    /// exclude hostile same-effective-user and root namespace mutation. The
    /// method never creates a missing database.
    pub fn open_existing_nofollow_in_trusted_directory(
        directory: File,
        expected_directory: &Path,
        database_name: &str,
    ) -> Result<Self, StorageError> {
        #[cfg(target_os = "linux")]
        {
            linux::open_existing_with_hook(directory, expected_directory, database_name, |_| Ok(()))
        }

        #[cfg(not(target_os = "linux"))]
        {
            let _ = (directory, expected_directory, database_name);
            Err(StorageError::Io(Error::new(
                ErrorKind::Unsupported,
                "trusted existing SQLite open requires Linux",
            )))
        }
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use crate::sqlite::migrations::run_migrations;
    use rusqlite::{ffi, Connection, OpenFlags};
    use std::ffi::CString;
    use std::fs::{Metadata, OpenOptions};
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::raw::{c_char, c_int};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    use std::path::Component;

    const O_RDWR: c_int = 0o2;
    const O_CLOEXEC: c_int = 0o2000000;
    const O_DIRECTORY: c_int = 0o200000;
    const O_NOFOLLOW: c_int = 0o400000;
    const O_PATH: c_int = 0o10000000;

    const SQLITE_FLAGS: OpenFlags = OpenFlags::SQLITE_OPEN_READ_WRITE
        .union(OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .union(OpenFlags::SQLITE_OPEN_NOFOLLOW);

    unsafe extern "C" {
        fn openat(directory: c_int, pathname: *const c_char, flags: c_int, ...) -> c_int;
        fn geteuid() -> u32;
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(crate) enum TrustedOpenStage {
        AfterDescriptorValidation,
        AfterSqliteOpen,
    }

    pub(crate) fn open_existing_with_hook<F>(
        directory: File,
        expected_directory: &Path,
        database_name: &str,
        mut hook: F,
    ) -> Result<SqliteStore, StorageError>
    where
        F: FnMut(TrustedOpenStage) -> Result<(), StorageError>,
    {
        validate_expected_directory_path(expected_directory)?;
        validate_database_name(database_name)?;

        // SAFETY: geteuid takes no arguments and has no memory-safety
        // preconditions.
        let effective_uid = unsafe { geteuid() };
        let directory_metadata = directory.metadata()?;
        validate_directory_metadata(&directory_metadata, effective_uid, "directory descriptor")?;
        verify_directory_path(&directory_metadata, expected_directory, effective_uid)?;

        let database = open_database_guard(&directory, database_name, effective_uid)?;
        let database_metadata = database.metadata()?;
        validate_existing_sidecars(&directory, database_name, effective_uid)?;

        hook(TrustedOpenStage::AfterDescriptorValidation)?;
        verify_guarded_paths(
            &directory_metadata,
            &database_metadata,
            expected_directory,
            database_name,
            effective_uid,
        )?;
        validate_existing_sidecars(&directory, database_name, effective_uid)?;

        let database_path = expected_directory.join(database_name);
        let connection = Connection::open_with_flags(&database_path, SQLITE_FLAGS)?;

        hook(TrustedOpenStage::AfterSqliteOpen)?;
        require_database_read_write(&connection)?;
        require_database_not_moved(&connection)?;
        verify_guarded_paths(
            &directory_metadata,
            &database_metadata,
            expected_directory,
            database_name,
            effective_uid,
        )?;
        validate_existing_sidecars(&directory, database_name, effective_uid)?;

        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        run_migrations(&connection)?;

        require_database_not_moved(&connection)?;
        verify_guarded_paths(
            &directory_metadata,
            &database_metadata,
            expected_directory,
            database_name,
            effective_uid,
        )?;
        validate_existing_sidecars(&directory, database_name, effective_uid)?;

        Ok(SqliteStore::from_trusted_existing_connection(
            connection, directory, database,
        ))
    }

    fn validate_expected_directory_path(path: &Path) -> Result<(), StorageError> {
        if !path.is_absolute() || path.as_os_str().as_bytes().contains(&0) {
            return Err(invalid_input(
                "trusted SQLite directory path must be an absolute ordinary path",
            ));
        }
        let mut components = path.components();
        if components.next() != Some(Component::RootDir)
            || components.any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(invalid_input(
                "trusted SQLite directory path contains a non-ordinary component",
            ));
        }
        Ok(())
    }

    fn validate_database_name(name: &str) -> Result<(), StorageError> {
        let path = Path::new(name);
        let is_one_normal_component = matches!(
            path.components().collect::<Vec<_>>().as_slice(),
            [Component::Normal(component)] if *component == path.as_os_str()
        );
        if name.is_empty()
            || name == "."
            || name == ".."
            || name.contains("..")
            || name
                .chars()
                .any(|character| character.is_control() || "/\\\0?:#%".contains(character))
            || !is_one_normal_component
        {
            return Err(invalid_input(
                "trusted SQLite database name must be one ordinary non-URI component",
            ));
        }
        Ok(())
    }

    pub(crate) fn validate_directory_metadata(
        metadata: &Metadata,
        effective_uid: u32,
        description: &str,
    ) -> Result<(), StorageError> {
        if !metadata.is_dir() {
            return Err(invalid_data(format!(
                "trusted SQLite {description} is not a directory"
            )));
        }
        if metadata.uid() != effective_uid {
            return Err(permission_denied(format!(
                "trusted SQLite {description} is not owned by the effective user"
            )));
        }
        if metadata.mode() & 0o077 != 0 {
            return Err(permission_denied(format!(
                "trusted SQLite {description} grants group or other permissions"
            )));
        }
        Ok(())
    }

    pub(crate) fn validate_regular_leaf(
        metadata: &Metadata,
        effective_uid: u32,
        description: &str,
    ) -> Result<(), StorageError> {
        if !metadata.is_file() {
            return Err(invalid_data(format!(
                "trusted SQLite {description} is not a regular file"
            )));
        }
        if metadata.uid() != effective_uid {
            return Err(permission_denied(format!(
                "trusted SQLite {description} is not owned by the effective user"
            )));
        }
        if metadata.nlink() != 1 {
            return Err(invalid_data(format!(
                "trusted SQLite {description} must have exactly one link"
            )));
        }
        Ok(())
    }

    fn open_database_guard(
        directory: &File,
        database_name: &str,
        effective_uid: u32,
    ) -> Result<File, StorageError> {
        let path_guard = open_at(directory, database_name, O_PATH | O_CLOEXEC | O_NOFOLLOW)?;
        let path_metadata = path_guard.metadata()?;
        validate_regular_leaf(&path_metadata, effective_uid, "database leaf")?;

        let database = open_at(directory, database_name, O_RDWR | O_CLOEXEC | O_NOFOLLOW)?;
        let database_metadata = database.metadata()?;
        validate_regular_leaf(&database_metadata, effective_uid, "database descriptor")?;
        require_same_identity(
            &path_metadata,
            &database_metadata,
            "database changed while its guard was opened",
        )?;
        Ok(database)
    }

    fn validate_existing_sidecars(
        directory: &File,
        database_name: &str,
        effective_uid: u32,
    ) -> Result<(), StorageError> {
        for suffix in ["-wal", "-shm", "-journal"] {
            let name = format!("{database_name}{suffix}");
            let sidecar = match open_at(directory, &name, O_PATH | O_CLOEXEC | O_NOFOLLOW) {
                Ok(sidecar) => sidecar,
                Err(StorageError::Io(error)) if error.kind() == ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            validate_regular_leaf(
                &sidecar.metadata()?,
                effective_uid,
                &format!("sidecar `{name}`"),
            )?;
        }
        Ok(())
    }

    fn verify_guarded_paths(
        directory_metadata: &Metadata,
        database_metadata: &Metadata,
        expected_directory: &Path,
        database_name: &str,
        effective_uid: u32,
    ) -> Result<(), StorageError> {
        verify_directory_path(directory_metadata, expected_directory, effective_uid)?;
        let path_guard = open_path_guard(&expected_directory.join(database_name), false)?;
        let path_metadata = path_guard.metadata()?;
        validate_regular_leaf(&path_metadata, effective_uid, "database pathname")?;
        require_same_identity(
            database_metadata,
            &path_metadata,
            "trusted SQLite database pathname no longer identifies its guard",
        )
    }

    fn verify_directory_path(
        directory_metadata: &Metadata,
        expected_directory: &Path,
        effective_uid: u32,
    ) -> Result<(), StorageError> {
        let path_guard = open_path_guard(expected_directory, true)?;
        let path_metadata = path_guard.metadata()?;
        validate_directory_metadata(&path_metadata, effective_uid, "directory pathname")?;
        require_same_identity(
            directory_metadata,
            &path_metadata,
            "trusted SQLite directory pathname no longer identifies its descriptor",
        )
    }

    fn open_path_guard(path: &Path, directory: bool) -> Result<File, StorageError> {
        let mut options = OpenOptions::new();
        options.read(true);
        let mut flags = O_PATH | O_CLOEXEC | O_NOFOLLOW;
        if directory {
            flags |= O_DIRECTORY;
        }
        options.custom_flags(flags).open(path).map_err(Into::into)
    }

    fn open_at(directory: &File, name: &str, flags: c_int) -> Result<File, StorageError> {
        let name = CString::new(name)
            .map_err(|_| invalid_input("trusted SQLite descriptor-relative name contains NUL"))?;
        // SAFETY: the directory descriptor and C string are valid for the
        // duration of the call. None of the call sites supplies O_CREAT, so no
        // variadic mode argument is required.
        let descriptor = unsafe { openat(directory.as_raw_fd(), name.as_ptr(), flags) };
        if descriptor < 0 {
            return Err(StorageError::Io(Error::last_os_error()));
        }
        // SAFETY: openat returned a new owned descriptor and ownership is
        // transferred exactly once to File.
        Ok(unsafe { File::from_raw_fd(descriptor) })
    }

    fn require_same_identity(
        expected: &Metadata,
        actual: &Metadata,
        message: &str,
    ) -> Result<(), StorageError> {
        if expected.dev() != actual.dev() || expected.ino() != actual.ino() {
            return Err(invalid_data(message));
        }
        Ok(())
    }

    fn require_database_not_moved(connection: &Connection) -> Result<(), StorageError> {
        let mut moved: c_int = 0;
        // SAFETY: the connection is exclusively owned during construction;
        // `main` is NUL-terminated, and SQLite writes one integer to `moved`.
        let result = unsafe {
            ffi::sqlite3_file_control(
                connection.handle(),
                b"main\0".as_ptr().cast(),
                ffi::SQLITE_FCNTL_HAS_MOVED,
                (&mut moved as *mut c_int).cast(),
            )
        };
        if result == ffi::SQLITE_NOTFOUND {
            return Err(StorageError::Io(Error::new(
                ErrorKind::Unsupported,
                "SQLite target does not support the required moved-file check",
            )));
        }
        if result != ffi::SQLITE_OK {
            return Err(StorageError::Database(rusqlite::Error::SqliteFailure(
                ffi::Error::new(result),
                Some("SQLite moved-file check failed".to_string()),
            )));
        }
        if moved != 0 {
            return Err(invalid_data(
                "SQLite reports that the trusted database pathname moved",
            ));
        }
        Ok(())
    }

    fn require_database_read_write(connection: &Connection) -> Result<(), StorageError> {
        // SAFETY: the connection is exclusively owned during construction and
        // `main` is a valid NUL-terminated database name.
        let read_only =
            unsafe { ffi::sqlite3_db_readonly(connection.handle(), b"main\0".as_ptr().cast()) };
        match read_only {
            0 => Ok(()),
            1 => Err(permission_denied(
                "trusted SQLite database opened read-only instead of read-write",
            )),
            _ => Err(invalid_data(
                "trusted SQLite connection has no main database",
            )),
        }
    }

    fn invalid_input(message: impl Into<String>) -> StorageError {
        StorageError::Io(Error::new(ErrorKind::InvalidInput, message.into()))
    }

    fn invalid_data(message: impl Into<String>) -> StorageError {
        StorageError::Io(Error::new(ErrorKind::InvalidData, message.into()))
    }

    fn permission_denied(message: impl Into<String>) -> StorageError {
        StorageError::Io(Error::new(ErrorKind::PermissionDenied, message.into()))
    }
}

#[cfg(all(test, target_os = "linux"))]
pub(super) use linux::{
    open_existing_with_hook, validate_directory_metadata, validate_regular_leaf, TrustedOpenStage,
};
