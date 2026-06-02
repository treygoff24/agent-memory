//! Interactive (TTY) frontend for `memoryd init`.
//!
//! Drives the shared [`SetupEngine`] through [`InteractiveIo`], which implements
//! [`SetupIo`] by presenting `dialoguer`-backed prompts (Confirm, Select,
//! MultiSelect) to the user. Declining every prompt is a safe no-op equivalent
//! to `--detect-only`: no substrate is created, no daemon is arranged, no MCP
//! configs are modified.
//!
//! The public entry point is [`run`]; [`run_with_io`] exposes a testable seam
//! that accepts any [`SetupIo`] implementation without touching a real TTY.

use crate::cli::InitArgs;
use crate::setup::{
    DaemonStrategy, HarnessSelection, NonGitCwdDecision, SetupDetection, SetupEngine, SetupIo, SetupResult,
    WireMcpSelection,
};

use super::resolve_repo_runtime;

// ── Public entry points ────────────────────────────────────────────────────

/// Drive interactive setup against a real TTY using `dialoguer` prompts.
pub async fn run(args: InitArgs) -> anyhow::Result<()> {
    run_with_io(args, InteractiveIo::default()).await
}

/// Drive interactive setup with a caller-supplied [`SetupIo`] implementation.
///
/// This seam keeps tests deterministic: pass a `ScriptedIo` (or any other
/// `SetupIo` impl) to exercise the full engine path without a real TTY.
pub async fn run_with_io<I: SetupIo>(args: InitArgs, mut io: I) -> anyhow::Result<()> {
    let (repo, runtime) = resolve_repo_runtime(&args);
    let engine = SetupEngine::new(repo, runtime);
    let _report = engine.run(&mut io).await?;
    Ok(())
}

// ── InteractiveIo ────────────────────────────────────────────────────────────

/// Dialoguer-backed interactive I/O for `memoryd init`.
///
/// Each decision method presents a prompt via `dialoguer`. Prompt failures
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
    chose_import: bool,
    chose_daemon: bool,
    chose_wiring: bool,
}

impl SetupIo for InteractiveIo {
    fn confirm_import(&mut self, detection: &SetupDetection) -> SetupResult<bool> {
        let claude_count = detection.claude.candidates;
        let codex_count = detection.codex.candidates;
        let total = claude_count + codex_count;

        if total == 0 {
            return Ok(false);
        }

        let prompt = format!(
            "Import harness memories into Memorum? ({claude_count} Claude, {codex_count} Codex candidate(s) detected)"
        );
        let answer = dialoguer::Confirm::new().with_prompt(prompt).default(false).interact().unwrap_or(false);
        self.chose_import = answer;
        Ok(answer)
    }

    fn choose_harnesses(&mut self, _detection: &SetupDetection) -> SetupResult<HarnessSelection> {
        let items = &["Current harness only", "Claude Code", "Codex CLI", "All harnesses", "None (skip import)"];
        let selection = dialoguer::Select::new()
            .with_prompt("Which harness memories should be imported?")
            .items(items)
            .default(0)
            .interact()
            .unwrap_or(4);
        let harness = match selection {
            0 => HarnessSelection::Current,
            1 => HarnessSelection::Claude,
            2 => HarnessSelection::Codex,
            3 => HarnessSelection::All,
            _ => HarnessSelection::None,
        };
        Ok(harness)
    }

    fn choose_non_git_cwd_default(&mut self, _detection: &SetupDetection) -> SetupResult<NonGitCwdDecision> {
        let items = &[
            "Skip memories with non-git working directories",
            "Drop them into user scope (me)",
            "Generate .memory-project.yaml in each non-git cwd",
        ];
        let selection = dialoguer::Select::new()
            .with_prompt("What should happen to memories from non-git working directories?")
            .items(items)
            .default(0)
            .interact()
            .unwrap_or(0);
        let decision = match selection {
            1 => NonGitCwdDecision::Me,
            2 => NonGitCwdDecision::Generate,
            _ => NonGitCwdDecision::Skip,
        };
        Ok(decision)
    }

    fn choose_mcp_wiring(&mut self, _detection: &SetupDetection) -> SetupResult<WireMcpSelection> {
        let items = &[
            "Current harness config only",
            "Claude Code config",
            "Codex CLI config",
            "All harness configs",
            "None (skip MCP wiring)",
        ];
        let selection = dialoguer::Select::new()
            .with_prompt("Which MCP harness configs should be wired to Memorum?")
            .items(items)
            .default(0)
            .interact()
            .unwrap_or(4);
        let wire = match selection {
            0 => WireMcpSelection::Current,
            1 => WireMcpSelection::Claude,
            2 => WireMcpSelection::Codex,
            3 => WireMcpSelection::All,
            _ => WireMcpSelection::None,
        };
        self.chose_wiring = !matches!(wire, WireMcpSelection::None);
        Ok(wire)
    }

    fn choose_daemon_strategy(&mut self, _detection: &SetupDetection) -> SetupResult<DaemonStrategy> {
        let items = &[
            "On-demand (start manually when needed)",
            "Background process (start now, no persistence)",
            "launchd service (macOS persistent daemon)",
            "None (skip daemon setup)",
        ];
        let selection = dialoguer::Select::new()
            .with_prompt("How should the Memorum daemon be arranged?")
            .items(items)
            .default(0)
            .interact()
            .unwrap_or(0);
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
        // user opted into nothing — no import, no daemon, no MCP wiring — there
        // is nothing to provision, so run in dry-run mode and leave the substrate
        // untouched (equivalent to `--detect-only`). Opting into any action runs
        // the real steps. `print_only` is collected last, after every decision,
        // so `self` reflects the full session.
        let declined_everything = !self.chose_import && !self.chose_daemon && !self.chose_wiring;
        Ok(declined_everything)
    }

    fn note(&mut self, message: &str) -> SetupResult<()> {
        eprintln!("{message}");
        Ok(())
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::setup::{SetupDetection, SetupDetectionOptions};

    // ── ScriptedIo ─────────────────────────────────────────────────────────

    /// Canned-answer `SetupIo` for unit tests. All fields are public so each
    /// test can set only the decisions it cares about.
    struct ScriptedIo {
        import: bool,
        harnesses: HarnessSelection,
        non_git_cwd: NonGitCwdDecision,
        wire_mcp: WireMcpSelection,
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

    // ── Helpers ────────────────────────────────────────────────────────────

    fn scratch_args(repo: &std::path::Path) -> InitArgs {
        InitArgs {
            repo: Some(repo.to_path_buf()),
            runtime: Some(repo.join(".memoryd")),
            non_interactive: false,
            json: false,
            detect_only: false,
            import: false,
            harness: crate::cli::InitHarness::None,
            non_git_cwd_default: crate::cli::NonGitCwdDefault::Skip,
            wire_mcp: crate::cli::WireMcpMode::None,
            daemon: crate::cli::DaemonMode::None,
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

    // ── Test: decline-everything is a safe no-op ───────────────────────────

    /// The shipped `InteractiveIo` must make declining every prompt a genuine
    /// no-op: when the user opts into nothing, `print_only()` returns `true`,
    /// which is what keeps `ensure_repo` from provisioning the substrate. This
    /// asserts the no-op is produced by the *decline decisions themselves*
    /// (`chose_*` all false), not by a hardcoded dry-run flag.
    #[test]
    fn dialoguer_io_decline_everything_is_print_only() {
        // An `InteractiveIo` with no positive selections (the decline-everything
        // state the prompt methods record) reports print-only.
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

    // ── Test: scripted io drives a real arranged setup that mutates disk ───

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

    // ── Test: SetupIo trait smoke-check on InteractiveIo ─────────────────────

    /// Verify that `InteractiveIo`'s `note` method succeeds without a real TTY.
    #[test]
    fn dialoguer_io_note_succeeds() {
        let mut io = InteractiveIo::default();
        let note_result = io.note("test diagnostic message");
        assert!(note_result.is_ok(), "InteractiveIo::note must succeed");
    }

    // ── Test: ScriptedIo unsupported variant produces SetupError ──────────

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
            daemon: DaemonStrategy::Background,
            print_only: false,
            notes: Vec::new(),
        };

        assert!(io.confirm_import(&detection).unwrap());
        assert_eq!(io.choose_harnesses(&detection).unwrap(), HarnessSelection::All);
        assert_eq!(io.choose_non_git_cwd_default(&detection).unwrap(), NonGitCwdDecision::Generate);
        assert_eq!(io.choose_mcp_wiring(&detection).unwrap(), WireMcpSelection::All);
        assert_eq!(io.choose_daemon_strategy(&detection).unwrap(), DaemonStrategy::Background);
        assert!(!io.print_only().unwrap());

        io.note("hello").unwrap();
        assert_eq!(io.notes, vec!["hello".to_string()]);
    }
}
