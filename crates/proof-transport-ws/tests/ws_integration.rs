use axum::http::{Request, StatusCode};
use proof_kernel::{
    ExecutionContext, ExecutionError, Governance, OperationHandler, Registry, RegistryEntry,
    VersionStatus,
};
use proof_transport_ws::{ws_router, SharedWsState, WsAppState};
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;

struct EchoHandler;

impl OperationHandler for EchoHandler {
    fn operation(&self) -> &str {
        "test.echo"
    }

    fn execute(&self, input: &Value, _context: &ExecutionContext) -> Result<Value, ExecutionError> {
        Ok(json!({"message": input["message"]}))
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
        Err(ExecutionError::HandlerFailed("handler failed".to_string()))
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
        status: VersionStatus::Active,
        deprecated_since: None,
        replacement_operation: None,
    }
}

fn state() -> SharedWsState {
    let registry = Registry::new(vec![
        registry_entry("test.echo", Governance::AgentExecutable),
        registry_entry("test.failing", Governance::AgentExecutable),
        registry_entry("test.human_only", Governance::HumanOnly),
        registry_entry("test.unknown", Governance::AgentExecutable),
    ])
    .unwrap();
    let state = Arc::new(WsAppState::with_registry("/tmp/proof-ws-test", registry));
    state.register_handler(Arc::new(EchoHandler));
    state.register_handler(Arc::new(FailingHandler));
    state
}

fn request() -> Request<axum::body::Body> {
    Request::builder()
        .method("GET")
        .uri("/ws")
        .header("host", "localhost")
        .header("connection", "Upgrade")
        .header("upgrade", "websocket")
        .header("sec-websocket-version", "13")
        .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==")
        .body(axum::body::Body::empty())
        .unwrap()
}

fn app(state: &SharedWsState) -> axum::Router {
    ws_router(state.clone())
}

#[tokio::test]
async fn websocket_upgrade_is_rejected_without_handshake_headers() {
    let state = state();
    let response = app(&state)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/ws")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn websocket_handler_compiles_with_http_oneshot_request() {
    let state = state();
    let service = app(&state);
    let response = service.oneshot(request()).await.unwrap();
    assert_eq!(response.status(), StatusCode::UPGRADE_REQUIRED);
}
