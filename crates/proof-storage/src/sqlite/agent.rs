//! Durable agent definitions and append-only runtime events.

use proof_kernel::{AgentDefinition, AgentRunEvent, AgentRunEventKind, AgentStore};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use uuid::Uuid;

use super::store::SqliteStore;
use crate::StorageError;

impl SqliteStore {
    /// Saves an immutable agent definition.
    pub fn save_agent_definition(&self, agent: &AgentDefinition) -> Result<(), StorageError> {
        let serialized = serde_json::to_string(agent)?;
        let connection = self.conn.lock().unwrap();
        connection.execute(
            "INSERT OR IGNORE INTO agent_definitions (
                id, name, provider, model, created_at, definition_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                agent.id.to_string(),
                agent.name,
                agent.provider,
                agent.model,
                agent.created_at.to_rfc3339(),
                serialized,
            ],
        )?;
        let existing: String = connection.query_row(
            "SELECT definition_json FROM agent_definitions
             WHERE id = ?1 OR name = ?2 LIMIT 1",
            params![agent.id.to_string(), agent.name],
            |row| row.get(0),
        )?;
        if existing != serialized {
            return Err(StorageError::Conflict(format!(
                "conflicting agent definition: {}",
                agent.id
            )));
        }
        Ok(())
    }

    /// Loads an agent definition by ID.
    pub fn load_agent_definition(
        &self,
        agent_id: &Uuid,
    ) -> Result<Option<AgentDefinition>, StorageError> {
        self.conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT definition_json FROM agent_definitions WHERE id = ?1",
                [agent_id.to_string()],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(|serialized| serde_json::from_str(&serialized).map_err(StorageError::from))
            .transpose()
    }

    /// Lists agent definitions in creation order.
    pub fn list_agent_definitions(&self) -> Result<Vec<AgentDefinition>, StorageError> {
        let connection = self.conn.lock().unwrap();
        let mut statement = connection.prepare_cached(
            "SELECT definition_json FROM agent_definitions ORDER BY created_at, id",
        )?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        rows.map(|row| {
            let serialized = row?;
            serde_json::from_str(&serialized).map_err(StorageError::from)
        })
        .collect()
    }

    /// Appends one immutable event to an agent run.
    pub fn save_agent_run_event(&self, event: &AgentRunEvent) -> Result<(), StorageError> {
        let serialized = serde_json::to_string(event)?;
        let connection = self.conn.lock().unwrap();
        let transaction = Transaction::new_unchecked(&connection, TransactionBehavior::Immediate)?;
        let existing = transaction
            .query_row(
                "SELECT event_json FROM agent_run_events
                 WHERE id = ?1 OR (run_id = ?2 AND sequence = ?3)
                 LIMIT 1",
                params![
                    event.id.to_string(),
                    event.run_id.to_string(),
                    i64::from(event.sequence),
                ],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        if let Some(existing) = existing {
            if existing != serialized {
                return Err(StorageError::Conflict(format!(
                    "conflicting event {} for agent run {}",
                    event.sequence, event.run_id
                )));
            }
            transaction.commit()?;
            return Ok(());
        }

        reject_if_agent_trace_sealed(&transaction, &event.run_id, "append an event")?;
        validate_event_run_status(&transaction, event)?;
        validate_next_event_sequence(&transaction, event)?;
        transaction.execute(
            "INSERT INTO agent_run_events (
                id, run_id, sequence, kind, data_digest, created_at, event_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                event.id.to_string(),
                event.run_id.to_string(),
                i64::from(event.sequence),
                event_kind(event.kind),
                event.data_digest.hex(),
                event.created_at.to_rfc3339(),
                serialized,
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Lists append-only events for an agent run.
    pub fn list_agent_run_events(&self, run_id: &Uuid) -> Result<Vec<AgentRunEvent>, StorageError> {
        let connection = self.conn.lock().unwrap();
        let mut statement = connection.prepare_cached(
            "SELECT event_json FROM agent_run_events
             WHERE run_id = ?1 ORDER BY sequence",
        )?;
        let rows = statement.query_map([run_id.to_string()], |row| row.get::<_, String>(0))?;
        rows.map(|row| {
            let serialized = row?;
            serde_json::from_str(&serialized).map_err(StorageError::from)
        })
        .collect()
    }
}

pub(super) fn reject_if_agent_trace_sealed(
    connection: &Connection,
    run_id: &Uuid,
    action: &str,
) -> Result<(), StorageError> {
    let terminal = connection
        .query_row(
            "SELECT events.id, events.sequence, events.kind
             FROM agent_run_events AS events
             JOIN agent_runs AS runs ON runs.id = events.run_id
             WHERE events.run_id = ?1
               AND (
                   (runs.status = 'succeeded' AND events.kind = 'completed')
                   OR (runs.status = 'failed'
                       AND events.kind IN ('failed', 'budget_exceeded'))
               )
             ORDER BY events.sequence
             LIMIT 1",
            [run_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?;
    if let Some((event_id, sequence, kind)) = terminal {
        return Err(StorageError::Conflict(format!(
            "agent run {run_id} trace is sealed by {kind} event {event_id} at sequence {sequence}; cannot {action}"
        )));
    }
    let status = connection
        .query_row(
            "SELECT status FROM agent_runs WHERE id = ?1",
            [run_id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if status.as_deref() == Some("cancelled") {
        return Err(StorageError::Conflict(format!(
            "agent run {run_id} trace is sealed by cancelled run status; cannot {action}"
        )));
    }
    Ok(())
}

pub(super) fn reject_if_approval_is_bound_to_sealed_trace(
    connection: &Connection,
    request_id: &Uuid,
    action: &str,
) -> Result<(), StorageError> {
    let run_id = connection
        .query_row(
            "SELECT run_id FROM agent_run_steps
             WHERE approval_request_id = ?1
             LIMIT 1",
            [request_id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let Some(run_id) = run_id else {
        return Ok(());
    };
    let run_id = Uuid::parse_str(&run_id).map_err(|error| {
        StorageError::Conflict(format!(
            "approval request {request_id} is bound to an invalid agent run ID: {error}"
        ))
    })?;
    reject_if_agent_trace_sealed(connection, &run_id, action)
}

fn validate_event_run_status(
    connection: &Connection,
    event: &AgentRunEvent,
) -> Result<(), StorageError> {
    let status = connection
        .query_row(
            "SELECT status FROM agent_runs WHERE id = ?1",
            [event.run_id.to_string()],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or_else(|| StorageError::NotFound(format!("agent run {}", event.run_id)))?;
    let matches_terminal_status = match event.kind {
        AgentRunEventKind::Completed => Some(status == "succeeded"),
        AgentRunEventKind::Failed | AgentRunEventKind::BudgetExceeded => Some(status == "failed"),
        _ => None,
    };
    if let Some(matches_terminal_status) = matches_terminal_status {
        if !matches_terminal_status {
            return Err(StorageError::Conflict(format!(
                "terminal {} event for agent run {} does not match stored run status {status}",
                event_kind(event.kind),
                event.run_id
            )));
        }
    } else if matches!(status.as_str(), "succeeded" | "failed" | "cancelled") {
        return Err(StorageError::Conflict(format!(
            "agent run {} is already {status}; only its matching terminal event may be appended",
            event.run_id
        )));
    }
    Ok(())
}

fn validate_next_event_sequence(
    connection: &Connection,
    event: &AgentRunEvent,
) -> Result<(), StorageError> {
    let (count, minimum, maximum) = connection.query_row(
        "SELECT COUNT(*), MIN(sequence), MAX(sequence)
         FROM agent_run_events WHERE run_id = ?1",
        [event.run_id.to_string()],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<i64>>(1)?,
                row.get::<_, Option<i64>>(2)?,
            ))
        },
    )?;
    let expected = if count == 0 {
        0
    } else {
        let expected_maximum = count.checked_sub(1).ok_or_else(|| {
            StorageError::Conflict(format!("agent run {} event count underflow", event.run_id))
        })?;
        if minimum != Some(0) || maximum != Some(expected_maximum) {
            return Err(StorageError::Conflict(format!(
                "existing event trace for agent run {} is not contiguous",
                event.run_id
            )));
        }
        count
    };
    if i64::from(event.sequence) != expected {
        return Err(StorageError::Conflict(format!(
            "agent run {} event sequence must be contiguous: expected {expected}, supplied {}",
            event.run_id, event.sequence
        )));
    }
    Ok(())
}

impl AgentStore for SqliteStore {
    fn save_agent_definition(&self, agent: &AgentDefinition) -> Result<(), String> {
        SqliteStore::save_agent_definition(self, agent).map_err(|error| error.to_string())
    }

    fn load_agent_definition(&self, agent_id: &Uuid) -> Result<Option<AgentDefinition>, String> {
        SqliteStore::load_agent_definition(self, agent_id).map_err(|error| error.to_string())
    }

    fn list_agent_definitions(&self) -> Result<Vec<AgentDefinition>, String> {
        SqliteStore::list_agent_definitions(self).map_err(|error| error.to_string())
    }

    fn save_agent_run_event(&self, event: &AgentRunEvent) -> Result<(), String> {
        SqliteStore::save_agent_run_event(self, event).map_err(|error| error.to_string())
    }

    fn list_agent_run_events(&self, run_id: &Uuid) -> Result<Vec<AgentRunEvent>, String> {
        SqliteStore::list_agent_run_events(self, run_id).map_err(|error| error.to_string())
    }
}

fn event_kind(kind: AgentRunEventKind) -> &'static str {
    match kind {
        AgentRunEventKind::Started => "started",
        AgentRunEventKind::ModelRequested => "model_requested",
        AgentRunEventKind::ModelResponded => "model_responded",
        AgentRunEventKind::ToolRequested => "tool_requested",
        AgentRunEventKind::ToolSucceeded => "tool_succeeded",
        AgentRunEventKind::ToolFailed => "tool_failed",
        AgentRunEventKind::ApprovalRequired => "approval_required",
        AgentRunEventKind::ApprovalResumed => "approval_resumed",
        AgentRunEventKind::Completed => "completed",
        AgentRunEventKind::Failed => "failed",
        AgentRunEventKind::BudgetExceeded => "budget_exceeded",
    }
}
