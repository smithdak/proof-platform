//! Operation execution endpoint.

use super::super::state::SharedState;
use super::errors::{execution_error_response, internal_error};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use proof_kernel::ExecutionContext;
use serde_json::{json, Value};
use std::path::PathBuf;

pub(crate) async fn execute_operation(
    State(state): State<SharedState>,
    Path((name, version)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let keypair = state.keypair.clone();
    let context = ExecutionContext {
        actor: keypair.principal_id,
        // This endpoint has no authenticated human identity. Caller-supplied
        // headers must never elevate the workspace service actor.
        principal_kind: Some(proof_kernel::PrincipalKind::Agent),
        delegation_id: None,
        delegation_chain: None,
        workspace_path: PathBuf::from(&state.workspace_path),
        timestamp: chrono::Utc::now(),
    };

    let outcome = match state
        .engine
        .read()
        .unwrap()
        .execute_evidenced(&name, &version, &body, &context)
    {
        Ok(outcome) => outcome,
        Err(error) => return Err(execution_error_response(&error)),
    };

    let proof =
        serde_json::to_value(&outcome.proof).map_err(|error| internal_error(error.to_string()))?;

    Ok(Json(json!({
        "operation": name,
        "version": version,
        "status": "executed",
        "result": outcome.output,
        "proof": proof,
    })))
}
