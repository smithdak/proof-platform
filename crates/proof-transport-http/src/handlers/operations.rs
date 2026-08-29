//! Operation execution endpoint.

use super::super::state::SharedState;
use super::errors::{execution_error_response, internal_error};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use proof_kernel::{create_proof, ExecutionContext, ExecutionError};
use serde_json::{json, Value};
use std::{path::PathBuf, sync::Arc};

pub(crate) async fn execute_operation(
    State(state): State<SharedState>,
    Path((name, version)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let keypair = state.keypair.clone();
    let context = ExecutionContext {
        actor: keypair.principal_id,
        principal_kind: Some(proof_kernel::PrincipalKind::Agent),
        delegation_id: None,
        delegation_chain: None,
        workspace_path: PathBuf::from(&state.workspace_path),
        timestamp: chrono::Utc::now(),
    };

    let result = match state
        .engine
        .read()
        .unwrap()
        .execute(&name, &version, &body, &context)
    {
        Ok(result) => result,
        Err(error) => return Err(execution_error_response(&error)),
    };

    let proof = match create_proof(
        keypair.principal_id,
        context.delegation_id,
        &name,
        &body,
        &result,
        context.timestamp,
        &keypair,
    ) {
        Ok(proof) => proof,
        Err(error) => return Err(internal_error(error.to_string())),
    };

    let proof = serde_json::to_value(&proof).map_err(|error| internal_error(error.to_string()))?;

    Ok(Json(json!({
        "operation": name,
        "version": version,
        "status": "executed",
        "result": result,
        "proof": proof,
    })))
}
