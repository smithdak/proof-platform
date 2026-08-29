//! Data-driven operation registry.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Governance {
    AgentExecutable,
    HumanOnly,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VersionStatus {
    #[default]
    Active,
    Deprecated,
    Sunset,
}

impl VersionStatus {
    fn is_active(status: &Self) -> bool {
        *status == Self::Active
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistryEntry {
    pub operation: String,
    pub domain: String,
    pub version: String,
    pub action: String,
    pub description: String,
    pub input_schema: String,
    pub output_schema: String,
    pub required_authority: String,
    pub governance: Governance,
    pub idempotency: String,
    pub consequence: String,
    pub evidence_contract: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub benchmark: Option<String>,
    #[serde(default, skip_serializing_if = "VersionStatus::is_active")]
    pub status: VersionStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deprecated_since: Option<chrono::NaiveDate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replacement_operation: Option<String>,
}

#[derive(Debug, Error)]
pub enum RegistryError {
    #[error("registry directory not found: {0}")]
    DirectoryMissing(String),
    #[error("failed to read registry: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid registry file {path}: {source}")]
    InvalidJson {
        path: String,
        source: serde_json::Error,
    },
    #[error("duplicate operation {operation}:{version}")]
    Duplicate { operation: String, version: String },
}

#[derive(Debug, Clone, Default)]
pub struct Registry {
    entries: Vec<RegistryEntry>,
    index: BTreeMap<(String, String), usize>,
}

impl Registry {
    pub fn load_from_directory(directory: impl AsRef<Path>) -> Result<Self, RegistryError> {
        let directory = directory.as_ref();
        if !directory.exists() {
            return Err(RegistryError::DirectoryMissing(
                directory.display().to_string(),
            ));
        }
        let mut entries = Vec::new();
        visit(directory, &mut entries)?;
        Self::new(entries)
    }

    pub fn new(mut entries: Vec<RegistryEntry>) -> Result<Self, RegistryError> {
        entries.sort_by(|left, right| {
            (&left.domain, &left.operation, &left.version).cmp(&(
                &right.domain,
                &right.operation,
                &right.version,
            ))
        });
        let mut index = BTreeMap::new();
        for (position, entry) in entries.iter().enumerate() {
            let key = (entry.operation.clone(), entry.version.clone());
            if index.insert(key, position).is_some() {
                return Err(RegistryError::Duplicate {
                    operation: entry.operation.clone(),
                    version: entry.version.clone(),
                });
            }
        }
        Ok(Self { entries, index })
    }

    pub fn operations(&self) -> &[RegistryEntry] {
        &self.entries
    }

    pub fn find(&self, operation: &str, version: &str) -> Option<&RegistryEntry> {
        self.index
            .get(&(operation.to_string(), version.to_string()))
            .copied()
            .map(|position| &self.entries[position])
    }

    pub fn active_operations(&self) -> Vec<&RegistryEntry> {
        self.entries
            .iter()
            .filter(|entry| entry.status == VersionStatus::Active)
            .collect()
    }
}

fn visit(directory: &Path, entries: &mut Vec<RegistryEntry>) -> Result<(), RegistryError> {
    for item in fs::read_dir(directory)? {
        let path = item?.path();
        if path.is_dir() {
            visit(&path, entries)?;
        } else if is_registry_entry_file(&path) {
            let contents = fs::read_to_string(&path)?;
            let entry = serde_json::from_str::<RegistryEntry>(&contents).map_err(|source| {
                RegistryError::InvalidJson {
                    path: path.display().to_string(),
                    source,
                }
            })?;
            entries.push(entry);
        }
    }
    Ok(())
}

fn is_registry_entry_file(path: &Path) -> bool {
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    file_name.ends_with(".json")
        && !file_name.ends_with(".input.json")
        && !file_name.ends_with(".output.json")
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn entry(operation: &str, governance: Governance, benchmark: Option<&str>) -> RegistryEntry {
        RegistryEntry {
            operation: operation.to_string(),
            domain: "content".to_string(),
            version: "v1".to_string(),
            action: format!("content:{operation}"),
            description: "test operation".to_string(),
            input_schema: "input.json".to_string(),
            output_schema: "output.json".to_string(),
            required_authority: "delegation-grant".to_string(),
            governance,
            idempotency: "required-uuidv7".to_string(),
            consequence: "content-mutation".to_string(),
            evidence_contract: "operation-effect-v1".to_string(),
            benchmark: benchmark.map(ToString::to_string),
            status: VersionStatus::Active,
            deprecated_since: None,
            replacement_operation: None,
        }
    }

    #[test]
    fn defaults_lifecycle_fields_for_active_entries() {
        let entry = entry("object.create", Governance::AgentExecutable, None);
        let json = serde_json::to_string(&entry).unwrap();
        let deserialized: RegistryEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.status, VersionStatus::Active);
        assert_eq!(deserialized.deprecated_since, None);
        assert_eq!(deserialized.replacement_operation, None);
        assert!(!json.contains("status"));
    }

    #[test]
    fn deserializes_lifecycle_metadata() {
        let mut entry = entry("object.create", Governance::AgentExecutable, None);
        entry.status = VersionStatus::Deprecated;
        entry.deprecated_since = Some(chrono::NaiveDate::from_ymd_opt(2026, 1, 2).unwrap());
        entry.replacement_operation = Some("object.create:v2".to_string());
        let serialized = serde_json::to_value(&entry).unwrap();
        let deserialized: RegistryEntry = serde_json::from_value(serialized).unwrap();
        assert_eq!(deserialized.status, VersionStatus::Deprecated);
        assert_eq!(
            deserialized.deprecated_since,
            Some(chrono::NaiveDate::from_ymd_opt(2026, 1, 2).unwrap())
        );
        assert_eq!(
            deserialized.replacement_operation.as_deref(),
            Some("object.create:v2")
        );
    }

    #[test]
    fn active_operations_excludes_deprecated_and_sunset() {
        let mut deprecated = entry("object.create", Governance::AgentExecutable, None);
        deprecated.status = VersionStatus::Deprecated;
        let mut sunset = entry("object.edit", Governance::AgentExecutable, None);
        sunset.status = VersionStatus::Sunset;
        let active = entry("object.delete", Governance::AgentExecutable, None);
        let registry = Registry::new(vec![deprecated, sunset, active]).unwrap();
        let active_operations = registry.active_operations();
        assert_eq!(active_operations.len(), 1);
        assert_eq!(active_operations[0].operation, "object.delete");
    }

    #[test]
    fn loads_recursively_and_finds_versions() {
        let directory = tempfile::tempdir().unwrap();
        let nested = directory.path().join("content");
        fs::create_dir_all(&nested).unwrap();
        for (index, item) in [
            entry("object.create", Governance::AgentExecutable, Some("B1")),
            entry("object.delete", Governance::HumanOnly, None),
        ]
        .into_iter()
        .enumerate()
        {
            let path = nested.join(format!("{index}.json"));
            let mut file = fs::File::create(path).unwrap();
            serde_json::to_writer(&mut file, &item).unwrap();
            writeln!(file).unwrap();
        }
        let registry = Registry::load_from_directory(directory.path()).unwrap();
        assert_eq!(registry.operations().len(), 2);
        assert_eq!(
            registry.find("object.create", "v1").unwrap().governance,
            Governance::AgentExecutable
        );
        assert!(registry.find("object.create", "v2").is_none());
    }

    #[test]
    fn ignores_input_and_output_schema_files() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("object-create.input.json"),
            r#"{"type":"object"}"#,
        )
        .unwrap();
        fs::write(
            directory.path().join("object-create.output.json"),
            r#"{"type":"object"}"#,
        )
        .unwrap();
        fs::write(
            directory.path().join("object-create.json"),
            serde_json::to_vec(&entry("object.create", Governance::AgentExecutable, None)).unwrap(),
        )
        .unwrap();

        let registry = Registry::load_from_directory(directory.path()).unwrap();

        assert_eq!(registry.operations().len(), 1);
        assert!(registry.find("object.create", "v1").is_some());
    }

    #[test]
    fn rejects_invalid_and_missing_registry() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("invalid.json"), "{invalid").unwrap();
        assert!(matches!(
            Registry::load_from_directory(directory.path()),
            Err(RegistryError::InvalidJson { .. })
        ));
        assert!(matches!(
            Registry::load_from_directory("/does/not/exist"),
            Err(RegistryError::DirectoryMissing(_))
        ));
    }

    #[test]
    fn rejects_duplicate_operations() {
        assert!(matches!(
            Registry::new(vec![
                entry("object.create", Governance::AgentExecutable, None),
                entry("object.create", Governance::HumanOnly, None),
            ]),
            Err(RegistryError::Duplicate { .. })
        ));
    }
}
