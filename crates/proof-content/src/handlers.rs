use std::sync::Arc;

use proof_kernel::{ExecutionContext, ExecutionError, OperationHandler};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use uuid::Uuid;

use crate::{
    digest::canonical_digest, object::Object, schema::SchemaDefinition,
};

const SCHEMA_CREATE: &str = "schema.create";
const OBJECT_CREATE: &str = "object.create";
const OBJECT_EDIT: &str = "object.edit";
const APPROVE: &str = "content.approve";
const RELEASE: &str = "content.release";

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
    let registry_path = context.workspace_path.join("registry").join(file_name);
    let contents = std::fs::read_to_string(registry_path).map_err(|error| {
        ExecutionError::HandlerFailed(format!("failed to read registry schema: {error}"))
    })?;
    serde_json::from_str(&contents).map_err(|error| {
        ExecutionError::HandlerFailed(format!(
            "invalid registry schema for {operation}: {error}"
        ))
    })
}

fn object_schema_from_value(value: Value) -> Result<SchemaDefinition, ExecutionError> {
    serde_json::from_value(value).map_err(|error| {
        ExecutionError::HandlerFailed(format!("invalid schema definition: {error}"))
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
            serde_json::to_value(HandlerResult {
                operation,
                data,
            })
            .map_err(|error| ExecutionError::HandlerFailed(error.to_string()))
        })?
    }
}

struct GenericContentHandler {
    operation: &'static str,
    schema_file: &'static str,
    execute_fn:
        fn(&Value, &ExecutionContext) -> Result<serde_json::Value, ExecutionError>,
}

impl OperationHandler for GenericContentHandler {
    fn operation(&self) -> &str {
        self.operation
    }

    fn execute(
        &self,
        input: &Value,
        context: &ExecutionContext,
    ) -> Result<Value, ExecutionError> {
        let raw_schema = registry_schema(context, self.operation, self.schema_file)?;
        JsonSchema::parse(raw_schema)?.validate(input)?;
        (self.execute_fn)(input, context).result(self.operation)
    }
}

fn execute_schema_create(
    input: &Value,
    _context: &ExecutionContext,
) -> Result<serde_json::Value, ExecutionError> {
    let schema = object_schema_from_value(input.clone())?;
    schema
        .validate()
        .map_err(|error| ExecutionError::HandlerFailed(error.to_string()))?;
    Ok(json!({
        "schema_id": schema.id,
        "name": schema.name,
        "version": schema.version,
        "fields": schema.fields,
        "created_at": schema.created_at,
        "content_digest": canonical_digest(&schema),
    }))
}

fn execute_object_create(
    input: &Value,
    _context: &ExecutionContext,
) -> Result<serde_json::Value, ExecutionError> {
    let request: ObjectCreateRequest = serde_json::from_value(input.clone())
        .map_err(|error| ExecutionError::HandlerFailed(error.to_string()))?;
    let schema = object_schema_from_value(request.schema)?;
    let object = Object::create(&schema, request.locale, request.content)
        .map_err(|error| ExecutionError::HandlerFailed(error.to_string()))?;
    Ok(json!({
        "object": object,
        "content_digest": canonical_digest(&object),
    }))
}

#[derive(Debug, Deserialize)]
struct ObjectCreateRequest {
    schema: Value,
    locale: String,
    content: Value,
}

fn execute_object_edit(
    input: &Value,
    _context: &ExecutionContext,
) -> Result<serde_json::Value, ExecutionError> {
    let request: ObjectEditRequest = serde_json::from_value(input.clone())
        .map_err(|error| ExecutionError::HandlerFailed(error.to_string()))?;
    let schema = object_schema_from_value(request.schema)?;
    let mut object = serde_json::from_value::<Object>(request.object)
        .map_err(|error| ExecutionError::HandlerFailed(error.to_string()))?;
    if object.id != request.object_id {
        return Err(input_error("object_id does not match supplied object"));
    }
    object
        .update_content(&schema, request.content)
        .map_err(|error| ExecutionError::HandlerFailed(error.to_string()))?;
    Ok(json!({
        "object": object,
        "previous_revision": object.revision.saturating_sub(1),
        "content_digest": canonical_digest(&object),
    }))
}

#[derive(Debug, Deserialize)]
struct ObjectEditRequest {
    schema: Value,
    object: Value,
    object_id: Uuid,
    content: Value,
}

fn execute_approve(
    input: &Value,
    context: &ExecutionContext,
) -> Result<serde_json::Value, ExecutionError> {
    let request: ApproveRequest = serde_json::from_value(input.clone())
        .map_err(|error| ExecutionError::HandlerFailed(error.to_string()))?;
    let mut object = serde_json::from_value::<Object>(request.object)
        .map_err(|error| ExecutionError::HandlerFailed(error.to_string()))?;
    object
        .transition_to(crate::object::ObjectStatus::Approved)
        .map_err(|error| ExecutionError::HandlerFailed(error.to_string()))?;
    Ok(json!({
        "approved_by": context.actor,
        "approved_at": context.timestamp,
        "object": object,
    }))
}

#[derive(Debug, Deserialize)]
struct ApproveRequest {
    object: Value,
    #[serde(default)]
    notes: Option<String>,
}

fn execute_release(
    input: &Value,
    context: &ExecutionContext,
) -> Result<serde_json::Value, ExecutionError> {
    let request: ReleaseRequest = serde_json::from_value(input.clone())
        .map_err(|error| ExecutionError::HandlerFailed(error.to_string()))?;
    let mut object = serde_json::from_value::<Object>(request.object)
        .map_err(|error| ExecutionError::HandlerFailed(error.to_string()))?;
    object
        .transition_to(crate::object::ObjectStatus::Committed)
        .map_err(|error| ExecutionError::HandlerFailed(error.to_string()))?;
    let actor: crate::principal::PrincipalId =
        serde_json::from_value(serde_json::to_value(context.actor).unwrap()).unwrap();
    let release =
        crate::release::Release::new(request.edition_id, request.environment, actor);
    Ok(json!({
        "release": release,
        "object": object,
    }))
}

#[derive(Debug, Deserialize)]
struct ReleaseRequest {
    edition_id: Uuid,
    environment: String,
    object: Value,
}

pub fn content_handlers() -> Vec<Arc<dyn OperationHandler>> {
    vec![
        Arc::new(GenericContentHandler {
            operation: SCHEMA_CREATE,
            schema_file: "content/schema-create.input.json",
            execute_fn: execute_schema_create,
        }),
        Arc::new(GenericContentHandler {
            operation: OBJECT_CREATE,
            schema_file: "content/object-create.input.json",
            execute_fn: execute_object_create,
        }),
        Arc::new(GenericContentHandler {
            operation: OBJECT_EDIT,
            schema_file: "content/object-edit.input.json",
            execute_fn: execute_object_edit,
        }),
        Arc::new(GenericContentHandler {
            operation: APPROVE,
            schema_file: "content/approve.input.json",
            execute_fn: execute_approve,
        }),
        Arc::new(GenericContentHandler {
            operation: RELEASE,
            schema_file: "content/release.input.json",
            execute_fn: execute_release,
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use proof_kernel::PrincipalId;
    use serde_json::json;
    use std::path::PathBuf;

    fn context() -> ExecutionContext {
        ExecutionContext {
            actor: proof_kernel::PrincipalId::now(),
            delegation_id: None,
            delegation_chain: None,
            workspace_path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .to_path_buf(),
            timestamp: chrono::Utc::now(),
        }
    }

    #[test]
    fn exposes_five_lifecycle_handlers() {
        let operations: Vec<_> = content_handlers()
            .iter()
            .map(|handler| handler.operation().to_string())
            .collect();
        assert_eq!(
            operations,
            vec![
                "schema.create".to_string(),
                "object.create".to_string(),
                "object.edit".to_string(),
                "content.approve".to_string(),
                "content.release".to_string(),
            ]
        );
    }

    #[test]
    fn schema_create_validates_and_canonicalizes() {
        let handler = &content_handlers()[0];
        let result = handler
            .execute(
                &json!({
                    "id": uuid::Uuid::now_v7().to_string(),
                    "name": "Article",
                    "version": 1,
                    "fields": [{"name": "title", "field_type": "text", "required": true, "localized": false}],
                    "created_at": chrono::Utc::now().to_rfc3339()
                }),
                &context(),
            )
            .unwrap();
        assert!(result.to_string().contains("Article"));
    }
}
