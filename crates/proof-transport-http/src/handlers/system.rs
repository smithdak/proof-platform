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

fn read_commerce_records(
    state: &SharedState,
    kind: &str,
) -> Result<Vec<Value>, (axum::http::StatusCode, Json<Value>)> {
    let dir = std::path::Path::new(&state.workspace_path).join(".proof/data/commerce");
    let mut records = vec![];
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.filter_map(|entry| entry.ok()) {
            if let Some(name) = entry.file_name().to_str() {
                if name.starts_with(&format!("{kind}-")) && name.ends_with(".json") {
                    if let Ok(content) = std::fs::read_to_string(entry.path()) {
                        if let Ok(value) = serde_json::from_str::<Value>(&content) {
                            records.push(value);
                        }
                    }
                }
            }
        }
    }
    records.sort_by(|left, right| {
        left["id"]
            .as_str()
            .unwrap_or_default()
            .cmp(right["id"].as_str().unwrap_or_default())
    });
    Ok(records)
}

pub(crate) async fn list_catalog(State(state): State<SharedState>) -> impl IntoResponse {
    read_commerce_records(&state, "catalog").map(|catalogs| Json(json!({ "catalogs": catalogs })))
}

pub(crate) async fn list_orders(State(state): State<SharedState>) -> impl IntoResponse {
    read_commerce_records(&state, "order").map(|orders| Json(json!({ "orders": orders })))
}
