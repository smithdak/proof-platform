use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use proof_kernel::{
    create_proof, generate_keypair_for, ExecutionContext, ExecutionError, Governance,
    OperationHandler, PrincipalKind, Registry, RegistryEntry, VersionStatus,
};
use proof_storage::SqliteStore;
use proof_transport_http::{router, router_with_limits, AppState, HttpLimits, RateLimitConfig};
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
        status: VersionStatus::Active,
        deprecated_since: None,
        replacement_operation: None,
    }
}

fn commerce_entry(operation: &str, governance: Governance) -> RegistryEntry {
    RegistryEntry {
        operation: operation.to_string(),
        domain: "commerce".to_string(),
        version: "v1".to_string(),
        action: format!("commerce:{}", operation.replace('.', "_")),
        description: format!("Commerce operation {operation}"),
        input_schema: format!("registry/commerce/{operation}.input.json"),
        output_schema: r#"{"type":"object"}"#.to_string(),
        required_authority: "delegation-grant".to_string(),
        governance,
        idempotency: "required-uuidv7".to_string(),
        consequence: "commerce-mutation".to_string(),
        evidence_contract: "operation-effect-v1".to_string(),
        benchmark: None,
        status: VersionStatus::Active,
        deprecated_since: None,
        replacement_operation: None,
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

fn commerce_state() -> (Arc<AppState>, tempfile::TempDir) {
    let temp = tempfile::TempDir::new().unwrap();
    let schemas = temp.path().join(".proof/registry/commerce");
    std::fs::create_dir_all(&schemas).unwrap();
    std::fs::write(
        schemas.join("catalog.create.input.json"),
        r#"{"type":"object","required":["name"],"properties":{"name":{"type":"string"}},"additionalProperties":false}"#,
    )
    .unwrap();
    std::fs::write(
        schemas.join("catalog.update.input.json"),
        r#"{"type":"object","required":["catalog_id"],"properties":{"catalog_id":{"type":"string"}},"additionalProperties":false}"#,
    )
    .unwrap();
    std::fs::write(
        schemas.join("order.create.input.json"),
        r#"{"type":"object","required":["catalog_id"],"properties":{"catalog_id":{"type":"string"}},"additionalProperties":false}"#,
    )
    .unwrap();
    std::fs::write(
        schemas.join("order.approve.input.json"),
        r#"{"type":"object","required":["order_id"],"properties":{"order_id":{"type":"string"}},"additionalProperties":false}"#,
    )
    .unwrap();
    std::fs::write(
        schemas.join("order.fulfill.input.json"),
        r#"{"type":"object","required":["order_id"],"properties":{"order_id":{"type":"string"}},"additionalProperties":false}"#,
    )
    .unwrap();
    let registry = Registry::new(vec![
        commerce_entry("catalog.create", Governance::AgentExecutable),
        commerce_entry("catalog.update", Governance::AgentExecutable),
        commerce_entry("order.create", Governance::AgentExecutable),
        commerce_entry("order.approve", Governance::HumanOnly),
        commerce_entry("order.fulfill", Governance::AgentExecutable),
    ])
    .unwrap();
    (
        Arc::new(AppState::with_registry(
            temp.path().to_str().unwrap(),
            registry,
        )),
        temp,
    )
}

fn registry() -> Registry {
    Registry::new(vec![
        registry_entry("test.echo", Governance::AgentExecutable),
        registry_entry("test.failing", Governance::AgentExecutable),
        registry_entry("test.unhandled", Governance::AgentExecutable),
        registry_entry("test.human_only", Governance::HumanOnly),
    ])
    .unwrap()
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

async fn raw_response(
    app: axum::Router,
    request: Request<Body>,
) -> (StatusCode, axum::http::HeaderMap, Value) {
    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, headers, json)
}

#[tokio::test]
async fn rate_limit_returns_429_with_retry_after_per_client() {
    let limits = HttpLimits {
        rate_limit: RateLimitConfig {
            requests_per_minute: 1,
        },
        body_limit: 1024,
    };
    let request = || {
        Request::builder()
            .method("POST")
            .uri("/v1/operations/test.echo/v1")
            .header("content-type", "application/json")
            .header("x-forwarded-for", "203.0.113.10")
            .body(Body::from(json!({"message": "hello"}).to_string()))
            .unwrap()
    };
    let app = router_with_limits(app_state(), limits.clone());

    let (first_status, _, _) = raw_response(app.clone(), request()).await;
    assert_eq!(first_status, StatusCode::OK);

    let (limited_status, headers, body) = raw_response(app.clone(), request()).await;
    assert_eq!(limited_status, StatusCode::TOO_MANY_REQUESTS);
    assert!(headers.contains_key("retry-after"));
    assert_eq!(body["error"], "rate limit exceeded");

    let request = Request::builder()
        .method("POST")
        .uri("/v1/operations/test.echo/v1")
        .header("content-type", "application/json")
        .header("x-forwarded-for", "203.0.113.11")
        .body(Body::from(json!({"message": "hello"}).to_string()))
        .unwrap();
    let (other_client_status, _, _) = raw_response(app, request).await;
    assert_eq!(other_client_status, StatusCode::OK);
}

#[tokio::test]
async fn oversized_json_body_returns_413() {
    let limits = HttpLimits {
        rate_limit: RateLimitConfig {
            requests_per_minute: 10,
        },
        body_limit: 8,
    };
    let request = Request::builder()
        .method("POST")
        .uri("/v1/operations/test.echo/v1")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"message":"too long"}"#))
        .unwrap();
    let (status, _, body) = raw_response(router_with_limits(app_state(), limits), request).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(body["error"], "request body too large");
}

#[tokio::test]
async fn post_rejects_non_json_content_type_and_malformed_json() {
    let request = Request::builder()
        .method("POST")
        .uri("/v1/operations/test.echo/v1")
        .header("content-type", "text/plain")
        .body(Body::from("hello"))
        .unwrap();
    let (status, _, body) = raw_response(router(app_state()), request).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "Content-Type must be application/json");

    let request = Request::builder()
        .method("POST")
        .uri("/v1/operations/test.echo/v1")
        .header("content-type", "application/json")
        .body(Body::from("{\"message\":"))
        .unwrap();
    let (status, _, body) = raw_response(router(app_state()), request).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "invalid JSON");
    assert!(body["detail"].as_str().is_some());
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

#[tokio::test]
async fn proof_endpoints_return_filter_and_verify_stored_proofs() {
    let store = SqliteStore::in_memory().unwrap();
    let state = Arc::new(AppState::with_registry_and_store(
        "/tmp/proof-http-test",
        registry(),
        store,
    ));
    let keypair = generate_keypair_for(PrincipalKind::Agent);
    let input = json!({"message": "hello"});
    let output = json!({"ok": true});
    let matching = create_proof(
        keypair.principal_id,
        None,
        "object.create::v1",
        &input,
        &output,
        chrono::Utc::now(),
        &keypair,
    )
    .unwrap();
    let other_actor = generate_keypair_for(PrincipalKind::Human);
    let other = create_proof(
        other_actor.principal_id,
        None,
        "other.operation",
        &input,
        &output,
        chrono::Utc::now(),
        &other_actor,
    )
    .unwrap();
    state.store.save_proof(&matching).unwrap();
    state.store.save_proof(&other).unwrap();
    let principal = proof_kernel::principal_from_keypair(&keypair);
    state.store.save_principal(&principal).unwrap();

    let app = router(state.clone());
    let (status, body) = response_json(app, "GET", "/proofs", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["items"].as_array().unwrap().len(), 2);
    assert_eq!(body["total"], 2);
    assert_eq!(body["limit"], 20);
    assert_eq!(body["offset"], 0);

    let app = router(state.clone());
    let (_, body) = response_json(
        app,
        "GET",
        &format!("/proofs?operation={}", "object.create"),
        None,
    )
    .await;
    assert_eq!(body["items"].as_array().unwrap().len(), 1);
    assert_eq!(body["items"][0]["body"]["id"], matching.body.id.to_string());

    let app = router(state.clone());
    let (_, body) = response_json(
        app,
        "GET",
        &format!("/proofs?actor={}", keypair.principal_id),
        None,
    )
    .await;
    assert_eq!(body["items"].as_array().unwrap().len(), 1);

    let app = router(state.clone());
    let (status, body) =
        response_json(app, "GET", &format!("/proofs/{}", matching.body.id), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["proof"]["body"]["id"], matching.body.id.to_string());
    assert_eq!(body["verification"], "verified");

    let app = router(state.clone());
    let (status, body) = response_json(
        app,
        "POST",
        "/proofs/verify",
        Some(json!({"proof_id": matching.body.id})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["valid"], true);
}

#[tokio::test]
async fn proof_listing_supports_combined_filters_pagination_and_sort() {
    let store = SqliteStore::in_memory().unwrap();
    let state = Arc::new(AppState::with_registry_and_store(
        "/tmp/proof-http-test",
        registry(),
        store,
    ));
    let keypair = generate_keypair_for(PrincipalKind::Agent);
    let other_keypair = generate_keypair_for(PrincipalKind::Human);
    let base = chrono::Utc::now();

    let proofs = vec![
        create_proof(
            keypair.principal_id,
            None,
            "object.create::v1",
            &json!({"id": "first"}),
            &json!({"actor": "first"}),
            base,
            &keypair,
        )
        .unwrap(),
        create_proof(
            keypair.principal_id,
            None,
            "object.create::v2",
            &json!({"id": "second"}),
            &json!({"actor": "second"}),
            base + chrono::Duration::milliseconds(1),
            &keypair,
        )
        .unwrap(),
        create_proof(
            other_keypair.principal_id,
            None,
            "object.create::v1",
            &json!({"id": "third"}),
            &json!({"actor": "third"}),
            base + chrono::Duration::milliseconds(2),
            &other_keypair,
        )
        .unwrap(),
        create_proof(
            keypair.principal_id,
            None,
            "other.operation::v1",
            &json!({"id": "excluded"}),
            &json!({}),
            base + chrono::Duration::milliseconds(3),
            &keypair,
        )
        .unwrap(),
    ];
    for proof in &proofs {
        state.store.save_proof(proof).unwrap();
    }

    let query = format!(
        "/proofs?operation={}&version={}&actor={}",
        "object.create", "v1", keypair.principal_id
    );
    let (_, body) = response_json(router(state.clone()), "GET", &query, None).await;
    assert_eq!(body["total"], 1);
    assert_eq!(
        body["items"][0]["body"]["id"],
        proofs[0].body.id.to_string()
    );

    let (_, body) = response_json(
        router(state.clone()),
        "GET",
        "/proofs?limit=2&offset=1&sort=timestamp&order=desc",
        None,
    )
    .await;
    assert_eq!(body["total"], 4);
    assert_eq!(body["limit"], 2);
    assert_eq!(body["offset"], 1);
    let ids: Vec<String> = body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|proof| proof["body"]["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        ids,
        vec![proofs[2].body.id, proofs[1].body.id]
            .into_iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
    );

    let (_, body) = response_json(
        router(state.clone()),
        "GET",
        "/proofs?sort=id&order=asc",
        None,
    )
    .await;
    let mut expected: Vec<String> = proofs
        .iter()
        .map(|proof| proof.body.id.to_string())
        .collect();
    expected.sort();
    let ids: Vec<String> = body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|proof| proof["body"]["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(ids, expected);

    let (_, body) = response_json(
        router(state.clone()),
        "GET",
        "/proofs?limit=250&offset=10",
        None,
    )
    .await;
    assert_eq!(body["limit"], 100);
    assert_eq!(body["offset"], 10);
    assert_eq!(body["items"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn proof_listing_rejects_invalid_sort_and_order() {
    let state = Arc::new(AppState::with_registry("/tmp/proof-http-test", registry()));
    let (status, body) =
        response_json(router(state.clone()), "GET", "/proofs?sort=actor", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "sort must be timestamp or id");

    let (status, body) = response_json(router(state), "GET", "/proofs?order=down", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "order must be asc or desc");
}

#[tokio::test]
async fn capabilities_lists_registry_operations() {
    let state = app_state();
    let (_, body) = response_json(router(state.clone()), "GET", "/capabilities", None).await;

    assert_eq!(
        body,
        json!({
            "operations": [
                {"name": "test.echo", "version": "v1", "domain": "test", "governance": "agent-executable"},
                {"name": "test.failing", "version": "v1", "domain": "test", "governance": "agent-executable"},
                {"name": "test.human_only", "version": "v1", "domain": "test", "governance": "human-only"},
                {"name": "test.unhandled", "version": "v1", "domain": "test", "governance": "agent-executable"},
            ]
        })
    );
}

#[tokio::test]
async fn commerce_operations_execute_through_dispatch_and_list_endpoints() {
    let (state, _workspace) = commerce_state();

    let (status, body) = response_json(
        router(state.clone()),
        "POST",
        "/v1/operations/catalog.create/v1",
        Some(json!({"name": "Spring"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "executed");
    assert_eq!(body["result"]["operation"], "catalog.create");
    let catalog_id = body["result"]["data"]["catalog_id"].as_str().unwrap();

    let (status, body) = response_json(
        router(state.clone()),
        "POST",
        "/v1/operations/catalog.update/v1",
        Some(json!({"catalog_id": catalog_id})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["result"]["operation"], "catalog.update");

    let (status, body) = response_json(
        router(state.clone()),
        "POST",
        "/v1/operations/order.create/v1",
        Some(json!({"catalog_id": catalog_id})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let order_id = body["result"]["data"]["order_id"].as_str().unwrap();

    let (_, body) = response_json(router(state.clone()), "GET", "/catalog", None).await;
    assert_eq!(body["catalogs"].as_array().unwrap().len(), 1);
    assert_eq!(body["catalogs"][0]["id"], catalog_id);

    let (_, body) = response_json(router(state.clone()), "GET", "/orders", None).await;
    assert_eq!(body["orders"].as_array().unwrap().len(), 1);
    assert_eq!(body["orders"][0]["id"], order_id);
    assert_eq!(body["orders"][0]["status"], "pending_approval");
}

#[tokio::test]
async fn commerce_order_lifecycle_requires_human_approval_before_fulfillment() {
    let (state, _workspace) = commerce_state();

    let (_, catalog) = response_json(
        router(state.clone()),
        "POST",
        "/v1/operations/catalog.create/v1",
        Some(json!({"name": "Seasonal"})),
    )
    .await;
    let catalog_id = catalog["result"]["data"]["catalog_id"].as_str().unwrap();
    let (_, order) = response_json(
        router(state.clone()),
        "POST",
        "/v1/operations/order.create/v1",
        Some(json!({"catalog_id": catalog_id})),
    )
    .await;
    let order_id = order["result"]["data"]["order_id"].as_str().unwrap();

    let (status, body) = response_json(
        router(state.clone()),
        "POST",
        "/v1/operations/order.fulfill/v1",
        Some(json!({"order_id": order_id})),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(body["error"]
        .as_str()
        .unwrap()
        .contains("only approved orders can be fulfilled"));

    let (status, _) = response_json(
        router(state.clone()),
        "POST",
        "/v1/operations/order.approve/v1",
        Some(json!({"order_id": order_id})),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, body) = response_json(
        router(state.clone()),
        "POST",
        "/v1/operations/order.fulfill/v1",
        Some(json!({"order_id": order_id})),
    )
    .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(body["error"]
        .as_str()
        .unwrap()
        .contains("only approved orders can be fulfilled"));
}

#[tokio::test]
async fn proof_version_filter_applies_with_operation_filter() {
    let store = SqliteStore::in_memory().unwrap();
    let state = Arc::new(AppState::with_registry_and_store(
        "/tmp/proof-http-test",
        registry(),
        store,
    ));
    let keypair = generate_keypair_for(PrincipalKind::Agent);
    let version_one = create_proof(
        keypair.principal_id,
        None,
        "object.create::v1",
        &json!({"id": "one"}),
        &json!({}),
        chrono::Utc::now(),
        &keypair,
    )
    .unwrap();
    let version_two = create_proof(
        keypair.principal_id,
        None,
        "object.create::v2",
        &json!({"id": "two"}),
        &json!({}),
        chrono::Utc::now() + chrono::Duration::milliseconds(1),
        &keypair,
    )
    .unwrap();
    state.store.save_proof(&version_one).unwrap();
    state.store.save_proof(&version_two).unwrap();

    let (_, body) = response_json(
        router(state.clone()),
        "GET",
        "/proofs?operation=object.create&version=v1",
        None,
    )
    .await;

    assert_eq!(body["total"], 1);
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["body"]["id"], version_one.body.id.to_string());
}

#[tokio::test]
async fn get_proof_returns_signature_verification_status() {
    let store = SqliteStore::in_memory().unwrap();
    let state = Arc::new(AppState::with_registry_and_store(
        "/tmp/proof-http-test",
        registry(),
        store,
    ));
    let keypair = generate_keypair_for(PrincipalKind::Agent);
    let proof = create_proof(
        keypair.principal_id,
        None,
        "object.create::v1",
        &json!({"id": "signed"}),
        &json!({}),
        chrono::Utc::now(),
        &keypair,
    )
    .unwrap();
    let mut tampered = proof.clone();
    tampered.body.output_digest = proof_kernel::digest(
        proof_kernel::ArtifactKind::OperationOutput,
        &proof_kernel::canonicalize(&json!({"tampered": true})).unwrap(),
    );
    state.store.save_proof(&proof).unwrap();
    state
        .store
        .save_principal(&proof_kernel::principal_from_keypair(&keypair))
        .unwrap();

    let (_, body) = response_json(
        router(state.clone()),
        "GET",
        &format!("/proofs/{}", proof.body.id),
        None,
    )
    .await;
    assert_eq!(body["verification"], "verified");

    // Overwrite via the raw connection path (same ID, newer timestamp is required
    // by replay protection — the test tampers the digest, not the timestamp).
    let serialized = serde_json::to_string(&tampered).unwrap();
    state
        .store
        .connection()
        .execute(
            "UPDATE proofs SET signature = ?2 WHERE id = ?1",
            rusqlite::params![tampered.body.id.to_string(), serialized],
        )
        .unwrap();
    let (_, body) = response_json(
        router(state.clone()),
        "GET",
        &format!("/proofs/{}", proof.body.id),
        None,
    )
    .await;
    assert_eq!(body["verification"], "invalid");
}

#[tokio::test]
async fn audit_endpoint_returns_saved_execution_contexts() {
    let store = SqliteStore::in_memory().unwrap();
    let state = Arc::new(AppState::with_registry_and_store(
        "/tmp/proof-http-test",
        registry(),
        store,
    ));
    state
        .store
        .save_execution_context(&ExecutionContext {
            actor: state.keypair.principal_id,
            delegation_id: None,
            delegation_chain: None,
            workspace_path: "/tmp/proof-http-test".into(),
            timestamp: chrono::Utc::now(),
        })
        .unwrap();

    let (status, body) = response_json(router(state), "GET", "/audit", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["contexts"].as_array().unwrap().len(), 1);
}
