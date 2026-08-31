use chrono::{Duration, Utc};
use proof_kernel::delegation::DelegationScope;
use proof_kernel::{
    generate_keypair_for, principal_from_keypair, Delegation, ExecutionStore, PrincipalKind,
};
use proof_storage::SqliteStore;
use uuid::Uuid;

#[test]
fn complete_delegation_round_trips_through_public_storage_api() {
    let store = SqliteStore::in_memory().unwrap();
    let issuer = generate_keypair_for(PrincipalKind::Human);
    let recipient = generate_keypair_for(PrincipalKind::Agent);
    store
        .save_principal(&principal_from_keypair(&issuer))
        .unwrap();
    store
        .save_principal(&principal_from_keypair(&recipient))
        .unwrap();
    let valid_from = Utc::now();
    let delegation = Delegation {
        id: Uuid::now_v7(),
        issuer: issuer.principal_id,
        recipient: recipient.principal_id,
        allowed_actions: vec!["content:*".to_string()],
        resource_scope: vec!["workspace:preview/*".to_string()],
        scope: DelegationScope {
            allowed_operations: Some(vec!["release.publish".to_string()]),
            allowed_domains: Some(vec!["content".to_string()]),
            resource_scope: None,
        },
        valid_from,
        valid_until: valid_from + Duration::minutes(5),
        revoked: false,
    };

    store.save_delegation(&delegation).unwrap();

    assert_eq!(
        ExecutionStore::load_delegation(&store, &delegation.id).unwrap(),
        Some(delegation)
    );
}
