#![cfg(target_os = "linux")]

use super::trusted_open::{
    open_existing_with_hook, validate_directory_metadata, validate_regular_leaf, TrustedOpenStage,
};
use rusqlite::Connection;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;
use tempfile::TempDir;

fn private_directory(path: &Path) {
    fs::create_dir(path).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

fn write_new(path: &Path, bytes: &[u8]) {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
}

fn seed_sqlite(path: &Path, marker: &str) {
    let connection = Connection::open(path).unwrap();
    connection
        .execute(
            &format!("CREATE TABLE {marker} (value INTEGER NOT NULL)"),
            [],
        )
        .unwrap();
    connection
        .execute(&format!("INSERT INTO {marker} VALUES (7)"), [])
        .unwrap();
    drop(connection);
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

#[test]
fn wrong_effective_owner_is_rejected_by_the_metadata_barrier() {
    let directory = TempDir::new().unwrap();
    let metadata = directory.path().metadata().unwrap();
    let wrong_uid = metadata.uid().wrapping_add(1);

    let error = validate_directory_metadata(&metadata, wrong_uid, "test directory").unwrap_err();

    assert_eq!(
        match error {
            crate::StorageError::Io(error) => error.kind(),
            other => panic!("unexpected error: {other}"),
        },
        std::io::ErrorKind::PermissionDenied
    );

    let database = directory.path().join("proof.db");
    write_new(&database, b"");
    let metadata = database.metadata().unwrap();
    let wrong_uid = metadata.uid().wrapping_add(1);
    let error = validate_regular_leaf(&metadata, wrong_uid, "test database").unwrap_err();
    assert_eq!(
        match error {
            crate::StorageError::Io(error) => error.kind(),
            other => panic!("unexpected error: {other}"),
        },
        std::io::ErrorKind::PermissionDenied
    );
}

#[test]
fn directory_substitution_before_sqlite_open_is_rejected_without_touching_replacement() {
    let parent = TempDir::new().unwrap();
    let trusted = parent.path().join("trusted");
    let replacement = parent.path().join("replacement");
    let displaced = parent.path().join("displaced");
    private_directory(&trusted);
    private_directory(&replacement);
    write_new(&trusted.join("proof.db"), b"");
    let replacement_bytes = b"replacement directory sentinel";
    write_new(&replacement.join("proof.db"), replacement_bytes);
    let directory = File::open(&trusted).unwrap();

    let error = open_existing_with_hook(directory, &trusted, "proof.db", |stage| {
        if stage == TrustedOpenStage::AfterDescriptorValidation {
            fs::rename(&trusted, &displaced)?;
            fs::rename(&replacement, &trusted)?;
        }
        Ok(())
    })
    .err()
    .unwrap();

    assert!(error
        .to_string()
        .contains("directory pathname no longer identifies"));
    assert_eq!(
        fs::read(trusted.join("proof.db")).unwrap(),
        replacement_bytes
    );
    assert_eq!(fs::metadata(displaced.join("proof.db")).unwrap().len(), 0);
}

#[test]
fn database_substitution_before_sqlite_open_is_rejected_without_touching_replacement() {
    let directory = TempDir::new().unwrap();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let database = directory.path().join("proof.db");
    let replacement = directory.path().join("replacement.db");
    let displaced = directory.path().join("displaced.db");
    write_new(&database, b"");
    let replacement_bytes = b"replacement database sentinel";
    write_new(&replacement, replacement_bytes);
    let handle = File::open(directory.path()).unwrap();

    let error = open_existing_with_hook(handle, directory.path(), "proof.db", |stage| {
        if stage == TrustedOpenStage::AfterDescriptorValidation {
            fs::rename(&database, &displaced)?;
            fs::rename(&replacement, &database)?;
        }
        Ok(())
    })
    .err()
    .unwrap();

    assert!(error
        .to_string()
        .contains("database pathname no longer identifies"));
    assert_eq!(fs::read(&database).unwrap(), replacement_bytes);
    assert_eq!(fs::metadata(&displaced).unwrap().len(), 0);
}

#[test]
fn database_substitution_after_sqlite_open_trips_moved_barrier_before_migration() {
    let directory = TempDir::new().unwrap();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let database = directory.path().join("proof.db");
    let replacement = directory.path().join("replacement.db");
    let displaced = directory.path().join("displaced.db");
    seed_sqlite(&database, "original_marker");
    seed_sqlite(&replacement, "replacement_marker");
    let replacement_bytes = fs::read(&replacement).unwrap();
    let handle = File::open(directory.path()).unwrap();

    let error = open_existing_with_hook(handle, directory.path(), "proof.db", |stage| {
        if stage == TrustedOpenStage::AfterSqliteOpen {
            fs::rename(&database, &displaced)?;
            fs::rename(&replacement, &database)?;
        }
        Ok(())
    })
    .err()
    .unwrap();

    assert!(error
        .to_string()
        .contains("reports that the trusted database pathname moved"));
    assert_eq!(fs::read(&database).unwrap(), replacement_bytes);
    let replacement = Connection::open(&database).unwrap();
    let marker: i64 = replacement
        .query_row("SELECT value FROM replacement_marker", [], |row| row.get(0))
        .unwrap();
    assert_eq!(marker, 7);
    assert!(replacement
        .prepare("SELECT 1 FROM schema_migrations")
        .is_err());
    let original = Connection::open(&displaced).unwrap();
    assert!(original.prepare("SELECT 1 FROM schema_migrations").is_err());
}
