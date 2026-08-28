use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ContentError {
    #[error("field '{field}' is required")]
    MissingField { field: String },
    #[error("field '{field}' has type {actual:?}, expected {expected}")]
    InvalidFieldType {
        field: String,
        actual: String,
        expected: String,
    },
    #[error("schema object must be a JSON object")]
    NotAnObject,
    #[error("invalid object transition from {from} to {to}")]
    InvalidTransition { from: String, to: String },
    #[error("changeset is {status:?}; commit requires Approved")]
    ChangesetNotApproved { status: String },
    #[error("changeset is empty; commit requires at least one edit")]
    EmptyChangeset,
    #[error("edit '{edit}' is invalid: {reason}")]
    InvalidEdit { edit: String, reason: String },
    #[error("changeset base state mismatch")]
    BaseStateMismatch { expected: String, actual: String },
    #[error("edit target '{edit}' conflicts with base object {base}")]
    EditTargetMismatch { edit: String, base: String },
    #[error("schema '{schema_id}' version {schema_version} is required by edit '{edit}'")]
    SchemaMismatch {
        edit: String,
        schema_id: uuid::Uuid,
        schema_version: u32,
    },
    #[error("field '{field}' has unsupported default value: {value}")]
    InvalidDefaultValue { field: String, value: Value },
    #[error("schema has no fields")]
    EmptySchema,
    #[error("object id '{object_id}' is not tracked in the base state")]
    MissingBaseObject { object_id: uuid::Uuid },
}

impl ContentError {
    pub fn invalid_type(field: &str, expected: &str, actual: &Value) -> Self {
        Self::InvalidFieldType {
            field: field.to_string(),
            actual: describe_value(actual),
            expected: expected.to_string(),
        }
    }
}

pub fn describe_value(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(_) => "boolean".to_string(),
        Value::Number(_) => "number".to_string(),
        Value::String(_) => "string".to_string(),
        Value::Array(_) => "array".to_string(),
        Value::Object(_) => "object".to_string(),
    }
}
