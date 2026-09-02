use std::{collections::BTreeMap, sync::Arc};

use proof_kernel::{raw_artifact_sha256, ArtifactDigest};

use crate::ControlShellError;

/// One independently frozen manifest entry. The digest is supplied separately
/// from the bytes it authenticates so a bundle can never certify itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticManifestEntry {
    path: String,
    media_type: String,
    sha256: ArtifactDigest,
}

impl StaticManifestEntry {
    pub fn new(
        path: impl Into<String>,
        media_type: impl Into<String>,
        sha256: ArtifactDigest,
    ) -> Self {
        Self {
            path: path.into(),
            media_type: media_type.into(),
            sha256,
        }
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn media_type(&self) -> &str {
        &self.media_type
    }

    pub fn sha256(&self) -> ArtifactDigest {
        self.sha256
    }
}

/// One separately supplied embedded byte source.
#[derive(Clone)]
pub struct StaticSource {
    path: String,
    bytes: Arc<[u8]>,
}

impl StaticSource {
    pub fn new(path: impl Into<String>, bytes: impl Into<Arc<[u8]>>) -> Self {
        Self {
            path: path.into(),
            bytes: bytes.into(),
        }
    }
}

/// One immutable, digest-bound static response.
#[derive(Clone)]
pub struct StaticAsset {
    path: String,
    media_type: String,
    bytes: Arc<[u8]>,
}

impl StaticAsset {
    pub(crate) fn media_type(&self) -> &str {
        &self.media_type
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// Closed in-memory application source. It never performs a filesystem lookup.
pub trait StaticBundle: Send + Sync {
    fn validate(&self) -> Result<(), ControlShellError>;
    fn asset(&self, path: &str) -> Option<StaticAsset>;
    fn paths(&self) -> Vec<String>;
}

/// Digest-verifying embedded bundle assembled from an independent frozen
/// manifest and a separately supplied closed byte inventory.
#[derive(Clone)]
pub struct EmbeddedStaticBundle {
    manifest: Arc<[StaticManifestEntry]>,
    assets: Arc<[StaticAsset]>,
}

impl EmbeddedStaticBundle {
    pub fn from_frozen_manifest(
        manifest: Vec<StaticManifestEntry>,
        sources: Vec<StaticSource>,
    ) -> Result<Self, ControlShellError> {
        let mut source_by_path = BTreeMap::new();
        for source in sources {
            if source_by_path.insert(source.path, source.bytes).is_some() {
                return Err(ControlShellError::StaticBundleInvalid);
            }
        }

        let mut previous_path: Option<&str> = None;
        let mut assets = Vec::with_capacity(manifest.len());
        for entry in &manifest {
            if previous_path.is_some_and(|previous| previous >= entry.path.as_str())
                || !valid_manifest_entry(entry)
            {
                return Err(ControlShellError::StaticBundleInvalid);
            }
            previous_path = Some(&entry.path);
            let bytes = source_by_path
                .remove(&entry.path)
                .ok_or(ControlShellError::StaticBundleInvalid)?;
            if raw_artifact_sha256(&bytes) != entry.sha256 {
                return Err(ControlShellError::StaticBundleInvalid);
            }
            assets.push(StaticAsset {
                path: entry.path.clone(),
                media_type: entry.media_type.clone(),
                bytes,
            });
        }
        if !source_by_path.is_empty() {
            return Err(ControlShellError::StaticBundleInvalid);
        }

        let bundle = Self {
            manifest: manifest.into(),
            assets: assets.into(),
        };
        bundle.validate()?;
        Ok(bundle)
    }
}

impl StaticBundle for EmbeddedStaticBundle {
    fn validate(&self) -> Result<(), ControlShellError> {
        if self.manifest.len() != self.assets.len()
            || self.manifest.first().map(StaticManifestEntry::path) != Some("/")
        {
            return Err(ControlShellError::StaticBundleInvalid);
        }
        for (entry, asset) in self.manifest.iter().zip(self.assets.iter()) {
            if !valid_manifest_entry(entry)
                || entry.path != asset.path
                || entry.media_type != asset.media_type
                || raw_artifact_sha256(&asset.bytes) != entry.sha256
            {
                return Err(ControlShellError::StaticBundleInvalid);
            }
        }
        Ok(())
    }

    fn asset(&self, path: &str) -> Option<StaticAsset> {
        self.assets.iter().find(|asset| asset.path == path).cloned()
    }

    fn paths(&self) -> Vec<String> {
        self.assets.iter().map(|asset| asset.path.clone()).collect()
    }
}

fn valid_manifest_entry(entry: &StaticManifestEntry) -> bool {
    if entry.path == "/" {
        return entry.media_type == "text/html; charset=utf-8";
    }
    let Some(name) = entry.path.strip_prefix("/assets/") else {
        return false;
    };
    if name.is_empty()
        || name.contains('/')
        || name.contains("..")
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        return false;
    }
    let digest = entry.sha256.encoded();
    let Some(hex) = digest.strip_prefix("sha256:") else {
        return false;
    };
    let digest_bearing = name
        .split('.')
        .any(|component| component.as_bytes() == hex.as_bytes());
    let media_matches = if name.ends_with(".css") {
        entry.media_type == "text/css; charset=utf-8"
    } else if name.ends_with(".js") {
        entry.media_type == "application/javascript; charset=utf-8"
    } else {
        false
    };
    digest_bearing && media_matches
}
