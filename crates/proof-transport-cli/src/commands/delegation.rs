use crate::{open_store, Cli, Workspace};
use anyhow::{bail, Context, Result};
use proof_kernel::{
    delegation::DelegationScope, principal_from_keypair, Delegation, DelegationChain, PrincipalId,
};
use proof_storage::SqliteStore;
use rusqlite::OptionalExtension;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct DelegationGrantScope {
    actions: Vec<String>,
    resources: Vec<String>,
    #[serde(default)]
    operation_scope: StrictOperationScope,
}

#[derive(Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct StrictOperationScope {
    #[serde(default)]
    allowed_operations: Option<Vec<String>>,
    #[serde(default)]
    allowed_domains: Option<Vec<String>>,
    #[serde(default)]
    resource_scope: Option<String>,
}

impl From<StrictOperationScope> for DelegationScope {
    fn from(value: StrictOperationScope) -> Self {
        Self {
            allowed_operations: value.allowed_operations,
            allowed_domains: value.allowed_domains,
            resource_scope: value.resource_scope,
        }
    }
}

pub(crate) fn save_delegation(store: &SqliteStore, delegation: &Delegation) -> Result<()> {
    store
        .save_delegation(delegation)
        .map_err(anyhow::Error::from)
}

fn save_delegation_principal(
    store: &SqliteStore,
    principal_id: PrincipalId,
    kind: proof_kernel::PrincipalKind,
    public_key: Option<&ed25519_dalek::VerifyingKey>,
) -> Result<()> {
    let kind_json = serde_json::to_string(&kind)?;
    let expected_public_key = public_key.map(|key| key.as_bytes().to_vec());
    let connection = store.connection();
    let transaction = rusqlite::Transaction::new_unchecked(
        &connection,
        rusqlite::TransactionBehavior::Immediate,
    )?;
    let existing = transaction
        .query_row(
            "SELECT kind, public_key FROM principals WHERE id = ?1",
            [principal_id.to_string()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)),
        )
        .optional()?;
    if let Some((stored_kind, stored_public_key)) = existing {
        if stored_kind != kind_json {
            bail!("delegation principal {principal_id} is already enrolled with a different kind");
        }
        if expected_public_key
            .as_ref()
            .is_some_and(|expected| expected != &stored_public_key)
        {
            bail!(
                "delegation principal {principal_id} is already enrolled with a different public key"
            );
        }
        transaction.commit()?;
        return Ok(());
    }
    transaction.execute(
        "INSERT INTO principals (id, kind, display_name, public_key)
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![
            principal_id.to_string(),
            kind_json,
            serde_json::to_string(&kind)?,
            expected_public_key.unwrap_or_else(|| vec![0_u8; 32]),
        ],
    )?;
    transaction.commit()?;
    Ok(())
}

fn load_delegations(store: &SqliteStore) -> Result<Vec<Delegation>> {
    let ids = {
        let connection = store.connection();
        let mut statement = connection.prepare(
            "
            SELECT id
            FROM delegations
            ORDER BY valid_from, id
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
                .with_context(|| format!("delegation disappeared while listing: {id}"))
        })
        .collect()
}

fn load_delegation(store: &SqliteStore, delegation_id: &str) -> Result<Delegation> {
    let id = uuid::Uuid::parse_str(delegation_id).context("invalid delegation ID")?;
    store
        .load_delegation(&id)?
        .with_context(|| format!("delegation not found: {id}"))
}

fn revoke_delegation(
    store: &SqliteStore,
    delegation_id: &str,
    issuer: PrincipalId,
) -> Result<usize> {
    let id = uuid::Uuid::parse_str(delegation_id).context("invalid delegation ID")?;
    let changed = store.connection().execute(
        "UPDATE delegations SET revoked = TRUE WHERE id = ?1 AND issuer = ?2",
        rusqlite::params![id.to_string(), issuer.to_string()],
    )?;
    Ok(changed)
}

pub fn cmd_delegation_grant(cli: &Cli, agent_id: &str, scope_json: &str) -> Result<()> {
    let ws = Workspace::open(&cli.workspace)?;
    let scope: DelegationGrantScope =
        serde_json::from_str(scope_json).context("invalid scope JSON")?;
    let agent_uuid = uuid::Uuid::parse_str(agent_id).context("invalid agent ID")?;
    let delegation = Delegation {
        id: uuid::Uuid::now_v7(),
        issuer: ws.actor,
        recipient: PrincipalId::new(agent_uuid),
        allowed_actions: scope.actions,
        resource_scope: scope.resources,
        scope: scope.operation_scope.into(),
        valid_from: chrono::Utc::now(),
        valid_until: chrono::Utc::now() + chrono::Duration::hours(24),
        revoked: false,
    };
    let store = open_store(&ws.root)?;
    store.save_principal(&principal_from_keypair(&ws.keypair))?;
    save_delegation_principal(
        &store,
        delegation.recipient,
        proof_kernel::PrincipalKind::Agent,
        None,
    )?;
    save_delegation(&store, &delegation)?;
    println!(
        "{}",
        serde_json::json!({
            "status": "granted",
            "delegation_id": delegation.id.to_string(),
            "agent_id": agent_id,
            "valid_until": delegation.valid_until.to_rfc3339(),
        })
    );
    Ok(())
}

pub fn cmd_delegation_list(cli: &Cli) -> Result<()> {
    let ws = Workspace::open(&cli.workspace)?;
    let store = open_store(&ws.root)?;
    let delegations = load_delegations(&store)?;
    let summaries: Vec<_> = delegations
        .iter()
        .map(|delegation| {
            serde_json::json!({
                "delegation_id": delegation.id.to_string(),
                "issuer": delegation.issuer.to_string(),
                "recipient": delegation.recipient.to_string(),
                "allowed_actions": delegation.allowed_actions,
            "resource_scope": delegation.resource_scope,
            "operation_scope": delegation.scope,
                "valid_from": delegation.valid_from.to_rfc3339(),
                "valid_until": delegation.valid_until.to_rfc3339(),
                "revoked": delegation.revoked,
            })
        })
        .collect();
    println!(
        "{}",
        serde_json::json!({"count": summaries.len(), "delegations": summaries})
    );
    Ok(())
}

pub fn cmd_delegation_revoke(cli: &Cli, delegation_id: &str) -> Result<()> {
    let ws = Workspace::open(&cli.workspace)?;
    let store = open_store(&ws.root)?;
    let changed = revoke_delegation(&store, delegation_id, ws.actor)?;
    if changed == 0 {
        bail!("delegation not found: {delegation_id}");
    }
    println!(
        "{}",
        serde_json::json!({"status": "revoked", "delegation_id": delegation_id})
    );
    Ok(())
}

pub fn cmd_delegation_validate(cli: &Cli, delegation_id: &str) -> Result<()> {
    let ws = Workspace::open(&cli.workspace)?;
    let store = open_store(&ws.root)?;
    let delegation = load_delegation(&store, delegation_id)?;
    let chain = DelegationChain {
        root: delegation.issuer,
        grants: vec![delegation.clone()],
    };
    let now = chrono::Utc::now();
    let result = chain
        .validate(delegation.recipient, now)
        .map_err(|error| anyhow::anyhow!(error.to_string()));
    let valid = result.is_ok();
    let reason = result.err().map(|error| error.to_string());
    println!(
        "{}",
        serde_json::json!({
            "delegation_id": delegation.id.to_string(),
            "issuer": delegation.issuer.to_string(),
            "recipient": delegation.recipient.to_string(),
            "allowed_actions": delegation.allowed_actions,
            "resource_scope": delegation.resource_scope,
            "operation_scope": delegation.scope,
            "valid": valid,
            "checked_at": now.to_rfc3339(),
            "reason": reason,
        })
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    fn initialized_cli(directory: &assert_fs::TempDir) -> Cli {
        let cli = Cli::parse_from(["proof", "-w", directory.path().to_str().unwrap(), "init"]);
        crate::commands::content::cmd_init(&cli).unwrap();
        cli
    }

    #[test]
    fn grant_save_load_list_and_validate_round_trip_complete_scope() {
        let directory = assert_fs::TempDir::new().unwrap();
        let cli = initialized_cli(&directory);
        let workspace = Workspace::open(&cli.workspace).unwrap();
        cmd_delegation_grant(
            &cli,
            &workspace.actor.to_string(),
            r#"{
                "actions":["content:release_publish"],
                "resources":["preview"],
                "operation_scope":{
                    "allowed_operations":["release.publish"],
                    "allowed_domains":["content"]
                }
            }"#,
        )
        .unwrap();
        let store = open_store(&workspace.root).unwrap();
        let id: String = store
            .connection()
            .query_row("SELECT id FROM delegations", [], |row| row.get(0))
            .unwrap();
        let id = uuid::Uuid::parse_str(&id).unwrap();
        let loaded = store.load_delegation(&id).unwrap().unwrap();
        assert_eq!(loaded.allowed_actions, ["content:release_publish"]);
        assert_eq!(loaded.resource_scope, ["preview"]);
        assert_eq!(
            loaded.scope.allowed_operations.as_deref(),
            Some(&["release.publish".to_string()][..])
        );
        assert_eq!(
            loaded.scope.allowed_domains.as_deref(),
            Some(&["content".to_string()][..])
        );
        assert!(loaded.scope.resource_scope.is_none());
        cmd_delegation_list(&cli).unwrap();
        cmd_delegation_validate(&cli, &id.to_string()).unwrap();
    }

    #[test]
    fn legacy_scope_remains_readable_without_silently_bounding_it() {
        let directory = assert_fs::TempDir::new().unwrap();
        let cli = initialized_cli(&directory);
        let workspace = Workspace::open(&cli.workspace).unwrap();
        cmd_delegation_grant(
            &cli,
            &workspace.actor.to_string(),
            r#"{"actions":["read"],"resources":["legacy"]}"#,
        )
        .unwrap();
        let store = open_store(&workspace.root).unwrap();
        let id: String = store
            .connection()
            .query_row("SELECT id FROM delegations", [], |row| row.get(0))
            .unwrap();
        let loaded = store
            .load_delegation(&uuid::Uuid::parse_str(&id).unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(loaded.allowed_actions, ["read"]);
        assert_eq!(loaded.resource_scope, ["legacy"]);
        assert_eq!(loaded.scope, DelegationScope::default());
    }

    #[test]
    fn grant_to_distinct_recipient_preserves_workspace_agent_principal() {
        let directory = assert_fs::TempDir::new().unwrap();
        let cli = initialized_cli(&directory);
        let workspace = Workspace::open(&cli.workspace).unwrap();
        let store = open_store(&workspace.root).unwrap();
        let original = store.load_principal(&workspace.actor).unwrap();
        assert_eq!(original.kind, proof_kernel::PrincipalKind::Agent);
        let recipient = uuid::Uuid::now_v7();

        cmd_delegation_grant(
            &cli,
            &recipient.to_string(),
            r#"{"actions":["read"],"resources":["preview"]}"#,
        )
        .unwrap();

        let reloaded = store.load_principal(&workspace.actor).unwrap();
        assert_eq!(reloaded.id, original.id);
        assert_eq!(reloaded.kind, original.kind);
        assert_eq!(reloaded.public_key, original.public_key);
        let recipient = store.load_principal(&PrincipalId::new(recipient)).unwrap();
        assert_eq!(recipient.kind, proof_kernel::PrincipalKind::Agent);
    }

    #[test]
    fn grant_rejects_recipient_enrolled_with_a_different_kind() {
        let directory = assert_fs::TempDir::new().unwrap();
        let cli = initialized_cli(&directory);
        let workspace = Workspace::open(&cli.workspace).unwrap();
        let store = open_store(&workspace.root).unwrap();
        let human = proof_kernel::generate_keypair_for(proof_kernel::PrincipalKind::Human);
        let human_principal = principal_from_keypair(&human);
        store.save_principal(&human_principal).unwrap();

        let error = cmd_delegation_grant(
            &cli,
            &human.principal_id.to_string(),
            r#"{"actions":["read"],"resources":["preview"]}"#,
        )
        .unwrap_err();

        assert!(error.to_string().contains("different kind"));
        let reloaded = store.load_principal(&human.principal_id).unwrap();
        assert_eq!(reloaded.id, human_principal.id);
        assert_eq!(reloaded.kind, human_principal.kind);
        assert_eq!(reloaded.public_key, human_principal.public_key);
        let count: i64 = store
            .connection()
            .query_row("SELECT COUNT(*) FROM delegations", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn grant_rejects_unknown_scope_fields_without_persisting() {
        let directory = assert_fs::TempDir::new().unwrap();
        let cli = initialized_cli(&directory);
        let workspace = Workspace::open(&cli.workspace).unwrap();
        let error = cmd_delegation_grant(
            &cli,
            &workspace.actor.to_string(),
            r#"{
                "actions":[],
                "resources":[],
                "operation_scope":{
                    "allowed_operations":["release.publish"],
                    "allowed_domains":["content"],
                    "ignored":true
                }
            }"#,
        )
        .unwrap_err();
        assert!(error.to_string().contains("invalid scope JSON"));
        let store = open_store(&workspace.root).unwrap();
        let count: i64 = store
            .connection()
            .query_row("SELECT COUNT(*) FROM delegations", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }
}
