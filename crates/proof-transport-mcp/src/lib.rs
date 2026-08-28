//! MCP (Model Context Protocol) transport adapter for the Proof platform.

use proof_kernel::{ExecutionEngine, ExecutionError, Registry};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// An MCP tool definition derived from a Proof operation registry entry.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
    #[serde(rename = "outputSchema")]
    pub output_schema: Value,
}

/// An MCP tools/list response.
#[derive(Clone, Debug, Serialize)]
pub struct McpToolsList {
    pub tools: Vec<McpTool>,
}

/// An MCP tool call request.
#[derive(Clone, Debug, Deserialize)]
pub struct McpToolCall {
    pub name: String,
    pub arguments: Value,
}

/// An MCP tool call result.
#[derive(Clone, Debug, Serialize)]
pub struct McpToolResult {
    pub content: Vec<McpContent>,
    #[serde(rename = "isError", skip_serializing_if = "std::ops::Not::not")]
    pub is_error: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct McpContent {
    #[serde(rename = "type")]
    pub content_type: String,
    pub text: String,
}

impl McpToolResult {
    pub fn success(text: String) -> Self {
        Self {
            content: vec![McpContent {
                content_type: "text".to_string(),
                text,
            }],
            is_error: false,
        }
    }

    pub fn error(text: String) -> Self {
        Self {
            content: vec![McpContent {
                content_type: "text".to_string(),
                text,
            }],
            is_error: true,
        }
    }
}

/// Generates MCP tool definitions from Proof operation registry entries.
///
/// Registry schema fields may contain either a schema reference or an inline
/// JSON Schema document. Inline documents are emitted directly so MCP clients
/// receive the same contract recorded by the registry.
pub fn tools_from_registry(registry: &Registry) -> McpToolsList {
    let tools: Vec<McpTool> = registry
        .operations()
        .iter()
        .map(|entry| McpTool {
            name: format!(
                "proof_{}_{}_{}",
                entry.domain,
                entry.version,
                entry.operation.replace('.', "_")
            ),
            description: entry.description.clone(),
            input_schema: schema_value(&entry.input_schema),
            output_schema: schema_value(&entry.output_schema),
        })
        .collect();
    McpToolsList { tools }
}

/// Handles an MCP tool call through the kernel execution engine.
///
/// The engine is authoritative for registry lookup, schema-independent
/// governance checks, handler dispatch, and human-only rejection.
pub fn handle_tool_call(
    call: &McpToolCall,
    engine: &ExecutionEngine,
    actor: proof_kernel::PrincipalId,
    workspace_path: std::path::PathBuf,
) -> McpToolResult {
    let Some(entry) = engine
        .operations()
        .iter()
        .find(|candidate| tool_name(candidate) == call.name)
    else {
        return McpToolResult::error(format!("Unknown operation: {}", call.name));
    };

    if let Err(error) = validate_value(&schema_value(&entry.input_schema), &call.arguments) {
        return McpToolResult::error(format!("Invalid input for {}: {}", entry.operation, error));
    }

    let context = proof_kernel::ExecutionContext {
        actor,
        delegation_id: None,
        workspace_path,
        timestamp: chrono::Utc::now(),
    };

    match engine.execute(&entry.operation, &entry.version, &call.arguments, &context) {
        Ok(output) => {
            if let Err(error) = validate_value(&schema_value(&entry.output_schema), &output) {
                McpToolResult::error(format!(
                    "Invalid output from {}: {}",
                    entry.operation, error
                ))
            } else {
                match serde_json::to_string(&output) {
                    Ok(text) => McpToolResult::success(text),
                    Err(error) => McpToolResult::error(format!(
                        "Failed to encode output from {}: {}",
                        entry.operation, error
                    )),
                }
            }
        }
        Err(ExecutionError::HumanOnly) => McpToolResult::error(format!(
            "Operation {} is human-only and cannot be executed by an agent",
            entry.operation
        )),
        Err(error) => McpToolResult::error(error.to_string()),
    }
}

fn tool_name(entry: &proof_kernel::RegistryEntry) -> String {
    format!(
        "proof_{}_{}_{}",
        entry.domain,
        entry.version,
        entry.operation.replace('.', "_")
    )
}

fn schema_value(schema: &str) -> Value {
    if schema.trim_start().starts_with('{') {
        serde_json::from_str(schema).unwrap_or_else(|_| Value::String(schema.to_string()))
    } else {
        Value::String(schema.to_string())
    }
}

fn validate_value(schema: &Value, value: &Value) -> Result<(), String> {
    let compiled =
        jsonschema::Validator::new(schema).map_err(|error| format!("invalid schema: {error}"))?;
    let errors: Vec<String> = compiled
        .iter_errors(value)
        .map(|error| error.to_string())
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proof_kernel::RegistryEntry;

    #[test]
    fn tools_generated_from_registry() {
        let entries = vec![RegistryEntry {
            operation: "object.create".to_string(),
            domain: "content".to_string(),
            version: "v1".to_string(),
            action: "content:object_create".to_string(),
            description: "Create an object".to_string(),
            input_schema: "a.json".to_string(),
            output_schema: "b.json".to_string(),
            required_authority: "delegation-grant".to_string(),
            governance: proof_kernel::Governance::AgentExecutable,
            idempotency: "required-uuidv7".to_string(),
            consequence: "content-mutation".to_string(),
            evidence_contract: "operation-effect-v1".to_string(),
            benchmark: None,
        }];
        // Registry needs to be constructed from entries
        // For now test the transformation function directly
        let registry = Registry::new(entries).unwrap();
        let tools = tools_from_registry(&registry);
        assert_eq!(tools.tools.len(), 1);
        assert_eq!(tools.tools[0].name, "proof_content_v1_object_create");
    }
}
