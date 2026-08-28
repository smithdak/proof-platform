//! The storage backend trait for execution evidence, plus a test recorder.

use super::context::{AuditFilter, ExecutionContext};
use super::error::ExecutionError;
use crate::delegation::Delegation;
use crate::evidence::Proof;
use serde_json::Value;
use uuid::Uuid;

pub trait ExecutionStore: Send + Sync {
    /// Loads a stored delegation by ID. Return `None` when it is unknown.
    fn load_delegation(&self, delegation_id: &Uuid) -> Result<Option<Delegation>, String> {
        let _ = delegation_id;
        Ok(None)
    }

    /// Persists a generated proof.
    fn save_proof(&self, proof: &Proof) -> Result<(), String>;

    /// Loads the most recent proof recorded for an operation/version.
    fn latest_proof_for_operation(
        &self,
        _operation: &str,
        _version: &str,
    ) -> Result<Option<Proof>, String> {
        Ok(None)
    }

    /// Persists the execution context and returns its storage identifier.
    fn save_execution_context(&self, context: &ExecutionContext) -> Result<String, String>;

    /// Loads audit contexts matching the filter.
    ///
    /// The default returns no records so storage backends can adopt audit
    /// querying independently of kernel changes.
    fn load_audit_contexts(&self, _filter: &AuditFilter) -> Result<Vec<ExecutionContext>, String> {
        Ok(Vec::new())
    }
}

/// A simple in-memory execution store for testing.
#[derive(Default)]
pub struct RecordingStore {
    pub proofs: std::sync::Mutex<Vec<Proof>>,
    pub contexts: std::sync::Mutex<Vec<ExecutionContext>>,
    pub delegations: std::sync::Mutex<Vec<Delegation>>,
}

impl ExecutionStore for RecordingStore {
    fn load_delegation(&self, delegation_id: &Uuid) -> Result<Option<Delegation>, String> {
        Ok(self
            .delegations
            .lock()
            .unwrap()
            .iter()
            .find(|delegation| &delegation.id == delegation_id)
            .cloned())
    }

    fn save_proof(&self, proof: &Proof) -> Result<(), String> {
        self.proofs.lock().unwrap().push(proof.clone());
        Ok(())
    }

    fn save_execution_context(&self, context: &ExecutionContext) -> Result<String, String> {
        self.contexts.lock().unwrap().push(context.clone());
        Ok(Uuid::now_v7().to_string())
    }

    fn latest_proof_for_operation(
        &self,
        operation: &str,
        version: &str,
    ) -> Result<Option<Proof>, String> {
        let full_operation = format!("{operation}::{version}");
        Ok(self
            .proofs
            .lock()
            .unwrap()
            .iter()
            .filter(|proof| proof.body.operation == full_operation)
            .max_by_key(|proof| proof.body.timestamp)
            .cloned())
    }
}

/// A handler that executes a specific operation.
pub trait OperationHandler: Send + Sync {
    /// The operation name this handler executes.
    fn operation(&self) -> &str;
    /// Executes the operation with the given input and context.
    fn execute(&self, input: &Value, context: &ExecutionContext) -> Result<Value, ExecutionError>;
}
