use std::collections::BTreeMap;
use std::sync::Arc;

use proof_kernel::{ExecutionContext, ExecutionError, IdempotencyPolicy, OperationHandler};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use uuid::Uuid;

use crate::preview::publish_preview;
use crate::{
    digest::canonical_digest, object::Object, object::ObjectStatus, schema::SchemaDefinition,
};

const SCHEMA_CREATE: &str = "schema.create";
const OBJECT_CREATE: &str = "object.create";
const OBJECT_EDIT: &str = "object.edit";
const APPROVE: &str = "content.approve";
const RELEASE: &str = "content.release";
const RELEASE_PUBLISH: &str = "release.publish";
const CHANGESET_COMMIT: &str = "changeset.commit";
const EDITION_CREATE: &str = "edition.create";

#[derive(Debug, Deserialize)]
struct JsonSchema {
    #[serde(default)]
    required: Vec<String>,
    #[serde(default)]
    properties: Map<String, Value>,
    #[serde(rename = "additionalProperties", default)]
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
        context.workspace_path.join("registry").join(file_name),
        context
            .workspace_path
            .join(".proof/registry")
            .join(file_name),
        context.workspace_path.join("schemas").join(file_name),
        context
            .workspace_path
            .join("crates/proof-content/registry")
            .join(file_name),
        context
            .workspace_path
            .join("crates/proof-content/schemas")
            .join(file_name),
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
            serde_json::to_value(HandlerResult { operation, data })
                .map_err(|error| ExecutionError::HandlerFailed(error.to_string()))
        })?
    }
}

struct GenericContentHandler {
    operation: &'static str,
    schema_file: &'static str,
    idempotency_policy: IdempotencyPolicy,
    execute_fn: fn(&Value, &ExecutionContext) -> Result<serde_json::Value, ExecutionError>,
}

struct ReleasePublishHandler;

impl ReleasePublishHandler {
    fn execute_v1(
        &self,
        input: &Value,
        context: &ExecutionContext,
    ) -> Result<Value, ExecutionError> {
        let raw_schema = registry_schema(
            context,
            RELEASE_PUBLISH,
            "content/release-publish.input.json",
        )?;
        JsonSchema::parse(raw_schema)?.validate(input)?;
        execute_release_publish(input, context).result(RELEASE_PUBLISH)
    }

    fn execute_v2(
        &self,
        input: &Value,
        context: &ExecutionContext,
    ) -> Result<Value, ExecutionError> {
        let raw_schema = registry_schema(
            context,
            RELEASE_PUBLISH,
            "content/release-publish-v2.input.json",
        )?;
        JsonSchema::parse(raw_schema)?.validate(input)?;
        publish_preview(input, context).map_err(ExecutionError::from)
    }
}

impl OperationHandler for ReleasePublishHandler {
    fn operation(&self) -> &str {
        RELEASE_PUBLISH
    }

    fn idempotency_policy(&self) -> IdempotencyPolicy {
        IdempotencyPolicy::None
    }

    fn idempotency_policy_for(&self, version: &str) -> IdempotencyPolicy {
        if version == "v2" {
            IdempotencyPolicy::RequiredUuidV7ExactReplay
        } else {
            IdempotencyPolicy::None
        }
    }

    fn execute(&self, input: &Value, context: &ExecutionContext) -> Result<Value, ExecutionError> {
        self.execute_v1(input, context)
    }

    fn execute_versioned(
        &self,
        version: &str,
        input: &Value,
        context: &ExecutionContext,
    ) -> Result<Value, ExecutionError> {
        match version {
            "v1" => self.execute_v1(input, context),
            "v2" => self.execute_v2(input, context),
            _ => Err(input_error(format!(
                "unsupported release.publish version: {version}"
            ))),
        }
    }
}

impl OperationHandler for GenericContentHandler {
    fn operation(&self) -> &str {
        self.operation
    }

    fn idempotency_policy(&self) -> IdempotencyPolicy {
        self.idempotency_policy
    }

    fn execute(&self, input: &Value, context: &ExecutionContext) -> Result<Value, ExecutionError> {
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

fn object_store_dir(context: &ExecutionContext) -> std::path::PathBuf {
    context.workspace_path.join(".proof/data/objects")
}

fn changeset_store_dir(context: &ExecutionContext) -> std::path::PathBuf {
    context.workspace_path.join(".proof/data/changesets")
}

fn edition_store_dir(context: &ExecutionContext) -> std::path::PathBuf {
    context.workspace_path.join(".proof/data/editions")
}

fn save_object(context: &ExecutionContext, object: &Object) -> Result<(), ExecutionError> {
    let dir = object_store_dir(context);
    std::fs::create_dir_all(&dir).map_err(|error| {
        ExecutionError::HandlerFailed(format!("failed to create object store: {error}"))
    })?;
    let path = dir.join(format!("{}.json", object.id));
    let serialized = serde_json::to_string_pretty(object)
        .map_err(|error| ExecutionError::HandlerFailed(error.to_string()))?;
    std::fs::write(&path, serialized)
        .map_err(|error| ExecutionError::HandlerFailed(format!("failed to save object: {error}")))
}

fn remove_object(context: &ExecutionContext, id: Uuid) -> Result<(), ExecutionError> {
    let path = object_store_dir(context).join(format!("{id}.json"));
    if path.exists() {
        std::fs::remove_file(&path).map_err(|error| {
            ExecutionError::HandlerFailed(format!("failed to delete object {id}: {error}"))
        })?;
    }
    Ok(())
}

fn save_changeset(
    context: &ExecutionContext,
    changeset: &crate::changeset::ChangeSet,
) -> Result<(), ExecutionError> {
    let dir = changeset_store_dir(context);
    std::fs::create_dir_all(&dir).map_err(|error| {
        ExecutionError::HandlerFailed(format!("failed to create changeset store: {error}"))
    })?;
    let serialized = serde_json::to_string_pretty(changeset)
        .map_err(|error| ExecutionError::HandlerFailed(error.to_string()))?;
    std::fs::write(dir.join(format!("{}.json", changeset.id)), serialized).map_err(|error| {
        ExecutionError::HandlerFailed(format!(
            "failed to save changeset {}: {error}",
            changeset.id
        ))
    })
}

fn save_edition(
    context: &ExecutionContext,
    edition: &crate::edition::Edition,
) -> Result<(), ExecutionError> {
    let dir = edition_store_dir(context);
    std::fs::create_dir_all(&dir).map_err(|error| {
        ExecutionError::HandlerFailed(format!("failed to create edition store: {error}"))
    })?;
    let serialized = serde_json::to_string_pretty(edition)
        .map_err(|error| ExecutionError::HandlerFailed(error.to_string()))?;
    std::fs::write(dir.join(format!("{}.json", edition.id)), serialized).map_err(|error| {
        ExecutionError::HandlerFailed(format!("failed to save edition {}: {error}", edition.id))
    })
}

fn load_object(context: &ExecutionContext, id: Uuid) -> Result<Object, ExecutionError> {
    let path = object_store_dir(context).join(format!("{id}.json"));
    let contents = std::fs::read_to_string(&path).map_err(|error| {
        ExecutionError::HandlerFailed(format!("failed to load object {id}: {error}"))
    })?;
    serde_json::from_str(&contents).map_err(|error| {
        ExecutionError::HandlerFailed(format!("invalid object file for {id}: {error}"))
    })
}

fn context_actor(context: &ExecutionContext) -> crate::principal::PrincipalId {
    serde_json::from_value(serde_json::to_value(context.actor).unwrap_or_default())
        .unwrap_or_default()
}

fn load_schema_for_object(
    context: &ExecutionContext,
    object: &Object,
) -> Result<SchemaDefinition, ExecutionError> {
    let schema_path = context.workspace_path.join(format!(
        ".proof/data/schemas/{}-{}.json",
        object.schema_id, object.schema_version
    ));
    let contents = std::fs::read_to_string(&schema_path).map_err(|_| {
        ExecutionError::HandlerFailed(format!(
            "schema {} version {} not found for object {}",
            object.schema_id, object.schema_version, object.id
        ))
    })?;
    serde_json::from_str(&contents).map_err(|error| {
        ExecutionError::HandlerFailed(format!("invalid schema definition: {error}"))
    })
}

fn execute_object_edit(
    input: &Value,
    context: &ExecutionContext,
) -> Result<serde_json::Value, ExecutionError> {
    let request: ObjectEditRequest = serde_json::from_value(input.clone())
        .map_err(|error| ExecutionError::HandlerFailed(error.to_string()))?;
    let mut object = load_object(context, request.object_id)?;
    if object.status() != ObjectStatus::Draft {
        return Err(input_error(format!(
            "object {} is {:?}; only Draft objects can be edited",
            request.object_id,
            object.status()
        )));
    }
    for (field, value) in &request.edits {
        let content = object.content.as_object_mut().ok_or_else(|| {
            input_error(format!("object {} content is not a JSON object", object.id))
        })?;
        content.insert(field.clone(), value.clone());
    }
    let schema = load_schema_for_object(context, &object)?;
    object
        .update_content(&schema, object.content.clone())
        .map_err(|error| ExecutionError::HandlerFailed(error.to_string()))?;
    save_object(context, &object)?;
    let previous_revision = object.revision.saturating_sub(1);
    Ok(json!({
        "object": serde_json::to_value(&object).unwrap_or_default(),
        "previous_revision": previous_revision,
        "content_digest": canonical_digest(&object),
    }))
}

#[derive(Debug, Deserialize)]
struct ObjectEditRequest {
    object_id: Uuid,
    edits: BTreeMap<String, Value>,
}

fn execute_approve(
    input: &Value,
    context: &ExecutionContext,
) -> Result<serde_json::Value, ExecutionError> {
    let request: ApproveRequest = serde_json::from_value(input.clone())
        .map_err(|error| ExecutionError::HandlerFailed(error.to_string()))?;
    let mut object = load_object(context, request.object_id)?;
    object
        .transition_to(ObjectStatus::Approved)
        .map_err(|error| ExecutionError::HandlerFailed(error.to_string()))?;
    save_object(context, &object)?;
    Ok(json!({
        "status": "approved",
        "approved_by": context.actor,
        "approved_at": context.timestamp,
        "object": serde_json::to_value(&object).unwrap_or_default(),
    }))
}

#[derive(Debug, Deserialize)]
struct ApproveRequest {
    object_id: Uuid,
    #[serde(default)]
    notes: Option<String>,
}

fn execute_release(
    input: &Value,
    context: &ExecutionContext,
) -> Result<serde_json::Value, ExecutionError> {
    let request: ReleaseRequest = serde_json::from_value(input.clone())
        .map_err(|error| ExecutionError::HandlerFailed(error.to_string()))?;
    let mut object = load_object(context, request.object_id)?;
    object
        .transition_to(ObjectStatus::Published)
        .map_err(|error| ExecutionError::HandlerFailed(error.to_string()))?;
    save_object(context, &object)?;
    let actor = context_actor(context);
    let release = crate::release::Release::new(request.edition_id, request.environment, actor);
    Ok(json!({
        "release": release,
        "object": serde_json::to_value(&object).unwrap_or_default(),
        "status": "published",
    }))
}

fn execute_release_publish(
    input: &Value,
    context: &ExecutionContext,
) -> Result<serde_json::Value, ExecutionError> {
    let request: ReleasePublishRequest = serde_json::from_value(input.clone())
        .map_err(|error| ExecutionError::HandlerFailed(error.to_string()))?;
    if request.environment.trim().is_empty() {
        return Err(input_error("environment must not be empty"));
    }
    if request.version_label.trim().is_empty() {
        return Err(input_error("version_label must not be empty"));
    }
    let actor = context_actor(context);
    let release = crate::release::Release::new(Uuid::now_v7(), request.environment, actor);
    Ok(json!({
        "release": release,
        "version_label": request.version_label,
    }))
}

#[derive(Debug, Deserialize)]
struct ReleasePublishRequest {
    environment: String,
    version_label: String,
}

#[derive(Debug, Deserialize)]
struct ReleaseRequest {
    edition_id: Uuid,
    environment: String,
    object_id: Uuid,
}

fn execute_changeset_commit(
    input: &Value,
    context: &ExecutionContext,
) -> Result<serde_json::Value, ExecutionError> {
    let request: ChangesetCommitRequest = serde_json::from_value(input.clone())
        .map_err(|error| ExecutionError::HandlerFailed(error.to_string()))?;
    let changeset_path =
        changeset_store_dir(context).join(format!("{}.json", request.changeset_id));
    let contents = std::fs::read_to_string(&changeset_path).map_err(|error| {
        ExecutionError::HandlerFailed(format!(
            "failed to load changeset {}: {error}",
            request.changeset_id
        ))
    })?;
    let changeset: crate::changeset::ChangeSet = serde_json::from_str(&contents)
        .map_err(|error| ExecutionError::HandlerFailed(format!("invalid changeset: {error}")))?;
    let mut base_state = load_base_state(context)?;
    let base_object_ids: Vec<_> = base_state.keys().copied().collect();
    let schemas = load_all_schemas(context)?;
    let (committed_changeset, result_state) = changeset
        .commit_with_result(&schemas, &mut base_state)
        .map_err(|error| ExecutionError::HandlerFailed(error.to_string()))?;
    for object in result_state.values() {
        save_object(context, object)?;
    }
    for object_id in base_object_ids {
        if !result_state.contains_key(&object_id) {
            remove_object(context, object_id)?;
        }
    }
    save_changeset(context, &committed_changeset)?;
    Ok(json!({
        "changeset": committed_changeset,
        "objects_count": result_state.len(),
    }))
}

#[derive(Debug, Deserialize)]
struct ChangesetCommitRequest {
    #[serde(rename = "idempotency_key")]
    _idempotency_key: Uuid,
    changeset_id: Uuid,
    #[serde(default)]
    _notes: Option<String>,
}

fn execute_edition_create(
    input: &Value,
    context: &ExecutionContext,
) -> Result<serde_json::Value, ExecutionError> {
    let request: EditionCreateRequest = serde_json::from_value(input.clone())
        .map_err(|error| ExecutionError::HandlerFailed(error.to_string()))?;
    let changeset_path =
        changeset_store_dir(context).join(format!("{}.json", request.changeset_id));
    let contents = std::fs::read_to_string(&changeset_path).map_err(|error| {
        ExecutionError::HandlerFailed(format!(
            "failed to load changeset {}: {error}",
            request.changeset_id
        ))
    })?;
    let changeset: crate::changeset::ChangeSet = serde_json::from_str(&contents)
        .map_err(|error| ExecutionError::HandlerFailed(format!("invalid changeset: {error}")))?;
    if changeset.status != crate::changeset::ChangeSetStatus::Committed {
        return Err(input_error(format!(
            "changeset {} is not committed",
            request.changeset_id
        )));
    }

    let objects = load_base_state(context)?.into_values().collect();
    let edition = crate::edition::Edition::new(request.changeset_id, objects);
    save_edition(context, &edition)?;
    Ok(json!({"edition": edition}))
}

#[derive(Debug, Deserialize)]
struct EditionCreateRequest {
    #[serde(rename = "idempotency_key")]
    _idempotency_key: Uuid,
    changeset_id: Uuid,
}

fn load_base_state(
    context: &ExecutionContext,
) -> Result<crate::changeset::BaseState, ExecutionError> {
    let dir = object_store_dir(context);
    let mut state = crate::changeset::BaseState::new();
    if !dir.exists() {
        return Ok(state);
    }
    let entries = std::fs::read_dir(&dir).map_err(|error| {
        ExecutionError::HandlerFailed(format!("failed to read object store: {error}"))
    })?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            let contents = std::fs::read_to_string(&path).map_err(|error| {
                ExecutionError::HandlerFailed(format!("failed to read object file: {error}"))
            })?;
            let object: Object = serde_json::from_str(&contents).map_err(|error| {
                ExecutionError::HandlerFailed(format!("invalid object file: {error}"))
            })?;
            state.insert(object.id, object);
        }
    }
    Ok(state)
}

fn load_all_schemas(context: &ExecutionContext) -> Result<Vec<SchemaDefinition>, ExecutionError> {
    let dir = context.workspace_path.join(".proof/data/schemas");
    let mut schemas = Vec::new();
    let entries = std::fs::read_dir(&dir).map_err(|error| {
        ExecutionError::HandlerFailed(format!("failed to read schema store: {error}"))
    })?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            let contents = std::fs::read_to_string(&path).map_err(|error| {
                ExecutionError::HandlerFailed(format!("failed to read schema file: {error}"))
            })?;
            let schema: SchemaDefinition = serde_json::from_str(&contents).map_err(|error| {
                ExecutionError::HandlerFailed(format!("invalid schema file: {error}"))
            })?;
            schemas.push(schema);
        }
    }
    Ok(schemas)
}

pub fn content_handlers() -> Vec<Arc<dyn OperationHandler>> {
    vec![
        Arc::new(GenericContentHandler {
            operation: SCHEMA_CREATE,
            schema_file: "content/schema-create.input.json",
            idempotency_policy: IdempotencyPolicy::None,
            execute_fn: execute_schema_create,
        }),
        Arc::new(GenericContentHandler {
            operation: OBJECT_CREATE,
            schema_file: "content/object-create.input.json",
            idempotency_policy: IdempotencyPolicy::None,
            execute_fn: execute_object_create,
        }),
        Arc::new(GenericContentHandler {
            operation: OBJECT_EDIT,
            schema_file: "content/object-edit.input.json",
            idempotency_policy: IdempotencyPolicy::None,
            execute_fn: execute_object_edit,
        }),
        Arc::new(GenericContentHandler {
            operation: APPROVE,
            schema_file: "content/approve.input.json",
            idempotency_policy: IdempotencyPolicy::None,
            execute_fn: execute_approve,
        }),
        Arc::new(GenericContentHandler {
            operation: RELEASE,
            schema_file: "content/release.input.json",
            idempotency_policy: IdempotencyPolicy::None,
            execute_fn: execute_release,
        }),
        Arc::new(ReleasePublishHandler),
        Arc::new(GenericContentHandler {
            operation: CHANGESET_COMMIT,
            schema_file: "content/changeset-commit.input.json",
            idempotency_policy: IdempotencyPolicy::RequiredUuidV7ExactReplay,
            execute_fn: execute_changeset_commit,
        }),
        Arc::new(GenericContentHandler {
            operation: EDITION_CREATE,
            schema_file: "content/edition-create.input.json",
            idempotency_policy: IdempotencyPolicy::RequiredUuidV7ExactReplay,
            execute_fn: execute_edition_create,
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use proof_kernel::PrincipalId;
    use serde_json::json;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn context() -> ExecutionContext {
        ExecutionContext {
            actor: PrincipalId::now(),
            principal_kind: Some(proof_kernel::PrincipalKind::Agent),
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

    fn test_context() -> (ExecutionContext, TempDir) {
        let dir = TempDir::new().unwrap();
        let registry_dir = dir.path().join("registry/content");
        std::fs::create_dir_all(&registry_dir).unwrap();
        for file in [
            "schema-create.input.json",
            "object-create.input.json",
            "object-edit.input.json",
            "approve.input.json",
            "release.input.json",
            "release-publish.input.json",
            "changeset-commit.input.json",
            "edition-create.input.json",
        ] {
            let src = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("registry/content")
                .join(file);
            std::fs::copy(&src, registry_dir.join(file)).unwrap();
        }
        let ctx = ExecutionContext {
            actor: PrincipalId::now(),
            principal_kind: Some(proof_kernel::PrincipalKind::Agent),
            delegation_id: None,
            delegation_chain: None,
            workspace_path: dir.path().to_path_buf(),
            timestamp: chrono::Utc::now(),
        };
        (ctx, dir)
    }

    fn make_schema() -> SchemaDefinition {
        SchemaDefinition {
            id: uuid::Uuid::now_v7(),
            name: "Article".to_string(),
            version: 1,
            fields: vec![crate::schema::SchemaField {
                name: "title".to_string(),
                field_type: crate::schema::FieldType::Text,
                required: true,
                localized: false,
                default_value: None,
            }],
            created_at: chrono::Utc::now(),
        }
    }

    fn create_and_save_object(ctx: &ExecutionContext, schema: &SchemaDefinition) -> Object {
        let object = Object::create(schema, "en", json!({"title": "Hello"})).unwrap();
        std::fs::create_dir_all(object_store_dir(ctx)).unwrap();
        let path = object_store_dir(ctx).join(format!("{}.json", object.id));
        std::fs::write(&path, serde_json::to_string(&object).unwrap()).unwrap();
        object
    }

    fn save_schema(ctx: &ExecutionContext, schema: &SchemaDefinition) {
        let dir = ctx.workspace_path.join(".proof/data/schemas");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{}-{}.json", schema.id, schema.version));
        std::fs::write(&path, serde_json::to_string(schema).unwrap()).unwrap();
    }

    #[test]
    fn exposes_eight_handlers_with_exact_replay_only_for_mutations() {
        let handlers = content_handlers();
        let operations: Vec<_> = handlers
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
                "release.publish".to_string(),
                "changeset.commit".to_string(),
                "edition.create".to_string(),
            ]
        );
        let replay_operations: Vec<_> = handlers
            .iter()
            .filter(|handler| {
                handler.idempotency_policy() == IdempotencyPolicy::RequiredUuidV7ExactReplay
            })
            .map(|handler| handler.operation())
            .collect();
        assert_eq!(
            replay_operations,
            vec!["changeset.commit", "edition.create"]
        );
    }

    #[test]
    fn target_input_schemas_reject_unknown_fields() {
        let (ctx, _dir) = test_context();
        for (handler_index, input) in [
            (
                6,
                json!({
                    "idempotency_key": uuid::Uuid::now_v7(),
                    "changeset_id": uuid::Uuid::now_v7(),
                    "unexpected": true,
                }),
            ),
            (
                7,
                json!({
                    "idempotency_key": uuid::Uuid::now_v7(),
                    "changeset_id": uuid::Uuid::now_v7(),
                    "unexpected": true,
                }),
            ),
        ] {
            assert!(content_handlers()[handler_index]
                .execute(&input, &ctx)
                .is_err());
        }
    }

    #[test]
    fn resolves_input_schema_from_initialized_workspace_registry() {
        let dir = TempDir::new().unwrap();
        let registry_dir = dir.path().join(".proof/registry/content");
        std::fs::create_dir_all(&registry_dir).unwrap();
        let root_schema = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("registry/content/schema-create.input.json");
        std::fs::copy(root_schema, registry_dir.join("schema-create.input.json")).unwrap();
        let context = ExecutionContext {
            actor: PrincipalId::now(),
            principal_kind: Some(proof_kernel::PrincipalKind::Agent),
            delegation_id: None,
            delegation_chain: None,
            workspace_path: dir.path().to_path_buf(),
            timestamp: chrono::Utc::now(),
        };

        let result = content_handlers()[0]
            .execute(
                &json!({
                    "id": uuid::Uuid::now_v7(),
                    "name": "Article",
                    "version": 1,
                    "fields": [{"name": "title", "field_type": "text", "required": true, "localized": false}],
                    "created_at": chrono::Utc::now().to_rfc3339(),
                }),
                &context,
            )
            .unwrap();
        assert_eq!(result["operation"], "schema.create");
    }

    #[test]
    fn schema_create_validates_and_canonicalizes() {
        let (ctx, _dir) = test_context();
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
                &ctx,
            )
            .unwrap();
        assert!(result.to_string().contains("Article"));
    }

    #[test]
    fn object_edit_applies_edits_and_saves() {
        let (ctx, dir) = test_context();
        let schema = make_schema();
        save_schema(&ctx, &schema);
        let object = create_and_save_object(&ctx, &schema);

        let handler = &content_handlers()[2];
        let result = handler
            .execute(
                &json!({
                    "object_id": object.id.to_string(),
                    "edits": {"title": "Updated Title"}
                }),
                &ctx,
            )
            .unwrap();
        assert!(result["data"]["content_digest"].is_string());
        assert_eq!(result["data"]["previous_revision"], 1);

        let saved: Object = serde_json::from_str(
            &std::fs::read_to_string(
                dir.path()
                    .join(".proof/data/objects")
                    .join(format!("{}.json", object.id)),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(saved.content["title"], "Updated Title");
        assert_eq!(saved.revision, 2);
    }

    #[test]
    fn object_edit_rejects_non_draft() {
        let (ctx, _dir) = test_context();
        let schema = make_schema();
        save_schema(&ctx, &schema);
        let mut object = create_and_save_object(&ctx, &schema);
        object.transition_to(ObjectStatus::Submitted).unwrap();
        let path = object_store_dir(&ctx).join(format!("{}.json", object.id));
        std::fs::write(&path, serde_json::to_string(&object).unwrap()).unwrap();

        let handler = &content_handlers()[2];
        let result = handler.execute(
            &json!({"object_id": object.id.to_string(), "edits": {"title": "Nope"}}),
            &ctx,
        );
        assert!(result.is_err());
    }

    #[test]
    fn approve_transitions_submitted_to_approved() {
        let (ctx, dir) = test_context();
        let schema = make_schema();
        let mut object = create_and_save_object(&ctx, &schema);
        object.transition_to(ObjectStatus::Submitted).unwrap();
        let path = object_store_dir(&ctx).join(format!("{}.json", object.id));
        std::fs::write(&path, serde_json::to_string(&object).unwrap()).unwrap();

        let handler = &content_handlers()[3];
        let result = handler
            .execute(&json!({"object_id": object.id.to_string()}), &ctx)
            .unwrap();
        assert_eq!(result["data"]["status"], "approved");

        let saved: Object = serde_json::from_str(
            &std::fs::read_to_string(
                dir.path()
                    .join(".proof/data/objects")
                    .join(format!("{}.json", object.id)),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(saved.status(), ObjectStatus::Approved);
    }

    #[test]
    fn approve_rejects_draft() {
        let (ctx, _dir) = test_context();
        let schema = make_schema();
        let object = create_and_save_object(&ctx, &schema);

        let handler = &content_handlers()[3];
        let result = handler.execute(&json!({"object_id": object.id.to_string()}), &ctx);
        assert!(result.is_err());
    }

    #[test]
    fn release_transitions_committed_to_published() {
        let (ctx, dir) = test_context();
        let schema = make_schema();
        let mut object = create_and_save_object(&ctx, &schema);
        object.transition_to(ObjectStatus::Submitted).unwrap();
        object.transition_to(ObjectStatus::Approved).unwrap();
        object.transition_to(ObjectStatus::Committed).unwrap();
        let path = object_store_dir(&ctx).join(format!("{}.json", object.id));
        std::fs::write(&path, serde_json::to_string(&object).unwrap()).unwrap();

        let handler = &content_handlers()[4];
        let result = handler
            .execute(
                &json!({
                    "object_id": object.id.to_string(),
                    "edition_id": uuid::Uuid::now_v7().to_string(),
                    "environment": "staging"
                }),
                &ctx,
            )
            .unwrap();
        assert_eq!(result["data"]["status"], "published");

        let saved: Object = serde_json::from_str(
            &std::fs::read_to_string(
                dir.path()
                    .join(".proof/data/objects")
                    .join(format!("{}.json", object.id)),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(saved.status(), ObjectStatus::Published);
    }

    #[test]
    fn release_rejects_draft() {
        let (ctx, _dir) = test_context();
        let schema = make_schema();
        let object = create_and_save_object(&ctx, &schema);

        let handler = &content_handlers()[4];
        let result = handler.execute(
            &json!({
                "object_id": object.id.to_string(),
                "edition_id": uuid::Uuid::now_v7().to_string(),
                "environment": "staging"
            }),
            &ctx,
        );
        assert!(result.is_err());
    }
}
