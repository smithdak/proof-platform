//! Content-addressed blob storage backed by SQLite metadata and filesystem objects.

use crate::StorageError;
use proof_kernel::ContentDigest;
use rusqlite::{params, Connection, OptionalExtension};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tempfile::NamedTempFile;

pub struct ContentAddressedStore {
    connection: Mutex<Connection>,
    blob_directory: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlobReference<'a> {
    pub artifact_kind: &'a str,
    pub artifact_id: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GarbageCollectionResult {
    pub removed_blobs: u64,
    pub reclaimed_bytes: u64,
}

impl ContentAddressedStore {
    /// Returns the CAS connection for introspection and maintenance.
    pub fn connection(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.connection.lock().unwrap()
    }

    pub fn open(database_path: &Path, blob_directory: &Path) -> Result<Self, StorageError> {
        if let Some(parent) = database_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::create_dir_all(blob_directory)?;
        let canonical_blob_directory = fs::canonicalize(blob_directory)?;
        let connection = Connection::open(database_path)?;
        connection.execute_batch(
            "
            PRAGMA foreign_keys = ON;

            CREATE TABLE IF NOT EXISTS content_blobs (
                digest TEXT PRIMARY KEY,
                size_bytes INTEGER NOT NULL,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
            );

            CREATE TABLE IF NOT EXISTS content_blob_references (
                digest TEXT NOT NULL REFERENCES content_blobs(digest) ON DELETE CASCADE,
                artifact_kind TEXT NOT NULL,
                artifact_id TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                PRIMARY KEY (digest, artifact_kind, artifact_id)
            );

            CREATE INDEX IF NOT EXISTS idx_content_blob_references_artifact
                ON content_blob_references(artifact_kind, artifact_id);
            ",
        )?;
        Ok(Self {
            connection: Mutex::new(connection),
            blob_directory: canonical_blob_directory,
        })
    }

    pub fn digest(content: &[u8]) -> ContentDigest {
        ContentDigest::from_bytes(*blake3::hash(content).as_bytes())
    }

    pub fn put(&self, content: &[u8]) -> Result<ContentDigest, StorageError> {
        let digest = Self::digest(content);
        if self.exists(&digest)? {
            return Ok(digest);
        }

        let destination = self.blob_path(&digest.hex());
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut temporary_file = NamedTempFile::new_in(&self.blob_directory)?;
        temporary_file.write_all(content)?;
        temporary_file.flush()?;
        temporary_file.persist(&destination).map_err(|error| {
            if destination.exists() {
                io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "blob already exists after concurrent put",
                )
            } else {
                error.error
            }
        })?;
        self.connection
            .lock()
            .unwrap()
            .execute(
                "INSERT OR IGNORE INTO content_blobs(digest, size_bytes) VALUES (?1, ?2)",
                params![digest.hex(), content.len() as u64],
            )
            .map_err(StorageError::from)?;
        Ok(digest)
    }

    pub fn get(&self, digest: &ContentDigest) -> Result<Option<Vec<u8>>, StorageError> {
        if !self.exists(digest)? {
            return Ok(None);
        }
        match fs::read(self.blob_path(&digest.hex())) {
            Ok(content) if Self::digest(&content) == *digest => Ok(Some(content)),
            Ok(_) => Err(StorageError::Conflict(format!(
                "blob integrity mismatch: {digest}"
            ))),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub fn exists(&self, digest: &ContentDigest) -> Result<bool, StorageError> {
        let digest_hex = digest.hex();
        let recorded = self
            .connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT 1 FROM content_blobs WHERE digest = ?1",
                params![digest_hex],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        Ok(recorded && self.blob_path(&digest_hex).is_file())
    }

    pub fn add_reference(
        &self,
        digest: &ContentDigest,
        reference: BlobReference<'_>,
    ) -> Result<(), StorageError> {
        if !self.exists(digest)? {
            return Err(StorageError::NotFound(digest.to_string()));
        }
        self.connection
            .lock()
            .unwrap()
            .execute(
                "INSERT OR IGNORE INTO content_blob_references(digest, artifact_kind, artifact_id)
                 VALUES (?1, ?2, ?3)",
                params![digest.hex(), reference.artifact_kind, reference.artifact_id],
            )
            .map_err(StorageError::from)?;
        Ok(())
    }

    pub fn remove_reference(
        &self,
        digest: &ContentDigest,
        reference: BlobReference<'_>,
    ) -> Result<bool, StorageError> {
        let deleted = self
            .connection
            .lock()
            .unwrap()
            .execute(
                "DELETE FROM content_blob_references
                 WHERE digest = ?1 AND artifact_kind = ?2 AND artifact_id = ?3",
                params![digest.hex(), reference.artifact_kind, reference.artifact_id],
            )
            .map_err(StorageError::from)?;
        Ok(deleted > 0)
    }

    pub fn references(
        &self,
        digest: &ContentDigest,
    ) -> Result<Vec<(String, String)>, StorageError> {
        self.connection
            .lock()
            .unwrap()
            .prepare_cached(
                "SELECT artifact_kind, artifact_id FROM content_blob_references
                 WHERE digest = ?1 ORDER BY artifact_kind, artifact_id",
            )?
            .query_map(params![digest.hex()], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)
    }

    pub fn size(&self, digest: &ContentDigest) -> Result<Option<u64>, StorageError> {
        self.connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT size_bytes FROM content_blobs WHERE digest = ?1",
                params![digest.hex()],
                |row| row.get(0),
            )
            .optional()
            .map_err(StorageError::from)
    }

    pub fn delete(&self, digest: &ContentDigest) -> Result<bool, StorageError> {
        let reference_count: u64 = self
            .connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM content_blob_references WHERE digest = ?1",
                params![digest.hex()],
                |row| row.get(0),
            )
            .map_err(StorageError::from)?;
        if reference_count > 0 || !self.exists(digest)? {
            return Ok(false);
        }

        let mut connection = self.connection.lock().unwrap();
        let transaction = connection.transaction()?;
        let deleted = transaction.execute(
            "DELETE FROM content_blobs WHERE digest = ?1",
            params![digest.hex()],
        )?;
        transaction.commit()?;
        if deleted == 0 {
            return Ok(false);
        }
        match fs::remove_file(self.blob_path(&digest.hex())) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(true),
            Err(error) => Err(error.into()),
        }
    }

    pub fn collect_garbage(&self) -> Result<GarbageCollectionResult, StorageError> {
        let mut connection = self.connection.lock().unwrap();
        let transaction = connection.transaction()?;
        let unreferenced: Vec<(String, u64)> = transaction
            .prepare(
                "SELECT digest, size_bytes FROM content_blobs
                 WHERE NOT EXISTS (
                     SELECT 1 FROM content_blob_references
                     WHERE content_blob_references.digest = content_blobs.digest
                 )",
            )?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;

        let mut removed_blobs = 0_u64;
        let mut reclaimed_bytes = 0_u64;
        for (digest_hex, size_bytes) in unreferenced {
            let path = Self::blob_path_in(&self.blob_directory, &digest_hex);
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
            transaction.execute(
                "DELETE FROM content_blobs WHERE digest = ?1",
                params![digest_hex],
            )?;
            removed_blobs += 1;
            reclaimed_bytes += size_bytes;
        }
        transaction.commit()?;

        let mut removed_orphans = 0_u64;
        for path in self.orphaned_blob_paths(&connection)? {
            match fs::remove_file(&path) {
                Ok(()) => removed_orphans += 1,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }

        Ok(GarbageCollectionResult {
            removed_blobs: removed_blobs + removed_orphans,
            reclaimed_bytes,
        })
    }

    fn orphaned_blob_paths(&self, connection: &Connection) -> Result<Vec<PathBuf>, StorageError> {
        let mut paths = Vec::new();
        let shard_directories = match fs::read_dir(&self.blob_directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(paths),
            Err(error) => return Err(error.into()),
        };
        for shard_directory in shard_directories {
            let shard_directory = shard_directory?.path();
            if !shard_directory.is_dir() {
                continue;
            }
            for entry in fs::read_dir(&shard_directory)? {
                let path = entry?.path();
                let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
                    continue;
                };
                let known = connection
                    .query_row(
                        "SELECT 1 FROM content_blobs WHERE digest = ?1",
                        params![file_name],
                        |_| Ok(()),
                    )
                    .optional()?
                    .is_some();
                if !known && path.is_file() {
                    paths.push(path);
                }
            }
        }
        Ok(paths)
    }

    fn blob_path(&self, digest_hex: &str) -> PathBuf {
        Self::blob_path_in(&self.blob_directory, digest_hex)
    }

    fn blob_path_in(blob_directory: &Path, digest_hex: &str) -> PathBuf {
        blob_directory.join(&digest_hex[0..2]).join(digest_hex)
    }
}
