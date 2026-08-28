use crate::{load_registry, Cli};
use anyhow::Result;
use base64::Engine as _;
use proof_storage::SqliteStore;
use serde_json::json;
use std::path::PathBuf;

pub fn cmd_workspace_init(path: &str) -> Result<()> {
    let root = PathBuf::from(path);
    let ws = crate::Workspace::init(&root)?;
    println!(
        "{}",
        serde_json::json!({
            "status": "initialized",
            "workspace_path": root.display().to_string(),
            "actor_id": ws.actor.to_string(),
        })
    );
    Ok(())
}

pub fn cmd_workspace_status(cli: &Cli) -> Result<()> {
    let root = cli
        .workspace
        .canonicalize()
        .unwrap_or_else(|_| cli.workspace.clone());
    let registry_count = load_registry(&root)
        .map(|registry| registry.operations().len())
        .unwrap_or(0);
    let proofs_dir = root.join(".proof/data/proofs");
    let proof_count = std::fs::read_dir(&proofs_dir)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok())
                .filter(|entry| entry.path().extension().map_or(false, |ext| ext == "json"))
                .count()
        })
        .unwrap_or(0);
    let db_path = root.join(".proof/storage/storage.db");
    let principal_count = if db_path.exists() {
        let store =
            SqliteStore::open(&db_path).map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let count: u64 =
            store
                .connection()
                .query_row("SELECT COUNT(*) FROM principals", [], |row| row.get(0))?;
        count as usize
    } else {
        0
    };
    println!(
        "{}",
        serde_json::json!({
            "workspace_path": root.display().to_string(),
            "registered_operations": registry_count,
            "stored_proofs": proof_count,
            "stored_principals": principal_count,
        })
    );
    Ok(())
}

pub fn cmd_keypair_export(cli: &Cli) -> Result<()> {
    let keypair = crate::Workspace::load_keypair(&cli.workspace)?;
    let public_key = keypair.signing_key.verifying_key().to_bytes();
    println!(
        "{}",
        serde_json::json!({
            "principal_id": keypair.principal_id.to_string(),
            "public_key": base64::engine::general_purpose::STANDARD.encode(public_key),
        })
    );
    Ok(())
}

pub fn cmd_keypair_rotate(cli: &Cli) -> Result<()> {
    let old_keypair = crate::Workspace::load_keypair(&cli.workspace)?;
    let new_keypair = crate::Workspace::rotate(&cli.workspace)?;
    println!(
        "{}",
        serde_json::json!({
            "status": "rotated",
            "old_principal_id": old_keypair.principal_id.to_string(),
            "new_principal_id": new_keypair.principal_id.to_string(),
        })
    );
    Ok(())
}
