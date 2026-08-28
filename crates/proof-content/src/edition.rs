use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::digest::canonical_digest;
use crate::object::Object;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Edition {
    pub id: Uuid,
    pub changeset_id: Uuid,
    pub objects: Vec<Object>,
    pub created_at: DateTime<Utc>,
    pub content_digest: String,
}

impl Edition {
    pub fn new(changeset_id: Uuid, mut objects: Vec<Object>) -> Self {
        objects.sort_by(|left, right| left.id.cmp(&right.id));
        let content_digest = canonical_digest(&objects);
        Self {
            id: Uuid::now_v7(),
            changeset_id,
            objects,
            created_at: Utc::now(),
            content_digest,
        }
    }

    pub fn object(&self, id: Uuid) -> Option<&Object> {
        self.objects.iter().find(|object| object.id == id)
    }
}
