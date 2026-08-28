//! HTTP/REST transport adapter for the Proof platform.

use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use serde_json::{json, Value};
use std::sync::Arc;

pub struct AppState {
    pub version: String,
    pub started_at: String,
}

pub type SharedState = Arc<AppState>;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/", get(root))
        .route("/capabilities", get(capabilities))
        .route("/health", get(health))
        .route("/v1/operations/:name/:version", post(execute_operation))
}

async fn root() -> Json<Value> {
    Json(json!({
        "name": "proof",
        "description": "Governed agent-native content platform",
        "api_version": "v1"
    }))
}

async fn health() -> Json<Value> {
    Json(json!({"status": "ok"}))
}

async fn capabilities() -> Json<Value> {
    Json(json!({
        "operations": [
            {"name": "object.create", "version": "v1", "domain": "content"},
            {"name": "schema.create", "version": "v1", "domain": "content"},
            {"name": "changeset.commit", "version": "v1", "domain": "content"}
        ]
    }))
}

async fn execute_operation(
    State(_state): State<SharedState>,
    axum::extract::Path((name, version)): axum::extract::Path<(String, String)>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, StatusCode> {
    // TODO: route through kernel registry to actual operation handlers
    Ok(Json(json!({
        "operation": name,
        "version": version,
        "status": "received",
        "input": body
    })))
}
