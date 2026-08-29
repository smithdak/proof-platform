//! Analytics record storage: snapshots, queries, and insights.

use super::store::SqliteStore;
use crate::StorageError;
use chrono::{DateTime, Utc};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

fn parse_uuid(value: &str, context: &str) -> Result<Uuid, StorageError> {
    Uuid::parse_str(value)
        .map_err(|error| StorageError::Conflict(format!("invalid {context}: {error}")))
}

fn parse_timestamp(value: &str) -> Result<DateTime<Utc>, StorageError> {
    DateTime::parse_from_rfc3339(value)
        .map_err(|error| StorageError::Conflict(format!("invalid timestamp: {error}")))
        .map(|timestamp| timestamp.with_timezone(&Utc))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalyticsSnapshot {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub digest: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalyticsQuery {
    pub id: Uuid,
    pub snapshot_id: Uuid,
    pub name: String,
    pub filter: Value,
    pub aggregation: Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalyticsInsightStatus {
    Pending,
    Approved,
}

impl AnalyticsInsightStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
        }
    }

    fn from_str(value: &str) -> Result<Self, StorageError> {
        match value {
            "pending" => Ok(Self::Pending),
            "approved" => Ok(Self::Approved),
            other => Err(StorageError::Conflict(format!(
                "unknown analytics insight status: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalyticsInsight {
    pub id: Uuid,
    pub query_id: Uuid,
    pub result_digest: String,
    pub status: AnalyticsInsightStatus,
    pub approved_at: Option<DateTime<Utc>>,
    pub approved_by: Option<String>,
}

impl SqliteStore {
    pub fn save_analytics_snapshot(
        &self,
        snapshot: &AnalyticsSnapshot,
    ) -> Result<(), StorageError> {
        self.conn.lock().unwrap().execute(
            "
            INSERT INTO analytics_snapshot (id, name, description, digest, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                description = excluded.description,
                digest = excluded.digest
            ",
            params![
                snapshot.id.to_string(),
                snapshot.name,
                snapshot.description,
                snapshot.digest,
                snapshot.created_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn load_analytics_snapshot(&self, id: &Uuid) -> Result<AnalyticsSnapshot, StorageError> {
        let connection = self.conn.lock().unwrap();
        let row = connection
            .query_row(
                "
                SELECT id, name, description, digest, created_at
                FROM analytics_snapshot WHERE id = ?1
                ",
                [id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                },
            )
            .optional()?;
        let Some((id, name, description, digest, created_at)) = row else {
            return Err(StorageError::NotFound(id.to_string()));
        };
        Ok(AnalyticsSnapshot {
            id: parse_uuid(&id, "analytics snapshot ID")?,
            name,
            description,
            digest,
            created_at: parse_timestamp(&created_at)?,
        })
    }

    pub fn list_analytics_snapshots(&self) -> Result<Vec<AnalyticsSnapshot>, StorageError> {
        let connection = self.conn.lock().unwrap();
        let mut statement = connection.prepare_cached(
            "
            SELECT id, name, description, digest, created_at
            FROM analytics_snapshot ORDER BY created_at
            ",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(id, name, description, digest, created_at)| {
                Ok(AnalyticsSnapshot {
                    id: parse_uuid(&id, "analytics snapshot ID")?,
                    name,
                    description,
                    digest,
                    created_at: parse_timestamp(&created_at)?,
                })
            })
            .collect()
    }

    pub fn list_all_analytics_queries(&self) -> Result<Vec<AnalyticsQuery>, StorageError> {
        let connection = self.conn.lock().unwrap();
        let ids: Vec<String> = {
            let mut statement =
                connection.prepare_cached("SELECT id FROM analytics_query ORDER BY created_at")?;
            let ids = statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            ids
        };
        drop(connection);
        ids.iter()
            .map(|id| self.load_analytics_query(&parse_uuid(id, "analytics query ID")?))
            .collect()
    }

    pub fn delete_analytics_snapshot(&self, id: &Uuid) -> Result<bool, StorageError> {
        let deleted = self.conn.lock().unwrap().execute(
            "DELETE FROM analytics_snapshot WHERE id = ?1",
            [id.to_string()],
        )?;
        Ok(deleted > 0)
    }

    pub fn save_analytics_query(&self, query: &AnalyticsQuery) -> Result<(), StorageError> {
        self.conn.lock().unwrap().execute(
            "
            INSERT INTO analytics_query
                (id, snapshot_id, name, filter, aggregation, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(id) DO UPDATE SET
                snapshot_id = excluded.snapshot_id,
                name = excluded.name,
                filter = excluded.filter,
                aggregation = excluded.aggregation,
                updated_at = excluded.updated_at
            ",
            params![
                query.id.to_string(),
                query.snapshot_id.to_string(),
                query.name,
                serde_json::to_string(&query.filter)?,
                serde_json::to_string(&query.aggregation)?,
                query.created_at.to_rfc3339(),
                query.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn load_analytics_query(&self, id: &Uuid) -> Result<AnalyticsQuery, StorageError> {
        let connection = self.conn.lock().unwrap();
        let row = connection
            .query_row(
                "
                SELECT id, snapshot_id, name, filter, aggregation, created_at, updated_at
                FROM analytics_query WHERE id = ?1
                ",
                [id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                    ))
                },
            )
            .optional()?;
        let Some((id, snapshot_id, name, filter, aggregation, created_at, updated_at)) = row else {
            return Err(StorageError::NotFound(id.to_string()));
        };
        Ok(AnalyticsQuery {
            id: parse_uuid(&id, "analytics query ID")?,
            snapshot_id: parse_uuid(&snapshot_id, "analytics snapshot ID")?,
            name,
            filter: serde_json::from_str(&filter)?,
            aggregation: serde_json::from_str(&aggregation)?,
            created_at: parse_timestamp(&created_at)?,
            updated_at: parse_timestamp(&updated_at)?,
        })
    }

    pub fn list_analytics_queries(
        &self,
        snapshot_id: &Uuid,
    ) -> Result<Vec<AnalyticsQuery>, StorageError> {
        let connection = self.conn.lock().unwrap();
        let ids: Vec<String> = {
            let mut statement = connection.prepare_cached(
                "SELECT id FROM analytics_query WHERE snapshot_id = ?1 ORDER BY created_at, id",
            )?;
            let ids = statement
                .query_map([snapshot_id.to_string()], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            ids
        };
        drop(connection);
        ids.iter()
            .map(|id| self.load_analytics_query(&parse_uuid(id, "analytics query ID")?))
            .collect()
    }

    pub fn delete_analytics_query(&self, id: &Uuid) -> Result<bool, StorageError> {
        let deleted = self.conn.lock().unwrap().execute(
            "DELETE FROM analytics_query WHERE id = ?1",
            [id.to_string()],
        )?;
        Ok(deleted > 0)
    }

    pub fn save_analytics_insight(&self, insight: &AnalyticsInsight) -> Result<(), StorageError> {
        self.conn.lock().unwrap().execute(
            "
            INSERT INTO analytics_insight
                (id, query_id, result_digest, status, approved_at, approved_by)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(id) DO UPDATE SET
                query_id = excluded.query_id,
                result_digest = excluded.result_digest,
                status = excluded.status,
                approved_at = excluded.approved_at,
                approved_by = excluded.approved_by
            ",
            params![
                insight.id.to_string(),
                insight.query_id.to_string(),
                insight.result_digest,
                insight.status.as_str(),
                insight.approved_at.map(|timestamp| timestamp.to_rfc3339()),
                insight.approved_by,
            ],
        )?;
        Ok(())
    }

    pub fn load_analytics_insight(&self, id: &Uuid) -> Result<AnalyticsInsight, StorageError> {
        let connection = self.conn.lock().unwrap();
        let row = connection
            .query_row(
                "
                SELECT id, query_id, result_digest, status, approved_at, approved_by
                FROM analytics_insight WHERE id = ?1
                ",
                [id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, Option<String>>(5)?,
                    ))
                },
            )
            .optional()?;
        let Some((id, query_id, result_digest, status, approved_at, approved_by)) = row else {
            return Err(StorageError::NotFound(id.to_string()));
        };
        Ok(AnalyticsInsight {
            id: parse_uuid(&id, "analytics insight ID")?,
            query_id: parse_uuid(&query_id, "analytics query ID")?,
            result_digest,
            status: AnalyticsInsightStatus::from_str(&status)?,
            approved_at: approved_at
                .map(|timestamp| parse_timestamp(&timestamp))
                .transpose()?,
            approved_by,
        })
    }

    pub fn list_analytics_insights(
        &self,
        query_id: &Uuid,
    ) -> Result<Vec<AnalyticsInsight>, StorageError> {
        let connection = self.conn.lock().unwrap();
        let ids: Vec<String> = {
            let mut statement = connection.prepare_cached(
                "SELECT id FROM analytics_insight WHERE query_id = ?1 ORDER BY rowid",
            )?;
            let ids = statement
                .query_map([query_id.to_string()], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            ids
        };
        drop(connection);
        ids.iter()
            .map(|id| self.load_analytics_insight(&parse_uuid(id, "analytics insight ID")?))
            .collect()
    }

    pub fn delete_analytics_insight(&self, id: &Uuid) -> Result<bool, StorageError> {
        let deleted = self.conn.lock().unwrap().execute(
            "DELETE FROM analytics_insight WHERE id = ?1",
            [id.to_string()],
        )?;
        Ok(deleted > 0)
    }
}
