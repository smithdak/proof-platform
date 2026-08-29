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
    pub decision: ModelDecision,
    pub usage: ModelUsage,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ModelGatewayError {
    #[error("model provider rejected the request: {0}")]
    Request(String),
    #[error("model provider returned an invalid response: {0}")]
    InvalidResponse(String),
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
}
