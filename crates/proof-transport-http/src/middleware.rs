//! Request validation, rate limiting middleware, and JSON helpers.

use super::limits::env_value;
use super::limits::{
    HttpLimits, HttpMiddlewareState, RateLimiter, TokenBucket, CONTENT_LENGTH, JSON_METHODS,
};
use super::state::SharedState;
use axum::http::HeaderValue;
use axum::{
    body::{Body, Bytes},
    extract::Request,
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use std::time::Duration;

pub(crate) async fn validate_request(
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

pub(crate) const CONTENT_TYPE: &str = "content-type";

pub(crate) fn client_ip(request: &Request<Body>) -> String {
    request
        .headers()
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|forwarded| forwarded.split(',').next())
        .unwrap_or("unknown")
        .trim()
        .to_string()
}

pub(crate) fn validate_content_type(request: &Request<Body>) -> Option<axum::response::Response> {
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

pub(crate) fn parse_json(bytes: &Bytes) -> Result<(), String> {
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

pub(crate) fn payload_too_large(limit: usize) -> axum::response::Response {
    json_error_response(
        StatusCode::PAYLOAD_TOO_LARGE,
        json!({
            "error": "request body too large",
            "limit_bytes": limit,
        }),
    )
}

pub(crate) fn rate_limited_response(retry_after: Duration) -> axum::response::Response {
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
