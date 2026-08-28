use crate::{open_store, Cli, Workspace};
use anyhow::{bail, Context, Result};
use proof_kernel::{Delegation, DelegationChain, PrincipalId};
use proof_storage::SqliteStore;

pub(crate) fn delegation_from_row(
    id: String,
    issuer: String,
    recipient: String,
    allowed_actions: String,
    resource_scope: String,
    valid_from: String,
    valid_until: String,
    revoked: i64,
) -> Result<(Delegation, serde_json::Value)> {
    let parse_json = |label: &str, value: &str| {
        serde_json::from_str::<Vec<String>>(value)
            .with_context(|| format!("invalid delegation {label}: {value}"))
    };
    let delegation = Delegation {
        id: uuid::Uuid::parse_str(&id)?,
        issuer: PrincipalId::new(uuid::Uuid::parse_str(&issuer)?),
        recipient: PrincipalId::new(uuid::Uuid::parse_str(&recipient)?),
        allowed_actions: parse_json("allowed_actions", &allowed_actions)?,
        resource_scope: parse_json("resource_scope", &resource_scope)?,
        scope: Default::default(),
        valid_from: chrono::DateTime::parse_from_rfc3339(&valid_from)?.with_timezone(&chrono::Utc),
        valid_until: chrono::DateTime::parse_from_rfc3339(&valid_until)?
            .with_timezone(&chrono::Utc),
        revoked: revoked != 0,
    };
    let summary = serde_json::json!({
        "delegation_id": delegation.id.to_string(),
        "issuer": delegation.issuer.to_string(),
        "recipient": delegation.recipient.to_string(),
        "allowed_actions": delegation.allowed_actions,
        "resource_scope": delegation.resource_scope,
        "valid_from": delegation.valid_from.to_rfc3339(),
        "valid_until": delegation.valid_until.to_rfc3339(),
        "revoked": delegation.revoked,
    });
    Ok((delegation, summary))
}

pub(crate) fn save_delegation(store: &SqliteStore, delegation: &Delegation) -> Result<()> {
    store.connection().execute(
        "
        INSERT INTO delegations (
            id, issuer, recipient, allowed_actions, resource_scope,
            valid_from, valid_until, revoked
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        ON CONFLICT(id) DO UPDATE SET
            issuer = excluded.issuer,
            recipient = excluded.recipient,
            allowed_actions = excluded.allowed_actions,
            resource_scope = excluded.resource_scope,
            valid_from = excluded.valid_from,
            valid_until = excluded.valid_until,
            revoked = excluded.revoked
        ",
        rusqlite::params![
            delegation.id.to_string(),
            delegation.issuer.to_string(),
            delegation.recipient.to_string(),
            serde_json::to_string(&delegation.allowed_actions)?,
            serde_json::to_string(&delegation.resource_scope)?,
            delegation.valid_from.to_rfc3339(),
            delegation.valid_until.to_rfc3339(),
            delegation.revoked,
        ],
    )?;
    Ok(())
}

fn save_delegation_principal(
    store: &SqliteStore,
    principal_id: PrincipalId,
    kind: proof_kernel::PrincipalKind,
    public_key: Option<&ed25519_dalek::VerifyingKey>,
    _created_at: chrono::DateTime<chrono::Utc>,
) -> Result<()> {
    let public_key = match public_key {
        Some(public_key) => public_key.as_bytes().to_vec(),
        None => vec![0u8; 32],
    };
    store.connection().execute(
        "
        INSERT INTO principals (id, kind, display_name, public_key)
        VALUES (?1, ?2, ?3, ?4)
        ON CONFLICT(id) DO UPDATE SET
            kind = excluded.kind,
            display_name = excluded.display_name
        ",
        rusqlite::params![
            principal_id.to_string(),
            serde_json::to_string(&kind)?,
            serde_json::to_string(&kind)?,
            public_key,
        ],
    )?;
    Ok(())
}

fn load_delegations(store: &SqliteStore) -> Result<Vec<Delegation>> {
    let connection = store.connection();
    let mut statement = connection.prepare(
        "
        SELECT id, issuer, recipient, allowed_actions, resource_scope,
               valid_from, valid_until, revoked
        FROM delegations
        ORDER BY valid_from, id
        ",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, i64>(7)?,
        ))
    })?;
    rows.map(|row| {
        let (
            id,
            issuer,
            recipient,
            allowed_actions,
            resource_scope,
            valid_from,
            valid_until,
            revoked,
        ) = row?;
        delegation_from_row(
            id,
            issuer,
            recipient,
            allowed_actions,
            resource_scope,
            valid_from,
            valid_until,
            revoked,
        )
        .map(|(delegation, _)| delegation)
    })
    .collect()
}

fn load_delegation(store: &SqliteStore, delegation_id: &str) -> Result<Delegation> {
    let id = uuid::Uuid::parse_str(delegation_id).context("invalid delegation ID")?;
    let delegation = store
        .connection()
        .query_row(
            "
            SELECT id, issuer, recipient, allowed_actions, resource_scope,
                   valid_from, valid_until, revoked
            FROM delegations
            WHERE id = ?1
            ",
            [id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, i64>(7)?,
                ))
            },
        )
        .map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => anyhow::anyhow!("delegation not found: {id}"),
            error => error.into(),
        })?;
    let (id, issuer, recipient, allowed_actions, resource_scope, valid_from, valid_until, revoked) =
        delegation;
    Ok(delegation_from_row(
        id,
        issuer,
        recipient,
        allowed_actions,
        resource_scope,
        valid_from,
        valid_until,
        revoked,
    )?
    .0)
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
    let scope: serde_json::Value =
        serde_json::from_str(scope_json).context("invalid scope JSON")?;
    let allowed_actions = scope["actions"]
        .as_array()
        .context("scope missing actions array")?
        .iter()
        .map(|action| {
            action
                .as_str()
                .context("scope action must be a string")
                .map(ToString::to_string)
        })
        .collect::<Result<Vec<_>>>()?;
    let resource_scope = scope["resources"]
        .as_array()
        .context("scope missing resources array")?
        .iter()
        .map(|resource| {
            resource
                .as_str()
                .context("scope resource must be a string")
                .map(ToString::to_string)
        })
        .collect::<Result<Vec<_>>>()?;
    let agent_uuid = uuid::Uuid::parse_str(agent_id).context("invalid agent ID")?;
    let delegation = Delegation {
        id: uuid::Uuid::now_v7(),
        issuer: ws.actor,
        recipient: PrincipalId::new(agent_uuid),
        allowed_actions,
        resource_scope,
        scope: Default::default(),
        valid_from: chrono::Utc::now(),
        valid_until: chrono::Utc::now() + chrono::Duration::hours(24),
        revoked: false,
    };
    let store = open_store(&ws.root)?;
    save_delegation_principal(
        &store,
        ws.actor,
        proof_kernel::PrincipalKind::Human,
        Some(&ws.keypair.signing_key.verifying_key()),
        ws.keypair.created_at,
    )?;
    save_delegation_principal(
        &store,
        delegation.recipient,
        proof_kernel::PrincipalKind::Agent,
        None,
        delegation.valid_from,
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
            "valid": valid,
            "checked_at": now.to_rfc3339(),
            "reason": reason,
        })
    );
    Ok(())
}
