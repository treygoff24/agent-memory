use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

use serde_json::Value;

#[test]
fn list_outputs_all_19_catalog_entries() {
    let output = memorum_eval(["--list"]);

    assert!(output.status.success(), "expected --list to exit 0: {}", diagnostic(&output));

    let stdout = String::from_utf8(output.stdout).expect("--list stdout should be utf-8");
    let entry_count = stdout.lines().filter(|line| line.starts_with('#')).count();
    assert_eq!(entry_count, 19, "catalog output should list all Stream H tests:\n{stdout}");
}

#[test]
fn filtered_json_run_reports_spec_result_fields() {
    let output = memorum_eval(["--filter", "t01", "--output", "json"]);

    assert!(output.status.success(), "expected filtered run to exit 0: {}", diagnostic(&output));

    let report = json_stdout(output);
    assert_eq!(report["total"], 1);
    assert_eq!(report["failed"], 0);
    assert_eq!(report["partial"], false);
    assert_eq!(report["missing_credentials"].as_array().expect("missing_credentials should be an array").len(), 0);

    let test = &report["tests"][0];
    assert_eq!(test["number"], 1);
    assert_eq!(test["name"], "exact_identifier_recall");
    assert!(
        matches!(test["status"].as_str(), Some("passed" | "failed")),
        "status should be present and terminal: {test:#?}"
    );
    assert!(test.get("failure_detail").is_some(), "failure_detail field should always be present: {test:#?}");
}

#[test]
fn mock_harness_skips_real_harness_tests_without_counting_failures() {
    let output = memorum_eval(["--harness", "mock", "--output", "json"]);

    assert!(output.status.success(), "expected partial mock run to exit 0: {}", diagnostic(&output));

    let report = json_stdout(output);
    assert_eq!(report["total"], 19);
    assert_eq!(report["failed"], 0);
    assert_eq!(report["partial"], true);

    let tests = report["tests"].as_array().expect("tests should be an array");
    let t13 = find_test(tests, 13);
    let t15 = find_test(tests, 15);
    assert_eq!(t13["status"], "passed", "mock mode should execute MockHarness test #13: {t13:#?}");
    assert_eq!(t15["status"], "passed", "mock mode should execute MockHarness test #15: {t15:#?}");

    let skipped_real_harness_tests = skipped_real_harness_tests(tests);

    #[cfg(not(feature = "stream-i-deps"))]
    {
        assert_eq!(skipped_real_harness_tests.len(), 1, "mock mode should skip #19 until Stream I deps land");
        for test in skipped_real_harness_tests {
            assert_eq!(test["status"], "skipped");
            assert_eq!(test["skip_reason"], "STREAM_I_DEPS_DISABLED");
            assert_eq!(test["failure_detail"], Value::Null);
        }
    }

    #[cfg(feature = "stream-i-deps")]
    {
        assert!(
            skipped_real_harness_tests.is_empty(),
            "mock mode should execute #19 when Stream I deps are enabled: {skipped_real_harness_tests:#?}"
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
        .env("MEMORUM_EVAL_CARGO", &fake_cargo)
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

    for test_number in ["t13", "t15"] {
        let output = Command::new(env!("CARGO_BIN_EXE_memorum-eval"))
            .args(["--harness", "all", "--filter", test_number, "--output", "json"])
            .env("MEMORUM_EVAL_CARGO", &fake_cargo)
            .env("MEMORUM_EVAL_CLAUDE_KEY", "fake-present-claude-key")
            .env("MEMORUM_EVAL_CODEX_KEY", "fake-present-codex-key")
            .env("PATH", &fake_harness_path)
            .output()
            .expect("spawn memorum-eval");

        assert!(!output.status.success(), "fake cargo should make dispatched real-harness run fail");

        let report = json_stdout(output);
        assert_eq!(report["total"], 1);
        assert_eq!(report["missing_credentials"].as_array().expect("missing_credentials array").len(), 0);

        let test = &report["tests"][0];
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
        .env("MEMORUM_EVAL_CARGO", &fake_cargo)
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
    let output = memorum_eval([
        "--filter",
        "t01",
        "--output",
        "json",
        "--output-file",
        output_path.to_str().expect("temp path should be utf-8"),
    ]);

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

fn fake_failing_cargo() -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("memorum-eval-fake-cargo-{}.sh", std::process::id()));
    fs::write(&path, "#!/bin/sh\nprintf 'fake cargo failure for args: %s\\n' \"$*\" >&2\nexit 42\n")
        .expect("write fake cargo");
    let mut permissions = fs::metadata(&path).expect("fake cargo metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).expect("chmod fake cargo");
    path
}

fn fake_harness_path() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("memorum-eval-fake-harnesses-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("create fake harness dir");
    write_fake_harness(&dir, "claude");
    write_fake_harness(&dir, "codex");
    dir
}

fn write_fake_harness(dir: &std::path::Path, name: &str) {
    let path = dir.join(name);
    fs::write(&path, "#!/bin/sh\nif [ \"$1\" = \"--help\" ]; then echo '--mcp-config'; exit 0; fi\nexit 42\n")
        .expect("write fake harness");
    let mut permissions = fs::metadata(&path).expect("fake harness metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).expect("chmod fake harness");
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
