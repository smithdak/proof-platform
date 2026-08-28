//! Structured observability utilities for the Proof platform.

use std::fmt;
use std::time::Instant;

use axum::body::Body;
use axum::http::{HeaderValue, Request, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::Context;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::Layer;

/// Configures verbosity for the process-wide tracing subscriber.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Verbosity {
    /// Warnings and errors.
    Normal,
    /// Information, warnings, and errors.
    Verbose,
    /// Debug output, warnings, and errors.
    Debug,
    /// Trace output and all lower levels.
    Trace,
}

impl Verbosity {
    const fn level(self) -> Level {
        match self {
            Self::Normal => Level::WARN,
            Self::Verbose => Level::INFO,
            Self::Debug => Level::DEBUG,
            Self::Trace => Level::TRACE,
        }
    }
}

/// A JSON event emitted by `JsonCollector`.
#[derive(Debug)]
pub struct JsonEvent {
    pub level: Level,
    pub message: String,
    pub fields: Vec<(String, String)>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

struct CollectorVisitor {
    level: Level,
    timestamp: chrono::DateTime<chrono::Utc>,
    message: Option<String>,
    fields: Vec<(String, String)>,
}

impl CollectorVisitor {
    fn new(level: Level, timestamp: chrono::DateTime<chrono::Utc>) -> Self {
        Self {
            level,
            timestamp,
            message: None,
            fields: Vec::new(),
        }
    }
}

impl Visit for CollectorVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            self.message = Some(format!("{value:?}"));
        } else {
            self.fields
                .push((field.name().to_string(), format!("{value:?}")));
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_string());
        } else {
            self.fields
                .push((field.name().to_string(), value.to_string()));
        }
    }
}

struct JsonCollector {
    max_level: Level,
    enabled: bool,
}

impl JsonCollector {
    fn write_event(&self, event: &Event<'_>) {
        let metadata = event.metadata();
        let mut visitor = CollectorVisitor::new(*metadata.level(), chrono::Utc::now());
        event.record(&mut visitor);
        let json = serde_json::json!({
            "level": visitor.level.to_string(),
            "message": visitor.message.unwrap_or_default(),
            "fields": visitor.fields.into_iter().collect::<Vec<_>>(),
            "timestamp": visitor.timestamp.to_rfc3339(),
        });
        eprintln!("{json}");
    }
}

impl<S: Subscriber> Layer<S> for JsonCollector {
    fn enabled(&self, metadata: &tracing::Metadata<'_>, _: Context<'_, S>) -> bool {
        metadata.level() <= &self.max_level && self.enabled
    }

    fn on_event(&self, event: &Event<'_>, _: Context<'_, S>) {
        self.write_event(event);
    }
}

/// Installs a JSON tracing subscriber writing to stderr.
///
/// Reinitializing tracing is process-wide, so this returns `false` if a
/// subscriber is already installed.
pub fn init_json_stderr(verbosity: Verbosity) -> bool {
    let collector = JsonCollector {
        max_level: verbosity.level(),
        enabled: true,
    };
    tracing::subscriber::set_global_default(tracing_subscriber::registry().with(collector)).is_ok()
}

/// Observability data for one governed operation execution.
#[derive(Debug)]
pub struct OperationSpan {
    operation: String,
    version: String,
    actor: String,
    proof_id: Option<String>,
    success: Option<bool>,
    started_at: Instant,
}

impl OperationSpan {
    /// Starts an operation execution span.
    pub fn new(
        operation: impl Into<String>,
        version: impl Into<String>,
        actor: impl Into<String>,
    ) -> Self {
        let operation = operation.into();
        let version = version.into();
        let actor = actor.into();
        tracing::info!(
            operation = %operation,
            version = %version,
            actor = %actor,
            "operation started"
        );
        Self {
            operation,
            version,
            actor,
            proof_id: None,
            success: None,
            started_at: Instant::now(),
        }
    }

    /// Records the proof generated for the execution.
    pub fn set_proof_id(&mut self, proof_id: impl Into<String>) {
        self.proof_id = Some(proof_id.into());
    }

    /// Records success and emits the completion event.
    pub fn record_success(mut self) {
        self.success = Some(true);
        self.finish();
    }

    /// Records failure and emits the completion event.
    pub fn record_failure(mut self) {
        self.success = Some(false);
        self.finish();
    }

    fn finish(self) {
        let duration = self.started_at.elapsed();
        let proof_id = self.proof_id.unwrap_or_default();
        match self.success {
            Some(true) => tracing::info!(
                operation = %self.operation,
                version = %self.version,
                actor = %self.actor,
                proof_id = %proof_id,
                duration_ms = duration.as_millis() as u64,
                success = true,
                "operation completed"
            ),
            _ => tracing::warn!(
                operation = %self.operation,
                version = %self.version,
                actor = %self.actor,
                proof_id = %proof_id,
                duration_ms = duration.as_millis() as u64,
                success = false,
                "operation failed"
            ),
        }
    }
}

/// A request identifier propagated to handlers and responses.
#[derive(Clone, Debug)]
pub struct RequestId(pub String);

/// Middleware that adds a request ID and structured request metrics.
pub async fn request_middleware(request: Request<Body>, next: Next) -> Response {
    let request_id = uuid::Uuid::now_v7().to_string();
    let path = request.uri().path().to_owned();
    let started_at = Instant::now();
    let mut request = request;
    request
        .extensions_mut()
        .insert(RequestId(request_id.clone()));

    let response = next.run(request).await;
    let status = response.status();
    let mut response = response;
    response.headers_mut().insert(
        "x-request-id",
        HeaderValue::from_str(&request_id).expect("UUID is a valid header value"),
    );
    tracing::info!(
        request_id = %request_id,
        path = %path,
        status = status.as_u16(),
        duration_ms = started_at.elapsed().as_millis() as u64,
        "request completed"
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn middleware_records_response() {
        async fn endpoint() -> StatusCode {
            StatusCode::OK
        }
        let response = tower::ServiceExt::oneshot(
            axum::routing::get(endpoint)
                .layer::<_, std::convert::Infallible>(axum::middleware::from_fn(request_middleware))
                .with_state(()),
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().contains_key("x-request-id"));
    }
}
