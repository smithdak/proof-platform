use proof_kernel::{
    ExecutionContext, ExecutionEngine, ExecutionError, Governance, Registry, RegistryEntry,
};
use proof_transport_mcp::{
    handle_tool_call_with_keypair, list_tools, tool_annotations, tools_from_registry,
    McpCursorError, McpToolAnnotations, McpToolCall,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const WORKFLOW_OPERATIONS: &[(&str, Governance)] = &[
    ("workflow.define", Governance::AgentExecutable),
    ("workflow.trigger", Governance::AgentExecutable),
    ("workflow.step.complete", Governance::AgentExecutable),
    ("workflow.approve", Governance::HumanOnly),
];

const ANALYTICS_OPERATIONS: &[(&str, Governance)] = &[
    ("analytics.snapshot.create", Governance::AgentExecutable),
    ("analytics.query.create", Governance::AgentExecutable),
    ("analytics.query.execute", Governance::AgentExecutable),
    ("analytics.insight.approve", Governance::HumanOnly),
];

fn domain_registry_entries(domain: &str) -> Vec<RegistryEntry> {
    let registry_directory = workspace_registry_path().join(domain);
    std::fs::read_dir(&registry_directory)
        .unwrap_or_else(|error| panic!("{domain} registry directory should exist: {error}"))
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .path()
                .extension()
                .and_then(|extension| extension.to_str())
                == Some("json")
        })
        .map(|entry| entry.path())
        .filter(|path| {
            let name = path.file_name().unwrap().to_string_lossy();
            !name.contains("input") && !name.contains("output")
        })
        .map(|path| {
            let contents = std::fs::read_to_string(path).unwrap();
            serde_json::from_str(&contents).unwrap()
        })
        .collect()
}

fn domain_tools_match_registry(domain: &str, domain_prefix: &str) {
    let entries = domain_registry_entries(domain);
    let registry = Registry::new(entries).unwrap();
    let tools = tools_from_registry(&registry);

    let operations: &[(&str, Governance)] = match domain {
        "workflow" => WORKFLOW_OPERATIONS,
        "analytics" => ANALYTICS_OPERATIONS,
        other => panic!("unknown domain: {other}"),
    };

    for (operation, governance) in operations {
        let tool_name = format!("proof_{domain_prefix}_v1_{}", operation.replace('.', "_"));
        let tool = tools
            .tools
            .iter()
            .find(|tool| tool.name == tool_name)
            .unwrap_or_else(|| panic!("missing {domain} tool {tool_name}"));
        assert_eq!(tool.input_schema["type"], "object");
        assert_eq!(tool.output_schema["type"], "object");
        let human_only = *governance == Governance::HumanOnly;
        let entry = registry.find(operation, "v1").unwrap();
        let consequence = entry.consequence.as_str();
        let read_only = human_only
            || consequence.contains("query")
            || consequence.contains("read")
            || consequence.contains("approval");
        assert_eq!(tool.annotations.unwrap().read_only, Some(read_only));
        assert_eq!(tool.annotations.unwrap().destructive, Some(false));
        assert_eq!(tool.annotations.unwrap().idempotent, Some(true));
        assert_eq!(entry.governance, *governance);
    }
}

fn workspace_registry_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("registry")
}

fn registry_schema_properties(operation: &str) -> Value {
    let file_name = format!(
        "{}/{}.input.json",
        workspace_registry_path().join("commerce").display(),
        operation.replace('.', "-")
    );
    let contents = std::fs::read_to_string(file_name).unwrap();
    serde_json::from_str::<Value>(&contents).unwrap()["properties"]
        .as_object()
        .unwrap()
        .keys()
        .map(|key| (key.clone(), Value::Bool(true)))
        .collect::<serde_json::Map<String, Value>>()
        .into()
}

const AGENT_INPUT_SCHEMA: &str = r#"{
  "type": "object",
  "additionalProperties": false,
  "required": ["message"],
  "properties": {
    "message": {"type": "string", "minLength": 1}
  }
}"#;

const AGENT_OUTPUT_SCHEMA: &str = r#"{
  "type": "object",
  "additionalProperties": false,
  "required": ["echo", "handled_by"],
  "properties": {
    "echo": true,
    "handled_by": {"type": "string"}
  }
}"#;

const HUMAN_INPUT_SCHEMA: &str = r#"{
  "type": "object",
  "additionalProperties": false,
  "required": ["confirm"],
  "properties": {
    "confirm": {"type": "boolean", "const": true}
  }
}"#;

struct EchoHandler {
    operation: String,
}

#[derive(Clone, Copy)]
struct CatalogCreateHandler;

impl proof_kernel::OperationHandler for CatalogCreateHandler {
    fn operation(&self) -> &str {
        "catalog.create"
    }

    fn execute(&self, input: &Value, context: &ExecutionContext) -> Result<Value, ExecutionError> {
        let workspace = context.workspace_path.join(".proof/data/commerce/catalogs");
        std::fs::create_dir_all(&workspace).map_err(|error| {
            ExecutionError::HandlerFailed(format!("failed to create catalog store: {error}"))
        })?;
        let catalog_id = uuid::Uuid::now_v7();
        Ok(json!({
            "operation": "catalog.create",
            "data": {
                "catalog_id": catalog_id.to_string(),
                "name": input["name"],
                "description": input["description"].as_str().unwrap_or("").to_string(),
                "created_at": chrono::Utc::now().to_rfc3339(),
                "content_digest": format!("sha256:{catalog_id}"),
            }
        }))
    }
}

struct OrderCreateHandler {
    catalog_id: String,
}

impl proof_kernel::OperationHandler for OrderCreateHandler {
    fn operation(&self) -> &str {
        "order.create"
    }

    fn execute(&self, _input: &Value, context: &ExecutionContext) -> Result<Value, ExecutionError> {
        let workspace = context.workspace_path.join(".proof/data/commerce/orders");
        std::fs::create_dir_all(&workspace).map_err(|error| {
            ExecutionError::HandlerFailed(format!("failed to create order store: {error}"))
        })?;
        let order_id = uuid::Uuid::now_v7();
        Ok(json!({
            "operation": "order.create",
            "data": {
                "order_id": order_id.to_string(),
                "lines": [{"catalog_id": self.catalog_id, "name": "catalog", "quantity": 1}],
                "status": "pending",
                "created_at": chrono::Utc::now().to_rfc3339(),
                "content_digest": format!("sha256:{order_id}"),
            }
        }))
    }
}

#[derive(Clone, Copy)]
struct OrderApproveHandler;

impl proof_kernel::OperationHandler for OrderApproveHandler {
    fn operation(&self) -> &str {
        "order.approve"
    }

    fn execute(
        &self,
        _input: &Value,
        _context: &ExecutionContext,
    ) -> Result<Value, ExecutionError> {
        Ok(json!({
            "operation": "order.approve",
            "data": {
                "order_id": "not-called",
                "status": "approved",
                "approved_at": chrono::Utc::now().to_rfc3339(),
                "content_digest": "sha256:approved",
            }
        }))
    }
}

impl proof_kernel::OperationHandler for EchoHandler {
    fn operation(&self) -> &str {
        &self.operation
    }

    fn execute(&self, input: &Value, _context: &ExecutionContext) -> Result<Value, ExecutionError> {
        Ok(json!({"echo": input, "handled_by": self.operation}))
    }
}

fn registry_entry(operation: &str, governance: Governance) -> RegistryEntry {
    RegistryEntry {
        operation: operation.to_string(),
        domain: "content".to_string(),
        version: "v1".to_string(),
        action: format!("content:{}", operation.replace('.', "_")),
        description: format!("Test operation {}", operation),
        input_schema: if operation == "test.human_only" {
            HUMAN_INPUT_SCHEMA.to_string()
        } else {
            AGENT_INPUT_SCHEMA.to_string()
        },
        output_schema: AGENT_OUTPUT_SCHEMA.to_string(),
        required_authority: "delegation-grant".to_string(),
        governance,
        idempotency: "required-uuidv7".to_string(),
        consequence: consequence_for_operation(operation).to_string(),
        evidence_contract: "operation-effect-v1".to_string(),
        benchmark: None,
        status: proof_kernel::VersionStatus::Active,
        deprecated_since: None,
        replacement_operation: None,
    }
}

fn consequence_for_operation(operation: &str) -> &'static str {
    match operation {
        "test.echo" => "content-query",
        "test.human_only" => "content-approval",
        _ => "content-mutation",
    }
}

fn registry_entries() -> Vec<RegistryEntry> {
    vec![
        registry_entry("test.echo", Governance::AgentExecutable),
        registry_entry("test.human_only", Governance::HumanOnly),
    ]
}

fn engine() -> (ExecutionEngine, proof_kernel::Keypair) {
    let registry = Registry::new(registry_entries()).unwrap();
    let keypair = proof_kernel::generate_keypair();
    let mut engine = ExecutionEngine::new_with_keypair(registry.clone(), keypair.clone());
    engine.register_handler(Arc::new(EchoHandler {
        operation: "test.echo".to_string(),
    }));
    (engine, keypair)
}

fn commerce_engine(
    workspace_path: &Path,
) -> (
    ExecutionEngine,
    Registry,
    String,
    String,
    proof_kernel::Keypair,
) {
    let mut entries = registry_entries();
    entries.extend(commerce_registry_entries());
    let registry = Registry::new(entries).unwrap();
    let keypair = proof_kernel::generate_keypair();
    let mut engine = ExecutionEngine::new_with_keypair(registry.clone(), keypair.clone());
    engine.register_handler(Arc::new(EchoHandler {
        operation: "test.echo".to_string(),
    }));
    engine.register_handler(Arc::new(CatalogCreateHandler));
    let catalog = handle_tool_call_with_keypair(
        &call(
            "proof_commerce_v1_catalog_create",
            json!({"name": "Default catalog"}),
        ),
        &engine,
        &keypair,
        workspace_path.to_path_buf(),
    );
    assert!(!catalog.is_error, "{}", catalog.content[0].text);
    let catalog_id = catalog.structured_content.unwrap()["result"]["data"]["catalog_id"]
        .as_str()
        .unwrap()
        .to_string();
    engine.register_handler(Arc::new(OrderCreateHandler {
        catalog_id: catalog_id.clone(),
    }));
    let order = handle_tool_call_with_keypair(
        &call(
            "proof_commerce_v1_order_create",
            json!({"lines": [{"catalog_id": catalog_id, "name": "catalog", "quantity": 1}]}),
        ),
        &engine,
        &keypair,
        workspace_path.to_path_buf(),
    );
    assert!(!order.is_error, "{}", order.content[0].text);
    let order_id = order.structured_content.unwrap()["result"]["data"]["order_id"]
        .as_str()
        .unwrap()
        .to_string();
    engine.register_handler(Arc::new(OrderApproveHandler));
    (engine, registry, catalog_id, order_id, keypair)
}

fn commerce_registry_entries() -> Vec<RegistryEntry> {
    let registry_directory = workspace_registry_path().join("commerce");
    std::fs::read_dir(registry_directory)
        .expect("commerce registry directory should exist")
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .path()
                .extension()
                .and_then(|extension| extension.to_str())
                == Some("json")
        })
        .map(|entry| entry.path())
        .filter(|path| {
            !path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .contains("input")
                && !path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .contains("output")
        })
        .map(|path| {
            let contents = std::fs::read_to_string(path).unwrap();
            serde_json::from_str(&contents).unwrap()
        })
        .collect()
}

fn call(name: &str, arguments: Value) -> McpToolCall {
    McpToolCall {
        name: name.to_string(),
        arguments,
    }
}

#[test]
fn generated_tools_match_registry_schemas() {
    let registry = Registry::new(registry_entries()).unwrap();
    let tools = tools_from_registry(&registry);

    let agent_tool = tools
        .tools
        .iter()
        .find(|tool| tool.name == "proof_content_v1_test_echo")
        .unwrap();
    let human_tool = tools
        .tools
        .iter()
        .find(|tool| tool.name == "proof_content_v1_test_human_only")
        .unwrap();

    let expected_agent_input: Value = serde_json::from_str(AGENT_INPUT_SCHEMA).unwrap();
    let expected_agent_output: Value = serde_json::from_str(AGENT_OUTPUT_SCHEMA).unwrap();
    let expected_human_input: Value = serde_json::from_str(HUMAN_INPUT_SCHEMA).unwrap();

    assert_eq!(tools.tools.len(), 2);
    assert_eq!(agent_tool.description, "Test operation test.echo");
    assert_eq!(agent_tool.input_schema, expected_agent_input);
    assert_eq!(agent_tool.output_schema, expected_agent_output);
    assert_eq!(
        agent_tool.annotations,
        Some(tool_annotations(&registry.operations()[0]))
    );
    assert_eq!(
        agent_tool.annotations,
        Some(McpToolAnnotations {
            destructive: Some(false),
            idempotent: Some(true),
            read_only: Some(true),
        })
    );
    assert_eq!(human_tool.annotations.unwrap().read_only, Some(true));
    assert_eq!(human_tool.annotations.unwrap().destructive, Some(false));
    assert_eq!(human_tool.annotations.unwrap().idempotent, Some(true));
    assert_eq!(human_tool.description, "Test operation test.human_only");
    assert_eq!(human_tool.input_schema, expected_human_input);
    assert_eq!(human_tool.output_schema, expected_agent_output);
}

#[test]
fn agent_executable_tool_succeeds_through_mcp_flow() {
    let (engine, keypair) = engine();
    let result = handle_tool_call_with_keypair(
        &call("proof_content_v1_test_echo", json!({"message": "hello"})),
        &engine,
        &keypair,
        PathBuf::from("/tmp/proof-mcp-test"),
    );

    assert!(!result.is_error);
    assert_eq!(result.content.len(), 1);
    assert_eq!(result.content[0].content_type, "text");
    let structured = result.structured_content.unwrap();
    assert_eq!(structured["result"]["handled_by"], "test.echo");
    assert_eq!(structured["result"]["echo"]["message"], "hello");
    assert_eq!(
        structured["proof"]["body"]["actor"],
        structured["proof"]["body"]["actor"]
    );
    assert_eq!(structured["proof"]["body"]["operation"], "test.echo::v1");
    assert!(structured["proof"]["signature"].as_array().unwrap().len() == 64);
}

#[test]
fn tools_are_paginated_with_cursor_protocol() {
    let entries: Vec<RegistryEntry> = (0..25)
        .map(|index| {
            let operation = format!("test.tool_{index:02}");
            registry_entry(&operation, Governance::AgentExecutable)
        })
        .collect();
    let registry = Registry::new(entries).unwrap();

    let first_page = list_tools(&registry, None).unwrap();
    assert_eq!(first_page.tools.len(), 20);
    assert!(first_page.next_cursor.is_some());
    assert_eq!(first_page.tools[0].name, "proof_content_v1_test_tool_00");
    assert_eq!(first_page.tools[19].name, "proof_content_v1_test_tool_19");

    let second_page = list_tools(&registry, first_page.next_cursor.as_deref()).unwrap();
    assert_eq!(second_page.tools.len(), 5);
    assert!(second_page.next_cursor.is_none());
    assert_eq!(second_page.tools[0].name, "proof_content_v1_test_tool_20");
    assert_eq!(second_page.tools[4].name, "proof_content_v1_test_tool_24");

    assert!(matches!(
        list_tools(&registry, Some("not-base64")),
        Err(McpCursorError::Invalid)
    ));
    assert!(matches!(
        list_tools(&registry, Some("MjY=")),
        Err(McpCursorError::OutOfRange)
    ));
}

#[test]
fn destructive_operations_are_annotated() {
    let mut entry = registry_entry("test.delete", Governance::AgentExecutable);
    entry.operation = "test.delete".to_string();
    entry.idempotency = "not-required".to_string();
    entry.consequence = "content-deletion".to_string();
    let annotations = tool_annotations(&entry);
    assert_eq!(
        annotations,
        McpToolAnnotations {
            destructive: Some(true),
            idempotent: Some(false),
            read_only: Some(false),
        }
    );
}

#[test]
fn commerce_registry_tools_have_commerce_schema_and_governance() {
    let registry = Registry::new(commerce_registry_entries()).unwrap();
    let tools = tools_from_registry(&registry);
    let expected = [
        (
            "proof_commerce_v1_catalog_create",
            "catalog.create",
            Some(false),
            Some(true),
        ),
        (
            "proof_commerce_v1_catalog_update",
            "catalog.update",
            Some(false),
            Some(true),
        ),
        (
            "proof_commerce_v1_order_create",
            "order.create",
            Some(false),
            Some(true),
        ),
        (
            "proof_commerce_v1_order_approve",
            "order.approve",
            Some(true),
            Some(true),
        ),
        (
            "proof_commerce_v1_order_fulfill",
            "order.fulfill",
            Some(false),
            Some(true),
        ),
    ];

    for (tool_name, operation, read_only, idempotent) in expected {
        let tool = tools
            .tools
            .iter()
            .find(|tool| tool.name == tool_name)
            .unwrap_or_else(|| panic!("missing commerce tool {tool_name}"));
        assert_eq!(tool.input_schema["type"], "object");
        assert_eq!(tool.output_schema["type"], "object");
        assert_eq!(tool.annotations.unwrap().read_only, read_only);
        assert_eq!(tool.annotations.unwrap().idempotent, idempotent);
        assert_eq!(
            tool.input_schema["properties"]
                .as_object()
                .unwrap()
                .keys()
                .collect::<Vec<_>>(),
            registry_schema_properties(operation)
                .as_object()
                .unwrap()
                .keys()
                .collect::<Vec<_>>()
        );
        let human_only = operation == "order.approve";
        assert_eq!(
            registry.find(operation, "v1").unwrap().governance,
            if human_only {
                Governance::HumanOnly
            } else {
                Governance::AgentExecutable
            }
        );
    }
}

#[test]
fn commerce_lifecycle_is_executable_and_approval_is_human_only() {
    let workspace_path = std::env::temp_dir().join(format!(
        "proof-mcp-commerce-{}",
        uuid::Uuid::now_v7().simple()
    ));
    let (engine, registry, _catalog_id, order_id, keypair) = commerce_engine(&workspace_path);

    let approval = handle_tool_call_with_keypair(
        &call(
            "proof_commerce_v1_order_approve",
            json!({"order_id": order_id}),
        ),
        &engine,
        &keypair,
        workspace_path.clone(),
    );
    assert!(approval.is_error);
    assert_eq!(
        approval.content[0].text,
        "Operation order.approve is human-only and cannot be executed by an agent"
    );
    let commerce_tools = tools_from_registry(&registry);
    let approval_tool = commerce_tools
        .tools
        .iter()
        .find(|tool| tool.name == "proof_commerce_v1_order_approve")
        .unwrap();
    assert_eq!(approval_tool.annotations.unwrap().read_only, Some(true));
    assert_eq!(approval_tool.annotations.unwrap().idempotent, Some(true));
    assert_eq!(approval_tool.annotations.unwrap().destructive, Some(false));
    std::fs::remove_dir_all(workspace_path).ok();
}

#[test]
fn mcp_rejects_human_only_tool_before_handler_dispatch() {
    let (engine, keypair) = engine();
    let result = handle_tool_call_with_keypair(
        &call("proof_content_v1_test_human_only", json!({"confirm": true})),
        &engine,
        &keypair,
        PathBuf::from("/tmp/proof-mcp-test"),
    );

    assert!(result.is_error);
    assert_eq!(
        result.content[0].text,
        "Operation test.human_only is human-only and cannot be executed by an agent"
    );
}

#[test]
fn mcp_validates_input_and_output_against_registry_schemas() {
    let (base_engine, base_keypair) = engine();
    let invalid_input = handle_tool_call_with_keypair(
        &call("proof_content_v1_test_echo", json!({"message": ""})),
        &base_engine,
        &base_keypair,
        PathBuf::from("/tmp/proof-mcp-test"),
    );
    assert!(invalid_input.is_error);
    assert!(invalid_input.content[0].text.contains("Invalid input"));

    let (mut invalid_engine, invalid_keypair) = engine();
    invalid_engine.register_handler(Arc::new(BadOutputHandler));
    let invalid_output = handle_tool_call_with_keypair(
        &call("proof_content_v1_test_echo", json!({"message": "hello"})),
        &invalid_engine,
        &invalid_keypair,
        PathBuf::from("/tmp/proof-mcp-test"),
    );
    assert!(invalid_output.is_error);
    assert!(invalid_output.content[0].text.contains("Invalid output"));
}

#[test]
fn mcp_rejects_unknown_tool() {
    let (engine, keypair) = engine();
    let result = handle_tool_call_with_keypair(
        &call("proof_content_v1_missing", json!({})),
        &engine,
        &keypair,
        PathBuf::from("/tmp/proof-mcp-test"),
    );

    assert!(result.is_error);
    assert_eq!(
        result.content[0].text,
        "Unknown operation: proof_content_v1_missing"
    );
}

#[test]
fn workflow_registry_tools_have_schema_and_governance() {
    domain_tools_match_registry("workflow", "workflow");
}

#[test]
fn analytics_registry_tools_have_schema_and_governance() {
    domain_tools_match_registry("analytics", "analytics");
}

struct BadOutputHandler;

impl proof_kernel::OperationHandler for BadOutputHandler {
    fn operation(&self) -> &str {
        "test.echo"
    }

    fn execute(
        &self,
        _input: &Value,
        _context: &ExecutionContext,
    ) -> Result<Value, ExecutionError> {
        Ok(json!({"unexpected": true}))
    }
}
