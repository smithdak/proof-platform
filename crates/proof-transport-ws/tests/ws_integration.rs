use axum::Router;
use futures_util::{SinkExt, StreamExt};
use proof_kernel::{
    ExecutionContext, ExecutionError, Governance, OperationHandler, Registry, RegistryEntry,
    VersionStatus,
};
use proof_transport_ws::{ws_router, WsAppState};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio_tungstenite::tungstenite::Message as WsMessage;

struct EchoHandler;

impl OperationHandler for EchoHandler {
    fn operation(&self) -> &str {
        "test.echo"
    }

    fn execute(&self, input: &Value, _context: &ExecutionContext) -> Result<Value, ExecutionError> {
        Ok(json!({"message": input["message"], "handled_by": "test.echo"}))
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

fn test_state() -> WsAppState {
    let registry = Registry::new(vec![registry_entry(
        "test.echo",
        Governance::AgentExecutable,
    )])
    .expect("echo entry should build a valid registry");
    WsAppState::with_registry("/tmp/proof-ws-test", registry)
}

fn router(state: WsAppState) -> (Router, std::sync::Arc<WsAppState>) {
    let shared = Arc::new(state);
    shared.register_handler(Arc::new(EchoHandler));
    (ws_router(shared.clone()), shared)
}

async fn connect(
    router: Router,
) -> (
    futures_util::stream::SplitSink<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        WsMessage,
    >,
    futures_util::stream::SplitStream<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let (socket, _response) = tokio_tungstenite::connect_async(format!("ws://{address}/ws"))
        .await
        .unwrap();
    socket.split()
}

async fn send_json(socket: &mut (impl SinkExt<WsMessage> + std::marker::Unpin), value: &Value) {
    let payload = serde_json::to_string(value).unwrap();
    socket
        .send(WsMessage::Text(payload))
        .await
        .unwrap_or_else(|_| panic!("websocket send should succeed"));
}

async fn read_json<T>(socket: &mut T) -> Value
where
    T: futures_util::Stream<Item = Result<WsMessage, tokio_tungstenite::tungstenite::Error>>
        + Unpin,
{
    use futures_util::StreamExt;
    let message = socket.next().await.unwrap().unwrap();
    serde_json::from_str(&message.to_text().unwrap()).unwrap()
}

#[tokio::test]
async fn executes_operation_and_returns_proof() {
    let (router, _state) = router(test_state());
    let (mut sender, mut receiver) = connect(router).await;

    let request = json!({
        "id": 1,
        "method": "execute",
        "params": {
            "operation": "test.echo",
            "version": "v1",
            "input": {"message": "hello", "idempotency_key": "018f0d7a-bdea-7000-8000-0123456789ab"}
        }
    });
    send_json(&mut sender, &request).await;
    let response = read_json(&mut receiver).await;

    assert_eq!(response["id"], 1);
    assert_eq!(response["result"]["status"], "executed");
    assert_eq!(response["result"]["result"]["message"], "hello");
    assert!(response["result"]["proof"].is_object());
}

#[tokio::test]
async fn returns_operation_not_found_error() {
    let (router, _state) = router(test_state());
    let (mut sender, mut receiver) = connect(router).await;

    let request = json!({
        "id": 2,
        "method": "execute",
        "params": {"operation": "test.missing", "version": "v1"}
    });
    send_json(&mut sender, &request).await;
    let response = read_json(&mut receiver).await;

    assert_eq!(response["error"]["code"], -32001);
}

#[tokio::test]
async fn returns_missing_params_error() {
    let (router, _state) = router(test_state());
    let (mut sender, mut receiver) = connect(router).await;

    let request = json!({"id": 3, "method": "execute", "params": {}});
    send_json(&mut sender, &request).await;
    let response = read_json(&mut receiver).await;

    assert_eq!(response["error"]["code"], -32602);
    assert!(response["error"]["message"]
        .as_str()
        .unwrap()
        .contains("operation and version are required"));
}

#[tokio::test]
async fn returns_unknown_method_error() {
    let (router, _state) = router(test_state());
    let (mut sender, mut receiver) = connect(router).await;

    let request = json!({"id": 4, "method": "bogus", "params": {}});
    send_json(&mut sender, &request).await;
    let response = read_json(&mut receiver).await;

    assert_eq!(response["error"]["code"], -32601);
    assert!(response["error"]["message"]
        .as_str()
        .unwrap()
        .contains("unknown method"));
}

#[tokio::test]
async fn returns_invalid_json_error() {
    let (router, _state) = router(test_state());
    let (mut sender, mut receiver) = connect(router).await;

    sender
        .send(WsMessage::Text("not json".to_string()))
        .await
        .unwrap();
    let response = read_json(&mut receiver).await;

    assert_eq!(response["error"]["code"], -32700);
    assert!(response["error"]["message"]
        .as_str()
        .unwrap()
        .contains("invalid JSON"));
}

#[tokio::test]
async fn returns_missing_method_error() {
    let (router, _state) = router(test_state());
    let (mut sender, mut receiver) = connect(router).await;

    let request = json!({"id": 5});
    send_json(&mut sender, &request).await;
    let response = read_json(&mut receiver).await;

    assert_eq!(response["error"]["code"], -32600);
    assert!(response["error"]["message"]
        .as_str()
        .unwrap()
        .contains("missing method"));
}

#[tokio::test]
async fn lists_tools_from_registry() {
    let (router, _state) = router(test_state());
    let (mut sender, mut receiver) = connect(router).await;

    let request = json!({"id": 6, "method": "list_tools"});
    send_json(&mut sender, &request).await;
    let response = read_json(&mut receiver).await;

    assert_eq!(response["id"], 6);
    let tools = response["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["operation"], "test.echo");
}
