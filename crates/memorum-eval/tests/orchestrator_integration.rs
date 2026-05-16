use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

#[test]
fn list_outputs_all_20_catalog_entries() {
    let output = memorum_eval(["--list"]);

    assert!(output.status.success(), "expected --list to exit 0: {}", diagnostic(&output));

    let stdout = String::from_utf8(output.stdout).expect("--list stdout should be utf-8");
    let entry_count = stdout.lines().filter(|line| line.starts_with('#')).count();
    assert_eq!(entry_count, 20, "catalog output should list all eval tests:\n{stdout}");
    assert!(
        stdout.contains("#20 web_source_grounding"),
        "catalog output should include the web source grounding eval:\n{stdout}"
    );
}

#[test]
fn filtered_json_run_reports_spec_result_fields() {
    let fake_cargo = fake_passing_cargo();
    let output = Command::new(env!("CARGO_BIN_EXE_memorum-eval"))
        .args(["--filter", "t01", "--output", "json"])
        .env("MEMORUM_EVAL_CARGO", fake_cargo.path())
        .output()
        .expect("spawn memorum-eval");

    assert!(output.status.success(), "expected filtered run to exit 0: {}", diagnostic(&output));

    let report = json_stdout(output);
    assert_eq!(report["total"], 1);
    assert_eq!(report["failed"], 0);
    assert_eq!(report["partial"], false);
    assert_eq!(report["missing_credentials"].as_array().expect("missing_credentials should be an array").len(), 0);

    let test = &report["tests"][0];
    assert_eq!(test["number"], 1);
    assert_eq!(test["name"], "exact_identifier_recall");
    assert_eq!(test["status"], "passed", "fake passing cargo should report a passed test row: {test:#?}");
    assert!(test.get("failure_detail").is_some(), "failure_detail field should always be present: {test:#?}");
}

#[test]
fn mock_harness_runs_catalog_without_live_credentials_or_real_cargo_failures() {
    let fake_cargo = fake_passing_cargo();
    let output = Command::new(env!("CARGO_BIN_EXE_memorum-eval"))
        .args(["--harness", "mock", "--output", "json"])
        .env("MEMORUM_EVAL_CARGO", fake_cargo.path())
        .output()
        .expect("spawn memorum-eval");

    assert!(output.status.success(), "expected partial mock run to exit 0: {}", diagnostic(&output));

    let report = json_stdout(output);
    assert_eq!(report["total"], 20);
    assert_eq!(report["failed"], 0);
    assert_eq!(
        report["partial"], true,
        "mock real-harness semantics are honest skips even when simulator dispatches pass: {report:#?}"
    );

    let tests = report["tests"].as_array().expect("tests should be an array");
    let t13 = find_test(tests, 13);
    let t15 = find_test(tests, 15);
    let t20 = find_test(tests, 20);
    assert_eq!(t13["status"], "skipped", "mock mode must not pass Test #13 without real semantics: {t13:#?}");
    assert_eq!(
        t13["skip_reason"], "MOCK_HARNESS_SEMANTIC_NOT_EXERCISED",
        "mock semantic skip is reported honestly: {t13:#?}"
    );
    assert_eq!(t15["status"], "skipped", "mock mode must not pass Test #15 without real semantics: {t15:#?}");
    assert_eq!(
        t15["skip_reason"], "MOCK_HARNESS_SEMANTIC_NOT_EXERCISED",
        "mock semantic skip is reported honestly: {t15:#?}"
    );
    assert_eq!(t20["status"], "passed", "mock mode should dispatch simulator test #20: {t20:#?}");

    let skipped_real_harness_tests = skipped_real_harness_tests(tests);

    #[cfg(not(feature = "stream-i-deps"))]
    {
        assert_eq!(
            skipped_real_harness_tests.len(),
            3,
            "mock mode should skip #13/#15 plus #19 until Stream I deps land"
        );
        let t19 = find_test(tests, 19);
        assert_eq!(t19["status"], "skipped");
        assert_eq!(t19["skip_reason"], "STREAM_I_DEPS_DISABLED");
        assert_eq!(t19["failure_detail"], Value::Null);
    }

    #[cfg(feature = "stream-i-deps")]
    {
        assert!(
            skipped_real_harness_tests.len() == 2,
            "mock mode should skip #13/#15 and execute #19 when Stream I deps are enabled: {skipped_real_harness_tests:#?}"
        );
        let t19 = find_test(tests, 19);
        assert_eq!(t19["status"], "passed", "mock mode should execute MockHarness test #19: {t19:#?}");
    }
}

#[test]
fn runnable_catalog_failure_propagates_to_json_and_exit_code() {
    let fake_cargo = fake_failing_cargo();
    let output = Command::new(env!("CARGO_BIN_EXE_memorum-eval"))
        .args(["--filter", "t01", "--output", "json"])
        .env("MEMORUM_EVAL_CARGO", fake_cargo.path())
        .output()
        .expect("spawn memorum-eval");

    assert!(!output.status.success(), "expected dispatched catalog failure: {}", diagnostic(&output));

    let report = json_stdout(output);
    assert_eq!(report["total"], 1);
    assert_eq!(report["passed"], 0);
    assert_eq!(report["failed"], 1);

    let test = &report["tests"][0];
    assert_eq!(test["number"], 1);
    assert_eq!(test["status"], "failed");
    assert!(
        test["failure_detail"].as_str().is_some_and(|detail| detail.contains("fake cargo failure")),
        "failure_detail should include subprocess stderr: {test:#?}"
    );
}

#[test]
fn real_harness_with_present_credentials_does_not_skip_as_not_implemented() {
    let fake_cargo = fake_failing_cargo();
    let fake_harness_path = fake_harness_path();

    for (test_number, expected_number) in [("t13", 13), ("t15", 15)] {
        let output = Command::new(env!("CARGO_BIN_EXE_memorum-eval"))
            .args(["--harness", "all", "--filter", test_number, "--output", "json"])
            .env("MEMORUM_EVAL_CARGO", fake_cargo.path())
            .env("MEMORUM_EVAL_CLAUDE_KEY", "fake-present-claude-key")
            .env("MEMORUM_EVAL_CODEX_KEY", "fake-present-codex-key")
            .env("PATH", fake_harness_path.path())
            .output()
            .expect("spawn memorum-eval");

        assert!(!output.status.success(), "fake cargo should make dispatched real-harness run fail");

        let report = json_stdout(output);
        assert_eq!(report["total"], 1);
        assert_eq!(report["missing_credentials"].as_array().expect("missing_credentials array").len(), 0);

        let test = &report["tests"][0];
        assert_eq!(test["number"], expected_number);
        assert_eq!(test["status"], "failed", "present credentials should dispatch through cargo: {test:#?}");
        assert_eq!(test["skip_reason"], Value::Null, "real-harness dispatch must not skip: {test:#?}");
        assert!(
            !test.to_string().contains("REAL_HARNESS_ORCHESTRATOR_NOT_IMPLEMENTED"),
            "real-harness dispatch must not use the old not-implemented skip: {test:#?}"
        );
    }
}

#[test]
fn t16_dispatches_instead_of_stale_stream_g_skip() {
    let fake_cargo = fake_failing_cargo();
    let output = Command::new(env!("CARGO_BIN_EXE_memorum-eval"))
        .args(["--harness", "mock", "--filter", "t16", "--output", "json"])
        .env("MEMORUM_EVAL_CARGO", fake_cargo.path())
        .output()
        .expect("spawn memorum-eval");

    assert!(!output.status.success(), "fake cargo should make dispatched #16 fail");

    let report = json_stdout(output);
    assert_eq!(report["total"], 1);

    let test = &report["tests"][0];
    assert_eq!(test["number"], 16);
    assert_eq!(test["status"], "failed", "#16 should run through cargo dispatch: {test:#?}");
    assert_eq!(test["skip_reason"], Value::Null, "#16 must not be pre-skipped: {test:#?}");
    assert!(
        !test.to_string().contains("STREAM_G_DEPS_NOT_SHIPPED"),
        "#16 should let the runtime test decide dependency availability: {test:#?}"
    );
}

#[test]
fn output_file_receives_same_json_report_as_stdout() {
    let output_path = std::env::temp_dir().join(format!("memorum-eval-output-{}.json", std::process::id()));
    let fake_cargo = fake_passing_cargo();
    let output = Command::new(env!("CARGO_BIN_EXE_memorum-eval"))
        .args([
            "--filter",
            "t01",
            "--output",
            "json",
            "--output-file",
            output_path.to_str().expect("temp path should be utf-8"),
        ])
        .env("MEMORUM_EVAL_CARGO", fake_cargo.path())
        .output()
        .expect("spawn memorum-eval");

    assert!(output.status.success(), "expected output-file run to exit 0: {}", diagnostic(&output));

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf-8");
    let file_body = fs::read_to_string(&output_path).expect("output file should be written");
    fs::remove_file(output_path).expect("output file should be cleaned up");
    assert_eq!(
        serde_json::from_str::<Value>(&stdout).expect("stdout JSON should parse"),
        serde_json::from_str::<Value>(&file_body).expect("file JSON should parse")
    );
}

fn memorum_eval<const N: usize>(args: [&str; N]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_memorum-eval")).args(args).output().expect("spawn memorum-eval")
}

struct TempPath {
    _dir: tempfile::TempDir,
    path: PathBuf,
}

impl TempPath {
    fn path(&self) -> &Path {
        &self.path
    }
}

fn fake_failing_cargo() -> TempPath {
    fake_cargo_script(
        "memorum-eval-fake-cargo",
        "#!/bin/sh\nprintf 'fake cargo failure for args: %s\\n' \"$*\" >&2\nexit 42\n",
    )
}

fn fake_passing_cargo() -> TempPath {
    fake_cargo_script(
        "memorum-eval-fake-pass-cargo",
        "#!/bin/sh\nprintf 'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\\n'\nprintf 'MEMORUM_EVAL_ASSERTIONS=1\\n'\nexit 0\n",
    )
}

fn fake_sleeping_cargo() -> TempPath {
    fake_cargo_script("memorum-eval-sleep-cargo", "#!/bin/sh\nsleep 30\nprintf 'should not complete\\n'\nexit 0\n")
}

fn fake_no_cleanup_checking_cargo() -> TempPath {
    fake_cargo_script(
        "memorum-eval-no-cleanup-cargo",
        "#!/bin/sh\nif [ \"$MEMORUM_EVAL_NO_CLEANUP\" != \"1\" ]; then echo missing no-cleanup env >&2; exit 42; fi\nprintf 'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\\n'\nprintf 'MEMORUM_EVAL_ASSERTIONS=1\\n'\nexit 0\n",
    )
}

fn fake_harness_path() -> TempPath {
    let dir = tempfile::Builder::new().prefix("memorum-eval-fake-harnesses").tempdir().expect("fake harness dir");
    write_fake_harness(dir.path(), "claude");
    write_fake_harness(dir.path(), "codex");
    let path = dir.path().to_path_buf();
    TempPath { _dir: dir, path }
}

fn fake_cargo_script(prefix: &str, body: &str) -> TempPath {
    let dir = tempfile::Builder::new().prefix(prefix).tempdir().expect("fake cargo tempdir");
    let path = dir.path().join(format!("{prefix}.sh"));
    write_executable(&path, body, "fake cargo");
    TempPath { _dir: dir, path }
}

fn write_fake_harness(dir: &Path, name: &str) {
    let path = dir.join(name);
    write_executable(
        &path,
        "#!/bin/sh\nif [ \"$1\" = \"--help\" ]; then echo '--mcp-config'; exit 0; fi\nexit 42\n",
        "fake harness",
    );
}

fn write_executable(path: &Path, body: &str, label: &str) {
    fs::write(path, body).unwrap_or_else(|error| panic!("write {label}: {error}"));
    let mut permissions = fs::metadata(path).unwrap_or_else(|error| panic!("{label} metadata: {error}")).permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap_or_else(|error| panic!("chmod {label}: {error}"));
}

fn find_test(tests: &[Value], number: u64) -> &Value {
    tests
        .iter()
        .find(|test| test["number"].as_u64() == Some(number))
        .unwrap_or_else(|| panic!("missing test #{number} in {tests:#?}"))
}

fn skipped_real_harness_tests(tests: &[Value]) -> Vec<&Value> {
    tests.iter().filter(|test| test["mode"] == "real_harness").filter(|test| test["status"] == "skipped").collect()
}

/// H-B1 regression: T17 and T18 must not be permanently skipped at the orchestrator
/// level; they should be cargo-dispatched so their in-test skip guards run.
#[test]
fn t17_and_t18_dispatch_through_cargo_not_permanently_skipped() {
    let fake_cargo = fake_failing_cargo();
    for (test_number, expected_number) in [("t17", 17), ("t18", 18)] {
        let output = Command::new(env!("CARGO_BIN_EXE_memorum-eval"))
            .args(["--harness", "mock", "--filter", test_number, "--output", "json"])
            .env("MEMORUM_EVAL_CARGO", fake_cargo.path())
            .output()
            .expect("spawn memorum-eval");

        // fake cargo fails, so the test should be FAILED (dispatched), not SKIPPED (pre-empted)
        assert!(!output.status.success(), "fake cargo should cause {test_number} to fail");

        let report = json_stdout(output);
        assert_eq!(report["total"], 1);
        let test = &report["tests"][0];
        assert_eq!(test["number"], expected_number);
        assert_eq!(
            test["status"], "failed",
            "{test_number} should run through cargo dispatch, not be pre-skipped: {test:#?}"
        );
        assert_eq!(
            test["skip_reason"],
            Value::Null,
            "{test_number} skip_reason must be null when dispatched: {test:#?}"
        );

        // Verify the old semantic skip constants are not present
        let report_str = report.to_string();
        assert!(
            !report_str.contains("SEMANTIC_PARTIAL_LEASE_REENTRANCY_NOT_SHIPPED"),
            "{test_number}: old T17 orchestrator skip constant must not appear: {report_str}"
        );
        assert!(
            !report_str.contains("STREAM_D_ROTATION_CONTRACT_NOT_SHIPPED"),
            "{test_number}: old T18 orchestrator skip constant must not appear: {report_str}"
        );
    }
}

/// H-B2 regression: T19 must not be skipped in default builds now that Stream I is shipped.
#[test]
fn t19_is_not_skipped_by_default_with_stream_i_feature() {
    let fake_cargo = fake_failing_cargo();
    let output = Command::new(env!("CARGO_BIN_EXE_memorum-eval"))
        .args(["--harness", "mock", "--filter", "t19", "--output", "json"])
        .env("MEMORUM_EVAL_CARGO", fake_cargo.path())
        .output()
        .expect("spawn memorum-eval");

    let report = json_stdout(output);
    assert_eq!(report["total"], 1);
    let test = &report["tests"][0];
    assert_eq!(test["number"], 19);

    // With stream-i-deps enabled by default, T19 should reach MockHarness or cargo dispatch,
    // not be pre-skipped with STREAM_I_DEPS_DISABLED.
    assert_ne!(
        test["status"], "skipped",
        "T19 must not be pre-skipped when stream-i-deps feature is enabled by default: {test:#?}"
    );
    assert_ne!(
        test["skip_reason"].as_str(),
        Some("STREAM_I_DEPS_DISABLED"),
        "T19 must not carry STREAM_I_DEPS_DISABLED skip when stream-i-deps is a default feature: {test:#?}"
    );
}

/// H-R4 regression: cargo-test success that includes a MEMORUM_EVAL_SKIP: marker
/// should be recorded as skipped, not passed.
#[test]
fn skip_marker_in_cargo_stdout_produces_skipped_result_not_pass() {
    let fake_cargo = fake_cargo_script(
        "memorum-eval-skip-cargo",
        "#!/bin/sh\nprintf 'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\\n'\nprintf 'MEMORUM_EVAL_SKIP:STREAM_G_RC_HANDLER_NOT_SHIPPED\\n'\nexit 0\n",
    );

    let output = Command::new(env!("CARGO_BIN_EXE_memorum-eval"))
        .args(["--harness", "mock", "--filter", "t16", "--output", "json"])
        .env("MEMORUM_EVAL_CARGO", fake_cargo.path())
        .output()
        .expect("spawn memorum-eval");

    // Should exit 0 (skips are not failures in mock mode)
    assert!(output.status.success(), "skip marker run should exit 0: {}", diagnostic(&output));

    let report = json_stdout(output);
    assert_eq!(report["total"], 1);
    let test = &report["tests"][0];
    assert_eq!(test["number"], 16);
    assert_eq!(test["status"], "skipped", "skip-marker stdout should produce a skipped result: {test:#?}");
    assert!(
        test["skip_reason"].as_str().is_some_and(|r| r.contains("STREAM_G_RC_HANDLER_NOT_SHIPPED")),
        "skip_reason should contain the extracted reason: {test:#?}"
    );
    assert_eq!(test["skip_kind"], "runtime_self_skip", "ordinary runtime skip markers are classified distinctly");
}

#[test]
fn deferred_feature_skip_marker_reports_feature_deferred_kind() {
    let fake_cargo = fake_cargo_script(
        "memorum-eval-deferred-cargo",
        "#!/bin/sh\nprintf 'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\\n'\nprintf 'MEMORUM_EVAL_SKIP:STREAM_D_ROTATION_CONTRACT_NOT_SHIPPED\\n'\nexit 0\n",
    );

    let output = Command::new(env!("CARGO_BIN_EXE_memorum-eval"))
        .env("MEMORUM_EVAL_CARGO", fake_cargo.path())
        .args(["--harness", "mock", "--filter", "18", "--output", "json"])
        .output()
        .expect("run memorum-eval");

    assert!(output.status.success(), "deferred feature skip run should exit 0: {}", diagnostic(&output));
    let report = json_stdout(output);
    assert_eq!(report["total"], 1);
    let test = &report["tests"][0];
    assert_eq!(test["number"], 18);
    assert_eq!(test["status"], "skipped");
    assert_eq!(test["skip_reason"], "STREAM_D_ROTATION_CONTRACT_NOT_SHIPPED");
    assert_eq!(test["skip_kind"], "feature_deferred");
}

/// H-B3 regression: cargo-test success that includes MEMORUM_EVAL_ASSERTIONS=<n>
/// should populate the assertions field accurately (not hardcoded 1).
#[test]
fn assertion_count_marker_in_cargo_stdout_populates_assertions_field() {
    let fake_cargo = fake_cargo_script(
        "memorum-eval-assert-cargo",
        "#!/bin/sh\nprintf 'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\\n'\nprintf 'MEMORUM_EVAL_ASSERTIONS=7\\n'\nexit 0\n",
    );

    let output = Command::new(env!("CARGO_BIN_EXE_memorum-eval"))
        .args(["--harness", "mock", "--filter", "t01", "--output", "json"])
        .env("MEMORUM_EVAL_CARGO", fake_cargo.path())
        .output()
        .expect("spawn memorum-eval");

    assert!(output.status.success(), "assertion-count run should exit 0: {}", diagnostic(&output));

    let report = json_stdout(output);
    assert_eq!(report["total"], 1);
    let test = &report["tests"][0];
    assert_eq!(test["number"], 1);
    assert_eq!(test["status"], "passed", "assertion-count run should pass: {test:#?}");
    assert_eq!(
        test["assertions"].as_u64(),
        Some(7),
        "assertions field should reflect the MEMORUM_EVAL_ASSERTIONS marker, not hardcoded 1: {test:#?}"
    );
    assert_eq!(test["assertions_passed"].as_u64(), Some(7), "assertions_passed should match assertions: {test:#?}");
    assert_eq!(test["assertions_failed"].as_u64(), Some(0));
}

#[test]
fn timeout_flag_stops_slow_cargo_dispatch() {
    let fake_cargo = fake_sleeping_cargo();
    let started = std::time::Instant::now();
    let output = Command::new(env!("CARGO_BIN_EXE_memorum-eval"))
        .args(["--harness", "mock", "--filter", "t01", "--timeout", "1", "--output", "json"])
        .env("MEMORUM_EVAL_CARGO", fake_cargo.path())
        .output()
        .expect("spawn memorum-eval");

    assert!(started.elapsed() < std::time::Duration::from_secs(5), "--timeout should stop the cargo child promptly");
    assert!(!output.status.success(), "timeout run should exit non-zero: {}", diagnostic(&output));

    let report = json_stdout(output);
    assert_eq!(report["total"], 1);
    assert_eq!(report["timed_out"], true);
    assert_eq!(report["failed"], 1);

    let test = &report["tests"][0];
    assert_eq!(test["number"], 1);
    assert_eq!(test["status"], "failed");
    assert_eq!(test["failure_detail"], "TIMEOUT");
}

#[test]
fn no_cleanup_flag_is_passed_to_cargo_dispatch() {
    let fake_cargo = fake_no_cleanup_checking_cargo();
    let output = Command::new(env!("CARGO_BIN_EXE_memorum-eval"))
        .args(["--harness", "mock", "--filter", "t01", "--no-cleanup", "--output", "json"])
        .env("MEMORUM_EVAL_CARGO", fake_cargo.path())
        .output()
        .expect("spawn memorum-eval");

    assert!(output.status.success(), "--no-cleanup should be visible to cargo dispatch: {}", diagnostic(&output));

    let report = json_stdout(output);
    assert_eq!(report["total"], 1);
    assert_eq!(report["failed"], 0);

    let test = &report["tests"][0];
    assert_eq!(test["number"], 1);
    assert_eq!(test["status"], "passed");
}

fn json_stdout(output: std::process::Output) -> Value {
    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf-8");
    serde_json::from_str(&stdout).unwrap_or_else(|err| panic!("stdout should be JSON: {err}\n{stdout}"))
}

fn diagnostic(output: &std::process::Output) -> String {
    format!(
        "status={}\nstdout={}\nstderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}
