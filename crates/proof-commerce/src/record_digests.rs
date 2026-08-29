use crate::digest::canonical_digest;
use crate::models::{Catalog, Order, Product};

pub fn catalog_digest(catalog: &Catalog) -> String {
    canonical_digest(catalog)
}

pub fn product_digest(product: &Product) -> String {
    canonical_digest(product)
}

pub fn order_digest(order: &Order) -> String {
    canonical_digest(order)
}

pub fn order_line_digest(order_line: &crate::models::OrderLine) -> String {
    canonical_digest(order_line)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn catalog() -> Catalog {
        Catalog::new("Main", "Primary").unwrap()
    }

    fn product(catalog_id: uuid::Uuid) -> Product {
        Product::new(catalog_id, "Widget").unwrap()
    }

    fn order(catalog_id: uuid::Uuid) -> Order {
        Order::new(vec![
            crate::models::OrderLine::new(catalog_id, "Widget", 2).unwrap()
        ])
        .unwrap()
    }

    #[test]
    fn digests_canonical_records() {
        let catalog = catalog();
        let product = product(catalog.id);
        let order = order(catalog.id);
        for digest in [
            catalog_digest(&catalog),
            product_digest(&product),
            order_digest(&order),
            order_line_digest(&order.lines[0]),
        ] {
            assert!(digest.starts_with("sha256:"));
            assert_eq!(digest.len(), 71);
        }
    }

    #[test]
    fn digests_change_with_record_content() {
        let catalog = catalog();
        let mut updated_catalog = catalog.clone();
        updated_catalog
            .update(None, Some("Updated".to_string()))
            .unwrap();
        let order = order(catalog.id);
        let mut updated_order = order.clone();
        updated_order
            .transition_to(crate::models::OrderStatus::Approved)
            .unwrap();

        assert_ne!(catalog_digest(&catalog), catalog_digest(&updated_catalog));
        assert_ne!(order_digest(&order), order_digest(&updated_order));
    }
}
