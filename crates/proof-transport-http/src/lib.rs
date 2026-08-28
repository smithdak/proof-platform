//! HTTP/REST transport adapter for the Proof platform.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{get, post},
    Router,
};
use proof_kernel::{
    create_proof, generate_keypair, ExecutionContext, ExecutionEngine, ExecutionError, Keypair,
    OperationHandler, Registry,
};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{Arc, RwLock},
};

pub struct AppState {
    pub workspace_path: String,
    pub version: String,
    pub engine: Arc<RwLock<ExecutionEngine>>,
    pub keypair: Keypair,
}

pub type SharedState = Arc<AppState>;

impl AppState {
    pub fn new(workspace_path: impl Into<String>) -> Result<Self, proof_kernel::RegistryError> {
        let workspace_path = workspace_path.into();
        let registry =
            Registry::load_from_directory(PathBuf::from(&workspace_path).join(".proof/registry"))?;
        Ok(Self::with_registry(workspace_path, registry))
    }

    pub fn with_registry(workspace_path: impl Into<String>, registry: Registry) -> Self {
        Self {
            workspace_path: workspace_path.into(),
            version: "0.1.0".to_string(),
            engine: Arc::new(RwLock::new(ExecutionEngine::new(registry))),
            keypair: generate_keypair(),
        }
    }

    pub fn register_handler(&self, handler: Arc<dyn OperationHandler>) {
        self.engine.write().unwrap().register_handler(handler);
    }
}

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
    State(state): State<SharedState>,
    Path((name, version)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let keypair = state.keypair.clone();
    let context = ExecutionContext {
        actor: keypair.principal_id,
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

fn execution_error_response(error: &ExecutionError) -> (StatusCode, Json<Value>) {
    let status = match error {
        ExecutionError::OperationNotFound { .. } => StatusCode::NOT_FOUND,
        ExecutionError::HumanOnly => StatusCode::FORBIDDEN,
        ExecutionError::NoHandler(_)
        | ExecutionError::HandlerFailed(_)
        | ExecutionError::EvidenceFailed(_)
        | ExecutionError::Delegation(_)
        | ExecutionError::StorageFailed(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, Json(json!({"error": error.to_string()})))
}

fn internal_error(error: String) -> (StatusCode, Json<Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": error })),
    )
}
