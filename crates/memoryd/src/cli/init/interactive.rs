//! Interactive (TTY) frontend for `memoryd init`.
//!
//! Drives the shared [`SetupEngine`] through [`InteractiveIo`], which implements
//! [`SetupIo`] by presenting `dialoguer`-backed prompts (Confirm, Select) to the
//! user. Declining every prompt is a safe no-op equivalent to `--detect-only`:
//! no substrate is created, no daemon is arranged, no MCP configs are modified.
//!
//! Explicitly passed selector flags pre-answer their prompt instead of being
//! re-asked: `--import` answers the import confirm, `--harness`/`--wire-mcp`/
//! `--daemon`/`--non-git-cwd-default` answer their selects, and `--print-only`
//! forces a dry run regardless of what is opted into.
//!
//! The wizard opens with a detection summary (what was found and *how* it was
//! found — env var, settings file, or default path) and closes with a rendered
//! epilogue of what happened plus concrete next steps.
//!
//! The public entry point is [`run`]; [`run_with_io`] exposes a testable seam
//! that accepts any [`SetupIo`] implementation without touching a real TTY.

use std::path::{Path, PathBuf};

use crate::cli::InitArgs;
use crate::setup::{
    DaemonStrategy, HarnessDetection, HarnessSelection, NonGitCwdDecision, SetupDetection, SetupDiscoverySource,
    SetupEngine, SetupIo, SetupReport, SetupResult, SetupSocketState, SetupStep, SetupStepStatus, WireHooksSelection,
    WireMcpSelection,
};

use super::resolve_repo_runtime;

/// Drive interactive setup against a real TTY using `dialoguer` prompts.
pub async fn run(args: InitArgs) -> anyhow::Result<()> {
    let (repo, runtime) = resolve_repo_runtime(&args);
    let socket = crate::socket::resolve_socket_path(&runtime);
    let io = InteractiveIo::from_args(&args, &repo, &runtime, &socket);
    run_with_io(args, io).await
}

/// Drive interactive setup with a caller-supplied [`SetupIo`] implementation.
///
/// This seam keeps tests deterministic: pass a `ScriptedIo` (or any other
/// `SetupIo` impl) to exercise the full engine path without a real TTY.
pub async fn run_with_io<I: SetupIo>(args: InitArgs, mut io: I) -> anyhow::Result<()> {
    let (repo, runtime) = resolve_repo_runtime(&args);
    let socket = crate::socket::resolve_socket_path(&runtime);
    let engine = SetupEngine::new(&repo, &runtime);
    let report = engine.run(&mut io).await?;
    print!("{}", render_epilogue(&report, &repo, &runtime, &socket));
    Ok(())
}

/// Selector answers seeded from explicitly passed CLI flags. A `Some` value
/// answers the corresponding prompt without asking; `None` prompts.
#[derive(Debug, Default)]
pub struct SeededDecisions {
    pub import: Option<bool>,
    pub harnesses: Option<HarnessSelection>,
    pub non_git_cwd: Option<NonGitCwdDecision>,
    pub wire_mcp: Option<WireMcpSelection>,
    pub wire_hooks: Option<WireHooksSelection>,
    pub daemon: Option<DaemonStrategy>,
    /// `--print-only`: force a dry run even when actions are opted into.
    pub print_only: bool,
}

/// Repo/runtime/socket paths shown in the wizard intro and prompts.
#[derive(Debug, Clone)]
struct WizardHeader {
    repo: PathBuf,
    runtime: PathBuf,
    socket: PathBuf,
}

/// Dialoguer-backed interactive I/O for `memoryd init`.
///
/// Each unseeded decision presents a prompt via `dialoguer`. Prompt failures
/// (e.g. the user hits Ctrl-D) fall back to the safe/skip default so a
/// partially-answered session never mutates state in an unexpected way.
///
/// The struct accumulates the decisions as they are made so that [`print_only`]
/// can honor the module-level "declining every prompt is a safe no-op" contract:
/// when the user opts into nothing (no import, no daemon, no MCP wiring) the
/// session runs in dry-run mode and never provisions the substrate.
///
/// [`print_only`]: InteractiveIo::print_only
#[derive(Debug, Default)]
pub struct InteractiveIo {
    seeds: SeededDecisions,
    header: Option<WizardHeader>,
    intro_printed: bool,
    chose_import: bool,
    chose_daemon: bool,
    chose_wiring: bool,
    chose_hooks: bool,
}

impl InteractiveIo {
    /// Build the wizard I/O from parsed CLI args, seeding prompts from any
    /// explicitly passed selector flags.
    fn from_args(args: &InitArgs, repo: &Path, runtime: &Path, socket: &Path) -> Self {
        Self {
            seeds: SeededDecisions {
                import: args.import.then_some(true),
                harnesses: args.harness.map(Into::into),
                non_git_cwd: args.non_git_cwd_default.map(Into::into),
                wire_mcp: args.wire_mcp.map(Into::into),
                wire_hooks: args.wire_hooks.map(Into::into),
                daemon: args.daemon.map(Into::into),
                print_only: args.print_only,
            },
            header: Some(WizardHeader {
                repo: repo.to_path_buf(),
                runtime: runtime.to_path_buf(),
                socket: socket.to_path_buf(),
            }),
            ..Self::default()
        }
    }

    /// Print the welcome banner and detection summary once, before the first
    /// prompt. Explains what was found, how it was found, and the opt-in
    /// contract — the parts a first-time user needs before answering anything.
    fn print_intro_once(&mut self, detection: &SetupDetection) {
        if self.intro_printed {
            return;
        }
        self.intro_printed = true;

        println!("Memorum setup");
        if let Some(header) = &self.header {
            println!("  repo:    {}", header.repo.display());
            println!("  runtime: {}", header.runtime.display());
            println!("  socket:  {}", header.socket.display());
        }
        println!();
        println!("Detected on this machine:");
        println!("  Claude Code memory: {}", describe_harness(&detection.claude, "topic file", "$CLAUDE_CONFIG_DIR"));
        println!("  Codex CLI memory:   {}", describe_harness(&detection.codex, "candidate", "$CODEX_HOME"));
        println!("  Daemon socket:      {}", describe_socket(detection.daemon.socket_state));
        let parse_errors = detection.claude.parse_errors + detection.codex.parse_errors;
        if parse_errors > 0 {
            println!("  ({parse_errors} file(s) could not be parsed and will be skipped)");
        }
        println!();
        println!("This wizard walks through importing that memory, arranging the Memorum");
        println!("daemon, and wiring passive-recall hooks into your coding agents. (The MCP");
        println!("bridge is an opt-in compatibility surface, off by default.)");
        println!("Nothing changes unless you opt in — declining every prompt exits without");
        println!("touching anything.");
        println!();
        if self.seeds.print_only {
            println!("--print-only: this is a dry run. Decisions are collected and reported,");
            println!("but no files are written.");
            println!();
        }
    }
}

impl SetupIo for InteractiveIo {
    fn confirm_import(&mut self, detection: &SetupDetection) -> SetupResult<bool> {
        self.print_intro_once(detection);

        let total = detection.claude.candidates + detection.codex.candidates;
        if total == 0 {
            println!("No prior harness memory found — nothing to import.");
            self.chose_import = false;
            return Ok(false);
        }
        if let Some(seeded) = self.seeds.import {
            println!("Import prior harness memory: {} (--import)", if seeded { "yes" } else { "no" });
            self.chose_import = seeded;
            return Ok(seeded);
        }

        let prompt = format!(
            "Import prior harness memory into Memorum? ({} Claude, {} Codex candidate(s))",
            detection.claude.candidates, detection.codex.candidates
        );
        let answer = dialoguer::Confirm::new().with_prompt(prompt).default(false).interact().unwrap_or(false);
        self.chose_import = answer;
        Ok(answer)
    }

    fn choose_harnesses(&mut self, _detection: &SetupDetection) -> SetupResult<HarnessSelection> {
        if let Some(seeded) = self.seeds.harnesses {
            if self.chose_import {
                println!("Harnesses to import: {seeded} (--harness)");
            }
            return Ok(seeded);
        }
        // The harness selection only matters when an import was opted into;
        // don't make a user who declined import answer follow-up questions.
        if !self.chose_import {
            return Ok(HarnessSelection::None);
        }
        let items = &["Current harness only", "Claude Code", "Codex CLI", "All harnesses", "None (skip import)"];
        Ok(prompt_harness_selection("Which harness memories should be imported?", items, 0, 4))
    }

    fn choose_non_git_cwd_default(&mut self, _detection: &SetupDetection) -> SetupResult<NonGitCwdDecision> {
        if let Some(seeded) = self.seeds.non_git_cwd {
            return Ok(seeded);
        }
        if !self.chose_import {
            return Ok(NonGitCwdDecision::Skip);
        }
        println!();
        println!("Some imported memories may come from sessions whose working directory");
        println!("was not a git checkout, so they can't be tied to a project automatically.");
        let items = &[
            "Create a project for each directory (recommended; saved and recall-active, no file written)",
            "Skip them (re-import later if wanted)",
            "Keep them under your user scope (me)",
            "Generate a .memory-project.yaml in each non-git directory",
        ];
        let selection = dialoguer::Select::new()
            .with_prompt("What should happen to memories from non-git working directories?")
            .items(items)
            .default(0)
            .interact()
            .unwrap_or(0);
        let decision = match selection {
            1 => NonGitCwdDecision::Skip,
            2 => NonGitCwdDecision::Me,
            3 => NonGitCwdDecision::Generate,
            _ => NonGitCwdDecision::DeriveProject,
        };
        Ok(decision)
    }

    fn choose_mcp_wiring(&mut self, _detection: &SetupDetection) -> SetupResult<WireMcpSelection> {
        if let Some(seeded) = self.seeds.wire_mcp {
            println!("MCP wiring: {seeded} (--wire-mcp)");
            self.chose_wiring = !matches!(seeded, WireMcpSelection::None);
            return Ok(seeded);
        }
        println!();
        println!("MCP wiring registers a `memorum` server with your coding agents so they");
        println!("can read and write memories. It edits harness config (e.g. via `claude");
        println!("mcp add` / `~/.codex/config.toml`); existing entries are left intact.");
        let items = &[
            "Current harness config only",
            "Claude Code config",
            "Codex CLI config",
            "All harness configs",
            "None (skip MCP wiring)",
        ];
        let wire = prompt_harness_selection("Which MCP harness configs should be wired to Memorum?", items, 4, 4);
        self.chose_wiring = !matches!(wire, WireMcpSelection::None);
        Ok(wire)
    }

    fn choose_hook_wiring(&mut self, _detection: &SetupDetection) -> SetupResult<WireHooksSelection> {
        if let Some(seeded) = self.seeds.wire_hooks {
            println!("Hook wiring: {seeded} (--wire-hooks)");
            self.chose_hooks = !matches!(seeded, WireHooksSelection::None);
            return Ok(seeded);
        }
        println!();
        println!("Passive-recall hooks inject relevant memories automatically — a base block");
        println!("at session start and a prompt-relevant delta on each turn — so the agent");
        println!("just remembers, with no extra API cost. This edits settings.json (Claude)");
        println!("and the Codex hooks config; Codex also needs a one-time `/hooks` trust.");
        let items = &[
            "Current harness config only (recommended)",
            "Claude Code config",
            "Codex CLI config",
            "All harness configs",
            "None (skip hook wiring)",
        ];
        // Default-on: index 0 (current harness). The magical "it just remembers"
        // UX is the point, so the safe-but-passive default is to wire, not skip.
        let wire = prompt_harness_selection("Which harness configs should be wired for passive recall?", items, 0, 0);
        self.chose_hooks = !matches!(wire, WireHooksSelection::None);
        Ok(wire)
    }

    fn choose_daemon_strategy(&mut self, _detection: &SetupDetection) -> SetupResult<DaemonStrategy> {
        if let Some(seeded) = self.seeds.daemon {
            println!("Daemon arrangement: {} (--daemon)", daemon_label(seeded));
            self.chose_daemon = !matches!(seeded, DaemonStrategy::None);
            return Ok(seeded);
        }
        println!();
        println!("The daemon embeds memories locally with Qwen3-Embedding-0.6B. The model");
        if let Some(header) = &self.header {
            println!("weights (~1 GB) download once, on first daemon start, into");
            println!("{}/models — they are never bundled.", header.runtime.display());
        } else {
            println!("weights (~1 GB) download once, on first daemon start, into the runtime");
            println!("models directory — they are never bundled.");
        }
        let items = &[
            "On-demand (start manually when needed)",
            "Background process (start now, no persistence)",
            "launchd service (macOS persistent daemon)",
            "None (skip daemon setup)",
        ];
        let selection = dialoguer::Select::new()
            .with_prompt("How should the Memorum daemon be arranged?")
            .items(items)
            .default(3)
            .interact()
            .unwrap_or(3);
        let strategy = match selection {
            1 => DaemonStrategy::Background,
            2 => {
                // Offer the launchd upgrade confirmation so users understand the
                // persistence implication before it is applied.
                let confirmed = dialoguer::Confirm::new()
                    .with_prompt("Install a launchd service that starts Memorum at login?")
                    .default(false)
                    .interact()
                    .unwrap_or(false);
                if confirmed {
                    DaemonStrategy::Launchd
                } else {
                    DaemonStrategy::OnDemand
                }
            }
            3 => DaemonStrategy::None,
            _ => DaemonStrategy::OnDemand,
        };
        // Selecting a daemon arrangement (anything other than `None`) is opting
        // into provisioning. `OnDemand` counts: it still wants a substrate that a
        // later `memoryd` start will serve.
        self.chose_daemon = !matches!(strategy, DaemonStrategy::None);
        Ok(strategy)
    }

    fn print_only(&mut self) -> SetupResult<bool> {
        // Honor the "declining every prompt is a safe no-op" contract: when the
        // user opted into nothing — no import, no daemon, no MCP wiring, no hook
        // wiring — there is nothing to provision, so run in dry-run mode and
        // leave the substrate untouched (equivalent to `--detect-only`). Opting
        // into any action (including hook wiring) runs the real steps, unless
        // `--print-only` forces a dry run. `print_only` is collected last, after
        // every decision, so `self` reflects the full session.
        let declined_everything = !self.chose_import && !self.chose_daemon && !self.chose_wiring && !self.chose_hooks;
        Ok(self.seeds.print_only || declined_everything)
    }

    fn note(&mut self, message: &str) -> SetupResult<()> {
        eprintln!("{message}");
        Ok(())
    }
}

/// One-line detection summary for a harness: count, root, and *how* the root
/// was discovered — the part that makes nonstandard profile setups legible.
fn describe_harness(detection: &HarnessDetection, noun: &str, env_var: &str) -> String {
    let Some(root) = &detection.root else {
        return "not found".to_string();
    };
    let provenance = match detection.source {
        Some(SetupDiscoverySource::FlagOverride) => "via flag override".to_string(),
        Some(SetupDiscoverySource::EnvVar) => format!("via {env_var}"),
        Some(SetupDiscoverySource::SettingsFile) => "via settings.json autoMemoryDirectory".to_string(),
        Some(SetupDiscoverySource::Default) | None => "default location".to_string(),
    };
    if detection.candidates == 0 {
        return format!("none at {} ({provenance})", root.display());
    }
    format!("{} {noun}(s) at {} ({provenance})", detection.candidates, root.display())
}

fn describe_socket(state: SetupSocketState) -> String {
    match state {
        SetupSocketState::Live => "live — a daemon is already running".to_string(),
        SetupSocketState::Stale => "stale socket file (no daemon listening)".to_string(),
        SetupSocketState::Absent => "absent — fresh setup".to_string(),
    }
}

/// Present the shared 5-item harness-target `Select` (Current/Claude/Codex/All/
/// None, in that order) and map the chosen index to a [`HarnessSelection`].
///
/// `items` must list the five choices in the same Current→None order; the
/// index→variant mapping lives here so that ordering is a single source of
/// truth across `--harness`, `--wire-mcp`, and `--wire-hooks`. `default_ix` is
/// the highlighted choice; `fallback_ix` is used when the prompt cannot run
/// (e.g. Ctrl-D), matching each caller's prior `unwrap_or` default.
fn prompt_harness_selection(
    prompt: &str,
    items: &[&str; 5],
    default_ix: usize,
    fallback_ix: usize,
) -> HarnessSelection {
    let selection =
        dialoguer::Select::new().with_prompt(prompt).items(items).default(default_ix).interact().unwrap_or(fallback_ix);
    match selection {
        0 => HarnessSelection::Current,
        1 => HarnessSelection::Claude,
        2 => HarnessSelection::Codex,
        3 => HarnessSelection::All,
        _ => HarnessSelection::None,
    }
}

fn daemon_label(strategy: DaemonStrategy) -> &'static str {
    match strategy {
        DaemonStrategy::OnDemand => "on-demand",
        DaemonStrategy::Background => "background process",
        DaemonStrategy::Launchd => "launchd service",
        DaemonStrategy::None => "none",
    }
}

fn step_label(step: SetupStep) -> &'static str {
    match step {
        SetupStep::Detect => "detect",
        SetupStep::EnsureRepo => "repo",
        SetupStep::EnsureDaemon => "daemon",
        SetupStep::Import => "import",
        SetupStep::WireMcp => "MCP wiring",
        SetupStep::WireHooks => "hook wiring",
        SetupStep::Verify => "verify",
    }
}

fn status_label(status: SetupStepStatus) -> &'static str {
    match status {
        SetupStepStatus::Succeeded => "ok",
        SetupStepStatus::Failed => "FAILED",
        SetupStepStatus::Skipped => "skipped",
        // `Expected` marks a probe that "failed" by design (e.g. no socket
        // under an on-demand daemon); render it as fine rather than alarming.
        SetupStepStatus::Expected => "ok",
    }
}

/// Render the human closing summary: what each step did, then concrete next
/// steps. Pure so it can be unit-tested without a TTY.
fn render_epilogue(report: &SetupReport, repo: &Path, runtime: &Path, socket: &Path) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    let push = |out: &mut String, line: &str| {
        let _ = writeln!(out, "{line}");
    };

    push(&mut out, "");
    if report.decisions.print_only {
        push(&mut out, "Dry run — nothing was changed. Re-run `memoryd init` and opt in to apply.");
    }
    push(&mut out, "Setup summary:");
    for step in &report.steps {
        let first_line = step.message.as_deref().map(|m| m.lines().next().unwrap_or("")).unwrap_or("");
        let line = if first_line.is_empty() {
            format!("  {:<11} {}", step_label(step.step), status_label(step.status))
        } else {
            format!("  {:<11} {} — {}", step_label(step.step), status_label(step.status), first_line)
        };
        push(&mut out, &line);
    }

    push(&mut out, "");
    push(&mut out, "Next steps:");
    if report.restart_required {
        push(&mut out, "  - Restart your coding agent (Claude Code / Codex CLI) so the `memorum` MCP server loads.");
    }
    match report.decisions.daemon {
        DaemonStrategy::OnDemand => {
            push(&mut out, "  - Start the daemon when needed:");
            push(
                &mut out,
                &format!(
                    "      memoryd serve --repo \"{}\" --runtime \"{}\" --socket \"{}\"",
                    repo.display(),
                    runtime.display(),
                    socket.display()
                ),
            );
        }
        DaemonStrategy::Background => {
            push(&mut out, &format!("  - The daemon is running in the background (socket: {}).", socket.display()));
        }
        DaemonStrategy::Launchd => {
            push(&mut out, "  - launchd keeps the daemon running and restarts it at login.");
        }
        DaemonStrategy::None => {
            push(&mut out, "  - No daemon was arranged. Start one manually with `memoryd serve` when ready.");
        }
    }
    push(&mut out, &format!("  - Check health anytime: memoryd status --socket \"{}\"", socket.display()));
    push(
        &mut out,
        &format!("      and: memoryd doctor --repo \"{}\" --runtime \"{}\"", repo.display(), runtime.display()),
    );
    if matches!(report.decisions.wire_mcp, WireMcpSelection::None) {
        push(&mut out, "  - Wire an MCP client later with `memoryd init --wire-mcp <harness>` (docs/mcp-wiring.md).");
    } else if !report.decisions.print_only {
        push(
            &mut out,
            "  - First round-trip: ask your agent to call memory_write, then memory_search for the same text.",
        );
    }
    if matches!(report.decisions.wire_hooks, WireHooksSelection::None) {
        push(&mut out, "  - Wire passive recall later with `memoryd init --wire-hooks <harness>`.");
    } else if matches!(report.decisions.wire_hooks, WireHooksSelection::Codex | WireHooksSelection::All) {
        // Codex skips non-managed hooks until they are trusted — surface the
        // exact one-time step so passive recall actually fires.
        push(&mut out, "  - Codex hooks are inactive until trusted: open Codex, run `/hooks`, trust the Memorum hook.");
    }
    push(&mut out, "  - Something off? See docs/troubleshooting.md.");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::setup::{SetupDetectionOptions, SetupStepReport};

    /// Canned-answer `SetupIo` for unit tests. All fields are public so each
    /// test can set only the decisions it cares about.
    struct ScriptedIo {
        import: bool,
        harnesses: HarnessSelection,
        non_git_cwd: NonGitCwdDecision,
        wire_mcp: WireMcpSelection,
        wire_hooks: WireHooksSelection,
        daemon: DaemonStrategy,
        print_only: bool,
        notes: Vec<String>,
    }

    impl Default for ScriptedIo {
        fn default() -> Self {
            Self {
                import: false,
                harnesses: HarnessSelection::None,
                non_git_cwd: NonGitCwdDecision::Skip,
                wire_mcp: WireMcpSelection::None,
                // Default to declining hook wiring so the canned IO preserves
                // the existing decline-everything semantics; tests that exercise
                // wiring set this explicitly.
                wire_hooks: WireHooksSelection::None,
                daemon: DaemonStrategy::None,
                print_only: true,
                notes: Vec::new(),
            }
        }
    }

    impl SetupIo for ScriptedIo {
        fn confirm_import(&mut self, _detection: &SetupDetection) -> SetupResult<bool> {
            Ok(self.import)
        }

        fn choose_harnesses(&mut self, _detection: &SetupDetection) -> SetupResult<HarnessSelection> {
            Ok(self.harnesses)
        }

        fn choose_non_git_cwd_default(&mut self, _detection: &SetupDetection) -> SetupResult<NonGitCwdDecision> {
            Ok(self.non_git_cwd)
        }

        fn choose_mcp_wiring(&mut self, _detection: &SetupDetection) -> SetupResult<WireMcpSelection> {
            Ok(self.wire_mcp)
        }

        fn choose_hook_wiring(&mut self, _detection: &SetupDetection) -> SetupResult<WireHooksSelection> {
            Ok(self.wire_hooks)
        }

        fn choose_daemon_strategy(&mut self, _detection: &SetupDetection) -> SetupResult<DaemonStrategy> {
            Ok(self.daemon)
        }

        fn print_only(&mut self) -> SetupResult<bool> {
            Ok(self.print_only)
        }

        fn note(&mut self, message: &str) -> SetupResult<()> {
            self.notes.push(message.to_string());
            Ok(())
        }
    }

    fn scratch_args(repo: &std::path::Path) -> InitArgs {
        InitArgs {
            repo: Some(repo.to_path_buf()),
            runtime: Some(repo.join(".memoryd")),
            non_interactive: false,
            json: false,
            detect_only: false,
            import: false,
            harness: Some(crate::cli::HarnessTargetArg::None),
            non_git_cwd_default: Some(crate::cli::NonGitCwdDefault::Skip),
            wire_mcp: Some(crate::cli::HarnessTargetArg::None),
            wire_hooks: Some(crate::cli::HarnessTargetArg::None),
            daemon: Some(crate::cli::DaemonMode::None),
            print_only: false,
        }
    }

    fn empty_detection(repo: &std::path::Path) -> SetupDetection {
        let temp_claude = tempfile::tempdir().expect("tempdir");
        let temp_codex = tempfile::tempdir().expect("tempdir");
        let options = SetupDetectionOptions {
            claude_root_override: Some(temp_claude.path().to_path_buf()),
            codex_root_override: Some(temp_codex.path().to_path_buf()),
            socket_path: Some(repo.join("memoryd.sock")),
        };
        SetupDetection::run_with_options(options).expect("detection")
    }

    /// The shipped `InteractiveIo` must make declining every prompt a genuine
    /// no-op: when the user opts into nothing, `print_only()` returns `true`,
    /// which is what keeps `ensure_repo` from provisioning the substrate. This
    /// asserts the no-op is produced by the *decline decisions themselves*
    /// (`chose_*` all false), not by a hardcoded dry-run flag.
    #[test]
    fn dialoguer_io_decline_everything_is_print_only() {
        let mut io = InteractiveIo::default();
        assert!(io.print_only().expect("print_only"), "declining everything must run as a dry-run no-op");
    }

    /// Conversely, opting into any single action (import, daemon, or wiring)
    /// flips `InteractiveIo` out of no-op mode so the real steps run.
    #[test]
    fn dialoguer_io_any_action_runs_real_steps() {
        let mut import_only = InteractiveIo { chose_import: true, ..InteractiveIo::default() };
        assert!(!import_only.print_only().expect("print_only"), "opting into import must run real steps");

        let mut daemon_only = InteractiveIo { chose_daemon: true, ..InteractiveIo::default() };
        assert!(!daemon_only.print_only().expect("print_only"), "opting into a daemon must run real steps");

        let mut wiring_only = InteractiveIo { chose_wiring: true, ..InteractiveIo::default() };
        assert!(!wiring_only.print_only().expect("print_only"), "opting into MCP wiring must run real steps");
    }

    /// `--print-only` forces a dry run even when actions were opted into.
    #[test]
    fn print_only_seed_forces_dry_run() {
        let mut io = InteractiveIo {
            seeds: SeededDecisions { print_only: true, ..SeededDecisions::default() },
            chose_import: true,
            chose_daemon: true,
            chose_wiring: true,
            ..InteractiveIo::default()
        };
        assert!(io.print_only().expect("print_only"), "--print-only must force a dry run");
    }

    /// Seeded selector answers must be returned without touching a TTY — this
    /// test running headless under `cargo test` is itself the proof.
    #[test]
    fn seeded_decisions_answer_without_prompting() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut detection = empty_detection(temp.path());
        detection.codex.candidates = 1;

        let mut io = InteractiveIo {
            seeds: SeededDecisions {
                import: Some(true),
                harnesses: Some(HarnessSelection::All),
                non_git_cwd: Some(NonGitCwdDecision::Me),
                wire_mcp: Some(WireMcpSelection::Claude),
                wire_hooks: Some(WireHooksSelection::Claude),
                daemon: Some(DaemonStrategy::OnDemand),
                print_only: false,
            },
            ..InteractiveIo::default()
        };

        assert!(io.confirm_import(&detection).expect("confirm_import"));
        assert_eq!(io.choose_harnesses(&detection).expect("harnesses"), HarnessSelection::All);
        assert_eq!(io.choose_non_git_cwd_default(&detection).expect("non_git"), NonGitCwdDecision::Me);
        assert_eq!(io.choose_mcp_wiring(&detection).expect("wire"), WireMcpSelection::Claude);
        assert_eq!(io.choose_hook_wiring(&detection).expect("wire_hooks"), WireHooksSelection::Claude);
        assert_eq!(io.choose_daemon_strategy(&detection).expect("daemon"), DaemonStrategy::OnDemand);
        assert!(!io.print_only().expect("print_only"), "seeded opt-ins must run real steps");
    }

    /// An explicit `--import` seed cannot opt into import when discovery found
    /// no candidate memories; there is nothing real to import.
    #[test]
    fn seeded_import_with_no_candidates_skips_import() {
        let temp = tempfile::tempdir().expect("tempdir");
        let detection = empty_detection(temp.path());

        let mut io = InteractiveIo {
            seeds: SeededDecisions { import: Some(true), ..SeededDecisions::default() },
            chose_import: true,
            ..InteractiveIo::default()
        };

        assert!(!io.confirm_import(&detection).expect("confirm_import"));
        assert!(!io.chose_import, "no candidates must clear the import opt-in");
    }

    /// With no seeds and nothing detected, the import confirm declines without
    /// prompting — a fresh machine with no prior memory asks zero questions
    /// about import.
    #[test]
    fn no_candidates_skips_import_prompt() {
        let temp = tempfile::tempdir().expect("tempdir");
        let detection = empty_detection(temp.path());

        let mut io = InteractiveIo::default();
        assert!(!io.confirm_import(&detection).expect("confirm_import"));
        // Follow-up import questions are also skipped once import is declined.
        assert_eq!(io.choose_harnesses(&detection).expect("harnesses"), HarnessSelection::None);
        assert_eq!(io.choose_non_git_cwd_default(&detection).expect("non_git"), NonGitCwdDecision::Skip);
    }

    /// `from_args` maps explicit CLI flags into seeds and leaves omitted
    /// selectors unseeded (so they prompt).
    #[test]
    fn from_args_seeds_only_explicit_flags() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut args = scratch_args(temp.path());
        args.harness = None;
        args.daemon = Some(crate::cli::DaemonMode::Background);
        args.import = true;

        let repo = temp.path().to_path_buf();
        let runtime = repo.join(".memoryd");
        let socket = runtime.join("memoryd.sock");
        let io = InteractiveIo::from_args(&args, &repo, &runtime, &socket);

        assert_eq!(io.seeds.import, Some(true));
        assert!(io.seeds.harnesses.is_none(), "omitted --harness must stay unseeded");
        assert_eq!(io.seeds.daemon, Some(DaemonStrategy::Background));
        assert_eq!(io.seeds.wire_mcp, Some(WireMcpSelection::None));
        assert!(!io.seeds.print_only);
    }

    /// Engine-level proof that a decline-everything session (the decision shape
    /// the shipped `InteractiveIo` produces: nothing imported, no daemon, no
    /// wiring, and therefore `print_only = true`) creates no substrate. Uses
    /// `ScriptedIo` to drive the engine deterministically without a real TTY.
    #[tokio::test]
    async fn decline_everything_is_safe_noop() {
        let temp = tempfile::tempdir().expect("tempdir");
        let repo = temp.path().join("repo");

        // Exactly what InteractiveIo records for a decline-everything session.
        let io = ScriptedIo::default(); // import=false, daemon=None, wire=None, print_only=true
        run_with_io(scratch_args(&repo), io).await.expect("run_with_io succeeds");

        // No substrate directory must have been created.
        assert!(!repo.join(".memorum").exists(), "declining everything must not create the substrate directory");
    }

    /// A scripted io that opts into provisioning (import requested) with
    /// `print_only = false` must drive the real engine steps and create the
    /// substrate on disk — the complement of the decline-everything no-op.
    #[tokio::test]
    async fn scripted_io_drives_full_setup_path() {
        let temp = tempfile::tempdir().expect("tempdir");
        let repo = temp.path().join("repo");
        let runtime = repo.join(".memoryd");

        let io = ScriptedIo {
            import: true,
            harnesses: HarnessSelection::None, // no harness data to import in the scratch env
            non_git_cwd: NonGitCwdDecision::Me,
            wire_mcp: WireMcpSelection::None, // avoid touching real harness configs in CI
            wire_hooks: WireHooksSelection::None, // ditto: no real harness config edits in CI
            daemon: DaemonStrategy::None,
            print_only: false, // real run: provisions the substrate
            notes: Vec::new(),
        };

        let (repo_path, runtime_path) = (repo.clone(), runtime.clone());
        let engine = SetupEngine::new(&repo_path, &runtime_path);

        // Run via the engine directly so we can inspect the report.
        let temp_claude = tempfile::tempdir().expect("tempdir");
        let temp_codex = tempfile::tempdir().expect("tempdir");
        let options = SetupDetectionOptions {
            claude_root_override: Some(temp_claude.path().to_path_buf()),
            codex_root_override: Some(temp_codex.path().to_path_buf()),
            socket_path: Some(runtime_path.join("memoryd.sock")),
        };

        let mut io_mut = io;
        let report = engine.run_with_options(&mut io_mut, options).await.expect("engine runs");

        // Decisions are reflected in the report.
        assert!(report.decisions.import_memories, "import decision must be true");
        assert_eq!(report.decisions.non_git_cwd_default, NonGitCwdDecision::Me);
        assert_eq!(report.decisions.wire_mcp, WireMcpSelection::None);
        assert_eq!(report.decisions.daemon, DaemonStrategy::None);
        assert!(!report.decisions.print_only, "real run must not be print-only");

        // A non-print-only run actually provisions the substrate on disk.
        assert!(repo.join(".memorum").exists(), "a real (non-dry-run) setup must create the substrate directory");
    }

    /// The epilogue must surface the facts a first-time user needs: per-step
    /// outcomes, the restart requirement, and how to verify.
    #[test]
    fn epilogue_renders_steps_restart_and_verify_commands() {
        let temp = tempfile::tempdir().expect("tempdir");
        let detection = empty_detection(temp.path());
        let decisions = crate::setup::SetupDecisions {
            import_memories: true,
            harnesses: HarnessSelection::All,
            non_git_cwd_default: NonGitCwdDecision::Skip,
            wire_mcp: WireMcpSelection::All,
            wire_hooks: WireHooksSelection::All,
            daemon: DaemonStrategy::OnDemand,
            print_only: false,
        };
        let mut report = SetupReport::new(detection, decisions).with_restart_required(true);
        report
            .push_step(SetupStepReport::new(SetupStep::EnsureRepo, SetupStepStatus::Succeeded).with_message(
                "initialized Memorum repo at /tmp/x\nEmbedding model: Qwen3 (second line must not leak)",
            ));
        report.push_step(SetupStepReport::new(SetupStep::WireMcp, SetupStepStatus::Succeeded));

        let repo = Path::new("/tmp/x");
        let runtime = Path::new("/tmp/x/.memoryd");
        let socket = Path::new("/tmp/x/.memoryd/memoryd.sock");
        let rendered = render_epilogue(&report, repo, runtime, socket);

        assert!(rendered.contains("Setup summary:"), "{rendered}");
        assert!(rendered.contains("initialized Memorum repo"), "{rendered}");
        assert!(!rendered.contains("second line must not leak"), "messages must be truncated to one line: {rendered}");
        assert!(rendered.contains("Restart your coding agent"), "{rendered}");
        assert!(rendered.contains("memoryd status --socket"), "{rendered}");
        assert!(rendered.contains("memoryd serve --repo"), "on-demand daemon must include a start command: {rendered}");
        assert!(rendered.contains("memory_write"), "wired setups should point at the first round-trip: {rendered}");
        assert!(!rendered.contains("Dry run"), "{rendered}");
    }

    /// A print-only report leads with the dry-run banner, and a no-wiring run
    /// points at how to wire later instead of the MCP round-trip.
    #[test]
    fn epilogue_dry_run_and_unwired_variants() {
        let temp = tempfile::tempdir().expect("tempdir");
        let detection = empty_detection(temp.path());
        let decisions = crate::setup::SetupDecisions {
            import_memories: false,
            harnesses: HarnessSelection::None,
            non_git_cwd_default: NonGitCwdDecision::Skip,
            wire_mcp: WireMcpSelection::None,
            wire_hooks: WireHooksSelection::None,
            daemon: DaemonStrategy::None,
            print_only: true,
        };
        let report = SetupReport::new(detection, decisions);
        let rendered = render_epilogue(&report, Path::new("/r"), Path::new("/r/.memoryd"), Path::new("/r/s.sock"));

        assert!(rendered.contains("Dry run — nothing was changed"), "{rendered}");
        assert!(rendered.contains("--wire-mcp"), "unwired runs must say how to wire later: {rendered}");
        assert!(rendered.contains("No daemon was arranged"), "{rendered}");
    }

    /// Detection lines must say where memory was found *and how*: provenance is
    /// what makes nonstandard profile layouts (CLAUDE_CONFIG_DIR etc.) legible.
    #[test]
    fn describe_harness_includes_provenance() {
        let detection = HarnessDetection {
            root: Some(PathBuf::from("/home/u/.claude-personal/projects")),
            source: Some(SetupDiscoverySource::EnvVar),
            candidates: 3,
            parse_errors: 0,
        };
        let line = describe_harness(&detection, "topic file", "$CLAUDE_CONFIG_DIR");
        assert!(line.contains("3 topic file(s)"), "{line}");
        assert!(line.contains(".claude-personal/projects"), "{line}");
        assert!(line.contains("$CLAUDE_CONFIG_DIR"), "{line}");

        let missing = HarnessDetection { root: None, source: None, candidates: 0, parse_errors: 0 };
        assert_eq!(describe_harness(&missing, "topic file", "$CLAUDE_CONFIG_DIR"), "not found");
    }

    /// Verify that `InteractiveIo`'s `note` method succeeds without a real TTY.
    #[test]
    fn dialoguer_io_note_succeeds() {
        let mut io = InteractiveIo::default();
        let note_result = io.note("test diagnostic message");
        assert!(note_result.is_ok(), "InteractiveIo::note must succeed");
    }

    /// Verify the ScriptedIo returns expected values for all methods.
    #[test]
    fn scripted_io_returns_canned_answers() {
        let temp = tempfile::tempdir().expect("tempdir");
        let detection = empty_detection(temp.path());

        let mut io = ScriptedIo {
            import: true,
            harnesses: HarnessSelection::All,
            non_git_cwd: NonGitCwdDecision::Generate,
            wire_mcp: WireMcpSelection::All,
            wire_hooks: WireHooksSelection::All,
            daemon: DaemonStrategy::Background,
            print_only: false,
            notes: Vec::new(),
        };

        assert!(io.confirm_import(&detection).unwrap());
        assert_eq!(io.choose_harnesses(&detection).unwrap(), HarnessSelection::All);
        assert_eq!(io.choose_non_git_cwd_default(&detection).unwrap(), NonGitCwdDecision::Generate);
        assert_eq!(io.choose_mcp_wiring(&detection).unwrap(), WireMcpSelection::All);
        assert_eq!(io.choose_hook_wiring(&detection).unwrap(), WireHooksSelection::All);
        assert_eq!(io.choose_daemon_strategy(&detection).unwrap(), DaemonStrategy::Background);
        assert!(!io.print_only().unwrap());

        io.note("hello").unwrap();
        assert_eq!(io.notes, vec!["hello".to_string()]);
    }
}
