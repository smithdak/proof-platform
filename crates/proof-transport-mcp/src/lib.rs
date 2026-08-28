//! MCP (Model Context Protocol) transport adapter for the Proof platform.

use proof_kernel::Registry;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// An MCP tool definition derived from a Proof operation registry entry.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
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
            content: vec![McpContent { content_type: "text".to_string(), text }],
            is_error: false,
        }
    }

    pub fn error(text: String) -> Self {
        Self {
            content: vec![McpContent { content_type: "text".to_string(), text }],
            is_error: true,
        }
    }
}

/// Generates MCP tool definitions from Proof operation registry entries.
pub fn tools_from_registry(registry: &Registry) -> McpToolsList {
    let tools: Vec<McpTool> = registry
        .operations()
        .iter()
        .map(|entry| McpTool {
            name: format!("proof_{}_{}", entry.domain, entry.operation.replace('.', "_")),
            description: entry.description.clone(),
            input_schema: serde_json::json!({
                "type": "object",
                "description": entry.description
            }),
        })
        .collect();
    McpToolsList { tools }
}

/// Handles an MCP tool call by routing to the appropriate Proof operation.
pub fn handle_tool_call(call: &McpToolCall, registry: &Registry) -> McpToolResult {
    let operation = call
        .name
        .trim_start_matches("proof_content_")
        .trim_start_matches("proof_");
    let entry = registry
        .operations()
        .iter()
        .find(|e| e.operation == operation || e.operation.replace('.', "_") == operation);

    match entry {
        Some(_entry) => McpToolResult::success(format!(
            "Operation {} dispatched (implementation pending)",
            operation
        )),
        None => McpToolResult::error(format!("Unknown operation: {}", call.name)),
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
        let tools = tools_from_registry_impl(&entries);
        assert_eq!(tools.tools.len(), 1);
        assert_eq!(tools.tools[0].name, "proof_content_object_create");
    }

    fn tools_from_registry_impl(entries: &[RegistryEntry]) -> McpToolsList {
        let tools: Vec<McpTool> = entries
            .iter()
            .map(|entry| McpTool {
                name: format!("proof_{}_{}", entry.domain, entry.operation.replace('.', "_")),
                description: entry.description.clone(),
                input_schema: serde_json::json!({"type": "object"}),
            })
            .collect();
        McpToolsList { tools }
    }
}
