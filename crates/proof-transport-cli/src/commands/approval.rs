use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use base64::Engine as _;
use chrono::{DateTime, Utc};
use proof_kernel::{
    generate_keypair_for, principal_from_keypair, ApprovalOutcome, Keypair, PrincipalKind,
    SignedApprovalDecision,
};
use proof_storage::SqliteStore;
use uuid::Uuid;

use crate::{open_store, Cli, Workspace};

/// Creates and enrolls a human approver for the current workspace.
pub fn cmd_approver_init(cli: &Cli) -> Result<()> {
    let workspace = Workspace::open(&cli.workspace)?;
    let approver = generate_keypair_for(PrincipalKind::Human);
    save_approver_keypair(&workspace.root, &approver)?;
    let store = open_store(&workspace.root)?;
    store.save_principal(&principal_from_keypair(&approver))?;
    println!(
        "{}",
        serde_json::json!({
            "status": "enrolled",
            "approver_id": approver.principal_id.to_string(),
            "key_path": approver_key_path(&workspace.root, approver.principal_id.as_uuid()),
        })
    );
    Ok(())
}

/// Lists durable approval requests and their current state.
pub fn cmd_approval_list(cli: &Cli) -> Result<()> {
    let workspace = Workspace::open(&cli.workspace)?;
    let store = open_store(&workspace.root)?;
    let requests = store.list_approval_requests()?;
    let mut summaries = Vec::with_capacity(requests.len());
    for request in requests {
        let decision = store.load_approval_decision(&request.body.id)?;
        let execution = store.load_approval_execution(&request.body.id)?;
        let status = match (&decision, &execution) {
            (_, Some(_)) => "executed",
            (Some(decision), None) if decision.body.outcome == ApprovalOutcome::Approved => {
                "approved"
            }
            (Some(_), None) => "denied",
            (None, None) if request.body.expires_at < Utc::now() => "expired",
            (None, None) => "pending",
        };
        summaries.push(serde_json::json!({
            "request_id": request.body.id,
            "operation": request.body.operation,
            "version": request.body.version,
            "requested_by": request.body.requested_by,
            "requested_at": request.body.requested_at,
            "expires_at": request.body.expires_at,
            "status": status,
            "decision": decision,
            "execution_proof_id": execution.map(|record| record.proof.body.id),
        }));
    }
    println!(
        "{}",
        serde_json::json!({"count": summaries.len(), "approvals": summaries})
    );
    Ok(())
}

/// Signs an approval decision with an enrolled human key.
pub fn cmd_approval_approve(
    cli: &Cli,
    request_id: &str,
    approver: &str,
    reason: Option<&str>,
) -> Result<()> {
    decide(cli, request_id, approver, ApprovalOutcome::Approved, reason)
}

/// Signs a denial decision with an enrolled human key.
pub fn cmd_approval_deny(
    cli: &Cli,
    request_id: &str,
    approver: &str,
    reason: Option<&str>,
) -> Result<()> {
    decide(cli, request_id, approver, ApprovalOutcome::Denied, reason)
}

fn decide(
    cli: &Cli,
    request_id: &str,
    approver_id: &str,
    outcome: ApprovalOutcome,
    reason: Option<&str>,
) -> Result<()> {
    let workspace = Workspace::open(&cli.workspace)?;
    let store = open_store(&workspace.root)?;
    let request_id = Uuid::parse_str(request_id).context("invalid approval request ID")?;
    let approver_id = Uuid::parse_str(approver_id).context("invalid approver ID")?;
    let decision = sign_approval_decision(
        &workspace.root,
        &store,
        request_id,
        approver_id,
        outcome,
        reason.map(ToString::to_string),
        Utc::now(),
    )?;
    println!(
        "{}",
        serde_json::json!({
            "status": match outcome {
                ApprovalOutcome::Approved => "approved",
                ApprovalOutcome::Denied => "denied",
            },
            "request_id": request_id,
            "decision_id": decision.body.id,
            "decided_by": decision.body.decided_by,
            "decided_at": decision.body.decided_at,
        })
    );
    Ok(())
}

pub(crate) fn sign_approval_decision(
    root: &Path,
    store: &SqliteStore,
    request_id: Uuid,
    approver_id: Uuid,
    outcome: ApprovalOutcome,
    reason: Option<String>,
    decided_at: DateTime<Utc>,
) -> Result<SignedApprovalDecision> {
    let request = store
        .load_approval_request(&request_id)?
        .with_context(|| format!("approval request not found: {request_id}"))?;
    if let Some(existing) = store.load_approval_decision(&request_id)? {
        bail!(
            "approval request {request_id} was already decided as {:?} by {}",
            existing.body.outcome,
            existing.body.decided_by
        );
    }
    let requester = store
        .load_principal(&request.body.requested_by)
        .with_context(|| {
            format!(
                "approval requester is not enrolled: {}",
                request.body.requested_by
            )
        })?;
    request
        .verify(&requester)
        .context("approval request signature verification failed")?;

    let approver = load_approver_keypair(root, approver_id)?;
    let trusted = store
        .load_principal(&approver.principal_id)
        .context("approver is not enrolled")?;
    let actual = principal_from_keypair(&approver);
    if trusted.kind != PrincipalKind::Human
        || trusted.id != actual.id
        || trusted.public_key != actual.public_key
    {
        bail!("approver key does not match the enrolled human principal");
    }
    let decision = SignedApprovalDecision::create(&request, outcome, reason, decided_at, &approver)
        .context("could not sign approval decision")?;
    store.save_approval_decision(&decision)?;
    Ok(decision)
}

fn approver_key_path(root: &Path, approver_id: Uuid) -> PathBuf {
    root.join(".proof/approvers")
        .join(format!("{approver_id}.json"))
}

fn save_approver_keypair(root: &Path, keypair: &Keypair) -> Result<()> {
    let directory = root.join(".proof/approvers");
    std::fs::create_dir_all(&directory)?;
    let path = approver_key_path(root, keypair.principal_id.as_uuid());
    let serialized = serde_json::to_vec_pretty(&serde_json::json!({
        "principal_id": keypair.principal_id.as_uuid(),
        "kind": keypair.kind,
        "created_at": keypair.created_at,
        "public_key": keypair.signing_key.verifying_key().to_bytes(),
        "signing_key": base64::engine::general_purpose::STANDARD.encode(keypair.signing_key.to_bytes()),
    }))?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&path)
        .with_context(|| format!("could not create approver key: {}", path.display()))?;
    file.write_all(&serialized)?;
    file.sync_all()?;
    Ok(())
}

pub(crate) fn load_approver_keypair(root: &Path, approver_id: Uuid) -> Result<Keypair> {
    let path = approver_key_path(root, approver_id);
    let stored: crate::workspace::StoredKeypair = serde_json::from_str(
        &std::fs::read_to_string(&path)
            .with_context(|| format!("approver key not found: {}", path.display()))?,
    )?;
    if stored.principal_id != approver_id || stored.kind != PrincipalKind::Human {
        bail!("approver key file does not contain the requested human identity");
    }
    let signing_key = base64::engine::general_purpose::STANDARD
        .decode(stored.signing_key)
        .context("invalid stored approver signing key")?;
    let signing_key: [u8; 32] = signing_key
        .try_into()
        .map_err(|_| anyhow::anyhow!("stored approver signing key must be 32 bytes"))?;
    let signing_key = ed25519_dalek::SigningKey::from_bytes(&signing_key);
    if signing_key.verifying_key().to_bytes() != stored.public_key {
        bail!("stored approver keypair public key mismatch");
    }
    Ok(Keypair {
        principal_id: proof_kernel::PrincipalId::new(stored.principal_id),
        kind: stored.kind,
        created_at: stored.created_at,
        signing_key,
    })
}

pub(crate) fn trusted_approver_ids(root: &Path, store: &SqliteStore) -> Result<Vec<Uuid>> {
    let directory = root.join(".proof/approvers");
    if !directory.exists() {
        return Ok(Vec::new());
    }
    let mut approvers = Vec::new();
    for entry in std::fs::read_dir(directory)? {
        let path = entry?.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        let Ok(approver_id) = Uuid::parse_str(stem) else {
            continue;
        };
        let Ok(keypair) = load_approver_keypair(root, approver_id) else {
            continue;
        };
        let Ok(trusted) = store.load_principal(&keypair.principal_id) else {
            continue;
        };
        let actual = principal_from_keypair(&keypair);
        if trusted.kind == PrincipalKind::Human
            && trusted.id == actual.id
            && trusted.public_key == actual.public_key
        {
            approvers.push(approver_id);
        }
    }
    approvers.sort_unstable();
    Ok(approvers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use proof_kernel::{ApprovalOutcome, SignedApprovalRequest};

    fn initialized_cli(directory: &assert_fs::TempDir) -> Cli {
        let cli = Cli::parse_from(["proof", "-w", directory.path().to_str().unwrap(), "init"]);
        crate::commands::content::cmd_init(&cli).unwrap();
        cli
    }

    fn only_approver_id(directory: &assert_fs::TempDir) -> String {
        std::fs::read_dir(directory.path().join(".proof/approvers"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path()
            .file_stem()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string()
    }

    fn save_request(cli: &Cli) -> SignedApprovalRequest {
        let workspace = Workspace::open(&cli.workspace).unwrap();
        let requested_at = Utc::now();
        let request = SignedApprovalRequest::create(
            "release.publish",
            "v1",
            &serde_json::json!({"release_id": "release-1"}),
            requested_at,
            requested_at + chrono::Duration::minutes(15),
            &workspace.keypair,
        )
        .unwrap();
        open_store(&workspace.root)
            .unwrap()
            .save_approval_request(&request)
            .unwrap();
        request
    }

    #[test]
    fn approver_init_enrolls_human_and_writes_private_key() {
        let directory = assert_fs::TempDir::new().unwrap();
        let cli = initialized_cli(&directory);

        cmd_approver_init(&cli).unwrap();

        let approver_id = only_approver_id(&directory);
        let principal_id = proof_kernel::PrincipalId::new(Uuid::parse_str(&approver_id).unwrap());
        let principal = open_store(&cli.workspace)
            .unwrap()
            .load_principal(&principal_id)
            .unwrap();
        assert_eq!(principal.kind, PrincipalKind::Human);
    }

    #[test]
    fn approval_commands_sign_approve_and_deny_decisions() {
        let approved_directory = assert_fs::TempDir::new().unwrap();
        let approved_cli = initialized_cli(&approved_directory);
        cmd_approver_init(&approved_cli).unwrap();
        let approved_approver = only_approver_id(&approved_directory);
        let approved_request = save_request(&approved_cli);
        cmd_approval_list(&approved_cli).unwrap();
        cmd_approval_approve(
            &approved_cli,
            &approved_request.body.id.to_string(),
            &approved_approver,
            Some("reviewed"),
        )
        .unwrap();
        let approved = open_store(&approved_cli.workspace)
            .unwrap()
            .load_approval_decision(&approved_request.body.id)
            .unwrap()
            .unwrap();
        assert_eq!(approved.body.outcome, ApprovalOutcome::Approved);

        let denied_directory = assert_fs::TempDir::new().unwrap();
        let denied_cli = initialized_cli(&denied_directory);
        cmd_approver_init(&denied_cli).unwrap();
        let denied_approver = only_approver_id(&denied_directory);
        let denied_request = save_request(&denied_cli);
        cmd_approval_deny(
            &denied_cli,
            &denied_request.body.id.to_string(),
            &denied_approver,
            Some("unsafe"),
        )
        .unwrap();
        let denied = open_store(&denied_cli.workspace)
            .unwrap()
            .load_approval_decision(&denied_request.body.id)
            .unwrap()
            .unwrap();
        assert_eq!(denied.body.outcome, ApprovalOutcome::Denied);
    }
}
