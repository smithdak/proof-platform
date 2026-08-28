use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::principal::PrincipalId;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    pub id: Uuid,
    pub edition_id: Uuid,
    pub environment: String,
    pub published_at: DateTime<Utc>,
    pub published_by: PrincipalId,
}

impl Release {
    pub fn new(
        edition_id: Uuid,
        environment: impl Into<String>,
        published_by: PrincipalId,
    ) -> Self {
        Self {
            id: Uuid::now_v7(),
            edition_id,
            environment: environment.into(),
            published_at: Utc::now(),
            published_by,
        }
    }
}
