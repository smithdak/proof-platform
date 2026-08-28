//! Health, readiness, and catalog endpoints.

use super::super::state::SharedState;
use axum::{extract::State, response::IntoResponse, Json};
use proof_kernel::VersionStatus;
use serde_json::{json, Value};

pub(crate) async fn root() -> impl IntoResponse {
    Json(json!({
        "name": "proof",
        "description": "Governed agent-native content platform",
        "api_version": "v1"
    }))
}

pub(crate) async fn health() -> impl IntoResponse {
    Json(json!({"status": "ok"}))
}

pub(crate) async fn capabilities(State(state): State<SharedState>) -> impl IntoResponse {
    let engine = state.engine.read().unwrap();
    let operations: Vec<Value> = engine
        .operations()
        .iter()
        .filter(|entry| entry.status == VersionStatus::Active)
        .into_iter()
        .map(|entry| {
            json!({
                "name": entry.operation,
                "version": entry.version,
                "domain": entry.domain,
                "governance": entry.governance,
            })
        })
        .collect();
    Json(json!({ "operations": operations }))
}

pub(crate) async fn list_schemas(State(state): State<SharedState>) -> impl IntoResponse {
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

pub(crate) async fn list_objects(State(state): State<SharedState>) -> impl IntoResponse {
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
