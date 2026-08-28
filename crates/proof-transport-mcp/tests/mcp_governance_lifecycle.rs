use proof_kernel::{
    ExecutionContext, ExecutionEngine, ExecutionError, Governance, PrincipalId, Registry,
    RegistryEntry,
};
use proof_transport_mcp::{
    handle_tool_call, list_tools, tool_annotations, tools_from_registry, McpCursorError,
    McpToolAnnotations, McpToolCall,
};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;

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

fn engine() -> ExecutionEngine {
    let registry = Registry::new(registry_entries()).unwrap();
    let mut engine = ExecutionEngine::new(registry);
    engine.register_handler(Arc::new(EchoHandler {
        operation: "test.echo".to_string(),
    }));
    engine
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
    let engine = engine();
    let result = handle_tool_call(
        &call("proof_content_v1_test_echo", json!({"message": "hello"})),
        &engine,
        PrincipalId::now(),
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
    assert!(structured["proof"]["body"]["operation"] == "test.echo");
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
fn mcp_rejects_human_only_tool_before_handler_dispatch() {
    let engine = engine();
    let result = handle_tool_call(
        &call("proof_content_v1_test_human_only", json!({"confirm": true})),
        &engine,
        PrincipalId::now(),
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
    let base_engine = engine();
    let invalid_input = handle_tool_call(
        &call("proof_content_v1_test_echo", json!({"message": ""})),
        &base_engine,
        PrincipalId::now(),
        PathBuf::from("/tmp/proof-mcp-test"),
    );
    assert!(invalid_input.is_error);
    assert!(invalid_input.content[0].text.contains("Invalid input"));

    let mut invalid_engine = engine();
    invalid_engine.register_handler(Arc::new(BadOutputHandler));
    let invalid_output = handle_tool_call(
        &call("proof_content_v1_test_echo", json!({"message": "hello"})),
        &invalid_engine,
        PrincipalId::now(),
        PathBuf::from("/tmp/proof-mcp-test"),
    );
    assert!(invalid_output.is_error);
    assert!(invalid_output.content[0].text.contains("Invalid output"));
}

#[test]
fn mcp_rejects_unknown_tool() {
    let engine = engine();
    let result = handle_tool_call(
        &call("proof_content_v1_missing", json!({})),
        &engine,
        PrincipalId::now(),
        PathBuf::from("/tmp/proof-mcp-test"),
    );

    assert!(result.is_error);
    assert_eq!(
        result.content[0].text,
        "Unknown operation: proof_content_v1_missing"
    );
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
