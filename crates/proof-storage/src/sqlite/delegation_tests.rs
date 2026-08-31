use super::store::SqliteStore;
use chrono::{Duration, TimeZone, Utc};
use proof_kernel::delegation::DelegationScope;
use proof_kernel::{
    generate_keypair_for, principal_from_keypair, Delegation, DelegationChain, ExecutionContext,
    ExecutionEngine, ExecutionError, ExecutionStore, Governance, OperationHandler, PrincipalKind,
    Registry, RegistryEntry, VersionStatus,
};
use rusqlite::params;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

fn delegation(scope: DelegationScope) -> (Arc<SqliteStore>, Delegation) {
    let store = Arc::new(SqliteStore::in_memory().unwrap());
    let issuer = generate_keypair_for(PrincipalKind::Human);
    let recipient = generate_keypair_for(PrincipalKind::Agent);
    store
        .save_principal(&principal_from_keypair(&issuer))
        .unwrap();
    store
        .save_principal(&principal_from_keypair(&recipient))
        .unwrap();
    let grant = Delegation {
        id: Uuid::now_v7(),
        issuer: issuer.principal_id,
        recipient: recipient.principal_id,
        allowed_actions: vec!["content:*".to_string()],
        resource_scope: vec!["workspace:preview/*".to_string()],
        scope,
        valid_from: Utc.with_ymd_and_hms(2026, 8, 30, 12, 0, 0).unwrap(),
        valid_until: Utc.with_ymd_and_hms(2026, 8, 30, 12, 5, 0).unwrap(),
        revoked: false,
    };
    (store, grant)
}

fn exact_release_scope() -> DelegationScope {
    DelegationScope {
        allowed_operations: Some(vec!["release.publish".to_string()]),
        allowed_domains: Some(vec!["content".to_string()]),
        resource_scope: None,
    }
}

#[test]
fn exact_bounded_delegation_round_trips_through_execution_store() {
    let (store, grant) = delegation(exact_release_scope());
    store.save_delegation(&grant).unwrap();

    let loaded = ExecutionStore::load_delegation(store.as_ref(), &grant.id)
        .unwrap()
        .unwrap();

    assert_eq!(loaded, grant);
    let scope_json: String = store
        .connection()
        .query_row(
            "SELECT scope_json FROM delegations WHERE id = ?1",
            [grant.id.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        scope_json,
        r#"{"allowed_operations":["release.publish"],"allowed_domains":["content"]}"#
    );
}

#[test]
fn known_optional_structured_resource_scope_round_trips() {
    let scope = DelegationScope {
        allowed_operations: Some(vec!["object.edit".to_string()]),
        allowed_domains: Some(vec!["content".to_string()]),
        resource_scope: Some("edition:synthetic".to_string()),
    };
    let (store, grant) = delegation(scope);
    store.save_delegation(&grant).unwrap();

    let loaded = store.load_delegation(&grant.id).unwrap().unwrap();

    assert_eq!(loaded.scope, grant.scope);
}

#[test]
fn legacy_insert_uses_readable_empty_scope() {
    let (store, grant) = delegation(DelegationScope::default());
    store
        .connection()
        .execute(
            "INSERT INTO delegations (
                 id, issuer, recipient, allowed_actions, resource_scope,
                 valid_from, valid_until, revoked
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                grant.id.to_string(),
                grant.issuer.to_string(),
                grant.recipient.to_string(),
                serde_json::to_string(&grant.allowed_actions).unwrap(),
                serde_json::to_string(&grant.resource_scope).unwrap(),
                grant.valid_from.to_rfc3339(),
                grant.valid_until.to_rfc3339(),
                grant.revoked,
            ],
        )
        .unwrap();

    let loaded = store.load_delegation(&grant.id).unwrap().unwrap();

    assert_eq!(loaded.scope, DelegationScope::default());
}

#[test]
fn missing_delegation_returns_none() {
    let store = SqliteStore::in_memory().unwrap();

    assert_eq!(store.load_delegation(&Uuid::now_v7()).unwrap(), None);
}

#[test]
fn revoked_and_expired_grants_round_trip_without_becoming_missing() {
    let (store, mut revoked) = delegation(exact_release_scope());
    revoked.revoked = true;
    store.save_delegation(&revoked).unwrap();

    let (_, mut expired) = delegation(exact_release_scope());
    expired.issuer = revoked.issuer;
    expired.recipient = revoked.recipient;
    expired.valid_from = revoked.valid_from - Duration::minutes(10);
    expired.valid_until = revoked.valid_from - Duration::minutes(5);
    store.save_delegation(&expired).unwrap();

    assert_eq!(store.load_delegation(&revoked.id).unwrap(), Some(revoked));
    assert_eq!(store.load_delegation(&expired.id).unwrap(), Some(expired));
}

#[test]
fn malformed_or_unknown_structured_scope_fails_closed() {
    for scope_json in [
        "not-json",
        r#"{"unknown_scope":true}"#,
        r#"{"allowed_operations":"release.publish"}"#,
        r#"{"resource_scope":["wrong-type"]}"#,
    ] {
        let (store, grant) = delegation(exact_release_scope());
        store.save_delegation(&grant).unwrap();
        store
            .connection()
            .execute(
                "UPDATE delegations SET scope_json = ?1 WHERE id = ?2",
                params![scope_json, grant.id.to_string()],
            )
            .unwrap();

        let error = ExecutionStore::load_delegation(store.as_ref(), &grant.id).unwrap_err();

        assert!(error.contains("invalid stored delegation scope_json"));
    }
}

#[test]
fn malformed_legacy_identity_json_time_and_boolean_fields_fail_closed() {
    for (column, value, expected_field) in [
        ("issuer", "not-a-uuid", "issuer"),
        ("recipient", "not-a-uuid", "recipient"),
        ("allowed_actions", "{}", "allowed_actions"),
        ("resource_scope", "{}", "resource_scope"),
        ("valid_from", "not-a-time", "valid_from"),
        ("valid_until", "not-a-time", "valid_until"),
        ("revoked", "2", "revoked"),
    ] {
        let (store, grant) = delegation(exact_release_scope());
        store.save_delegation(&grant).unwrap();
        let connection = store.connection();
        if matches!(column, "issuer" | "recipient") {
            connection
                .pragma_update(None, "foreign_keys", "OFF")
                .unwrap();
        }
        connection
            .execute(
                &format!("UPDATE delegations SET {column} = ?1 WHERE id = ?2"),
                params![value, grant.id.to_string()],
            )
            .unwrap();
        drop(connection);

        let error = store.load_delegation(&grant.id).unwrap_err().to_string();

        assert!(error.contains(expected_field), "unexpected error: {error}");
    }
}

struct EchoHandler;

impl OperationHandler for EchoHandler {
    fn operation(&self) -> &str {
        "release.publish"
    }

    fn execute(&self, input: &Value, _context: &ExecutionContext) -> Result<Value, ExecutionError> {
        Ok(input.clone())
    }
}

fn release_registry() -> Registry {
    Registry::new(vec![RegistryEntry {
        operation: "release.publish".to_string(),
        domain: "content".to_string(),
        version: "v2".to_string(),
        action: "content:release_publish".to_string(),
        description: "test release".to_string(),
        input_schema: "input.json".to_string(),
        output_schema: "output.json".to_string(),
        required_authority: "delegation-grant".to_string(),
        governance: Governance::AgentExecutable,
        idempotency: "none".to_string(),
        consequence: "content-release".to_string(),
        evidence_contract: "operation-effect-v1".to_string(),
        benchmark: None,
        status: VersionStatus::Active,
        deprecated_since: None,
        replacement_operation: None,
    }])
    .unwrap()
}

#[test]
fn engine_uses_loaded_operation_and_domain_scope() {
    let (store, mut stored_grant) = delegation(exact_release_scope());
    let engine_keypair = generate_keypair_for(PrincipalKind::Agent);
    store
        .save_principal(&principal_from_keypair(&engine_keypair))
        .unwrap();
    stored_grant.recipient = engine_keypair.principal_id;
    stored_grant.allowed_actions = vec!["*".to_string()];
    stored_grant.resource_scope = vec!["*".to_string()];
    store.save_delegation(&stored_grant).unwrap();
    let chain_grant = stored_grant.clone();
    let mut context = ExecutionContext {
        actor: stored_grant.recipient,
        principal_kind: Some(PrincipalKind::Agent),
        delegation_id: Some(stored_grant.id),
        delegation_chain: Some(DelegationChain {
            root: stored_grant.issuer,
            grants: vec![chain_grant],
        }),
        workspace_path: PathBuf::from("/tmp/proof-storage-delegation-test"),
        timestamp: stored_grant.valid_from + Duration::seconds(1),
    };
    let mut engine = ExecutionEngine::new_with_keypair(release_registry(), engine_keypair)
        .with_storage(store.clone());
    engine.register_handler(Arc::new(EchoHandler));

    assert_eq!(
        engine
            .execute("release.publish", "v2", &json!({"ok": true}), &context)
            .unwrap(),
        json!({"ok": true})
    );

    stored_grant.scope.allowed_operations = Some(vec!["object.edit".to_string()]);
    store.save_delegation(&stored_grant).unwrap();
    context.delegation_chain.as_mut().unwrap().grants[0] = stored_grant.clone();
    assert_eq!(
        engine
            .execute("release.publish", "v2", &json!({}), &context)
            .unwrap_err(),
        ExecutionError::ScopeViolation
    );

    stored_grant.scope.allowed_operations = Some(vec!["release.publish".to_string()]);
    stored_grant.scope.allowed_domains = Some(vec!["commerce".to_string()]);
    store.save_delegation(&stored_grant).unwrap();
    context.delegation_chain.as_mut().unwrap().grants[0] = stored_grant.clone();
    assert_eq!(
        engine
            .execute("release.publish", "v2", &json!({}), &context)
            .unwrap_err(),
        ExecutionError::ScopeViolation
    );
}
