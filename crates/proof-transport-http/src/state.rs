//! Shared application state for the HTTP transport.

use proof_kernel::generate_keypair;
use proof_kernel::{ExecutionEngine, Keypair, OperationHandler, Registry};
use proof_storage::{SqliteStore, StorageError};
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

fn registry_schema(
    domain: &str,
    workspace_path: &str,
    operation: &str,
) -> Result<serde_json::Value, proof_kernel::ExecutionError> {
    let file_name = operation.replace('.', "-");
    let path = PathBuf::from(workspace_path)
        .join(".proof/registry")
        .join(domain)
        .join(format!("{file_name}.input.json"));
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
    store: &proof_storage::SqliteStore,
) -> Result<serde_json::Value, proof_kernel::ExecutionError> {
    use proof_storage::{Catalog, Order, OrderLine, OrderStatus};

    let not_found = |id: &str, kind: &str| {
        proof_kernel::ExecutionError::HandlerFailed(format!("{kind} {id} not found"))
    };
    let map_store_error = |error: proof_storage::StorageError| {
        proof_kernel::ExecutionError::HandlerFailed(error.to_string())
    };
    match operation {
        "catalog.create" => {
            let catalog = Catalog {
                id: uuid::Uuid::now_v7(),
                name: input["name"].as_str().unwrap_or_default().to_string(),
                description: input["description"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                created_at: context.timestamp,
                updated_at: context.timestamp,
            };
            store.save_catalog(&catalog).map_err(map_store_error)?;
            Ok(
                serde_json::json!({"operation": operation, "data": {"catalog_id": catalog.id.to_string()}}),
            )
        }
        "catalog.update" => {
            let id = input["catalog_id"].as_str().unwrap_or_default().to_string();
            let catalog_id = uuid::Uuid::parse_str(&id).map_err(|_| not_found(&id, "catalog"))?;
            let mut catalog = store
                .load_catalog(&catalog_id)
                .map_err(|_| not_found(&id, "catalog"))?;
            if let Some(name) = input["name"].as_str() {
                catalog.name = name.to_string();
            }
            if let Some(description) = input["description"].as_str() {
                catalog.description = description.to_string();
            }
            catalog.updated_at = context.timestamp;
            store.save_catalog(&catalog).map_err(map_store_error)?;
            Ok(
                serde_json::json!({"operation": operation, "data": {"catalog": serde_json::to_value(&catalog).unwrap_or_default()}}),
            )
        }
        "order.create" => {
            let catalog_id = uuid::Uuid::parse_str(
                input["catalog_id"].as_str().unwrap_or_default(),
            )
            .map_err(|_| not_found(input["catalog_id"].as_str().unwrap_or_default(), "catalog"))?;
            let order = Order {
                id: uuid::Uuid::now_v7(),
                status: OrderStatus::Pending,
                created_at: context.timestamp,
                approved_at: None,
                fulfilled_at: None,
                lines: vec![OrderLine {
                    catalog_id,
                    name: input["catalog_id"].as_str().unwrap_or_default().to_string(),
                    quantity: 1,
                }],
            };
            store.save_order(&order).map_err(map_store_error)?;
            Ok(
                serde_json::json!({"operation": operation, "data": {"order_id": order.id.to_string()}}),
            )
        }
        "order.approve" => {
            let id = input["order_id"].as_str().unwrap_or_default().to_string();
            let order_id = uuid::Uuid::parse_str(&id).map_err(|_| not_found(&id, "order"))?;
            let mut order = store
                .load_order(&order_id)
                .map_err(|_| not_found(&id, "order"))?;
            if order.status == OrderStatus::Approved || order.status == OrderStatus::Fulfilled {
                return Err(proof_kernel::ExecutionError::HandlerFailed(format!(
                    "order {id} is already {}",
                    order.status.as_str()
                )));
            }
            order.status = OrderStatus::Approved;
            order.approved_at = Some(context.timestamp);
            store.save_order(&order).map_err(map_store_error)?;
            Ok(
                serde_json::json!({"operation": operation, "data": {"status": "approved", "order": serde_json::to_value(&order).unwrap_or_default()}}),
            )
        }
        "order.fulfill" => {
            let id = input["order_id"].as_str().unwrap_or_default().to_string();
            let order_id = uuid::Uuid::parse_str(&id).map_err(|_| not_found(&id, "order"))?;
            let mut order = store
                .load_order(&order_id)
                .map_err(|_| not_found(&id, "order"))?;
            if order.status != OrderStatus::Approved {
                return Err(proof_kernel::ExecutionError::HandlerFailed(format!(
                    "order {id} is {}; only approved orders can be fulfilled",
                    order.status.as_str()
                )));
            }
            order.status = OrderStatus::Fulfilled;
            order.fulfilled_at = Some(context.timestamp);
            store.save_order(&order).map_err(map_store_error)?;
            Ok(
                serde_json::json!({"operation": operation, "data": {"status": "fulfilled", "order": serde_json::to_value(&order).unwrap_or_default()}}),
            )
        }
        _ => Err(proof_kernel::ExecutionError::NoHandler(
            operation.to_string(),
        )),
    }
}

fn execute_workflow_operation(
    operation: &str,
    input: &serde_json::Value,
    context: &proof_kernel::ExecutionContext,
    store: &proof_storage::SqliteStore,
) -> Result<serde_json::Value, proof_kernel::ExecutionError> {
    use proof_storage::{
        WorkflowDefinition, WorkflowRun, WorkflowRunStatus, WorkflowStep, WorkflowStepKind,
        WorkflowStepStatus, WorkflowStepTemplate,
    };

    let not_found = |id: &str, kind: &str| {
        proof_kernel::ExecutionError::HandlerFailed(format!("{kind} {id} not found"))
    };
    let map_store_error = |error: proof_storage::StorageError| {
        proof_kernel::ExecutionError::HandlerFailed(error.to_string())
    };
    let parse_id =
        |id: &str, kind: &str| uuid::Uuid::parse_str(id).map_err(|_| not_found(id, kind));

    match operation {
        "workflow.define" => {
            let steps: Vec<WorkflowStepTemplate> = input["steps"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or(&[])
                .iter()
                .map(|step| WorkflowStepTemplate {
                    name: step["name"].as_str().unwrap_or_default().to_string(),
                    kind: if step["kind"].as_str() == Some("human") {
                        WorkflowStepKind::Human
                    } else {
                        WorkflowStepKind::Agent
                    },
                    description: step["description"].as_str().unwrap_or_default().to_string(),
                })
                .collect();
            let definition = WorkflowDefinition {
                id: uuid::Uuid::now_v7(),
                name: input["name"].as_str().unwrap_or_default().to_string(),
                description: input["description"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                steps,
                created_at: context.timestamp,
                updated_at: context.timestamp,
            };
            store
                .save_workflow_definition(&definition)
                .map_err(map_store_error)?;
            Ok(
                serde_json::json!({"operation": operation, "data": {"workflow_id": definition.id.to_string()}}),
            )
        }
        "workflow.trigger" => {
            let workflow_id = parse_id(
                input["workflow_id"].as_str().unwrap_or_default(),
                "workflow",
            )?;
            let definition = store
                .load_workflow_definition(&workflow_id)
                .map_err(|_| not_found(&workflow_id.to_string(), "workflow"))?;
            let run = WorkflowRun {
                id: uuid::Uuid::now_v7(),
                workflow_definition_id: definition.id,
                status: WorkflowRunStatus::InProgress,
                created_at: context.timestamp,
                completed_at: None,
                approved_at: None,
            };
            store.save_workflow_run(&run).map_err(map_store_error)?;
            for (ordinal, template) in definition.steps.iter().enumerate() {
                store
                    .save_workflow_step(&WorkflowStep {
                        id: uuid::Uuid::now_v7(),
                        run_id: run.id,
                        name: template.name.clone(),
                        kind: template.kind,
                        description: template.description.clone(),
                        status: WorkflowStepStatus::Pending,
                        ordinal: ordinal as u32,
                        completed_at: None,
                    })
                    .map_err(map_store_error)?;
            }
            Ok(serde_json::json!({"operation": operation, "data": {"run_id": run.id.to_string()}}))
        }
        "workflow.step.complete" => {
            let run_id = input["run_id"].as_str().unwrap_or_default();
            let run_id = parse_id(run_id, "workflow run")?;
            store
                .load_workflow_run(&run_id)
                .map_err(|_| not_found(&run_id.to_string(), "workflow run"))?;
            let mut steps = store
                .list_workflow_steps(&run_id)
                .map_err(map_store_error)?;
            let Some(step) = steps
                .iter_mut()
                .find(|step| step.status == WorkflowStepStatus::Pending)
            else {
                return Err(proof_kernel::ExecutionError::HandlerFailed(format!(
                    "workflow run {run_id} has no pending steps"
                )));
            };
            let step_name = step.name.clone();
            let mut completed_step = step.clone();
            completed_step.status = WorkflowStepStatus::Completed;
            completed_step.completed_at = Some(context.timestamp);
            store
                .save_workflow_step(&completed_step)
                .map_err(map_store_error)?;
            let all_complete = steps
                .iter()
                .all(|step| step.status == WorkflowStepStatus::Completed);
            if all_complete {
                let mut run = store.load_workflow_run(&run_id).map_err(map_store_error)?;
                run.completed_at = Some(context.timestamp);
                store.save_workflow_run(&run).map_err(map_store_error)?;
            }
            Ok(serde_json::json!({
                "operation": operation,
                "data": {
                    "run_id": run_id,
                    "completed_step": step_name,
                }
            }))
        }
        "workflow.approve" => {
            let run_id = input["run_id"].as_str().unwrap_or_default();
            let run_id = parse_id(run_id, "workflow run")?;
            let mut run = store
                .load_workflow_run(&run_id)
                .map_err(|_| not_found(&run_id.to_string(), "workflow run"))?;
            if run.status == WorkflowRunStatus::Approved {
                return Err(proof_kernel::ExecutionError::HandlerFailed(format!(
                    "workflow run {run_id} is already approved"
                )));
            }
            let steps = store
                .list_workflow_steps(&run_id)
                .map_err(map_store_error)?;
            let all_complete = steps
                .iter()
                .all(|step| step.status == WorkflowStepStatus::Completed);
            if !all_complete {
                return Err(proof_kernel::ExecutionError::HandlerFailed(format!(
                    "workflow run {run_id} has incomplete steps; complete all steps before approval"
                )));
            }
            run.status = WorkflowRunStatus::Approved;
            run.approved_at = Some(context.timestamp);
            store.save_workflow_run(&run).map_err(map_store_error)?;
            Ok(serde_json::json!({
                "operation": operation,
                "data": {"run_id": run_id, "status": "approved"}
            }))
        }
        _ => Err(proof_kernel::ExecutionError::NoHandler(
            operation.to_string(),
        )),
    }
}

fn execute_analytics_operation(
    operation: &str,
    input: &serde_json::Value,
    context: &proof_kernel::ExecutionContext,
    store: &proof_storage::SqliteStore,
) -> Result<serde_json::Value, proof_kernel::ExecutionError> {
    use proof_storage::{
        AnalyticsInsight, AnalyticsInsightStatus, AnalyticsQuery, AnalyticsSnapshot,
    };

    let not_found = |id: &str, kind: &str| {
        proof_kernel::ExecutionError::HandlerFailed(format!("{kind} {id} not found"))
    };
    let map_store_error = |error: proof_storage::StorageError| {
        proof_kernel::ExecutionError::HandlerFailed(error.to_string())
    };
    let parse_id =
        |id: &str, kind: &str| uuid::Uuid::parse_str(id).map_err(|_| not_found(id, kind));
    let snapshot_digest = |snapshot: &AnalyticsSnapshot| format!("snapshot-{}-digest", snapshot.id);

    match operation {
        "analytics.snapshot.create" => {
            let snapshot = AnalyticsSnapshot {
                id: uuid::Uuid::now_v7(),
                name: input["name"]
                    .as_str()
                    .unwrap_or_else(|| "unnamed snapshot")
                    .to_string(),
                description: input["description"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                digest: String::new(),
                created_at: context.timestamp,
            };
            let mut snapshot = snapshot;
            snapshot.digest = snapshot_digest(&snapshot);
            store
                .save_analytics_snapshot(&snapshot)
                .map_err(map_store_error)?;
            Ok(
                serde_json::json!({"operation": operation, "data": {"snapshot_id": snapshot.id.to_string()}}),
            )
        }
        "analytics.query.create" => {
            let snapshot_id = parse_id(
                input["snapshot_id"].as_str().unwrap_or_default(),
                "snapshot",
            )?;
            store
                .load_analytics_snapshot(&snapshot_id)
                .map_err(|_| not_found(&snapshot_id.to_string(), "snapshot"))?;
            let now = context.timestamp;
            let query = AnalyticsQuery {
                id: uuid::Uuid::now_v7(),
                snapshot_id,
                name: input["name"]
                    .as_str()
                    .unwrap_or_else(|| "unnamed query")
                    .to_string(),
                filter: input["filter"].clone(),
                aggregation: input["aggregation"].clone(),
                created_at: now,
                updated_at: now,
            };
            store
                .save_analytics_query(&query)
                .map_err(map_store_error)?;
            Ok(
                serde_json::json!({"operation": operation, "data": {"query_id": query.id.to_string()}}),
            )
        }
        "analytics.query.execute" => {
            let query_id = parse_id(input["query_id"].as_str().unwrap_or_default(), "query")?;
            let query = store
                .load_analytics_query(&query_id)
                .map_err(|_| not_found(&query_id.to_string(), "query"))?;
            let snapshot = store
                .load_analytics_snapshot(&query.snapshot_id)
                .map_err(|_| not_found(&query.snapshot_id.to_string(), "snapshot"))?;
            let insight = AnalyticsInsight {
                id: uuid::Uuid::now_v7(),
                query_id: query.id,
                result_digest: snapshot_digest(&snapshot),
                status: AnalyticsInsightStatus::Pending,
                approved_at: None,
                approved_by: None,
            };
            store
                .save_analytics_insight(&insight)
                .map_err(map_store_error)?;
            Ok(
                serde_json::json!({"operation": operation, "data": {"insight_id": insight.id.to_string()}}),
            )
        }
        "analytics.insight.approve" => {
            let insight_id = parse_id(input["result_id"].as_str().unwrap_or_default(), "insight")?;
            let mut insight = store
                .load_analytics_insight(&insight_id)
                .map_err(|_| not_found(&insight_id.to_string(), "insight"))?;
            if insight.status == AnalyticsInsightStatus::Approved {
                return Err(proof_kernel::ExecutionError::HandlerFailed(format!(
                    "analytics insight {insight_id} is already approved"
                )));
            }
            insight.status = AnalyticsInsightStatus::Approved;
            insight.approved_at = Some(context.timestamp);
            insight.approved_by = Some(context.actor.to_string());
            store
                .save_analytics_insight(&insight)
                .map_err(map_store_error)?;
            Ok(
                serde_json::json!({"operation": operation, "data": {"insight_id": insight.id.to_string(), "status": "approved"}}),
            )
        }
        _ => Err(proof_kernel::ExecutionError::NoHandler(
            operation.to_string(),
        )),
    }
}

struct CommerceHandler {
    operation: &'static str,
    store: Arc<SqliteStore>,
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
        let schema = registry_schema(
            "commerce",
            &context.workspace_path.to_string_lossy(),
            self.operation,
        )?;
        validate_json_schema(&schema, input)?;
        execute_commerce_operation(self.operation, input, context, &self.store)
    }
}

struct WorkflowHandler {
    operation: &'static str,
    store: Arc<SqliteStore>,
}

impl proof_kernel::OperationHandler for WorkflowHandler {
    fn operation(&self) -> &str {
        self.operation
    }

    fn execute(
        &self,
        input: &serde_json::Value,
        context: &proof_kernel::ExecutionContext,
    ) -> Result<serde_json::Value, proof_kernel::ExecutionError> {
        let schema = registry_schema(
            "workflow",
            &context.workspace_path.to_string_lossy(),
            self.operation,
        )?;
        validate_json_schema(&schema, input)?;
        execute_workflow_operation(self.operation, input, context, &self.store)
    }
}

struct AnalyticsHandler {
    operation: &'static str,
    store: Arc<SqliteStore>,
}

impl proof_kernel::OperationHandler for AnalyticsHandler {
    fn operation(&self) -> &str {
        self.operation
    }

    fn execute(
        &self,
        input: &serde_json::Value,
        context: &proof_kernel::ExecutionContext,
    ) -> Result<serde_json::Value, proof_kernel::ExecutionError> {
        let schema = registry_schema(
            "analytics",
            &context.workspace_path.to_string_lossy(),
            self.operation,
        )?;
        validate_json_schema(&schema, input)?;
        execute_analytics_operation(self.operation, input, context, &self.store)
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
        let shared_store = Arc::new(store);
        let mut engine = ExecutionEngine::new(registry);
        for operation in [
            "catalog.create",
            "catalog.update",
            "order.create",
            "order.approve",
            "order.fulfill",
        ] {
            engine.register_handler(Arc::new(CommerceHandler {
                operation,
                store: shared_store.clone(),
            }));
        }
        for operation in [
            "workflow.define",
            "workflow.trigger",
            "workflow.step.complete",
            "workflow.approve",
        ] {
            engine.register_handler(Arc::new(WorkflowHandler {
                operation,
                store: shared_store.clone(),
            }));
        }
        for operation in [
            "analytics.snapshot.create",
            "analytics.query.create",
            "analytics.query.execute",
            "analytics.insight.approve",
        ] {
            engine.register_handler(Arc::new(AnalyticsHandler {
                operation,
                store: shared_store.clone(),
            }));
        }
        Self {
            workspace_path: workspace_path.into(),
            version: "0.1.0".to_string(),
            engine: Arc::new(RwLock::new(engine)),
            keypair: generate_keypair(),
            store: shared_store,
        }
    }

    pub fn register_handler(&self, handler: Arc<dyn OperationHandler>) {
        self.engine.write().unwrap().register_handler(handler);
    }
}
