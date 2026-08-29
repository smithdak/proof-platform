use anyhow::{bail, Context, Result};
use base64::Engine;
use proof_kernel::{generate_keypair, principal_from_keypair, PrincipalId, Proof};
use proof_storage::SqliteStore;
use serde_json::Value;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

#[cfg(unix)]
const PRIVATE_DIRECTORY_MODE: u32 = 0o700;
#[cfg(unix)]
const PRIVATE_FILE_MODE: u32 = 0o600;

#[derive(Debug, serde::Deserialize)]
pub struct StoredKeypair {
    pub principal_id: uuid::Uuid,
    pub kind: proof_kernel::PrincipalKind,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub public_key: [u8; 32],
    pub signing_key: String,
}

#[derive(Clone)]
pub struct Workspace {
    pub root: PathBuf,
    pub keypair: proof_kernel::Keypair,
    pub actor: PrincipalId,
}

impl Workspace {
    pub fn init(root: &PathBuf) -> Result<Self> {
        let proof_dir = root.join(".proof");
        ensure_private_directory(&proof_dir)?;
        std::fs::create_dir_all(proof_dir.join("registry"))?;
        std::fs::create_dir_all(proof_dir.join("storage"))?;
        for subdir in [
            "schemas",
            "objects",
            "changesets",
            "editions",
            "releases",
            "proofs",
        ] {
            std::fs::create_dir_all(proof_dir.join("data").join(subdir))?;
        }
        let keypair = generate_keypair();
        let actor = keypair.principal_id;
        let config = serde_json::json!({
            "actor_id": actor.to_string(),
            "version": "0.1.0",
        });
        std::fs::write(
            proof_dir.join("config.json"),
            serde_json::to_string_pretty(&config)?,
        )?;
        let keypair_json = serde_json::json!({
            "principal_id": keypair.principal_id.as_uuid(),
            "kind": keypair.kind,
            "created_at": keypair.created_at,
            "public_key": keypair.signing_key.verifying_key().to_bytes(),
            "signing_key": base64::engine::general_purpose::STANDARD
                .encode(keypair.signing_key.to_bytes()),
        });
        write_private_file(
            &proof_dir.join("keypair.json"),
            serde_json::to_string_pretty(&keypair_json)?.as_bytes(),
        )?;
        let store = SqliteStore::open(&proof_dir.join("storage/storage.db"))
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        store
            .save_principal(&principal_from_keypair(&keypair))
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        Ok(Self {
            root: root.clone(),
            keypair,
            actor,
        })
    }

    pub fn open(root: &PathBuf) -> Result<Self> {
        let config_path = root.join(".proof/config.json");
        let config: Value = serde_json::from_str(
            &std::fs::read_to_string(&config_path)
                .context("workspace not initialized — run `proof init` first")?,
        )?;
        let actor_text = config["actor_id"]
            .as_str()
            .context("workspace config missing actor_id")?;
        let actor: PrincipalId =
            PrincipalId::new(uuid::Uuid::parse_str(actor_text).context("invalid actor_id")?);
        let keypair = Self::load_keypair(root)?;
        Ok(Self {
            root: root.clone(),
            keypair,
            actor,
        })
    }

    pub fn save_json(&self, subdir: &str, id: &str, value: &Value) -> Result<()> {
        let dir = self.root.join(".proof/data").join(subdir);
        std::fs::create_dir_all(&dir)?;
        std::fs::write(
            dir.join(format!("{id}.json")),
            serde_json::to_string_pretty(value)?,
        )?;
        Ok(())
    }

    pub fn load_json(&self, subdir: &str, id: &str) -> Result<Value> {
        let path = self
            .root
            .join(".proof/data")
            .join(subdir)
            .join(format!("{id}.json"));
        Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
    }

    pub fn save_proof(&self, proof: &Proof) -> Result<()> {
        let dir = self.root.join(".proof/data/proofs");
        std::fs::create_dir_all(&dir)?;
        std::fs::write(
            dir.join(format!("{}.json", proof.body.id)),
            serde_json::to_string_pretty(proof)?,
        )?;
        Ok(())
    }

    pub fn make_proof(
        &self,
        operation: &str,
        version: &str,
        input: &Value,
        output: &Value,
    ) -> Result<Proof> {
        let input_c = proof_kernel::canonicalize(input)?;
        let output_c = proof_kernel::canonicalize(output)?;
        let input_digest =
            proof_kernel::digest(proof_kernel::ArtifactKind::OperationInput, &input_c);
        let output_digest =
            proof_kernel::digest(proof_kernel::ArtifactKind::OperationOutput, &output_c);
        let proof = Proof::new(
            uuid::Uuid::now_v7(),
            self.actor,
            None,
            format!("{operation}::{version}"),
            input_digest,
            output_digest,
            chrono::Utc::now(),
        );
        proof
            .sign(&self.keypair)
            .map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub fn load_keypair(root: &PathBuf) -> Result<proof_kernel::Keypair> {
        let proof_dir = root.join(".proof");
        harden_private_directory(&proof_dir)?;
        harden_private_key_directory_if_present(&proof_dir.join("rotated"))?;
        harden_private_key_directory_if_present(&proof_dir.join("approvers"))?;
        let path = proof_dir.join("keypair.json");
        harden_private_file(&path)?;
        let raw = std::fs::read_to_string(&path)
            .context("workspace keypair missing — run `proof init` first")?;
        let stored: StoredKeypair = serde_json::from_str(&raw)?;
        let signing_key_bytes: Vec<u8> = base64::engine::general_purpose::STANDARD
            .decode(stored.signing_key)
            .context("invalid stored signing key")?;
        let signing_bytes: [u8; 32] = signing_key_bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("stored signing key must be 32 bytes"))?;
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&signing_bytes);
        if signing_key.verifying_key().to_bytes() != stored.public_key {
            bail!("stored keypair public key mismatch");
        }
        let actor = PrincipalId::new(stored.principal_id);
        Ok(proof_kernel::Keypair {
            principal_id: actor,
            kind: stored.kind,
            created_at: stored.created_at,
            signing_key,
        })
    }

    pub fn rotate(root: &PathBuf) -> Result<proof_kernel::Keypair> {
        let old_keypair = Self::load_keypair(root)?;
        let proof_dir = root.join(".proof");
        let rotated_dir = proof_dir.join("rotated");
        ensure_private_directory(&rotated_dir)?;
        let rotated_at = chrono::Utc::now();
        let rotated_file_name = format!("keypair-{}.json", rotated_at.timestamp_millis());
        let old_keypair_json = serde_json::json!({
            "principal_id": old_keypair.principal_id.as_uuid(),
            "kind": old_keypair.kind,
            "created_at": old_keypair.created_at,
            "public_key": old_keypair.signing_key.verifying_key().to_bytes(),
            "signing_key": base64::engine::general_purpose::STANDARD
                .encode(old_keypair.signing_key.to_bytes()),
            "rotated_at": rotated_at,
        });
        write_private_file(
            &rotated_dir.join(rotated_file_name),
            serde_json::to_string_pretty(&old_keypair_json)?.as_bytes(),
        )?;

        let new_keypair = generate_keypair();
        let actor = new_keypair.principal_id;
        let config_path = proof_dir.join("config.json");
        let mut config: Value = serde_json::from_str(&std::fs::read_to_string(&config_path)?)?;
        config["actor_id"] = serde_json::json!(actor.to_string());
        std::fs::write(&config_path, serde_json::to_string_pretty(&config)?)?;
        let keypair_json = serde_json::json!({
            "principal_id": new_keypair.principal_id.as_uuid(),
            "kind": new_keypair.kind,
            "created_at": new_keypair.created_at,
            "public_key": new_keypair.signing_key.verifying_key().to_bytes(),
            "signing_key": base64::engine::general_purpose::STANDARD
                .encode(new_keypair.signing_key.to_bytes()),
        });
        write_private_file(
            &proof_dir.join("keypair.json"),
            serde_json::to_string_pretty(&keypair_json)?.as_bytes(),
        )?;

        let storage_dir = proof_dir.join("storage");
        std::fs::create_dir_all(&storage_dir)?;
        let store = SqliteStore::open(&storage_dir.join("storage.db"))
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        store
            .save_principal(&principal_from_keypair(&new_keypair))
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;

        Ok(new_keypair)
    }
}

fn ensure_private_directory(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path)?;
    harden_private_directory(path)
}

fn harden_private_directory(path: &Path) -> Result<()> {
    let metadata = std::fs::symlink_metadata(path)
        .with_context(|| format!("private directory missing: {}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("private path is not a directory: {}", path.display());
    }
    #[cfg(unix)]
    std::fs::set_permissions(
        path,
        std::fs::Permissions::from_mode(PRIVATE_DIRECTORY_MODE),
    )?;
    Ok(())
}

fn harden_private_file(path: &Path) -> Result<()> {
    let metadata = std::fs::symlink_metadata(path)
        .with_context(|| format!("private key file missing: {}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!("private key path is not a regular file: {}", path.display());
    }
    #[cfg(unix)]
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(PRIVATE_FILE_MODE))?;
    Ok(())
}

fn harden_private_key_directory_if_present(path: &Path) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => harden_private_directory(path)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    }
    for entry in std::fs::read_dir(path)? {
        harden_private_file(&entry?.path())?;
    }
    Ok(())
}

fn write_private_file(path: &Path, contents: &[u8]) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                bail!("private key path is not a regular file: {}", path.display());
            }
            #[cfg(unix)]
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(PRIVATE_FILE_MODE))?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }

    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    options.mode(PRIVATE_FILE_MODE);
    let mut file = options
        .open(path)
        .with_context(|| format!("could not write private key: {}", path.display()))?;
    #[cfg(unix)]
    file.set_permissions(std::fs::Permissions::from_mode(PRIVATE_FILE_MODE))?;
    file.write_all(contents)?;
    file.sync_all()?;
    Ok(())
}

pub fn save_workspace_json(root: &Path, subdir: &str, id: &str, value: &Value) -> Result<()> {
    let dir = root.join(".proof/data").join(subdir);
    std::fs::create_dir_all(&dir)?;
    std::fs::write(
        dir.join(format!("{id}.json")),
        serde_json::to_string_pretty(value)?,
    )?;
    Ok(())
}

pub fn load_workspace_json(root: &Path, subdir: &str, id: &str) -> Result<Value> {
    let path = root
        .join(".proof/data")
        .join(subdir)
        .join(format!("{id}.json"));
    Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn mode(path: &Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn workspace_keys_are_private_and_existing_modes_are_repaired() {
        let directory = assert_fs::TempDir::new().unwrap();
        let root = directory.path().to_path_buf();
        Workspace::init(&root).unwrap();

        let proof_dir = root.join(".proof");
        let key_path = proof_dir.join("keypair.json");
        assert_eq!(mode(&proof_dir), PRIVATE_DIRECTORY_MODE);
        assert_eq!(mode(&key_path), PRIVATE_FILE_MODE);

        std::fs::set_permissions(&proof_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o644)).unwrap();
        Workspace::open(&root).unwrap();
        assert_eq!(mode(&proof_dir), PRIVATE_DIRECTORY_MODE);
        assert_eq!(mode(&key_path), PRIVATE_FILE_MODE);

        Workspace::rotate(&root).unwrap();
        assert_eq!(mode(&key_path), PRIVATE_FILE_MODE);
        let rotated_dir = proof_dir.join("rotated");
        assert_eq!(mode(&rotated_dir), PRIVATE_DIRECTORY_MODE);
        let rotated_keys = std::fs::read_dir(&rotated_dir)
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(rotated_keys.len(), 1);
        assert_eq!(mode(&rotated_keys[0].path()), PRIVATE_FILE_MODE);

        std::fs::set_permissions(&rotated_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::set_permissions(
            rotated_keys[0].path(),
            std::fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        Workspace::open(&root).unwrap();
        assert_eq!(mode(&rotated_dir), PRIVATE_DIRECTORY_MODE);
        assert_eq!(mode(&rotated_keys[0].path()), PRIVATE_FILE_MODE);
    }
}
