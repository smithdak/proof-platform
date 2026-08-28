//! HTTP/REST transport adapter for the Proof platform.

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header::CONTENT_TYPE, HeaderValue, Method, Request, StatusCode},
    response::{IntoResponse, Json},
    routing::{get, post},
    Router,
};
use bytes::Bytes;
use proof_kernel::{
    create_proof, generate_keypair, principal_from_keypair, ExecutionContext, ExecutionEngine,
    ExecutionError, Keypair, OperationHandler, Proof, Registry,
};
use proof_storage::SqliteStore;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, RwLock},
    time::{Duration, Instant},
};
use tower_http::limit::RequestBodyLimitLayer;
use uuid::Uuid;

pub struct AppState {
    pub workspace_path: String,
    pub version: String,
    pub engine: Arc<RwLock<ExecutionEngine>>,
    pub keypair: Keypair,
    pub store: Arc<SqliteStore>,
}

pub type SharedState = Arc<AppState>;

const DEFAULT_RATE_LIMIT_PER_MINUTE: usize = 100;
const DEFAULT_REQUEST_BODY_LIMIT: usize = 1_024 * 1_024;
const JSON_METHODS: [Method; 2] = [Method::POST, Method::PUT];
const CONTENT_LENGTH: &str = "content-length";

impl AppState {
    pub fn new(workspace_path: impl Into<String>) -> Result<Self, proof_kernel::RegistryError> {
        let workspace_path = workspace_path.into();
        let registry =
            Registry::load_from_directory(PathBuf::from(&workspace_path).join(".proof/registry"))?;
        let database_path =
            PathBuf::from(&workspace_path).join(".proof/data/proofs/proofs.sqlite3");
        if let Some(parent) = database_path.parent() {
            std::fs::create_dir_all(parent).map_err(proof_kernel::RegistryError::Io)?;
        }
        let store = SqliteStore::open(&database_path).map_err(|error| {
            proof_kernel::RegistryError::Io(std::io::Error::other(error.to_string()))
        })?;
        Ok(Self::with_registry_and_store(
            workspace_path,
            registry,
            store,
        ))
    }

    pub fn with_registry(workspace_path: impl Into<String>, registry: Registry) -> Self {
        Self::with_registry_and_store(
            workspace_path,
            registry,
            SqliteStore::in_memory().expect("in-memory SQLite should initialize"),
        )
    }

    pub fn with_registry_and_store(
        workspace_path: impl Into<String>,
        registry: Registry,
        store: SqliteStore,
    ) -> Self {
        Self {
            workspace_path: workspace_path.into(),
            version: "0.1.0".to_string(),
            engine: Arc::new(RwLock::new(ExecutionEngine::new(registry))),
            keypair: generate_keypair(),
            store: Arc::new(store),
        }
    }

    pub fn register_handler(&self, handler: Arc<dyn OperationHandler>) {
        self.engine.write().unwrap().register_handler(handler);
    }
}

#[derive(Clone, Debug)]
pub struct RateLimitConfig {
    pub requests_per_minute: usize,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            requests_per_minute: env_value(
                "PROOF_RATE_LIMIT_PER_MINUTE",
                DEFAULT_RATE_LIMIT_PER_MINUTE,
            ),
        }
    }
}

#[derive(Clone, Debug)]
pub struct HttpLimits {
    pub rate_limit: RateLimitConfig,
    pub body_limit: usize,
}

impl Default for HttpLimits {
    fn default() -> Self {
        Self {
            rate_limit: RateLimitConfig::default(),
            body_limit: env_value("PROOF_REQUEST_BODY_LIMIT", DEFAULT_REQUEST_BODY_LIMIT),
        }
    }
}

#[derive(Clone, Debug)]
struct TokenBucket {
    capacity: usize,
    tokens: f64,
    refill_per_second: f64,
    last_refill: Instant,
}

impl TokenBucket {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            tokens: capacity as f64,
            refill_per_second: capacity as f64 / 60.0,
            last_refill: Instant::now(),
        }
    }

    fn take(&mut self) -> Option<Duration> {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.last_refill = now;
        self.tokens = (self.tokens + elapsed * self.refill_per_second).min(self.capacity as f64);

        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            None
        } else {
            Some(Duration::from_secs_f64(
                (1.0 - self.tokens) / self.refill_per_second,
            ))
        }
    }
}

#[derive(Default)]
struct RateLimiter {
    buckets: RwLock<BTreeMap<String, TokenBucket>>,
}

impl RateLimiter {
    fn new(_config: &RateLimitConfig) -> Self {
        Self {
            buckets: RwLock::new(BTreeMap::new()),
        }
    }
}

#[derive(Clone)]
struct HttpMiddlewareState {
    limiter: Arc<RateLimiter>,
    config: RateLimitConfig,
    body_limit: usize,
}

pub fn router(state: SharedState) -> Router {
    router_with_limits(state, HttpLimits::default())
}

pub fn router_with_limits(state: SharedState, limits: HttpLimits) -> Router {
    let middleware_state = HttpMiddlewareState {
        limiter: Arc::new(RateLimiter::new(&limits.rate_limit)),
        config: limits.rate_limit,
        body_limit: limits.body_limit,
    };
    Router::new()
        .route("/", get(root))
        .route("/health", get(health))
        .route("/capabilities", get(capabilities))
        .route("/v1/operations/:name/:version", post(execute_operation))
        .route("/v1/schemas", get(list_schemas))
        .route("/v1/objects", get(list_objects))
        .route("/v1/proofs", get(list_proofs))
        .route("/v1/proofs/:id", get(get_proof))
        .route("/proofs", get(list_proofs_filtered))
        .route("/proofs/:id", get(get_proof))
        .route("/proofs/verify", post(verify_proof))
        .route("/audit", get(list_audit))
        .with_state(state)
        .layer(RequestBodyLimitLayer::new(limits.body_limit))
        .layer(axum::middleware::from_fn_with_state(
            middleware_state,
            validate_request,
        ))
}

fn env_value(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

fn client_ip(request: &Request<Body>) -> String {
    request
        .headers()
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| request_ip(request))
}

fn request_ip(request: &Request<Body>) -> String {
    request
        .extensions()
        .get::<std::net::SocketAddr>()
        .map(|address| address.ip().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

async fn validate_request(
    axum::extract::State(state): axum::extract::State<HttpMiddlewareState>,
    request: Request<Body>,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let client_ip = client_ip(&request);
    let retry_after = {
        let mut buckets = state.limiter.buckets.write().unwrap();
        buckets
            .entry(client_ip)
            .or_insert_with(|| TokenBucket::new(state.config.requests_per_minute))
            .take()
    };

    if let Some(retry_after) = retry_after {
        return rate_limited_response(retry_after);
    }

    let method = request.method().clone();
    if JSON_METHODS.contains(&method) {
        if let Some(content_length) = request
            .headers()
            .get(CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<usize>().ok())
        {
            if content_length > state.body_limit {
                return payload_too_large(state.body_limit);
            }
        }
        if let Some(response) = validate_content_type(&request) {
            return response;
        }
        let (parts, body) = request.into_parts();
        let bytes = match axum::body::to_bytes(body, state.body_limit).await {
            Ok(bytes) => bytes,
            Err(_) => {
                return payload_too_large(state.body_limit);
            }
        };
        if let Err(error) = parse_json(&bytes) {
            return json_error_response(
                StatusCode::BAD_REQUEST,
                json!({
                    "error": "invalid JSON",
                    "detail": error,
                }),
            );
        }
        let request = Request::from_parts(parts, Body::from(bytes));
        return next.run(request).await;
    }

    next.run(request).await
}

fn validate_content_type(request: &Request<Body>) -> Option<axum::response::Response> {
    let content_type = request
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .unwrap_or_default();
    if content_type.eq_ignore_ascii_case("application/json") {
        return None;
    }
    Some(json_error_response(
        StatusCode::BAD_REQUEST,
        json!({"error": "Content-Type must be application/json"}),
    ))
}

fn parse_json(bytes: &Bytes) -> Result<(), String> {
    serde_json::from_slice::<Value>(bytes)
        .map(|_| ())
        .map_err(|error| {
            if bytes.is_empty() {
                "request body must contain a JSON object".to_string()
            } else {
                error.to_string()
            }
        })
}

fn payload_too_large(limit: usize) -> axum::response::Response {
    json_error_response(
        StatusCode::PAYLOAD_TOO_LARGE,
        json!({
            "error": "request body too large",
            "limit_bytes": limit,
        }),
    )
}

fn rate_limited_response(retry_after: Duration) -> axum::response::Response {
    let mut response = json_error_response(
        StatusCode::TOO_MANY_REQUESTS,
        json!({
            "error": "rate limit exceeded",
            "retry_after_seconds": retry_after.as_secs().max(1),
        }),
    );
    response.headers_mut().insert(
        "Retry-After",
        HeaderValue::from(retry_after.as_secs().max(1)),
    );
    response
}

fn json_error_response(status: StatusCode, body: Value) -> axum::response::Response {
    (status, Json(body)).into_response()
}

async fn root() -> impl IntoResponse {
    Json(json!({
        "name": "proof",
        "description": "Governed agent-native content platform",
        "api_version": "v1"
    }))
}

async fn health() -> impl IntoResponse {
    Json(json!({"status": "ok"}))
}

async fn capabilities() -> impl IntoResponse {
    Json(json!({
        "operations": [
            {"name": "object.create", "version": "v1", "domain": "content", "governance": "agent-executable"},
            {"name": "schema.create", "version": "v1", "domain": "content", "governance": "agent-executable"},
            {"name": "changeset.create", "version": "v1", "domain": "content", "governance": "agent-executable"},
            {"name": "edition.create", "version": "v1", "domain": "content", "governance": "agent-executable"},
            {"name": "release.publish", "version": "v1", "domain": "content", "governance": "agent-executable"}
        ]
    }))
}

async fn list_schemas(State(state): State<SharedState>) -> impl IntoResponse {
    let dir = std::path::Path::new(&state.workspace_path).join(".proof/data/schemas");
    let mut schemas = vec![];
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                if let Ok(value) = serde_json::from_str::<Value>(&content) {
                    schemas.push(value);
                }
            }
        }
    }
    Json(json!({"schemas": schemas}))
}

async fn list_objects(State(state): State<SharedState>) -> impl IntoResponse {
    let dir = std::path::Path::new(&state.workspace_path).join(".proof/data/objects");
    let mut objects = vec![];
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                if let Ok(value) = serde_json::from_str::<Value>(&content) {
                    objects.push(value);
                }
            }
        }
    }
    Json(json!({"objects": objects}))
}

#[derive(Default, serde::Deserialize)]
struct ProofFilters {
    operation: Option<String>,
    version: Option<String>,
    actor: Option<Uuid>,
}

async fn list_proofs(
    State(state): State<SharedState>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    list_proofs_inner(&state, ProofFilters::default()).await
}

async fn list_proofs_filtered(
    State(state): State<SharedState>,
    Query(filters): Query<ProofFilters>,
) -> impl IntoResponse {
    list_proofs_inner(&state, filters).await
}

async fn list_proofs_inner(
    state: &SharedState,
    filters: ProofFilters,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let connection = state.store.connection();
    let mut sql = "
        SELECT signature, operation, actor
        FROM proofs
        WHERE (?1 IS NULL OR operation = ?1)
          AND (?2 IS NULL OR actor = ?2)
        ORDER BY timestamp DESC
    "
    .to_string();
    if filters.operation.is_some() && filters.version.is_some() {
        sql.push_str(" LIMIT 0");
    }
    let mut statement = connection
        .prepare(&sql)
        .map_err(|error| internal_error(error.to_string()))?;
    let serialized_proofs = statement
        .query_map(
            rusqlite::params![
                filters.operation,
                filters.actor.map(|actor| actor.to_string()),
            ],
            |row| row.get::<_, String>(0),
        )
        .map_err(|error| internal_error(error.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| internal_error(error.to_string()))?;
    let proofs = serialized_proofs
        .iter()
        .map(|serialized| serde_json::from_str::<Proof>(serialized))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| internal_error(error.to_string()))?;
    Ok(Json(json!({ "proofs": proofs })))
}

async fn get_proof(
    State(state): State<SharedState>,
    Path(proof_id): Path<Uuid>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let proof = state
        .store
        .load_proof(&proof_id)
        .map_err(|error| match error {
            proof_storage::StorageError::NotFound(_) => (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "proof not found"})),
            ),
            error => internal_error(error.to_string()),
        })?;
    Ok(Json(json!({
        "proof": proof,
        "verification": verification_status(&proof),
    })))
}

#[derive(serde::Deserialize)]
struct VerifyProofRequest {
    proof_id: Uuid,
}

async fn verify_proof(
    State(state): State<SharedState>,
    Json(request): Json<VerifyProofRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let proof = state
        .store
        .load_proof(&request.proof_id)
        .map_err(|error| match error {
            proof_storage::StorageError::NotFound(_) => (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "proof not found"})),
            ),
            error => internal_error(error.to_string()),
        })?;
    let proof_actor = proof.body.actor;
    let public_key = if proof_actor == state.keypair.principal_id {
        principal_from_keypair(&state.keypair).public_key
    } else {
        state
            .store
            .load_principal(&proof_actor)
            .map_err(|error| match error {
                proof_storage::StorageError::NotFound(_) => (
                    StatusCode::NOT_FOUND,
                    Json(json!({"error": "signing principal not found"})),
                ),
                error => internal_error(error.to_string()),
            })?
            .public_key
    };
    Ok(Json(json!({
        "proof_id": request.proof_id,
        "valid": proof.verify(&public_key).is_ok(),
    })))
}

async fn list_audit(
    State(state): State<SharedState>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let connection = state.store.connection();
    let mut statement = connection
        .prepare(
            "SELECT id, actor, workspace_path, timestamp
             FROM execution_contexts
             ORDER BY timestamp DESC",
        )
        .map_err(|error| internal_error(error.to_string()))?;
    let audit = statement
        .query_map([], |row| {
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "actor": row.get::<_, String>(1)?,
                "workspace_path": row.get::<_, String>(2)?,
                "timestamp": row.get::<_, String>(3)?,
            }))
        })
        .map_err(|error| internal_error(error.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| internal_error(error.to_string()))?;
    Ok(Json(json!({ "contexts": audit })))
}

fn verification_status(proof: &Proof) -> &'static str {
    match proof {
        Proof { .. } => "unverified",
    }
}

async fn execute_operation(
    State(state): State<SharedState>,
    Path((name, version)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let keypair = state.keypair.clone();
    let context = ExecutionContext {
        actor: keypair.principal_id,
        delegation_id: None,
        delegation_chain: None,
        workspace_path: PathBuf::from(&state.workspace_path),
        timestamp: chrono::Utc::now(),
    };

    let result = match state
        .engine
        .read()
        .unwrap()
        .execute(&name, &version, &body, &context)
    {
        Ok(result) => result,
        Err(error) => return Err(execution_error_response(&error)),
    };

    let proof = match create_proof(
        keypair.principal_id,
        context.delegation_id,
        &name,
        &body,
        &result,
        context.timestamp,
        &keypair,
    ) {
        Ok(proof) => proof,
        Err(error) => return Err(internal_error(error.to_string())),
    };

    let proof = serde_json::to_value(&proof).map_err(|error| internal_error(error.to_string()))?;

    Ok(Json(json!({
        "operation": name,
        "version": version,
        "status": "executed",
        "result": result,
        "proof": proof,
    })))
}

fn execution_error_response(error: &ExecutionError) -> (StatusCode, Json<Value>) {
    let status = match error {
        ExecutionError::OperationNotFound { .. } => StatusCode::NOT_FOUND,
        ExecutionError::HumanOnly => StatusCode::FORBIDDEN,
        ExecutionError::Sunset => StatusCode::GONE,
        ExecutionError::NoHandler(_)
        | ExecutionError::HandlerFailed(_)
        | ExecutionError::EvidenceFailed(_)
        | ExecutionError::Delegation(_)
        | ExecutionError::StorageFailed(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, Json(json!({"error": error.to_string()})))
}

fn internal_error(error: String) -> (StatusCode, Json<Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "error": error })),
    )
}
