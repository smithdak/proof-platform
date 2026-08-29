use std::sync::Arc;

use proof_kernel::{ExecutionContext, ExecutionError, OperationHandler};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use uuid::Uuid;

use crate::{
    digest::canonical_digest,
    models::{
        WorkflowDefinition, WorkflowRun, WorkflowRunStatus, WorkflowStep, WorkflowStepBlueprint,
        WorkflowStepStatus,
    },
};

const WORKFLOW_DEFINE: &str = "workflow.define";
const WORKFLOW_TRIGGER: &str = "workflow.trigger";
const WORKFLOW_STEP_COMPLETE: &str = "workflow.step.complete";
const WORKFLOW_APPROVE: &str = "workflow.approve";

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
            .join("crates/proof-workflow")
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

struct GenericWorkflowHandler {
    operation: &'static str,
    schema_file: &'static str,
    execute_fn: fn(&Value, &ExecutionContext) -> Result<Value, ExecutionError>,
}

impl OperationHandler for GenericWorkflowHandler {
    fn operation(&self) -> &str {
        self.operation
    }

    fn execute(&self, input: &Value, context: &ExecutionContext) -> Result<Value, ExecutionError> {
        let raw_schema = registry_schema(context, self.operation, self.schema_file)?;
        JsonSchema::parse(raw_schema)?.validate(input)?;
        (self.execute_fn)(input, context).result(self.operation)
    }
}

fn definition_store_dir(context: &ExecutionContext) -> std::path::PathBuf {
    context
        .workspace_path
        .join(".proof/data/workflow/definitions")
}

fn run_store_dir(context: &ExecutionContext) -> std::path::PathBuf {
    context.workspace_path.join(".proof/data/workflow/runs")
}

fn step_store_dir(context: &ExecutionContext) -> std::path::PathBuf {
    context.workspace_path.join(".proof/data/workflow/steps")
}

fn save<T: Serialize>(
    _context: &ExecutionContext,
    dir: std::path::PathBuf,
    id: Uuid,
    value: &T,
) -> Result<(), ExecutionError> {
    std::fs::create_dir_all(&dir).map_err(|error| {
        ExecutionError::HandlerFailed(format!("failed to create workflow store: {error}"))
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

fn parse_steps(input: &Value) -> Result<Vec<WorkflowStepBlueprint>, ExecutionError> {
    let raw_steps = input
        .get("steps")
        .and_then(Value::as_array)
        .ok_or_else(|| input_error("missing required field: steps"))?;
    raw_steps
        .iter()
        .map(|raw_step| {
            let key = raw_step
                .get("key")
                .and_then(Value::as_str)
                .ok_or_else(|| input_error("invalid workflow step: missing key"))?;
            let name = raw_step
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| input_error("invalid workflow step: missing name"))?;
            let requires_approval = raw_step
                .get("requires_approval")
                .and_then(Value::as_bool)
                .ok_or_else(|| input_error("invalid workflow step: missing requires_approval"))?;
            WorkflowStepBlueprint::new(key, name, requires_approval).map_err(input_error)
        })
        .collect()
}

fn load_definition(
    context: &ExecutionContext,
    definition_id: Uuid,
) -> Result<WorkflowDefinition, ExecutionError> {
    load(
        context,
        definition_store_dir(context),
        definition_id,
        "workflow definition",
    )
}

fn load_steps(
    context: &ExecutionContext,
    run: &WorkflowRun,
    definition: &WorkflowDefinition,
) -> Result<Vec<WorkflowStep>, ExecutionError> {
    definition
        .steps
        .iter()
        .map(|blueprint| {
            load::<WorkflowStep>(
                context,
                step_store_dir(context),
                step_id(run.id, &blueprint.key)?,
                "workflow step",
            )
        })
        .collect()
}

fn step_id(run_id: Uuid, key: &str) -> Result<Uuid, ExecutionError> {
    let digest = canonical_digest(&json!({"run_id": run_id, "step_key": key}));
    let hex_digest = digest.trim_start_matches("sha256:");
    let bytes = hex::decode(hex_digest)
        .map_err(|error| input_error(format!("failed to derive workflow step id: {error}")))?;
    Ok(Uuid::from_slice(&bytes[..16]).expect("16 bytes form a UUID"))
}

fn execute_define(input: &Value, context: &ExecutionContext) -> Result<Value, ExecutionError> {
    let name = input
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| input_error("missing required field: name"))?;
    let description = input
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let definition =
        WorkflowDefinition::new(name, description, parse_steps(input)?).map_err(input_error)?;
    save(
        context,
        definition_store_dir(context),
        definition.id,
        &definition,
    )?;
    Ok(json!({
        "workflow_definition_id": definition.id,
        "name": definition.name,
        "description": definition.description,
        "steps": definition.steps,
        "created_at": definition.created_at,
        "content_digest": canonical_digest(&definition),
    }))
}

fn execute_trigger(input: &Value, context: &ExecutionContext) -> Result<Value, ExecutionError> {
    let definition_id = parse_id(input, "workflow_id")?;
    let definition = load_definition(context, definition_id)?;
    let run = WorkflowRun::new(definition.id);
    let steps: Result<Vec<WorkflowStep>, ExecutionError> = definition
        .steps
        .iter()
        .map(|blueprint| {
            let now = run.created_at;
            let step = WorkflowStep {
                id: step_id(run.id, &blueprint.key)?,
                workflow_run_id: run.id,
                workflow_definition_id: definition.id,
                key: blueprint.key.clone(),
                name: blueprint.name.clone(),
                requires_approval: blueprint.requires_approval,
                status: WorkflowStepStatus::Pending,
                created_at: now,
                updated_at: now,
            };
            save(context, step_store_dir(context), step.id, &step)?;
            Ok(step)
        })
        .collect();
    let steps = steps?;
    save(context, run_store_dir(context), run.id, &run)?;
    Ok(json!({
        "workflow_run_id": run.id,
        "workflow_definition_id": run.workflow_definition_id,
        "status": run.status,
        "steps": steps.iter().map(|step| json!({
            "key": step.key,
            "name": step.name,
            "requires_approval": step.requires_approval,
            "status": step.status,
        })).collect::<Vec<_>>(),
        "created_at": run.created_at,
        "content_digest": canonical_digest(&run),
    }))
}

fn execute_step_complete(
    input: &Value,
    context: &ExecutionContext,
) -> Result<Value, ExecutionError> {
    let run_id = parse_id(input, "run_id")?;
    let run = load::<WorkflowRun>(context, run_store_dir(context), run_id, "workflow run")?;
    let definition = load_definition(context, run.workflow_definition_id)?;
    let mut steps = load_steps(context, &run, &definition)?;
    let step_index = steps
        .iter()
        .position(|step| step.status == WorkflowStepStatus::Pending)
        .ok_or_else(|| input_error("workflow run has no pending step"))?;
    if steps[..step_index]
        .iter()
        .any(|step| step.status != WorkflowStepStatus::Approved)
    {
        return Err(input_error(format!(
            "workflow step {} cannot complete before earlier steps are approved",
            steps[step_index].key
        )));
    }
    let step_id = steps[step_index].id;
    let step_key = steps[step_index].key.clone();
    steps[step_index].complete().map_err(input_error)?;
    save(
        context,
        step_store_dir(context),
        step_id,
        &steps[step_index],
    )?;
    let all_steps_approved = steps
        .iter()
        .all(|step| step.status == WorkflowStepStatus::Approved);
    let mut run = run;
    if all_steps_approved {
        run.transition_to(WorkflowRunStatus::Completed)
            .map_err(|error| input_error(error.to_string()))?;
    } else if run.status == WorkflowRunStatus::Pending {
        run.transition_to(WorkflowRunStatus::InProgress)
            .map_err(|error| input_error(error.to_string()))?;
    }
    save(context, run_store_dir(context), run.id, &run)?;
    Ok(json!({
        "workflow_run_id": run.id,
        "step_key": step_key,
        "step_status": steps[step_index].status,
        "run_status": run.status,
        "updated_at": run.updated_at,
        "content_digest": canonical_digest(&run),
    }))
}

fn execute_approve(input: &Value, context: &ExecutionContext) -> Result<Value, ExecutionError> {
    if context.principal_kind != Some(proof_kernel::PrincipalKind::Human) {
        return Err(ExecutionError::HumanOnly);
    }
    let run_id = parse_id(input, "run_id")?;
    let run = load::<WorkflowRun>(context, run_store_dir(context), run_id, "workflow run")?;
    let definition = load_definition(context, run.workflow_definition_id)?;
    let mut steps = load_steps(context, &run, &definition)?;
    let step_index = steps
        .iter()
        .position(|step| step.status == WorkflowStepStatus::Completed)
        .ok_or_else(|| input_error("workflow run has no completed step awaiting approval"))?;
    let step_id = steps[step_index].id;
    steps[step_index].approve().map_err(input_error)?;
    save(
        context,
        step_store_dir(context),
        step_id,
        &steps[step_index],
    )?;
    Ok(json!({
        "workflow_run_id": run.id,
        "step_key": steps[step_index].key,
        "step_status": steps[step_index].status,
        "approved_at": steps[step_index].updated_at,
        "content_digest": canonical_digest(&steps[step_index]),
    }))
}

pub fn workflow_handlers() -> Vec<Arc<dyn OperationHandler>> {
    vec![
        Arc::new(GenericWorkflowHandler {
            operation: WORKFLOW_DEFINE,
            schema_file: "workflow/workflow-define.input.json",
            execute_fn: execute_define,
        }),
        Arc::new(GenericWorkflowHandler {
            operation: WORKFLOW_TRIGGER,
            schema_file: "workflow/workflow-trigger.input.json",
            execute_fn: execute_trigger,
        }),
        Arc::new(GenericWorkflowHandler {
            operation: WORKFLOW_STEP_COMPLETE,
            schema_file: "workflow/workflow-step-complete.input.json",
            execute_fn: execute_step_complete,
        }),
        Arc::new(GenericWorkflowHandler {
            operation: WORKFLOW_APPROVE,
            schema_file: "workflow/workflow-approve.input.json",
            execute_fn: execute_approve,
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
        workflow_handlers()
            .iter()
            .find(|handler| handler.operation() == operation)
            .unwrap()
            .clone()
    }

    fn define(context: &ExecutionContext) -> Value {
        handler(WORKFLOW_DEFINE)
            .execute(
                &json!({
                    "name": "Release",
                    "description": "Ship safely",
                    "steps": [
                        {"key": "review", "name": "Review", "requires_approval": true},
                        {"key": "deploy", "name": "Deploy", "requires_approval": false}
                    ]
                }),
                context,
            )
            .unwrap()
    }

    fn trigger(context: &ExecutionContext, definition_id: &str) -> Value {
        handler(WORKFLOW_TRIGGER)
            .execute(&json!({"workflow_id": definition_id}), context)
            .unwrap()
    }

    #[test]
    fn all_operations_are_registered() {
        let handlers = workflow_handlers();
        let operations: Vec<_> = handlers.iter().map(|handler| handler.operation()).collect();
        assert_eq!(operations.len(), 4);
        assert!(operations.contains(&WORKFLOW_DEFINE));
        assert!(operations.contains(&WORKFLOW_TRIGGER));
        assert!(operations.contains(&WORKFLOW_STEP_COMPLETE));
        assert!(operations.contains(&WORKFLOW_APPROVE));
    }

    #[test]
    fn workflow_define_validates_and_persists() {
        let (context, _dir) = context(PrincipalKind::Agent);
        let result = define(&context);
        assert_eq!(result["operation"], WORKFLOW_DEFINE);
        assert!(result["data"]["workflow_definition_id"].is_string());
        assert!(result["data"]["content_digest"].is_string());
        assert!(definition_store_dir(&context)
            .join(format!(
                "{}.json",
                result["data"]["workflow_definition_id"].as_str().unwrap()
            ))
            .exists());
    }

    #[test]
    fn workflow_define_rejects_unknown_field() {
        let (context, _dir) = context(PrincipalKind::Agent);
        let result = handler(WORKFLOW_DEFINE)
            .execute(&json!({"name": "Release", "unexpected": true}), &context);
        assert!(result.is_err());
    }

    #[test]
    fn workflow_trigger_round_trips_definition() {
        let (context, _dir) = context(PrincipalKind::Agent);
        let definition = define(&context);
        let definition_id = definition["data"]["workflow_definition_id"]
            .as_str()
            .unwrap();
        let result = trigger(&context, definition_id);
        assert_eq!(result["operation"], WORKFLOW_TRIGGER);
        assert_eq!(result["data"]["status"], "pending");
        assert_eq!(result["data"]["steps"][0]["status"], "pending");
        assert!(run_store_dir(&context)
            .join(format!(
                "{}.json",
                result["data"]["workflow_run_id"].as_str().unwrap()
            ))
            .exists());
    }

    #[test]
    fn workflow_trigger_requires_existing_definition() {
        let (context, _dir) = context(PrincipalKind::Agent);
        let result = handler(WORKFLOW_TRIGGER).execute(
            &json!({"workflow_definition_id": Uuid::now_v7().to_string()}),
            &context,
        );
        assert!(result.is_err());
    }

    #[test]
    fn workflow_step_completes_and_run_starts() {
        let (context, _dir) = context(PrincipalKind::Agent);
        let definition = define(&context);
        let definition_id = definition["data"]["workflow_definition_id"]
            .as_str()
            .unwrap();
        let run = trigger(&context, definition_id);
        let run_id = run["data"]["workflow_run_id"].as_str().unwrap();
        let result = handler(WORKFLOW_STEP_COMPLETE)
            .execute(&json!({"run_id": run_id}), &context)
            .unwrap();
        assert_eq!(result["operation"], WORKFLOW_STEP_COMPLETE);
        assert_eq!(result["data"]["step_status"], "completed");
        assert_eq!(result["data"]["run_status"], "in_progress");
    }

    #[test]
    fn workflow_step_requires_lifecycle_order() {
        let (context, _dir) = context(PrincipalKind::Agent);
        let definition = define(&context);
        let definition_id = definition["data"]["workflow_definition_id"]
            .as_str()
            .unwrap();
        let run = trigger(&context, definition_id);
        let run_id = run["data"]["workflow_run_id"].as_str().unwrap();
        handler(WORKFLOW_STEP_COMPLETE)
            .execute(&json!({"run_id": run_id}), &context)
            .unwrap();
        let result = handler(WORKFLOW_STEP_COMPLETE).execute(&json!({"run_id": run_id}), &context);
        assert_eq!(
            result.unwrap_err().to_string(),
            "handler execution failed: workflow step deploy cannot complete before earlier steps are approved"
        );
    }

    #[test]
    fn workflow_approve_is_human_only_and_lifecycles_run() {
        let (agent_context, _dir) = context(PrincipalKind::Agent);
        let definition = define(&agent_context);
        let definition_id = definition["data"]["workflow_definition_id"]
            .as_str()
            .unwrap();
        let run = trigger(&agent_context, definition_id);
        let run_id = run["data"]["workflow_run_id"].as_str().unwrap().to_string();
        handler(WORKFLOW_STEP_COMPLETE)
            .execute(&json!({"run_id": run_id}), &agent_context)
            .unwrap();

        let human_context = ExecutionContext {
            actor: PrincipalId::now(),
            principal_kind: Some(PrincipalKind::Human),
            delegation_id: None,
            delegation_chain: None,
            workspace_path: agent_context.workspace_path.clone(),
            timestamp: chrono::Utc::now(),
        };
        let result = handler(WORKFLOW_APPROVE)
            .execute(&json!({"run_id": run_id}), &human_context)
            .unwrap();
        assert_eq!(result["operation"], WORKFLOW_APPROVE);
        assert_eq!(result["data"]["step_status"], "approved");

        let completed = handler(WORKFLOW_STEP_COMPLETE)
            .execute(&json!({"run_id": run_id}), &agent_context)
            .unwrap();
        assert_eq!(completed["data"]["run_status"], "completed");
    }

    #[test]
    fn workflow_approve_rejects_agent() {
        let (agent_context, _dir) = context(PrincipalKind::Agent);
        let definition = define(&agent_context);
        let definition_id = definition["data"]["workflow_definition_id"]
            .as_str()
            .unwrap();
        let run = trigger(&agent_context, definition_id);
        let run_id = run["data"]["workflow_run_id"].as_str().unwrap();
        let result = handler(WORKFLOW_APPROVE).execute(&json!({"run_id": run_id}), &agent_context);
        assert_eq!(result.unwrap_err(), ExecutionError::HumanOnly);
    }
}
