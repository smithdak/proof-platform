//! Lossless, fail-closed persistence for delegation grants.

use super::store::SqliteStore;
use crate::StorageError;
use chrono::{DateTime, Utc};
use proof_kernel::delegation::DelegationScope;
use proof_kernel::{Delegation, PrincipalId};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredDelegationScopeV1 {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    allowed_operations: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    allowed_domains: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    resource_scope: Option<String>,
}

impl From<&DelegationScope> for StoredDelegationScopeV1 {
    fn from(scope: &DelegationScope) -> Self {
        Self {
            allowed_operations: scope.allowed_operations.clone(),
            allowed_domains: scope.allowed_domains.clone(),
            resource_scope: scope.resource_scope.clone(),
        }
    }
}

impl From<StoredDelegationScopeV1> for DelegationScope {
    fn from(scope: StoredDelegationScopeV1) -> Self {
        Self {
            allowed_operations: scope.allowed_operations,
            allowed_domains: scope.allowed_domains,
            resource_scope: scope.resource_scope,
        }
    }
}

struct StoredDelegationRow {
    id: String,
    issuer: String,
    recipient: String,
    allowed_actions: String,
    resource_scope: String,
    scope_json: String,
    valid_from: String,
    valid_until: String,
    revoked: i64,
}

impl StoredDelegationRow {
    fn decode(self, requested_id: Uuid) -> Result<Delegation, StorageError> {
        let id = parse_uuid("id", &self.id)?;
        if id != requested_id {
            return Err(invalid_field(
                "id",
                format!("row ID {id} does not match requested ID {requested_id}"),
            ));
        }
        let issuer = PrincipalId::new(parse_uuid("issuer", &self.issuer)?);
        let recipient = PrincipalId::new(parse_uuid("recipient", &self.recipient)?);
        let allowed_actions = parse_string_list("allowed_actions", &self.allowed_actions)?;
        let resource_scope = parse_string_list("resource_scope", &self.resource_scope)?;
        let scope = serde_json::from_str::<StoredDelegationScopeV1>(&self.scope_json)
            .map(DelegationScope::from)
            .map_err(|error| invalid_field("scope_json", error))?;
        let valid_from = parse_timestamp("valid_from", &self.valid_from)?;
        let valid_until = parse_timestamp("valid_until", &self.valid_until)?;
        if valid_from > valid_until {
            return Err(invalid_field(
                "validity window",
                "valid_from is later than valid_until",
            ));
        }
        let revoked = match self.revoked {
            0 => false,
            1 => true,
            value => {
                return Err(invalid_field(
                    "revoked",
                    format!("expected 0 or 1, got {value}"),
                ));
            }
        };

        Ok(Delegation {
            id,
            issuer,
            recipient,
            allowed_actions,
            resource_scope,
            scope,
            valid_from,
            valid_until,
            revoked,
        })
    }
}

fn parse_uuid(field: &str, value: &str) -> Result<Uuid, StorageError> {
    Uuid::parse_str(value).map_err(|error| invalid_field(field, error))
}

fn parse_string_list(field: &str, value: &str) -> Result<Vec<String>, StorageError> {
    serde_json::from_str(value).map_err(|error| invalid_field(field, error))
}

fn parse_timestamp(field: &str, value: &str) -> Result<DateTime<Utc>, StorageError> {
    DateTime::parse_from_rfc3339(value)
        .map(|timestamp| timestamp.with_timezone(&Utc))
        .map_err(|error| invalid_field(field, error))
}

fn invalid_field(field: &str, error: impl std::fmt::Display) -> StorageError {
    StorageError::Conflict(format!("invalid stored delegation {field}: {error}"))
}

impl SqliteStore {
    /// Persists a complete delegation grant, including its structured scope.
    pub fn save_delegation(&self, delegation: &Delegation) -> Result<(), StorageError> {
        if delegation.valid_from > delegation.valid_until {
            return Err(StorageError::Conflict(
                "delegation valid_from is later than valid_until".to_string(),
            ));
        }
        let scope_json = serde_json::to_string(&StoredDelegationScopeV1::from(&delegation.scope))?;
        self.conn.lock().unwrap().execute(
            "
            INSERT INTO delegations (
                id, issuer, recipient, allowed_actions, resource_scope, scope_json,
                valid_from, valid_until, revoked
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            ON CONFLICT(id) DO UPDATE SET
                issuer = excluded.issuer,
                recipient = excluded.recipient,
                allowed_actions = excluded.allowed_actions,
                resource_scope = excluded.resource_scope,
                scope_json = excluded.scope_json,
                valid_from = excluded.valid_from,
                valid_until = excluded.valid_until,
                revoked = excluded.revoked
            ",
            rusqlite::params![
                delegation.id.to_string(),
                delegation.issuer.to_string(),
                delegation.recipient.to_string(),
                serde_json::to_string(&delegation.allowed_actions)?,
                serde_json::to_string(&delegation.resource_scope)?,
                scope_json,
                delegation.valid_from.to_rfc3339(),
                delegation.valid_until.to_rfc3339(),
                delegation.revoked,
            ],
        )?;
        Ok(())
    }

    /// Loads a complete delegation grant by ID, failing on corrupt persisted data.
    pub fn load_delegation(
        &self,
        delegation_id: &Uuid,
    ) -> Result<Option<Delegation>, StorageError> {
        let row = self
            .conn
            .lock()
            .unwrap()
            .query_row(
                "
                SELECT id, issuer, recipient, allowed_actions, resource_scope, scope_json,
                       valid_from, valid_until, revoked
                FROM delegations
                WHERE id = ?1
                ",
                [delegation_id.to_string()],
                |row| {
                    Ok(StoredDelegationRow {
                        id: row.get(0)?,
                        issuer: row.get(1)?,
                        recipient: row.get(2)?,
                        allowed_actions: row.get(3)?,
                        resource_scope: row.get(4)?,
                        scope_json: row.get(5)?,
                        valid_from: row.get(6)?,
                        valid_until: row.get(7)?,
                        revoked: row.get(8)?,
                    })
                },
            )
            .optional()?;

        row.map(|row| row.decode(*delegation_id)).transpose()
    }
}
