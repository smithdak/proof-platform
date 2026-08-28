//! HTTP/REST transport adapter for the Proof platform.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{get, post},
    Router,
};
use proof_kernel::{
    create_proof, generate_keypair, principal_from_keypair, ExecutionContext, ExecutionEngine,
    ExecutionError, Keypair, OperationHandler, Proof, Registry,
};
use proof_storage::SqliteStore;
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{Arc, RwLock},
};
use uuid::Uuid;

pub struct AppState {
    pub workspace_path: String,
    pub version: String,
    pub engine: Arc<RwLock<ExecutionEngine>>,
    pub keypair: Keypair,
    pub store: Arc<SqliteStore>,
}

pub type SharedState = Arc<AppState>;

impl AppState {
    pub fn new(workspace_path: impl Into<String>) -> Result<Self, proof_kernel::RegistryError> {
        let workspace_path = workspace_path.into();
        let registry =
            Registry::load_from_directory(PathBuf::from(&workspace_path).join(".proof/registry"))?;
        let database_path =
            PathBuf::from(&workspace_path).join(".proof/data/proofs/proofs.sqlite3");
        if let Some(parent) = database_path.parent() {
            std::fs::create_dir_all(parent).map_err(proof_kernel::RegistryError::Io)?;
        }
        let store = SqliteStore::open(&database_path).map_err(|error| {
            proof_kernel::RegistryError::Io(std::io::Error::other(error.to_string()))
        })?;
        Ok(Self::with_registry_and_store(
            workspace_path,
            registry,
            store,
        ))
    }

    pub fn with_registry(workspace_path: impl Into<String>, registry: Registry) -> Self {
        Self::with_registry_and_store(
            workspace_path,
            registry,
            SqliteStore::in_memory().expect("in-memory SQLite should initialize"),
        )
    }

    pub fn with_registry_and_store(
        workspace_path: impl Into<String>,
        registry: Registry,
        store: SqliteStore,
    ) -> Self {
        Self {
            workspace_path: workspace_path.into(),
            version: "0.1.0".to_string(),
            engine: Arc::new(RwLock::new(ExecutionEngine::new(registry))),
            keypair: generate_keypair(),
            store: Arc::new(store),
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
        .route("/proofs/:id", get(get_proof))
        .route("/proofs", get(list_proofs_filtered))
        .route("/proofs/verify", post(verify_proof))
        .route("/audit", get(list_audit))
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

#[derive(Default, serde::Deserialize)]
struct ProofFilters {
    operation: Option<String>,
    version: Option<String>,
    actor: Option<Uuid>,
}

async fn list_proofs(
    State(state): State<SharedState>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    list_proofs_inner(&state, ProofFilters::default()).await
}

async fn list_proofs_filtered(
    State(state): State<SharedState>,
    Query(filters): Query<ProofFilters>,
) -> impl IntoResponse {
    list_proofs_inner(&state, filters).await
}

async fn list_proofs_inner(
    state: &SharedState,
    filters: ProofFilters,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let connection = state.store.connection();
    let mut sql = "
        SELECT signature, operation, actor
        FROM proofs
        WHERE (?1 IS NULL OR operation = ?1)
          AND (?2 IS NULL OR actor = ?2)
        ORDER BY timestamp DESC
    "
    .to_string();
    if filters.operation.is_some() && filters.version.is_some() {
        sql.push_str(" LIMIT 0");
    }
    let mut statement = connection
        .prepare(&sql)
        .map_err(|error| internal_error(error.to_string()))?;
    let serialized_proofs = statement
        .query_map(
            rusqlite::params![
                filters.operation,
                filters.actor.map(|actor| actor.to_string()),
            ],
            |row| row.get::<_, String>(0),
        )
        .map_err(|error| internal_error(error.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| internal_error(error.to_string()))?;
    let proofs = serialized_proofs
        .iter()
        .map(|serialized| serde_json::from_str::<Proof>(serialized))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| internal_error(error.to_string()))?;
    Ok(Json(json!({ "proofs": proofs })))
}

async fn get_proof(
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
    Ok(Json(json!({
        "proof": proof,
        "verification": verification_status(&proof),
    })))
}

#[derive(serde::Deserialize)]
struct VerifyProofRequest {
    proof_id: Uuid,
}

async fn verify_proof(
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

async fn list_audit(
    State(state): State<SharedState>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let connection = state.store.connection();
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
    Ok(Json(json!({ "contexts": audit })))
}

fn verification_status(proof: &Proof) -> &'static str {
    match proof {
        Proof { .. } => "unverified",
    }
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
