//! HTTP transport for the Proof platform.

mod handlers;
mod limits;
pub(crate) mod middleware;
mod state;

pub use state::{AppState, SharedState};

use axum::{
    middleware as axum_mw,
    routing::{get, post},
    Router,
};
use std::sync::Arc;

use axum::extract::DefaultBodyLimit as RequestBodyLimitLayer;
use handlers::operations::execute_operation;
use handlers::proofs::{get_proof, list_audit, list_proofs, list_proofs_filtered, verify_proof};
use handlers::system::{
    capabilities, health, list_catalog, list_objects, list_orders, list_schemas,
    list_workflow_runs, list_workflows, root,
};
use handlers::system::{list_analytics_queries, list_analytics_snapshots};
pub use limits::HttpMiddlewareState;
pub use limits::{HttpLimits, RateLimitConfig};
use limits::{RateLimiter, CONTENT_LENGTH, JSON_METHODS};
use middleware::validate_request;

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
        .route("/catalog", get(list_catalog))
        .route("/orders", get(list_orders))
        .route("/workflows", get(list_workflows))
        .route("/workflow-runs", get(list_workflow_runs))
        .route("/analytics-snapshots", get(list_analytics_snapshots))
        .route("/analytics-queries", get(list_analytics_queries))
        .route("/v1/proofs", get(list_proofs))
        .route("/v1/proofs/:id", get(get_proof))
        .route("/proofs", get(list_proofs_filtered))
        .route("/proofs/:id", get(get_proof))
        .route("/proofs/verify", post(verify_proof))
        .route("/audit", get(list_audit))
        .with_state(state)
        .layer(RequestBodyLimitLayer::max(limits.body_limit))
        .layer(axum::middleware::from_fn_with_state(
            middleware_state,
            validate_request,
        ))
}
