//! HTTP/REST transport adapter for the Proof platform.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{get, post},
    Router,
};
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub workspace_path: String,
    pub version: String,
}

pub type SharedState = Arc<AppState>;

pub fn router(state: SharedState) -> Router {
    Router::new()
        .route("/", get(root))
        .route("/health", get(health))
        .route("/capabilities", get(capabilities))
        .route("/v1/operations/:name/:version", post(execute_operation))
        .route("/v1/schemas", get(list_schemas))
        .route("/v1/objects", get(list_objects))
        .route("/v1/proofs", get(list_proofs))
        .with_state(state)
}

async fn root() -> impl IntoResponse {
    Json(json!({
        "name": "proof",
        "description": "Governed agent-native content platform",
        "api_version": "v1"
    }))
}

async fn health() -> impl IntoResponse {
    Json(json!({"status": "ok"}))
}

async fn capabilities() -> impl IntoResponse {
    Json(json!({
        "operations": [
            {"name": "object.create", "version": "v1", "domain": "content", "governance": "agent-executable"},
            {"name": "schema.create", "version": "v1", "domain": "content", "governance": "agent-executable"},
            {"name": "changeset.create", "version": "v1", "domain": "content", "governance": "agent-executable"},
            {"name": "edition.create", "version": "v1", "domain": "content", "governance": "agent-executable"},
            {"name": "release.publish", "version": "v1", "domain": "content", "governance": "agent-executable"}
        ]
    }))
}

async fn list_schemas(State(state): State<SharedState>) -> impl IntoResponse {
    let dir = std::path::Path::new(&state.workspace_path).join(".proof/data/schemas");
    let mut schemas = vec![];
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                if let Ok(value) = serde_json::from_str::<Value>(&content) {
                    schemas.push(value);
                }
            }
        }
    }
    Json(json!({"schemas": schemas}))
}

async fn list_objects(State(state): State<SharedState>) -> impl IntoResponse {
    let dir = std::path::Path::new(&state.workspace_path).join(".proof/data/objects");
    let mut objects = vec![];
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                if let Ok(value) = serde_json::from_str::<Value>(&content) {
                    objects.push(value);
                }
            }
        }
    }
    Json(json!({"objects": objects}))
}

async fn list_proofs(State(state): State<SharedState>) -> impl IntoResponse {
    let dir = std::path::Path::new(&state.workspace_path).join(".proof/data/proofs");
    let mut proofs = vec![];
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                if let Ok(value) = serde_json::from_str::<Value>(&content) {
                    proofs.push(value);
                }
            }
        }
    }
    Json(json!({"proofs": proofs}))
}

async fn execute_operation(
    Path((name, version)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, StatusCode> {
    // TODO: route through kernel registry to actual operation handlers
    Ok(Json(json!({
        "operation": name,
        "version": version,
        "status": "accepted",
        "input": body
    })))
}
