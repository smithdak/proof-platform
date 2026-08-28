//! Proof query, verification, and audit trail endpoints.

use super::super::state::SharedState;
use super::errors::{bad_request, internal_error};
use crate::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use proof_kernel::{principal_from_keypair, Proof};
use proof_storage::SqliteStore;
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Default, serde::Deserialize)]
pub struct ProofFilters {
    operation: Option<String>,
    version: Option<String>,
    actor: Option<Uuid>,
    limit: Option<usize>,
    offset: Option<usize>,
    sort: Option<String>,
    order: Option<String>,
}

pub(crate) async fn list_proofs(
    State(state): State<SharedState>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    list_proofs_inner(&state, ProofFilters::default()).await
}

pub(crate) async fn list_proofs_filtered(
    State(state): State<SharedState>,
    Query(filters): Query<ProofFilters>,
) -> impl IntoResponse {
    list_proofs_inner(&state, filters).await
}

async fn list_proofs_inner(
    state: &SharedState,
    filters: ProofFilters,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let limit = filters.limit.unwrap_or(20).min(100);
    let offset = filters.offset.unwrap_or(0);
    let sort = match filters.sort.as_deref() {
        None | Some("timestamp") => "timestamp",
        Some("id") => "id",
        Some(_) => {
            return Err(bad_request("sort must be timestamp or id"));
        }
    };
    let order = match filters.order.as_deref() {
        None | Some("desc") => "DESC",
        Some("asc") => "ASC",
        Some(_) => {
            return Err(bad_request("order must be asc or desc"));
        }
    };
    let total = {
        let filter = proof_storage::ProofFilter {
            operation: filters.operation.clone(),
            version: filters.version.clone(),
            actor: filters.actor.map(|actor| actor.to_string()),
        };
        state
            .store
            .count_proofs(&filter)
            .map_err(|error| internal_error(error.to_string()))?
    };
    let mut sql = "
        SELECT signature, operation, actor
        FROM proofs
        WHERE (?1 IS NULL OR operation LIKE ?1 || '::%')
          AND (?2 IS NULL OR version = ?2)
          AND (?3 IS NULL OR actor = ?3)
        ORDER BY {sort} {order}, id
        LIMIT ?4 OFFSET ?5
    "
    .to_string();
    sql = sql.replace("{sort}", sort).replace("{order}", order);
    let serialized_proofs = {
        let connection = state.store.connection();
        let mut statement = connection
            .prepare(&sql)
            .map_err(|error| internal_error(error.to_string()))?;
        let serialized_proofs = statement
            .query_map(
                rusqlite::params![
                    filters.operation,
                    filters.version,
                    filters.actor.map(|actor| actor.to_string()),
                    limit,
                    offset,
                ],
                |row| row.get::<_, String>(0),
            )
            .map_err(|error| internal_error(error.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| internal_error(error.to_string()))?;
        serialized_proofs
    };
    let proofs = serialized_proofs
        .iter()
        .map(|serialized| serde_json::from_str::<Proof>(serialized))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| internal_error(error.to_string()))?;
    Ok(Json(json!({
        "items": proofs,
        "total": total,
        "limit": limit,
        "offset": offset,
    })))
}

pub(crate) async fn get_proof(
    State(state): State<SharedState>,
    Path(proof_id): Path<Uuid>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let proof = state
        .store
        .load_proof(&proof_id)
        .map_err(|error| match error {
            proof_storage::StorageError::NotFound(_) => (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "proof not found"})),
            ),
            error => internal_error(error.to_string()),
        })?;
    let verification = proof_verification_status(&state, &proof)?;
    Ok(Json(json!({
        "proof": proof,
        "verification": verification,
    })))
}

fn proof_verification_status(
    state: &AppState,
    proof: &Proof,
) -> Result<&'static str, (StatusCode, Json<Value>)> {
    let verification = if proof.body.actor == state.keypair.principal_id {
        Ok(proof.verify(&principal_from_keypair(&state.keypair).public_key))
    } else {
        state
            .store
            .load_principal(&proof.body.actor)
            .map(|principal| proof.verify(&principal.public_key))
            .map_err(|error| match error {
                proof_storage::StorageError::NotFound(_) => (
                    StatusCode::NOT_FOUND,
                    Json(json!({"error": "signing principal not found"})),
                ),
                error => internal_error(error.to_string()),
            })
    }?;
    Ok(if verification.is_ok() {
        "verified"
    } else {
        "invalid"
    })
}

#[derive(serde::Deserialize)]
pub struct VerifyProofRequest {
    proof_id: Uuid,
}

pub(crate) async fn verify_proof(
    State(state): State<SharedState>,
    Json(request): Json<VerifyProofRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let proof = state
        .store
        .load_proof(&request.proof_id)
        .map_err(|error| match error {
            proof_storage::StorageError::NotFound(_) => (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "proof not found"})),
            ),
            error => internal_error(error.to_string()),
        })?;
    let proof_actor = proof.body.actor;
    let public_key = if proof_actor == state.keypair.principal_id {
        principal_from_keypair(&state.keypair).public_key
    } else {
        state
            .store
            .load_principal(&proof_actor)
            .map_err(|error| match error {
                proof_storage::StorageError::NotFound(_) => (
                    StatusCode::NOT_FOUND,
                    Json(json!({"error": "signing principal not found"})),
                ),
                error => internal_error(error.to_string()),
            })?
            .public_key
    };
    Ok(Json(json!({
        "proof_id": request.proof_id,
        "valid": proof.verify(&public_key).is_ok(),
    })))
}

pub(crate) async fn list_audit(
    State(state): State<SharedState>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let connection = state.store.connection();
    let audit = {
        let mut statement = connection
            .prepare(
                "SELECT id, actor, workspace_path, timestamp
                 FROM execution_contexts
                 ORDER BY timestamp DESC",
            )
            .map_err(|error| internal_error(error.to_string()))?;
        let audit = statement
            .query_map([], |row| {
                Ok(json!({
                    "id": row.get::<_, String>(0)?,
                    "actor": row.get::<_, String>(1)?,
                    "workspace_path": row.get::<_, String>(2)?,
                    "timestamp": row.get::<_, String>(3)?,
                }))
            })
            .map_err(|error| internal_error(error.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| internal_error(error.to_string()))?;
        audit
    };
    Ok(Json(json!({ "contexts": audit })))
}
