use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

pub type CatalogId = Uuid;
pub type ProductId = Uuid;
pub type OrderId = Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderStatus {
    Pending,
    Approved,
    Fulfilled,
}

impl OrderStatus {
    pub fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Pending, Self::Approved) | (Self::Approved, Self::Fulfilled)
        )
    }
}

#[derive(Debug, Clone, Error)]
#[error("invalid order transition from {from:?} to {to}")]
pub struct OrderTransitionError {
    from: OrderStatus,
    to: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Catalog {
    pub id: CatalogId,
    pub name: String,
    pub description: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Catalog {
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Result<Self, String> {
        let now = Utc::now();
        let name = name.into();
        if name.trim().is_empty() {
            return Err("name must not be empty".to_string());
        }
        Ok(Self {
            id: Uuid::now_v7(),
            name,
            description: description.into(),
            created_at: now,
            updated_at: now,
        })
    }

    pub fn update(
        &mut self,
        name: Option<String>,
        description: Option<String>,
    ) -> Result<(), String> {
        if let Some(name) = name {
            if name.trim().is_empty() {
                return Err("name must not be empty".to_string());
            }
            self.name = name;
            self.updated_at = Utc::now();
        }
        if let Some(description) = description {
            self.description = description;
            self.updated_at = Utc::now();
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Product {
    pub id: ProductId,
    pub catalog_id: CatalogId,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

impl Product {
    pub fn new(catalog_id: CatalogId, name: impl Into<String>) -> Result<Self, String> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err("name must not be empty".to_string());
        }
        Ok(Self {
            id: Uuid::now_v7(),
            catalog_id,
            name,
            created_at: Utc::now(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrderLine {
    pub catalog_id: CatalogId,
    pub name: String,
    pub quantity: u32,
}

impl OrderLine {
    pub fn new(
        catalog_id: CatalogId,
        name: impl Into<String>,
        quantity: u32,
    ) -> Result<Self, String> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err("name must not be empty".to_string());
        }
        if quantity == 0 {
            return Err("quantity must be at least 1".to_string());
        }
        Ok(Self {
            catalog_id,
            name,
            quantity,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Order {
    pub id: OrderId,
    pub lines: Vec<OrderLine>,
    pub status: OrderStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Order {
    pub fn new(lines: Vec<OrderLine>) -> Result<Self, String> {
        if lines.is_empty() {
            return Err("order must contain at least one line".to_string());
        }
        let now = Utc::now();
        Ok(Self {
            id: Uuid::now_v7(),
            lines,
            status: OrderStatus::Pending,
            created_at: now,
            updated_at: now,
        })
    }

    pub fn transition_to(&mut self, next: OrderStatus) -> Result<(), OrderTransitionError> {
        if !self.status.can_transition_to(next) {
            return Err(OrderTransitionError {
                from: self.status,
                to: format!("{next:?}"),
            });
        }
        self.status = next;
        self.updated_at = Utc::now();
        Ok(())
    }
}
