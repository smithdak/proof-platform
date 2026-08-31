#![cfg(target_os = "linux")]

use proof_kernel::{generate_keypair_for, principal_from_keypair, PrincipalKind};
use proof_storage::SqliteStore;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{symlink, OpenOptionsExt, PermissionsExt};
use std::path::Path;
use tempfile::TempDir;

fn make_private(directory: &Path) {
    fs::set_permissions(directory, fs::Permissions::from_mode(0o700)).unwrap();
}

fn create_file(path: &Path, bytes: &[u8]) {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
}

fn open_store(directory: &TempDir) -> Result<SqliteStore, proof_storage::StorageError> {
    SqliteStore::open_existing_nofollow_in_trusted_directory(
        File::open(directory.path()).unwrap(),
        directory.path(),
        "proof.db",
    )
}

#[test]
fn existing_database_migrates_round_trips_checkpoints_and_reopens() {
    let directory = TempDir::new().unwrap();
    make_private(directory.path());
    create_file(&directory.path().join("proof.db"), b"");

    let store = open_store(&directory).unwrap();
    let version: i64 = store
        .connection()
        .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(version, 12);
    let journal_mode: String = store
        .connection()
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap();
    assert_eq!(journal_mode, "wal");

    let keypair = generate_keypair_for(PrincipalKind::Agent);
    let principal = principal_from_keypair(&keypair);
    store.save_principal(&principal).unwrap();
    let loaded = store.load_principal(&principal.id).unwrap();
    assert_eq!(loaded.id, principal.id);
    store
        .connection()
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
        .unwrap();
    drop(store);

    let reopened = open_store(&directory).unwrap();
    assert_eq!(
        reopened.load_principal(&principal.id).unwrap().id,
        principal.id
    );
}

#[test]
fn missing_database_and_unsafe_names_fail_without_creation() {
    let directory = TempDir::new().unwrap();
    make_private(directory.path());

    assert!(open_store(&directory).is_err());
    assert!(!directory.path().join("proof.db").exists());

    for name in [
        "",
        ".",
        "..",
        "nested/proof.db",
        "nested\\proof.db",
        "proof..db",
        "file:proof.db",
        "proof.db?mode=rw",
        "proof.db#fragment",
        "%2e%2e",
        "proof\0db",
    ] {
        let result = SqliteStore::open_existing_nofollow_in_trusted_directory(
            File::open(directory.path()).unwrap(),
            directory.path(),
            name,
        );
        assert!(
            result.is_err(),
            "unsafe name unexpectedly accepted: {name:?}"
        );
    }
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
}

#[test]
fn database_symlink_and_hard_link_are_rejected_without_touching_targets() {
    let symlink_directory = TempDir::new().unwrap();
    make_private(symlink_directory.path());
    let symlink_target = symlink_directory.path().join("target.db");
    let target_bytes = b"database symlink target";
    create_file(&symlink_target, target_bytes);
    symlink(&symlink_target, symlink_directory.path().join("proof.db")).unwrap();

    assert!(open_store(&symlink_directory).is_err());
    assert_eq!(fs::read(&symlink_target).unwrap(), target_bytes);

    let hard_link_directory = TempDir::new().unwrap();
    make_private(hard_link_directory.path());
    let hard_link_target = hard_link_directory.path().join("target.db");
    create_file(&hard_link_target, b"database hard-link target");
    fs::hard_link(
        &hard_link_target,
        hard_link_directory.path().join("proof.db"),
    )
    .unwrap();
    let before = fs::read(&hard_link_target).unwrap();

    assert!(open_store(&hard_link_directory).is_err());
    assert_eq!(fs::read(&hard_link_target).unwrap(), before);
}

#[test]
fn existing_sidecar_symlink_and_hard_link_are_rejected_without_touching_targets() {
    let symlink_directory = TempDir::new().unwrap();
    make_private(symlink_directory.path());
    create_file(&symlink_directory.path().join("proof.db"), b"");
    let symlink_target = symlink_directory.path().join("wal-target");
    let target_bytes = b"sidecar symlink target";
    create_file(&symlink_target, target_bytes);
    symlink(
        &symlink_target,
        symlink_directory.path().join("proof.db-wal"),
    )
    .unwrap();

    assert!(open_store(&symlink_directory).is_err());
    assert_eq!(fs::read(&symlink_target).unwrap(), target_bytes);
    assert_eq!(
        fs::metadata(symlink_directory.path().join("proof.db"))
            .unwrap()
            .len(),
        0
    );

    let hard_link_directory = TempDir::new().unwrap();
    make_private(hard_link_directory.path());
    create_file(&hard_link_directory.path().join("proof.db"), b"");
    let hard_link_target = hard_link_directory.path().join("journal-target");
    create_file(&hard_link_target, b"sidecar hard-link target");
    fs::hard_link(
        &hard_link_target,
        hard_link_directory.path().join("proof.db-journal"),
    )
    .unwrap();
    let before = fs::read(&hard_link_target).unwrap();

    assert!(open_store(&hard_link_directory).is_err());
    assert_eq!(fs::read(&hard_link_target).unwrap(), before);
    assert_eq!(
        fs::metadata(hard_link_directory.path().join("proof.db"))
            .unwrap()
            .len(),
        0
    );
}

#[test]
fn directory_mode_type_path_identity_and_relative_path_fail_before_mutation() {
    let wrong_mode = TempDir::new().unwrap();
    make_private(wrong_mode.path());
    create_file(&wrong_mode.path().join("proof.db"), b"");
    fs::set_permissions(wrong_mode.path(), fs::Permissions::from_mode(0o750)).unwrap();
    assert!(open_store(&wrong_mode).is_err());
    assert_eq!(
        fs::metadata(wrong_mode.path().join("proof.db"))
            .unwrap()
            .len(),
        0
    );
    make_private(wrong_mode.path());

    let first = TempDir::new().unwrap();
    let second = TempDir::new().unwrap();
    make_private(first.path());
    make_private(second.path());
    create_file(&first.path().join("proof.db"), b"");
    create_file(&second.path().join("proof.db"), b"other directory");
    let mismatch = SqliteStore::open_existing_nofollow_in_trusted_directory(
        File::open(first.path()).unwrap(),
        second.path(),
        "proof.db",
    );
    assert!(mismatch.is_err());
    assert_eq!(
        fs::read(second.path().join("proof.db")).unwrap(),
        b"other directory"
    );

    let nondirectory = SqliteStore::open_existing_nofollow_in_trusted_directory(
        File::open(first.path().join("proof.db")).unwrap(),
        first.path(),
        "proof.db",
    );
    assert!(nondirectory.is_err());
    assert_eq!(
        fs::metadata(first.path().join("proof.db")).unwrap().len(),
        0
    );

    let relative = SqliteStore::open_existing_nofollow_in_trusted_directory(
        File::open(first.path()).unwrap(),
        Path::new("relative/storage"),
        "proof.db",
    );
    assert!(relative.is_err());
    assert_eq!(
        fs::metadata(first.path().join("proof.db")).unwrap().len(),
        0
    );

    let directory_target = first.path().join("database-directory");
    fs::create_dir(&directory_target).unwrap();
    let wrong_type = SqliteStore::open_existing_nofollow_in_trusted_directory(
        File::open(first.path()).unwrap(),
        first.path(),
        "database-directory",
    );
    assert!(wrong_type.is_err());
}

#[test]
fn symbolic_link_directory_path_is_rejected() {
    let parent = TempDir::new().unwrap();
    let real = parent.path().join("real");
    let linked = parent.path().join("linked");
    fs::create_dir(&real).unwrap();
    make_private(&real);
    create_file(&real.join("proof.db"), b"");
    symlink(&real, &linked).unwrap();

    let result = SqliteStore::open_existing_nofollow_in_trusted_directory(
        File::open(&real).unwrap(),
        &linked,
        "proof.db",
    );

    assert!(result.is_err());
    assert_eq!(fs::metadata(real.join("proof.db")).unwrap().len(), 0);
}
