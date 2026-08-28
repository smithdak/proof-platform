use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use uuid::Uuid;

use crate::error::ContentError;
use crate::schema::SchemaDefinition;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectStatus {
    Draft,
    Submitted,
    Approved,
    Committed,
    Published,
}

impl ObjectStatus {
    pub fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Draft, Self::Submitted)
                | (Self::Submitted, Self::Approved)
                | (Self::Approved, Self::Committed)
                | (Self::Committed, Self::Published)
                | (Self::Published, Self::Draft)
        )
    }
}

#[derive(Debug, Clone, Error)]
#[error("invalid object transition from {from:?} to {to:?}")]
pub struct ObjectTransitionError {
    from: ObjectStatus,
    to: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Object {
    pub id: Uuid,
    pub schema_id: Uuid,
    pub schema_version: u32,
    pub locale: String,
    pub content: Value,
    pub revision: u32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    status: ObjectStatus,
}

impl Object {
    pub fn create(
        schema: &SchemaDefinition,
        locale: impl Into<String>,
        content: Value,
    ) -> Result<Self, ContentError> {
        schema.validate_object(&content)?;
        let now = Utc::now();
        Ok(Self {
            id: Uuid::now_v7(),
            schema_id: schema.id,
            schema_version: schema.version,
            locale: locale.into(),
            content,
            revision: 1,
            created_at: now,
            updated_at: now,
            status: ObjectStatus::Draft,
        })
    }

    pub fn status(&self) -> ObjectStatus {
        self.status
    }

    pub fn transition_to(&mut self, next: ObjectStatus) -> Result<(), ObjectTransitionError> {
        if !self.status.can_transition_to(next) {
            return Err(ObjectTransitionError {
                from: self.status,
                to: format!("{next:?}"),
            });
        }
        self.status = next;
        self.updated_at = Utc::now();
        if next == ObjectStatus::Draft {
            self.revision += 1;
        }
        Ok(())
    }

    pub fn update_content(
        &mut self,
        schema: &SchemaDefinition,
        content: Value,
    ) -> Result<(), ContentError> {
        if self.status() != ObjectStatus::Draft {
            return Err(ContentError::InvalidEdit {
                edit: format!("{}", self.id),
                reason: "only Draft objects can be updated".to_string(),
            });
        }
        if schema.id != self.schema_id || schema.version != self.schema_version {
            return Err(ContentError::SchemaMismatch {
                edit: format!("{}", self.id),
                schema_id: schema.id,
                schema_version: schema.version,
            });
        }
        schema.validate_object(&content)?;
        self.content = content;
        self.revision += 1;
        self.updated_at = Utc::now();
        Ok(())
    }
}
