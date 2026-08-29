//! Agent definitions, runtime budgets, and append-only run events.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use uuid::Uuid;

use crate::canonical::{canonicalize, digest, ArtifactKind, ContentDigest};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentTool {
    pub operation: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentLimits {
    pub max_steps: u32,
    pub max_model_calls: u32,
    pub max_total_tokens: u64,
    pub max_duration_seconds: u64,
    pub max_output_tokens_per_call: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cost_microusd: Option<u64>,
}

impl Default for AgentLimits {
    fn default() -> Self {
        Self {
            max_steps: 16,
            max_model_calls: 24,
            max_total_tokens: 100_000,
            max_duration_seconds: 900,
            max_output_tokens_per_call: 4_096,
            max_cost_microusd: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentDefinition {
    pub id: Uuid,
    pub name: String,
    pub instructions: String,
    pub provider: String,
    pub model: String,
    pub tools: Vec<AgentTool>,
    pub limits: AgentLimits,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunEventKind {
    Started,
    ModelRequested,
    ModelResponded,
    ToolRequested,
    ToolSucceeded,
    ToolFailed,
    ApprovalRequired,
    ApprovalResumed,
    Completed,
    Failed,
    BudgetExceeded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRunEvent {
    pub id: Uuid,
    pub run_id: Uuid,
    pub sequence: u32,
    pub kind: AgentRunEventKind,
    pub data: Value,
    pub data_digest: ContentDigest,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AgentDefinitionError {
    #[error("agent name must not be empty")]
    EmptyName,
    #[error("agent instructions must not be empty")]
    EmptyInstructions,
    #[error("agent model provider must not be empty")]
    EmptyProvider,
    #[error("agent model must not be empty")]
    EmptyModel,
    #[error("agent must allow at least one tool")]
    NoTools,
    #[error("agent tool operation must use domain.action format: {0}")]
    InvalidOperation(String),
    #[error("agent tool version must use v<N> format: {0}")]
    InvalidVersion(String),
    #[error("agent tool is duplicated: {operation}::{version}")]
    DuplicateTool { operation: String, version: String },
    #[error("agent limits must all be greater than zero")]
    InvalidLimits,
    #[error("agent event data could not be canonicalized")]
    InvalidEventData,
}

impl AgentTool {
    pub fn new(
        operation: impl Into<String>,
        version: impl Into<String>,
    ) -> Result<Self, AgentDefinitionError> {
        let operation = operation.into();
        if !valid_operation(&operation) {
            return Err(AgentDefinitionError::InvalidOperation(operation));
        }
        let version = version.into();
        if !valid_version(&version) {
            return Err(AgentDefinitionError::InvalidVersion(version));
        }
        Ok(Self { operation, version })
    }

    pub fn key(&self) -> String {
        format!("{}::{}", self.operation, self.version)
    }
}

impl AgentLimits {
    pub fn validate(&self) -> Result<(), AgentDefinitionError> {
        if self.max_steps == 0
            || self.max_model_calls == 0
            || self.max_total_tokens == 0
            || self.max_duration_seconds == 0
            || self.max_output_tokens_per_call == 0
            || self.max_cost_microusd == Some(0)
        {
            return Err(AgentDefinitionError::InvalidLimits);
        }
        Ok(())
    }
}

impl AgentDefinition {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        name: impl Into<String>,
        instructions: impl Into<String>,
        provider: impl Into<String>,
        model: impl Into<String>,
        tools: Vec<AgentTool>,
        limits: AgentLimits,
        created_at: DateTime<Utc>,
    ) -> Result<Self, AgentDefinitionError> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(AgentDefinitionError::EmptyName);
        }
        let instructions = instructions.into();
        if instructions.trim().is_empty() {
            return Err(AgentDefinitionError::EmptyInstructions);
        }
        let provider = provider.into();
        if provider.trim().is_empty() {
            return Err(AgentDefinitionError::EmptyProvider);
        }
        let model = model.into();
        if model.trim().is_empty() {
            return Err(AgentDefinitionError::EmptyModel);
        }
        if tools.is_empty() {
            return Err(AgentDefinitionError::NoTools);
        }
        let mut unique = BTreeSet::new();
        for tool in &tools {
            if !valid_operation(&tool.operation) {
                return Err(AgentDefinitionError::InvalidOperation(
                    tool.operation.clone(),
                ));
            }
            if !valid_version(&tool.version) {
                return Err(AgentDefinitionError::InvalidVersion(tool.version.clone()));
            }
            if !unique.insert((tool.operation.clone(), tool.version.clone())) {
                return Err(AgentDefinitionError::DuplicateTool {
                    operation: tool.operation.clone(),
                    version: tool.version.clone(),
                });
            }
        }
        limits.validate()?;
        Ok(Self {
            id: Uuid::now_v7(),
            name,
            instructions,
            provider,
            model,
            tools,
            limits,
            created_at,
        })
    }
}

impl AgentRunEvent {
    pub fn create(
        run_id: Uuid,
        sequence: u32,
        kind: AgentRunEventKind,
        data: Value,
        created_at: DateTime<Utc>,
    ) -> Result<Self, AgentDefinitionError> {
        let canonical = canonicalize(&data).map_err(|_| AgentDefinitionError::InvalidEventData)?;
        Ok(Self {
            id: Uuid::now_v7(),
            run_id,
            sequence,
            kind,
            data,
            data_digest: digest(ArtifactKind::AgentEvent, &canonical),
            created_at,
        })
    }
}

pub trait AgentStore: Send + Sync {
    fn save_agent_definition(&self, agent: &AgentDefinition) -> Result<(), String>;
    fn load_agent_definition(&self, agent_id: &Uuid) -> Result<Option<AgentDefinition>, String>;
    fn list_agent_definitions(&self) -> Result<Vec<AgentDefinition>, String>;
    fn save_agent_run_event(&self, event: &AgentRunEvent) -> Result<(), String>;
    fn list_agent_run_events(&self, run_id: &Uuid) -> Result<Vec<AgentRunEvent>, String>;
}

#[derive(Default)]
pub struct RecordingAgentStore {
    definitions: Mutex<BTreeMap<Uuid, AgentDefinition>>,
    events: Mutex<BTreeMap<Uuid, AgentRunEvent>>,
}

impl AgentStore for RecordingAgentStore {
    fn save_agent_definition(&self, agent: &AgentDefinition) -> Result<(), String> {
        let mut definitions = self
            .definitions
            .lock()
            .map_err(|_| "agent definition lock poisoned".to_string())?;
        if definitions
            .values()
            .any(|existing| existing.name == agent.name && existing.id != agent.id)
        {
            return Err(format!("agent name already exists: {}", agent.name));
        }
        match definitions.get(&agent.id) {
            Some(existing) if existing == agent => Ok(()),
            Some(_) => Err(format!("conflicting agent definition: {}", agent.id)),
            None => {
                definitions.insert(agent.id, agent.clone());
                Ok(())
            }
        }
    }

    fn load_agent_definition(&self, agent_id: &Uuid) -> Result<Option<AgentDefinition>, String> {
        Ok(self
            .definitions
            .lock()
            .map_err(|_| "agent definition lock poisoned".to_string())?
            .get(agent_id)
            .cloned())
    }

    fn list_agent_definitions(&self) -> Result<Vec<AgentDefinition>, String> {
        let mut definitions = self
            .definitions
            .lock()
            .map_err(|_| "agent definition lock poisoned".to_string())?
            .values()
            .cloned()
            .collect::<Vec<_>>();
        definitions.sort_by_key(|agent| (agent.created_at, agent.id));
        Ok(definitions)
    }

    fn save_agent_run_event(&self, event: &AgentRunEvent) -> Result<(), String> {
        let mut events = self
            .events
            .lock()
            .map_err(|_| "agent run event lock poisoned".to_string())?;
        if events.values().any(|existing| {
            existing.run_id == event.run_id
                && existing.sequence == event.sequence
                && existing.id != event.id
        }) {
            return Err(format!(
                "agent run event sequence {} already exists for run {}",
                event.sequence, event.run_id
            ));
        }
        match events.get(&event.id) {
            Some(existing) if existing == event => Ok(()),
            Some(_) => Err(format!("conflicting agent run event: {}", event.id)),
            None => {
                events.insert(event.id, event.clone());
                Ok(())
            }
        }
    }

    fn list_agent_run_events(&self, run_id: &Uuid) -> Result<Vec<AgentRunEvent>, String> {
        let mut events = self
            .events
            .lock()
            .map_err(|_| "agent run event lock poisoned".to_string())?
            .values()
            .filter(|event| event.run_id == *run_id)
            .cloned()
            .collect::<Vec<_>>();
        events.sort_by_key(|event| event.sequence);
        Ok(events)
    }
}

fn valid_operation(operation: &str) -> bool {
    let mut parts = operation.split('.');
    parts.next().is_some_and(|part| !part.is_empty())
        && parts.next().is_some_and(|part| !part.is_empty())
        && parts.all(|part| !part.is_empty())
}

fn valid_version(version: &str) -> bool {
    version.strip_prefix('v').is_some_and(|number| {
        !number.is_empty() && number.chars().all(|character| character.is_ascii_digit())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn definition() -> AgentDefinition {
        AgentDefinition::new(
            "release-manager",
            "Prepare and publish releases.",
            "openai",
            "test-model",
            vec![AgentTool::new("release.publish", "v1").unwrap()],
            AgentLimits::default(),
            Utc::now(),
        )
        .unwrap()
    }

    #[test]
    fn definition_validates_tools_and_limits() {
        assert!(AgentTool::new("release.publish", "v1").is_ok());
        assert!(AgentTool::new("release", "v1").is_err());
        assert!(AgentTool::new("release.publish", "1").is_err());
        let tool = AgentTool::new("release.publish", "v1").unwrap();
        assert_eq!(tool.key(), "release.publish::v1");

        let mut limits = AgentLimits::default();
        limits.max_steps = 0;
        assert_eq!(limits.validate(), Err(AgentDefinitionError::InvalidLimits));
    }

    #[test]
    fn definition_rejects_duplicate_tools() {
        let tool = AgentTool::new("release.publish", "v1").unwrap();
        let error = AgentDefinition::new(
            "release-manager",
            "Publish releases.",
            "openai",
            "test-model",
            vec![tool.clone(), tool],
            AgentLimits::default(),
            Utc::now(),
        )
        .unwrap_err();
        assert!(matches!(error, AgentDefinitionError::DuplicateTool { .. }));
    }

    #[test]
    fn accepts_nested_operation_names() {
        let tool = AgentTool::new("analytics.snapshot.create", "v1").unwrap();
        assert_eq!(tool.key(), "analytics.snapshot.create::v1");
    }

    #[test]
    fn event_digest_and_recording_store_round_trip() {
        let store = RecordingAgentStore::default();
        let agent = definition();
        store.save_agent_definition(&agent).unwrap();
        store.save_agent_definition(&agent).unwrap();
        assert_eq!(
            store.load_agent_definition(&agent.id).unwrap(),
            Some(agent.clone())
        );
        assert_eq!(store.list_agent_definitions().unwrap(), vec![agent]);

        let run_id = Uuid::now_v7();
        let event = AgentRunEvent::create(
            run_id,
            0,
            AgentRunEventKind::Started,
            json!({"goal": "publish"}),
            Utc::now(),
        )
        .unwrap();
        let canonical = canonicalize(&event.data).unwrap();
        assert_eq!(
            event.data_digest,
            digest(ArtifactKind::AgentEvent, &canonical)
        );
        store.save_agent_run_event(&event).unwrap();
        assert_eq!(store.list_agent_run_events(&run_id).unwrap(), vec![event]);
    }
}
