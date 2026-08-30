use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use uuid::Uuid;

use crate::digest::canonical_digest;
use crate::error::ContentError;
use crate::object::{Object, ObjectStatus};
use crate::schema::SchemaDefinition;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeSetStatus {
    Draft,
    Submitted,
    Approved,
    Committed,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChangeSetEdit {
    ObjectCreate(ObjectCreateEdit),
    ObjectUpdate(ObjectUpdateEdit),
    ObjectDelete(ObjectDeleteEdit),
}

impl ChangeSetEdit {
    pub fn object_id(&self) -> Uuid {
        match self {
            Self::ObjectCreate(edit) => edit.object.id,
            Self::ObjectUpdate(edit) => edit.object_id,
            Self::ObjectDelete(edit) => edit.object_id,
        }
    }

    fn describe(&self) -> String {
        match self {
            Self::ObjectCreate(_) => "object_create".to_string(),
            Self::ObjectUpdate(_) => "object_update".to_string(),
            Self::ObjectDelete(_) => "object_delete".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObjectCreateEdit {
    pub object: Object,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObjectUpdateEdit {
    pub object_id: Uuid,
    pub expected_revision: u32,
    pub content: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObjectDeleteEdit {
    pub object_id: Uuid,
    pub expected_revision: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChangeSet {
    pub id: Uuid,
    pub intent: String,
    pub base_state_digest: String,
    pub edits: Vec<ChangeSetEdit>,
    pub created_at: DateTime<Utc>,
    pub status: ChangeSetStatus,
}

pub type BaseState = BTreeMap<Uuid, Object>;

impl ChangeSet {
    pub fn new(
        intent: impl Into<String>,
        base_state: &BaseState,
        edits: Vec<ChangeSetEdit>,
    ) -> Self {
        Self {
            id: Uuid::now_v7(),
            intent: intent.into(),
            base_state_digest: state_digest(base_state),
            edits,
            created_at: Utc::now(),
            status: ChangeSetStatus::Draft,
        }
    }

    pub fn transition_to(&mut self, next: ChangeSetStatus) -> Result<(), ContentError> {
        let allowed = matches!(
            (self.status, next),
            (ChangeSetStatus::Draft, ChangeSetStatus::Submitted)
                | (ChangeSetStatus::Submitted, ChangeSetStatus::Approved)
                | (ChangeSetStatus::Submitted, ChangeSetStatus::Rejected)
                | (ChangeSetStatus::Approved, ChangeSetStatus::Committed)
        );
        if !allowed {
            return Err(ContentError::InvalidEdit {
                edit: format!("{}", self.id),
                reason: format!(
                    "invalid changeset transition from {:?} to {next:?}",
                    self.status
                ),
            });
        }
        self.status = next;
        Ok(())
    }

    pub fn validate(
        &self,
        schemas: &[SchemaDefinition],
        base_state: &BaseState,
    ) -> Result<(), ContentError> {
        let actual_digest = state_digest(base_state);
        if actual_digest != self.base_state_digest {
            return Err(ContentError::BaseStateMismatch {
                expected: self.base_state_digest.clone(),
                actual: actual_digest,
            });
        }
        if self.edits.is_empty() {
            return Err(ContentError::EmptyChangeset);
        }

        let mut expected_revisions: BTreeMap<Uuid, u32> = base_state
            .iter()
            .map(|(id, object)| (*id, object.revision))
            .collect();

        let mut staged: BTreeMap<Uuid, Object> = base_state.clone();
        for (index, edit) in self.edits.iter().enumerate() {
            let description = format!("edit[{index}]:{}", edit.describe());
            match edit {
                ChangeSetEdit::ObjectCreate(create) => {
                    if staged.contains_key(&create.object.id) {
                        return Err(ContentError::InvalidEdit {
                            edit: description,
                            reason: "object already exists".to_string(),
                        });
                    }
                    let schema = required_schema(schemas, &create.object, &description)?;
                    schema.validate_object(&create.object.content)?;
                    if create.object.status() != ObjectStatus::Draft {
                        return Err(ContentError::InvalidEdit {
                            edit: description,
                            reason: "created objects must start as Draft".to_string(),
                        });
                    }
                    staged.insert(create.object.id, create.object.clone());
                }
                ChangeSetEdit::ObjectUpdate(update) => {
                    if !expected_revisions.contains_key(&update.object_id) {
                        return Err(ContentError::MissingBaseObject {
                            object_id: update.object_id,
                        });
                    }
                    let expected_revision = expected_revisions
                        .get(&update.object_id)
                        .copied()
                        .unwrap_or_default();
                    if expected_revision != update.expected_revision {
                        return Err(ContentError::EditTargetMismatch {
                            edit: description,
                            base: format!("revision {expected_revision}"),
                        });
                    }
                    let existing = staged.get_mut(&update.object_id).ok_or_else(|| {
                        ContentError::MissingBaseObject {
                            object_id: update.object_id,
                        }
                    })?;
                    let schema = schema_for_object(schemas, existing).ok_or_else(|| {
                        ContentError::SchemaMismatch {
                            edit: description,
                            schema_id: existing.schema_id,
                            schema_version: existing.schema_version,
                        }
                    })?;
                    schema.validate_object(&update.content)?;
                    existing.content = update.content.clone();
                    existing.revision += 1;
                    existing.updated_at = Utc::now();
                    expected_revisions.insert(update.object_id, expected_revision + 1);
                }
                ChangeSetEdit::ObjectDelete(delete) => {
                    let expected_revision = *expected_revisions.get(&delete.object_id).ok_or(
                        ContentError::MissingBaseObject {
                            object_id: delete.object_id,
                        },
                    )?;
                    if expected_revision != delete.expected_revision {
                        return Err(ContentError::EditTargetMismatch {
                            edit: description,
                            base: format!("revision {expected_revision}"),
                        });
                    }
                    let existing = staged.get(&delete.object_id).ok_or_else(|| {
                        ContentError::MissingBaseObject {
                            object_id: delete.object_id,
                        }
                    })?;
                    let _ = existing;
                    expected_revisions.remove(&delete.object_id);
                    staged.remove(&delete.object_id);
                }
            }
        }
        Ok(())
    }

    pub fn commit(
        self,
        schemas: &[SchemaDefinition],
        base_state: &mut BaseState,
    ) -> Result<BaseState, ContentError> {
        self.commit_with_result(schemas, base_state)
            .map(|(_, state)| state)
    }

    /// Commits the ChangeSet and returns the committed record with the next
    /// object state. `commit` remains available for callers that only need the
    /// resulting state.
    pub fn commit_with_result(
        mut self,
        schemas: &[SchemaDefinition],
        base_state: &mut BaseState,
    ) -> Result<(Self, BaseState), ContentError> {
        self.validate(schemas, base_state)?;
        if self.status != ChangeSetStatus::Approved {
            return Err(ContentError::ChangesetNotApproved {
                status: format!("{:?}", self.status),
            });
        }

        let candidate = base_state.clone();
        let result = Self::apply(self.edits.clone(), &candidate, schemas);
        match result {
            Ok(next_state) => {
                *base_state = next_state.clone();
                self.status = ChangeSetStatus::Committed;
                Ok((self, next_state))
            }
            Err(error) => {
                *base_state = candidate;
                Err(error)
            }
        }
    }

    fn apply(
        edits: Vec<ChangeSetEdit>,
        state: &BaseState,
        schemas: &[SchemaDefinition],
    ) -> Result<BaseState, ContentError> {
        let mut next = state.clone();
        for edit in edits {
            match edit {
                ChangeSetEdit::ObjectCreate(create) => {
                    let mut object = create.object;
                    for status in [
                        ObjectStatus::Submitted,
                        ObjectStatus::Approved,
                        ObjectStatus::Committed,
                    ] {
                        object
                            .transition_to(status)
                            .map_err(|_| ContentError::InvalidEdit {
                                edit: "commit".to_string(),
                                reason: "created object could not advance".to_string(),
                            })?;
                    }
                    next.insert(object.id, object);
                }
                ChangeSetEdit::ObjectUpdate(update) => {
                    let object =
                        next.get_mut(&update.object_id)
                            .ok_or(ContentError::MissingBaseObject {
                                object_id: update.object_id,
                            })?;
                    object.content = update.content;
                    object.revision += 1;
                    object.updated_at = Utc::now();
                }
                ChangeSetEdit::ObjectDelete(delete) => {
                    next.remove(&delete.object_id)
                        .ok_or(ContentError::MissingBaseObject {
                            object_id: delete.object_id,
                        })?;
                }
            }
        }
        let _ = schemas;
        Ok(next)
    }
}

pub fn state_digest(state: &BaseState) -> String {
    let objects: Vec<&Object> = state.values().collect();
    canonical_digest(&objects)
}

fn required_schema<'a>(
    schemas: &'a [SchemaDefinition],
    object: &Object,
    edit: &str,
) -> Result<&'a SchemaDefinition, ContentError> {
    schema_for_object(schemas, object).ok_or_else(|| ContentError::SchemaMismatch {
        edit: edit.to_string(),
        schema_id: object.schema_id,
        schema_version: object.schema_version,
    })
}

fn schema_for_object<'a>(
    schemas: &'a [SchemaDefinition],
    object: &Object,
) -> Option<&'a SchemaDefinition> {
    schemas
        .iter()
        .find(|schema| schema.id == object.schema_id && schema.version == object.schema_version)
}
