use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PrincipalId(pub uuid::Uuid);

impl PrincipalId {
    pub fn new() -> Self {
        Self(uuid::Uuid::now_v7())
    }
}

impl Default for PrincipalId {
    fn default() -> Self {
        Self::new()
    }
}
