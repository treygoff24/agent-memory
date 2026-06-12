//! Integration coverage for `memoryd uninstall`.
//!
//! These tests drive the real `memoryd` binary so they exercise the same
//! dispatch, flag parsing, and stdout/stderr split a teardown agent sees. The
//! hard invariants under test: stdout carries valid JSON and nothing else;
//! `--print-only` mutates nothing; unwiring removes only the `memorum`/`memoryd`
//! entry (user and project scope) and preserves everything else; `--purge`
//! refusal without the flag. Env is fully isolated — these never touch the real
//! home dir, Claude config, or Codex config.

mod common;

use std::path::PathBuf;
use std::process::{Command, Output};

use common::{assert_success, stderr, stdout};
use serde_json::Value;
use serial_test::serial;

/// `uninstall --print-only` against a config that holds a `memorum` entry must
/// emit a parseable report, leave the config byte-for-byte untouched, and report
/// the unwire step as `expected` (dry-run).
#[test]
#[serial]
fn print_only_mutates_nothing() {
    let env = TestEnv::new();
    let claude_json = env.write_claude_config(CLAUDE_WITH_MEMORUM);
    let before = std::fs::read_to_string(&claude_json).expect("read claude config");

    let output = env.run(["uninstall", "--print-only", "--harness", "claude"]);
    assert_success(&output);

    let report: Value = parse(&output);
    assert_eq!(report["schema_version"], 1);
    let unwire = find_step(&report, "unwire_claude").expect("unwire_claude step present");
    assert_eq!(unwire["status"], "expected", "print-only must not apply the unwire");

    let after = std::fs::read_to_string(&claude_json).expect("read claude config");
    assert_eq!(before, after, "print-only must not modify the config");
}

/// Applying the unwire removes the `memorum`/`memoryd` entry at both the Claude
/// user scope and a project scope, while preserving sibling servers, unrelated
/// projects, and other top-level fields.
#[test]
#[serial]
fn unwire_removes_only_memorum_entries_preserving_others() {
    let env = TestEnv::new();
    let claude_json = env.write_claude_config(CLAUDE_WITH_MEMORUM);

    let output = env.run(["uninstall", "--non-interactive", "--json", "--harness", "claude"]);
    assert_success(&output);

    let report: Value = parse(&output);
    let unwire = find_step(&report, "unwire_claude").expect("unwire_claude step present");
    assert_eq!(unwire["status"], "succeeded");

    let after: Value = serde_json::from_str(&std::fs::read_to_string(&claude_json).expect("read")).expect("json");
    // User scope: memorum gone, sibling preserved.
    let user = &after["mcpServers"];
    assert!(user.get("memorum").is_none(), "user-scope memorum must be removed");
    assert!(user.get("other").is_some(), "sibling server must survive");
    // Project /a: memorum gone (mcpServers dropped when empty), allowedTools kept.
    let project_a = &after["projects"]["/a"];
    assert!(project_a.get("mcpServers").is_none(), "empty project mcpServers should be dropped");
    assert_eq!(project_a["allowedTools"][0], "read");
    // Unrelated top-level field preserved.
    assert_eq!(after["model"], "claude-opus");
}

/// A `memorum`-named entry not commanded by `memoryd` is left untouched, and the
/// step reports `skipped`.
#[test]
#[serial]
fn unwire_leaves_foreign_memorum_entry_untouched() {
    let env = TestEnv::new();
    let claude_json =
        env.write_claude_config(r#"{ "mcpServers": { "memorum": { "command": "some-other-bin", "args": [] } } }"#);
    let before = std::fs::read_to_string(&claude_json).expect("read");

    let output = env.run(["uninstall", "--non-interactive", "--json", "--harness", "claude"]);
    assert_success(&output);

    let report: Value = parse(&output);
    let unwire = find_step(&report, "unwire_claude").expect("unwire_claude step present");
    assert_eq!(unwire["status"], "skipped", "a non-memoryd memorum entry is not ours to remove");

    let after = std::fs::read_to_string(&claude_json).expect("read");
    assert_eq!(before, after);
}

/// Without `--purge`, the purge step is `skipped` with the documented message
/// and the data is preserved. The full report shape is asserted here too.
#[test]
#[serial]
fn purge_is_refused_without_flag() {
    let env = TestEnv::new();
    let repo = env.temp.path().join("repo");
    std::fs::create_dir_all(repo.join(".memorum")).expect("memorum-shaped repo");

    let output =
        env.run(["uninstall", "--non-interactive", "--json", "--harness", "none", "--repo", repo.to_str().unwrap()]);
    assert_success(&output);

    let report: Value = parse(&output);
    // Report shape: schema_version + ordered steps with status.
    assert_eq!(report["schema_version"], 1);
    let purge = find_step(&report, "purge_data").expect("purge_data step present");
    assert_eq!(purge["status"], "skipped");
    assert_eq!(purge["message"], "data preserved; pass --purge to delete");
    assert!(repo.exists(), "data must be preserved without --purge");

    // Every documented step name is present.
    for step in ["detect", "stop_daemon", "remove_launchd", "purge_data", "verify"] {
        assert!(find_step(&report, step).is_some(), "missing step {step}");
    }
}

/// A non-TTY invocation with no machine mode must refuse with guidance and write
/// nothing to stdout — mirroring `init`.
#[test]
#[serial]
fn piped_invocation_without_machine_mode_refuses() {
    let env = TestEnv::new();
    let output = env.run(["uninstall", "--harness", "none"]);
    assert!(!output.status.success(), "non-TTY uninstall without a machine mode must fail");
    assert!(stdout(&output).trim().is_empty(), "refusal must not write stdout: {}", stdout(&output));
    let err = stderr(&output);
    assert!(err.contains("--print-only"), "refusal must point at the dry-run path: {err}");
    assert!(err.contains("--non-interactive"), "refusal must point at the scripted path: {err}");
}

const CLAUDE_WITH_MEMORUM: &str = r#"{
  "model": "claude-opus",
  "mcpServers": {
    "memorum": { "command": "memoryd", "args": ["mcp", "--socket", "/x"] },
    "other": { "command": "other-bin", "args": [] }
  },
  "projects": {
    "/a": {
      "mcpServers": { "memorum": { "command": "memoryd", "args": ["mcp"] } },
      "allowedTools": ["read"]
    }
  }
}"#;

struct TestEnv {
    temp: tempfile::TempDir,
    home: PathBuf,
    claude_config: PathBuf,
    codex_home: PathBuf,
}

impl TestEnv {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = temp.path().join("home");
        let claude_config = temp.path().join("claude-config");
        let codex_home = temp.path().join("codex-home");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::create_dir_all(&claude_config).expect("claude config dir");
        std::fs::create_dir_all(&codex_home).expect("codex home");
        Self { temp, home, claude_config, codex_home }
    }

    /// Write `$CLAUDE_CONFIG_DIR/.claude.json` and return its path.
    fn write_claude_config(&self, body: &str) -> PathBuf {
        let path = self.claude_config.join(".claude.json");
        std::fs::write(&path, body).expect("write claude config");
        path
    }

    fn run<const N: usize>(&self, args: [&str; N]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_memoryd"))
            .args(args)
            .env("HOME", &self.home)
            .env("CLAUDE_CONFIG_DIR", &self.claude_config)
            .env("CODEX_HOME", &self.codex_home)
            .env_remove("MEMORUM_REPO")
            .output()
            .expect("run memoryd")
    }
}

fn parse(output: &Output) -> Value {
    let raw = stdout(output);
    serde_json::from_str(&raw).unwrap_or_else(|error| {
        panic!("stdout must be pure JSON ({error})\nstdout:\n{raw}\nstderr:\n{}", stderr(output))
    })
}

fn find_step<'a>(report: &'a Value, name: &str) -> Option<&'a Value> {
    report["steps"].as_array()?.iter().find(|step| step["step"] == name)
}
