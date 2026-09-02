//! Immutable operator schema catalog built from exact source bytes.

use std::collections::{BTreeMap, BTreeSet};

use jsonschema::Validator;
use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;

use super::{
    valid_operation_name, valid_operation_version, ControlDigest, SchemaCatalogBinding,
    SchemaCatalogEntryBinding,
};
use crate::{
    control_digest_serialized, raw_artifact_sha256, Registry, RegistryEntry, VersionStatus,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorSchemaSource {
    pub registry_entry_path: String,
    pub registry_entry: Vec<u8>,
    pub input_schema_path: String,
    pub input_schema: Vec<u8>,
    pub output_schema_path: String,
    pub output_schema: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorSchemaSourceInventory {
    pub entries: Vec<OperatorSchemaSource>,
}

#[derive(Debug, Error)]
pub enum OperatorCatalogError {
    #[error("operator schema inventory is invalid: {0}")]
    InvalidInventory(String),
    #[error("operator registry entry is invalid: {0}")]
    InvalidRegistry(String),
    #[error("operator schema is invalid: {0}")]
    InvalidSchema(String),
    #[error("operator schema validation failed")]
    ValidationFailed,
}

struct CatalogOperation {
    input: Validator,
    output: Validator,
}

pub struct OperatorSchemaCatalog {
    binding: SchemaCatalogBinding,
    digest: ControlDigest,
    operations: BTreeMap<(String, String), CatalogOperation>,
}

impl OperatorSchemaCatalog {
    pub fn from_source_inventory(
        inventory: OperatorSchemaSourceInventory,
    ) -> Result<Self, OperatorCatalogError> {
        if inventory.entries.is_empty() {
            return Err(OperatorCatalogError::InvalidInventory(
                "inventory is empty".into(),
            ));
        }
        let mut paths = BTreeSet::new();
        let mut parsed = Vec::new();
        for source in inventory.entries {
            for path in [
                &source.registry_entry_path,
                &source.input_schema_path,
                &source.output_schema_path,
            ] {
                validate_path(path)?;
                if !paths.insert(path.clone()) {
                    return Err(OperatorCatalogError::InvalidInventory(format!(
                        "duplicate path {path}"
                    )));
                }
            }
            let entry = RegistryEntry::from(
                strict_from_slice::<StrictRegistryEntry>(&source.registry_entry)
                    .map_err(OperatorCatalogError::InvalidRegistry)?,
            );
            if entry.status != VersionStatus::Active {
                return Err(OperatorCatalogError::InvalidRegistry(
                    "catalog entries must be active".into(),
                ));
            }
            if !valid_operation_name(&entry.operation) || !valid_operation_version(&entry.version) {
                return Err(OperatorCatalogError::InvalidRegistry(
                    "operation, domain/action, or version syntax is invalid".into(),
                ));
            }
            if entry.input_schema != source.input_schema_path
                || entry.output_schema != source.output_schema_path
            {
                return Err(OperatorCatalogError::InvalidRegistry(
                    "schema reference does not match inventory path".into(),
                ));
            }
            let input: Value = strict_from_slice(&source.input_schema)
                .map_err(OperatorCatalogError::InvalidSchema)?;
            let output: Value = strict_from_slice(&source.output_schema)
                .map_err(OperatorCatalogError::InvalidSchema)?;
            require_draft_2020_12(&input)?;
            require_draft_2020_12(&output)?;
            reject_remote_refs(&input)?;
            reject_remote_refs(&output)?;
            let input_validator = Validator::new(&input)
                .map_err(|e| OperatorCatalogError::InvalidSchema(e.to_string()))?;
            let output_validator = Validator::new(&output)
                .map_err(|e| OperatorCatalogError::InvalidSchema(e.to_string()))?;
            let binding = SchemaCatalogEntryBinding {
                operation: entry.operation.clone(),
                version: entry.version.clone(),
                registry_entry_path: source.registry_entry_path,
                registry_entry_sha256: raw_artifact_sha256(&source.registry_entry),
                input_schema_path: source.input_schema_path,
                input_schema_sha256: raw_artifact_sha256(&source.input_schema),
                output_schema_path: source.output_schema_path,
                output_schema_sha256: raw_artifact_sha256(&source.output_schema),
            };
            parsed.push((
                entry,
                binding,
                CatalogOperation {
                    input: input_validator,
                    output: output_validator,
                },
            ));
        }
        Registry::new(parsed.iter().map(|(entry, _, _)| entry.clone()).collect())
            .map_err(|e| OperatorCatalogError::InvalidRegistry(e.to_string()))?;
        parsed.sort_by(|a, b| (&a.0.operation, &a.0.version).cmp(&(&b.0.operation, &b.0.version)));
        let binding = SchemaCatalogBinding {
            schema: SchemaCatalogBinding::SCHEMA.into(),
            entries: parsed.iter().map(|(_, b, _)| b.clone()).collect(),
        };
        let digest = control_digest_serialized("Proof-Operator-Schema-Catalog-v1", &binding)
            .map_err(|e| OperatorCatalogError::InvalidInventory(e.to_string()))?;
        let operations = parsed
            .into_iter()
            .map(|(entry, _, operation)| ((entry.operation, entry.version), operation))
            .collect();
        Ok(Self {
            binding,
            digest,
            operations,
        })
    }
    pub fn binding(&self) -> &SchemaCatalogBinding {
        &self.binding
    }
    pub const fn digest(&self) -> ControlDigest {
        self.digest
    }
    pub fn validate_input(
        &self,
        operation: &str,
        version: &str,
        value: &Value,
    ) -> Result<(), OperatorCatalogError> {
        self.validate(operation, version, value, true)
    }
    pub fn validate_output(
        &self,
        operation: &str,
        version: &str,
        value: &Value,
    ) -> Result<(), OperatorCatalogError> {
        self.validate(operation, version, value, false)
    }
    fn validate(
        &self,
        operation: &str,
        version: &str,
        value: &Value,
        input: bool,
    ) -> Result<(), OperatorCatalogError> {
        let validators = self
            .operations
            .get(&(operation.into(), version.into()))
            .ok_or(OperatorCatalogError::ValidationFailed)?;
        let validator = if input {
            &validators.input
        } else {
            &validators.output
        };
        validator
            .validate(value)
            .map_err(|_| OperatorCatalogError::ValidationFailed)
    }
}

fn require_draft_2020_12(value: &Value) -> Result<(), OperatorCatalogError> {
    if value.get("$schema").and_then(Value::as_str)
        != Some("https://json-schema.org/draft/2020-12/schema")
    {
        return Err(OperatorCatalogError::InvalidSchema(
            "schema must declare JSON Schema Draft 2020-12".into(),
        ));
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StrictRegistryEntry {
    operation: String,
    domain: String,
    version: String,
    action: String,
    description: String,
    input_schema: String,
    output_schema: String,
    required_authority: String,
    governance: crate::Governance,
    idempotency: String,
    consequence: String,
    evidence_contract: String,
    #[serde(default)]
    benchmark: Option<String>,
    #[serde(default)]
    status: VersionStatus,
    #[serde(default)]
    deprecated_since: Option<chrono::NaiveDate>,
    #[serde(default)]
    replacement_operation: Option<String>,
}
impl From<StrictRegistryEntry> for RegistryEntry {
    fn from(value: StrictRegistryEntry) -> Self {
        Self {
            operation: value.operation,
            domain: value.domain,
            version: value.version,
            action: value.action,
            description: value.description,
            input_schema: value.input_schema,
            output_schema: value.output_schema,
            required_authority: value.required_authority,
            governance: value.governance,
            idempotency: value.idempotency,
            consequence: value.consequence,
            evidence_contract: value.evidence_contract,
            benchmark: value.benchmark,
            status: value.status,
            deprecated_since: value.deprecated_since,
            replacement_operation: value.replacement_operation,
        }
    }
}

fn validate_path(path: &str) -> Result<(), OperatorCatalogError> {
    if path.is_empty()
        || path.len() > 256
        || path.starts_with('/')
        || !path.ends_with(".json")
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || !path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-/".contains(&b))
    {
        return Err(OperatorCatalogError::InvalidInventory(format!(
            "invalid path {path}"
        )));
    }
    Ok(())
}

fn reject_remote_refs(value: &Value) -> Result<(), OperatorCatalogError> {
    match value {
        Value::Object(object) => {
            if object
                .get("$ref")
                .and_then(Value::as_str)
                .is_some_and(|reference| !reference.starts_with('#'))
            {
                return Err(OperatorCatalogError::InvalidSchema(
                    "remote schema references are unsupported".into(),
                ));
            }
            for nested in object.values() {
                reject_remote_refs(nested)?;
            }
        }
        Value::Array(values) => {
            for nested in values {
                reject_remote_refs(nested)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn strict_from_slice<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> Result<T, String> {
    // The recursive duplicate-name pass runs before typed decoding.
    let mut duplicate_check = serde_json::Deserializer::from_slice(bytes);
    DuplicateCheckedValue::deserialize(&mut duplicate_check).map_err(|e| e.to_string())?;
    duplicate_check.end().map_err(|e| e.to_string())?;
    let mut typed = serde_json::Deserializer::from_slice(bytes);
    let result = T::deserialize(&mut typed).map_err(|e| e.to_string())?;
    typed.end().map_err(|e| e.to_string())?;
    Ok(result)
}

struct DuplicateCheckedValue;
impl<'de> Deserialize<'de> for DuplicateCheckedValue {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = DuplicateCheckedValue;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a JSON value without duplicate object names")
            }
            fn visit_bool<E>(self, _: bool) -> Result<Self::Value, E> {
                Ok(DuplicateCheckedValue)
            }
            fn visit_i64<E>(self, _: i64) -> Result<Self::Value, E> {
                Ok(DuplicateCheckedValue)
            }
            fn visit_u64<E>(self, _: u64) -> Result<Self::Value, E> {
                Ok(DuplicateCheckedValue)
            }
            fn visit_f64<E>(self, _: f64) -> Result<Self::Value, E> {
                Ok(DuplicateCheckedValue)
            }
            fn visit_str<E>(self, _: &str) -> Result<Self::Value, E> {
                Ok(DuplicateCheckedValue)
            }
            fn visit_string<E>(self, _: String) -> Result<Self::Value, E> {
                Ok(DuplicateCheckedValue)
            }
            fn visit_none<E>(self) -> Result<Self::Value, E> {
                Ok(DuplicateCheckedValue)
            }
            fn visit_unit<E>(self) -> Result<Self::Value, E> {
                Ok(DuplicateCheckedValue)
            }
            fn visit_some<D: serde::Deserializer<'de>>(
                self,
                d: D,
            ) -> Result<Self::Value, D::Error> {
                DuplicateCheckedValue::deserialize(d)
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<Self::Value, A::Error> {
                while seq.next_element::<DuplicateCheckedValue>()?.is_some() {}
                Ok(DuplicateCheckedValue)
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Self::Value, A::Error> {
                let mut names = BTreeSet::new();
                while let Some(name) = map.next_key::<String>()? {
                    if !names.insert(name.clone()) {
                        return Err(serde::de::Error::custom(format!(
                            "duplicate object name {name}"
                        )));
                    }
                    map.next_value::<DuplicateCheckedValue>()?;
                }
                Ok(DuplicateCheckedValue)
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inventory(registry: &[u8]) -> OperatorSchemaSourceInventory {
        let schema = br#"{"$schema":"https://json-schema.org/draft/2020-12/schema","type":"object","additionalProperties":false,"required":["value"],"properties":{"value":{"type":"string"}}}"#;
        OperatorSchemaSourceInventory {
            entries: vec![OperatorSchemaSource {
                registry_entry_path: "test/echo.json".into(),
                registry_entry: registry.into(),
                input_schema_path: "test/echo.input.json".into(),
                input_schema: schema.into(),
                output_schema_path: "test/echo.output.json".into(),
                output_schema: schema.into(),
            }],
        }
    }

    #[test]
    fn catalog_binds_exact_bytes_and_validates_both_sides() {
        let registry = br#"{"operation":"test.echo","domain":"test","version":"v1","action":"test:echo","description":"Echo","input_schema":"test/echo.input.json","output_schema":"test/echo.output.json","required_authority":"delegation-grant","governance":"agent-executable","idempotency":"none","consequence":"none","evidence_contract":"operation-effect-v1"}"#;
        let catalog = OperatorSchemaCatalog::from_source_inventory(inventory(registry)).unwrap();
        assert_eq!(
            catalog.binding().entries[0].registry_entry_sha256,
            raw_artifact_sha256(registry)
        );
        assert!(catalog
            .validate_input("test.echo", "v1", &serde_json::json!({"value":"x"}))
            .is_ok());
        assert!(catalog
            .validate_output("test.echo", "v1", &serde_json::json!({"extra":true}))
            .is_err());
        assert_eq!(catalog.digest(), catalog.digest());
    }

    #[test]
    fn catalog_rejects_duplicate_names_before_typed_decode() {
        let registry = br#"{"operation":"test.echo","operation":"test.other"}"#;
        assert!(matches!(
            OperatorSchemaCatalog::from_source_inventory(inventory(registry)),
            Err(OperatorCatalogError::InvalidRegistry(_))
        ));
    }

    #[test]
    fn catalog_rejects_missing_or_wrong_draft_before_compilation() {
        let registry = br#"{"operation":"test.echo","domain":"test","version":"v1","action":"test:echo","description":"Echo","input_schema":"test/echo.input.json","output_schema":"test/echo.output.json","required_authority":"delegation-grant","governance":"agent-executable","idempotency":"none","consequence":"none","evidence_contract":"operation-effect-v1"}"#;
        for schema in [
            br#"{"type":"object"}"#.as_slice(),
            br#"{"$schema":"http://json-schema.org/draft-07/schema#","type":"object"}"#.as_slice(),
        ] {
            let mut source = inventory(registry);
            source.entries[0].input_schema = schema.to_vec();
            assert!(matches!(
                OperatorSchemaCatalog::from_source_inventory(source),
                Err(OperatorCatalogError::InvalidSchema(_))
            ));
        }
    }

    #[test]
    fn catalog_rejects_noncanonical_operation_and_version_before_compilation() {
        for (operation, version) in [("Test.echo", "v1"), ("test.echo", "v01")] {
            let registry = format!(
                "{{\"operation\":\"{operation}\",\"domain\":\"test\",\"version\":\"{version}\",\"action\":\"test:echo\",\"description\":\"Echo\",\"input_schema\":\"test/echo.input.json\",\"output_schema\":\"test/echo.output.json\",\"required_authority\":\"delegation-grant\",\"governance\":\"agent-executable\",\"idempotency\":\"none\",\"consequence\":\"none\",\"evidence_contract\":\"operation-effect-v1\"}}"
            );
            assert!(matches!(
                OperatorSchemaCatalog::from_source_inventory(inventory(registry.as_bytes())),
                Err(OperatorCatalogError::InvalidRegistry(_))
            ));
        }
    }

    #[test]
    fn catalog_rejects_unsupported_remote_references() {
        let registry = br#"{"operation":"test.echo","domain":"test","version":"v1","action":"test:echo","description":"Echo","input_schema":"test/echo.input.json","output_schema":"test/echo.output.json","required_authority":"delegation-grant","governance":"agent-executable","idempotency":"none","consequence":"none","evidence_contract":"operation-effect-v1"}"#;
        for reference in [
            "https://example.invalid/schema",
            "urn:example:schema",
            "other.json",
        ] {
            let mut source = inventory(registry);
            source.entries[0].input_schema = format!(
                "{{\"$schema\":\"https://json-schema.org/draft/2020-12/schema\",\"$ref\":\"{reference}\"}}"
            )
            .into_bytes();
            assert!(matches!(
                OperatorSchemaCatalog::from_source_inventory(source),
                Err(OperatorCatalogError::InvalidSchema(_))
            ));
        }
    }
}
