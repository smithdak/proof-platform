use proof_kernel::ContentDigest;
use proof_storage::{BlobReference, ContentAddressedStore, StorageError};
use std::fs;
use tempfile::TempDir;

fn store_in(tempdir: &TempDir) -> ContentAddressedStore {
    ContentAddressedStore::open(
        &tempdir.path().join("metadata/cas.db"),
        &tempdir.path().join("blobs"),
    )
    .unwrap()
}

#[test]
fn computes_direct_blake3_digest() {
    assert_eq!(
        ContentAddressedStore::digest(b"content").hex(),
        blake3::hash(b"content").to_hex().to_string()
    );
}

#[test]
fn puts_gets_and_deduplicates_blobs() {
    let tempdir = TempDir::new().unwrap();
    let store = store_in(&tempdir);
    let content = b"shared binary payload".repeat(3);

    let first = store.put(&content).unwrap();
    let second = store.put(&content).unwrap();
    assert_eq!(first, second);
    assert_eq!(
        store.get(&first).unwrap().as_deref(),
        Some(content.as_slice())
    );
    assert!(store.exists(&first).unwrap());
    assert_eq!(store.size(&first).unwrap(), Some(content.len() as u64));
}

#[test]
fn verifies_blob_integrity() {
    let tempdir = TempDir::new().unwrap();
    let store = store_in(&tempdir);
    let digest = store.put(b"integrity checked").unwrap();
    let blob_path = tempdir
        .path()
        .join("blobs")
        .join(digest.hex()[0..2].to_string())
        .join(digest.hex());
    fs::write(&blob_path, b"corrupted").unwrap();

    assert!(matches!(
        store.get(&digest),
        Err(StorageError::Conflict(message)) if message.contains("integrity mismatch")
    ));
}

#[test]
fn references_retain_shared_digest() {
    let tempdir = TempDir::new().unwrap();
    let store = store_in(&tempdir);
    let content = b"referenced by multiple proofs";
    let digest = store.put(content).unwrap();
    let first = BlobReference {
        artifact_kind: "proof",
        artifact_id: "proof-1",
    };
    let second = BlobReference {
        artifact_kind: "proof",
        artifact_id: "proof-2",
    };
    let other_kind = BlobReference {
        artifact_kind: "edition",
        artifact_id: "proof-1",
    };
    for reference in [first, second, other_kind] {
        store.add_reference(&digest, reference).unwrap();
    }

    assert_eq!(store.references(&digest).unwrap().len(), 3);
    assert_eq!(store.delete(&digest).unwrap(), false);
    for reference in [first, second, other_kind] {
        assert!(store.remove_reference(&digest, reference).unwrap());
    }
    assert_eq!(store.delete(&digest).unwrap(), true);
    assert_eq!(store.get(&digest).unwrap(), None);
}

#[test]
fn reference_requires_existing_blob() {
    let tempdir = TempDir::new().unwrap();
    let store = store_in(&tempdir);
    let digest = ContentDigest::from_bytes([7; 32]);
    assert!(matches!(
        store.add_reference(
            &digest,
            BlobReference {
                artifact_kind: "proof",
                artifact_id: "missing"
            }
        ),
        Err(StorageError::NotFound(_))
    ));
}

#[test]
fn garbage_collection_removes_only_unreferenced_blobs() {
    let tempdir = TempDir::new().unwrap();
    let store = store_in(&tempdir);
    let referenced = store.put(b"retained").unwrap();
    let unreferenced = store.put(b"collected").unwrap();
    store
        .add_reference(
            &referenced,
            BlobReference {
                artifact_kind: "proof",
                artifact_id: "proof-retained",
            },
        )
        .unwrap();

    let result = store.collect_garbage().unwrap();
    assert_eq!(result.removed_blobs, 1);
    assert_eq!(result.reclaimed_bytes, 9);
    assert!(store.exists(&referenced).unwrap());
    assert_eq!(store.exists(&unreferenced).unwrap(), false);
}

#[test]
fn garbage_collection_removes_orphaned_filesystem_blobs() {
    let tempdir = TempDir::new().unwrap();
    let store = store_in(&tempdir);
    let digest = store.put(b"orphaned object").unwrap();
    let blob_path = tempdir
        .path()
        .join("blobs")
        .join(digest.hex()[0..2].to_string())
        .join(digest.hex());
    store
        .connection()
        .execute(
            "DELETE FROM content_blobs WHERE digest = ?1",
            [digest.hex()],
        )
        .unwrap();

    let result = store.collect_garbage().unwrap();
    assert_eq!(result.removed_blobs, 1);
    assert!(!blob_path.exists());
}

#[test]
fn reopening_a_store_preserves_blobs_and_references() {
    let tempdir = TempDir::new().unwrap();
    let content = b"durable across reopen";
    let store = store_in(&tempdir);
    let digest = store.put(content).unwrap();
    let reference = BlobReference {
        artifact_kind: "proof",
        artifact_id: "proof-reopen",
    };
    store.add_reference(&digest, reference).unwrap();
    drop(store);

    let reopened = store_in(&tempdir);
    assert_eq!(
        reopened.get(&digest).unwrap().as_deref(),
        Some(content.as_slice())
    );
    assert_eq!(
        reopened.references(&digest).unwrap(),
        [("proof".to_string(), "proof-reopen".to_string())]
    );
}
