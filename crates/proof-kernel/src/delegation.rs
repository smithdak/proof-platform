//! Bounded, time-limited delegation.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::identity::PrincipalId;

/// Operation-level execution boundary for a delegation grant.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DelegationScope {
    /// Operations allowed by this grant. `None` allows every operation in
    /// the grant's permitted domains.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_operations: Option<Vec<String>>,
    /// Domains allowed by this grant. `None` allows every domain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_domains: Option<Vec<String>>,
    /// Optional free-form resource boundary beyond the legacy resource scopes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_scope: Option<String>,
}

impl DelegationScope {
    pub fn scope_allows_operation(&self, operation: &str, domain: &str) -> bool {
        if let Some(domains) = &self.allowed_domains {
            if !domains.iter().any(|allowed| allowed == domain) {
                return false;
            }
        }

        if let Some(operations) = &self.allowed_operations {
            if !operations
                .iter()
                .any(|allowed| allowed == operation || wildcard(allowed, operation))
            {
                return false;
            }
        }

        true
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DelegationError {
    #[error("delegation chain cannot be empty")]
    EmptyChain,
    #[error("delegation chain does not start at root principal {0}")]
    InvalidRoot(PrincipalId),
    #[error("delegation links are not connected at link {index}")]
    BrokenChain { index: usize },
    #[error("delegation recipient at link {index} is not the executing agent")]
    InvalidTerminalAgent { index: usize },
    #[error("grantor at link {index} cannot delegate {action}")]
    InsufficientActionAuthority { index: usize, action: String },
    #[error("grantor at link {index} cannot delegate resource {resource}")]
    InsufficientResourceAuthority { index: usize, resource: String },
    #[error("delegation at link {index} is not valid at the requested time")]
    InvalidTime { index: usize },
    #[error("delegation at link {index} is revoked")]
    Revoked { index: usize },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Delegation {
    pub id: Uuid,
    pub issuer: PrincipalId,
    pub recipient: PrincipalId,
    pub allowed_actions: Vec<String>,
    pub resource_scope: Vec<String>,
    #[serde(default = "default_scope")]
    pub scope: DelegationScope,
    pub valid_from: DateTime<Utc>,
    pub valid_until: DateTime<Utc>,
    pub revoked: bool,
}

fn default_scope() -> DelegationScope {
    DelegationScope::default()
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

/// A delegation grant can delegate only authority it received. Root principals
/// implicitly hold unrestricted authority, so the first grant is measured
/// against that authority and later grants against their parent grant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DelegationChain {
    pub root: PrincipalId,
    pub grants: Vec<Delegation>,
}

impl DelegationChain {
    pub fn validate(
        &self,
        executing_agent: PrincipalId,
        now: DateTime<Utc>,
    ) -> Result<(), DelegationError> {
        validate_chain(self.root, executing_agent, &self.grants, now)
    }
}

pub fn validate_chain(
    root: PrincipalId,
    executing_agent: PrincipalId,
    grants: &[Delegation],
    now: DateTime<Utc>,
) -> Result<(), DelegationError> {
    let Some(first) = grants.first() else {
        return Err(DelegationError::EmptyChain);
    };

    if first.issuer != root {
        return Err(DelegationError::InvalidRoot(root));
    }

    for (index, grant) in grants.iter().enumerate() {
        if index > 0 && grant.issuer != grants[index - 1].recipient {
            return Err(DelegationError::BrokenChain { index });
        }

        if grant.revoked {
            return Err(DelegationError::Revoked { index });
        }
        if !(grant.valid_from <= now
            && now <= grant.valid_until
            && grant.valid_from <= grant.valid_until)
        {
            return Err(DelegationError::InvalidTime { index });
        }

        let allowed_actions: &[String] = if index == 0 {
            &["*".to_string()]
        } else {
            grants[index - 1].allowed_actions.as_slice()
        };
        if !grant.allowed_actions.iter().all(|action| {
            allowed_actions
                .iter()
                .any(|parent_action| pattern_is_covered(action, parent_action))
        }) {
            let action = grant
                .allowed_actions
                .iter()
                .find(|action| {
                    !allowed_actions
                        .iter()
                        .any(|parent_action| pattern_is_covered(action, parent_action))
                })
                .cloned()
                .expect("at least one action is outside the parent scope");
            return Err(DelegationError::InsufficientActionAuthority { index, action });
        }

        let allowed_resources: &[String] = if index == 0 {
            &["*".to_string()]
        } else {
            grants[index - 1].resource_scope.as_slice()
        };
        if !grant.resource_scope.iter().all(|resource| {
            allowed_resources
                .iter()
                .any(|parent_resource| pattern_is_covered(resource, parent_resource))
        }) {
            let resource = grant
                .resource_scope
                .iter()
                .find(|resource| {
                    !allowed_resources
                        .iter()
                        .any(|parent_resource| pattern_is_covered(resource, parent_resource))
                })
                .cloned()
                .expect("at least one resource is outside the parent scope");
            return Err(DelegationError::InsufficientResourceAuthority { index, resource });
        }
    }

    let last_index = grants.len() - 1;
    if grants[last_index].recipient != executing_agent {
        return Err(DelegationError::InvalidTerminalAgent { index: last_index });
    }

    Ok(())
}

fn pattern_is_covered(pattern: &str, authority_pattern: &str) -> bool {
    authority_pattern == "*"
        || pattern == authority_pattern
        || authority_pattern
            .strip_suffix('*')
            .is_some_and(|authority_prefix| pattern.starts_with(authority_prefix))
}

#[cfg(test)]
mod tests {
    use chrono::Duration;
    use std::cmp::max;

    use super::*;

    fn delegation(actions: &[&str], scopes: &[&str]) -> Delegation {
        Delegation {
            id: Uuid::now_v7(),
            issuer: PrincipalId::now(),
            recipient: PrincipalId::now(),
            allowed_actions: actions.iter().map(ToString::to_string).collect(),
            resource_scope: scopes.iter().map(ToString::to_string).collect(),
            scope: DelegationScope::default(),
            valid_from: Utc::now() - Duration::minutes(1),
            valid_until: Utc::now() + Duration::minutes(1),
            revoked: false,
        }
    }

    fn grant(
        issuer: PrincipalId,
        recipient: PrincipalId,
        actions: &[&str],
        scopes: &[&str],
    ) -> Delegation {
        Delegation {
            id: Uuid::now_v7(),
            issuer,
            recipient,
            allowed_actions: actions.iter().map(ToString::to_string).collect(),
            resource_scope: scopes.iter().map(ToString::to_string).collect(),
            scope: DelegationScope::default(),
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

    #[test]
    fn validates_or_rejects_chains_with_parent_scopes() {
        let root = PrincipalId::now();
        let manager = PrincipalId::now();
        let agent = PrincipalId::now();
        let grants = [
            grant(root, manager, &["content:*"], &["site/a/*"]),
            grant(
                manager,
                agent,
                &["content:object_create"],
                &["site/a/pages/*"],
            ),
        ];
        let now = Utc::now();
        validate_chain(root, agent, &grants, now).unwrap();

        let mut invalid = grants.clone();
        invalid[1].allowed_actions = vec!["site:object_create".to_string()];
        let result = validate_chain(root, agent, &invalid, now);
        assert!(
            result.is_err(),
            "unexpected chain validation result: {:?}",
            result
        );
        assert!(matches!(
            result,
            Err(DelegationError::InsufficientActionAuthority { index: 1, .. })
        ));

        let mut invalid = grants.clone();
        invalid[1].resource_scope = vec!["site/b/*".to_string()];
        let result = validate_chain(root, agent, &invalid, now);
        assert!(result.is_err());
        assert!(matches!(
            result,
            Err(DelegationError::InsufficientResourceAuthority { index: 1, .. })
        ));
    }

    #[test]
    fn rejects_empty_broken_root_and_terminal_chains() {
        let root = PrincipalId::now();
        let agent = PrincipalId::now();
        assert_eq!(
            validate_chain(root, agent, &[], Utc::now()),
            Err(DelegationError::EmptyChain)
        );

        let wrong_root = PrincipalId::now();
        let grants = [grant(wrong_root, agent, &["*"], &["*"])];
        assert_eq!(
            validate_chain(root, agent, &grants, Utc::now()),
            Err(DelegationError::InvalidRoot(root))
        );

        let manager = PrincipalId::now();
        let grants = [
            grant(root, manager, &["*"], &["*"]),
            grant(PrincipalId::now(), agent, &["*"], &["*"]),
        ];
        assert_eq!(
            validate_chain(root, agent, &grants, Utc::now()),
            Err(DelegationError::BrokenChain { index: 1 })
        );

        let other_agent = PrincipalId::now();
        let grants = [grant(root, other_agent, &["*"], &["*"])];
        assert_eq!(
            validate_chain(root, agent, &grants, Utc::now()),
            Err(DelegationError::InvalidTerminalAgent { index: 0 })
        );
    }

    #[test]
    fn rejects_revoked_invalid_or_not_yet_valid_chains() {
        let now = Utc::now();
        let root = PrincipalId::now();
        let agent = PrincipalId::now();
        let mut grants = [grant(root, agent, &["*"], &["*"])];

        grants[0].revoked = true;
        assert_eq!(
            validate_chain(root, agent, &grants, now),
            Err(DelegationError::Revoked { index: 0 })
        );

        grants[0].revoked = false;
        grants[0].valid_until = grants[0].valid_from - Duration::seconds(1);
        assert_eq!(
            validate_chain(root, agent, &grants, now),
            Err(DelegationError::InvalidTime { index: 0 })
        );

        grants[0].valid_from = now + Duration::seconds(1);
        grants[0].valid_until = now + Duration::minutes(1);
        assert_eq!(
            validate_chain(root, agent, &grants, now),
            Err(DelegationError::InvalidTime { index: 0 })
        );
    }

    #[test]
    fn validates_multi_link_expiry_at_execution_time() {
        let root = PrincipalId::now();
        let manager = PrincipalId::now();
        let agent = PrincipalId::now();
        let mut first = grant(root, manager, &["content:*"], &["site/a/*"]);
        let mut second = grant(manager, agent, &["content:*"], &["site/a/*"]);
        second.valid_until = max(first.valid_until, second.valid_until);
        first.valid_until = second.valid_until;

        assert!(validate_chain(
            root,
            agent,
            &[first.clone(), second.clone()],
            first.valid_until
        )
        .is_ok());
        assert_eq!(
            validate_chain(
                root,
                agent,
                &[first.clone(), second],
                first.valid_until + Duration::seconds(1)
            ),
            Err(DelegationError::InvalidTime { index: 0 })
        );
    }

    #[test]
    fn chain_struct_validates_for_executing_agent() {
        let root = PrincipalId::now();
        let agent = PrincipalId::now();
        let chain = DelegationChain {
            root,
            grants: vec![grant(root, agent, &["*"], &["*"])],
        };

        assert!(chain.validate(agent, Utc::now()).is_ok());
        assert!(chain.validate(PrincipalId::now(), Utc::now()).is_err());
    }
}
