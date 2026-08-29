//! Shared application state for the HTTP transport.

use proof_kernel::generate_keypair;
use proof_kernel::{ExecutionEngine, Keypair, OperationHandler, Registry};
use proof_storage::SqliteStore;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

pub type SharedState = Arc<AppState>;

pub struct AppState {
    pub workspace_path: String,
    pub version: String,
    pub engine: Arc<RwLock<ExecutionEngine>>,
    pub keypair: Keypair,
    pub store: Arc<SqliteStore>,
}

fn commerce_registry_schema(
    workspace_path: &str,
    operation: &str,
) -> Result<serde_json::Value, proof_kernel::ExecutionError> {
    let path = PathBuf::from(workspace_path)
        .join(".proof/registry/commerce")
        .join(format!("{operation}.input.json"));
    let contents = std::fs::read_to_string(path).map_err(|error| {
        proof_kernel::ExecutionError::HandlerFailed(format!(
            "failed to read registry schema: {error}"
        ))
    })?;
    serde_json::from_str(&contents).map_err(|error| {
        proof_kernel::ExecutionError::HandlerFailed(format!("invalid registry schema: {error}"))
    })
}

fn validate_json_schema(
    schema: &serde_json::Value,
    input: &serde_json::Value,
) -> Result<(), proof_kernel::ExecutionError> {
    let object = schema.as_object().ok_or_else(|| {
        proof_kernel::ExecutionError::HandlerFailed("registry schema is invalid".to_string())
    })?;
    let input = input.as_object().ok_or_else(|| {
        proof_kernel::ExecutionError::HandlerFailed("input must be a JSON object".to_string())
    })?;
    if let Some(required) = object.get("required").and_then(|value| value.as_array()) {
        for field in required {
            let Some(field) = field.as_str() else {
                continue;
            };
            if !input.contains_key(field) {
                return Err(proof_kernel::ExecutionError::HandlerFailed(format!(
                    "missing required field: {field}"
                )));
            }
        }
    }
    if object
        .get("additionalProperties")
        .and_then(|value| value.as_bool())
        == Some(false)
    {
        let properties = object
            .get("properties")
            .and_then(|value| value.as_object())
            .cloned()
            .unwrap_or_default();
        for field in input.keys() {
            if !properties.contains_key(field) {
                return Err(proof_kernel::ExecutionError::HandlerFailed(format!(
                    "unknown field: {field}"
                )));
            }
        }
    }
    Ok(())
}

fn execute_commerce_operation(
    operation: &str,
    input: &serde_json::Value,
    context: &proof_kernel::ExecutionContext,
) -> Result<serde_json::Value, proof_kernel::ExecutionError> {
    let workspace = context.workspace_path.join(".proof/data/commerce");
    std::fs::create_dir_all(&workspace).map_err(|error| {
        proof_kernel::ExecutionError::HandlerFailed(format!(
            "failed to create commerce store: {error}"
        ))
    })?;
    let read = |id: &str, kind: &str| -> Result<serde_json::Value, proof_kernel::ExecutionError> {
        let path = workspace.join(format!("{kind}-{id}.json"));
        let contents = std::fs::read_to_string(&path).map_err(|_| {
            proof_kernel::ExecutionError::HandlerFailed(format!("{kind} {id} not found"))
        })?;
        serde_json::from_str(&contents).map_err(|error| {
            proof_kernel::ExecutionError::HandlerFailed(format!("invalid {kind} {id}: {error}"))
        })
    };
    let write = |id: &str,
                 kind: &str,
                 value: &serde_json::Value|
     -> Result<(), proof_kernel::ExecutionError> {
        let path = workspace.join(format!("{kind}-{id}.json"));
        std::fs::write(
            &path,
            serde_json::to_string_pretty(value).unwrap_or_default(),
        )
        .map_err(|error| {
            proof_kernel::ExecutionError::HandlerFailed(format!("failed to save {kind}: {error}"))
        })
    };
    let mut record = input.clone();
    record["id"] = serde_json::json!(uuid::Uuid::now_v7().to_string());
    match operation {
        "catalog.create" => {
            record["status"] = serde_json::json!("active");
            record["created_at"] = serde_json::json!(context.timestamp.to_rfc3339());
            write(record["id"].as_str().unwrap(), "catalog", &record)?;
            Ok(serde_json::json!({"operation": operation, "data": {"catalog_id": record["id"]}}))
        }
        "catalog.update" => {
            let id = input["catalog_id"].as_str().unwrap_or_default().to_string();
            let mut catalog = read(&id, "catalog")?;
            if let Some(fields) = input.as_object() {
                for (key, value) in fields {
                    if key != "catalog_id" {
                        catalog[key] = value.clone();
                    }
                }
            }
            catalog["updated_at"] = serde_json::json!(context.timestamp.to_rfc3339());
            write(&id, "catalog", &catalog)?;
            Ok(serde_json::json!({"operation": operation, "data": {"catalog": catalog}}))
        }
        "order.create" => {
            record["status"] = serde_json::json!("pending_approval");
            record["created_at"] = serde_json::json!(context.timestamp.to_rfc3339());
            write(record["id"].as_str().unwrap(), "order", &record)?;
            Ok(serde_json::json!({"operation": operation, "data": {"order_id": record["id"]}}))
        }
        "order.approve" => {
            let id = input["order_id"].as_str().unwrap_or_default().to_string();
            let mut order = read(&id, "order")?;
            if order["status"] == "fulfilled" || order["status"] == "approved" {
                return Err(proof_kernel::ExecutionError::HandlerFailed(format!(
                    "order {id} is already {}",
                    order["status"]
                )));
            }
            order["status"] = serde_json::json!("approved");
            order["approved_by"] = serde_json::json!(context.actor.to_string());
            order["approved_at"] = serde_json::json!(context.timestamp.to_rfc3339());
            write(&id, "order", &order)?;
            Ok(
                serde_json::json!({"operation": operation, "data": {"status": "approved", "order": order}}),
            )
        }
        "order.fulfill" => {
            let id = input["order_id"].as_str().unwrap_or_default().to_string();
            let mut order = read(&id, "order")?;
            if order["status"] != "approved" {
                return Err(proof_kernel::ExecutionError::HandlerFailed(format!(
                    "order {id} is {}; only approved orders can be fulfilled",
                    order["status"]
                )));
            }
            order["status"] = serde_json::json!("fulfilled");
            order["fulfilled_at"] = serde_json::json!(context.timestamp.to_rfc3339());
            write(&id, "order", &order)?;
            Ok(
                serde_json::json!({"operation": operation, "data": {"status": "fulfilled", "order": order}}),
            )
        }
        _ => Err(proof_kernel::ExecutionError::NoHandler(
            operation.to_string(),
        )),
    }
}

struct CommerceHandler {
    operation: &'static str,
}

impl proof_kernel::OperationHandler for CommerceHandler {
    fn operation(&self) -> &str {
        self.operation
    }

    fn execute(
        &self,
        input: &serde_json::Value,
        context: &proof_kernel::ExecutionContext,
    ) -> Result<serde_json::Value, proof_kernel::ExecutionError> {
        let schema =
            commerce_registry_schema(&context.workspace_path.to_string_lossy(), self.operation)?;
        validate_json_schema(&schema, input)?;
        execute_commerce_operation(self.operation, input, context)
    }
}

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
        let mut engine = ExecutionEngine::new(registry);
        for operation in [
            "catalog.create",
            "catalog.update",
            "order.create",
            "order.approve",
            "order.fulfill",
        ] {
            engine.register_handler(Arc::new(CommerceHandler { operation }));
        }
        Self {
            workspace_path: workspace_path.into(),
            version: "0.1.0".to_string(),
            engine: Arc::new(RwLock::new(engine)),
            keypair: generate_keypair(),
            store: Arc::new(store),
        }
    }

    pub fn register_handler(&self, handler: Arc<dyn OperationHandler>) {
        self.engine.write().unwrap().register_handler(handler);
    }
}
