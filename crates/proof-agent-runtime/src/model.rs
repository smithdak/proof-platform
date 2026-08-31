//! Provider-neutral model request and tool-call contracts.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentFunctionTool {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    pub operation: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ModelInput {
    Goal { text: String },
    ToolOutput { call_id: String, output: Value },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelTurnRequest {
    pub model: String,
    pub instructions: String,
    pub input: ModelInput,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_response_id: Option<String>,
    pub tools: Vec<AgentFunctionTool>,
    pub max_output_tokens: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_microusd: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ModelDecision {
    ToolCall {
        call_id: String,
        name: String,
        arguments: Value,
    },
    Finish {
        output: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelTurn {
    pub response_id: String,
    /// Exact provider-returned model, when the gateway exposes it. Live runs
    /// require it; legacy gateways may omit it for compatibility.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub returned_model: Option<String>,
    /// Domain-separated digest of the complete parsed provider response body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_body_digest: Option<proof_kernel::ContentDigest>,
    pub decision: ModelDecision,
    pub usage: ModelUsage,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ModelGatewayError {
    #[error("model provider rejected the request: {0}")]
    Request(String),
    #[error("model provider returned an invalid response: {0}")]
    InvalidResponse(String),
    /// A local transport failure for which the gateway can certify that no
    /// request byte was written. This is deliberately distinct from a normal
    /// request error: the live runtime may consume its one retry only here.
    #[error("model provider failed before request bytes were sent: {0}")]
    CertifiedNoBytes(String),
    /// An explicit 429 response that did not create a Responses object.
    #[error("model provider explicitly rejected the request as retryable: {0}")]
    Explicit429(String),
    /// An explicit deterministic provider/configuration rejection that cannot
    /// have created a usable Responses object.
    #[error("model provider rejected the request terminally: {0}")]
    Terminal(String),
    /// The request may have crossed the provider boundary. Retrying it could
    /// repeat a governed consequence, so the live runtime seals it ambiguous.
    #[error("model provider outcome is ambiguous: {0}")]
    Ambiguous(String),
}

pub trait ModelGateway: Send + Sync {
    fn provider(&self) -> &str;
    fn complete(&self, request: &ModelTurnRequest) -> Result<ModelTurn, ModelGatewayError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_input_serializes_as_a_tagged_contract() {
        let input = ModelInput::ToolOutput {
            call_id: "call_1".to_string(),
            output: serde_json::json!({"ok": true}),
        };

        assert_eq!(
            serde_json::to_value(input).unwrap(),
            serde_json::json!({
                "type": "tool_output",
                "call_id": "call_1",
                "output": {"ok": true}
            })
        );
    }

    #[test]
    fn legacy_public_model_types_ignore_unknown_fields() {
        let tool: AgentFunctionTool = serde_json::from_value(serde_json::json!({
            "name": "legacy_tool",
            "description": "legacy",
            "parameters": {},
            "operation": "legacy.call",
            "version": "v1",
            "future_tool_field": true
        }))
        .unwrap();
        assert_eq!(tool.name, "legacy_tool");

        let request: ModelTurnRequest = serde_json::from_value(serde_json::json!({
            "model": "legacy-model",
            "instructions": "legacy",
            "input": {"type": "goal", "text": "go", "future_input_field": true},
            "tools": [tool],
            "max_output_tokens": 32,
            "future_request_field": true
        }))
        .unwrap();
        assert!(matches!(request.input, ModelInput::Goal { ref text } if text == "go"));

        let usage: ModelUsage = serde_json::from_value(serde_json::json!({
            "input_tokens": 0,
            "output_tokens": 0,
            "total_tokens": 0,
            "future_usage_field": true
        }))
        .unwrap();
        assert_eq!(usage, ModelUsage::default());
    }
}
