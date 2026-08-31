use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use rusqlite::{Connection, OptionalExtension};
use serde_json::{json, Value};
use uuid::Uuid;

fn private_tempdir() -> assert_fs::TempDir {
    let directory = assert_fs::TempDir::new_in(env!("CARGO_MANIFEST_DIR")).unwrap();
    #[cfg(unix)]
    std::fs::set_permissions(
        directory.path(),
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )
    .unwrap();
    directory
}

fn proof(workspace: &Path, arguments: &[&str]) -> Output {
    proof_command(workspace, arguments)
        .env("OPENAI_API_KEY", "poison-provider-key-must-not-be-read")
        .env("OPENAI_BASE_URL", "poison-provider-base-must-not-be-read")
        .output()
        .unwrap()
}

fn proof_command(workspace: &Path, arguments: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_proof"));
    command
        .arg("--workspace")
        .arg(workspace)
        .args(arguments)
        .env("OPENAI_API_KEY", "poison-provider-key-must-not-be-read")
        .env("OPENAI_BASE_URL", "poison-provider-base-must-not-be-read")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn successful_output(arguments: &[&str], output: Output) -> Value {
    assert!(
        output.status.success(),
        "command {:?} failed: {}",
        arguments,
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn successful_json(workspace: &Path, arguments: &[&str]) -> Value {
    successful_output(arguments, proof(workspace, arguments))
}

fn concurrent_successful_json(workspace: &Path, arguments: &[&str]) -> (Value, Value) {
    let first = proof_command(workspace, arguments).spawn().unwrap();
    let second = proof_command(workspace, arguments).spawn().unwrap();
    let first = successful_output(arguments, first.wait_with_output().unwrap());
    let second = successful_output(arguments, second.wait_with_output().unwrap());
    (first, second)
}

fn successful_exact_argv(argv: &[Value]) -> Value {
    assert_eq!(argv.first().and_then(Value::as_str), Some("proof"));
    let mut command = Command::new(env!("CARGO_BIN_EXE_proof"));
    for argument in &argv[1..] {
        command.arg(argument.as_str().unwrap());
    }
    let output = command
        .env("OPENAI_API_KEY", "poison-provider-key-must-not-be-read")
        .env("OPENAI_BASE_URL", "poison-provider-base-must-not-be-read")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "exact next argv failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn provision_frozen_registry(root: &Path) {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../registry/content");
    let target = root.join(".proof/registry/content");
    std::fs::create_dir(&target).unwrap();
    for name in [
        "release-publish.json",
        "release-publish.input.json",
        "release-publish.output.json",
        "release-publish-v2.json",
        "release-publish-v2.input.json",
        "release-publish-v2.output.json",
    ] {
        std::fs::copy(source.join(name), target.join(name)).unwrap();
    }
}

fn create_live_bindings(root: &Path, actor_id: &str) -> (String, String) {
    let live_agent = successful_json(
        root,
        &[
            "agent",
            "create",
            "--name",
            "live-release-manager",
            "--instructions",
            "Use only the frozen release publication tool.",
            "--provider",
            "openai",
            "--model",
            "gpt-5.6-sol",
            "--tool",
            "release.publish::v2",
            "--max-steps",
            "2",
            "--max-model-calls",
            "3",
            "--max-total-tokens",
            "10000",
            "--max-duration-seconds",
            "300",
            "--max-output-tokens-per-call",
            "1024",
            "--max-cost-microusd",
            "120000",
        ],
    );
    let agent_id = live_agent["agent"]["id"].as_str().unwrap().to_string();
    let scope = json!({
        "actions": [],
        "resources": [],
        "operation_scope": {
            "allowed_operations": ["release.publish"],
            "allowed_domains": ["content"],
            "resource_scope": null
        }
    })
    .to_string();
    let delegation = successful_json(root, &["delegation", "grant", actor_id, "--scope", &scope]);
    let delegation_id = delegation["delegation_id"].as_str().unwrap().to_string();
    (agent_id, delegation_id)
}

#[test]
fn child_process_empty_registry_fails_closed_without_a_run_or_registry_invention() {
    let directory = private_tempdir();
    let root = directory.path();
    successful_json(root, &["init"]);
    successful_json(root, &["approval", "approver-init"]);
    let preparation_id = Uuid::now_v7().to_string();
    let output = proof(root, &["agent", "live-prepare", "start", &preparation_id]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("registry entry is missing")
            || stderr.contains("registry directory is missing or unsafe")
            || stderr.contains("could not securely open directory")
            || stderr.contains("No such file or directory"),
        "{stderr}"
    );
    assert!(std::fs::read_dir(root.join(".proof/registry"))
        .unwrap()
        .next()
        .is_none());
    let connection = Connection::open(root.join(".proof/storage/storage.db")).unwrap();
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM agent_runs", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[cfg(target_os = "linux")]
#[test]
fn child_process_fresh_workspace_under_root_owned_sticky_tmp_reaches_approval() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let directory = assert_fs::TempDir::new_in("/tmp").unwrap();
    std::fs::set_permissions(directory.path(), PermissionsExt::from_mode(0o700)).unwrap();
    let tmp = directory.path().parent().unwrap();
    let tmp_metadata = std::fs::metadata(tmp).unwrap();
    assert_eq!(tmp, Path::new("/tmp"));
    assert_eq!(
        tmp_metadata.uid(),
        0,
        "test requires the real root-owned /tmp"
    );
    assert_ne!(
        tmp_metadata.permissions().mode() & 0o1000,
        0,
        "test requires sticky /tmp"
    );

    let root = directory.path();
    successful_json(root, &["init"]);
    provision_frozen_registry(root);
    successful_json(root, &["approval", "approver-init"]);
    let preparation_id = Uuid::now_v7().to_string();
    let started = successful_json(root, &["agent", "live-prepare", "start", &preparation_id]);
    assert_eq!(started["status"], "waiting_for_approval");
    assert_eq!(started["preparation_id"], preparation_id);
}

#[cfg(target_family = "unix")]
#[test]
fn child_process_symlinked_workspace_root_fails_before_key_or_database_mutation() {
    use std::os::unix::fs::{symlink, PermissionsExt};

    let container = private_tempdir();
    let real = container.path().join("real-workspace");
    std::fs::create_dir(&real).unwrap();
    std::fs::set_permissions(&real, PermissionsExt::from_mode(0o700)).unwrap();
    successful_json(&real, &["init"]);
    provision_frozen_registry(&real);
    successful_json(&real, &["approval", "approver-init"]);
    let linked = container.path().join("linked-workspace");
    symlink(&real, &linked).unwrap();
    let key_before = std::fs::read(real.join(".proof/keypair.json")).unwrap();
    let database_before = std::fs::read(real.join(".proof/storage/storage.db")).unwrap();

    let preparation_id = Uuid::now_v7().to_string();
    let output = proof(
        &linked,
        &["agent", "live-prepare", "start", &preparation_id],
    );
    assert!(!output.status.success());
    assert_eq!(
        std::fs::read(real.join(".proof/keypair.json")).unwrap(),
        key_before
    );
    assert_eq!(
        std::fs::read(real.join(".proof/storage/storage.db")).unwrap(),
        database_before
    );
    assert!(!real.join(".proof/live-prepare").exists());
}

#[cfg(target_family = "unix")]
#[test]
fn child_process_static_workspace_symlinks_fail_without_key_or_database_mutation() {
    use std::os::unix::fs::symlink;

    for attacked_leaf in [
        ".proof",
        "config.json",
        "keypair.json",
        "storage",
        "storage.db",
    ] {
        let directory = private_tempdir();
        let root = directory.path();
        successful_json(root, &["init"]);
        provision_frozen_registry(root);
        successful_json(root, &["approval", "approver-init"]);

        let (key_path, database_path) = match attacked_leaf {
            ".proof" => {
                let proof = root.join(".proof");
                let real = root.join("real-proof");
                std::fs::rename(&proof, &real).unwrap();
                symlink(&real, &proof).unwrap();
                (real.join("keypair.json"), real.join("storage/storage.db"))
            }
            "config.json" | "keypair.json" => {
                let leaf = root.join(".proof").join(attacked_leaf);
                let real = root.join(format!("real-{attacked_leaf}"));
                std::fs::rename(&leaf, &real).unwrap();
                symlink(&real, &leaf).unwrap();
                (
                    if attacked_leaf == "keypair.json" {
                        real
                    } else {
                        root.join(".proof/keypair.json")
                    },
                    root.join(".proof/storage/storage.db"),
                )
            }
            "storage" => {
                let storage = root.join(".proof/storage");
                let real = root.join("real-storage");
                std::fs::rename(&storage, &real).unwrap();
                symlink(&real, &storage).unwrap();
                (root.join(".proof/keypair.json"), real.join("storage.db"))
            }
            "storage.db" => {
                let database = root.join(".proof/storage/storage.db");
                let real = root.join("real-storage.db");
                std::fs::rename(&database, &real).unwrap();
                symlink(&real, &database).unwrap();
                (root.join(".proof/keypair.json"), real)
            }
            _ => unreachable!(),
        };
        let key_before = std::fs::read(&key_path).unwrap();
        let database_before = std::fs::read(&database_path).unwrap();

        let preparation_id = Uuid::now_v7().to_string();
        let output = proof(root, &["agent", "live-prepare", "start", &preparation_id]);
        assert!(
            !output.status.success(),
            "{attacked_leaf} symlink unexpectedly passed"
        );
        assert_eq!(std::fs::read(&key_path).unwrap(), key_before);
        assert_eq!(std::fs::read(&database_path).unwrap(), database_before);
    }
}

#[cfg(target_family = "unix")]
#[test]
fn child_process_private_config_key_and_database_hard_links_fail_closed() {
    for attacked_leaf in ["config.json", "keypair.json", "storage.db"] {
        let directory = private_tempdir();
        let root = directory.path();
        successful_json(root, &["init"]);
        provision_frozen_registry(root);
        successful_json(root, &["approval", "approver-init"]);
        let leaf = if attacked_leaf == "storage.db" {
            root.join(".proof/storage/storage.db")
        } else {
            root.join(".proof").join(attacked_leaf)
        };
        let alias = root.join(format!("hard-link-{attacked_leaf}"));
        let before = std::fs::read(&leaf).unwrap();
        std::fs::hard_link(&leaf, &alias).unwrap();

        let preparation_id = Uuid::now_v7().to_string();
        let output = proof(root, &["agent", "live-prepare", "start", &preparation_id]);
        assert!(
            !output.status.success(),
            "{attacked_leaf} hard link unexpectedly passed"
        );
        assert_eq!(std::fs::read(&leaf).unwrap(), before);
        assert_eq!(std::fs::read(&alias).unwrap(), before);
    }
}

#[cfg(target_family = "unix")]
#[test]
fn child_process_nonprivate_workspace_paths_and_unsafe_ancestry_fail_closed() {
    use std::os::unix::fs::PermissionsExt;

    for attacked_path in [
        ".",
        ".proof",
        ".proof/storage",
        ".proof/config.json",
        ".proof/keypair.json",
    ] {
        let directory = private_tempdir();
        let root = directory.path();
        successful_json(root, &["init"]);
        provision_frozen_registry(root);
        successful_json(root, &["approval", "approver-init"]);
        let key = root.join(".proof/keypair.json");
        let database = root.join(".proof/storage/storage.db");
        let key_before = std::fs::read(&key).unwrap();
        let database_before = std::fs::read(&database).unwrap();
        let attacked = root.join(attacked_path);
        let mode = if attacked.is_dir() { 0o750 } else { 0o640 };
        std::fs::set_permissions(&attacked, PermissionsExt::from_mode(mode)).unwrap();

        let preparation_id = Uuid::now_v7().to_string();
        let output = proof(root, &["agent", "live-prepare", "start", &preparation_id]);
        assert!(
            !output.status.success(),
            "nonprivate {attacked_path} unexpectedly passed"
        );
        assert_eq!(std::fs::read(&key).unwrap(), key_before);
        assert_eq!(std::fs::read(&database).unwrap(), database_before);
    }

    let container = private_tempdir();
    let root = container.path().join("workspace");
    std::fs::create_dir(&root).unwrap();
    std::fs::set_permissions(&root, PermissionsExt::from_mode(0o700)).unwrap();
    successful_json(&root, &["init"]);
    provision_frozen_registry(&root);
    successful_json(&root, &["approval", "approver-init"]);
    let key = root.join(".proof/keypair.json");
    let database = root.join(".proof/storage/storage.db");
    let key_before = std::fs::read(&key).unwrap();
    let database_before = std::fs::read(&database).unwrap();
    std::fs::set_permissions(container.path(), PermissionsExt::from_mode(0o777)).unwrap();

    let preparation_id = Uuid::now_v7().to_string();
    let output = proof(&root, &["agent", "live-prepare", "start", &preparation_id]);
    assert!(!output.status.success());
    assert_eq!(std::fs::read(&key).unwrap(), key_before);
    assert_eq!(std::fs::read(&database).unwrap(), database_before);
}

#[cfg(target_family = "unix")]
#[test]
fn child_process_kill_after_durable_run_recovers_the_same_approval_boundary() {
    let directory = private_tempdir();
    let root = directory.path();
    successful_json(root, &["init"]);
    provision_frozen_registry(root);
    successful_json(root, &["approval", "approver-init"]);
    let preparation_id = Uuid::now_v7().to_string();
    let arguments = ["agent", "live-prepare", "start", &preparation_id];
    let mut child = proof_command(root, &arguments).spawn().unwrap();
    let connection = Connection::open(root.join(".proof/storage/storage.db")).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let run_id = loop {
        if let Some(run_id) = connection
            .query_row("SELECT id FROM agent_runs LIMIT 1", [], |row| {
                row.get::<_, String>(0)
            })
            .optional()
            .unwrap()
        {
            child.kill().unwrap();
            child.wait().unwrap();
            break run_id;
        }
        assert!(
            child.try_wait().unwrap().is_none(),
            "preparation child exited before the durable run could be interrupted"
        );
        assert!(
            Instant::now() < deadline,
            "preparation child did not persist its bound run before timeout"
        );
        std::thread::yield_now();
    };
    let preparation = root.join(".proof/live-prepare").join(&preparation_id);
    assert!(preparation.join("dispatch.json").is_file());
    assert!(
        !preparation.join("awaiting.json").exists(),
        "kill must precede CLI publication of the approval boundary"
    );

    let recovered = successful_json(root, &arguments);
    assert_eq!(recovered["run_id"], run_id);
    assert_eq!(recovered["status"], "waiting_for_approval");
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM agent_runs", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    let replay = successful_json(root, &arguments);
    assert_eq!(replay, recovered);
}

#[test]
fn child_process_start_approval_finish_is_provider_free_and_replay_stable() {
    let directory = private_tempdir();
    let root = directory.path();
    let initialized = successful_json(root, &["init"]);
    provision_frozen_registry(root);
    let actor_id = initialized["actor_id"].as_str().unwrap().to_string();
    let approver = successful_json(root, &["approval", "approver-init"]);
    let approver_id = approver["approver_id"].as_str().unwrap().to_string();
    let preparation_id = Uuid::now_v7().to_string();

    let start_arguments = ["agent", "live-prepare", "start", &preparation_id];
    let (started, concurrent_started) = concurrent_successful_json(root, &start_arguments);
    assert_eq!(concurrent_started, started);
    assert_eq!(started["status"], "waiting_for_approval");
    assert_eq!(started["approver_id"], approver_id);
    assert_eq!(started["next_argv"][3], "approval");
    let run_id = started["run_id"].as_str().unwrap().to_string();

    successful_exact_argv(started["next_argv"].as_array().unwrap());

    let (agent_id, delegation_id) = create_live_bindings(root, &actor_id);
    let policy =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../evals/release-manager-live-v1.json");
    let policy_text = policy.to_str().unwrap();

    let finish_arguments = [
        "agent",
        "live-prepare",
        "finish",
        &preparation_id,
        "--agent-id",
        &agent_id,
        "--delegation-id",
        &delegation_id,
        "--policy-file",
        policy_text,
    ];
    let (finished, concurrent_finished) = concurrent_successful_json(root, &finish_arguments);
    assert_eq!(concurrent_finished, finished);
    assert_eq!(
        finished["schema"],
        "proof-release-manager-live-readiness/v1"
    );
    assert_eq!(finished["preflight"]["run_id"], run_id);
    assert_eq!(finished["preflight"]["score_bps"], 10_000);
    assert_eq!(finished["preflight"]["passed_checks"], 10);
    assert_eq!(finished["bindings"]["agent_id"], agent_id);
    assert_eq!(finished["bindings"]["delegation_id"], delegation_id);
    assert_eq!(finished["bindings"]["approver_principal_id"], approver_id);

    let database = root.join(".proof/storage/storage.db");
    let connection = Connection::open(database).unwrap();
    let before: (i64, i64, i64, i64) = connection
        .query_row(
            "SELECT
                (SELECT COUNT(*) FROM agent_runs),
                (SELECT COUNT(*) FROM agent_run_events WHERE run_id = ?1),
                (SELECT COUNT(*) FROM agent_run_evaluations WHERE run_id = ?1),
                (SELECT COUNT(*) FROM agent_checkpoints WHERE checkpoint_json LIKE '%agent_runtime_v2%')",
            [&run_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(before.0, 1, "no provider-backed live run may be created");
    assert_eq!(before.2, 2, "one runtime and one exact trace evaluation");
    assert_eq!(before.3, 0, "no live v2 checkpoint may be created");
    assert!(!root.join(".proof/artifacts").exists());

    let replay = successful_json(root, &finish_arguments);
    assert_eq!(replay, finished);
    let after: (i64, i64, i64, i64) = connection
        .query_row(
            "SELECT
                (SELECT COUNT(*) FROM agent_runs),
                (SELECT COUNT(*) FROM agent_run_events WHERE run_id = ?1),
                (SELECT COUNT(*) FROM agent_run_evaluations WHERE run_id = ?1),
                (SELECT COUNT(*) FROM agent_checkpoints WHERE checkpoint_json LIKE '%agent_runtime_v2%')",
            [&run_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(after, before);
}

#[cfg(target_family = "unix")]
#[test]
fn child_process_symlinked_edition_component_fails_closed_and_retry_is_stable() {
    use std::os::unix::fs::symlink;

    let directory = private_tempdir();
    let root = directory.path();
    let initialized = successful_json(root, &["init"]);
    provision_frozen_registry(root);
    let actor_id = initialized["actor_id"].as_str().unwrap().to_string();
    successful_json(root, &["approval", "approver-init"]);
    let preparation_id = Uuid::now_v7().to_string();
    let started = successful_json(root, &["agent", "live-prepare", "start", &preparation_id]);
    successful_exact_argv(started["next_argv"].as_array().unwrap());
    let (agent_id, delegation_id) = create_live_bindings(root, &actor_id);
    let policy =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../evals/release-manager-live-v1.json");
    let policy_text = policy.to_str().unwrap();
    let finish_arguments = [
        "agent",
        "live-prepare",
        "finish",
        &preparation_id,
        "--agent-id",
        &agent_id,
        "--delegation-id",
        &delegation_id,
        "--policy-file",
        policy_text,
    ];

    let editions = root.join(".proof/data/editions");
    std::fs::remove_dir(&editions).unwrap();
    let outside = root.join("outside-editions");
    std::fs::create_dir(&outside).unwrap();
    symlink(&outside, &editions).unwrap();

    let first = proof(root, &finish_arguments);
    assert!(!first.status.success());
    assert!(String::from_utf8_lossy(&first.stderr).contains("symbolic link"));
    let connection = Connection::open(root.join(".proof/storage/storage.db")).unwrap();
    let run_id = started["run_id"].as_str().unwrap();
    let counts = connection
        .query_row(
            "SELECT
                (SELECT COUNT(*) FROM agent_runs),
                (SELECT COUNT(*) FROM agent_run_events WHERE run_id = ?1),
                (SELECT COUNT(*) FROM agent_run_evaluations WHERE run_id = ?1)",
            [run_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .unwrap();

    let retry = proof(root, &finish_arguments);
    assert!(!retry.status.success());
    assert!(String::from_utf8_lossy(&retry.stderr).contains("symbolic link"));
    let replay_counts = connection
        .query_row(
            "SELECT
                (SELECT COUNT(*) FROM agent_runs),
                (SELECT COUNT(*) FROM agent_run_events WHERE run_id = ?1),
                (SELECT COUNT(*) FROM agent_run_evaluations WHERE run_id = ?1)",
            [run_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(replay_counts, counts);
    assert!(std::fs::read_dir(&outside).unwrap().next().is_none());
    assert!(!root
        .join(".proof/live-prepare")
        .join(&preparation_id)
        .join("ready.json")
        .exists());
}

#[cfg(target_family = "unix")]
#[test]
fn child_process_symlinked_final_edition_leaf_fails_without_touching_target() {
    use std::os::unix::fs::symlink;

    let directory = private_tempdir();
    let root = directory.path();
    let initialized_workspace = successful_json(root, &["init"]);
    provision_frozen_registry(root);
    let actor_id = initialized_workspace["actor_id"]
        .as_str()
        .unwrap()
        .to_string();
    successful_json(root, &["approval", "approver-init"]);
    let preparation_id = Uuid::now_v7().to_string();
    let started = successful_json(root, &["agent", "live-prepare", "start", &preparation_id]);
    successful_exact_argv(started["next_argv"].as_array().unwrap());
    let (agent_id, delegation_id) = create_live_bindings(root, &actor_id);
    let policy =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../evals/release-manager-live-v1.json");
    let policy_text = policy.to_str().unwrap();
    let finish_arguments = [
        "agent",
        "live-prepare",
        "finish",
        &preparation_id,
        "--agent-id",
        &agent_id,
        "--delegation-id",
        &delegation_id,
        "--policy-file",
        policy_text,
    ];

    let initialized: Value = serde_json::from_slice(
        &std::fs::read(
            root.join(".proof/live-prepare")
                .join(&preparation_id)
                .join("initialized.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let edition_id = initialized["edition"]["id"].as_str().unwrap();
    let outside = root.join("outside-edition.json");
    let outside_bytes = b"unrelated immutable bytes";
    std::fs::write(&outside, outside_bytes).unwrap();
    symlink(
        &outside,
        root.join(".proof/data/editions")
            .join(format!("{edition_id}.json")),
    )
    .unwrap();

    let first = proof(root, &finish_arguments);
    assert!(!first.status.success());
    assert_eq!(std::fs::read(&outside).unwrap(), outside_bytes);
    let retry = proof(root, &finish_arguments);
    assert!(!retry.status.success());
    assert_eq!(std::fs::read(&outside).unwrap(), outside_bytes);
    assert!(!root
        .join(".proof/live-prepare")
        .join(&preparation_id)
        .join("ready.json")
        .exists());
}

#[cfg(target_family = "unix")]
#[test]
fn child_process_symlinked_data_component_fails_closed_and_retry_is_stable() {
    use std::os::unix::fs::symlink;

    let directory = private_tempdir();
    let root = directory.path();
    let initialized = successful_json(root, &["init"]);
    provision_frozen_registry(root);
    let actor_id = initialized["actor_id"].as_str().unwrap().to_string();
    successful_json(root, &["approval", "approver-init"]);
    let preparation_id = Uuid::now_v7().to_string();
    let started = successful_json(root, &["agent", "live-prepare", "start", &preparation_id]);
    successful_exact_argv(started["next_argv"].as_array().unwrap());
    let (agent_id, delegation_id) = create_live_bindings(root, &actor_id);
    let policy =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../evals/release-manager-live-v1.json");
    let policy_text = policy.to_str().unwrap();
    let finish_arguments = [
        "agent",
        "live-prepare",
        "finish",
        &preparation_id,
        "--agent-id",
        &agent_id,
        "--delegation-id",
        &delegation_id,
        "--policy-file",
        policy_text,
    ];

    let data = root.join(".proof/data");
    std::fs::remove_dir_all(&data).unwrap();
    let outside = root.join("outside-data");
    std::fs::create_dir(&outside).unwrap();
    symlink(&outside, &data).unwrap();

    let first = proof(root, &finish_arguments);
    assert!(!first.status.success());
    assert!(String::from_utf8_lossy(&first.stderr).contains("symbolic link"));
    let connection = Connection::open(root.join(".proof/storage/storage.db")).unwrap();
    let run_id = started["run_id"].as_str().unwrap();
    let counts = connection
        .query_row(
            "SELECT
                (SELECT COUNT(*) FROM agent_runs),
                (SELECT COUNT(*) FROM agent_run_events WHERE run_id = ?1),
                (SELECT COUNT(*) FROM agent_run_evaluations WHERE run_id = ?1)",
            [run_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .unwrap();

    let retry = proof(root, &finish_arguments);
    assert!(!retry.status.success());
    assert!(String::from_utf8_lossy(&retry.stderr).contains("symbolic link"));
    let replay_counts = connection
        .query_row(
            "SELECT
                (SELECT COUNT(*) FROM agent_runs),
                (SELECT COUNT(*) FROM agent_run_events WHERE run_id = ?1),
                (SELECT COUNT(*) FROM agent_run_evaluations WHERE run_id = ?1)",
            [run_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(replay_counts, counts);
    assert!(std::fs::read_dir(&outside).unwrap().next().is_none());
    assert!(!root
        .join(".proof/live-prepare")
        .join(&preparation_id)
        .join("ready.json")
        .exists());
}
