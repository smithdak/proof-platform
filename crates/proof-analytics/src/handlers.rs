use std::sync::Arc;

use proof_kernel::{ExecutionContext, ExecutionError, OperationHandler};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use uuid::Uuid;

use crate::{
    digest::canonical_digest,
    models::{
        AnalyticsInsight, AnalyticsQuery, AnalyticsQueryStatus, AnalyticsSnapshot,
        AnalyticsSnapshotId,
    },
};

const SNAPSHOT_CREATE: &str = "analytics.snapshot.create";
const QUERY_CREATE: &str = "analytics.query.create";
const QUERY_EXECUTE: &str = "analytics.query.execute";
const INSIGHT_APPROVE: &str = "analytics.insight.approve";

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
        context
            .workspace_path
            .join(".proof/registry")
            .join(file_name),
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("registry")
            .join(file_name),
        context
            .workspace_path
            .join("crates/proof-analytics")
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

struct GenericAnalyticsHandler {
    operation: &'static str,
    schema_file: &'static str,
    execute_fn: fn(&Value, &ExecutionContext) -> Result<Value, ExecutionError>,
}

impl OperationHandler for GenericAnalyticsHandler {
    fn operation(&self) -> &str {
        self.operation
    }

    fn execute(&self, input: &Value, context: &ExecutionContext) -> Result<Value, ExecutionError> {
        let raw_schema = registry_schema(context, self.operation, self.schema_file)?;
        JsonSchema::parse(raw_schema)?.validate(input)?;
        (self.execute_fn)(input, context).result(self.operation)
    }
}

fn snapshot_store_dir(context: &ExecutionContext) -> std::path::PathBuf {
    context
        .workspace_path
        .join(".proof/data/analytics/snapshots")
}

fn query_store_dir(context: &ExecutionContext) -> std::path::PathBuf {
    context.workspace_path.join(".proof/data/analytics/queries")
}

fn insight_store_dir(context: &ExecutionContext) -> std::path::PathBuf {
    context
        .workspace_path
        .join(".proof/data/analytics/insights")
}

fn save<T: Serialize>(
    _context: &ExecutionContext,
    dir: std::path::PathBuf,
    id: Uuid,
    value: &T,
) -> Result<(), ExecutionError> {
    std::fs::create_dir_all(&dir).map_err(|error| {
        ExecutionError::HandlerFailed(format!("failed to create analytics store: {error}"))
    })?;
    let path = dir.join(format!("{id}.json"));
    let serialized = serde_json::to_string_pretty(value)
        .map_err(|error| ExecutionError::HandlerFailed(error.to_string()))?;
    std::fs::write(&path, serialized)
        .map_err(|error| ExecutionError::HandlerFailed(format!("failed to save record: {error}")))
}

fn load<T: for<'de> Deserialize<'de>>(
    _context: &ExecutionContext,
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

fn execute_snapshot_create(
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
    let digest = input
        .get("digest")
        .and_then(Value::as_str)
        .ok_or_else(|| input_error("missing required field: digest"))?;
    let snapshot = AnalyticsSnapshot::new(name, description, digest).map_err(input_error)?;
    save(context, snapshot_store_dir(context), snapshot.id, &snapshot)?;
    Ok(json!({
        "snapshot_id": snapshot.id,
        "name": snapshot.name,
        "description": snapshot.description,
        "digest": snapshot.digest,
        "created_at": snapshot.created_at,
        "content_digest": canonical_digest(&snapshot),
    }))
}

fn execute_query_create(
    input: &Value,
    context: &ExecutionContext,
) -> Result<Value, ExecutionError> {
    let snapshot_id = parse_id(input, "snapshot_id")?;
    let snapshot_id = AnalyticsSnapshotId::from(snapshot_id);
    let _snapshot = load::<AnalyticsSnapshot>(
        context,
        snapshot_store_dir(context),
        snapshot_id,
        "analytics snapshot",
    )?;
    let name = input
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| input_error("missing required field: name"))?;
    let filter = input
        .get("filter")
        .cloned()
        .ok_or_else(|| input_error("missing required field: filter"))?;
    let aggregation = input
        .get("aggregation")
        .and_then(Value::as_str)
        .ok_or_else(|| input_error("missing required field: aggregation"))?;
    let query = AnalyticsQuery::new(snapshot_id, name, filter, aggregation).map_err(input_error)?;
    save(context, query_store_dir(context), query.id, &query)?;
    Ok(json!({
        "query_id": query.id,
        "snapshot_id": query.snapshot_id,
        "name": query.name,
        "filter": query.filter,
        "aggregation": query.aggregation,
        "status": query.status,
        "created_at": query.created_at,
        "updated_at": query.updated_at,
        "content_digest": canonical_digest(&query),
    }))
}

fn aggregate(values: &[f64], aggregation: &str) -> Result<Value, ExecutionError> {
    match aggregation {
        "count" => Ok(json!(values.len())),
        "sum" => Ok(json!(values.iter().sum::<f64>())),
        "average" => {
            if values.is_empty() {
                return Err(input_error("cannot average an empty value set"));
            }
            let sum = values.iter().sum::<f64>();
            Ok(json!(sum / values.len() as f64))
        }
        _ => Err(input_error(format!(
            "unsupported aggregation: {aggregation}"
        ))),
    }
}

fn execute_query_execute(
    input: &Value,
    context: &ExecutionContext,
) -> Result<Value, ExecutionError> {
    let query_id = parse_id(input, "query_id")?;
    let mut query = load::<AnalyticsQuery>(
        context,
        query_store_dir(context),
        query_id,
        "analytics query",
    )?;
    let _snapshot = load::<AnalyticsSnapshot>(
        context,
        snapshot_store_dir(context),
        query.snapshot_id,
        "analytics snapshot",
    )?;
    let values = input
        .get("values")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .map(|value| {
                    value
                        .as_f64()
                        .ok_or_else(|| input_error("invalid value: expected number"))
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .unwrap_or_else(|| Ok(Vec::new()))?;
    let result = aggregate(&values, &query.aggregation)?;
    let result_digest = canonical_digest(&result);
    query
        .transition_to(AnalyticsQueryStatus::Executed)
        .map_err(|error| input_error(error.to_string()))?;
    save(context, query_store_dir(context), query.id, &query)?;
    let insight = AnalyticsInsight::new(query.id, result_digest.clone()).map_err(input_error)?;
    save(context, insight_store_dir(context), insight.id, &insight)?;
    Ok(json!({
        "query_id": query.id,
        "snapshot_id": query.snapshot_id,
        "result": result,
        "result_digest": result_digest,
        "query_status": query.status,
        "content_digest": canonical_digest(&query),
    }))
}

fn execute_insight_approve(
    input: &Value,
    context: &ExecutionContext,
) -> Result<Value, ExecutionError> {
    if context.principal_kind != Some(proof_kernel::PrincipalKind::Human) {
        return Err(ExecutionError::HumanOnly);
    }
    let insight_id = parse_id(input, "insight_id")?;
    let mut insight = load::<AnalyticsInsight>(
        context,
        insight_store_dir(context),
        insight_id,
        "analytics insight",
    )?;
    insight
        .approve(context.actor.as_uuid(), context.timestamp)
        .map_err(input_error)?;
    save(context, insight_store_dir(context), insight.id, &insight)?;
    Ok(json!({
        "insight_id": insight.id,
        "query_id": insight.query_id,
        "result_digest": insight.result_digest,
        "status": insight.status,
        "approved_at": insight.approved_at,
        "approved_by": insight.approved_by,
        "content_digest": canonical_digest(&insight),
    }))
}

pub fn analytics_handlers() -> Vec<Arc<dyn OperationHandler>> {
    vec![
        Arc::new(GenericAnalyticsHandler {
            operation: SNAPSHOT_CREATE,
            schema_file: "analytics/analytics-snapshot-create.input.json",
            execute_fn: execute_snapshot_create,
        }),
        Arc::new(GenericAnalyticsHandler {
            operation: QUERY_CREATE,
            schema_file: "analytics/analytics-query-create.input.json",
            execute_fn: execute_query_create,
        }),
        Arc::new(GenericAnalyticsHandler {
            operation: QUERY_EXECUTE,
            schema_file: "analytics/analytics-query-execute.input.json",
            execute_fn: execute_query_execute,
        }),
        Arc::new(GenericAnalyticsHandler {
            operation: INSIGHT_APPROVE,
            schema_file: "analytics/analytics-insight-approve.input.json",
            execute_fn: execute_insight_approve,
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use proof_kernel::{PrincipalId, PrincipalKind};
    use tempfile::TempDir;

    fn context(principal_kind: PrincipalKind) -> (ExecutionContext, TempDir) {
        let dir = TempDir::new().unwrap();
        let context = ExecutionContext {
            actor: PrincipalId::now(),
            principal_kind: Some(principal_kind),
            delegation_id: None,
            delegation_chain: None,
            workspace_path: dir.path().to_path_buf(),
            timestamp: chrono::Utc::now(),
        };
        (context, dir)
    }

    fn handler(operation: &str) -> Arc<dyn OperationHandler> {
        analytics_handlers()
            .iter()
            .find(|handler| handler.operation() == operation)
            .unwrap()
            .clone()
    }

    fn create_snapshot(context: &ExecutionContext) -> Value {
        handler(SNAPSHOT_CREATE)
            .execute(
                &json!({
                    "name": "Activity",
                    "description": "Aggregate activity",
                    "digest": "sha256:snapshot"
                }),
                context,
            )
            .unwrap()
    }

    fn create_query(context: &ExecutionContext, snapshot_id: &str) -> Value {
        handler(QUERY_CREATE)
            .execute(
                &json!({
                    "snapshot_id": snapshot_id,
                    "name": "Object count",
                    "filter": {"kind": "content"},
                    "aggregation": "count"
                }),
                context,
            )
            .unwrap()
    }

    fn execute_query(context: &ExecutionContext, query_id: &str) -> Value {
        handler(QUERY_EXECUTE)
            .execute(
                &json!({"query_id": query_id, "values": [1.0, 2.0, 3.0]}),
                context,
            )
            .unwrap()
    }

    fn human_context(context: &ExecutionContext) -> ExecutionContext {
        ExecutionContext {
            actor: PrincipalId::now(),
            principal_kind: Some(PrincipalKind::Human),
            delegation_id: None,
            delegation_chain: None,
            workspace_path: context.workspace_path.clone(),
            timestamp: chrono::Utc::now(),
        }
    }

    fn try_execute(
        operation: &str,
        input: &Value,
        context: &ExecutionContext,
    ) -> Result<Value, ExecutionError> {
        analytics_handlers()
            .iter()
            .find(|handler| handler.operation() == operation)
            .unwrap()
            .execute(input, context)
    }

    #[test]
    fn all_operations_are_registered() {
        let handlers = analytics_handlers();
        let operations: Vec<_> = handlers.iter().map(|handler| handler.operation()).collect();
        assert_eq!(
            operations,
            vec![
                SNAPSHOT_CREATE,
                QUERY_CREATE,
                QUERY_EXECUTE,
                INSIGHT_APPROVE
            ]
        );
    }

    #[test]
    fn snapshot_create_validates_and_persists() {
        let (context, _dir) = context(PrincipalKind::Agent);
        let result = create_snapshot(&context);
        assert_eq!(result["operation"], SNAPSHOT_CREATE);
        assert!(result["data"]["snapshot_id"].is_string());
        assert_eq!(result["data"]["digest"], "sha256:snapshot");
        assert!(snapshot_store_dir(&context)
            .join(format!(
                "{}.json",
                result["data"]["snapshot_id"].as_str().unwrap()
            ))
            .exists());
    }

    #[test]
    fn snapshot_create_rejects_unknown_field() {
        let (context, _dir) = context(PrincipalKind::Agent);
        let execute = || {
            try_execute(
                SNAPSHOT_CREATE,
                &json!({"name": "Activity", "digest": "sha256:snapshot", "unexpected": true}),
                &context,
            )
        };
        assert!(execute().is_err());
    }

    #[test]
    fn query_create_round_trips_snapshot() {
        let (context, _dir) = context(PrincipalKind::Agent);
        let snapshot = create_snapshot(&context);
        let snapshot_id = snapshot["data"]["snapshot_id"].as_str().unwrap();
        let result = create_query(&context, snapshot_id);
        assert_eq!(result["operation"], QUERY_CREATE);
        assert_eq!(result["data"]["snapshot_id"], snapshot_id);
        assert_eq!(result["data"]["status"], "pending");
        assert!(query_store_dir(&context)
            .join(format!(
                "{}.json",
                result["data"]["query_id"].as_str().unwrap()
            ))
            .exists());
    }

    #[test]
    fn query_create_requires_existing_snapshot() {
        let (context, _dir) = context(PrincipalKind::Agent);
        let execute = || {
            try_execute(
                QUERY_CREATE,
                &json!({
                    "snapshot_id": Uuid::now_v7(),
                    "name": "Object count",
                    "filter": {},
                    "aggregation": "count"
                }),
                &context,
            )
        };
        assert!(execute().is_err());
    }

    #[test]
    fn query_execute_computes_sum_and_persists() {
        let (context, _dir) = context(PrincipalKind::Agent);
        let snapshot = create_snapshot(&context);
        let snapshot_id = snapshot["data"]["snapshot_id"].as_str().unwrap();
        let query = create_query(&context, snapshot_id);
        let _total_query = handler(QUERY_CREATE)
            .execute(
                &json!({
                    "snapshot_id": snapshot_id,
                    "name": "Total",
                    "filter": {},
                    "aggregation": "sum"
                }),
                &context,
            )
            .unwrap();
        let query_id = query["data"]["query_id"].as_str().unwrap();
        let result = execute_query(&context, query_id);
        assert_eq!(result["operation"], QUERY_EXECUTE);
        assert_eq!(result["data"]["result"], 3);
        assert_eq!(result["data"]["query_status"], "executed");
        assert!(result["data"]["result_digest"].is_string());
        assert!(insight_store_dir(&context).read_dir().unwrap().count() == 1);
    }

    #[test]
    fn query_execute_rejects_executed_query() {
        let (context, _dir) = context(PrincipalKind::Agent);
        let snapshot = create_snapshot(&context);
        let snapshot_id = snapshot["data"]["snapshot_id"].as_str().unwrap();
        let query = create_query(&context, snapshot_id);
        let query_id = query["data"]["query_id"].as_str().unwrap();
        let execute = || try_execute(QUERY_EXECUTE, &json!({"query_id": query_id}), &context);
        execute().unwrap();
        assert_eq!(
            execute().unwrap_err().to_string(),
            "handler execution failed: invalid analytics query transition from Executed to executed"
        );
    }

    #[test]
    fn insight_approves_and_lifecycles_once() {
        let (agent_context, _dir) = context(PrincipalKind::Agent);
        let snapshot = create_snapshot(&agent_context);
        let snapshot_id = snapshot["data"]["snapshot_id"].as_str().unwrap();
        let query = create_query(&agent_context, snapshot_id);
        let query_id = query["data"]["query_id"].as_str().unwrap();
        execute_query(&agent_context, query_id);
        let insight_id = insight_store_dir(&agent_context)
            .read_dir()
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .file_name()
            .into_string()
            .unwrap()
            .trim_end_matches(".json")
            .to_string();
        let human_context = human_context(&agent_context);
        let result = handler(INSIGHT_APPROVE)
            .execute(&json!({"insight_id": insight_id}), &human_context)
            .unwrap();
        assert_eq!(result["operation"], INSIGHT_APPROVE);
        assert_eq!(result["data"]["status"], "approved");
        let repeat =
            handler(INSIGHT_APPROVE).execute(&json!({"insight_id": insight_id}), &human_context);
        assert_eq!(
            repeat.unwrap_err().to_string(),
            "handler execution failed: analytics insight sha256:4e07408562bedb8b60ce05c1decfe3ad16b72230967de01f640b7e4729b49fce is not pending"
        );
    }

    #[test]
    fn insight_approve_is_human_only() {
        let (agent_context, _dir) = context(PrincipalKind::Agent);
        let result = handler(INSIGHT_APPROVE)
            .execute(&json!({"insight_id": Uuid::now_v7()}), &agent_context);
        assert_eq!(result.unwrap_err(), ExecutionError::HumanOnly);
    }
}
