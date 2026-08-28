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
}

fn visit(directory: &Path, entries: &mut Vec<RegistryEntry>) -> Result<(), RegistryError> {
    for item in fs::read_dir(directory)? {
        let path = item?.path();
        if path.is_dir() {
            visit(&path, entries)?;
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("json") {
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
        }
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
