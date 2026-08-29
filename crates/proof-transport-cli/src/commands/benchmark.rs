use crate::{build_engine, load_registry, open_store, Cli, Workspace};
use anyhow::{bail, Context, Result};
use proof_kernel::{Benchmark, BenchmarkResult, BenchmarkRunner, ExecutionContext, RegistryEntry};
use proof_storage::SqliteStore;
use serde_json::{json, Value};

pub fn cmd_benchmark_run(
    cli: &Cli,
    operation: &str,
    version: &str,
    threshold_ms: u64,
    runs: u32,
    input: &str,
) -> Result<()> {
    if runs == 0 {
        bail!("--runs must be greater than zero");
    }
    let ws = Workspace::open(&cli.workspace)?;
    let input_value: Value = serde_json::from_str(input).context("invalid input JSON")?;
    let registry = load_registry(&ws.root)?;
    let entry = registry
        .find(operation, version)
        .with_context(|| format!("operation not found: {operation} {version}"))?
        .clone();
    let benchmark = benchmark_contract(&entry, threshold_ms)?;
    let engine = build_engine(registry)?;
    let context = ExecutionContext {
        actor: ws.actor,
        principal_kind: Some(proof_kernel::PrincipalKind::Agent),
        delegation_id: None,
        delegation_chain: None,
        workspace_path: ws.root.clone(),
        timestamp: chrono::Utc::now(),
    };
    let runner = BenchmarkRunner;
    let mut results = Vec::with_capacity(runs as usize);
    let store = open_store(&ws.root)?;
    for _ in 0..runs {
        let result = runner
            .run(
                &engine,
                &benchmark,
                operation,
                version,
                &input_value,
                &context,
            )
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        store
            .save_benchmark_result(&result)
            .map_err(anyhow::Error::from)?;
        results.push(result);
    }
    let summary = benchmark_summary(&results);
    let passed = summary["failed"] == 0;
    println!(
        "{}",
        serde_json::json!({
            "status": if passed { "passed" } else { "failed" },
            "operation": operation,
            "version": version,
            "benchmark": benchmark.name,
            "threshold_ms": threshold_ms,
            "summary": summary,
        })
    );
    if !passed {
        bail!("benchmark failed");
    }
    Ok(())
}

fn benchmark_contract(entry: &RegistryEntry, threshold_ms: u64) -> Result<Benchmark> {
    let benchmark_id = entry.benchmark.as_deref().with_context(|| {
        format!(
            "operation {} {} does not declare a benchmark",
            entry.operation, entry.version
        )
    })?;
    if threshold_ms == 0 {
        bail!("--threshold-ms must be greater than zero");
    }
    BenchmarkRunner::benchmark(
        benchmark_id,
        "CLI operation benchmark",
        threshold_ms,
        serde_json::json!({"type": "object"}),
    )
    .map_err(|error| anyhow::anyhow!(error.to_string()))
}

fn benchmark_summary(results: &[BenchmarkResult]) -> Value {
    let mut durations: Vec<u64> = results.iter().map(|result| result.duration_ms).collect();
    durations.sort_unstable();
    let percentile = |percentile: f64| -> u64 {
        if durations.is_empty() {
            0
        } else {
            let index = ((durations.len() as f64 - 1.0) * percentile).ceil() as usize;
            durations[index.min(durations.len() - 1)]
        }
    };
    let count = durations.len() as u64;
    let total: u128 = durations.iter().map(|duration| *duration as u128).sum();
    serde_json::json!({
        "runs": count,
        "passed": results.iter().filter(|result| result.passed).count(),
        "failed": results.iter().filter(|result| !result.passed).count(),
        "avg_ms": if count == 0 { 0.0 } else { total as f64 / count as f64 },
        "p95_ms": percentile(0.95),
        "max_ms": durations.last().copied().unwrap_or(0),
    })
}

pub fn cmd_benchmark_report(cli: &Cli) -> Result<()> {
    let ws = Workspace::open(&cli.workspace)?;
    let store = open_store(&ws.root)?;
    let connection = store.connection();
    let mut statement = connection.prepare_cached(
        "
        SELECT
            operation,
            version,
            COUNT(*) AS runs,
            SUM(passed) AS passed,
            ROUND(AVG(duration_ms), 3) AS avg_ms,
            MAX(duration_ms) AS max_ms
        FROM benchmark_results
        GROUP BY operation, version
        ORDER BY operation, version
        ",
    )?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, f64>(4)?,
                row.get::<_, i64>(5)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    println!(
        "{:<28} {:<10} {:>6} {:>8} {:>12} {:>10}",
        "OPERATION", "VERSION", "RUNS", "PASSED", "AVG MS", "MAX MS"
    );
    for (operation, version, runs, passed, average, maximum) in rows {
        println!(
            "{:<28} {:<10} {:>6} {:>8} {:>12.3} {:>10}",
            operation, version, runs, passed, average, maximum
        );
    }
    Ok(())
}
