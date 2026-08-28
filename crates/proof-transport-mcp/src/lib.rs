//! MCP (Model Context Protocol) transport adapter for the Proof platform.

use proof_kernel::{
    create_proof, generate_keypair, ExecutionEngine, ExecutionError, Registry, RegistryEntry,
};

const TOOL_PAGE_SIZE: usize = 20;
const BASE64_ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<McpToolAnnotations>,
}

/// MCP tool annotations derived from operation governance and consequences.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpToolAnnotations {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destructive: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idempotent: Option<bool>,
    #[serde(rename = "readOnly", skip_serializing_if = "Option::is_none")]
    pub read_only: Option<bool>,
}

/// An MCP tools/list response.
#[derive(Clone, Debug, Serialize)]
pub struct McpToolsList {
    pub tools: Vec<McpTool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// A decoded MCP cursor. Offsets remain encoded as opaque base64 strings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct McpCursor {
    pub offset: usize,
}

#[derive(Debug)]
pub enum McpCursorError {
    Invalid,
    OutOfRange,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<Value>,
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
            structured_content: None,
        }
    }

    pub fn error(text: String) -> Self {
        Self {
            content: vec![McpContent {
                content_type: "text".to_string(),
                text,
            }],
            is_error: true,
            structured_content: None,
        }
    }

    pub fn execution(result: Value, proof: Value) -> Self {
        let content = match serde_json::to_string(&result) {
            Ok(text) => McpContent {
                content_type: "text".to_string(),
                text,
            },
            Err(error) => McpContent {
                content_type: "text".to_string(),
                text: format!("Failed to serialize execution result: {error}"),
            },
        };
        Self {
            content: vec![content],
            is_error: false,
            structured_content: Some(serde_json::json!({
                "result": result,
                "proof": proof,
            })),
        }
    }
}

/// Decodes an MCP tools/list cursor.
pub fn parse_cursor(cursor: Option<&str>, tool_count: usize) -> Result<McpCursor, McpCursorError> {
    let Some(cursor) = cursor else {
        return Ok(McpCursor { offset: 0 });
    };
    if cursor.is_empty() {
        return Err(McpCursorError::Invalid);
    }
    let bytes = base64_decode(cursor).ok_or(McpCursorError::Invalid)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| McpCursorError::Invalid)?;
    let offset = text.parse::<usize>().map_err(|_| McpCursorError::Invalid)?;
    if offset >= tool_count {
        return Err(McpCursorError::OutOfRange);
    }
    Ok(McpCursor { offset })
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
            annotations: Some(tool_annotations(entry)),
        })
        .collect();
    McpToolsList {
        tools,
        next_cursor: None,
    }
}

/// Lists a page of MCP tools using opaque base64 offsets as MCP cursors.
pub fn list_tools(
    registry: &Registry,
    cursor: Option<&str>,
) -> Result<McpToolsList, McpCursorError> {
    let entries = registry.operations();
    let start = parse_cursor(cursor, entries.len())?.offset;
    let end = (start + TOOL_PAGE_SIZE).min(entries.len());
    let next_cursor = if end < entries.len() {
        Some(base64_encode(format!("{end}").as_bytes()))
    } else {
        None
    };
    let tools = entries[start..end]
        .iter()
        .map(|entry| McpTool {
            name: tool_name(entry),
            description: entry.description.clone(),
            input_schema: schema_value(&entry.input_schema),
            output_schema: schema_value(&entry.output_schema),
            annotations: Some(tool_annotations(entry)),
        })
        .collect();
    Ok(McpToolsList { tools, next_cursor })
}

/// Maps registry governance and consequence fields to MCP tool annotations.
pub fn tool_annotations(entry: &RegistryEntry) -> McpToolAnnotations {
    let read_only = consequence_matches(
        entry,
        &[
            "read", "query", "list", "inspect", "get", "review", "approval",
        ],
    );
    let destructive = !read_only
        && consequence_matches(
            entry,
            &[
                "delete", "deletion", "remove", "destroy", "revoke", "expire", "purge", "cancel",
            ],
        );
    McpToolAnnotations {
        destructive: Some(destructive),
        idempotent: Some(!destructive),
        read_only: Some(read_only),
    }
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
    let keypair = generate_keypair();
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
        delegation_chain: None,
        workspace_path,
        timestamp: chrono::Utc::now(),
    };

    match engine.execute(&entry.operation, &entry.version, &call.arguments, &context) {
        Ok(output) => {
            if let Err(error) = validate_value(&schema_value(&entry.output_schema), &output) {
                return McpToolResult::error(format!(
                    "Invalid output from {}: {}",
                    entry.operation, error
                ));
            } else {
                let signed_actor = keypair.principal_id;
                return match create_proof(
                    signed_actor,
                    context.delegation_id,
                    &entry.operation,
                    &call.arguments,
                    &output,
                    context.timestamp,
                    &keypair,
                ) {
                    Ok(proof) => match serde_json::to_value(proof) {
                        Ok(proof_value) => McpToolResult::execution(output, proof_value),
                        Err(error) => McpToolResult::error(format!(
                            "Failed to encode proof from {}: {}",
                            entry.operation, error
                        )),
                    },
                    Err(error) => McpToolResult::error(format!(
                        "Failed to generate proof for {}: {}",
                        entry.operation, error
                    )),
                };
            }
        }
        Err(ExecutionError::HumanOnly) => McpToolResult::error(format!(
            "Operation {} is human-only and cannot be executed by an agent",
            entry.operation
        )),
        Err(error) => McpToolResult::error(error.to_string()),
    }
}

fn consequence_matches(entry: &RegistryEntry, keywords: &[&str]) -> bool {
    keywords
        .iter()
        .any(|keyword| entry.consequence.to_ascii_lowercase().contains(keyword))
}

fn base64_encode(data: &[u8]) -> String {
    let mut output = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let byte_0 = chunk[0] as u32;
        let byte_1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let byte_2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let group = (byte_0 << 16) | (byte_1 << 8) | byte_2;
        output.push(BASE64_ALPHABET[(group >> 18 & 0x3f) as usize] as char);
        output.push(BASE64_ALPHABET[(group >> 12 & 0x3f) as usize] as char);
        if chunk.len() > 1 {
            output.push(BASE64_ALPHABET[(group >> 6 & 0x3f) as usize] as char);
        } else {
            output.push('=');
        }
        if chunk.len() > 2 {
            output.push(BASE64_ALPHABET[(group & 0x3f) as usize] as char);
        } else {
            output.push('=');
        }
    }
    output
}

fn base64_decode(data: &str) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut buffer = 0_u32;
    let mut bits = 0_u32;
    for character in data.chars() {
        if character == '=' {
            break;
        }
        let value = BASE64_ALPHABET
            .iter()
            .position(|candidate| *candidate as char == character)
            .map(|position| position as u32)?;
        buffer = (buffer << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            bytes.push((buffer >> bits) as u8);
        }
    }
    Some(bytes)
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
            status: proof_kernel::VersionStatus::Active,
            deprecated_since: None,
            replacement_operation: None,
        }];
        // Registry needs to be constructed from entries
        // For now test the transformation function directly
        let registry = Registry::new(entries).unwrap();
        let tools = tools_from_registry(&registry);
        assert_eq!(tools.tools.len(), 1);
        assert_eq!(tools.tools[0].name, "proof_content_v1_object_create");
        assert_eq!(
            tools.tools[0].annotations,
            Some(McpToolAnnotations {
                destructive: Some(false),
                idempotent: Some(true),
                read_only: Some(false),
            })
        );
    }
}
