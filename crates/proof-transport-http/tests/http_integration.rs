use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use proof_kernel::{
    ExecutionContext, ExecutionError, Governance, OperationHandler, Registry, RegistryEntry,
};
use proof_transport_http::{router, AppState};
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;

struct EchoHandler;

impl OperationHandler for EchoHandler {
    fn operation(&self) -> &str {
        "test.echo"
    }

    fn execute(&self, input: &Value, _context: &ExecutionContext) -> Result<Value, ExecutionError> {
        Ok(json!({
            "message": input["message"],
            "handled_by": "test.echo"
        }))
    }
}

struct FailingHandler;

impl OperationHandler for FailingHandler {
    fn operation(&self) -> &str {
        "test.failing"
    }

    fn execute(
        &self,
        _input: &Value,
        _context: &ExecutionContext,
    ) -> Result<Value, ExecutionError> {
        Err(ExecutionError::HandlerFailed(
            "handler exploded".to_string(),
        ))
    }
}

fn registry_entry(operation: &str, governance: Governance) -> RegistryEntry {
    RegistryEntry {
        operation: operation.to_string(),
        domain: "test".to_string(),
        version: "v1".to_string(),
        action: format!("test:{}", operation.replace('.', "_")),
        description: format!("Test operation {operation}"),
        input_schema: r#"{"type":"object"}"#.to_string(),
        output_schema: r#"{"type":"object"}"#.to_string(),
        required_authority: "delegation-grant".to_string(),
        governance,
        idempotency: "required-uuidv7".to_string(),
        consequence: "test-mutation".to_string(),
        evidence_contract: "operation-effect-v1".to_string(),
        benchmark: None,
    }
}

fn app_state() -> Arc<AppState> {
    let registry = Registry::new(vec![
        registry_entry("test.echo", Governance::AgentExecutable),
        registry_entry("test.failing", Governance::AgentExecutable),
        registry_entry("test.unhandled", Governance::AgentExecutable),
        registry_entry("test.human_only", Governance::HumanOnly),
    ])
    .unwrap();
    let state = Arc::new(AppState::with_registry("/tmp/proof-http-test", registry));
    state.register_handler(Arc::new(EchoHandler));
    state.register_handler(Arc::new(FailingHandler));
    state
}

async fn response_json(
    app: axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(
            body.map(|body| body.to_string()).unwrap_or_default(),
        ))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, json)
}

#[tokio::test]
async fn successful_execution_returns_200_with_result_and_proof() {
    let app = router(app_state());
    let (status, body) = response_json(
        app,
        "POST",
        "/v1/operations/test.echo/v1",
        Some(json!({"message": "hello"})),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let proof = body["proof"].as_object().unwrap();
    assert_eq!(proof.len(), 2);
    assert!(proof.contains_key("body"));
    assert!(proof.contains_key("signature"));

    let proof_body = proof["body"].as_object().unwrap();
    assert_eq!(proof_body.len(), 6);
    assert!(proof_body
        .get("id")
        .and_then(Value::as_str)
        .map(|value| !value.is_empty())
        .unwrap_or(false));
    assert!(proof_body
        .get("actor")
        .and_then(Value::as_str)
        .map(|value| !value.is_empty())
        .unwrap_or(false));
    assert_eq!(proof_body["operation"], "test.echo");
    assert!(proof_body
        .get("input_digest")
        .and_then(Value::as_str)
        .map(|value| !value.is_empty())
        .unwrap_or(false));
    assert!(proof_body
        .get("output_digest")
        .and_then(Value::as_str)
        .map(|value| !value.is_empty())
        .unwrap_or(false));
    assert!(proof_body
        .get("timestamp")
        .and_then(Value::as_str)
        .map(|value| !value.is_empty())
        .unwrap_or(false));
    assert_eq!(proof["signature"].as_array().unwrap().len(), 64);

    assert_eq!(body["operation"], "test.echo");
    assert_eq!(body["version"], "v1");
    assert_eq!(body["status"], "executed");
    assert_eq!(
        body["result"],
        json!({"message": "hello", "handled_by": "test.echo"})
    );
}

#[tokio::test]
async fn human_only_operation_returns_403() {
    let app = router(app_state());
    let (status, body) = response_json(
        app,
        "POST",
        "/v1/operations/test.human_only/v1",
        Some(json!({"confirmed": true})),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(
        body,
        json!({"error": "operation is human-only, agents cannot execute"})
    );
}

#[tokio::test]
async fn unknown_operation_returns_404() {
    let app = router(app_state());
    let (status, body) = response_json(
        app,
        "POST",
        "/v1/operations/test.missing/v1",
        Some(json!({})),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        body,
        json!({"error": "operation not found: test.missing v1"})
    );
}

#[tokio::test]
async fn missing_handler_returns_500() {
    let app = router(app_state());
    let (status, body) = response_json(
        app,
        "POST",
        "/v1/operations/test.unhandled/v1",
        Some(json!({})),
    )
    .await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        body,
        json!({"error": "no handler registered for: test.unhandled"})
    );
}

#[tokio::test]
async fn handler_failure_returns_500() {
    let app = router(app_state());
    let (status, body) = response_json(
        app,
        "POST",
        "/v1/operations/test.failing/v1",
        Some(json!({})),
    )
    .await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        body,
        json!({"error": "handler execution failed: handler exploded"})
    );
}
