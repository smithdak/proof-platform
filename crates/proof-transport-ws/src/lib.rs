//! WebSocket transport adapter for the Proof platform.

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
    routing::get,
    Router,
};
use proof_kernel::{
    create_proof, generate_keypair, ExecutionContext, ExecutionEngine, ExecutionError, Keypair,
    OperationHandler, Registry,
};
use proof_storage::SqliteStore;
use serde_json::{json, Value};
use std::{path::PathBuf, sync::Arc};

pub struct WsAppState {
    pub workspace_path: String,
    pub version: String,
    pub registry: Registry,
    pub engine: Arc<std::sync::RwLock<ExecutionEngine>>,
    pub keypair: Keypair,
    pub store: Arc<SqliteStore>,
}

pub type SharedWsState = Arc<WsAppState>;

impl WsAppState {
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
            registry: registry.clone(),
            engine: Arc::new(std::sync::RwLock::new(ExecutionEngine::new(
                registry.clone(),
            ))),
            keypair: generate_keypair(),
            store: Arc::new(store),
        }
    }

    pub fn register_handler(&self, handler: Arc<dyn OperationHandler>) {
        self.engine.write().unwrap().register_handler(handler);
    }
}

pub fn ws_router(state: SharedWsState) -> Router {
    Router::new()
        .route("/ws", get(websocket_handler))
        .with_state(state)
}

async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<SharedWsState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: SharedWsState) {
    while let Some(message) = socket.recv().await {
        let Ok(message) = message else {
            break;
        };

        let Message::Text(payload) = message else {
            let response = json!({
                "error": {"code": -32600, "message": "only text messages are supported"}
            });
            if send_json(&mut socket, response).await.is_err() {
                break;
            }
            continue;
        };

        let response = handle_request(&state, payload).await;
        if send_json(&mut socket, response).await.is_err() {
            break;
        }
    }
}

async fn send_json(socket: &mut WebSocket, value: Value) -> Result<(), axum::Error> {
    let payload = serde_json::to_string(&value).unwrap_or_else(|_| {
        json!({
            "error": {"code": -32603, "message": "failed to serialize response"}
        })
        .to_string()
    });
    socket.send(Message::Text(payload)).await
}

async fn handle_request(state: &SharedWsState, payload: String) -> Value {
    let Ok(request) = serde_json::from_str::<Value>(&payload) else {
        return json_error(-32700, "invalid JSON", None);
    };
    let id = request.get("id").cloned();
    let Some(method) = request
        .get("method")
        .and_then(Value::as_str)
        .map(str::to_string)
    else {
        return json_error(-32600, "missing method", id);
    };

    let empty = Value::Object(serde_json::Map::new());
    let params = request.get("params").unwrap_or(&empty).clone();
    match method.as_str() {
        "execute" => handle_execute(state, params, id).await,
        "list_tools" => handle_list_tools(state, id),
        _ => json_error(-32601, &format!("unknown method: {method}"), id),
    }
}

async fn handle_execute(state: &SharedWsState, params: Value, id: Option<Value>) -> Value {
    let operation = params
        .get("operation")
        .and_then(Value::as_str)
        .map(str::to_string);
    let version = params
        .get("version")
        .and_then(Value::as_str)
        .map(str::to_string);
    let (Some(operation), Some(version)) = (operation, version) else {
        return json_error(-32602, "operation and version are required", id);
    };

    let keypair = state.keypair.clone();
    let timestamp = chrono::Utc::now();
    let context = ExecutionContext {
        actor: keypair.principal_id,
        delegation_id: None,
        delegation_chain: None,
        workspace_path: PathBuf::from(&state.workspace_path),
        timestamp,
    };

    let result = state.engine.read().unwrap().execute(
        &operation,
        &version,
        params.get("input").unwrap_or(&Value::Null),
        &context,
    );

    match result {
        Ok(result) => match create_proof(
            keypair.principal_id,
            context.delegation_id,
            &operation,
            params.get("input").unwrap_or(&Value::Null),
            &result,
            timestamp,
            &keypair,
        ) {
            Ok(proof) => {
                let proof = serde_json::to_value(&proof).unwrap_or(Value::Null);
                json!({
                    "id": id,
                    "result": {
                        "operation": operation,
                        "version": version,
                        "status": "executed",
                        "result": result,
                        "proof": proof
                    }
                })
            }
            Err(error) => json_error(-32603, &error.to_string(), id),
        },
        Err(error) => execution_error(error, id),
    }
}

fn handle_list_tools(state: &SharedWsState, id: Option<Value>) -> Value {
    let registry_json = serde_json::to_value(state.registry.active_operations());
    match registry_json {
        Ok(tools) => json!({
            "id": id,
            "result": {"tools": tools}
        }),
        Err(error) => json_error(-32603, &error.to_string(), id),
    }
}

fn execution_error(error: ExecutionError, id: Option<Value>) -> Value {
    let code = match &error {
        ExecutionError::OperationNotFound { .. } => -32001,
        ExecutionError::HumanOnly
        | ExecutionError::ScopeViolation
        | ExecutionError::Delegation(_) => -32002,
        ExecutionError::Sunset => -32003,
        ExecutionError::BenchmarkExpired { .. } => -32004,
        ExecutionError::NoHandler(_)
        | ExecutionError::HandlerFailed(_)
        | ExecutionError::EvidenceFailed(_)
        | ExecutionError::StorageFailed(_) => -32005,
    };
    json!({
        "id": id,
        "error": {"code": code, "message": error.to_string()}
    })
}

fn json_error(code: i32, message: &str, id: Option<Value>) -> Value {
    json!({
        "id": id,
        "error": {"code": code, "message": message}
    })
}
