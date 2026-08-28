//! MCP (Model Context Protocol) transport adapter for the Proof platform.

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
pub fn tools_from_registry(
    entries: &[proof_kernel::RegistryEntry],
) -> Vec<McpTool> {
    entries
        .iter()
        .map(|entry| McpTool {
            name: format!("proof.{}", entry.operation),
            description: entry.description.clone(),
            input_schema: serde_json::json!({
                "type": "object",
                "description": entry.description
            }),
        })
        .collect()
}
