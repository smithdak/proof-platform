use crate::{load_registry, open_content_store, open_store, Cli, Workspace};
use anyhow::{bail, Context, Result};
use proof_kernel::{Delegation, Principal, PrincipalId, Proof};
use proof_storage::{SqliteStore, StorageError};
use serde_json::Value;
use std::collections::HashSet;
use std::io::Read;
use std::path::Path;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct WorkspaceArchive {
    pub format_version: u32,
    pub exported_at: chrono::DateTime<chrono::Utc>,
    pub registry: Value,
    pub proofs: Vec<Proof>,
    pub principals: Vec<ExportedPrincipal>,
    pub delegations: Vec<Delegation>,
    pub blobs: Vec<ExportedBlob>,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ExportedPrincipal {
    pub id: String,
    pub kind: proof_kernel::PrincipalKind,
    pub public_key: [u8; 32],
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ExportedBlob {
    pub digest: String,
    pub size: u64,
    pub created_at: String,
    pub content: Vec<u8>,
    pub references: Vec<ExportedBlobReference>,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ExportedBlobReference {
    pub artifact_kind: String,
    pub artifact_id: String,
    pub created_at: String,
}

fn load_exported_principal(row: (String, String, Vec<u8>, String)) -> Result<ExportedPrincipal> {
    let (id, kind, public_key, created_at) = row;
    let public_key_bytes: [u8; 32] = public_key
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid exported principal public key"))?;
    Ok(ExportedPrincipal {
        id,
        kind: serde_json::from_str(&kind)?,
        public_key: public_key_bytes,
        created_at: chrono::DateTime::parse_from_rfc3339(&created_at)?.with_timezone(&chrono::Utc),
    })
}

fn decode_exported_principal(exported: &ExportedPrincipal) -> Result<Principal> {
    let id = uuid::Uuid::parse_str(&exported.id)
        .with_context(|| format!("invalid exported principal ID: {}", exported.id))?;
    let public_key = ed25519_dalek::VerifyingKey::from_bytes(&exported.public_key)
        .with_context(|| format!("invalid exported principal public key: {}", exported.id))?;
    Ok(Principal {
        id: PrincipalId::new(id),
        kind: exported.kind,
        public_key,
        created_at: exported.created_at,
    })
}

fn load_existing_principal(
    store: &SqliteStore,
    principal_id: &PrincipalId,
) -> Result<Option<Principal>> {
    match store.load_principal(principal_id) {
        Ok(principal) => Ok(Some(principal)),
        Err(StorageError::NotFound(_)) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn preflight_exported_principals(
    store: &SqliteStore,
    exported: &[ExportedPrincipal],
) -> Result<Vec<Principal>> {
    let mut seen = HashSet::with_capacity(exported.len());
    let mut principals = Vec::with_capacity(exported.len());
    for exported in exported {
        let principal = decode_exported_principal(exported)?;
        if !seen.insert(principal.id) {
            bail!("duplicate exported principal ID: {}", principal.id);
        }
        if let Some(existing) = load_existing_principal(store, &principal.id)? {
            if existing.kind != principal.kind || existing.public_key != principal.public_key {
                bail!(
                    "imported principal {} conflicts with its immutable enrolled kind or public key",
                    principal.id
                );
            }
        }
        principals.push(principal);
    }
    Ok(principals)
}

fn preflight_exported_delegations(
    store: &SqliteStore,
    delegations: &[Delegation],
    imported_principal_ids: &HashSet<PrincipalId>,
) -> Result<()> {
    let mut seen = HashSet::with_capacity(delegations.len());
    for delegation in delegations {
        if !seen.insert(delegation.id) {
            bail!("duplicate exported delegation ID: {}", delegation.id);
        }
        if delegation.valid_from > delegation.valid_until {
            bail!(
                "imported delegation {} has an invalid validity window",
                delegation.id
            );
        }
        for (role, principal_id) in [
            ("issuer", delegation.issuer),
            ("recipient", delegation.recipient),
        ] {
            if !imported_principal_ids.contains(&principal_id)
                && load_existing_principal(store, &principal_id)?.is_none()
            {
                bail!(
                    "imported delegation {} {role} is not enrolled: {principal_id}",
                    delegation.id
                );
            }
        }
        if let Some(existing) = store.load_delegation(&delegation.id)? {
            if existing != *delegation {
                bail!(
                    "imported delegation {} conflicts with the existing durable grant",
                    delegation.id
                );
            }
        }
    }
    Ok(())
}

fn save_imported_delegation_exact(store: &SqliteStore, delegation: &Delegation) -> Result<()> {
    let connection = store.connection();
    let transaction = rusqlite::Transaction::new_unchecked(
        &connection,
        rusqlite::TransactionBehavior::Immediate,
    )?;
    let changed = transaction.execute(
        "
        INSERT INTO delegations (
            id, issuer, recipient, allowed_actions, resource_scope, scope_json,
            valid_from, valid_until, revoked
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
        ON CONFLICT(id) DO UPDATE SET id = excluded.id
        WHERE issuer = excluded.issuer
          AND recipient = excluded.recipient
          AND allowed_actions = excluded.allowed_actions
          AND resource_scope = excluded.resource_scope
          AND scope_json = excluded.scope_json
          AND valid_from = excluded.valid_from
          AND valid_until = excluded.valid_until
          AND revoked = excluded.revoked
        ",
        rusqlite::params![
            delegation.id.to_string(),
            delegation.issuer.to_string(),
            delegation.recipient.to_string(),
            serde_json::to_string(&delegation.allowed_actions)?,
            serde_json::to_string(&delegation.resource_scope)?,
            serde_json::to_string(&delegation.scope)?,
            delegation.valid_from.to_rfc3339(),
            delegation.valid_until.to_rfc3339(),
            delegation.revoked,
        ],
    )?;
    if changed != 1 {
        bail!(
            "imported delegation {} conflicts with the existing durable grant",
            delegation.id
        );
    }
    transaction.commit()?;
    Ok(())
}

fn load_exported_delegations(store: &SqliteStore) -> Result<Vec<Delegation>> {
    let ids = {
        let connection = store.connection();
        let mut statement = connection.prepare(
            "
            SELECT id
            FROM delegations
            ORDER BY id
            ",
        )?;
        let ids = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        ids
    };
    ids.into_iter()
        .map(|id| {
            let id = uuid::Uuid::parse_str(&id).context("invalid stored delegation ID")?;
            store
                .load_delegation(&id)?
                .with_context(|| format!("delegation disappeared while exporting: {id}"))
        })
        .collect()
}

fn load_exported_blobs(store: &proof_storage::ContentAddressedStore) -> Result<Vec<ExportedBlob>> {
    let mut blobs = Vec::new();
    let rows: Vec<(String, u64, String)> = {
        let connection = store.connection();
        let mut statement = connection
            .prepare("SELECT digest, size_bytes, created_at FROM content_blobs ORDER BY digest")?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, u64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    for (digest_hex, size, created_at) in rows {
        let mut digest_bytes = [0_u8; 32];
        hex::decode_to_slice(&digest_hex, &mut digest_bytes)
            .map_err(|_| anyhow::anyhow!("invalid blob digest: {digest_hex}"))?;
        let digest = proof_kernel::ContentDigest::from_bytes(digest_bytes);
        let content = store
            .get(&digest)?
            .ok_or_else(|| anyhow::anyhow!("missing blob content: {digest_hex}"))?;
        let mut references = Vec::new();
        for (artifact_kind, artifact_id) in store.references(&digest)? {
            references.push(ExportedBlobReference {
                artifact_kind,
                artifact_id,
                created_at: chrono::Utc::now().to_rfc3339(),
            });
        }
        blobs.push(ExportedBlob {
            digest: digest_hex,
            size,
            created_at,
            content,
            references,
        });
    }
    Ok(blobs)
}

fn store_exported_blob(
    store: &proof_storage::ContentAddressedStore,
    blob: &ExportedBlob,
) -> Result<()> {
    let digest = store.put(&blob.content)?;
    if digest.hex() != blob.digest {
        bail!(
            "blob content digest mismatch: expected {}, got {}",
            blob.digest,
            digest.hex()
        );
    }
    for reference in &blob.references {
        store.add_reference(
            &digest,
            proof_storage::BlobReference {
                artifact_kind: &reference.artifact_kind,
                artifact_id: &reference.artifact_id,
            },
        )?;
    }
    Ok(())
}

fn add_directory_to_archive(
    builder: &mut tar::Builder<flate2::write::GzEncoder<std::fs::File>>,
    source: &Path,
    prefix: &str,
) -> Result<()> {
    if !source.exists() {
        return Ok(());
    }
    builder.append_dir_all(prefix, source)?;
    Ok(())
}

fn read_archive_files(
    archive: &mut tar::Archive<flate2::read::GzDecoder<std::fs::File>>,
) -> Result<Vec<(String, String, Value)>> {
    let mut files = Vec::new();
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry
            .path()?
            .components()
            .map(|component| component.as_os_str().to_string_lossy().to_string())
            .collect::<Vec<_>>();
        if path.len() != 3 || path[0] != "workspace-data" {
            continue;
        }
        let mut contents = String::new();
        entry.read_to_string(&mut contents)?;
        let value = serde_json::from_str(&contents)?;
        files.push((path[1].clone(), path[2].clone(), value));
    }
    Ok(files)
}

fn import_workspace_files(root: &Path, files: Vec<(String, String, Value)>) -> Result<()> {
    for (subdirectory, id, value) in files {
        let directory = root.join(".proof/data").join(subdirectory);
        std::fs::create_dir_all(&directory)?;
        std::fs::write(directory.join(id), serde_json::to_string_pretty(&value)?)?;
    }
    Ok(())
}

pub fn cmd_export(cli: &Cli, output: &Path) -> Result<()> {
    let workspace = Workspace::open(&cli.workspace)?;
    let store = open_store(&workspace.root)?;
    let cas = open_content_store(&workspace.root)?;

    let proofs_dir = workspace.root.join(".proof/data/proofs");
    if proofs_dir.exists() {
        for proof_path in std::fs::read_dir(&proofs_dir)? {
            let proof_path = proof_path?;
            let raw = std::fs::read_to_string(proof_path.path())?;
            let proof: Proof = serde_json::from_str(&raw)?;
            store.save_proof(&proof)?;
        }
    }

    let registry_entries = store.load_registry().map_err(anyhow::Error::from)?;
    let registry = if registry_entries.is_empty() {
        serde_json::to_value(load_registry(&workspace.root)?.operations())?
    } else {
        serde_json::to_value(&registry_entries)?
    };

    let mut proofs = Vec::new();
    let proof_ids = {
        let proof_connection = store.connection();
        let mut proof_statement =
            proof_connection.prepare("SELECT id FROM proofs ORDER BY timestamp, id")?;
        let rows = proof_statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    for proof_id_text in proof_ids {
        let proof_id = uuid::Uuid::parse_str(&proof_id_text)?;
        proofs.push(store.load_proof(&proof_id)?);
    }

    let principal_rows = {
        let principal_connection = store.connection();
        let mut principal_statement = principal_connection
            .prepare("SELECT id, kind, public_key FROM principals ORDER BY id")?;
        let rows = principal_statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                    chrono::Utc::now().to_rfc3339(),
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    let mut principals = Vec::new();
    for row in principal_rows {
        principals.push(load_exported_principal(row)?);
    }

    let archive = WorkspaceArchive {
        format_version: 1,
        exported_at: chrono::Utc::now(),
        registry,
        proofs,
        principals,
        delegations: load_exported_delegations(&store)?,
        blobs: load_exported_blobs(&cas)?,
    };

    let output_file = std::fs::File::create(output)
        .with_context(|| format!("cannot create export archive: {}", output.display()))?;
    let encoder = flate2::write::GzEncoder::new(output_file, flate2::Compression::default());
    let mut builder = tar::Builder::new(encoder);
    let manifest = serde_json::to_vec_pretty(&archive)?;
    let mut header = tar::Header::new_gnu();
    header.set_size(manifest.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder.append_data(&mut header, "manifest.json", manifest.as_slice())?;
    add_directory_to_archive(
        &mut builder,
        &workspace.root.join(".proof/registry"),
        "registry",
    )?;
    add_directory_to_archive(
        &mut builder,
        &workspace.root.join(".proof/data"),
        "workspace-data",
    )?;
    builder.finish()?;
    builder.into_inner()?.finish()?;
    Ok(())
}

pub fn cmd_import(cli: &Cli, input: &Path) -> Result<()> {
    let workspace = Workspace::open(&cli.workspace)?;
    let store = open_store(&workspace.root)?;
    let cas = open_content_store(&workspace.root)?;

    let input_file = std::fs::File::open(input)
        .with_context(|| format!("cannot open import archive: {}", input.display()))?;
    let decoder = flate2::read::GzDecoder::new(input_file);
    let mut manifest_bytes = Vec::new();
    {
        let mut archive = tar::Archive::new(decoder);
        for entry in archive.entries()? {
            let mut entry = entry?;
            let path = entry.path()?;
            if path == Path::new("manifest.json") {
                entry.read_to_end(&mut manifest_bytes)?;
                break;
            }
        }
    }
    if manifest_bytes.is_empty() {
        bail!("archive missing manifest.json");
    }
    let manifest: WorkspaceArchive = serde_json::from_slice(&manifest_bytes)?;
    if manifest.format_version != 1 {
        bail!(
            "unsupported archive format version: {}",
            manifest.format_version
        );
    }

    let imported_principals = preflight_exported_principals(&store, &manifest.principals)?;
    let imported_principal_ids = imported_principals
        .iter()
        .map(|principal| principal.id)
        .collect::<HashSet<_>>();
    preflight_exported_delegations(&store, &manifest.delegations, &imported_principal_ids)?;

    for proof in &manifest.proofs {
        let principal = match load_existing_principal(&store, &proof.body.actor)? {
            Some(principal) => principal,
            None => imported_principals
                .iter()
                .find(|principal| principal.id == proof.body.actor)
                .cloned()
                .with_context(|| format!("missing principal for proof {}", proof.body.id))?,
        };
        proof
            .verify(&principal.public_key)
            .map_err(anyhow::Error::from)?;
    }

    for principal in &imported_principals {
        store.save_principal(principal)?;
    }
    for delegation in &manifest.delegations {
        save_imported_delegation_exact(&store, delegation)?;
    }

    let mut imported_proofs = 0_usize;
    let mut newer_proofs = 0_usize;
    for proof in &manifest.proofs {
        let existing = store.load_proof(&proof.body.id).ok();
        if existing
            .as_ref()
            .map(|existing| existing.body.timestamp < proof.body.timestamp)
            .unwrap_or(true)
        {
            store.save_proof(proof)?;
            imported_proofs += 1;
            if existing.is_some() {
                newer_proofs += 1;
            }
        }
    }

    for blob in &manifest.blobs {
        store_exported_blob(&cas, blob)?;
    }

    let registry: Vec<proof_kernel::RegistryEntry> = serde_json::from_value(manifest.registry)?;
    store.save_registry(&registry)?;

    let mut files_archive =
        tar::Archive::new(flate2::read::GzDecoder::new(std::fs::File::open(input)?));
    let files = read_archive_files(&mut files_archive)?;
    let workspace_file_count = files.len();
    import_workspace_files(&workspace.root, files)?;

    println!(
        "{}",
        serde_json::json!({
            "status": "imported",
            "format_version": manifest.format_version,
            "proofs": imported_proofs,
            "newer_proofs_replaced": newer_proofs,
            "principals": manifest.principals.len(),
            "delegations": manifest.delegations.len(),
            "blobs": manifest.blobs.len(),
            "workspace_files": workspace_file_count,
        })
    );
    Ok(())
}
