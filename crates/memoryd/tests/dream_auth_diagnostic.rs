use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use memoryd::dream::error::HarnessCliError;
use memoryd::dream::harness::{run_hardened_command, HardenedCommand, MinimalEnvironment};
use memoryd::protocol::PromptTransport;

#[tokio::test]
async fn auth_probe_diagnostic_preserves_operator_message_and_redacts_api_key() {
    let temp = tempfile::tempdir().expect("tempdir");
    let probe = temp.path().join("auth-probe");
    write_executable(
        &probe,
        r#"#!/bin/sh
printf 'not logged in: session expired for sk-ant-api03-abcdef1234567890; run cli auth login\n' >&2
exit 1
"#,
    );

    let error = run_hardened_command(
        HardenedCommand {
            program: probe,
            args: Vec::new(),
            prompt_transport: PromptTransport::Stdin,
            expect_json: false,
            timeout: Duration::from_secs(2),
            kill_grace: Duration::from_millis(250),
            scratch_root: temp.path().join("scratch"),
            environment: MinimalEnvironment::from_pairs([
                ("PATH", std::env::var("PATH").expect("PATH is set")),
                ("HOME", temp.path().display().to_string()),
            ]),
            redact_stderr: false,
        },
        "",
    )
    .await
    .expect_err("auth probe should fail");

    let HarnessCliError::SubprocessExit { stderr_tail, .. } = error else {
        panic!("expected subprocess exit, got {error:?}");
    };
    assert!(stderr_tail.contains("not logged in: session expired"), "{stderr_tail}");
    assert!(stderr_tail.contains("run cli auth login"), "{stderr_tail}");
    assert!(stderr_tail.contains("[redacted-secret]"), "{stderr_tail}");
    assert!(!stderr_tail.contains("sk-ant-api03"), "{stderr_tail}");
    assert!(!stderr_tail.contains("sha256"), "{stderr_tail}");
    assert!(stderr_tail.len() <= 4 * 1024 + 128, "diagnostic should stay capped: {}", stderr_tail.len());
}

fn write_executable(path: &std::path::Path, body: &str) {
    let mut file = std::fs::File::create(path).expect("create script");
    file.write_all(body.as_bytes()).expect("write script");
    let mut permissions = file.metadata().expect("metadata").permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).expect("chmod script");
}
