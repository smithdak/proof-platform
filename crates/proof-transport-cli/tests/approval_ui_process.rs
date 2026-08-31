#![cfg(target_os = "linux")]

use std::path::Path;
use std::process::{Command, Stdio};

fn proof(workspace: &Path, arguments: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_proof"))
        .arg("--workspace")
        .arg(workspace)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env(
            "PROOF_APPROVAL_SENTINEL",
            "synthetic-process-secret-sentinel",
        )
        .output()
        .unwrap()
}

#[test]
fn non_tty_launch_fails_before_output_or_credential_creation() {
    let directory = assert_fs::TempDir::new_in(env!("CARGO_MANIFEST_DIR")).unwrap();
    std::fs::set_permissions(
        directory.path(),
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )
    .unwrap();
    let initialized = proof(directory.path(), &["init"]);
    assert!(initialized.status.success());

    let argv = ["approval", "ui", "--port", "0"];
    assert!(argv.iter().all(|argument| {
        !argument.contains("synthetic-process-secret-sentinel")
            && !argument.contains("x-proof-session")
    }));
    let output = proof(directory.path(), &argv);
    assert!(!output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    for sentinel in [
        "synthetic-process-secret-sentinel",
        "0123456789abcdef",
        "1111111111111111111111111111111111111111111111111111111111111111",
    ] {
        assert!(!stdout.contains(sentinel));
        assert!(!stderr.contains(sentinel));
    }
    assert!(!stdout.contains("http://127.0.0.1:"));
    assert!(stderr.contains("controlling terminal"), "{stderr}");
}
