use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use base64::Engine as _;
use chrono::{DateTime, Utc};
use proof_agent_runtime::{runtime_approval_context, RuntimeApprovalContext};
use proof_kernel::{
    canonicalize, digest, generate_keypair_for, principal_from_keypair, AgentRunStatus,
    AgentRunStepStatus, ApprovalOutcome, ArtifactKind, Keypair, PrincipalKind,
    SignedApprovalDecision, SignedApprovalRequest,
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

    if let Some(runtime) = validated_native_runtime_approval_context(store, &request)? {
        if runtime.checkpoint_kind == "agent_runtime_v2" {
            let required_approver_id = runtime
                .required_approver_id
                .context("sealed live runtime approval is missing its required human approver")?;
            if approver_id != required_approver_id {
                bail!(
                    "approval request {request_id} requires the sealed human approver {required_approver_id}"
                );
            }
        }
    }

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

/// Validates the complete native runtime history behind a linked approval.
///
/// Generic approvals have no linked step and return `None`. Native v1 remains
/// compatible, while live v2 is required to reproduce the exact durable
/// request, waiting step, pending call, and policy-bound Human before a caller
/// may persist a decision.
pub(super) fn validated_native_runtime_approval_context(
    store: &SqliteStore,
    request: &SignedApprovalRequest,
) -> Result<Option<RuntimeApprovalContext>> {
    let Some(step) = store.find_agent_run_step_by_approval(&request.body.id)? else {
        return Ok(None);
    };
    let run = store
        .load_agent_run(&step.run_id)?
        .with_context(|| format!("native approval run not found: {}", step.run_id))?;
    if run.status != AgentRunStatus::WaitingForInput {
        bail!("native approval run is not waiting for input");
    }
    if run.actor != request.body.requested_by {
        bail!("native approval run actor does not match the signed requester");
    }
    if step.status != AgentRunStepStatus::WaitingForApproval
        || step.approval_request_id != Some(request.body.id)
        || step.operation != request.body.operation
        || step.version != request.body.version
        || step.input_digest != request.body.input_digest
    {
        bail!("native approval step does not match the signed request");
    }

    let checkpoints = store.list_agent_checkpoints(&run.id)?;
    let events = store.list_agent_run_events(&run.id)?;
    let runtime = runtime_approval_context(run.id, &checkpoints, &events)
        .context("native runtime approval history is missing, mixed, unsupported, or invalid")?;
    if runtime.run_id != run.id || Some(runtime.agent_id) != run.agent_id {
        bail!("native runtime approval context does not match the durable run");
    }
    let pending = runtime
        .pending_tool
        .as_ref()
        .context("native runtime approval context has no pending tool call")?;
    if pending.approval_request_id != Some(request.body.id)
        || pending.step_id != step.id
        || pending.operation != request.body.operation
        || pending.version != request.body.version
    {
        bail!("native runtime pending tool does not match the signed approval");
    }
    let pending_input_digest = digest(
        ArtifactKind::OperationInput,
        &canonicalize(&pending.arguments)
            .context("native runtime pending arguments cannot be canonicalized")?,
    );
    if pending_input_digest != request.body.input_digest
        || pending_input_digest != step.input_digest
    {
        bail!("native runtime pending arguments do not match the sealed input digest");
    }

    match runtime.checkpoint_kind.as_str() {
        "agent_runtime_v1" => {}
        "agent_runtime_v2" => {
            if runtime.sealed_approval_request.as_ref() != Some(request) {
                bail!("live runtime does not seal the exact signed approval request");
            }
            if runtime.sealed_step.as_ref() != Some(&step) {
                bail!("live runtime does not seal the exact durable waiting step");
            }
            if runtime.required_approver_id.is_none() {
                bail!("live runtime does not seal a required human approver");
            }
        }
        _ => bail!("native runtime approval checkpoint version is unsupported"),
    }
    Ok(Some(runtime))
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

    #[test]
    fn live_v2_decision_rejects_a_different_enrolled_human_before_persistence() {
        let fixture = crate::commands::live::tests::approval_live_fixture();
        let cli = Cli::parse_from([
            "proof",
            "--workspace",
            fixture.workspace.root.to_str().unwrap(),
            "approval",
            "list",
        ]);
        cmd_approver_init(&cli).unwrap();
        let wrong_approver_id = trusted_approver_ids(&fixture.workspace.root, &fixture.store)
            .unwrap()
            .into_iter()
            .find(|candidate| *candidate != fixture.approver_id)
            .unwrap();

        let error = sign_approval_decision(
            &fixture.workspace.root,
            &fixture.store,
            fixture.request.body.id,
            wrong_approver_id,
            ApprovalOutcome::Approved,
            Some("wrong human".to_string()),
            Utc::now(),
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("requires the sealed human approver"),
            "{error:#}"
        );
        assert_eq!(
            fixture
                .store
                .load_approval_decision(&fixture.request.body.id)
                .unwrap(),
            None
        );
    }

    #[test]
    fn live_v2_decision_rejects_a_durable_step_substitution_before_persistence() {
        let fixture = crate::commands::live::tests::approval_live_fixture();
        let mut step = fixture
            .store
            .find_agent_run_step_by_approval(&fixture.request.body.id)
            .unwrap()
            .unwrap();
        step.updated_at += chrono::Duration::milliseconds(1);
        let connection =
            rusqlite::Connection::open(fixture.workspace.root.join(".proof/storage/storage.db"))
                .unwrap();
        connection
            .execute(
                "UPDATE agent_run_steps SET step_json = ?1 WHERE id = ?2",
                rusqlite::params![serde_json::to_string(&step).unwrap(), step.id.to_string()],
            )
            .unwrap();

        let error = sign_approval_decision(
            &fixture.workspace.root,
            &fixture.store,
            fixture.request.body.id,
            fixture.approver_id,
            ApprovalOutcome::Approved,
            Some("substituted step".to_string()),
            Utc::now(),
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("does not seal the exact durable waiting step"),
            "{error:#}"
        );
        assert_eq!(
            fixture
                .store
                .load_approval_decision(&fixture.request.body.id)
                .unwrap(),
            None
        );
    }

    #[test]
    fn live_v2_decision_rejects_a_resigned_request_substitution_before_persistence() {
        use ed25519_dalek::Signer as _;

        let fixture = crate::commands::live::tests::approval_live_fixture();
        let mut substituted = fixture.request.clone();
        substituted.body.expires_at += chrono::Duration::seconds(1);
        let payload = proof_kernel::canonicalize_serialized(&substituted.body).unwrap();
        substituted.signature = fixture
            .workspace
            .keypair
            .signing_key
            .sign(payload.as_bytes())
            .to_bytes()
            .to_vec();
        let connection =
            rusqlite::Connection::open(fixture.workspace.root.join(".proof/storage/storage.db"))
                .unwrap();
        connection
            .execute(
                "UPDATE approval_requests SET request_json = ?1 WHERE id = ?2",
                rusqlite::params![
                    serde_json::to_string(&substituted).unwrap(),
                    substituted.body.id.to_string()
                ],
            )
            .unwrap();

        let error = sign_approval_decision(
            &fixture.workspace.root,
            &fixture.store,
            fixture.request.body.id,
            fixture.approver_id,
            ApprovalOutcome::Approved,
            Some("substituted request".to_string()),
            Utc::now(),
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("does not seal the exact signed approval request"),
            "{error:#}"
        );
        assert_eq!(
            fixture
                .store
                .load_approval_decision(&fixture.request.body.id)
                .unwrap(),
            None
        );
    }
}
