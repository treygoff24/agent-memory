//! `memoryd uninstall` — the clean reverse of `memoryd init` /
//! `scripts/install-memorum.sh`.
//!
//! Steps, each reported like init's `SetupReport` (`schema_version`, `steps[]`
//! with `succeeded`/`failed`/`skipped`/`expected` status and a message):
//!
//! 1. `detect` — resolve repo/runtime (same default resolution as init), probe
//!    socket liveness, find which harness configs hold a `memorum` MCP entry,
//!    and whether the `com.memorum.*` launchd plists are present.
//! 2. `stop_daemon` — SIGTERM the pid from `<runtime>/memoryd.pid`, or ask a
//!    live pid-file-less daemon for its pid via `Status`; wait briefly and
//!    verify exit. `skipped` if nothing is running.
//! 3. `remove_launchd` — `launchctl bootout` + delete the `com.memorum.daemon`
//!    and `com.memorum.dream-scheduled` plists. macOS-only; `skipped` elsewhere.
//! 4. `unwire_mcp` — remove only the `memorum`/`memoryd` MCP entry from the
//!    selected harness configs, at both Claude user scope and project scope.
//! 5. `purge_data` — only with `--purge`: delete the repo and runtime dirs after
//!    a Memorum-shape safety check and (on a TTY) typed confirmation.
//! 6. `verify` — confirm socket gone, plists gone, configs clean; report any
//!    leftover binaries with the `cargo uninstall` one-liners (never auto-removed).
//!
//! Stdout carries JSON and nothing else under `--json`; every diagnostic goes to
//! stderr. A bare invocation on a TTY runs a minimal confirm flow; a non-TTY
//! invocation without an explicit machine mode refuses with guidance, exactly
//! like `init`.

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::cli::{HarnessTargetArg, UninstallArgs};
use crate::setup::{
    claude_config_path, claude_settings_path, codex_config_path, codex_hooks_path, remove_memorum_hooks_json,
    remove_memorum_hooks_toml, remove_memorum_mcp_json, remove_memorum_mcp_toml, SetupStepStatus,
};
use crate::socket::{probe_live_socket, resolve_socket_path, SocketProbe};
use crate::{
    client,
    protocol::{RequestPayload, ResponsePayload, ResponseResult},
};

/// macOS launchd labels installed by `scripts/install-launchd.sh`.
const LAUNCHD_LABELS: &[&str] = &["com.memorum.daemon", "com.memorum.dream-scheduled"];

/// Binaries the installer puts on PATH. Reported (never removed) by `verify`.
const INSTALLED_BINARIES: &[(&str, &str)] = &[
    ("memoryd", "memoryd"),
    ("memoryd-tui", "memoryd-tui"),
    ("memoryd-web", "memoryd-web"),
    ("memory-merge-driver", "memory-merge-driver"),
];

/// Dispatch `memoryd uninstall` to the machine path, the interactive confirm
/// flow, or a refusal, mirroring `init`'s TTY routing.
pub async fn run(args: UninstallArgs) -> anyhow::Result<()> {
    if args.non_interactive || args.json || args.print_only {
        return run_machine(args).await;
    }

    if std::io::stdin().is_terminal() && std::io::stderr().is_terminal() {
        return run_interactive(args).await;
    }

    anyhow::bail!(
        "memoryd uninstall: this terminal cannot run the interactive confirm flow (stdin and stderr must both be a TTY).\n\
         \n\
         Pick an explicit mode instead:\n\
         \n\
         \x20 memoryd uninstall --print-only\n\
         \x20     Preview every teardown step (read-only, JSON on stdout).\n\
         \n\
         \x20 memoryd uninstall --non-interactive --json [--purge] [--harness all]\n\
         \x20     Scripted teardown driven by flags; emits a JSON report.\n\
         \n\
         AI agents removing Memorum for a user should follow docs/agent-onboarding.md."
    );
}

/// Interactive TTY path: print the resolved plan, ask for one confirmation, then
/// run the scripted teardown. Declining is a guaranteed no-op.
async fn run_interactive(args: UninstallArgs) -> anyhow::Result<()> {
    let (repo, runtime) = resolve_repo_runtime(&args);
    eprintln!("memoryd uninstall will:");
    eprintln!("  - stop the daemon (if running) and remove any launchd plist");
    eprintln!("  - unwire the `memorum` MCP entry from detected harness configs");
    if args.purge {
        eprintln!("  - DELETE the repo:    {}", repo.display());
        eprintln!("  - DELETE the runtime: {}", runtime.display());
    } else {
        eprintln!("  - preserve your data (repo and runtime are kept; pass --purge to delete)");
    }
    eprint!("Proceed? [y/N] ");
    std::io::stderr().flush().ok();

    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    if !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
        eprintln!("aborted; nothing was changed.");
        return Ok(());
    }

    let report = execute(&args, &repo, &runtime).await;
    for step in &report.steps {
        if let Some(message) = &step.message {
            eprintln!("[{:?}] {message}", step.status);
        }
    }
    fail_if_fatal(&report)
}

/// Machine path: run the scripted teardown and emit JSON to stdout.
async fn run_machine(args: UninstallArgs) -> anyhow::Result<()> {
    let (repo, runtime) = resolve_repo_runtime(&args);
    // On the non-interactive path there is no TTY to confirm a purge; the
    // explicit `--purge` flag is the confirmation.
    let report = execute(&args, &repo, &runtime).await;

    let json = serde_json::to_string_pretty(&report)?;
    println!("{json}");
    fail_if_fatal(&report)
}

fn fail_if_fatal(report: &UninstallReport) -> anyhow::Result<()> {
    if report.steps.iter().any(|step| step.status == SetupStepStatus::Failed) {
        std::process::exit(1);
    }
    Ok(())
}

/// Resolve the canonical repo root and per-device runtime directory.
///
/// Mirrors `memoryd init`: `--repo` → `$MEMORUM_REPO` → `~/memorum`, with runtime
/// defaulting to `<repo>/.memoryd`.
pub(crate) fn resolve_repo_runtime(args: &UninstallArgs) -> (PathBuf, PathBuf) {
    let default_repo = std::env::var("MEMORUM_REPO")
        .ok()
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join("memorum")))
        .unwrap_or_else(|| PathBuf::from("./memorum"));
    let repo = args.repo.clone().unwrap_or(default_repo);
    let runtime = args.runtime.clone().unwrap_or_else(|| repo.join(".memoryd"));
    (repo, runtime)
}

/// Run every teardown step against the resolved paths and collect the report.
async fn execute(args: &UninstallArgs, repo: &Path, runtime: &Path) -> UninstallReport {
    let socket = resolve_socket_path(runtime);
    let detection = Detection::probe(args, repo, runtime, &socket);

    let mut report = UninstallReport::new(detection.clone());
    report.push(detect_step(&detection));
    let stop_step = stop_daemon_step(runtime, &socket, args.print_only).await;
    let stop_status = stop_step.status;
    report.push(stop_step);
    report.push(remove_launchd_step(&detection, args.print_only));
    report.extend(unwire_mcp_steps(args, &detection));
    report.extend(unwire_hooks_steps(args, &detection));
    report.push(purge_data_step(args, &detection, stop_status));
    report.push(verify_step(&socket, &detection, args.purge, args.print_only));
    report
}

#[derive(Debug, Clone, Serialize)]
struct Detection {
    repo: PathBuf,
    runtime: PathBuf,
    socket: PathBuf,
    socket_state: SocketState,
    pid_file_present: bool,
    repo_looks_like_memorum: bool,
    launchd_plists: Vec<PathBuf>,
    claude_config: Option<HarnessConfigDetection>,
    codex_config: Option<HarnessConfigDetection>,
    /// Claude `settings.json` holding the passive-recall hooks (distinct from the
    /// `.claude.json` MCP file).
    claude_hooks: Option<HarnessConfigDetection>,
    /// Codex hook config — an existing `hooks.json` if present, else the inline
    /// `[hooks]` in `config.toml`.
    codex_hooks: Option<HarnessConfigDetection>,
}

#[derive(Debug, Clone, Serialize)]
struct HarnessConfigDetection {
    path: PathBuf,
    has_memorum_entry: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum SocketState {
    Live,
    Stale,
    Absent,
}

impl From<SocketProbe> for SocketState {
    fn from(probe: SocketProbe) -> Self {
        match probe {
            SocketProbe::Live => Self::Live,
            SocketProbe::Stale => Self::Stale,
            SocketProbe::Absent => Self::Absent,
        }
    }
}

impl Detection {
    fn probe(args: &UninstallArgs, repo: &Path, runtime: &Path, socket: &Path) -> Self {
        let home = dirs::home_dir();
        let claude_config_dir = std::env::var("CLAUDE_CONFIG_DIR").ok();
        let codex_home = std::env::var("CODEX_HOME").ok();
        let claude = claude_config_path(claude_config_dir.as_deref(), home.as_deref());
        let codex = codex_config_path(codex_home.as_deref(), home.as_deref());
        let claude_settings = claude_settings_path(claude_config_dir.as_deref(), home.as_deref());

        Self {
            repo: repo.to_path_buf(),
            runtime: runtime.to_path_buf(),
            socket: socket.to_path_buf(),
            socket_state: probe_live_socket(socket).into(),
            pid_file_present: pid_file_path(runtime).exists(),
            repo_looks_like_memorum: repo_is_memorum_shaped(repo),
            launchd_plists: detected_launchd_plists(),
            claude_config: claude.map(|path| harness_config_detection(path, claude_has_memorum_entry)),
            codex_config: codex.map(|path| harness_config_detection(path, codex_has_memorum_entry)),
            claude_hooks: claude_settings.map(|path| harness_config_detection(path, json_hooks_entry)),
            codex_hooks: detect_codex_hooks(codex_home.as_deref(), home.as_deref()),
        }
        .with_harness_filter(args.harness)
    }

    /// Drop config detections the `--harness` selection excludes so the report
    /// and the unwire steps agree on what is in scope.
    fn with_harness_filter(mut self, harness: Option<HarnessTargetArg>) -> Self {
        let (want_claude, want_codex) = match harness {
            None | Some(HarnessTargetArg::All) => (true, true),
            Some(HarnessTargetArg::Claude) => (true, false),
            Some(HarnessTargetArg::Codex) => (false, true),
            Some(HarnessTargetArg::None) => (false, false),
            Some(HarnessTargetArg::Current) => (
                self.claude_config.as_ref().is_some_and(|c| c.has_memorum_entry)
                    || self.claude_hooks.as_ref().is_some_and(|c| c.has_memorum_entry),
                self.codex_config.as_ref().is_some_and(|c| c.has_memorum_entry)
                    || self.codex_hooks.as_ref().is_some_and(|c| c.has_memorum_entry),
            ),
        };
        if !want_claude {
            self.claude_config = None;
            self.claude_hooks = None;
        }
        if !want_codex {
            self.codex_config = None;
            self.codex_hooks = None;
        }
        self
    }
}

fn harness_config_detection(path: PathBuf, has_entry: fn(&str) -> bool) -> HarnessConfigDetection {
    let has_memorum_entry = std::fs::read_to_string(&path).map(|body| has_entry(&body)).unwrap_or(false);
    HarnessConfigDetection { path, has_memorum_entry }
}

fn claude_has_memorum_entry(body: &str) -> bool {
    remove_memorum_mcp_json(body).map(|outcome| outcome.removed > 0).unwrap_or(false)
}

fn codex_has_memorum_entry(body: &str) -> bool {
    remove_memorum_mcp_toml(body).map(|outcome| outcome.removed > 0).unwrap_or(false)
}

/// Whether a JSON hooks file (Claude `settings.json` or Codex `hooks.json` —
/// both nest events under a top-level `hooks` object) holds a memorum recall
/// hook. One predicate because the two file shapes are identical for the
/// purposes of detection.
fn json_hooks_entry(body: &str) -> bool {
    remove_memorum_hooks_json(body).map(|outcome| outcome.removed > 0).unwrap_or(false)
}

fn codex_has_hooks_toml_entry(body: &str) -> bool {
    remove_memorum_hooks_toml(body).map(|outcome| outcome.removed > 0).unwrap_or(false)
}

/// Resolve which Codex file holds the recall hooks. An existing `hooks.json`
/// wins (that is what the installer prefers), so it is detected first; otherwise
/// the inline `[hooks]` in `config.toml` is used. Returns the detection for the
/// file the installer would have written, even when no entry is present yet, so
/// the report and unwire step agree on the target.
fn detect_codex_hooks(codex_home: Option<&str>, home: Option<&Path>) -> Option<HarnessConfigDetection> {
    let hooks_json = codex_hooks_path(codex_home, home);
    if let Some(path) = hooks_json.filter(|path| path.exists()) {
        return Some(harness_config_detection(path, json_hooks_entry));
    }
    codex_config_path(codex_home, home).map(|path| harness_config_detection(path, codex_has_hooks_toml_entry))
}

fn detected_launchd_plists() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    let agents = home.join("Library").join("LaunchAgents");
    LAUNCHD_LABELS.iter().map(|label| agents.join(format!("{label}.plist"))).filter(|path| path.exists()).collect()
}

fn pid_file_path(runtime: &Path) -> PathBuf {
    runtime.join("memoryd.pid")
}

/// Whether `repo` carries the structure `Substrate::init` lays down. Used to
/// refuse a `--purge` of a path that is not actually a Memorum repo unless the
/// caller explicitly named it via `--repo`.
fn repo_is_memorum_shaped(repo: &Path) -> bool {
    repo.join(".memorum").exists() || repo.join("config.yaml").exists()
}

fn detect_step(detection: &Detection) -> StepReport {
    let mut notes = vec![format!("repo {}", detection.repo.display()), format!("socket {:?}", detection.socket_state)];
    if detection.pid_file_present {
        notes.push("pid file present".to_string());
    }
    if !detection.launchd_plists.is_empty() {
        notes.push(format!("{} launchd plist(s)", detection.launchd_plists.len()));
    }
    let wired: Vec<&str> = [
        detection.claude_config.as_ref().filter(|c| c.has_memorum_entry).map(|_| "claude"),
        detection.codex_config.as_ref().filter(|c| c.has_memorum_entry).map(|_| "codex"),
    ]
    .into_iter()
    .flatten()
    .collect();
    if !wired.is_empty() {
        notes.push(format!("memorum MCP entry in: {}", wired.join(", ")));
    }
    let hooked: Vec<&str> = [
        detection.claude_hooks.as_ref().filter(|c| c.has_memorum_entry).map(|_| "claude"),
        detection.codex_hooks.as_ref().filter(|c| c.has_memorum_entry).map(|_| "codex"),
    ]
    .into_iter()
    .flatten()
    .collect();
    if !hooked.is_empty() {
        notes.push(format!("memorum recall hooks in: {}", hooked.join(", ")));
    }
    StepReport::new(UninstallStep::Detect, SetupStepStatus::Succeeded).with_message(notes.join("; "))
}

async fn stop_daemon_step(runtime: &Path, socket: &Path, print_only: bool) -> StepReport {
    let pid_file = pid_file_path(runtime);
    let socket_live = matches!(probe_live_socket(socket), SocketProbe::Live);
    let pid = read_pid(&pid_file);

    if !socket_live && pid.is_none() {
        return StepReport::new(UninstallStep::StopDaemon, SetupStepStatus::Skipped)
            .with_message("no live socket and no pid file; daemon not running");
    }

    if print_only {
        let target = pid.map(|p| format!("pid {p}")).unwrap_or_else(|| "live socket".to_string());
        return StepReport::new(UninstallStep::StopDaemon, SetupStepStatus::Expected)
            .with_message(format!("[dry-run] would SIGTERM the daemon ({target}) and remove {}", pid_file.display()));
    }

    match stop_daemon(pid, socket).await {
        Ok(message) => {
            let _ = std::fs::remove_file(&pid_file);
            StepReport::new(UninstallStep::StopDaemon, SetupStepStatus::Succeeded).with_message(message)
        }
        Err(message) => StepReport::new(UninstallStep::StopDaemon, SetupStepStatus::Failed).with_message(message),
    }
}

fn read_pid(pid_file: &Path) -> Option<u32> {
    std::fs::read_to_string(pid_file).ok().and_then(|raw| raw.trim().parse::<u32>().ok())
}

/// SIGTERM the daemon pid, waiting up to ~5s for exit. Missing pid files fall
/// back to the daemon's `Status` response so older/lost-pid daemons can still
/// be stopped without process-scan heuristics.
async fn stop_daemon(pid: Option<u32>, socket: &Path) -> Result<String, String> {
    let pid = match pid {
        Some(pid) => pid,
        None => pid_from_status(socket).await?,
    };

    if !process_alive(pid) {
        return Ok(format!("daemon pid {pid} was already gone"));
    }

    send_sigterm(pid).map_err(|error| format!("failed to signal pid {pid}: {error}"))?;

    for _ in 0..50 {
        if !process_alive(pid) {
            return Ok(format!("stopped daemon pid {pid}"));
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Err(format!("daemon pid {pid} did not exit within 5s after SIGTERM"))
}

async fn pid_from_status(socket: &Path) -> Result<u32, String> {
    let status = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        client::request(socket, "uninstall-status", RequestPayload::Status),
    )
    .await
    .map_err(|_| live_socket_no_pid_guidance(socket, "Status request timed out"))?
    .map_err(|error| live_socket_no_pid_guidance(socket, &format!("Status request failed: {error:#}")))?;

    match status.result {
        ResponseResult::Success(ResponsePayload::Status(status)) => status
            .daemon
            .map(|daemon| daemon.pid)
            .ok_or_else(|| live_socket_no_pid_guidance(socket, "Status response did not include a daemon pid")),
        ResponseResult::Success(other) => {
            Err(live_socket_no_pid_guidance(socket, &format!("Status request returned unexpected response: {other:?}")))
        }
        ResponseResult::Error(error) => Err(live_socket_no_pid_guidance(
            socket,
            &format!("Status request returned {}: {}", error.code, error.message),
        )),
    }
}

fn live_socket_no_pid_guidance(socket: &Path, reason: &str) -> String {
    format!(
        "socket at {} is live but no pid file was found ({reason}); stop the daemon manually before retrying",
        socket.display()
    )
}

#[cfg(unix)]
fn process_alive(pid: u32) -> bool {
    // SAFETY: signal 0 performs error checking without delivering a signal.
    unsafe { posix_kill(pid as i32, 0) == 0 }
}

#[cfg(not(unix))]
fn process_alive(_pid: u32) -> bool {
    false
}

#[cfg(unix)]
fn send_sigterm(pid: u32) -> std::io::Result<()> {
    const SIGTERM: i32 = 15;
    // SAFETY: `kill(2)` does not dereference Rust pointers; `pid`/`sig` are ints.
    let result = unsafe { posix_kill(pid as i32, SIGTERM) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(unix))]
fn send_sigterm(_pid: u32) -> std::io::Result<()> {
    Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "SIGTERM unsupported on this platform"))
}

#[cfg(unix)]
unsafe extern "C" {
    #[link_name = "kill"]
    fn posix_kill(pid: i32, sig: i32) -> i32;
}

fn remove_launchd_step(detection: &Detection, print_only: bool) -> StepReport {
    if !cfg!(target_os = "macos") {
        return StepReport::new(UninstallStep::RemoveLaunchd, SetupStepStatus::Skipped)
            .with_message("launchd is macOS-only; nothing to remove on this platform");
    }
    if detection.launchd_plists.is_empty() {
        return StepReport::new(UninstallStep::RemoveLaunchd, SetupStepStatus::Skipped)
            .with_message("no com.memorum.* launchd plist found");
    }
    if print_only {
        let paths = detection.launchd_plists.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ");
        return StepReport::new(UninstallStep::RemoveLaunchd, SetupStepStatus::Expected)
            .with_message(format!("[dry-run] would bootout and delete: {paths}"));
    }

    let mut removed = Vec::new();
    let mut errors = Vec::new();
    for plist in &detection.launchd_plists {
        match bootout_and_delete(plist) {
            Ok(()) => removed.push(plist.display().to_string()),
            Err(message) => errors.push(message),
        }
    }

    if errors.is_empty() {
        StepReport::new(UninstallStep::RemoveLaunchd, SetupStepStatus::Succeeded)
            .with_message(format!("removed launchd plist(s): {}", removed.join(", ")))
    } else {
        StepReport::new(UninstallStep::RemoveLaunchd, SetupStepStatus::Failed).with_message(errors.join("; "))
    }
}

/// `launchctl bootout gui/<uid> <plist>` then delete the plist. Bootout failure
/// is tolerated (the agent may already be unloaded); a failed delete is fatal.
fn bootout_and_delete(plist: &Path) -> Result<(), String> {
    let domain = format!("gui/{}", current_uid());
    let _ = std::process::Command::new("launchctl").arg("bootout").arg(&domain).arg(plist).output();
    std::fs::remove_file(plist).map_err(|error| format!("failed to delete {}: {error}", plist.display()))
}

#[cfg(unix)]
fn current_uid() -> u32 {
    // SAFETY: `getuid(2)` takes no arguments and cannot fail.
    unsafe { posix_getuid() }
}

#[cfg(not(unix))]
fn current_uid() -> u32 {
    0
}

#[cfg(unix)]
unsafe extern "C" {
    #[link_name = "getuid"]
    fn posix_getuid() -> u32;
}

fn unwire_mcp_steps(args: &UninstallArgs, detection: &Detection) -> Vec<StepReport> {
    let mut steps = Vec::new();
    if let Some(config) = &detection.claude_config {
        steps.push(unwire_one(UninstallStep::UnwireClaude, config, args.print_only, remove_memorum_mcp_json));
    }
    if let Some(config) = &detection.codex_config {
        steps.push(unwire_one(UninstallStep::UnwireCodex, config, args.print_only, remove_memorum_mcp_toml));
    }
    if steps.is_empty() {
        steps.push(
            StepReport::new(UninstallStep::UnwireMcp, SetupStepStatus::Skipped)
                .with_message("no harness config selected for unwiring"),
        );
    }
    steps
}

/// Remove the passive-recall hooks alongside the MCP entries. Claude hooks live
/// in `settings.json` (JSON); Codex hooks live in either `hooks.json` (JSON) or
/// the inline `[hooks]` of `config.toml` (TOML), so the Codex unwire function is
/// chosen by the detected file's extension. Removal keys on the stable `recall
/// hook` marker — never the absolute binary path — so upgraded paths are still
/// recognized, and sibling hooks survive.
fn unwire_hooks_steps(args: &UninstallArgs, detection: &Detection) -> Vec<StepReport> {
    let mut steps = Vec::new();
    if let Some(config) = &detection.claude_hooks {
        steps.push(unwire_one(UninstallStep::UnwireClaudeHooks, config, args.print_only, remove_memorum_hooks_json));
    }
    if let Some(config) = &detection.codex_hooks {
        let unwire = codex_hooks_unwire_fn(&config.path);
        steps.push(unwire_one(UninstallStep::UnwireCodexHooks, config, args.print_only, unwire));
    }
    steps
}

/// Pick the Codex hooks unwire function by file shape: `hooks.json` is JSON,
/// `config.toml`'s inline `[hooks]` is TOML.
fn codex_hooks_unwire_fn(path: &Path) -> UnwireFn {
    if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
        remove_memorum_hooks_json
    } else {
        remove_memorum_hooks_toml
    }
}

type UnwireFn = fn(&str) -> Result<crate::setup::ConfigUnwireOutcome, crate::setup::WireError>;

fn unwire_one(step: UninstallStep, config: &HarnessConfigDetection, print_only: bool, unwire: UnwireFn) -> StepReport {
    let body = match std::fs::read_to_string(&config.path) {
        Ok(body) => body,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return StepReport::new(step, SetupStepStatus::Skipped)
                .with_message(format!("{} not present", config.path.display()));
        }
        Err(error) => {
            return StepReport::new(step, SetupStepStatus::Failed)
                .with_message(format!("failed to read {}: {error}", config.path.display()));
        }
    };

    let outcome = match unwire(&body) {
        Ok(outcome) => outcome,
        Err(error) => {
            return StepReport::new(step, SetupStepStatus::Failed)
                .with_message(format!("failed to parse {}: {error}", config.path.display()));
        }
    };

    if outcome.removed == 0 {
        return StepReport::new(step, SetupStepStatus::Skipped)
            .with_message(format!("no memorum entry in {}", config.path.display()));
    }
    if print_only {
        return StepReport::new(step, SetupStepStatus::Expected).with_message(format!(
            "[dry-run] would remove {} memorum entry/entries from {}",
            outcome.removed,
            config.path.display()
        ));
    }

    match crate::setup::mcp_wire::write_config_file_safely(&config.path, &outcome.body) {
        Ok(()) => StepReport::new(step, SetupStepStatus::Succeeded).with_message(format!(
            "removed {} memorum entry/entries from {}",
            outcome.removed,
            config.path.display()
        )),
        Err(error) => StepReport::new(step, SetupStepStatus::Failed)
            .with_message(format!("failed to write {}: {error}", config.path.display())),
    }
}

fn purge_data_step(args: &UninstallArgs, detection: &Detection, stop_status: SetupStepStatus) -> StepReport {
    let repo = &detection.repo;
    let runtime = &detection.runtime;
    if !args.purge {
        return StepReport::new(UninstallStep::PurgeData, SetupStepStatus::Skipped)
            .with_message("data preserved; pass --purge to delete");
    }

    if stop_status == SetupStepStatus::Failed {
        return StepReport::new(UninstallStep::PurgeData, SetupStepStatus::Failed)
            .with_message("refusing to purge while the daemon may still be running; stop it manually and re-run");
    }

    // Refuse to delete a repo that does not look like a Memorum repo unless the
    // caller explicitly pointed `--repo` at it. This is the guard against
    // `--purge` nuking an arbitrary default path.
    let repo_explicit = args.repo.is_some();
    if !detection.repo_looks_like_memorum && !repo_explicit {
        return StepReport::new(UninstallStep::PurgeData, SetupStepStatus::Failed).with_message(format!(
            "refusing to purge {}: it does not look like a Memorum repo (no .memorum/ or config.yaml); pass --repo explicitly to override",
            repo.display()
        ));
    }

    if args.print_only {
        return StepReport::new(UninstallStep::PurgeData, SetupStepStatus::Expected).with_message(format!(
            "[dry-run] would delete repo {} and runtime {}",
            repo.display(),
            runtime.display()
        ));
    }

    let mut errors = Vec::new();
    for dir in dirs_to_purge(repo, runtime) {
        if dir.exists() {
            if let Err(error) = std::fs::remove_dir_all(&dir) {
                errors.push(format!("failed to delete {}: {error}", dir.display()));
            }
        }
    }

    if errors.is_empty() {
        StepReport::new(UninstallStep::PurgeData, SetupStepStatus::Succeeded).with_message(format!(
            "deleted repo {} and runtime {}",
            repo.display(),
            runtime.display()
        ))
    } else {
        StepReport::new(UninstallStep::PurgeData, SetupStepStatus::Failed).with_message(errors.join("; "))
    }
}

/// The directories `--purge` deletes: the repo, and the runtime when it is not
/// already nested under the repo (the default `<repo>/.memoryd` is removed with
/// the repo, but a custom runtime elsewhere needs its own deletion).
fn dirs_to_purge(repo: &Path, runtime: &Path) -> Vec<PathBuf> {
    if runtime.starts_with(repo) {
        vec![repo.to_path_buf()]
    } else {
        vec![repo.to_path_buf(), runtime.to_path_buf()]
    }
}

fn verify_step(socket: &Path, detection: &Detection, purged: bool, print_only: bool) -> StepReport {
    if print_only {
        // A dry-run applied nothing, so the live probes would report the
        // still-present state as residual. Report `expected` rather than failing
        // a preview that deliberately changed nothing.
        return StepReport::new(UninstallStep::Verify, SetupStepStatus::Expected).with_message(
            "[dry-run] would confirm socket gone, plists gone, configs clean, and report leftover binaries",
        );
    }

    let mut findings = Vec::new();

    if matches!(probe_live_socket(socket), SocketProbe::Live) {
        findings.push(format!("socket still live at {}", socket.display()));
    }
    for plist in detected_launchd_plists() {
        findings.push(format!("launchd plist still present: {}", plist.display()));
    }
    if detection.claude_config.as_ref().is_some_and(config_still_has_memorum) {
        findings.push("claude config still contains a memorum MCP entry".to_string());
    }
    if detection.codex_config.as_ref().is_some_and(config_still_has_memorum) {
        findings.push("codex config still contains a memorum MCP entry".to_string());
    }
    if detection.claude_hooks.as_ref().is_some_and(config_still_has_hooks) {
        findings.push("claude settings still contain a memorum recall hook".to_string());
    }
    if detection.codex_hooks.as_ref().is_some_and(config_still_has_hooks) {
        findings.push("codex config still contains a memorum recall hook".to_string());
    }

    let leftover = leftover_binaries();
    let mut message = if findings.is_empty() {
        let purged_note = if purged { "; data purged" } else { "; data preserved" };
        format!("socket gone, plists gone, configs clean{purged_note}")
    } else {
        format!("residual state: {}", findings.join("; "))
    };
    if !leftover.is_empty() {
        message.push_str(&format!(
            "\nleftover binaries on PATH (not removed): {}.\nRemove with: {}",
            leftover.join(", "),
            cargo_uninstall_oneliner(&leftover)
        ));
    }

    let status = if findings.is_empty() { SetupStepStatus::Succeeded } else { SetupStepStatus::Failed };
    StepReport::new(UninstallStep::Verify, status).with_message(message)
}

fn config_still_has_memorum(config: &HarnessConfigDetection) -> bool {
    let path = &config.path;
    let Ok(body) = std::fs::read_to_string(path) else {
        return false;
    };
    let json = path.extension().and_then(|ext| ext.to_str()) == Some("json");
    if json {
        claude_has_memorum_entry(&body)
    } else {
        codex_has_memorum_entry(&body)
    }
}

fn config_still_has_hooks(config: &HarnessConfigDetection) -> bool {
    let path = &config.path;
    let Ok(body) = std::fs::read_to_string(path) else {
        return false;
    };
    let json = path.extension().and_then(|ext| ext.to_str()) == Some("json");
    if json {
        json_hooks_entry(&body)
    } else {
        codex_has_hooks_toml_entry(&body)
    }
}

/// Crate-package names whose binaries are still on PATH.
fn leftover_binaries() -> Vec<String> {
    INSTALLED_BINARIES
        .iter()
        .filter(|(binary, _)| which::which(binary).is_ok())
        .map(|(_, package)| (*package).to_string())
        .collect()
}

fn cargo_uninstall_oneliner(packages: &[String]) -> String {
    format!("cargo uninstall {}", packages.join(" "))
}

/// Machine-readable outcome for `memoryd uninstall`. Mirrors the `SetupReport`
/// shape (`schema_version` + `steps[]`) so an agent can parse both with the same
/// expectations.
#[derive(Debug, Clone, Serialize)]
struct UninstallReport {
    schema_version: u32,
    detection: Detection,
    steps: Vec<StepReport>,
}

impl UninstallReport {
    fn new(detection: Detection) -> Self {
        Self { schema_version: 1, detection, steps: Vec::new() }
    }

    fn push(&mut self, step: StepReport) {
        self.steps.push(step);
    }

    fn extend(&mut self, steps: Vec<StepReport>) {
        self.steps.extend(steps);
    }
}

#[derive(Debug, Clone, Serialize)]
struct StepReport {
    step: UninstallStep,
    status: SetupStepStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

impl StepReport {
    fn new(step: UninstallStep, status: SetupStepStatus) -> Self {
        Self { step, status, message: None }
    }

    fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }
}

/// Teardown step identities. `UnwireMcp` is the umbrella name used when no
/// harness is in scope; `UnwireClaude`/`UnwireCodex` name the per-config steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum UninstallStep {
    Detect,
    StopDaemon,
    RemoveLaunchd,
    UnwireMcp,
    UnwireClaude,
    UnwireCodex,
    UnwireClaudeHooks,
    UnwireCodexHooks,
    PurgeData,
    Verify,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::UninstallArgs;

    fn args() -> UninstallArgs {
        UninstallArgs {
            repo: None,
            runtime: None,
            non_interactive: true,
            json: true,
            print_only: false,
            purge: false,
            harness: None,
        }
    }

    #[test]
    fn dirs_to_purge_collapses_nested_runtime() {
        let repo = PathBuf::from("/r");
        assert_eq!(dirs_to_purge(&repo, &repo.join(".memoryd")), vec![repo.clone()]);
        let elsewhere = PathBuf::from("/elsewhere");
        assert_eq!(dirs_to_purge(&repo, &elsewhere), vec![repo, elsewhere]);
    }

    #[test]
    fn resolve_prefers_explicit_flags() {
        let mut input = args();
        input.repo = Some(PathBuf::from("/explicit"));
        let (repo, runtime) = resolve_repo_runtime(&input);
        assert_eq!(repo, PathBuf::from("/explicit"));
        assert_eq!(runtime, PathBuf::from("/explicit/.memoryd"));
    }

    #[test]
    fn purge_refuses_unshaped_default_repo_without_explicit_flag() {
        let temp = tempfile::tempdir().expect("tempdir");
        let repo = temp.path().join("not-memorum");
        std::fs::create_dir_all(&repo).expect("repo dir");
        let runtime = repo.join(".memoryd");
        let mut input = args();
        input.purge = true; // repo is None → not explicit
        let detection = Detection::probe(&input, &repo, &runtime, &resolve_socket_path(&runtime));

        let step = purge_data_step(&input, &detection, SetupStepStatus::Skipped);
        assert_eq!(step.status, SetupStepStatus::Failed);
        assert!(repo.exists(), "refused purge must not delete the dir");
    }

    #[test]
    fn purge_skipped_without_flag() {
        let input = args();
        let temp = tempfile::tempdir().expect("tempdir");
        let repo = temp.path().join("repo");
        let runtime = repo.join(".memoryd");
        let detection = Detection::probe(&input, &repo, &runtime, &resolve_socket_path(&runtime));
        let step = purge_data_step(&input, &detection, SetupStepStatus::Skipped);
        assert_eq!(step.status, SetupStepStatus::Skipped);
        assert_eq!(step.message.as_deref(), Some("data preserved; pass --purge to delete"));
    }

    #[test]
    fn non_purge_run_reports_skipped_even_when_stop_failed() {
        let input = args(); // purge = false
        let temp = tempfile::tempdir().expect("tempdir");
        let repo = temp.path().join("repo");
        let runtime = repo.join(".memoryd");
        let detection = Detection::probe(&input, &repo, &runtime, &resolve_socket_path(&runtime));
        let step = purge_data_step(&input, &detection, SetupStepStatus::Failed);
        assert_eq!(step.status, SetupStepStatus::Skipped);
        assert_eq!(step.message.as_deref(), Some("data preserved; pass --purge to delete"));
    }

    #[test]
    fn harness_filter_drops_excluded_configs() {
        let temp = tempfile::tempdir().expect("tempdir");
        let repo = temp.path().join("repo");
        let runtime = repo.join(".memoryd");
        let mut input = args();
        input.harness = Some(HarnessTargetArg::None);
        let detection = Detection::probe(&input, &repo, &runtime, &resolve_socket_path(&runtime));
        assert!(detection.claude_config.is_none());
        assert!(detection.codex_config.is_none());
        assert!(detection.claude_hooks.is_none(), "hook detection also dropped by harness filter");
        assert!(detection.codex_hooks.is_none());
    }

    #[test]
    fn codex_hooks_unwire_fn_picks_by_extension() {
        let json = codex_hooks_unwire_fn(Path::new("/home/u/.codex/hooks.json"));
        // Codex hooks.json nests events under a top-level `hooks` object, like
        // Claude settings.json; the JSON unwire keys off that shape.
        let hooks_json = r#"{ "hooks": { "SessionStart": [ { "hooks": [ { "type": "command", "command": "\"/x/memoryd\" recall hook --socket \"/s\" --harness codex" } ] } ] } }"#;
        assert!(json(hooks_json).expect("unwire json").removed > 0);

        let toml = codex_hooks_unwire_fn(Path::new("/home/u/.codex/config.toml"));
        let inline = "\
[[hooks.SessionStart]]\n\
[[hooks.SessionStart.hooks]]\n\
type = \"command\"\n\
command = \"\\\"/x/memoryd\\\" recall hook --socket \\\"/s\\\" --harness codex\"\n";
        assert!(toml(inline).expect("unwire toml").removed > 0);
    }

    #[test]
    fn unwire_hooks_steps_removes_claude_settings_marker() {
        let temp = tempfile::tempdir().expect("tempdir");
        let settings = temp.path().join("settings.json");
        std::fs::write(
            &settings,
            r#"{ "hooks": { "SessionStart": [ { "matcher": "startup", "hooks": [ { "type": "command", "command": "\"/old/memoryd\" recall hook --socket \"/s\" --harness claude-code", "timeout": 2 } ] } ] } }"#,
        )
        .expect("write settings");

        let detection = HarnessConfigDetection { path: settings.clone(), has_memorum_entry: true };
        let step = unwire_one(UninstallStep::UnwireClaudeHooks, &detection, false, remove_memorum_hooks_json);
        assert_eq!(step.status, SetupStepStatus::Succeeded);

        let after = std::fs::read_to_string(&settings).expect("read back");
        assert!(!after.contains("recall hook"), "marker removed");
    }
}
