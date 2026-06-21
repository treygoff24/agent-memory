//! End-to-end acceptance for the agent-driven `memoryd init` onboarding flow.
//!
//! Unlike `cli_init_agent.rs` (which exercises the JSON contract with
//! `--daemon none` and `--print-only`, so it never touches a live daemon or a
//! real governance write), this test drives the *whole* onboarding spine
//! against real effects:
//!
//!   memoryd init --non-interactive --json --import \
//!     --harness all --wire-mcp none --daemon background \
//!     --repo <tmp> --runtime <tmp/.memoryd>
//!
//! `--daemon background` spawns a real child `memoryd serve` process bound to
//! the runtime socket; the importer writes through that live daemon into an
//! on-disk substrate, passing through the real governance engine. Nothing is
//! mocked. The test then:
//!
//!   - validates the `SetupReport` JSON shape, including `restart_required`;
//!   - confirms a real daemon bound the socket and the repo was initialized;
//!   - runs the `Doctor` probe in-process against the real substrate and
//!     asserts it reports clean (mirroring the engine's own verify probe);
//!   - inspects the on-disk repo and the substrate index to confirm exactly
//!     what the import landed;
//!   - asserts stdout is pure, parseable JSON with diagnostics on stderr only;
//!   - re-runs `init` and asserts the second run is idempotent — it neither
//!     corrupts the substrate nor double-imports the same sources.
//!
//! Import outcome (asserted against the real governance engine):
//! Through the real daemon, both imported memories *land* in the substrate as
//! governance candidates. The importer tags writes `source_kind = "import"`,
//! which the governance handler maps to a grounding source of kind
//! `AgentPrimary`. That source kind grounds only when its `source_ref` resolves
//! to a local file via `FileSourceResolver`, which requires a `file:`-prefixed
//! absolute path. The importer emits exactly that via
//! `import::pipeline::groundable_source_ref`, so the built-in `*-strict`
//! policies (all `requires_grounding = true`) accept the write. Setup also
//! provisions the local privacy key, so the writes are not refused for privacy.
//!
//! This test asserts that landing-as-candidate behavior end-to-end: both
//! fixture memories land on disk and become queryable, with zero grounding or
//! privacy refusals, and a re-run skips them idempotently. The daemon
//! lifecycle, doctor, stdout-purity, report-shape, and idempotency assertions
//! all hold against the real effects. See [`FIXTURE_MEMORY_COUNT`].
//!
//! Determinism and offline guarantees: `--wire-mcp none` means no harness CLI is
//! invoked; there is no network; every path is an ephemeral `/tmp` directory
//! (kept short so the Unix-domain socket stays under the macOS UDS path cap);
//! and the spawned daemon is reliably reaped by a teardown guard that runs even
//! when an assertion panics.

mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use memory_substrate::{MemoryQuery, Roots, Substrate};
use memoryd::handlers::handle_request;
use memoryd::import::report::HarnessCounters;
use memoryd::protocol::{RequestEnvelope, RequestPayload, ResponsePayload, ResponseResult};
use memoryd::setup::{SetupReport, SetupStep, SetupStepStatus};
use memoryd::socket::{probe_live_socket, SocketProbe};
use serial_test::serial;

use common::{assert_step, assert_success, parse_stdout, path_arg};

/// Number of importable memories the fixture corpus contains: one Claude topic
/// file plus one Codex Task Group. The importer emits a governance-groundable
/// `file:`-prefixed `source_ref` (`import::pipeline::groundable_source_ref`), so
/// both fixture memories pass the `*-strict` grounding policies, land on disk,
/// and become queryable; a re-run skips them idempotently.
const FIXTURE_MEMORY_COUNT: usize = 2;

/// Headline acceptance: a real background-daemon onboarding run brings up a live
/// daemon, initializes the substrate, emits a well-formed `SetupReport`, passes
/// an in-process doctor check, keeps stdout pure JSON, and is safe to re-run —
/// with the import outcome asserted against the real governance engine.
#[test]
#[serial]
fn background_onboarding_is_clean_idempotent_and_governance_truthful() {
    let env = TestEnv::new("e2e-main");
    env.seed_claude_fixture();
    env.seed_codex_fixture();

    // First run: full onboarding against a live daemon.
    let first = env.run_init_background();
    assert_success(&first);

    let report: SetupReport = parse_stdout(&first);
    // A background-daemon guard kills the spawned child even if asserts panic.
    let daemon = DaemonGuard::from_report(&report);

    assert_report_shape(&report);

    // EnsureRepo / EnsureDaemon succeed; Import runs and *succeeds as a step*
    // even when individual writes are refused (a refusal is a recorded outcome,
    // not a step failure); WireMcp and WireHooks are skipped (`--wire-mcp none`
    // / `--wire-hooks none`); Verify succeeds because the daemon is live and the
    // substrate is healthy.
    assert_step(&report, SetupStep::EnsureRepo, SetupStepStatus::Succeeded);
    assert_step(&report, SetupStep::EnsureDaemon, SetupStepStatus::Succeeded);
    assert_step(&report, SetupStep::Import, SetupStepStatus::Succeeded);
    assert_step(&report, SetupStep::WireMcp, SetupStepStatus::Skipped);
    assert_step(&report, SetupStep::WireHooks, SetupStepStatus::Skipped);
    assert_step(&report, SetupStep::Verify, SetupStepStatus::Succeeded);

    // A real daemon must be bound to the reported socket right now.
    let socket = report.detection.daemon.socket_path.clone();
    assert!(
        matches!(probe_live_socket(&socket), SocketProbe::Live),
        "background daemon must be live at {}",
        socket.display()
    );

    // Both fixture sources parse regardless of whether they land.
    let import = report.import_report.as_ref().expect("background import carries an import section");
    let claude = import.harnesses.get("claude-code").expect("claude-code counters");
    let codex = import.harnesses.get("codex").expect("codex counters");
    assert_eq!(claude.parsed, 1, "fixture parses one claude memory");
    assert_eq!(codex.parsed, 1, "fixture parses one codex memory");

    // The import *disposition* is the governance-truthful assertion.
    assert_first_run_disposition(claude, codex);

    // On-disk + in-process substrate inspection.
    // Stop the daemon first so the in-process opener owns the substrate cleanly
    // and the doctor probe mirrors exactly what the engine's verify step does
    // (open Substrate, dispatch a Doctor request in-process).
    drop(daemon);
    wait_for_socket_gone(&socket);

    let expected_landed = FIXTURE_MEMORY_COUNT;

    // Raw on-disk proof: count canonical memory `.md` files under the repo tree.
    let canonical = count_canonical_memory_files(&env.repo, &env.runtime);
    assert_eq!(canonical, expected_landed, "canonical memory files on disk under {}", env.repo.display());

    // Index proof: a default substrate query returns exactly the landed memories.
    let landed = open_and_count_memories(&env.repo, &env.runtime);
    assert_eq!(landed, expected_landed, "queryable memories in the substrate index");

    // The substrate is healthy whether or not anything landed: EnsureRepo built
    // a valid repo and the daemon left it clean.
    assert_doctor_clean(&env.repo, &env.runtime);

    // Idempotent re-run.
    // A second onboarding over the unchanged corpus must not corrupt the repo
    // and must not double-import.
    let second = env.run_init_background();
    assert_success(&second);
    let report2: SetupReport = parse_stdout(&second);
    let daemon2 = DaemonGuard::from_report(&report2);

    assert_report_shape(&report2);
    assert_step(&report2, SetupStep::EnsureRepo, SetupStepStatus::Succeeded);
    assert_step(&report2, SetupStep::Import, SetupStepStatus::Succeeded);
    assert_step(&report2, SetupStep::Verify, SetupStepStatus::Succeeded);

    let import2 = report2.import_report.as_ref().expect("re-run carries an import section");
    let claude2 = import2.harnesses.get("claude-code").expect("claude-code counters (re-run)");
    let codex2 = import2.harnesses.get("codex").expect("codex counters (re-run)");
    // No write is ever re-issued for an already-landed source, and no refused
    // source is laundered into a write on the second pass.
    assert_eq!(claude2.written_new, 0, "re-run must not write the claude memory anew");
    assert_eq!(codex2.written_new, 0, "re-run must not write the codex memory anew");
    assert_second_run_disposition(claude2, codex2);

    let socket2 = report2.detection.daemon.socket_path.clone();
    drop(daemon2);
    wait_for_socket_gone(&socket2);

    // The substrate holds exactly what the first run landed — no doubling, no
    // corruption — and stays healthy.
    let landed_after = open_and_count_memories(&env.repo, &env.runtime);
    assert_eq!(landed_after, expected_landed, "idempotent re-run must not change the landed memory count");
    assert_doctor_clean(&env.repo, &env.runtime);
}

/// Assert the first-run import disposition for each fixture harness against the
/// real governance engine: both fixture memories land as governance candidates
/// with zero grounding/privacy refusals.
fn assert_first_run_disposition(claude: &HarnessCounters, codex: &HarnessCounters) {
    // Imports land as governance *candidates* (confidence 0.7, above the Reality
    // Check review threshold but below hand-written memories), not as
    // directly-promoted `written_new`. See import::pipeline::build_write_meta.
    assert_eq!(claude.written_candidate, 1, "claude memory written as candidate");
    assert_eq!(codex.written_candidate, 1, "codex memory written as candidate");
    assert_eq!(claude.written_new, 0, "imports are candidates, not direct promotions");
    assert_eq!(codex.written_new, 0, "imports are candidates, not direct promotions");
    assert_eq!(claude.refused_grounding, 0, "no grounding refusal: the importer grounds writes via file: source_ref");
    assert_eq!(codex.refused_grounding, 0, "no grounding refusal: the importer grounds writes via file: source_ref");
    assert_eq!(claude.refused_privacy, 0, "no privacy refusal: setup provisions the privacy key");
    assert_eq!(codex.refused_privacy, 0, "no privacy refusal: setup provisions the privacy key");
}

/// Assert the second-run (idempotent) import disposition: both sources already
/// landed, so the importer's state file marks them seen and the re-run skips
/// them as idempotent.
fn assert_second_run_disposition(claude: &HarnessCounters, codex: &HarnessCounters) {
    assert_eq!(
        claude.skipped_idempotent + codex.skipped_idempotent,
        FIXTURE_MEMORY_COUNT,
        "re-run must skip both prior sources as idempotent"
    );
}

/// Validate the `SetupReport` envelope independent of step outcomes. Locks the
/// schema version, the presence of every expected step exactly once, the
/// `restart_required` flag (must be `false` whenever no MCP config was rewritten
/// under `--wire-mcp none`), and the live-path `Verify` per-probe breakdown.
fn assert_report_shape(report: &SetupReport) {
    assert_eq!(report.schema_version, 2, "setup report schema version");

    for step in [
        SetupStep::EnsureRepo,
        SetupStep::EnsureDaemon,
        SetupStep::Import,
        SetupStep::WireMcp,
        SetupStep::WireHooks,
        SetupStep::Verify,
    ] {
        let count = report.steps.iter().filter(|entry| entry.step == step).count();
        assert_eq!(count, 1, "step {step:?} must appear exactly once; steps: {:?}", report.steps);
    }

    // `--wire-mcp none` rewrote no harness config, so no restart is required.
    assert!(!report.restart_required, "restart_required must be false when no MCP config was wired");

    // The Verify step carries a per-probe breakdown on the live path; both
    // probes succeed against a live daemon and a healthy substrate.
    let verify = report.steps.iter().find(|entry| entry.step == SetupStep::Verify).expect("verify step present");
    let detail = verify.verify.as_ref().expect("verify step carries a per-probe breakdown");
    assert_eq!(detail.status_probe, SetupStepStatus::Succeeded, "live daemon => status probe succeeds");
    assert_eq!(detail.doctor_probe, SetupStepStatus::Succeeded, "healthy substrate => doctor probe succeeds");
}

/// Count canonical memory `.md` files written under the repo tree, skipping the
/// runtime directory (nested under the repo here) and git internals. This is the
/// raw on-disk proof of what the import produced, independent of any index query.
fn count_canonical_memory_files(repo: &Path, runtime: &Path) -> usize {
    let mut count = 0;
    let mut stack = vec![repo.to_path_buf()];
    while let Some(dir) = stack.pop() {
        // Never walk into the runtime dir or git internals: those are not
        // canonical memory content.
        if dir == runtime || dir.file_name().is_some_and(|name| name == ".git") {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "md") {
                count += 1;
            }
        }
    }
    count
}

/// Open the substrate read-side in-process and count every active memory via a
/// default (unfiltered) query. Proves what the import landed in the index.
fn open_and_count_memories(repo: &Path, runtime: &Path) -> usize {
    block_on(async {
        let substrate = Substrate::open(Roots::new(repo.to_path_buf(), runtime.to_path_buf()))
            .await
            .expect("open substrate for in-process query");
        let memories = substrate.query_memory(MemoryQuery::default()).await.expect("query memories");
        memories.len()
    })
}

/// Run the in-process doctor probe exactly as the engine's `verify` step does:
/// open the substrate and dispatch a `Doctor` request through `handle_request`.
/// Asserts the *substrate* is clean: the doctor responds over the transport and
/// reports zero substrate-level findings.
///
/// We deliberately scope "clean" to the substrate and ignore harness-CLI
/// findings. The doctor also probes the local `claude` / `codex` CLIs and emits
/// a `harness_cli_warning` (and flips `DoctorResponse::healthy` to `false`) when
/// no harness CLI is authenticated. That signal is environment-dependent — it
/// depends on whether the developer's machine has an authenticated harness on
/// PATH — and is orthogonal to whether onboarding produced a healthy substrate.
/// Asserting on it would make this test flaky across machines and CI. So we
/// assert only the substrate-level finding codes the onboarding flow actually
/// governs: `warning`, `repair_required`, and `events_log_mirror_lag`. (This
/// mirrors the daemon's own `doctor_is_healthy`, which derives substrate health
/// from exactly these codes before folding in harness availability.)
fn assert_doctor_clean(repo: &Path, runtime: &Path) {
    const SUBSTRATE_FINDING_CODES: [&str; 3] = ["warning", "repair_required", "events_log_mirror_lag"];
    block_on(async {
        let substrate = Substrate::open(Roots::new(repo.to_path_buf(), runtime.to_path_buf()))
            .await
            .expect("open substrate for doctor probe");
        let response =
            handle_request(&substrate, RequestEnvelope::new("setup-e2e-doctor", RequestPayload::Doctor)).await;
        match response.result {
            ResponseResult::Success(ResponsePayload::Doctor(doctor)) => {
                let substrate_findings: Vec<_> = doctor
                    .findings
                    .iter()
                    .filter(|finding| SUBSTRATE_FINDING_CODES.contains(&finding.code.as_str()))
                    .collect();
                assert!(
                    substrate_findings.is_empty(),
                    "onboarding must leave a clean substrate; substrate-level findings: {substrate_findings:?}"
                );
            }
            ResponseResult::Success(other) => panic!("doctor returned an unexpected payload: {other:?}"),
            ResponseResult::Error(error) => panic!("doctor returned an error: {} {}", error.code, error.message),
        }
    });
}

/// Drive a future to completion on a fresh single-threaded runtime. Integration
/// tests here are `#[test]` (not `#[tokio::test]`) so the binary subprocess and
/// the in-process probes stay decoupled; this helper bridges the few async
/// substrate calls.
fn block_on<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread().enable_all().build().expect("build tokio runtime").block_on(future)
}

/// RAII guard that reaps the background daemon spawned by `memoryd init --daemon
/// background`. The daemon is a detached grandchild of this test (the `init`
/// subprocess spawns it and exits), so it is not reachable via a `Child` handle.
/// We parse its pid out of the `EnsureDaemon` step message and kill it on drop,
/// which runs even when an assertion panics — no orphaned daemons, no leaked
/// sockets.
struct DaemonGuard {
    pid: Option<u32>,
    socket: PathBuf,
}

impl DaemonGuard {
    /// Extract the daemon pid from the report's `EnsureDaemon` message, which
    /// the engine formats as `started background daemon pid <pid> at <socket>`.
    fn from_report(report: &SetupReport) -> Self {
        let socket = report.detection.daemon.socket_path.clone();
        let pid = report
            .steps
            .iter()
            .find(|entry| entry.step == SetupStep::EnsureDaemon)
            .and_then(|entry| entry.message.as_deref())
            .and_then(parse_daemon_pid);
        Self { pid, socket }
    }
}

impl Drop for DaemonGuard {
    fn drop(&mut self) {
        if let Some(pid) = self.pid {
            // SIGKILL the daemon directly; it has no children of its own to
            // orphan. Ignore errors (already exited, etc.).
            let _ = Command::new("kill").arg("-KILL").arg(pid.to_string()).status();
        }
        // Best-effort socket removal so a later run starts from a clean slate.
        let _ = std::fs::remove_file(&self.socket);
    }
}

/// Pull the integer pid out of `"...started background daemon pid 12345 at ..."`.
fn parse_daemon_pid(message: &str) -> Option<u32> {
    let after = message.split("pid ").nth(1)?;
    let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// Poll until the daemon socket is no longer live (up to 5s). Gives the killed
/// daemon time to release the socket before the in-process opener takes over.
fn wait_for_socket_gone(socket: &Path) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if !matches!(probe_live_socket(socket), SocketProbe::Live) {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    // Not fatal on its own — the in-process opener may still succeed — but the
    // follow-on substrate query/doctor would surface any real contention.
}

/// Ephemeral environment for one end-to-end run.
///
/// Everything lives under a short `/tmp/memd-<prefix>-<pid>-<nonce>/` directory
/// so the runtime's `memoryd.sock` path stays under the macOS Unix-domain socket
/// length cap (a `tempfile::tempdir()` under `/var/folders/...` would blow past
/// it). The directory is removed on drop.
struct TestEnv {
    base: PathBuf,
    repo: PathBuf,
    runtime: PathBuf,
    home: PathBuf,
    claude_config: PathBuf,
    codex_home: PathBuf,
}

impl TestEnv {
    fn new(prefix: &str) -> Self {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).expect("clock after epoch").as_nanos();
        let base = PathBuf::from(format!("/tmp/memd-{prefix}-{}-{nonce}", std::process::id()));
        let repo = base.join("repo");
        let runtime = repo.join(".memoryd");
        let home = base.join("home");
        let claude_config = base.join("claude-config");
        let codex_home = base.join("codex-home");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::create_dir_all(claude_config.join("projects")).expect("claude projects");
        std::fs::create_dir_all(codex_home.join("memories")).expect("codex memories");
        Self { base, repo, runtime, home, claude_config, codex_home }
    }

    /// Seed a one-topic Claude corpus discoverable via `CLAUDE_CONFIG_DIR`. The
    /// importer scans `<CLAUDE_CONFIG_DIR>/projects/`; a `memory/*.md` topic file
    /// under a project directory parses to one importable memory.
    fn seed_claude_fixture(&self) {
        let memory_dir = self.claude_config.join("projects").join("atlasos").join("memory");
        std::fs::create_dir_all(&memory_dir).expect("claude memory dir");
        std::fs::write(
            memory_dir.join("build_commands.md"),
            "---\nname: Build commands\n---\nUse `cargo build --release` for prod builds.\n",
        )
        .expect("write claude topic file");
    }

    /// Seed a one-Task-Group Codex corpus discoverable via `CODEX_HOME`. Mirrors
    /// the fixture in `cli_init_agent.rs`.
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

    /// Run `memoryd init` in the full background-daemon onboarding mode against
    /// this environment's fixtures.
    fn run_init_background(&self) -> Output {
        Command::new(env!("CARGO_BIN_EXE_memoryd"))
            .args([
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
                "--wire-hooks",
                "none",
                "--daemon",
                "background",
                "--repo",
                path_arg(&self.repo),
                "--runtime",
                path_arg(&self.runtime),
            ])
            .env("HOME", &self.home)
            .env("CLAUDE_CONFIG_DIR", &self.claude_config)
            .env("CODEX_HOME", &self.codex_home)
            .env("GIT_AUTHOR_NAME", "Memorum Test")
            .env("GIT_AUTHOR_EMAIL", "memorum-test@example.invalid")
            .env("GIT_COMMITTER_NAME", "Memorum Test")
            .env("GIT_COMMITTER_EMAIL", "memorum-test@example.invalid")
            .env_remove("MEMORUM_REPO")
            .env_remove("MEMORUM_RUNTIME")
            .output()
            .expect("run memoryd init")
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

// stdout/stderr + JSON helpers (`assert_success`, `stdout`, `stderr`,
// `path_arg`, `parse_stdout`, `assert_step`) live in `tests/common/mod.rs` and
// are shared with `cli_init_agent.rs`.
