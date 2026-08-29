//! Round-trip integration tests for commerce storage.

use super::commerce::{Catalog, CatalogProduct, Order, OrderLine, OrderStatus};
use super::store::SqliteStore;
use crate::StorageError;
use chrono::{TimeZone, Utc};
use proof_kernel::{canonicalize, digest, generate_keypair_for, ArtifactKind, Proof};
use uuid::Uuid;

fn test_catalog() -> Catalog {
    let now = Utc::now();
    Catalog {
        id: Uuid::now_v7(),
        name: "Test Catalog".to_string(),
        description: "A test catalog".to_string(),
        created_at: now,
        updated_at: now,
    }
}

fn test_product(catalog_id: Uuid) -> CatalogProduct {
    CatalogProduct {
        id: Uuid::now_v7(),
        catalog_id,
        name: "Test Product".to_string(),
        description: Some("A test product".to_string()),
        price_cents: Some(1999),
        created_at: Utc::now(),
    }
}

fn test_order(catalog_id: Uuid) -> Order {
    Order {
        id: Uuid::now_v7(),
        status: OrderStatus::Pending,
        created_at: Utc::now(),
        approved_at: None,
        fulfilled_at: None,
        lines: vec![OrderLine {
            catalog_id,
            name: "Test Product".to_string(),
            quantity: 2,
        }],
    }
}

fn commerce_proof(operation: &str, timestamp: chrono::DateTime<Utc>) -> Proof {
    let keypair = generate_keypair_for(proof_kernel::PrincipalKind::Agent);
    let input_json =
        canonicalize(&serde_json::json!({"operation": operation, "step": "in"})).unwrap();
    let output_json =
        canonicalize(&serde_json::json!({"operation": operation, "step": "out"})).unwrap();
    let input = digest(ArtifactKind::OperationInput, &input_json);
    let output = digest(ArtifactKind::OperationOutput, &output_json);
    Proof::new(
        Uuid::now_v7(),
        keypair.principal_id,
        None,
        operation,
        input,
        output,
        timestamp,
    )
    .sign(&keypair)
    .unwrap()
}

#[test]
fn catalog_round_trips() {
    let store = SqliteStore::in_memory().unwrap();
    let catalog = test_catalog();

    store.save_catalog(&catalog).unwrap();
    let loaded = store.load_catalog(&catalog.id).unwrap();

    assert_eq!(loaded, catalog);
}

#[test]
fn catalog_update_round_trips() {
    let store = SqliteStore::in_memory().unwrap();
    let mut catalog = test_catalog();
    store.save_catalog(&catalog).unwrap();

    catalog.name = "Updated Catalog".to_string();
    catalog.description = "Updated description".to_string();
    catalog.updated_at = Utc::now();
    store.save_catalog(&catalog).unwrap();
    let loaded = store.load_catalog(&catalog.id).unwrap();

    assert_eq!(loaded, catalog);
}

#[test]
fn catalog_rejects_duplicate_and_stale_timestamps() {
    let store = SqliteStore::in_memory().unwrap();
    let mut catalog = test_catalog();
    store.save_catalog(&catalog).unwrap();

    assert!(matches!(
        store.save_catalog(&catalog),
        Err(StorageError::Conflict(_))
    ));

    catalog.name = "Stale Catalog".to_string();
    catalog.updated_at = catalog.updated_at - chrono::Duration::seconds(1);
    assert!(matches!(
        store.save_catalog(&catalog),
        Err(StorageError::Conflict(_))
    ));
    assert_eq!(
        store.load_catalog(&catalog.id).unwrap().name,
        "Test Catalog"
    );
}

#[test]
fn catalog_replaces_only_with_newer_timestamp() {
    let store = SqliteStore::in_memory().unwrap();
    let mut catalog = test_catalog();
    store.save_catalog(&catalog).unwrap();

    catalog.name = "Updated Catalog".to_string();
    catalog.description = "Updated description".to_string();
    catalog.updated_at = catalog.updated_at + chrono::Duration::seconds(1);
    store.save_catalog(&catalog).unwrap();

    assert_eq!(store.load_catalog(&catalog.id).unwrap(), catalog);
}

#[test]
fn catalog_list_round_trips() {
    let store = SqliteStore::in_memory().unwrap();
    let mut first = test_catalog();
    let mut second = test_catalog();
    second.created_at = first.created_at + chrono::Duration::seconds(1);
    first.created_at = Utc::now();
    store.save_catalog(&first).unwrap();
    store.save_catalog(&second).unwrap();

    let catalogs = store.list_catalogs().unwrap();

    assert_eq!(catalogs, vec![first, second]);
}

#[test]
fn catalog_delete_round_trips() {
    let store = SqliteStore::in_memory().unwrap();
    let catalog = test_catalog();
    store.save_catalog(&catalog).unwrap();

    let deleted = store.delete_catalog(&catalog.id).unwrap();
    let result = store.load_catalog(&catalog.id);

    assert!(deleted);
    assert!(matches!(result, Err(StorageError::NotFound(_))));
    assert!(!store.delete_catalog(&catalog.id).unwrap());
}

#[test]
fn catalog_delete_cascades_products() {
    let store = SqliteStore::in_memory().unwrap();
    let catalog = test_catalog();
    store.save_catalog(&catalog).unwrap();
    let product = test_product(catalog.id);
    store.save_product(&product).unwrap();

    store.delete_catalog(&catalog.id).unwrap();

    assert!(matches!(
        store.load_product(&product.id),
        Err(StorageError::NotFound(_))
    ));
}

#[test]
fn product_round_trips() {
    let store = SqliteStore::in_memory().unwrap();
    let catalog = test_catalog();
    store.save_catalog(&catalog).unwrap();
    let product = test_product(catalog.id);

    store.save_product(&product).unwrap();
    let loaded = store.load_product(&product.id).unwrap();

    assert_eq!(loaded, product);
}

#[test]
fn product_update_round_trips() {
    let store = SqliteStore::in_memory().unwrap();
    let catalog = test_catalog();
    store.save_catalog(&catalog).unwrap();
    let mut product = test_product(catalog.id);
    store.save_product(&product).unwrap();

    product.name = "Updated Product".to_string();
    product.description = None;
    product.price_cents = Some(2999);
    store.save_product(&product).unwrap();
    let loaded = store.load_product(&product.id).unwrap();

    assert_eq!(loaded, product);
}

#[test]
fn product_list_round_trips() {
    let store = SqliteStore::in_memory().unwrap();
    let catalog = test_catalog();
    store.save_catalog(&catalog).unwrap();
    let mut first = test_product(catalog.id);
    let mut second = test_product(catalog.id);
    second.created_at = first.created_at + chrono::Duration::seconds(1);
    store.save_product(&first).unwrap();
    store.save_product(&second).unwrap();
    let other_catalog = test_catalog();
    store.save_catalog(&other_catalog).unwrap();
    let other_product = test_product(other_catalog.id);
    store.save_product(&other_product).unwrap();

    let products = store.list_products(&catalog.id).unwrap();

    assert_eq!(products, vec![first, second]);
    assert_ne!(products, vec![other_product]);
}

#[test]
fn product_delete_round_trips() {
    let store = SqliteStore::in_memory().unwrap();
    let catalog = test_catalog();
    store.save_catalog(&catalog).unwrap();
    let product = test_product(catalog.id);
    store.save_product(&product).unwrap();

    let deleted = store.delete_product(&product.id).unwrap();
    let result = store.load_product(&product.id);

    assert!(deleted);
    assert!(matches!(result, Err(StorageError::NotFound(_))));
    assert!(!store.delete_product(&product.id).unwrap());
}

#[test]
fn order_round_trips() {
    let store = SqliteStore::in_memory().unwrap();
    let catalog = test_catalog();
    store.save_catalog(&catalog).unwrap();
    let order = test_order(catalog.id);

    store.save_order(&order).unwrap();
    let loaded = store.load_order(&order.id).unwrap();

    assert_eq!(loaded, order);
}

#[test]
fn order_status_transitions_round_trip() {
    let store = SqliteStore::in_memory().unwrap();
    let catalog = test_catalog();
    store.save_catalog(&catalog).unwrap();
    let mut order = test_order(catalog.id);
    store.save_order(&order).unwrap();

    order.status = OrderStatus::Approved;
    order.approved_at = Some(Utc::now());
    order.created_at = Utc::now();
    store.save_order(&order).unwrap();
    assert_eq!(
        store.load_order(&order.id).unwrap().status,
        OrderStatus::Approved
    );

    order.status = OrderStatus::Fulfilled;
    order.fulfilled_at = Some(Utc::now());
    order.created_at = Utc::now();
    store.save_order(&order).unwrap();
    let loaded = store.load_order(&order.id).unwrap();

    assert_eq!(loaded.status, OrderStatus::Fulfilled);
    assert_eq!(loaded.approved_at, order.approved_at);
    assert_eq!(loaded.fulfilled_at, order.fulfilled_at);
}

#[test]
fn order_rejects_duplicate_and_stale_timestamps() {
    let store = SqliteStore::in_memory().unwrap();
    let catalog = test_catalog();
    store.save_catalog(&catalog).unwrap();
    let mut order = test_order(catalog.id);
    store.save_order(&order).unwrap();

    assert!(matches!(
        store.save_order(&order),
        Err(StorageError::Conflict(_))
    ));

    order.status = OrderStatus::Approved;
    order.approved_at = Some(order.created_at - chrono::Duration::seconds(1));
    assert!(matches!(
        store.save_order(&order),
        Err(StorageError::Conflict(_))
    ));
    assert_eq!(
        store.load_order(&order.id).unwrap().status,
        OrderStatus::Pending
    );
}

#[test]
fn order_replaces_only_with_newer_timestamp() {
    let store = SqliteStore::in_memory().unwrap();
    let catalog = test_catalog();
    store.save_catalog(&catalog).unwrap();
    let mut order = test_order(catalog.id);
    store.save_order(&order).unwrap();

    let original_created_at = order.created_at;
    order.status = OrderStatus::Approved;
    order.approved_at = Some(order.created_at + chrono::Duration::seconds(1));
    order.created_at = order.created_at + chrono::Duration::seconds(2);
    store.save_order(&order).unwrap();
    order.created_at = original_created_at;

    assert_eq!(store.load_order(&order.id).unwrap(), order);
}

#[test]
fn commerce_history_returns_versioned_proofs_in_order() {
    let store = SqliteStore::in_memory().unwrap();
    let first = commerce_proof(
        "catalog.create::v1",
        Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 1).unwrap(),
    );
    let second = commerce_proof(
        "catalog.create::v1",
        Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 2).unwrap(),
    );
    let other_version = commerce_proof(
        "catalog.update::v2",
        Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 3).unwrap(),
    );
    store.save_proof(&first).unwrap();
    store.save_proof(&second).unwrap();
    store.save_proof(&other_version).unwrap();

    let history = store.get_commerce_history("catalog.create", "v1").unwrap();

    assert_eq!(history, vec![first, second]);
}

#[test]
fn order_list_round_trips() {
    let store = SqliteStore::in_memory().unwrap();
    let catalog = test_catalog();
    store.save_catalog(&catalog).unwrap();
    let mut first = test_order(catalog.id);
    let mut second = test_order(catalog.id);
    second.created_at = first.created_at + chrono::Duration::seconds(1);
    store.save_order(&first).unwrap();
    store.save_order(&second).unwrap();

    let orders = store.list_orders().unwrap();

    assert_eq!(orders, vec![first, second]);
}

#[test]
fn order_delete_round_trips() {
    let store = SqliteStore::in_memory().unwrap();
    let catalog = test_catalog();
    store.save_catalog(&catalog).unwrap();
    let order = test_order(catalog.id);
    store.save_order(&order).unwrap();

    let deleted = store.delete_order(&order.id).unwrap();
    let result = store.load_order(&order.id);

    assert!(deleted);
    assert!(matches!(result, Err(StorageError::NotFound(_))));
    assert!(!store.delete_order(&order.id).unwrap());
}
