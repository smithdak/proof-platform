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

fn store_error(error: proof_storage::StorageError) -> (axum::http::StatusCode, Json<Value>) {
    (
        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({"error": error.to_string()})),
    )
}

pub(crate) async fn list_catalog(State(state): State<SharedState>) -> impl IntoResponse {
    match state.store.list_catalogs() {
        Ok(items) => {
            let values: Vec<Value> = items
                .iter()
                .map(|item| serde_json::to_value(item).unwrap_or_default())
                .collect();
            Json(json!({ "catalogs": values }))
        }
        Err(error) => store_error(error).1,
    }
}

pub(crate) async fn list_orders(State(state): State<SharedState>) -> impl IntoResponse {
    match state.store.list_orders() {
        Ok(items) => {
            let values: Vec<Value> = items
                .iter()
                .map(|item| serde_json::to_value(item).unwrap_or_default())
                .collect();
            Json(json!({ "orders": values }))
        }
        Err(error) => store_error(error).1,
    }
}

pub(crate) async fn list_workflows(State(state): State<SharedState>) -> impl IntoResponse {
    match state.store.list_workflow_definitions() {
        Ok(items) => {
            let values: Vec<Value> = items
                .iter()
                .map(|item| serde_json::to_value(item).unwrap_or_default())
                .collect();
            Json(json!({ "workflows": values }))
        }
        Err(error) => store_error(error).1,
    }
}

pub(crate) async fn list_workflow_runs(State(state): State<SharedState>) -> impl IntoResponse {
    match state.store.list_workflow_runs(None) {
        Ok(items) => {
            let values: Vec<Value> = items
                .iter()
                .map(|item| serde_json::to_value(item).unwrap_or_default())
                .collect();
            Json(json!({ "workflow_runs": values }))
        }
        Err(error) => store_error(error).1,
    }
}

pub(crate) async fn list_analytics_snapshots(
    State(state): State<SharedState>,
) -> impl IntoResponse {
    match state.store.list_analytics_snapshots() {
        Ok(items) => {
            let values: Vec<Value> = items
                .iter()
                .map(|item| serde_json::to_value(item).unwrap_or_default())
                .collect();
            Json(json!({ "snapshots": values }))
        }
        Err(error) => store_error(error).1,
    }
}

pub(crate) async fn list_analytics_queries(State(state): State<SharedState>) -> impl IntoResponse {
    match state.store.list_all_analytics_queries() {
        Ok(items) => {
            let values: Vec<Value> = items
                .iter()
                .map(|item| serde_json::to_value(item).unwrap_or_default())
                .collect();
            Json(json!({ "queries": values }))
        }
        Err(error) => store_error(error).1,
    }
}
