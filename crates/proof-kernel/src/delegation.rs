//! Bounded, time-limited delegation.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::identity::PrincipalId;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Delegation {
    pub id: Uuid,
    pub issuer: PrincipalId,
    pub recipient: PrincipalId,
    pub allowed_actions: Vec<String>,
    pub resource_scope: Vec<String>,
    pub valid_from: DateTime<Utc>,
    pub valid_until: DateTime<Utc>,
    pub revoked: bool,
}

impl Delegation {
    pub fn is_valid(&self, action: &str, resource: &str, now: DateTime<Utc>) -> bool {
        !self.revoked
            && now >= self.valid_from
            && now <= self.valid_until
            && self.allows_action(action)
            && self.in_scope(resource)
    }

    pub fn allows_action(&self, action: &str) -> bool {
        self.allowed_actions
            .iter()
            .any(|pattern| pattern == "*" || pattern == action || wildcard(pattern, action))
    }

    pub fn in_scope(&self, resource: &str) -> bool {
        self.resource_scope
            .iter()
            .any(|pattern| pattern == "*" || pattern == resource || wildcard(pattern, resource))
    }
}

fn wildcard(pattern: &str, value: &str) -> bool {
    pattern
        .strip_suffix('*')
        .is_some_and(|prefix| value.starts_with(prefix))
}

#[cfg(test)]
mod tests {
    use chrono::Duration;

    use super::*;

    fn delegation(actions: &[&str], scopes: &[&str]) -> Delegation {
        Delegation {
            id: Uuid::now_v7(),
            issuer: PrincipalId::now(),
            recipient: PrincipalId::now(),
            allowed_actions: actions.iter().map(ToString::to_string).collect(),
            resource_scope: scopes.iter().map(ToString::to_string).collect(),
            valid_from: Utc::now() - Duration::minutes(1),
            valid_until: Utc::now() + Duration::minutes(1),
            revoked: false,
        }
    }

    #[test]
    fn exact_matches_authorize() {
        let now = Utc::now();
        let item = delegation(&["content:object_create"], &["site/a"]);
        assert!(item.is_valid("content:object_create", "site/a", now));
        assert!(!item.is_valid("content:object_delete", "site/a", now));
        assert!(!item.is_valid("content:object_create", "site/b", now));
    }

    #[test]
    fn wildcard_matches_authorize() {
        let now = Utc::now();
        let item = delegation(&["content:*"], &["site/a/*"]);
        assert!(item.is_valid("content:object_create", "site/a/page", now));
        assert!(!item.is_valid("site:object_create", "site/a/page", now));
        assert!(!item.is_valid("content:object_create", "site/b/page", now));
    }

    #[test]
    fn revocation_and_time_bound_authority() {
        let now = Utc::now();
        let mut item = delegation(&["content:*"], &["*"]);
        item.revoked = true;
        assert!(!item.is_valid("content:x", "anything", now));
        item.revoked = false;
        assert!(!item.is_valid(
            "content:x",
            "anything",
            item.valid_until + Duration::seconds(1)
        ));
        assert!(item.is_valid("content:x", "anything", item.valid_from));
    }
}
