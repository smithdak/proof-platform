//! Commerce record storage: catalogs, products, orders, and order lines.

use super::store::SqliteStore;
use crate::StorageError;
use chrono::format::ParseError;
use chrono::{DateTime, Utc};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

fn parse_uuid(value: &str, context: &str) -> Result<Uuid, StorageError> {
    Uuid::parse_str(value)
        .map_err(|error| StorageError::Conflict(format!("invalid {context}: {error}")))
}

fn parse_timestamp(value: &str) -> Result<DateTime<Utc>, StorageError> {
    DateTime::parse_from_rfc3339(value)
        .map_err(|error| StorageError::Conflict(format!("invalid timestamp: {error}")))
        .map(|timestamp| timestamp.with_timezone(&Utc))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Catalog {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogProduct {
    pub id: Uuid,
    pub catalog_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub price_cents: Option<i64>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderStatus {
    Pending,
    Approved,
    Fulfilled,
}

impl OrderStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Fulfilled => "fulfilled",
        }
    }

    fn from_str(value: &str) -> Result<Self, StorageError> {
        match value {
            "pending" => Ok(Self::Pending),
            "approved" => Ok(Self::Approved),
            "fulfilled" => Ok(Self::Fulfilled),
            other => Err(StorageError::Conflict(format!(
                "unknown order status: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrderLine {
    pub catalog_id: Uuid,
    pub name: String,
    pub quantity: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Order {
    pub id: Uuid,
    pub status: OrderStatus,
    pub created_at: DateTime<Utc>,
    pub approved_at: Option<DateTime<Utc>>,
    pub fulfilled_at: Option<DateTime<Utc>>,
    pub lines: Vec<OrderLine>,
}

impl SqliteStore {
    pub fn save_catalog(&self, catalog: &Catalog) -> Result<(), StorageError> {
        self.conn.lock().unwrap().execute(
            "
            INSERT INTO catalog (id, name, description, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                description = excluded.description,
                updated_at = excluded.updated_at
            ",
            params![
                catalog.id.to_string(),
                catalog.name,
                catalog.description,
                catalog.created_at.to_rfc3339(),
                catalog.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn load_catalog(&self, id: &Uuid) -> Result<Catalog, StorageError> {
        let connection = self.conn.lock().unwrap();
        let row = connection
            .query_row(
                "SELECT id, name, description, created_at, updated_at FROM catalog WHERE id = ?1",
                [id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                },
            )
            .optional()?;
        row.map_or_else(
            || Err(StorageError::NotFound(id.to_string())),
            |(id, name, description, created_at, updated_at)| {
                Ok(Catalog {
                    id: parse_uuid(&id, "catalog ID")?,
                    name,
                    description,
                    created_at: parse_timestamp(&created_at)?,
                    updated_at: parse_timestamp(&updated_at)?,
                })
            },
        )
    }

    pub fn list_catalogs(&self) -> Result<Vec<Catalog>, StorageError> {
        let connection = self.conn.lock().unwrap();
        let mut statement = connection.prepare_cached(
            "SELECT id, name, description, created_at, updated_at FROM catalog ORDER BY created_at",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(
                |(id, name, description, created_at, updated_at)| -> Result<Catalog, StorageError> {
                    Ok(Catalog {
                        id: parse_uuid(&id, "catalog ID")?,
                        name,
                        description,
                        created_at: parse_timestamp(&created_at)?,
                        updated_at: parse_timestamp(&updated_at)?,
                    })
                },
            )
            .collect()
    }

    pub fn delete_catalog(&self, id: &Uuid) -> Result<bool, StorageError> {
        let mut connection = self.conn.lock().unwrap();
        let transaction = connection.transaction()?;
        transaction.execute(
            "DELETE FROM catalog_product WHERE catalog_id = ?1",
            [id.to_string()],
        )?;
        let deleted = transaction.execute("DELETE FROM catalog WHERE id = ?1", [id.to_string()])?;
        transaction.commit()?;
        Ok(deleted > 0)
    }

    pub fn save_product(&self, product: &CatalogProduct) -> Result<(), StorageError> {
        self.conn.lock().unwrap().execute(
            "
            INSERT INTO catalog_product (id, catalog_id, name, description, price_cents, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(id) DO UPDATE SET
                catalog_id = excluded.catalog_id,
                name = excluded.name,
                description = excluded.description,
                price_cents = excluded.price_cents
            ",
            params![
                product.id.to_string(),
                product.catalog_id.to_string(),
                product.name,
                product.description,
                product.price_cents,
                product.created_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn load_product(&self, id: &Uuid) -> Result<CatalogProduct, StorageError> {
        let connection = self.conn.lock().unwrap();
        let row = connection
            .query_row(
                "
                SELECT id, catalog_id, name, description, price_cents, created_at
                FROM catalog_product WHERE id = ?1
                ",
                [id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, Option<i64>>(4)?,
                        row.get::<_, String>(5)?,
                    ))
                },
            )
            .optional()?;
        row.map(
            |(id, catalog_id, name, description, price_cents, created_at)| {
                Ok(CatalogProduct {
                    id: parse_uuid(&id, "product ID")?,
                    catalog_id: parse_uuid(&catalog_id, "catalog ID")?,
                    name,
                    description,
                    price_cents,
                    created_at: parse_timestamp(&created_at)?,
                })
            },
        )
        .unwrap_or_else(|| Err(StorageError::NotFound(id.to_string())))
    }

    pub fn list_products(&self, catalog_id: &Uuid) -> Result<Vec<CatalogProduct>, StorageError> {
        let connection = self.conn.lock().unwrap();
        let mut statement = connection.prepare_cached(
            "
            SELECT id, catalog_id, name, description, price_cents, created_at
            FROM catalog_product WHERE catalog_id = ?1 ORDER BY created_at
            ",
        )?;
        let rows = statement
            .query_map([catalog_id.to_string()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, String>(5)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(
                |(id, catalog_id, name, description, price_cents, created_at)| {
                    Ok(CatalogProduct {
                        id: parse_uuid(&id, "product ID")?,
                        catalog_id: parse_uuid(&catalog_id, "catalog ID")?,
                        name,
                        description,
                        price_cents,
                        created_at: parse_timestamp(&created_at)?,
                    })
                },
            )
            .collect()
    }

    pub fn delete_product(&self, id: &Uuid) -> Result<bool, StorageError> {
        let deleted = self.conn.lock().unwrap().execute(
            "DELETE FROM catalog_product WHERE id = ?1",
            [id.to_string()],
        )?;
        Ok(deleted > 0)
    }

    pub fn save_order(&self, order: &Order) -> Result<(), StorageError> {
        let mut connection = self.conn.lock().unwrap();
        let transaction = connection.transaction()?;
        transaction.execute(
            "
            INSERT INTO \"order\" (id, status, created_at, approved_at, fulfilled_at)
            VALUES (?1, ?2, ?3, ?4, ?5)
            ON CONFLICT(id) DO UPDATE SET
                status = excluded.status,
                approved_at = excluded.approved_at,
                fulfilled_at = excluded.fulfilled_at
            ",
            params![
                order.id.to_string(),
                order.status.as_str(),
                order.created_at.to_rfc3339(),
                order.approved_at.map(|timestamp| timestamp.to_rfc3339()),
                order.fulfilled_at.map(|timestamp| timestamp.to_rfc3339()),
            ],
        )?;
        transaction.execute(
            "DELETE FROM order_line WHERE order_id = ?1",
            [order.id.to_string()],
        )?;
        for line in &order.lines {
            transaction.execute(
                "
                INSERT INTO order_line (order_id, catalog_id, name, quantity)
                VALUES (?1, ?2, ?3, ?4)
                ",
                params![
                    order.id.to_string(),
                    line.catalog_id.to_string(),
                    line.name,
                    line.quantity,
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn load_order(&self, id: &Uuid) -> Result<Order, StorageError> {
        let connection = self.conn.lock().unwrap();
        let row = connection
            .query_row(
                "
                SELECT id, status, created_at, approved_at, fulfilled_at
                FROM \"order\" WHERE id = ?1
                ",
                [id.to_string()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, Option<String>>(4)?,
                    ))
                },
            )
            .optional()?;
        let Some((id_str, status_str, created_at, approved_at, fulfilled_at)) = row else {
            return Err(StorageError::NotFound(id.to_string()));
        };
        let mut order = Order {
            id: parse_uuid(&id_str, "order ID")?,
            status: OrderStatus::from_str(&status_str)?,
            created_at: parse_timestamp(&created_at)?,
            approved_at: approved_at
                .map(|timestamp| parse_timestamp(&timestamp))
                .transpose()?,
            fulfilled_at: fulfilled_at
                .map(|timestamp| parse_timestamp(&timestamp))
                .transpose()?,
            lines: Vec::new(),
        };
        let mut statement = connection.prepare_cached(
            "
            SELECT catalog_id, name, quantity FROM order_line
            WHERE order_id = ?1 ORDER BY id
            ",
        )?;
        order.lines = statement
            .query_map([id.to_string()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(|(catalog_id, name, quantity)| {
                Ok(OrderLine {
                    catalog_id: parse_uuid(&catalog_id, "catalog ID")?,
                    name,
                    quantity: u32::try_from(quantity).map_err(|_| {
                        StorageError::Conflict("order line quantity is negative".to_string())
                    })?,
                })
            })
            .collect::<Result<Vec<_>, StorageError>>()?;
        Ok(order)
    }

    pub fn list_orders(&self) -> Result<Vec<Order>, StorageError> {
        let connection = self.conn.lock().unwrap();
        let ids: Vec<String> = {
            let mut statement =
                connection.prepare_cached("SELECT id FROM \"order\" ORDER BY created_at")?;
            let ids = statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            ids
        };
        drop(connection);
        ids.iter()
            .map(|id| self.load_order(&parse_uuid(id, "order ID")?))
            .collect()
    }

    pub fn delete_order(&self, id: &Uuid) -> Result<bool, StorageError> {
        let mut connection = self.conn.lock().unwrap();
        let transaction = connection.transaction()?;
        transaction.execute(
            "DELETE FROM order_line WHERE order_id = ?1",
            [id.to_string()],
        )?;
        let deleted =
            transaction.execute("DELETE FROM \"order\" WHERE id = ?1", [id.to_string()])?;
        transaction.commit()?;
        Ok(deleted > 0)
    }
}
