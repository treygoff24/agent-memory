//! Integration coverage for the non-interactive (`agent`) frontend of
//! `memoryd init`.
//!
//! These tests drive the real `memoryd` binary so they exercise the same
//! dispatch, flag parsing, and stdout/stderr split a bootstrapping agent sees.
//! The hard invariant under test: stdout carries valid JSON and nothing else;
//! every diagnostic lands on stderr.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use memoryd::import::report::ImportReport;
use memoryd::setup::{SetupDetection, SetupReport, SetupStep, SetupStepStatus};
use serial_test::serial;

/// `init --detect-only --json` against an empty environment must produce a
/// parseable detection summary, write nothing to stderr, exit zero, and leave
/// the repo directory untouched.
#[test]
#[serial]
fn detect_only_emits_detection_json_with_zero_side_effects() {
    let env = TestEnv::new();
    let repo = env.temp.path().join("repo");

    let output = env.run_init([
        "init",
        "--detect-only",
        "--json",
        "--repo",
        path_arg(&repo),
        "--runtime",
        path_arg(&repo.join(".memoryd")),
    ]);

    assert_success(&output);
    assert!(stderr(&output).is_empty(), "detect-only must keep stderr clean:\n{}", stderr(&output));
    assert!(!repo.exists(), "detect-only must not create the repo directory");

    let detection: SetupDetection = parse_stdout(&output);
    // Empty harness roots with no candidates and an absent daemon socket.
    assert_eq!(detection.claude.candidates, 0);
    assert_eq!(detection.codex.candidates, 0);
}

/// The headline acceptance: a non-interactive import run emits a `SetupReport`
/// whose import section is byte-for-byte the same as a direct
/// `memoryd import --report`, with stdout carrying only the JSON report.
#[test]
#[serial]
fn non_interactive_import_matches_direct_import_report() {
    let env = TestEnv::new();
    env.seed_codex_fixture();
    let repo = env.temp.path().join("repo");

    let init_output = env.run_init([
        "init",
        "--non-interactive",
        "--json",
        "--import",
        "--harness",
        "all",
        "--non-git-cwd-default",
        "me",
        "--wire-mcp",
        "none",
        "--daemon",
        "none",
        "--print-only",
        "--repo",
        path_arg(&repo),
        "--runtime",
        path_arg(&repo.join(".memoryd")),
    ]);

    // `--daemon none` intentionally leaves the socket absent, so the engine's
    // verify probe fails by design; the agent path treats that as non-fatal.
    assert_success(&init_output);

    let report: SetupReport = parse_stdout(&init_output);
    let import_section = report.import_report.as_ref().expect("setup report carries an import section");

    // The import step ran and stdout is pure JSON (parse above would panic
    // otherwise). Diagnostics belong on stderr only.
    assert_step(&report, SetupStep::Import, SetupStepStatus::Succeeded);

    // Direct comparison against `memoryd import --report` over the same corpus
    // and the same disposition. Both run as dry-run / print-only so neither
    // needs a live daemon.
    let direct_report_path = env.temp.path().join("direct-import.json");
    let direct_repo = env.temp.path().join("direct-repo");
    let import_output = env.run_init([
        "import",
        "--harness",
        "all",
        "--dry-run",
        "--non-git-cwd-default",
        "me",
        "--report",
        path_arg(&direct_report_path),
        "--repo",
        path_arg(&direct_repo),
        "--quiet",
    ]);
    assert_success(&import_output);

    let direct: ImportReport = read_json(&direct_report_path);
    assert_eq!(
        canonical_json(import_section),
        canonical_json(&direct),
        "setup import section must equal a direct `memoryd import --report`"
    );

    // The fixture has exactly one Codex Task Group.
    let codex = import_section.harnesses.get("codex").expect("codex counters");
    assert_eq!(codex.parsed, 1, "fixture parses one codex memory");
    assert_eq!(codex.written_new, 1, "dry-run previews one write");
}

/// stdout must be parseable JSON even when nothing is imported, and the report
/// must record the import step as skipped.
#[test]
#[serial]
fn non_interactive_without_import_skips_and_stays_json() {
    let env = TestEnv::new();
    let repo = env.temp.path().join("repo");

    let output = env.run_init([
        "init",
        "--non-interactive",
        "--harness",
        "none",
        "--wire-mcp",
        "none",
        "--daemon",
        "none",
        "--repo",
        path_arg(&repo),
        "--runtime",
        path_arg(&repo.join(".memoryd")),
    ]);

    assert_success(&output);
    let report: SetupReport = parse_stdout(&output);
    assert_step(&report, SetupStep::Import, SetupStepStatus::Skipped);
    assert!(report.import_report.is_none(), "no import means no import section");
}

/// A non-TTY invocation with no machine flags still routes to the agent path
/// (deterministic JSON) rather than blocking on prompts.
#[test]
#[serial]
fn piped_bare_invocation_routes_to_agent_json() {
    let env = TestEnv::new();
    let repo = env.temp.path().join("repo");

    let output = env.run_init([
        "init",
        "--harness",
        "none",
        "--wire-mcp",
        "none",
        "--daemon",
        "none",
        "--repo",
        path_arg(&repo),
        "--runtime",
        path_arg(&repo.join(".memoryd")),
    ]);

    assert_success(&output);
    // Pure JSON, not the legacy advisory text.
    let _report: SetupReport = parse_stdout(&output);
}

// ---------------------------------------------------------------------------
// Test harness
// ---------------------------------------------------------------------------

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
        std::fs::create_dir_all(claude_config.join("projects")).expect("claude projects");
        std::fs::create_dir_all(codex_home.join("memories")).expect("codex memories");
        Self { temp, home, claude_config, codex_home }
    }

    /// Write a minimal one-Task-Group Codex corpus discoverable via `CODEX_HOME`.
    fn seed_codex_fixture(&self) {
        let memory_md = self.codex_home.join("memories").join("MEMORY.md");
        std::fs::write(
            &memory_md,
            "# Task Group: Atlas onboarding\n\
             \n\
             scope: how new contributors get started on AtlasOS\n\
             applies_to: cwd=/Users/u/Code/atlasos; reuse_rule=cwd-scoped\n\
             \n\
             ## Task 1: react-doctor flake\n\
             react-doctor flakes on cold start; rerun fixes it.\n\
             \n\
             ### keywords\n\
             - atlasos, onboarding\n",
        )
        .expect("write codex MEMORY.md");
    }

    /// Run the `memoryd` binary with the harness roots pinned to the fixture
    /// environment so discovery never touches the developer's real `~/.codex`
    /// or `~/.claude`.
    fn run_init<const N: usize>(&self, args: [&str; N]) -> Output {
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

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "command failed with status {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        stdout(output),
        stderr(output)
    );
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("stdout utf8")
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("stderr utf8")
}

fn path_arg(path: &Path) -> &str {
    path.to_str().expect("test paths are utf8")
}

/// Parse stdout as JSON of type `T`. Panics with the captured streams if stdout
/// is not pure, parseable JSON — this is the stdout-purity assertion.
fn parse_stdout<T: serde::de::DeserializeOwned>(output: &Output) -> T {
    let raw = stdout(output);
    serde_json::from_str(&raw).unwrap_or_else(|error| {
        panic!("stdout must be pure JSON ({error})\nstdout:\n{raw}\nstderr:\n{}", stderr(output))
    })
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> T {
    let raw = std::fs::read_to_string(path).expect("read json file");
    serde_json::from_str(&raw).expect("parse json file")
}

fn canonical_json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(&serde_json::to_value(value).expect("to value")).expect("to string")
}

fn assert_step(report: &SetupReport, step: SetupStep, status: SetupStepStatus) {
    let entry = report
        .steps
        .iter()
        .find(|entry| entry.step == step)
        .unwrap_or_else(|| panic!("setup report missing step {step:?}; steps: {:?}", report.steps));
    assert_eq!(entry.status, status, "step {step:?} status; message: {:?}", entry.message);
}
