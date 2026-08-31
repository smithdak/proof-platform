//! OpenAI Responses API model gateway.

use std::time::Duration;

use proof_kernel::{canonicalize, digest, ArtifactKind};
use reqwest::blocking::Client;
use serde_json::{json, Value};

use crate::model::{
    ModelDecision, ModelGateway, ModelGatewayError, ModelInput, ModelTurn, ModelTurnRequest,
    ModelUsage,
};

pub const DEFAULT_OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
const LIVE_MODEL: &str = "gpt-5.6-sol";
const LIVE_TOOL_NAME: &str = "proof_content_v2_release_publish";

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
            .bytes()
            .map_err(|error| ModelGatewayError::Request(error.to_string()))?;
        if !status.is_success() {
            // Classification is deliberately based on the HTTP response before
            // attempting JSON parsing.  OpenAI (and intermediaries) may return
            // an HTML/plain-text 400/401/403/404; those are deterministic
            // terminal rejections, not malformed successful completions.
            return Err(classify_http_failure(status.as_u16(), &body));
        }
        parse_response_body_for_request(&body, request).map_err(|error| match error {
            ModelGatewayError::InvalidResponse(detail) => ModelGatewayError::InvalidResponse(
                format!("HTTP {status} returned invalid success evidence: {detail}"),
            ),
            other => other,
        })
    }
}

fn parse_response_body_for_request(
    body: &[u8],
    request: &ModelTurnRequest,
) -> Result<ModelTurn, ModelGatewayError> {
    if is_exact_live_request(request) {
        let value: Value = serde_json::from_slice(body).map_err(|error| {
            ModelGatewayError::InvalidResponse(format!("response is not JSON: {error}"))
        })?;
        let usage = value
            .get("usage")
            .and_then(Value::as_object)
            .ok_or_else(|| {
                ModelGatewayError::InvalidResponse(
                    "live response usage must be a present object".to_string(),
                )
            })?;
        let input = usage.get("input_tokens").and_then(Value::as_u64);
        let output = usage.get("output_tokens").and_then(Value::as_u64);
        let total = usage.get("total_tokens").and_then(Value::as_u64);
        if input.is_none_or(|value| value == 0)
            || output.is_none_or(|value| value == 0)
            || total.is_none_or(|value| value == 0)
            || input.and_then(|input| output.and_then(|output| input.checked_add(output))) != total
        {
            return Err(ModelGatewayError::InvalidResponse(
                "live usage must contain present nonzero exact input/output/total counters"
                    .to_string(),
            ));
        }
    }
    parse_response_body(body)
}

fn classify_http_failure(status: u16, body: &[u8]) -> ModelGatewayError {
    let value = serde_json::from_slice::<Value>(body).ok();
    let message = value
        .as_ref()
        .and_then(|value| value.pointer("/error/message"))
        .and_then(Value::as_str)
        .unwrap_or("provider returned a non-JSON error body");
    let detail = format!("HTTP {status}: {message}");
    if status == 429 && value.as_ref().is_none_or(|value| value.get("id").is_none()) {
        ModelGatewayError::Explicit429(detail)
    } else if matches!(status, 400 | 401 | 403 | 404) {
        ModelGatewayError::Terminal(detail)
    } else {
        ModelGatewayError::Ambiguous(detail)
    }
}

pub(crate) fn request_body(request: &ModelTurnRequest) -> Result<Value, ModelGatewayError> {
    let exact_live = is_exact_live_request(request);
    let input = match &request.input {
        ModelInput::Goal { text } => Value::String(text.clone()),
        ModelInput::ToolOutput { call_id, output } => json!([{
            "type": "function_call_output",
            "call_id": call_id,
            "output": if exact_live {
                canonicalize(output)
                    .map_err(|error| ModelGatewayError::Request(format!("could not canonicalize tool output: {error}")))?
                    .to_string()
            } else {
                serde_json::to_string(output).map_err(|error| {
                    ModelGatewayError::Request(format!("could not encode tool output: {error}"))
                })?
            },
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
                // The frozen E0001 function declaration is strict; legacy
                // tools retain their historical non-strict declaration.
                "strict": exact_live,
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
    if exact_live {
        body["stream"] = Value::Bool(false);
        body["background"] = Value::Bool(false);
        body["service_tier"] = Value::String("default".to_string());
        body["previous_response_id"] = request
            .previous_response_id
            .as_ref()
            .map(|id| Value::String(id.clone()))
            .unwrap_or(Value::Null);
    } else if let Some(previous_response_id) = &request.previous_response_id {
        body["previous_response_id"] = Value::String(previous_response_id.clone());
    }
    Ok(body)
}

fn is_exact_live_request(request: &ModelTurnRequest) -> bool {
    request.model == LIVE_MODEL
        && request.tools.len() == 1
        && request.tools[0].name == LIVE_TOOL_NAME
        && request.tools[0].operation == "release.publish"
        && request.tools[0].version == "v2"
}

fn parse_response_body(body: &[u8]) -> Result<ModelTurn, ModelGatewayError> {
    let value: Value = serde_json::from_slice(body).map_err(|error| {
        ModelGatewayError::InvalidResponse(format!("response is not JSON: {error}"))
    })?;
    // Bind the original byte slice injectively without decoding or
    // normalizing it. The canonical array is a domain-separated digest input,
    // not a reserialization of the provider JSON.
    let exact_bytes = Value::Array(body.iter().map(|byte| json!(*byte)).collect());
    let body_digest = digest(
        ArtifactKind::Generic,
        &canonicalize(&exact_bytes).map_err(|error| {
            ModelGatewayError::InvalidResponse(format!(
                "response bytes cannot be bound exactly: {error}"
            ))
        })?,
    );
    parse_response(&value, body_digest)
}

fn parse_response(
    value: &Value,
    response_body_digest: proof_kernel::ContentDigest,
) -> Result<ModelTurn, ModelGatewayError> {
    let response_id = required_string(value, "id")?;
    if response_id.is_empty() {
        return Err(ModelGatewayError::InvalidResponse(
            "response field id must not be empty".to_string(),
        ));
    }
    let returned_model = value
        .get("model")
        .and_then(Value::as_str)
        .map(str::to_string);
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
    // Preserve the shared gateway's legacy tolerance. The sealed live runtime
    // separately requires present, nonzero, exact-equality usage evidence and
    // classifies zero/partial/synthesized values as ambiguous.
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
        returned_model,
        response_body_digest: Some(response_body_digest),
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

    fn parsed(value: Value) -> Result<ModelTurn, ModelGatewayError> {
        parse_response_body(&serde_json::to_vec(&value).unwrap())
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
        assert_eq!(body["tools"][0]["strict"], false);
        assert!(body.get("stream").is_none());
        assert!(body.get("background").is_none());
        assert!(body.get("service_tier").is_none());
    }

    #[test]
    fn legacy_and_live_request_shapes_remain_separate() {
        let mut legacy = request(ModelInput::Goal {
            text: "legacy".to_string(),
        });
        legacy.previous_response_id = None;
        let legacy_body = request_body(&legacy).unwrap();
        assert!(legacy_body.get("previous_response_id").is_none());
        assert!(legacy_body.get("stream").is_none());
        assert_eq!(legacy_body["tools"][0]["strict"], false);

        let mut live = legacy;
        live.model = LIVE_MODEL.to_string();
        live.tools[0].name = LIVE_TOOL_NAME.to_string();
        live.tools[0].operation = "release.publish".to_string();
        live.tools[0].version = "v2".to_string();
        let live_body = request_body(&live).unwrap();
        assert_eq!(live_body["previous_response_id"], Value::Null);
        assert_eq!(live_body["stream"], false);
        assert_eq!(live_body["background"], false);
        assert_eq!(live_body["service_tier"], "default");
        assert_eq!(live_body["tools"][0]["strict"], true);
    }

    #[test]
    fn exact_live_request_rejects_usage_presence_or_equality_lost_by_legacy_parser() {
        let mut live = request(ModelInput::Goal {
            text: "live".to_string(),
        });
        live.model = LIVE_MODEL.to_string();
        live.tools[0].name = LIVE_TOOL_NAME.to_string();
        live.tools[0].operation = "release.publish".to_string();
        live.tools[0].version = "v2".to_string();
        let valid = json!({
            "id": "resp_live",
            "model": LIVE_MODEL,
            "status": "completed",
            "output": [{"type":"message","content":[{"type":"output_text","text":"done"}]}],
            "usage": {"input_tokens": 3, "output_tokens": 2, "total_tokens": 5}
        });
        assert!(
            parse_response_body_for_request(&serde_json::to_vec(&valid).unwrap(), &live).is_ok()
        );
        for field in ["input_tokens", "output_tokens", "total_tokens"] {
            let mut missing = valid.clone();
            missing["usage"].as_object_mut().unwrap().remove(field);
            assert!(matches!(
                parse_response_body_for_request(&serde_json::to_vec(&missing).unwrap(), &live),
                Err(ModelGatewayError::InvalidResponse(_))
            ));
        }
        let mut unequal = valid;
        unequal["usage"]["total_tokens"] = json!(6);
        assert!(matches!(
            parse_response_body_for_request(&serde_json::to_vec(&unequal).unwrap(), &live),
            Err(ModelGatewayError::InvalidResponse(_))
        ));
    }

    #[test]
    fn parses_function_call_and_usage() {
        let turn = parsed(json!({
            "id": "resp_1",
            "model": "gpt-5.6-sol",
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
        let turn = parsed(json!({
            "id": "resp_2",
            "model": "gpt-5.6-sol",
            "status": "completed",
            "output": [{
                "type": "message",
                "content": [
                    {"type": "output_text", "text": "Release "},
                    {"type": "output_text", "text": "ready."}
                ]
            }],
            "usage": {"input_tokens": 3, "output_tokens": 2, "total_tokens": 5}
        }))
        .unwrap();

        assert_eq!(
            turn.decision,
            ModelDecision::Finish {
                output: "Release ready.".to_string()
            }
        );
    }

    #[test]
    fn classifies_http_status_before_parsing_error_json() {
        for status in [400, 401, 403, 404] {
            assert!(matches!(
                classify_http_failure(status, b"<html>rejected</html>"),
                ModelGatewayError::Terminal(_)
            ));
        }
        assert!(matches!(
            classify_http_failure(429, br#"{"error":{"message":"limited"}}"#),
            ModelGatewayError::Explicit429(_)
        ));
        assert!(matches!(
            classify_http_failure(429, br#"{"id":"resp_possible","error":{}}"#),
            ModelGatewayError::Ambiguous(_)
        ));
        for status in [408, 500, 502, 503] {
            assert!(matches!(
                classify_http_failure(status, b"not json"),
                ModelGatewayError::Ambiguous(_)
            ));
        }
    }

    #[test]
    fn rejects_non_json_or_missing_identity_but_preserves_legacy_usage_tolerance() {
        assert!(matches!(
            parse_response_body(b"not json"),
            Err(ModelGatewayError::InvalidResponse(_))
        ));
        let valid = json!({
            "id": "resp_1",
            "model": "gpt-5.6-sol",
            "status": "completed",
            "output": [{"type":"message","content":[{"type":"output_text","text":"done"}]}],
            "usage": {"input_tokens": 3, "output_tokens": 2, "total_tokens": 5}
        });
        let mut missing_id = valid.clone();
        missing_id.as_object_mut().unwrap().remove("id");
        assert!(matches!(
            parsed(missing_id),
            Err(ModelGatewayError::InvalidResponse(_))
        ));
        let mut missing_model = valid.clone();
        missing_model.as_object_mut().unwrap().remove("model");
        assert_eq!(parsed(missing_model).unwrap().returned_model, None);

        let mut missing_usage = valid.clone();
        missing_usage.as_object_mut().unwrap().remove("usage");
        assert_eq!(parsed(missing_usage).unwrap().usage, ModelUsage::default());
        for field in ["input_tokens", "output_tokens", "total_tokens"] {
            let mut missing = valid.clone();
            missing["usage"].as_object_mut().unwrap().remove(field);
            let usage = parsed(missing).unwrap().usage;
            if field == "input_tokens" {
                assert_eq!(usage.input_tokens, 0);
            } else if field == "output_tokens" {
                assert_eq!(usage.output_tokens, 0);
            } else {
                assert_eq!(usage.total_tokens, 5);
            }
            let mut zero = valid.clone();
            zero["usage"][field] = json!(0);
            assert_eq!(
                match field {
                    "input_tokens" => parsed(zero).unwrap().usage.input_tokens,
                    "output_tokens" => parsed(zero).unwrap().usage.output_tokens,
                    "total_tokens" => parsed(zero).unwrap().usage.total_tokens,
                    _ => unreachable!(),
                },
                0
            );
        }
        let mut unequal = valid;
        unequal["usage"]["total_tokens"] = json!(6);
        assert_eq!(parsed(unequal).unwrap().usage.total_tokens, 6);
    }

    #[test]
    fn response_body_digest_binds_exact_http_bytes() {
        let compact = br#"{"id":"resp_1","model":"gpt-5.6-sol","status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"done"}]}],"usage":{"input_tokens":3,"output_tokens":2,"total_tokens":5}}"#;
        let spaced = br#"{ "id":"resp_1", "model":"gpt-5.6-sol", "status":"completed", "output":[{"type":"message","content":[{"type":"output_text","text":"done"}]}], "usage":{"input_tokens":3,"output_tokens":2,"total_tokens":5} }"#;
        let compact_turn = parse_response_body(compact).unwrap();
        let spaced_turn = parse_response_body(spaced).unwrap();
        assert_eq!(compact_turn.decision, spaced_turn.decision);
        assert_ne!(
            compact_turn.response_body_digest,
            spaced_turn.response_body_digest
        );
    }
}
