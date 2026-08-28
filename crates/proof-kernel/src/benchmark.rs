//! Named operation benchmarks and timing validation.

use std::time::Instant;

use chrono::{DateTime, Utc};
use jsonschema::Validator;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::executor::{ExecutionContext, ExecutionEngine, ExecutionError};
use crate::registry::RegistryEntry;

#[derive(Debug, Error, PartialEq)]
pub enum BenchmarkError {
    #[error("invalid benchmark schema: {0}")]
    InvalidSchema(String),
    #[error("operation execution failed: {0}")]
    Execution(#[from] ExecutionError),
}

/// A named benchmark contract for an operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Benchmark {
    pub name: String,
    pub description: String,
    /// The maximum permitted execution time in milliseconds.
    pub max_duration_ms: u64,
    /// A JSON Schema that must validate the operation output.
    pub success_criteria: Value,
}

impl Benchmark {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        max_duration_ms: u64,
        success_criteria: Value,
    ) -> Result<Self, BenchmarkError> {
        Validator::new(&success_criteria)
            .map_err(|error| BenchmarkError::InvalidSchema(error.to_string()))?;
        if max_duration_ms == 0 {
            return Err(BenchmarkError::InvalidSchema(
                "max_duration_ms must be greater than zero".to_string(),
            ));
        }
        Ok(Self {
            name: name.into(),
            description: description.into(),
            max_duration_ms,
            success_criteria,
        })
    }
}

/// The outcome of a single benchmark run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkResult {
    pub benchmark: String,
    pub operation: String,
    pub version: String,
    pub passed: bool,
    pub duration_ms: u64,
    pub timestamp: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<String>,
}

impl BenchmarkResult {
    fn failure(benchmark: &Benchmark, operation: &RegistryEntry, reason: String) -> Self {
        Self {
            benchmark: benchmark.name.clone(),
            operation: operation.operation.clone(),
            version: operation.version.clone(),
            passed: false,
            duration_ms: 0,
            timestamp: Utc::now(),
            failure: Some(reason),
        }
    }
}

/// Executes operations through the kernel and evaluates benchmark contracts.
pub struct BenchmarkRunner;

impl BenchmarkRunner {
    /// Builds a benchmark from a registry entry's named benchmark ID.
    ///
    /// Benchmark definitions are not embedded in registry JSON, so callers supply
    /// the definition separately and this constructor enforces its contract.
    pub fn benchmark(
        benchmark_id: &str,
        description: &str,
        max_duration_ms: u64,
        success_criteria: Value,
    ) -> Result<Benchmark, BenchmarkError> {
        Benchmark::new(benchmark_id, description, max_duration_ms, success_criteria)
    }

    /// Runs an operation, measures its duration, and validates output against
    /// the benchmark's success criteria.
    pub fn run(
        &self,
        engine: &ExecutionEngine,
        benchmark: &Benchmark,
        operation: &str,
        version: &str,
        input: &Value,
        context: &ExecutionContext,
    ) -> Result<BenchmarkResult, BenchmarkError> {
        let entry = engine
            .operations()
            .iter()
            .find(|entry| entry.operation == operation && entry.version == version)
            .ok_or_else(|| {
                BenchmarkError::Execution(ExecutionError::OperationNotFound {
                    operation: operation.to_string(),
                    version: version.to_string(),
                })
            })?;

        let validator = Validator::new(&benchmark.success_criteria)
            .map_err(|error| BenchmarkError::InvalidSchema(error.to_string()))?;
        let started = Instant::now();
        let output = engine.execute(operation, version, input, context);
        let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);

        let output = match output {
            Ok(output) => output,
            Err(error) => {
                let mut result = BenchmarkResult::failure(benchmark, entry, error.to_string());
                result.duration_ms = duration_ms;
                return Ok(result);
            }
        };

        if duration_ms > benchmark.max_duration_ms {
            let mut result = BenchmarkResult::failure(
                benchmark,
                entry,
                format!(
                    "duration {} ms exceeded threshold {} ms",
                    duration_ms, benchmark.max_duration_ms
                ),
            );
            result.duration_ms = duration_ms;
            return Ok(result);
        }

        if validator.is_valid(&output) {
            Ok(BenchmarkResult {
                benchmark: benchmark.name.clone(),
                operation: entry.operation.clone(),
                version: entry.version.clone(),
                passed: true,
                duration_ms,
                timestamp: Utc::now(),
                failure: None,
            })
        } else {
            let mut result = BenchmarkResult::failure(
                benchmark,
                entry,
                "output did not satisfy benchmark success criteria".to_string(),
            );
            result.duration_ms = duration_ms;
            Ok(result)
        }
    }

    /// Convenience method for verifying a registry entry that declares a
    /// benchmark using the supplied benchmark definition.
    pub fn verify_entry(
        &self,
        engine: &ExecutionEngine,
        entry: &RegistryEntry,
        benchmark: &Benchmark,
        input: &Value,
        context: &ExecutionContext,
    ) -> Result<BenchmarkResult, BenchmarkError> {
        if entry.benchmark.as_deref() != Some(benchmark.name.as_str()) {
            return Err(BenchmarkError::InvalidSchema(format!(
                "benchmark {} is not declared by operation {} {}",
                benchmark.name, entry.operation, entry.version
            )));
        }
        self.run(
            engine,
            benchmark,
            &entry.operation,
            &entry.version,
            input,
            context,
        )
    }
}

impl ExecutionEngine {
    /// Verifies a registry entry's declared benchmark with the supplied
    /// definition. This is the kernel-facing integration point for transports
    /// that can execute operation benchmarks.
    pub fn verify_benchmark(
        &self,
        operation: &str,
        version: &str,
        benchmark: &Benchmark,
        input: &Value,
        context: &ExecutionContext,
    ) -> Result<BenchmarkResult, BenchmarkError> {
        let entry = self
            .operations()
            .iter()
            .find(|entry| entry.operation == operation && entry.version == version)
            .ok_or_else(|| {
                BenchmarkError::Execution(ExecutionError::OperationNotFound {
                    operation: operation.to_string(),
                    version: version.to_string(),
                })
            })?;
        BenchmarkRunner.verify_entry(self, entry, benchmark, input, context)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use super::*;
    use crate::executor::OperationHandler;
    use crate::identity::PrincipalId;
    use crate::registry::{Governance, Registry};
    use serde_json::json;

    struct SleepHandler {
        operation: &'static str,
        milliseconds: u64,
    }

    impl OperationHandler for SleepHandler {
        fn operation(&self) -> &str {
            self.operation
        }

        fn execute(
            &self,
            _input: &Value,
            _context: &ExecutionContext,
        ) -> Result<Value, ExecutionError> {
            std::thread::sleep(std::time::Duration::from_millis(self.milliseconds));
            Ok(json!({"status": "ready"}))
        }
    }

    fn entry(benchmark: Option<&str>) -> RegistryEntry {
        RegistryEntry {
            operation: "test.fast".to_string(),
            domain: "test".to_string(),
            version: "v1".to_string(),
            action: "test:fast".to_string(),
            description: "benchmarked operation".to_string(),
            input_schema: "input.json".to_string(),
            output_schema: "output.json".to_string(),
            required_authority: "delegation-grant".to_string(),
            governance: Governance::AgentExecutable,
            idempotency: "required-uuidv7".to_string(),
            consequence: "none".to_string(),
            evidence_contract: "operation-effect-v1".to_string(),
            benchmark: benchmark.map(ToString::to_string),
        }
    }

    fn engine(benchmark: Option<&str>) -> ExecutionEngine {
        let mut engine = ExecutionEngine::new(Registry::new(vec![entry(benchmark)]).unwrap());
        engine.register_handler(Arc::new(SleepHandler {
            operation: "test.fast",
            milliseconds: 2,
        }));
        engine
    }

    fn context() -> ExecutionContext {
        ExecutionContext {
            actor: PrincipalId::now(),
            delegation_id: None,
            delegation_chain: None,
            workspace_path: PathBuf::from("/tmp"),
            timestamp: Utc::now(),
        }
    }

    fn benchmark(max_duration_ms: u64) -> Benchmark {
        Benchmark::new(
            "B1",
            "fast operation",
            max_duration_ms,
            json!({"type": "object", "required": ["status"]}),
        )
        .unwrap()
    }

    #[test]
    fn rejects_invalid_schema_and_zero_threshold() {
        assert!(matches!(
            Benchmark::new("B1", "bad", 1, json!("not-a-schema")),
            Err(BenchmarkError::InvalidSchema(_))
        ));
        assert!(matches!(
            Benchmark::new("B1", "bad", 0, json!({})),
            Err(BenchmarkError::InvalidSchema(_))
        ));
    }

    #[test]
    fn passing_run_meets_threshold_and_schema() {
        let engine = engine(Some("B1"));
        let result = engine
            .verify_benchmark("test.fast", "v1", &benchmark(100), &json!({}), &context())
            .unwrap();
        assert!(result.passed);
        assert_eq!(result.benchmark, "B1");
        assert_eq!(result.operation, "test.fast");
        assert_eq!(result.version, "v1");
        assert!(result.failure.is_none());
    }

    #[test]
    fn slow_run_fails_with_duration() {
        let engine = engine(Some("B1"));
        let result = engine
            .verify_benchmark("test.fast", "v1", &benchmark(1), &json!({}), &context())
            .unwrap();
        assert!(!result.passed);
        assert!(result.duration_ms > 1);
        assert!(result.failure.unwrap().contains("exceeded threshold 1 ms"));
    }

    #[test]
    fn invalid_output_fails_success_criteria() {
        let engine = engine(Some("B1"));
        let criteria = json!({"type": "object", "required": ["different"]});
        let benchmark = Benchmark::new("B1", "strict", 100, criteria).unwrap();
        let result = engine
            .verify_benchmark("test.fast", "v1", &benchmark, &json!({}), &context())
            .unwrap();
        assert!(!result.passed);
        assert!(result.failure.unwrap().contains("success criteria"));
    }

    #[test]
    fn undeclared_benchmark_is_rejected() {
        let engine = engine(None);
        assert!(matches!(
            engine.verify_benchmark("test.fast", "v1", &benchmark(100), &json!({}), &context()),
            Err(BenchmarkError::InvalidSchema(_))
        ));
    }

    #[test]
    fn unknown_operation_is_rejected() {
        let engine = engine(Some("B1"));
        assert!(matches!(
            engine.verify_benchmark("missing", "v1", &benchmark(100), &json!({}), &context()),
            Err(BenchmarkError::Execution(
                ExecutionError::OperationNotFound { .. }
            ))
        ));
    }

    #[test]
    fn execution_error_is_reported_as_failed_benchmark() {
        let mut engine = ExecutionEngine::new(Registry::new(vec![entry(Some("B1"))]).unwrap());
        engine.register_handler(Arc::new(FailingHandler));
        let result = engine
            .verify_benchmark("test.fast", "v1", &benchmark(100), &json!({}), &context())
            .unwrap();
        assert!(!result.passed);
        assert!(result.duration_ms <= 100);
        assert!(result.failure.unwrap().contains("handler execution failed"));
    }

    struct FailingHandler;

    impl OperationHandler for FailingHandler {
        fn operation(&self) -> &str {
            "test.fast"
        }

        fn execute(
            &self,
            _input: &Value,
            _context: &ExecutionContext,
        ) -> Result<Value, ExecutionError> {
            Err(ExecutionError::HandlerFailed("deliberate".to_string()))
        }
    }
}
