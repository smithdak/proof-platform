use std::sync::Arc;

use proof_kernel::{ExecutionContext, ExecutionError, OperationHandler};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use uuid::Uuid;

use crate::{
    digest::canonical_digest,
    models::{Catalog, Order, OrderLine, OrderStatus},
};

const CATALOG_CREATE: &str = "catalog.create";
const CATALOG_UPDATE: &str = "catalog.update";
const ORDER_CREATE: &str = "order.create";
const ORDER_APPROVE: &str = "order.approve";
const ORDER_FULFILL: &str = "order.fulfill";

#[derive(Debug, Deserialize)]
struct JsonSchema {
    #[serde(default)]
    required: Vec<String>,
    #[serde(default)]
    properties: Map<String, Value>,
    #[serde(default)]
    additional_properties: bool,
}

impl JsonSchema {
    fn parse(schema: Value) -> Result<Self, ExecutionError> {
        serde_json::from_value(schema).map_err(|error| {
            ExecutionError::HandlerFailed(format!("registry schema is invalid: {error}"))
        })
    }

    fn validate(&self, input: &Value) -> Result<(), ExecutionError> {
        let Some(input) = input.as_object() else {
            return Err(input_error("input must be a JSON object"));
        };
        for key in self.required.iter().filter(|key| !input.contains_key(*key)) {
            return Err(input_error(format!("missing required field: {key}")));
        }
        for key in input.keys() {
            if !self.properties.contains_key(key) && !self.additional_properties {
                return Err(input_error(format!("unknown field: {key}")));
            }
        }
        Ok(())
    }
}

fn input_error(message: impl Into<String>) -> ExecutionError {
    ExecutionError::HandlerFailed(message.into())
}

fn registry_schema(
    context: &ExecutionContext,
    operation: &str,
    file_name: &str,
) -> Result<Value, ExecutionError> {
    let candidates = [
        context.workspace_path.join(file_name),
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("registry")
            .join(file_name),
        context
            .workspace_path
            .join("crates/proof-commerce")
            .join(file_name),
        context.workspace_path.join("schemas").join(file_name),
        context.workspace_path.join("registry").join(file_name),
    ];
    let registry_path = candidates
        .iter()
        .find(|path| path.exists())
        .unwrap_or(&candidates[0])
        .clone();
    let contents = std::fs::read_to_string(registry_path).map_err(|error| {
        ExecutionError::HandlerFailed(format!("failed to read registry schema: {error}"))
    })?;
    serde_json::from_str(&contents).map_err(|error| {
        ExecutionError::HandlerFailed(format!("invalid registry schema for {operation}: {error}"))
    })
}

#[derive(Debug, Serialize)]
struct HandlerResult<T> {
    operation: &'static str,
    data: T,
}

trait ToHandlerResult {
    fn result(self, operation: &'static str) -> Result<Value, ExecutionError>;
}

impl<T: Serialize> ToHandlerResult for Result<T, ExecutionError> {
    fn result(self, operation: &'static str) -> Result<Value, ExecutionError> {
        self.map(|data| {
            serde_json::to_value(HandlerResult { operation, data })
                .map_err(|error| ExecutionError::HandlerFailed(error.to_string()))
        })?
    }
}

struct GenericCommerceHandler {
    operation: &'static str,
    schema_file: &'static str,
    execute_fn: fn(&Value, &ExecutionContext) -> Result<Value, ExecutionError>,
}

impl OperationHandler for GenericCommerceHandler {
    fn operation(&self) -> &str {
        self.operation
    }

    fn execute(&self, input: &Value, context: &ExecutionContext) -> Result<Value, ExecutionError> {
        let raw_schema = registry_schema(context, self.operation, self.schema_file)?;
        JsonSchema::parse(raw_schema)?.validate(input)?;
        (self.execute_fn)(input, context).result(self.operation)
    }
}

fn catalog_store_dir(context: &ExecutionContext) -> std::path::PathBuf {
    context.workspace_path.join(".proof/data/commerce/catalogs")
}

fn order_store_dir(context: &ExecutionContext) -> std::path::PathBuf {
    context.workspace_path.join(".proof/data/commerce/orders")
}

fn save<T: Serialize>(
    context: &ExecutionContext,
    dir: std::path::PathBuf,
    id: Uuid,
    value: &T,
) -> Result<(), ExecutionError> {
    std::fs::create_dir_all(&dir).map_err(|error| {
        ExecutionError::HandlerFailed(format!("failed to create commerce store: {error}"))
    })?;
    let path = dir.join(format!("{id}.json"));
    let serialized = serde_json::to_string_pretty(value)
        .map_err(|error| ExecutionError::HandlerFailed(error.to_string()))?;
    std::fs::write(&path, serialized)
        .map_err(|error| ExecutionError::HandlerFailed(format!("failed to save record: {error}")))
}

fn load<T: for<'de> Deserialize<'de>>(
    context: &ExecutionContext,
    dir: std::path::PathBuf,
    id: Uuid,
    kind: &'static str,
) -> Result<T, ExecutionError> {
    let path = dir.join(format!("{id}.json"));
    let contents = std::fs::read_to_string(&path).map_err(|error| {
        ExecutionError::HandlerFailed(format!("failed to load {kind} {id}: {error}"))
    })?;
    serde_json::from_str(&contents).map_err(|error| {
        ExecutionError::HandlerFailed(format!("invalid {kind} file for {id}: {error}"))
    })
}

fn parse_id(input: &Value, key: &str) -> Result<Uuid, ExecutionError> {
    let value = input
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| input_error(format!("missing required field: {key}")))?;
    Uuid::parse_str(value).map_err(|error| input_error(format!("invalid {key}: {error}")))
}

fn execute_catalog_create(
    input: &Value,
    context: &ExecutionContext,
) -> Result<Value, ExecutionError> {
    let name = input
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| input_error("missing required field: name"))?;
    let description = input
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let catalog = Catalog::new(name, description).map_err(input_error)?;
    save(context, catalog_store_dir(context), catalog.id, &catalog)?;
    let output = json!({
        "catalog_id": catalog.id,
        "name": catalog.name,
        "description": catalog.description,
        "created_at": catalog.created_at,
        "content_digest": canonical_digest(&catalog),
    });
    Ok(output)
}

fn execute_catalog_update(
    input: &Value,
    context: &ExecutionContext,
) -> Result<Value, ExecutionError> {
    let catalog_id = parse_id(input, "catalog_id")?;
    let mut catalog = load::<Catalog>(context, catalog_store_dir(context), catalog_id, "catalog")?;
    catalog
        .update(
            input
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_string),
            input
                .get("description")
                .and_then(Value::as_str)
                .map(str::to_string),
        )
        .map_err(input_error)?;
    save(context, catalog_store_dir(context), catalog.id, &catalog)?;
    Ok(json!({
        "catalog_id": catalog.id,
        "name": catalog.name,
        "description": catalog.description,
        "updated_at": catalog.updated_at,
        "content_digest": canonical_digest(&catalog),
    }))
}

#[derive(Debug, Deserialize)]
struct OrderLineInput {
    catalog_id: Uuid,
    name: String,
    quantity: u32,
}

fn execute_order_create(
    input: &Value,
    context: &ExecutionContext,
) -> Result<Value, ExecutionError> {
    let raw_lines = input
        .get("lines")
        .and_then(Value::as_array)
        .ok_or_else(|| input_error("missing required field: lines"))?;
    let mut lines = Vec::new();
    for raw_line in raw_lines {
        let line: OrderLineInput = serde_json::from_value(raw_line.clone())
            .map_err(|error| input_error(format!("invalid order line: {error}")))?;
        lines.push(OrderLine::new(line.catalog_id, line.name, line.quantity).map_err(input_error)?);
    }
    let order = Order::new(lines).map_err(input_error)?;
    save(context, order_store_dir(context), order.id, &order)?;
    Ok(json!({
        "order_id": order.id,
        "lines": order.lines,
        "status": order.status,
        "created_at": order.created_at,
        "content_digest": canonical_digest(&order),
    }))
}

fn execute_order_transition(
    input: &Value,
    context: &ExecutionContext,
    next: OrderStatus,
) -> Result<Value, ExecutionError> {
    let order_id = parse_id(input, "order_id")?;
    let mut order = load::<Order>(context, order_store_dir(context), order_id, "order")?;
    order
        .transition_to(next)
        .map_err(|error| input_error(error.to_string()))?;
    save(context, order_store_dir(context), order.id, &order)?;
    match next {
        OrderStatus::Approved => Ok(json!({
            "order_id": order.id,
            "status": order.status,
            "approved_at": order.updated_at,
            "content_digest": canonical_digest(&order),
        })),
        OrderStatus::Fulfilled => Ok(json!({
            "order_id": order.id,
            "status": order.status,
            "fulfilled_at": order.updated_at,
            "content_digest": canonical_digest(&order),
        })),
        _ => Err(input_error("unsupported order transition")),
    }
}

fn execute_order_approve(
    input: &Value,
    context: &ExecutionContext,
) -> Result<Value, ExecutionError> {
    execute_order_transition(input, context, OrderStatus::Approved)
}

fn execute_order_fulfill(
    input: &Value,
    context: &ExecutionContext,
) -> Result<Value, ExecutionError> {
    execute_order_transition(input, context, OrderStatus::Fulfilled)
}

pub fn commerce_handlers() -> Vec<Arc<dyn OperationHandler>> {
    vec![
        Arc::new(GenericCommerceHandler {
            operation: CATALOG_CREATE,
            schema_file: "commerce/catalog-create.input.json",
            execute_fn: execute_catalog_create,
        }),
        Arc::new(GenericCommerceHandler {
            operation: CATALOG_UPDATE,
            schema_file: "commerce/catalog-update.input.json",
            execute_fn: execute_catalog_update,
        }),
        Arc::new(GenericCommerceHandler {
            operation: ORDER_CREATE,
            schema_file: "commerce/order-create.input.json",
            execute_fn: execute_order_create,
        }),
        Arc::new(GenericCommerceHandler {
            operation: ORDER_APPROVE,
            schema_file: "commerce/order-approve.input.json",
            execute_fn: execute_order_approve,
        }),
        Arc::new(GenericCommerceHandler {
            operation: ORDER_FULFILL,
            schema_file: "commerce/order-fulfill.input.json",
            execute_fn: execute_order_fulfill,
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use proof_kernel::PrincipalId;
    use tempfile::TempDir;

    fn context() -> (ExecutionContext, TempDir) {
        let dir = TempDir::new().unwrap();
        let context = ExecutionContext {
            actor: PrincipalId::now(),
            delegation_id: None,
            delegation_chain: None,
            workspace_path: dir.path().to_path_buf(),
            timestamp: chrono::Utc::now(),
        };
        (context, dir)
    }

    fn handler(operation: &str) -> Arc<dyn OperationHandler> {
        commerce_handlers()
            .iter()
            .find(|handler| handler.operation() == operation)
            .unwrap()
            .clone()
    }

    #[test]
    fn catalog_create_validates_and_persists() {
        let (context, _dir) = context();
        let result = handler("catalog.create")
            .execute(&json!({"name": "Main", "description": "Primary"}), &context)
            .unwrap();
        assert_eq!(result["operation"], "catalog.create");
        assert!(result["data"]["catalog_id"].is_string());
        assert!(result["data"]["content_digest"].is_string());
    }

    #[test]
    fn catalog_create_rejects_unknown_field() {
        let (context, _dir) = context();
        let result =
            handler("catalog.create").execute(&json!({"name": "Main", "extra": true}), &context);
        assert!(result.is_err());
    }

    #[test]
    fn catalog_update_round_trips_and_validates() {
        let (context, _dir) = context();
        let created = handler("catalog.create")
            .execute(&json!({"name": "Main"}), &context)
            .unwrap();
        let id = created["data"]["catalog_id"].as_str().unwrap().to_string();
        let result = handler("catalog.update")
            .execute(&json!({"catalog_id": id, "name": "Updated"}), &context)
            .unwrap();
        assert_eq!(result["operation"], "catalog.update");
        assert_eq!(result["data"]["name"], "Updated");
        let bad = handler("catalog.update")
            .execute(&json!({"catalog_id": id, "unexpected": 1}), &context);
        assert!(bad.is_err());
    }

    #[test]
    fn order_create_round_trips_and_validates() {
        let (context, _dir) = context();
        let catalog = handler("catalog.create")
            .execute(&json!({"name": "Main"}), &context)
            .unwrap();
        let catalog_id = catalog["data"]["catalog_id"].as_str().unwrap();
        let result = handler("order.create")
            .execute(
                &json!({
                    "lines": [{"catalog_id": catalog_id, "name": "Widget", "quantity": 2}]
                }),
                &context,
            )
            .unwrap();
        assert_eq!(result["operation"], "order.create");
        assert_eq!(result["data"]["status"], "pending");
        let bad = handler("order.create").execute(&json!({"lines": []}), &context);
        assert!(bad.is_err());
    }

    #[test]
    fn order_approve_and_fulfill_lifecycle() {
        let (context, _dir) = context();
        let catalog = handler("catalog.create")
            .execute(&json!({"name": "Main"}), &context)
            .unwrap();
        let catalog_id = catalog["data"]["catalog_id"].as_str().unwrap();
        let created = handler("order.create")
            .execute(
                &json!({
                    "lines": [{"catalog_id": catalog_id, "name": "Widget", "quantity": 1}]
                }),
                &context,
            )
            .unwrap();
        let order_id = created["data"]["order_id"].as_str().unwrap();
        let approved = handler("order.approve")
            .execute(&json!({"order_id": order_id}), &context)
            .unwrap();
        assert_eq!(approved["data"]["status"], "approved");
        let fulfilled = handler("order.fulfill")
            .execute(&json!({"order_id": order_id}), &context)
            .unwrap();
        assert_eq!(fulfilled["data"]["status"], "fulfilled");
    }

    #[test]
    fn order_approve_rejects_fulfilled_order() {
        let (context, _dir) = context();
        let catalog = handler("catalog.create")
            .execute(&json!({"name": "Main"}), &context)
            .unwrap();
        let catalog_id = catalog["data"]["catalog_id"].as_str().unwrap();
        let created = handler("order.create")
            .execute(
                &json!({
                    "lines": [{"catalog_id": catalog_id, "name": "Widget", "quantity": 1}]
                }),
                &context,
            )
            .unwrap();
        let order_id = created["data"]["order_id"].as_str().unwrap();
        handler("order.approve")
            .execute(&json!({"order_id": order_id}), &context)
            .unwrap();
        handler("order.fulfill")
            .execute(&json!({"order_id": order_id}), &context)
            .unwrap();
        let result = handler("order.approve").execute(&json!({"order_id": order_id}), &context);
        assert!(result.is_err());
    }
}
