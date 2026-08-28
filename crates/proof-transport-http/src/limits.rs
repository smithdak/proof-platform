//! Rate limiting and request size limits.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::sync::RwLock;
use std::time::{Duration, Instant};

pub(crate) const JSON_METHODS: [axum::http::Method; 2] =
    [axum::http::Method::POST, axum::http::Method::PUT];
pub(crate) const CONTENT_LENGTH: &str = "content-length";

const DEFAULT_RATE_LIMIT_PER_MINUTE: usize = 100;
const DEFAULT_REQUEST_BODY_LIMIT: usize = 1_024 * 1_024;

pub(crate) fn env_value(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
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
pub(crate) struct TokenBucket {
    capacity: usize,
    tokens: f64,
    refill_per_second: f64,
    last_refill: Instant,
}

impl TokenBucket {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            capacity,
            tokens: capacity as f64,
            refill_per_second: capacity as f64 / 60.0,
            last_refill: Instant::now(),
        }
    }

    pub(crate) fn take(&mut self) -> Option<Duration> {
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
pub(crate) struct RateLimiter {
    pub(crate) buckets: RwLock<BTreeMap<String, TokenBucket>>,
}

impl RateLimiter {
    pub(crate) fn new(_config: &RateLimitConfig) -> Self {
        Self {
            buckets: RwLock::new(BTreeMap::new()),
        }
    }
}

#[derive(Clone)]
pub struct HttpMiddlewareState {
    pub(crate) limiter: Arc<RateLimiter>,
    pub(crate) config: RateLimitConfig,
    pub(crate) body_limit: usize,
}
