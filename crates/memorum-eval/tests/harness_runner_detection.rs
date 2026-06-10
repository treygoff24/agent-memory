use serde_json::Value;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::sync::Mutex;

use memorum_eval::harness_runner::{HarnessRunner, RealHarness};

static PATH_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn detects_absent_real_harness_clis_without_panicking() {
    let _guard = path_lock();
    let original_path = std::env::var_os("PATH");
    let path_dir = unique_temp_tree();
    std::env::set_var("PATH", path_dir.path());

    for harness in [RealHarness::Claude, RealHarness::Codex] {
        let detected = HarnessRunner::detect_cli(harness).expect("CLI detection should not fail when binary is absent");
        assert_eq!(detected, None);
    }

    restore_path(original_path);
}

#[test]
fn detects_present_real_harness_clis_and_validates_mcp_config_flag_against_help() {
    let _guard = path_lock();
    let original_path = std::env::var_os("PATH");
    let path_dir = unique_temp_tree();
    write_fake_cli(path_dir.path(), "claude", "Claude help: --mcp-config <path>\n");
    write_fake_cli(path_dir.path(), "codex", "Codex help: --mcp-config <path>\n");
    std::env::set_var("PATH", path_dir.path());

    for harness in [RealHarness::Claude, RealHarness::Codex] {
        let cli = HarnessRunner::detect_cli(harness)
            .expect("CLI detection should succeed")
            .expect("fake CLI should be detected");
        assert_eq!(cli.path, path_dir.path().join(harness.binary_name()));
        assert_eq!(cli.mcp_config_flag, "--mcp-config");
    }

    restore_path(original_path);
}

#[test]
fn writes_harness_specific_mcp_config_files_without_extra_temp_files() {
    let sandbox_dir = unique_temp_tree();
    let sandbox = sandbox_dir.path().to_path_buf();
    let socket_path = sandbox.join("memoryd.sock");

    let claude_runner = HarnessRunner::new_with_socket(RealHarness::Claude, socket_path.clone());
    let codex_runner = HarnessRunner::new_with_socket(RealHarness::Codex, socket_path.clone());

    let claude_config =
        claude_runner.write_mcp_config_file(&sandbox, "run-claude").expect("claude mcp config should be written");
    let codex_config =
        codex_runner.write_mcp_config_file(&sandbox, "run-codex").expect("codex mcp config should be written");

    assert_eq!(claude_config, sandbox.join(".harness-mcp/claude-run-claude.json"));
    assert_eq!(codex_config, sandbox.join(".harness-mcp/codex-run-codex.toml"));

    let claude_body = fs::read_to_string(&claude_config).expect("claude config should be readable");
    let claude_json: Value = serde_json::from_str(&claude_body).expect("claude config should be valid json");
    let claude_server = &claude_json["mcpServers"]["memorum_eval"];
    assert_eq!(claude_server["command"], "memoryd");
    assert_eq!(claude_server["args"], serde_json::json!(["mcp", "--socket", socket_path.to_string_lossy()]));

    let codex_body = fs::read_to_string(&codex_config).expect("codex config should be readable");
    assert!(codex_body.contains("[mcp.memorum_eval]"));
    assert!(codex_body.contains("command = \"memoryd\""));
    assert!(codex_body.contains(&format!("args = [\"mcp\", \"--socket\", \"{}\"]", socket_path.display())));

    let mut entries = fs::read_dir(sandbox.join(".harness-mcp"))
        .expect("mcp config directory should exist")
        .map(|entry| entry.expect("directory entry should be readable").path())
        .collect::<Vec<_>>();
    entries.sort();
    assert_eq!(entries, vec![claude_config, codex_config]);
}

fn write_fake_cli(path_dir: &std::path::Path, name: &str, help_text: &str) {
    let path = path_dir.join(name);
    fs::write(&path, format!("#!/bin/sh\necho {}\n", shell_quote(help_text.trim_end())))
        .expect("fake CLI should be written");
    let mut permissions = fs::metadata(&path).expect("fake CLI metadata should be readable").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).expect("fake CLI should be executable");
    let output = std::process::Command::new(&path).arg("--help").output().expect("fake CLI should execute");
    let help = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert!(help.contains("--mcp-config"), "fake {name} help fixture should include --mcp-config: {help:?}");
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn path_lock() -> std::sync::MutexGuard<'static, ()> {
    PATH_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn restore_path(original_path: Option<std::ffi::OsString>) {
    if let Some(path) = original_path {
        std::env::set_var("PATH", path);
    } else {
        std::env::remove_var("PATH");
    }
}

/// A genuinely unique, auto-cleaned sandbox per test. The previous
/// nanos-since-epoch naming collided when two tests started within one macOS
/// clock tick — the test that finished first `remove_dir_all`'d the shared dir
/// out from under the other mid-exec.
fn unique_temp_tree() -> tempfile::TempDir {
    tempfile::Builder::new().prefix("memorum-eval-harness-runner-").tempdir().expect("test sandbox should be created")
}
