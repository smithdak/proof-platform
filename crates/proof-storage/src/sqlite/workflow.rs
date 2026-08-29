//! Workflow record storage: definitions, runs, and steps.

use super::store::SqliteStore;
use crate::StorageError;
use chrono::{DateTime, Utc};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

fn parse_uuid(value: &str, context: &str) -> Result<Uuid, StorageError> {
    Uuid::parse_str(value)
        .map_err(|error| StorageError::Conflict(format!("invalid {context}: {error}")))
}

fn parse_timestamp(value: &str) -> Result<DateTime<Utc>, StorageError> {
    DateTime::parse_from_rfc3339(value)
        .map_err(|error| StorageError::Conflict(format!("invalid timestamp: {error}")))
        .map(|timestamp| timestamp.with_timezone(&Utc))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowDefinition {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub steps: Vec<WorkflowStepTemplate>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowStepTemplate {
    pub name: String,
    pub kind: WorkflowStepKind,
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowRunStatus {
    Pending,
    InProgress,
    Approved,
}

impl WorkflowRunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Approved => "approved",
        }
    }

    fn from_str(value: &str) -> Result<Self, StorageError> {
        match value {
            "pending" => Ok(Self::Pending),
            "in_progress" => Ok(Self::InProgress),
            "approved" => Ok(Self::Approved),
            other => Err(StorageError::Conflict(format!(
                "unknown workflow run status: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowRun {
    pub id: Uuid,
    pub workflow_definition_id: Uuid,
    pub status: WorkflowRunStatus,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub approved_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStepKind {
    Agent,
    Human,
}

impl WorkflowStepKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::Human => "human",
        }
    }

    fn from_str(value: &str) -> Result<Self, StorageError> {
        match value {
            "agent" => Ok(Self::Agent),
            "human" => Ok(Self::Human),
            other => Err(StorageError::Conflict(format!(
                "unknown workflow step kind: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStepStatus {
    Pending,
    Completed,
}

impl WorkflowStepStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Completed => "completed",
        }
    }

    fn from_str(value: &str) -> Result<Self, StorageError> {
        match value {
            "pending" => Ok(Self::Pending),
            "completed" => Ok(Self::Completed),
            other => Err(StorageError::Conflict(format!(
                "unknown workflow step status: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowStep {
    pub id: Uuid,
    pub run_id: Uuid,
    pub name: String,
    pub kind: WorkflowStepKind,
    pub description: String,
    pub status: WorkflowStepStatus,
    pub ordinal: u32,
    pub completed_at: Option<DateTime<Utc>>,
}

impl SqliteStore {
    pub fn save_workflow_definition(
        &self,
        definition: &WorkflowDefinition,
    ) -> Result<(), StorageError> {
        if definition.steps.is_empty() {
            return Err(StorageError::Conflict(
                "workflow definition must contain at least one step".to_string(),
            ));
        }
        self.conn.lock().unwrap().execute(
            "
            INSERT INTO workflow_definition
                (id, name, description, steps, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                description = excluded.description,
                steps = excluded.steps,
                updated_at = excluded.updated_at
            ",
            params![
                definition.id.to_string(),
                definition.name,
                definition.description,
                serde_json::to_string(&definition.steps)?,
                definition.created_at.to_rfc3339(),
                definition.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn load_workflow_definition(&self, id: &Uuid) -> Result<WorkflowDefinition, StorageError> {
        let connection = self.conn.lock().unwrap();
        let row = connection
            .query_row(
                "
                SELECT id, name, description, steps, created_at, updated_at
                FROM workflow_definition WHERE id = ?1
                ",
                [id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                    ))
                },
            )
            .optional()?;
        let Some((id, name, description, steps, created_at, updated_at)) = row else {
            return Err(StorageError::NotFound(id.to_string()));
        };
        Ok(WorkflowDefinition {
            id: parse_uuid(&id, "workflow definition ID")?,
            name,
            description,
            steps: serde_json::from_str(&steps)?,
            created_at: parse_timestamp(&created_at)?,
            updated_at: parse_timestamp(&updated_at)?,
        })
    }

    pub fn list_workflow_definitions(&self) -> Result<Vec<WorkflowDefinition>, StorageError> {
        let connection = self.conn.lock().unwrap();
        let mut statement = connection.prepare_cached(
            "
            SELECT id, name, description, steps, created_at, updated_at
            FROM workflow_definition ORDER BY created_at
            ",
        )?;
        let definitions = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        definitions
            .into_iter()
            .map(|(id, name, description, steps, created_at, updated_at)| {
                Ok(WorkflowDefinition {
                    id: parse_uuid(&id, "workflow definition ID")?,
                    name,
                    description,
                    steps: serde_json::from_str(&steps)?,
                    created_at: parse_timestamp(&created_at)?,
                    updated_at: parse_timestamp(&updated_at)?,
                })
            })
            .collect()
    }

    pub fn delete_workflow_definition(&self, id: &Uuid) -> Result<bool, StorageError> {
        let mut connection = self.conn.lock().unwrap();
        let transaction = connection.transaction()?;
        transaction.execute(
            "DELETE FROM workflow_step WHERE run_id IN (
                SELECT id FROM workflow_run WHERE workflow_definition_id = ?1
            )",
            [id.to_string()],
        )?;
        transaction.execute(
            "DELETE FROM workflow_run WHERE workflow_definition_id = ?1",
            [id.to_string()],
        )?;
        let deleted = transaction.execute(
            "DELETE FROM workflow_definition WHERE id = ?1",
            [id.to_string()],
        )?;
        transaction.commit()?;
        Ok(deleted > 0)
    }

    pub fn save_workflow_run(&self, run: &WorkflowRun) -> Result<(), StorageError> {
        self.conn.lock().unwrap().execute(
            "
            INSERT INTO workflow_run
                (id, workflow_definition_id, status, created_at, completed_at, approved_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(id) DO UPDATE SET
                workflow_definition_id = excluded.workflow_definition_id,
                status = excluded.status,
                created_at = excluded.created_at,
                completed_at = excluded.completed_at,
                approved_at = excluded.approved_at
            ",
            params![
                run.id.to_string(),
                run.workflow_definition_id.to_string(),
                run.status.as_str(),
                run.created_at.to_rfc3339(),
                run.completed_at.map(|timestamp| timestamp.to_rfc3339()),
                run.approved_at.map(|timestamp| timestamp.to_rfc3339()),
            ],
        )?;
        Ok(())
    }

    pub fn load_workflow_run(&self, id: &Uuid) -> Result<WorkflowRun, StorageError> {
        let connection = self.conn.lock().unwrap();
        let row = connection
            .query_row(
                "
                SELECT id, workflow_definition_id, status, created_at, completed_at, approved_at
                FROM workflow_run WHERE id = ?1
                ",
                [id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, Option<String>>(5)?,
                    ))
                },
            )
            .optional()?;
        let Some((id, workflow_definition_id, status, created_at, completed_at, approved_at)) = row
        else {
            return Err(StorageError::NotFound(id.to_string()));
        };
        Ok(WorkflowRun {
            id: parse_uuid(&id, "workflow run ID")?,
            workflow_definition_id: parse_uuid(&workflow_definition_id, "workflow definition ID")?,
            status: WorkflowRunStatus::from_str(&status)?,
            created_at: parse_timestamp(&created_at)?,
            completed_at: completed_at
                .map(|timestamp| parse_timestamp(&timestamp))
                .transpose()?,
            approved_at: approved_at
                .map(|timestamp| parse_timestamp(&timestamp))
                .transpose()?,
        })
    }

    pub fn list_workflow_runs(
        &self,
        workflow_definition_id: Option<&Uuid>,
    ) -> Result<Vec<WorkflowRun>, StorageError> {
        let connection = self.conn.lock().unwrap();
        let (query, parameter): (&str, Vec<String>) = match workflow_definition_id {
            Some(_) => (
                "
            SELECT id FROM workflow_run
            WHERE workflow_definition_id = ?1
                ORDER BY created_at
                ",
                workflow_definition_id
                    .map(|id| id.to_string())
                    .into_iter()
                    .collect(),
            ),
            None => (
                "SELECT id FROM workflow_run ORDER BY created_at",
                Vec::new(),
            ),
        };
        let mut statement = connection.prepare_cached(query)?;
        let ids = statement
            .query_map(rusqlite::params_from_iter(parameter.iter()), |row| {
                row.get::<_, String>(0)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        drop(connection);
        ids.iter()
            .map(|id| self.load_workflow_run(&parse_uuid(id, "workflow run ID")?))
            .collect()
    }

    pub fn delete_workflow_run(&self, id: &Uuid) -> Result<bool, StorageError> {
        let mut connection = self.conn.lock().unwrap();
        let transaction = connection.transaction()?;
        transaction.execute(
            "DELETE FROM workflow_step WHERE run_id = ?1",
            [id.to_string()],
        )?;
        let deleted =
            transaction.execute("DELETE FROM workflow_run WHERE id = ?1", [id.to_string()])?;
        transaction.commit()?;
        Ok(deleted > 0)
    }

    pub fn save_workflow_step(&self, step: &WorkflowStep) -> Result<(), StorageError> {
        self.conn.lock().unwrap().execute(
            "
            INSERT INTO workflow_step
                (id, run_id, name, kind, description, status, ordinal, completed_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            ON CONFLICT(id) DO UPDATE SET
                run_id = excluded.run_id,
                name = excluded.name,
                kind = excluded.kind,
                description = excluded.description,
                status = excluded.status,
                ordinal = excluded.ordinal,
                completed_at = excluded.completed_at
            ",
            params![
                step.id.to_string(),
                step.run_id.to_string(),
                step.name,
                step.kind.as_str(),
                step.description,
                step.status.as_str(),
                step.ordinal,
                step.completed_at.map(|timestamp| timestamp.to_rfc3339()),
            ],
        )?;
        Ok(())
    }

    pub fn load_workflow_step(&self, id: &Uuid) -> Result<WorkflowStep, StorageError> {
        let connection = self.conn.lock().unwrap();
        let row = connection
            .query_row(
                "
                SELECT id, run_id, name, kind, description, status, ordinal, completed_at
                FROM workflow_step WHERE id = ?1
                ",
                [id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, i64>(6)?,
                        row.get::<_, Option<String>>(7)?,
                    ))
                },
            )
            .optional()?;
        let Some((id, run_id, name, kind, description, status, ordinal, completed_at)) = row else {
            return Err(StorageError::NotFound(id.to_string()));
        };
        Ok(WorkflowStep {
            id: parse_uuid(&id, "workflow step ID")?,
            run_id: parse_uuid(&run_id, "workflow run ID")?,
            name,
            kind: WorkflowStepKind::from_str(&kind)?,
            description,
            status: WorkflowStepStatus::from_str(&status)?,
            ordinal: ordinal.try_into().map_err(|_| {
                StorageError::Conflict("workflow step ordinal is negative".to_string())
            })?,
            completed_at: completed_at
                .map(|timestamp| parse_timestamp(&timestamp))
                .transpose()?,
        })
    }

    pub fn list_workflow_steps(&self, run_id: &Uuid) -> Result<Vec<WorkflowStep>, StorageError> {
        let connection = self.conn.lock().unwrap();
        let ids: Vec<String> = {
            let mut statement = connection.prepare_cached(
                "SELECT id FROM workflow_step WHERE run_id = ?1 ORDER BY ordinal",
            )?;
            let ids = statement
                .query_map([run_id.to_string()], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            ids
        };
        drop(connection);
        ids.iter()
            .map(|id| self.load_workflow_step(&parse_uuid(id, "workflow step ID")?))
            .collect()
    }

    pub fn delete_workflow_step(&self, id: &Uuid) -> Result<bool, StorageError> {
        let deleted = self
            .conn
            .lock()
            .unwrap()
            .execute("DELETE FROM workflow_step WHERE id = ?1", [id.to_string()])?;
        Ok(deleted > 0)
    }
}
