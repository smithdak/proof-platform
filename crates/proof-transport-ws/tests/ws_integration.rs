use axum::Router;
use futures_util::{SinkExt, StreamExt};
use proof_content::{
    ChangeSet, ChangeSetEdit, FieldType, Object, ObjectCreateEdit, SchemaDefinition, SchemaField,
};
use proof_kernel::{
    ExecutionContext, ExecutionEngine, ExecutionError, Governance, IdempotencyPolicy,
    OperationHandler, Proof, Registry, RegistryEntry, VersionStatus,
};
use proof_storage::SqliteStore;
use proof_transport_ws::{ws_router, WsAppState};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
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

fn copy_directory(source: &Path, destination: &Path) {
    std::fs::create_dir_all(destination).unwrap();
    for entry in std::fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if source_path.is_dir() {
            copy_directory(&source_path, &destination_path);
        } else {
            std::fs::copy(source_path, destination_path).unwrap();
        }
    }
}

fn content_state() -> (WsAppState, tempfile::TempDir, ChangeSet) {
    let temp = tempfile::TempDir::new().unwrap();
    let workspace = temp.path().to_path_buf();
    let repository_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    copy_directory(
        &repository_root.join("registry"),
        &workspace.join(".proof/registry"),
    );
    copy_directory(
        &repository_root.join("registry"),
        &workspace.join("registry"),
    );

    let schema = SchemaDefinition::new(
        "Article",
        1,
        vec![SchemaField {
            name: "title".to_string(),
            field_type: FieldType::Text,
            required: true,
            localized: false,
            default_value: None,
        }],
    );
    let schema_directory = workspace.join(".proof/data/schemas");
    std::fs::create_dir_all(&schema_directory).unwrap();
    std::fs::write(
        schema_directory.join(format!("{}-{}.json", schema.id, schema.version)),
        serde_json::to_string(&schema).unwrap(),
    )
    .unwrap();
    let object = Object::create(&schema, "en-US", json!({"title": "Created"})).unwrap();
    let mut changeset = ChangeSet::new(
        "Create object",
        &BTreeMap::new(),
        vec![ChangeSetEdit::ObjectCreate(ObjectCreateEdit { object })],
    );
    changeset
        .transition_to(proof_content::ChangeSetStatus::Submitted)
        .unwrap();
    changeset
        .transition_to(proof_content::ChangeSetStatus::Approved)
        .unwrap();
    let changeset_directory = workspace.join(".proof/data/changesets");
    std::fs::create_dir_all(&changeset_directory).unwrap();
    std::fs::write(
        changeset_directory.join(format!("{}.json", changeset.id)),
        serde_json::to_string(&changeset).unwrap(),
    )
    .unwrap();

    let registry = Registry::load_from_directory(workspace.join(".proof/registry")).unwrap();
    let proof_directory = workspace.join(".proof/data/proofs");
    std::fs::create_dir_all(&proof_directory).unwrap();
    let store = SqliteStore::open(&proof_directory.join("proofs.sqlite3")).unwrap();
    (
        WsAppState::with_registry_and_store(
            workspace.to_string_lossy().to_string(),
            registry,
            store,
        ),
        temp,
        changeset,
    )
}

struct ReplayEchoHandler;

impl OperationHandler for ReplayEchoHandler {
    fn operation(&self) -> &str {
        "test.echo"
    }

    fn idempotency_policy(&self) -> IdempotencyPolicy {
        IdempotencyPolicy::RequiredUuidV7ExactReplay
    }

    fn execute(&self, input: &Value, _context: &ExecutionContext) -> Result<Value, ExecutionError> {
        Ok(json!({"message": input["message"]}))
    }
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
    let (router, state) = router(test_state());
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
    let proof: Proof = serde_json::from_value(response["result"]["proof"].clone()).unwrap();
    assert_eq!(proof.body.operation, "test.echo::v1");
    proof
        .verify(&state.keypair.signing_key.verifying_key())
        .unwrap();
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

#[tokio::test]
async fn governed_content_operations_discover_and_replay_original_proof() {
    let (state, temp, changeset) = content_state();
    let workspace = temp.path();
    let shared = Arc::new(state);
    let router = ws_router(shared.clone());
    let (mut sender, mut receiver) = connect(router).await;

    send_json(&mut sender, &json!({"id": 1, "method": "list_tools"})).await;
    let discovery = read_json(&mut receiver).await;
    let tools = discovery["result"]["tools"].as_array().unwrap();
    for operation in ["changeset.commit", "edition.create"] {
        let entry = tools
            .iter()
            .find(|entry| entry["operation"] == operation)
            .unwrap();
        assert_eq!(entry["version"], "v1");
        assert_eq!(entry["governance"], "agent-executable");
        assert!(entry["input_schema"]
            .as_str()
            .unwrap()
            .contains("input.json"));
        assert!(entry["output_schema"]
            .as_str()
            .unwrap()
            .contains("output.json"));
    }

    let commit_key = uuid::Uuid::now_v7();
    let commit = |id, input| {
        json!({
            "id": id,
            "method": "execute",
            "params": {"operation": "changeset.commit", "version": "v1", "input": input}
        })
    };
    send_json(
        &mut sender,
        &commit(
            2,
            json!({
                "notes": "first commit",
                "changeset_id": changeset.id,
                "idempotency_key": commit_key,
            }),
        ),
    )
    .await;
    let first = read_json(&mut receiver).await;
    assert_eq!(first["result"]["status"], "executed");
    assert_eq!(first["result"]["operation"], "changeset.commit");
    let first_output = first["result"]["result"].clone();
    let first_proof = first["result"]["proof"].clone();
    let parsed_proof: Proof = serde_json::from_value(first_proof.clone()).unwrap();
    assert_eq!(parsed_proof.body.operation, "changeset.commit::v1");
    parsed_proof
        .verify(&shared.keypair.signing_key.verifying_key())
        .unwrap();
    let object_directory = workspace.join(".proof/data/objects");
    let object_count = std::fs::read_dir(&object_directory).unwrap().count();
    assert_eq!(object_count, 1);

    send_json(
        &mut sender,
        &commit(
            3,
            json!({
                "idempotency_key": commit_key,
                "changeset_id": changeset.id,
                "notes": "first commit",
            }),
        ),
    )
    .await;
    let retry = read_json(&mut receiver).await;
    assert_eq!(retry["result"]["result"], first_output);
    assert_eq!(retry["result"]["proof"], first_proof);
    assert_eq!(
        std::fs::read_dir(&object_directory).unwrap().count(),
        object_count
    );
    assert_eq!(
        shared
            .store
            .list_proofs_for_operation("changeset.commit", Some("v1"))
            .unwrap()
            .len(),
        1
    );

    send_json(
        &mut sender,
        &commit(
            4,
            json!({
                "idempotency_key": commit_key,
                "changeset_id": changeset.id,
                "notes": "different input",
            }),
        ),
    )
    .await;
    let conflict = read_json(&mut receiver).await;
    assert_eq!(conflict["error"]["code"], -32006);
    assert_eq!(
        std::fs::read_dir(&object_directory).unwrap().count(),
        object_count
    );

    let edition_key = uuid::Uuid::now_v7();
    let edition = |id, input| {
        json!({
            "id": id,
            "method": "execute",
            "params": {"operation": "edition.create", "version": "v1", "input": input}
        })
    };
    send_json(
        &mut sender,
        &edition(
            5,
            json!({"changeset_id": changeset.id, "idempotency_key": edition_key}),
        ),
    )
    .await;
    let edition_first = read_json(&mut receiver).await;
    assert_eq!(edition_first["result"]["status"], "executed");
    let edition_output = edition_first["result"]["result"].clone();
    let edition_proof = edition_first["result"]["proof"].clone();
    let parsed_edition_proof: Proof = serde_json::from_value(edition_proof.clone()).unwrap();
    assert_eq!(parsed_edition_proof.body.operation, "edition.create::v1");
    parsed_edition_proof
        .verify(&shared.keypair.signing_key.verifying_key())
        .unwrap();
    let edition_directory = workspace.join(".proof/data/editions");
    assert_eq!(std::fs::read_dir(&edition_directory).unwrap().count(), 1);

    send_json(
        &mut sender,
        &edition(
            6,
            json!({"idempotency_key": edition_key, "changeset_id": changeset.id}),
        ),
    )
    .await;
    let edition_retry = read_json(&mut receiver).await;
    assert_eq!(edition_retry["result"]["result"], edition_output);
    assert_eq!(edition_retry["result"]["proof"], edition_proof);
    assert_eq!(std::fs::read_dir(&edition_directory).unwrap().count(), 1);
    assert_eq!(
        shared
            .store
            .list_proofs_for_operation("edition.create", Some("v1"))
            .unwrap()
            .len(),
        1
    );
    let stored_edition_proofs = shared
        .store
        .list_proofs_for_operation("edition.create", Some("v1"))
        .unwrap();
    assert_eq!(stored_edition_proofs[0], parsed_edition_proof);

    send_json(
        &mut sender,
        &edition(
            7,
            json!({"idempotency_key": edition_key, "changeset_id": uuid::Uuid::now_v7()}),
        ),
    )
    .await;
    let edition_conflict = read_json(&mut receiver).await;
    assert_eq!(edition_conflict["error"]["code"], -32006);
    assert_eq!(std::fs::read_dir(&edition_directory).unwrap().count(), 1);
}

#[tokio::test]
async fn idempotency_validation_and_storage_codes_survive_network_boundary() {
    let mut state = test_state();
    let keypair = state.keypair.clone();
    state.engine = Arc::new(std::sync::RwLock::new(ExecutionEngine::new_with_keypair(
        state.registry.clone(),
        keypair,
    )));
    let shared = Arc::new(state);
    shared.register_handler(Arc::new(ReplayEchoHandler));
    let (mut sender, mut receiver) = connect(ws_router(shared)).await;

    send_json(
        &mut sender,
        &json!({
            "id": 8,
            "method": "execute",
            "params": {"operation": "test.echo", "version": "v1", "input": {"message": "missing key"}}
        }),
    )
    .await;
    assert_eq!(read_json(&mut receiver).await["error"]["code"], -32602);

    send_json(
        &mut sender,
        &json!({
            "id": 9,
            "method": "execute",
            "params": {
                "operation": "test.echo",
                "version": "v1",
                "input": {"message": "storage required", "idempotency_key": uuid::Uuid::now_v7()}
            }
        }),
    )
    .await;
    assert_eq!(read_json(&mut receiver).await["error"]["code"], -32005);
}
