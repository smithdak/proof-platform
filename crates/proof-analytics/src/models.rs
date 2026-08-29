use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

pub type AnalyticsSnapshotId = Uuid;
pub type AnalyticsQueryId = Uuid;
pub type AnalyticsInsightId = Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalyticsQueryStatus {
    Pending,
    Executed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalyticsInsightStatus {
    Pending,
    Approved,
}

#[derive(Debug, Clone, Error)]
#[error("invalid analytics query transition from {from:?} to {to}")]
pub struct AnalyticsQueryTransitionError {
    from: AnalyticsQueryStatus,
    to: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnalyticsSnapshot {
    pub id: AnalyticsSnapshotId,
    pub name: String,
    pub description: String,
    pub digest: String,
    pub created_at: DateTime<Utc>,
}

impl AnalyticsSnapshot {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        digest: impl Into<String>,
    ) -> Result<Self, String> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err("name must not be empty".to_string());
        }
        let digest = digest.into();
        if digest.trim().is_empty() {
            return Err("digest must not be empty".to_string());
        }
        Ok(Self {
            id: Uuid::now_v7(),
            name,
            description: description.into(),
            digest,
            created_at: Utc::now(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnalyticsQuery {
    pub id: AnalyticsQueryId,
    pub snapshot_id: AnalyticsSnapshotId,
    pub name: String,
    pub filter: serde_json::Value,
    pub aggregation: String,
    pub status: AnalyticsQueryStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl AnalyticsQuery {
    pub fn new(
        snapshot_id: AnalyticsSnapshotId,
        name: impl Into<String>,
        filter: serde_json::Value,
        aggregation: impl Into<String>,
    ) -> Result<Self, String> {
        if !filter.is_object() {
            return Err("filter must be a JSON object".to_string());
        }
        let name = name.into();
        if name.trim().is_empty() {
            return Err("name must not be empty".to_string());
        }
        let aggregation = aggregation.into();
        if aggregation.trim().is_empty() {
            return Err("aggregation must not be empty".to_string());
        }
        let now = Utc::now();
        Ok(Self {
            id: Uuid::now_v7(),
            snapshot_id,
            name,
            filter,
            aggregation,
            status: AnalyticsQueryStatus::Pending,
            created_at: now,
            updated_at: now,
        })
    }

    pub fn transition_to(
        &mut self,
        next: AnalyticsQueryStatus,
    ) -> Result<(), AnalyticsQueryTransitionError> {
        let allowed = matches!(
            (self.status, next),
            (
                AnalyticsQueryStatus::Pending,
                AnalyticsQueryStatus::Executed
            )
        );
        if allowed {
            self.status = next;
            self.updated_at = Utc::now();
            Ok(())
        } else {
            Err(AnalyticsQueryTransitionError {
                from: self.status,
                to: serde_json::to_value(next)
                    .expect("status serialization cannot fail")
                    .as_str()
                    .expect("status is a string")
                    .to_string(),
            })
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnalyticsInsight {
    pub id: AnalyticsInsightId,
    pub query_id: AnalyticsQueryId,
    pub result_digest: String,
    pub status: AnalyticsInsightStatus,
    pub approved_at: Option<DateTime<Utc>>,
    pub approved_by: Option<Uuid>,
}

impl AnalyticsInsight {
    pub fn new(
        query_id: AnalyticsQueryId,
        result_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let result_digest = result_digest.into();
        if result_digest.trim().is_empty() {
            return Err("result_digest must not be empty".to_string());
        }
        Ok(Self {
            id: Uuid::now_v7(),
            query_id,
            result_digest,
            status: AnalyticsInsightStatus::Pending,
            approved_at: None,
            approved_by: None,
        })
    }

    pub fn approve(&mut self, approved_by: Uuid, approved_at: DateTime<Utc>) -> Result<(), String> {
        if self.status != AnalyticsInsightStatus::Pending {
            return Err(format!(
                "analytics insight {} is not pending",
                self.result_digest
            ));
        }
        self.status = AnalyticsInsightStatus::Approved;
        self.approved_at = Some(approved_at);
        self.approved_by = Some(approved_by);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::str::FromStr;

    fn principal_id() -> Uuid {
        Uuid::from_str("00000000-0000-0000-0000-000000000001").unwrap()
    }

    fn valid_query() -> AnalyticsQuery {
        AnalyticsQuery::new(
            Uuid::now_v7(),
            "Objects by kind",
            json!({"kind": "content"}),
            "count",
        )
        .unwrap()
    }

    #[test]
    fn snapshot_validates_required_fields() {
        let snapshot =
            AnalyticsSnapshot::new("Activity", "Aggregate activity", "sha256:digest").unwrap();
        assert_eq!(snapshot.name, "Activity");
        assert_eq!(snapshot.digest, "sha256:digest");
        assert!(AnalyticsSnapshot::new(" ", "", "sha256:digest").is_err());
        assert!(AnalyticsSnapshot::new("Activity", "", " ").is_err());
    }

    #[test]
    fn query_validates_shape_and_required_fields() {
        let query = valid_query();
        assert_eq!(query.status, AnalyticsQueryStatus::Pending);
        assert!(query.filter.is_object());
        assert!(AnalyticsQuery::new(Uuid::now_v7(), " ", json!({}), "count").is_err());
        assert!(AnalyticsQuery::new(Uuid::now_v7(), "Name", json!({}), "").is_err());
        assert!(AnalyticsQuery::new(Uuid::now_v7(), "Name", json!([]), "count").is_err());
    }

    #[test]
    fn query_has_valid_lifecycle() {
        let mut query = valid_query();
        let first_updated_at = query.updated_at;
        query.transition_to(AnalyticsQueryStatus::Executed).unwrap();
        assert_eq!(query.status, AnalyticsQueryStatus::Executed);
        assert!(query.updated_at >= first_updated_at);
        assert!(query.transition_to(AnalyticsQueryStatus::Executed).is_err());
    }

    #[test]
    fn query_rejects_invalid_transition() {
        let mut query = valid_query();
        let mut query = valid_query();
        query.transition_to(AnalyticsQueryStatus::Executed).unwrap();
        let executed_error = query
            .transition_to(AnalyticsQueryStatus::Executed)
            .unwrap_err();
        assert_eq!(
            executed_error.to_string(),
            "invalid analytics query transition from Executed to executed"
        );
    }

    #[test]
    fn insight_validates_result_digest() {
        let insight = AnalyticsInsight::new(Uuid::now_v7(), "sha256:result").unwrap();
        assert_eq!(insight.status, AnalyticsInsightStatus::Pending);
        assert!(insight.approved_at.is_none());
        assert!(insight.approved_by.is_none());
        assert!(AnalyticsInsight::new(Uuid::now_v7(), " ").is_err());
    }

    #[test]
    fn insight_approves_pending_insight_once() {
        let mut insight = AnalyticsInsight::new(Uuid::now_v7(), "sha256:result").unwrap();
        let approved_at = Utc::now();
        insight.approve(principal_id(), approved_at).unwrap();
        assert_eq!(insight.status, AnalyticsInsightStatus::Approved);
        assert_eq!(insight.approved_at, Some(approved_at));
        assert_eq!(insight.approved_by, Some(principal_id()));
        assert!(insight.approve(principal_id(), approved_at).is_err());
    }
}
