//! OpenAI Responses API model gateway.

use std::time::Duration;

use reqwest::blocking::Client;
use serde_json::{json, Value};

use crate::model::{
    ModelDecision, ModelGateway, ModelGatewayError, ModelInput, ModelTurn, ModelTurnRequest,
    ModelUsage,
};

pub const DEFAULT_OPENAI_BASE_URL: &str = "https://api.openai.com/v1";

pub struct OpenAiResponsesGateway {
    api_key: String,
    base_url: String,
    client: Client,
}

impl OpenAiResponsesGateway {
    pub fn new(
        api_key: impl Into<String>,
        base_url: impl Into<String>,
    ) -> Result<Self, ModelGatewayError> {
        let api_key = api_key.into();
        if api_key.trim().is_empty() {
            return Err(ModelGatewayError::Request(
                "OPENAI_API_KEY must not be empty".to_string(),
            ));
        }
        let base_url = base_url.into().trim_end_matches('/').to_string();
        if base_url.trim().is_empty() {
            return Err(ModelGatewayError::Request(
                "OpenAI base URL must not be empty".to_string(),
            ));
        }
        let client = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|error| ModelGatewayError::Request(error.to_string()))?;
        Ok(Self {
            api_key,
            base_url,
            client,
        })
    }
}

impl ModelGateway for OpenAiResponsesGateway {
    fn provider(&self) -> &str {
        "openai"
    }

    fn complete(&self, request: &ModelTurnRequest) -> Result<ModelTurn, ModelGatewayError> {
        let response = self
            .client
            .post(format!("{}/responses", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&request_body(request)?)
            .send()
            .map_err(|error| ModelGatewayError::Request(error.to_string()))?;
        let status = response.status();
        let body = response
            .text()
            .map_err(|error| ModelGatewayError::Request(error.to_string()))?;
        let value: Value = serde_json::from_str(&body).map_err(|error| {
            ModelGatewayError::InvalidResponse(format!(
                "HTTP {status} returned non-JSON content: {error}"
            ))
        })?;
        if !status.is_success() {
            let message = value
                .pointer("/error/message")
                .and_then(Value::as_str)
                .unwrap_or("unknown OpenAI API error");
            return Err(ModelGatewayError::Request(format!(
                "HTTP {status}: {message}"
            )));
        }
        parse_response(&value)
    }
}

fn request_body(request: &ModelTurnRequest) -> Result<Value, ModelGatewayError> {
    let input = match &request.input {
        ModelInput::Goal { text } => Value::String(text.clone()),
        ModelInput::ToolOutput { call_id, output } => json!([{
            "type": "function_call_output",
            "call_id": call_id,
            "output": serde_json::to_string(output).map_err(|error| {
                ModelGatewayError::Request(format!("could not encode tool output: {error}"))
            })?,
        }]),
    };
    let tools = request
        .tools
        .iter()
        .map(|tool| {
            json!({
                "type": "function",
                "name": tool.name,
                "description": tool.description,
                "parameters": tool.parameters,
                "strict": false,
            })
        })
        .collect::<Vec<_>>();
    let mut body = json!({
        "model": request.model,
        "instructions": request.instructions,
        "input": input,
        "tools": tools,
        "tool_choice": "auto",
        "parallel_tool_calls": false,
        "store": true,
        "max_output_tokens": request.max_output_tokens,
    });
    if let Some(previous_response_id) = &request.previous_response_id {
        body["previous_response_id"] = Value::String(previous_response_id.clone());
    }
    Ok(body)
}

fn parse_response(value: &Value) -> Result<ModelTurn, ModelGatewayError> {
    let response_id = required_string(value, "id")?;
    let status = required_string(value, "status")?;
    if status != "completed" {
        return Err(ModelGatewayError::InvalidResponse(format!(
            "response {response_id} has non-completed status {status}"
        )));
    }
    let output = value
        .get("output")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            ModelGatewayError::InvalidResponse("response output must be an array".to_string())
        })?;
    let mut tool_calls = Vec::new();
    let mut text = String::new();
    for item in output {
        match item.get("type").and_then(Value::as_str) {
            Some("function_call") => {
                let arguments = required_string(item, "arguments")?;
                let arguments = serde_json::from_str(arguments).map_err(|error| {
                    ModelGatewayError::InvalidResponse(format!(
                        "function arguments are not valid JSON: {error}"
                    ))
                })?;
                tool_calls.push(ModelDecision::ToolCall {
                    call_id: required_string(item, "call_id")?.to_string(),
                    name: required_string(item, "name")?.to_string(),
                    arguments,
                });
            }
            Some("message") => {
                if let Some(content) = item.get("content").and_then(Value::as_array) {
                    for part in content {
                        if part.get("type").and_then(Value::as_str) == Some("output_text") {
                            if let Some(part_text) = part.get("text").and_then(Value::as_str) {
                                text.push_str(part_text);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    let decision = match tool_calls.len() {
        0 if !text.trim().is_empty() => ModelDecision::Finish { output: text },
        0 => {
            return Err(ModelGatewayError::InvalidResponse(
                "response contains neither a function call nor output text".to_string(),
            ))
        }
        1 => tool_calls.pop().expect("one tool call checked above"),
        count => {
            return Err(ModelGatewayError::InvalidResponse(format!(
                "response contains {count} parallel function calls"
            )))
        }
    };
    let usage = value.get("usage").unwrap_or(&Value::Null);
    let input_tokens = optional_u64(usage, "input_tokens")?;
    let output_tokens = optional_u64(usage, "output_tokens")?;
    let total_tokens = match usage.get("total_tokens") {
        Some(value) => value.as_u64().ok_or_else(|| {
            ModelGatewayError::InvalidResponse(
                "usage.total_tokens must be an unsigned integer".to_string(),
            )
        })?,
        None => input_tokens.checked_add(output_tokens).ok_or_else(|| {
            ModelGatewayError::InvalidResponse("token usage overflow".to_string())
        })?,
    };
    Ok(ModelTurn {
        response_id: response_id.to_string(),
        decision,
        usage: ModelUsage {
            input_tokens,
            output_tokens,
            total_tokens,
            cost_microusd: None,
        },
    })
}

fn required_string<'a>(value: &'a Value, key: &str) -> Result<&'a str, ModelGatewayError> {
    value.get(key).and_then(Value::as_str).ok_or_else(|| {
        ModelGatewayError::InvalidResponse(format!("response field {key} must be a string"))
    })
}

fn optional_u64(value: &Value, key: &str) -> Result<u64, ModelGatewayError> {
    match value.get(key) {
        Some(value) => value.as_u64().ok_or_else(|| {
            ModelGatewayError::InvalidResponse(format!("usage.{key} must be an unsigned integer"))
        }),
        None => Ok(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::AgentFunctionTool;

    fn request(input: ModelInput) -> ModelTurnRequest {
        ModelTurnRequest {
            model: "test-model".to_string(),
            instructions: "Use the supplied tools.".to_string(),
            input,
            previous_response_id: Some("resp_previous".to_string()),
            tools: vec![AgentFunctionTool {
                name: "proof_commerce_v1_catalog_create".to_string(),
                description: "Create a catalog".to_string(),
                parameters: json!({"type": "object"}),
                operation: "catalog.create".to_string(),
                version: "v1".to_string(),
            }],
            max_output_tokens: 512,
        }
    }

    #[test]
    fn gateway_validates_configuration() {
        assert!(OpenAiResponsesGateway::new("", DEFAULT_OPENAI_BASE_URL).is_err());
        assert!(OpenAiResponsesGateway::new("test-key", "").is_err());
        let gateway = OpenAiResponsesGateway::new("test-key", "https://example.test/v1/").unwrap();
        assert_eq!(gateway.provider(), "openai");
        assert_eq!(gateway.base_url, "https://example.test/v1");
    }

    #[test]
    fn continuation_request_resends_instructions_and_tool_output() {
        let body = request_body(&request(ModelInput::ToolOutput {
            call_id: "call_1".to_string(),
            output: json!({"ok": true}),
        }))
        .unwrap();

        assert_eq!(body["instructions"], "Use the supplied tools.");
        assert_eq!(body["previous_response_id"], "resp_previous");
        assert_eq!(body["parallel_tool_calls"], false);
        assert_eq!(body["input"][0]["type"], "function_call_output");
        assert_eq!(body["input"][0]["call_id"], "call_1");
        assert_eq!(body["input"][0]["output"], r#"{"ok":true}"#);
        assert_eq!(body["tools"][0]["name"], "proof_commerce_v1_catalog_create");
    }

    #[test]
    fn parses_function_call_and_usage() {
        let turn = parse_response(&json!({
            "id": "resp_1",
            "status": "completed",
            "output": [{
                "type": "function_call",
                "call_id": "call_1",
                "name": "proof_commerce_v1_catalog_create",
                "arguments": "{\"name\":\"Spring\"}"
            }],
            "usage": {"input_tokens": 12, "output_tokens": 5, "total_tokens": 17}
        }))
        .unwrap();

        assert_eq!(turn.response_id, "resp_1");
        assert_eq!(turn.usage.total_tokens, 17);
        assert!(matches!(
            turn.decision,
            ModelDecision::ToolCall { ref call_id, ref arguments, .. }
                if call_id == "call_1" && arguments["name"] == "Spring"
        ));
    }

    #[test]
    fn parses_final_output_text() {
        let turn = parse_response(&json!({
            "id": "resp_2",
            "status": "completed",
            "output": [{
                "type": "message",
                "content": [
                    {"type": "output_text", "text": "Release "},
                    {"type": "output_text", "text": "ready."}
                ]
            }]
        }))
        .unwrap();

        assert_eq!(
            turn.decision,
            ModelDecision::Finish {
                output: "Release ready.".to_string()
            }
        );
    }
}
