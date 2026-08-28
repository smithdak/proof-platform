use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::error::ContentError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    Text,
    RichText,
    Number,
    Boolean,
    Date,
    DateTime,
    Json,
    Reference,
}

impl FieldType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::RichText => "rich_text",
            Self::Number => "number",
            Self::Boolean => "boolean",
            Self::Date => "date",
            Self::DateTime => "date_time",
            Self::Json => "json",
            Self::Reference => "reference",
        }
    }

    pub fn accepts(&self, value: &Value) -> bool {
        match self {
            Self::Text | Self::RichText | Self::Date | Self::DateTime | Self::Reference => {
                value.is_string()
            }
            Self::Number => value.is_number(),
            Self::Boolean => value.is_boolean(),
            Self::Json => true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaField {
    pub name: String,
    pub field_type: FieldType,
    pub required: bool,
    pub localized: bool,
    pub default_value: Option<Value>,
}

impl SchemaField {
    fn validate_default(&self) -> Result<(), ContentError> {
        if let Some(value) = &self.default_value {
            if value.is_null() || !self.field_type.accepts(value) {
                return Err(ContentError::InvalidDefaultValue {
                    field: self.name.clone(),
                    value: value.clone(),
                });
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaDefinition {
    pub id: Uuid,
    pub name: String,
    pub version: u32,
    pub fields: Vec<SchemaField>,
    pub created_at: DateTime<Utc>,
}

impl SchemaDefinition {
    pub fn new(name: impl Into<String>, version: u32, fields: Vec<SchemaField>) -> Self {
        Self {
            id: Uuid::now_v7(),
            name: name.into(),
            version,
            fields,
            created_at: Utc::now(),
        }
    }

    pub fn validate(&self) -> Result<(), ContentError> {
        if self.fields.is_empty() {
            return Err(ContentError::EmptySchema);
        }
        for field in &self.fields {
            if field.name.is_empty() {
                return Err(ContentError::InvalidEdit {
                    edit: "schema".to_string(),
                    reason: "field name is empty".to_string(),
                });
            }
            field.validate_default()?;
        }
        Ok(())
    }

    pub fn field(&self, name: &str) -> Option<&SchemaField> {
        self.fields.iter().find(|field| field.name == name)
    }

    pub fn validate_object(&self, object: &Value) -> Result<(), ContentError> {
        self.validate()?;
        let Some(entries) = object.as_object() else {
            return Err(ContentError::NotAnObject);
        };

        for field in &self.fields {
            match entries.get(&field.name) {
                Some(Value::Null) | None if field.required => {
                    return Err(ContentError::MissingField {
                        field: field.name.clone(),
                    });
                }
                Some(
                    value @ (Value::Null
                    | Value::Bool(_)
                    | Value::Number(_)
                    | Value::String(_)
                    | Value::Array(_)
                    | Value::Object(_)),
                ) if !value.is_null() => {
                    if !field.field_type.accepts(value) {
                        return Err(ContentError::invalid_type(
                            &field.name,
                            field.field_type.as_str(),
                            value,
                        ));
                    }
                }
                _ => {}
            }
        }

        for key in entries.keys() {
            if self.field(key).is_none() {
                return Err(ContentError::InvalidEdit {
                    edit: key.clone(),
                    reason: format!("unknown schema field '{key}'"),
                });
            }
        }
        Ok(())
    }
}
