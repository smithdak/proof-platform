use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

pub type WorkflowDefinitionId = Uuid;
pub type WorkflowRunId = Uuid;
pub type WorkflowStepId = Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowRunStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStepStatus {
    Pending,
    Completed,
    Approved,
}

#[derive(Debug, Clone, Error)]
#[error("invalid workflow run transition from {from:?} to {to}")]
pub struct WorkflowRunTransitionError {
    from: WorkflowRunStatus,
    to: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowDefinition {
    pub id: WorkflowDefinitionId,
    pub name: String,
    pub description: String,
    pub steps: Vec<WorkflowStepBlueprint>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowStepBlueprint {
    pub key: String,
    pub name: String,
    pub requires_approval: bool,
}

impl WorkflowDefinition {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        steps: Vec<WorkflowStepBlueprint>,
    ) -> Result<Self, String> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err("name must not be empty".to_string());
        }
        if steps.is_empty() {
            return Err("workflow must contain at least one step".to_string());
        }
        let mut seen = std::collections::HashSet::new();
        for step in &steps {
            if !seen.insert(step.key.as_str()) {
                return Err(format!("duplicate workflow step key: {}", step.key));
            }
        }
        let now = Utc::now();
        Ok(Self {
            id: Uuid::now_v7(),
            name,
            description: description.into(),
            steps,
            created_at: now,
            updated_at: now,
        })
    }
}

impl WorkflowStepBlueprint {
    pub fn new(
        key: impl Into<String>,
        name: impl Into<String>,
        requires_approval: bool,
    ) -> Result<Self, String> {
        let key = key.into();
        if key.trim().is_empty() {
            return Err("step key must not be empty".to_string());
        }
        let name = name.into();
        if name.trim().is_empty() {
            return Err("step name must not be empty".to_string());
        }
        Ok(Self {
            key,
            name,
            requires_approval,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowRun {
    pub id: WorkflowRunId,
    pub workflow_definition_id: WorkflowDefinitionId,
    pub status: WorkflowRunStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl WorkflowRun {
    pub fn new(workflow_definition_id: WorkflowDefinitionId) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::now_v7(),
            workflow_definition_id,
            status: WorkflowRunStatus::Pending,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn transition_to(
        &mut self,
        next: WorkflowRunStatus,
    ) -> Result<(), WorkflowRunTransitionError> {
        let allowed = matches!(
            (self.status, next),
            (WorkflowRunStatus::Pending, WorkflowRunStatus::InProgress)
                | (WorkflowRunStatus::InProgress, WorkflowRunStatus::Completed)
                | (WorkflowRunStatus::InProgress, WorkflowRunStatus::Failed)
        );
        if allowed {
            self.status = next;
            self.updated_at = Utc::now();
            Ok(())
        } else {
            Err(WorkflowRunTransitionError {
                from: self.status,
                to: serde_json::to_value(next)
                    .expect("status serialization cannot fail")
                    .as_str()
                    .expect("status is a string")
                    .to_string(),
            })
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowStep {
    pub id: WorkflowStepId,
    pub workflow_run_id: WorkflowRunId,
    pub workflow_definition_id: WorkflowDefinitionId,
    pub key: String,
    pub name: String,
    pub requires_approval: bool,
    pub status: WorkflowStepStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl WorkflowStep {
    pub fn complete(&mut self) -> Result<(), String> {
        if self.status != WorkflowStepStatus::Pending {
            return Err(format!("workflow step {} is not pending", self.key));
        }
        self.status = if self.requires_approval {
            WorkflowStepStatus::Completed
        } else {
            WorkflowStepStatus::Approved
        };
        self.updated_at = Utc::now();
        Ok(())
    }

    pub fn approve(&mut self) -> Result<(), String> {
        if self.status != WorkflowStepStatus::Completed {
            return Err(format!(
                "workflow step {} must be completed before approval",
                self.key
            ));
        }
        self.status = WorkflowStepStatus::Approved;
        self.updated_at = Utc::now();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blueprint(key: &str, requires_approval: bool) -> WorkflowStepBlueprint {
        WorkflowStepBlueprint::new(key, format!("{key} step"), requires_approval).unwrap()
    }

    #[test]
    fn workflow_definition_validates_name_and_steps() {
        let definition = WorkflowDefinition::new(
            "Release",
            "Publish",
            vec![blueprint("review", true), blueprint("deploy", false)],
        )
        .unwrap();
        assert_eq!(definition.steps.len(), 2);
        assert!(WorkflowDefinition::new(" ", "", vec![blueprint("x", false)]).is_err());
        assert!(WorkflowDefinition::new("Empty", "", vec![]).is_err());
        assert!(WorkflowDefinition::new(
            "Duplicate",
            "",
            vec![blueprint("x", false), blueprint("x", true)]
        )
        .is_err());
    }

    #[test]
    fn workflow_step_blueprint_validates_fields() {
        assert!(WorkflowStepBlueprint::new("", "Name", false).is_err());
        assert!(WorkflowStepBlueprint::new("key", "", false).is_err());
        let step = WorkflowStepBlueprint::new("key", "Name", true).unwrap();
        assert!(step.requires_approval);
    }

    #[test]
    fn workflow_run_has_valid_lifecycle() {
        let mut run = WorkflowRun::new(Uuid::now_v7());
        assert_eq!(run.status, WorkflowRunStatus::Pending);
        run.transition_to(WorkflowRunStatus::InProgress).unwrap();
        run.transition_to(WorkflowRunStatus::Completed).unwrap();
        assert!(run.transition_to(WorkflowRunStatus::InProgress).is_err());
    }

    #[test]
    fn workflow_run_rejects_invalid_transition() {
        let mut run = WorkflowRun::new(Uuid::now_v7());
        let error = run.transition_to(WorkflowRunStatus::Completed).unwrap_err();
        assert_eq!(
            error.to_string(),
            "invalid workflow run transition from Pending to completed"
        );
    }

    #[test]
    fn workflow_step_completes_and_approves() {
        let now = Utc::now();
        let mut requiring = WorkflowStep {
            id: Uuid::now_v7(),
            workflow_run_id: Uuid::now_v7(),
            workflow_definition_id: Uuid::now_v7(),
            key: "review".to_string(),
            name: "Review".to_string(),
            requires_approval: true,
            status: WorkflowStepStatus::Pending,
            created_at: now,
            updated_at: now,
        };
        requiring.complete().unwrap();
        assert_eq!(requiring.status, WorkflowStepStatus::Completed);
        requiring.approve().unwrap();
        assert_eq!(requiring.status, WorkflowStepStatus::Approved);

        let mut automatic = WorkflowStep {
            requires_approval: false,
            ..requiring.clone()
        };
        automatic.status = WorkflowStepStatus::Pending;
        automatic.complete().unwrap();
        assert_eq!(automatic.status, WorkflowStepStatus::Approved);
    }

    #[test]
    fn workflow_step_rejects_invalid_operations() {
        let now = Utc::now();
        let mut step = WorkflowStep {
            id: Uuid::now_v7(),
            workflow_run_id: Uuid::now_v7(),
            workflow_definition_id: Uuid::now_v7(),
            key: "review".to_string(),
            name: "Review".to_string(),
            requires_approval: true,
            status: WorkflowStepStatus::Approved,
            created_at: now,
            updated_at: now,
        };
        assert!(step.complete().is_err());
        assert!(step.approve().is_err());
        step.status = WorkflowStepStatus::Pending;
        assert!(step.approve().is_err());
    }
}
