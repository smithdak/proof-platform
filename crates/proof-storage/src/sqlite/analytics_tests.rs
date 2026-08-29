use super::analytics::{
    AnalyticsInsight, AnalyticsInsightStatus, AnalyticsQuery, AnalyticsSnapshot,
};
use super::store::SqliteStore;
use crate::StorageError;
use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

fn test_snapshot() -> AnalyticsSnapshot {
    AnalyticsSnapshot {
        id: Uuid::now_v7(),
        name: "Q3 revenue".to_string(),
        description: "Frozen quarterly dataset".to_string(),
        digest: "blake3-analytics-digest".to_string(),
        created_at: Utc::now(),
    }
}

fn test_query(snapshot: &AnalyticsSnapshot) -> AnalyticsQuery {
    AnalyticsQuery {
        id: Uuid::now_v7(),
        snapshot_id: snapshot.id,
        name: "Revenue by region".to_string(),
        filter: json!({"region": "EMEA"}),
        aggregation: json!({"sum": "revenue"}),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn test_insight(query: &AnalyticsQuery) -> AnalyticsInsight {
    AnalyticsInsight {
        id: Uuid::now_v7(),
        query_id: query.id,
        result_digest: "blake3-result-digest".to_string(),
        status: AnalyticsInsightStatus::Pending,
        approved_at: None,
        approved_by: None,
    }
}

#[test]
fn analytics_snapshot_round_trips() {
    let store = SqliteStore::in_memory().unwrap();
    let snapshot = test_snapshot();

    store.save_analytics_snapshot(&snapshot).unwrap();
    let loaded = store.load_analytics_snapshot(&snapshot.id).unwrap();

    assert_eq!(loaded, snapshot);
}

#[test]
fn analytics_query_lifecycle_round_trips() {
    let store = SqliteStore::in_memory().unwrap();
    let snapshot = test_snapshot();
    let mut query = test_query(&snapshot);
    store.save_analytics_snapshot(&snapshot).unwrap();

    store.save_analytics_query(&query).unwrap();
    assert_eq!(store.load_analytics_query(&query.id).unwrap(), query);

    query.name = "Updated revenue".to_string();
    query.filter = json!({"region": "APAC"});
    query.updated_at = Utc::now();
    store.save_analytics_query(&query).unwrap();
    assert_eq!(store.load_analytics_query(&query.id).unwrap(), query);
    assert_eq!(
        store.list_analytics_queries(&snapshot.id).unwrap(),
        vec![query.clone()]
    );

    assert!(store.delete_analytics_query(&query.id).unwrap());
    assert!(matches!(
        store.load_analytics_query(&query.id),
        Err(StorageError::NotFound(_))
    ));
}

#[test]
fn analytics_insight_approval_round_trips() {
    let store = SqliteStore::in_memory().unwrap();
    let snapshot = test_snapshot();
    let query = test_query(&snapshot);
    let mut insight = test_insight(&query);
    store.save_analytics_snapshot(&snapshot).unwrap();
    store.save_analytics_query(&query).unwrap();

    store.save_analytics_insight(&insight).unwrap();
    assert_eq!(store.load_analytics_insight(&insight.id).unwrap(), insight);
    assert_eq!(
        store.list_analytics_insights(&query.id).unwrap(),
        vec![insight.clone()]
    );

    insight.status = AnalyticsInsightStatus::Approved;
    insight.approved_at = Some(Utc::now());
    insight.approved_by = Some("human-principal".to_string());
    store.save_analytics_insight(&insight).unwrap();
    assert_eq!(store.load_analytics_insight(&insight.id).unwrap(), insight);

    assert!(store.delete_analytics_insight(&insight.id).unwrap());
    assert!(matches!(
        store.load_analytics_insight(&insight.id),
        Err(StorageError::NotFound(_))
    ));
}
