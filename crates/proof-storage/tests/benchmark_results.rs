use chrono::{TimeZone, Utc};
use proof_kernel::BenchmarkResult;
use proof_storage::SqliteStore;
use serde_json::json;

fn result(benchmark: &str, operation: &str, version: &str, duration_ms: u64) -> BenchmarkResult {
    BenchmarkResult {
        benchmark: benchmark.to_string(),
        operation: operation.to_string(),
        version: version.to_string(),
        passed: true,
        duration_ms,
        timestamp: Utc.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap(),
        failure: None,
    }
}

#[test]
fn benchmark_results_round_trip() {
    let store = SqliteStore::in_memory().unwrap();
    let result = result("B1", "schema.create", "v1", 4);

    store.save_benchmark_result(&result).unwrap();

    assert_eq!(
        store.list_benchmark_results("schema.create", "v1").unwrap(),
        vec![result]
    );
}

#[test]
fn benchmark_results_are_filtered_and_ordered() {
    let store = SqliteStore::in_memory().unwrap();
    let first = result("B1", "schema.create", "v1", 1);
    let second = result("B1", "schema.create", "v1", 2);
    let other_version = result("B1", "schema.create", "v2", 3);
    let other_operation = result("B1", "object.create", "v1", 4);

    store.save_benchmark_result(&first).unwrap();
    store.save_benchmark_result(&other_version).unwrap();
    store.save_benchmark_result(&second).unwrap();
    store.save_benchmark_result(&other_operation).unwrap();

    assert_eq!(
        store.list_benchmark_results("schema.create", "v1").unwrap(),
        vec![first, second]
    );
}

#[test]
fn benchmark_results_preserve_failures() {
    let store = SqliteStore::in_memory().unwrap();
    let mut failed = result("B1", "schema.create", "v1", 100);
    failed.passed = false;
    failed.failure = Some("duration 100 ms exceeded threshold 10 ms".to_string());

    store.save_benchmark_result(&failed).unwrap();

    assert_eq!(
        store.list_benchmark_results("schema.create", "v1").unwrap(),
        vec![failed]
    );
}

#[test]
fn benchmark_result_rollback_restores_v1_schema() {
    use proof_storage::sqlite::rollback_to;
    use tempfile::TempDir;

    let directory = TempDir::new().unwrap();
    let store = SqliteStore::open(&directory.path().join("proof.db")).unwrap();
    store
        .save_benchmark_result(&result("B1", "schema.create", "v1", 5))
        .unwrap();

    rollback_to(&store.connection(), 1).unwrap();

    let table_count: i64 = store
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'benchmark_results'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(table_count, 0);
}

#[test]
fn benchmark_result_json_contract_is_stable() {
    let result = result("B1", "schema.create", "v1", 2);
    let value = serde_json::to_value(&result).unwrap();

    assert_eq!(value["benchmark"], json!("B1"));
    assert_eq!(value["operation"], json!("schema.create"));
    assert_eq!(value["version"], json!("v1"));
    assert_eq!(value["passed"], json!(true));
    assert_eq!(value["duration_ms"], json!(2));
}
