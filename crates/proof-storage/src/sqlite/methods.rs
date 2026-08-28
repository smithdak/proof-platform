//! Public query and persistence methods on `SqliteStore`.

use super::store::{ProofFilter, SqliteStore};
use crate::StorageError;
use chrono::{DateTime, Utc};
use ed25519_dalek::VerifyingKey;
use proof_kernel::{AuditFilter, ExecutionContext, Proof, RegistryEntry};
use rusqlite::params;
use uuid::Uuid;

impl SqliteStore {
    /// Returns the count of objects in the store.
    pub fn object_count(&self) -> Result<u64, StorageError> {
        let count: u64 =
            self.conn
                .lock()
                .unwrap()
                .query_row("SELECT COUNT(*) FROM objects", [], |row| row.get(0))?;
        Ok(count)
    }

    /// Returns the count of schemas in the store.
    pub fn schema_count(&self) -> Result<u64, StorageError> {
        let count: u64 =
            self.conn
                .lock()
                .unwrap()
                .query_row("SELECT COUNT(*) FROM schemas", [], |row| row.get(0))?;
        Ok(count)
    }

    /// Returns the count of proofs in the store.
    pub fn proof_count(&self) -> Result<u64, StorageError> {
        let count: u64 =
            self.conn
                .lock()
                .unwrap()
                .query_row("SELECT COUNT(*) FROM proofs", [], |row| row.get(0))?;
        Ok(count)
    }

    /// Returns the count of proofs matching the supplied filter.
    pub fn count_proofs(&self, filter: &ProofFilter) -> Result<u64, StorageError> {
        let sql = "
            SELECT COUNT(*)
            FROM proofs
            WHERE (?1 IS NULL OR operation LIKE ?1 || '::%')
              AND (?2 IS NULL OR version = ?2)
              AND (?3 IS NULL OR actor = ?3)
        ";
        let count: u64 = self.conn.lock().unwrap().query_row(
            sql,
            rusqlite::params![filter.operation, filter.version, filter.actor],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    #[cfg(test)]
    pub(crate) fn insert_raw_proof_row(
        &self,
        id: &Uuid,
        actor: &str,
        version: &str,
        operation: &str,
        timestamp: &str,
    ) -> Result<(), StorageError> {
        self.conn.lock().unwrap().execute(
            "INSERT INTO proofs (
                id, actor, version, delegation_id, operation, input_digest, output_digest,
                timestamp, signature
            ) VALUES (?1, ?2, ?3, NULL, ?4, '', '', ?5, '')",
            rusqlite::params![id.to_string(), actor, version, operation, timestamp],
        )?;
        Ok(())
    }

    /// Returns the count of audit contexts in the store.
    pub fn context_count(&self) -> Result<u64, StorageError> {
        let count: u64 = self.conn.lock().unwrap().query_row(
            "SELECT COUNT(*) FROM execution_contexts",
            [],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    /// Persists a principal so signed proofs can later be verified.
    pub fn save_principal(&self, principal: &proof_kernel::Principal) -> Result<(), StorageError> {
        self.conn.lock().unwrap().execute(
            "
            INSERT INTO principals (id, kind, display_name, public_key)
            VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT(id) DO UPDATE SET
                kind = excluded.kind,
                display_name = excluded.display_name,
                public_key = excluded.public_key
            ",
            rusqlite::params![
                principal.id.as_uuid().to_string(),
                serde_json::to_string(&principal.kind)?,
                serde_json::to_string(&principal.kind)?,
                principal.public_key.as_bytes().to_vec(),
            ],
        )?;
        Ok(())
    }

    /// Loads a principal by ID.
    pub fn load_principal(
        &self,
        principal_id: &proof_kernel::PrincipalId,
    ) -> Result<proof_kernel::Principal, StorageError> {
        let (id, kind, public_key) = self
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT id, kind, public_key FROM principals WHERE id = ?1",
                [principal_id.as_uuid().to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Vec<u8>>(2)?,
                    ))
                },
            )
            .map_err(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => {
                    StorageError::NotFound(principal_id.as_uuid().to_string())
                }
                error => error.into(),
            })?;
        let kind: proof_kernel::PrincipalKind = serde_json::from_str(&kind)?;
        let public_key_bytes: [u8; 32] = public_key
            .as_slice()
            .try_into()
            .map_err(|_| StorageError::Conflict("invalid principal public key".to_string()))?;
        let public_key = VerifyingKey::from_bytes(&public_key_bytes)
            .map_err(|_| StorageError::Conflict("invalid principal public key".to_string()))?;
        Ok(proof_kernel::Principal {
            id: proof_kernel::PrincipalId::new(Uuid::parse_str(&id).map_err(|error| {
                StorageError::Conflict(format!("invalid principal ID: {error}"))
            })?),
            kind,
            public_key,
            created_at: Utc::now(),
        })
    }

    /// Persists a serialized proof, replacing any prior proof with the same ID.
    pub fn save_proof(&self, proof: &Proof) -> Result<(), StorageError> {
        let serialized = serde_json::to_string(proof)?;
        let version = proof.body.operation.rsplit("::").next().map(str::to_string);
        let operation = proof.body.operation.clone();
        let actor = proof.body.actor.as_uuid().to_string();
        let id = proof.body.id.to_string();
        let delegation_id = proof
            .body
            .delegation_id
            .map(|delegation_id| delegation_id.to_string());
        let input_digest = proof.body.input_digest.hex();
        let output_digest = proof.body.output_digest.hex();
        let timestamp = proof.body.timestamp.to_rfc3339();
        let expires_at = proof
            .body
            .expires_at
            .map(|expires_at| expires_at.to_rfc3339());
        self.conn.lock().unwrap().execute(
            "
                INSERT INTO proofs (
                    id, actor, version, delegation_id, operation, input_digest, output_digest,
                    timestamp, expires_at, signature
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                ON CONFLICT(id) DO UPDATE SET
                    actor = excluded.actor,
                    version = excluded.version,
                    delegation_id = excluded.delegation_id,
                operation = excluded.operation,
                input_digest = excluded.input_digest,
                    output_digest = excluded.output_digest,
                    timestamp = excluded.timestamp,
                    expires_at = excluded.expires_at,
                    signature = excluded.signature
            ",
            rusqlite::params![
                id,
                actor,
                version,
                delegation_id,
                operation,
                input_digest,
                output_digest,
                timestamp,
                expires_at,
                serialized,
            ],
        )?;
        Ok(())
    }

    /// Loads a proof by ID, excluding expired proofs by default.
    pub fn load_proof(&self, proof_id: &Uuid) -> Result<Proof, StorageError> {
        self.load_proof_with_options(proof_id, false)
    }

    /// Loads a proof by ID, optionally including expired proofs.
    pub fn load_proof_with_options(
        &self,
        proof_id: &Uuid,
        include_expired: bool,
    ) -> Result<Proof, StorageError> {
        let serialized: String = self
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT signature FROM proofs
                 WHERE id = ?1
                   AND (?2 OR expires_at IS NULL OR expires_at > ?3)",
                rusqlite::params![
                    proof_id.to_string(),
                    include_expired,
                    Utc::now().to_rfc3339()
                ],
                |row| row.get(0),
            )
            .map_err(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => {
                    StorageError::NotFound(proof_id.to_string())
                }
                error => error.into(),
            })?;
        Ok(serde_json::from_str(&serialized)?)
    }

    /// Loads all non-expired proofs for an operation in ascending timestamp order.
    pub fn list_proofs_for_operation(
        &self,
        operation: &str,
        version: Option<&str>,
    ) -> Result<Vec<Proof>, StorageError> {
        self.list_proofs_for_operation_with_options(operation, version, false)
    }

    /// Loads all proofs for an operation, optionally including expired proofs.
    pub fn list_proofs_for_operation_with_options(
        &self,
        operation: &str,
        version: Option<&str>,
        include_expired: bool,
    ) -> Result<Vec<Proof>, StorageError> {
        let connection = self.conn.lock().unwrap();
        let serialized_proofs;
        if let Some(version) = version {
            let mut statement = connection.prepare_cached(
                "SELECT signature FROM proofs
                 WHERE operation = ?1 AND version = ?2
                   AND (?3 OR expires_at IS NULL OR expires_at > ?4)
                 ORDER BY timestamp, id",
            )?;
            serialized_proofs = statement
                .query_map(
                    rusqlite::params![
                        format!("{operation}::{version}"),
                        version,
                        include_expired,
                        Utc::now().to_rfc3339()
                    ],
                    |row| row.get::<_, String>(0),
                )?
                .collect::<Result<Vec<_>, _>>()?;
        } else {
            let mut statement = connection.prepare_cached(
                "SELECT signature FROM proofs
                 WHERE operation LIKE ?1 || '::%'
                   AND (?2 OR expires_at IS NULL OR expires_at > ?3)
                 ORDER BY timestamp, id",
            )?;
            serialized_proofs = statement
                .query_map(
                    rusqlite::params![operation, include_expired, Utc::now().to_rfc3339()],
                    |row| row.get::<_, String>(0),
                )?
                .collect::<Result<Vec<_>, _>>()?;
        }
        Ok(serialized_proofs
            .iter()
            .map(|serialized| serde_json::from_str(serialized))
            .collect::<Result<Vec<_>, _>>()?)
    }

    /// Loads all non-expired proofs signed by an actor in ascending timestamp order.
    pub fn list_proofs_for_actor(
        &self,
        actor_id: &proof_kernel::PrincipalId,
    ) -> Result<Vec<Proof>, StorageError> {
        self.list_proofs_for_actor_with_options(actor_id, false)
    }

    /// Loads all proofs signed by an actor, optionally including expired proofs.
    pub fn list_proofs_for_actor_with_options(
        &self,
        actor_id: &proof_kernel::PrincipalId,
        include_expired: bool,
    ) -> Result<Vec<Proof>, StorageError> {
        let connection = self.conn.lock().unwrap();
        let mut statement = connection.prepare_cached(
            "SELECT signature FROM proofs
             WHERE actor = ?1
               AND (?2 OR expires_at IS NULL OR expires_at > ?3)
             ORDER BY timestamp, id",
        )?;
        let serialized_proofs = statement
            .query_map(
                rusqlite::params![
                    actor_id.as_uuid().to_string(),
                    include_expired,
                    Utc::now().to_rfc3339()
                ],
                |row| row.get::<_, String>(0),
            )?
            .collect::<Result<Vec<_>, _>>()?;
        serialized_proofs
            .iter()
            .map(|serialized| Ok(serde_json::from_str(serialized)?))
            .collect()
    }

    /// Verifies signatures and digest continuity for the supplied proof chain.
    pub fn verify_proof_chain(&self, proof_ids: &[Uuid]) -> Result<(), StorageError> {
        let proofs = proof_ids
            .iter()
            .map(|proof_id| self.load_proof(proof_id))
            .collect::<Result<Vec<_>, _>>()?;
        for proof in &proofs {
            let principal =
                self.load_principal(&proof.body.actor)
                    .map_err(|error| match error {
                        StorageError::NotFound(_) => StorageError::Conflict(format!(
                            "missing principal for proof {}: {}",
                            proof.body.id, proof.body.actor
                        )),
                        error => error,
                    })?;
            proof.verify(&principal.public_key).map_err(|_| {
                StorageError::Conflict(format!("invalid signature for proof {}", proof.body.id))
            })?;
        }
        for pair in proofs.windows(2) {
            if pair[0].body.output_digest != pair[1].body.input_digest {
                return Err(StorageError::Conflict(format!(
                    "proof chain discontinuity between {} and {}",
                    pair[0].body.id, pair[1].body.id
                )));
            }
        }
        Ok(())
    }

    /// Deletes audit contexts strictly older than the supplied timestamp.
    pub fn delete_expired_contexts(&self, before: DateTime<Utc>) -> Result<u64, StorageError> {
        let deleted = self.conn.lock().unwrap().execute(
            "DELETE FROM execution_contexts WHERE timestamp < ?1",
            [before.to_rfc3339()],
        )?;
        Ok(deleted as u64)
    }

    /// Deletes proofs expired at or before the supplied timestamp.
    pub fn purge_expired_proofs(&self, now: DateTime<Utc>) -> Result<u64, StorageError> {
        let deleted = self.conn.lock().unwrap().execute(
            "DELETE FROM proofs WHERE expires_at IS NOT NULL AND expires_at <= ?1",
            [now.to_rfc3339()],
        )?;
        Ok(deleted as u64)
    }

    /// Persists registry entries, replacing the stored collection.
    pub fn save_registry(&self, entries: &[RegistryEntry]) -> Result<(), StorageError> {
        let connection = self.conn.lock().unwrap();
        let transaction = connection.unchecked_transaction()?;
        transaction.execute("DELETE FROM registry_entries", [])?;
        for entry in entries {
            transaction.execute(
                "
                INSERT INTO registry_entries (operation, version, data)
                VALUES (?1, ?2, ?3)
                ",
                rusqlite::params![
                    entry.operation,
                    entry.version,
                    serde_json::to_string(entry)?,
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Loads all persisted registry entries in operation/version order.
    pub fn load_registry(&self) -> Result<Vec<RegistryEntry>, StorageError> {
        let connection = self.conn.lock().unwrap();
        let mut statement =
            connection.prepare("SELECT data FROM registry_entries ORDER BY operation, version")?;
        let serialized_entries = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let entries = serialized_entries
            .iter()
            .map(|serialized| serde_json::from_str(serialized))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(entries)
    }

    /// Persists an execution context for the audit trail.
    pub fn save_execution_context(&self, context: &ExecutionContext) -> Result<Uuid, StorageError> {
        let context_id = Uuid::now_v7();
        self.conn.lock().unwrap().execute(
            "
            INSERT INTO execution_contexts (
                id, actor, delegation_id, workspace_path, timestamp
            ) VALUES (?1, ?2, ?3, ?4, ?5)
            ",
            rusqlite::params![
                context_id.to_string(),
                context.actor.as_uuid().to_string(),
                context
                    .delegation_id
                    .map(|delegation_id| delegation_id.to_string()),
                context.workspace_path.display().to_string(),
                context.timestamp.to_rfc3339(),
            ],
        )?;
        Ok(context_id)
    }

    /// Persists one benchmark iteration result.
    pub fn save_benchmark_result(
        &self,
        result: &proof_kernel::BenchmarkResult,
    ) -> Result<(), StorageError> {
        self.conn.lock().unwrap().execute(
            "
            INSERT INTO benchmark_results (
                benchmark, operation, version, passed, duration_ms, failure, recorded_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ",
            rusqlite::params![
                result.benchmark,
                result.operation,
                result.version,
                result.passed,
                i64::try_from(result.duration_ms).map_err(|_| StorageError::Conflict(
                    "benchmark duration exceeds SQLite's integer range".to_string()
                ))?,
                result.failure.as_deref(),
                result.timestamp.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    /// Loads benchmark results for an operation/version in recorded order.
    pub fn list_benchmark_results(
        &self,
        operation: &str,
        version: &str,
    ) -> Result<Vec<proof_kernel::BenchmarkResult>, StorageError> {
        let connection = self.conn.lock().unwrap();
        let mut statement = connection.prepare_cached(
            "
            SELECT benchmark, operation, version, passed, duration_ms, failure, recorded_at
            FROM benchmark_results
            WHERE operation = ?1 AND version = ?2
            ORDER BY id
            ",
        )?;
        let results = statement
            .query_map(rusqlite::params![operation, version], |row| {
                let passed: bool = row.get(3)?;
                let duration_ms: i64 = row.get(4)?;
                let timestamp: DateTime<Utc> =
                    DateTime::parse_from_rfc3339(&row.get::<_, String>(6)?)
                        .map_err(|error| {
                            rusqlite::Error::FromSqlConversionFailure(
                                6,
                                rusqlite::types::Type::Text,
                                error.into(),
                            )
                        })?
                        .with_timezone(&Utc);
                Ok(proof_kernel::BenchmarkResult {
                    benchmark: row.get(0)?,
                    operation: row.get(1)?,
                    version: row.get(2)?,
                    passed,
                    duration_ms: u64::try_from(duration_ms).map_err(|_| {
                        rusqlite::Error::FromSqlConversionFailure(
                            4,
                            rusqlite::types::Type::Integer,
                            "benchmark duration is negative".into(),
                        )
                    })?,
                    timestamp,
                    failure: row.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(results)
    }
}
