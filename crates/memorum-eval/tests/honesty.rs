use std::process::Command;

use memorum_eval::daemon_scaffold::DaemonScaffold;
use memorum_eval::harness_runner::{MockHarness, TestOutcome};
use serde_json::Value;

#[tokio::test]
async fn mock_harness_t13_t15_are_semantic_skips_not_passes() {
    let scaffold = DaemonScaffold::fresh().await;

    for test_id in [13, 15] {
        let outcome = MockHarness.run_test(test_id, &scaffold).expect("mock harness returns outcome");
        let TestOutcome::Skipped { metadata, reason } = outcome else {
            panic!("mock Test #{test_id} must skip rather than pass: {outcome:#?}");
        };

        assert_eq!(metadata.get("mode").map(String::as_str), Some("mock"));
        assert_eq!(reason, "MOCK_HARNESS_SEMANTIC_NOT_EXERCISED");
    }
}

#[test]
fn t19_feature_disabled_stub_uses_memorum_skip_marker() {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let target_dir = tempfile::tempdir().expect("isolated cargo target dir");
    let output = Command::new(cargo)
        .args([
            "test",
            "-p",
            "memorum-eval",
            "--no-default-features",
            "--target-dir",
            target_dir.path().to_str().expect("target dir path should be utf-8"),
            "--test",
            "t19_peer_update_framing",
            "--",
            "--nocapture",
        ])
        .output()
        .expect("run t19 without default features");

    assert!(output.status.success(), "t19 no-default-features test failed: {}", diagnostic(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("MEMORUM_EVAL_SKIP:STREAM_I_DEPS_DISABLED"),
        "t19 skip marker should use orchestrator prefix; stdout was:\n{stdout}"
    );
    assert!(!stdout.contains("SKIP: stream-i-deps feature disabled"));
}

#[test]
fn mock_orchestrator_reports_t13_t15_as_skipped() {
    let fake_cargo = fake_passing_cargo();
    let output = Command::new(env!("CARGO_BIN_EXE_memorum-eval"))
        .args(["--harness", "mock", "--output", "json"])
        .env("MEMORUM_EVAL_CARGO", &fake_cargo)
        .output()
        .expect("spawn memorum-eval");

    assert!(output.status.success(), "mock run should exit 0 despite honest partial status: {}", diagnostic(&output));
    let report: Value = serde_json::from_slice(&output.stdout).expect("json report");
    let tests = report["tests"].as_array().expect("tests array");

    for test_id in [13, 15] {
        let test = tests.iter().find(|test| test["number"] == test_id).expect("test result present");
        assert_eq!(test["status"], "skipped", "mock Test #{test_id} should not pass: {test:#?}");
        assert_eq!(test["skip_reason"], "MOCK_HARNESS_SEMANTIC_NOT_EXERCISED");
    }
}

fn fake_passing_cargo() -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("memorum-eval-honesty-cargo-{}.sh", std::process::id()));
    std::fs::write(
        &path,
        "#!/bin/sh\nprintf 'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\\n'\nprintf 'MEMORUM_EVAL_ASSERTIONS=1\\n'\nexit 0\n",
    )
    .expect("write fake cargo");
    let mut permissions = std::fs::metadata(&path).expect("fake cargo metadata").permissions();
    use std::os::unix::fs::PermissionsExt;
    permissions.set_mode(0o755);
    std::fs::set_permissions(&path, permissions).expect("fake cargo executable");
    path
}

fn diagnostic(output: &std::process::Output) -> String {
    format!(
        "status={}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}
