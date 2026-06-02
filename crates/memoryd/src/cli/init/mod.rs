//! `memoryd init` dispatch.
//!
//! Three frontends share the substrate setup engine:
//!
//! - [`agent`] — non-interactive / machine path. Drives the engine from flags
//!   and emits a JSON `SetupReport` on stdout (diagnostics on stderr).
//! - [`interactive`] — TTY path. Drives the shared engine through `DialoguerIo`,
//!   presenting `dialoguer` prompts for each setup decision.
//! - `detect_and_advise` — the legacy detect-and-advise advisory output kept
//!   for backward compatibility on a bare interactive `memoryd init`.
//!
//! Dispatch rules:
//! 1. `--detect-only` always runs the side-effect-free detection path (agent).
//! 2. `--non-interactive` / `--json` force the machine path (agent).
//! 3. Otherwise, when stdin is a TTY: a bare invocation (no action flags) keeps
//!    today's detect-and-advise output; an invocation that requests engine
//!    action (e.g. `--import`, `--print-only`) routes to the interactive stub.
//!    A non-default mutating selector (`--harness`/`--wire-mcp`/`--daemon`) on
//!    the advisory path is not yet honored interactively, so it emits a warning
//!    pointing at `--non-interactive` rather than being silently dropped.
//! 4. When stdin is not a TTY, route to the agent path.

pub mod agent;
pub mod interactive;

use std::io::IsTerminal;
use std::path::PathBuf;

use super::{DaemonMode, InitArgs, InitHarness, WireMcpMode};
use crate::import::discovery::{discover_claude_memory_root, discover_codex_memory_root};

/// Dispatch `memoryd init` to the right frontend.
pub async fn run(args: InitArgs) -> anyhow::Result<()> {
    // Detection is explicit and side-effect-free, so it bypasses TTY routing
    // and always uses the agent (JSON) path.
    if args.detect_only {
        return agent::run(args).await;
    }

    // Explicit machine output requested. `--non-interactive` and `--json` are
    // interchangeable here: both force the agent path, which always emits JSON.
    // They diverge only on a TTY, where `--json` alone forces the machine path
    // for a caller that wants structured output without disabling prompts
    // wholesale. On every non-TTY invocation (the common piped/CI case) either
    // flag has the identical effect.
    if args.non_interactive || args.json {
        return agent::run(args).await;
    }

    if std::io::stdin().is_terminal() {
        if requests_engine_action(&args) {
            // TTY + action flags: hand off to the interactive frontend, which
            // drives the engine via `DialoguerIo` prompts.
            return interactive::run(args).await;
        }
        // Bare interactive invocation keeps the legacy advisory output. The
        // mutating selectors (`--harness`/`--wire-mcp`/`--daemon`) are only
        // honored on the engine paths, so warn rather than silently drop them
        // when a non-default selector reaches the advisory path.
        warn_ignored_mutating_selectors(&args);
        return detect_and_advise(args).await;
    }

    // Non-TTY without explicit machine flags still routes to the agent path so
    // piped callers get a deterministic JSON report rather than prompts.
    agent::run(args).await
}

/// Whether the invocation asked the engine to take an action beyond detection.
///
/// The mode selectors (`--harness`, `--wire-mcp`, `--daemon`) carry defaults and
/// do not mutate anything on their own; only `--import` and `--print-only`
/// express action intent on the interactive path.
fn requests_engine_action(args: &InitArgs) -> bool {
    args.import || args.print_only
}

/// Warn when a non-default mutating selector reaches the legacy advisory path.
///
/// On a bare interactive `memoryd init`, the advisory path neither provisions a
/// daemon nor wires MCP. A user who passes `--daemon background` or `--wire-mcp
/// all` expecting those to take effect would otherwise get no signal that the
/// flag was ignored. The interactive path (`DialoguerIo`) is only reached via
/// `--import`/`--print-only` and presents prompts rather than honoring these mode
/// selectors as flags, so on the bare advisory path surface a clear note that
/// they require `--non-interactive` (or `--json`).
fn warn_ignored_mutating_selectors(args: &InitArgs) {
    let ignored = ignored_mutating_selectors(args);
    if ignored.is_empty() {
        return;
    }

    eprintln!(
        "warning: the interactive path does not honor {} yet; it only prints guidance. \
         Re-run with --non-interactive (or --json) to apply these selectors.",
        ignored.join(", ")
    );
}

/// The non-default mutating selectors that the advisory path will silently
/// ignore. Empty when only defaults were supplied.
fn ignored_mutating_selectors(args: &InitArgs) -> Vec<String> {
    let mut ignored = Vec::new();
    if args.harness != InitHarness::Current {
        ignored.push(format!("--harness {}", harness_flag_value(args.harness)));
    }
    if args.wire_mcp != WireMcpMode::Current {
        ignored.push(format!("--wire-mcp {}", wire_mcp_flag_value(args.wire_mcp)));
    }
    if args.daemon != DaemonMode::OnDemand {
        ignored.push(format!("--daemon {}", daemon_flag_value(args.daemon)));
    }
    ignored
}

fn harness_flag_value(harness: InitHarness) -> &'static str {
    match harness {
        InitHarness::Current => "current",
        InitHarness::Claude => "claude",
        InitHarness::Codex => "codex",
        InitHarness::All => "all",
        InitHarness::None => "none",
    }
}

fn wire_mcp_flag_value(mode: WireMcpMode) -> &'static str {
    match mode {
        WireMcpMode::Current => "current",
        WireMcpMode::Claude => "claude",
        WireMcpMode::Codex => "codex",
        WireMcpMode::All => "all",
        WireMcpMode::None => "none",
    }
}

fn daemon_flag_value(mode: DaemonMode) -> &'static str {
    match mode {
        DaemonMode::OnDemand => "on-demand",
        DaemonMode::Background => "background",
        DaemonMode::Launchd => "launchd",
        DaemonMode::None => "none",
    }
}

/// Resolve the canonical repo root and per-device runtime directory.
///
/// Mirrors `scripts/install-memorum.sh`: `--repo` flag → `$MEMORUM_REPO` →
/// `~/memorum`, with runtime defaulting to `<repo>/.memoryd`.
pub(crate) fn resolve_repo_runtime(args: &InitArgs) -> (PathBuf, PathBuf) {
    let default_repo = std::env::var("MEMORUM_REPO")
        .ok()
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join("memorum")))
        .unwrap_or_else(|| PathBuf::from("./memorum"));
    let repo = args.repo.clone().unwrap_or(default_repo);
    let runtime = args.runtime.clone().unwrap_or_else(|| repo.join(".memoryd"));
    (repo, runtime)
}

/// Legacy detect-and-advise output. Preserved for backward compatibility on a
/// bare interactive `memoryd init`; emits guidance to stdout and never mutates
/// the substrate.
async fn detect_and_advise(args: InitArgs) -> anyhow::Result<()> {
    let (repo, runtime) = resolve_repo_runtime(&args);
    let socket = runtime.join("memoryd.sock");

    println!("Memorum init");
    println!("  repo:    {}", repo.display());
    println!("  runtime: {}", runtime.display());
    println!("  socket:  {}", socket.display());
    println!();

    let already_initialised = repo.join(".memorum").exists();
    if already_initialised {
        println!("Detected existing Memorum substrate at {}.", repo.display());
        println!("Running detection-only: no re-init, no destructive changes.");
        println!("If you want to re-import harness memory, run `memoryd import` explicitly.");
        println!();
    }

    let claude_root = discover_claude_memory_root(None)?;
    let codex_root = discover_codex_memory_root(None)?;

    let claude_count = match &claude_root {
        Some(root) if root.path.exists() => walkdir::WalkDir::new(&root.path)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file())
            .filter(|entry| entry.path().extension().and_then(std::ffi::OsStr::to_str) == Some("md"))
            .filter(|entry| entry.path().file_name().and_then(std::ffi::OsStr::to_str) != Some("MEMORY.md"))
            .count(),
        _ => 0,
    };
    let codex_present = match &codex_root {
        Some(root) => root.path.join("MEMORY.md").exists(),
        None => false,
    };

    println!("Detected harness memory:");
    println!("  Claude Code: {claude_count} memory topic file(s)");
    println!("  Codex CLI:   {}", if codex_present { "MEMORY.md present" } else { "not found" });
    println!();

    let any = claude_count > 0 || codex_present;
    if !any {
        println!(
            "Nothing to import. Run `memoryd serve --init --repo \"{}\" --runtime \"{}\"` to start the daemon.",
            repo.display(),
            runtime.display()
        );
        return Ok(());
    }

    let proceed = dialoguer::Confirm::new()
        .with_prompt("Would you like to import detected harness memory now?")
        .default(true)
        .interact()
        .unwrap_or(false);

    if !proceed {
        println!("Skipped import. Run `memoryd import` later when ready.");
        return Ok(());
    }

    println!();
    println!("Run this command in a separate shell once the daemon is up:");
    println!("  memoryd import --repo \"{}\" --socket \"{}\"", repo.display(), socket.display(),);
    println!();
    println!("Next steps:");
    println!(
        "  - Start daemon: memoryd serve --init --repo \"{}\" --runtime \"{}\" --socket \"{}\"",
        repo.display(),
        runtime.display(),
        socket.display()
    );
    println!("  - Health check: memoryd doctor --repo \"{}\" --runtime \"{}\"", repo.display(), runtime.display());
    println!("  - Troubleshooting: docs/troubleshooting.md");
    println!("  - Importer details: docs/importer.md");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::NonGitCwdDefault;
    use serial_test::serial;

    fn args() -> InitArgs {
        InitArgs {
            repo: None,
            runtime: None,
            non_interactive: false,
            json: false,
            detect_only: false,
            import: false,
            harness: InitHarness::Current,
            non_git_cwd_default: NonGitCwdDefault::Skip,
            wire_mcp: WireMcpMode::Current,
            daemon: DaemonMode::OnDemand,
            print_only: false,
        }
    }

    #[test]
    fn resolve_repo_runtime_prefers_explicit_flags() {
        let mut input = args();
        input.repo = Some(PathBuf::from("/tmp/explicit-repo"));
        input.runtime = Some(PathBuf::from("/tmp/explicit-runtime"));

        let (repo, runtime) = resolve_repo_runtime(&input);
        assert_eq!(repo, PathBuf::from("/tmp/explicit-repo"));
        assert_eq!(runtime, PathBuf::from("/tmp/explicit-runtime"));
    }

    #[test]
    fn resolve_repo_runtime_defaults_runtime_under_repo() {
        let mut input = args();
        input.repo = Some(PathBuf::from("/tmp/explicit-repo"));

        let (repo, runtime) = resolve_repo_runtime(&input);
        assert_eq!(repo, PathBuf::from("/tmp/explicit-repo"));
        assert_eq!(runtime, PathBuf::from("/tmp/explicit-repo/.memoryd"));
    }

    #[test]
    #[serial]
    fn resolve_repo_runtime_falls_back_to_memorum_repo_env() {
        // Serialized via `#[serial]` so no other test reads the env
        // concurrently; the original value is restored before returning.
        let previous = std::env::var_os("MEMORUM_REPO");
        std::env::set_var("MEMORUM_REPO", "/tmp/env-repo");

        let (repo, runtime) = resolve_repo_runtime(&args());
        assert_eq!(repo, PathBuf::from("/tmp/env-repo"));
        assert_eq!(runtime, PathBuf::from("/tmp/env-repo/.memoryd"));

        match previous {
            Some(value) => std::env::set_var("MEMORUM_REPO", value),
            None => std::env::remove_var("MEMORUM_REPO"),
        }
    }

    #[test]
    fn no_warning_for_default_only_selectors() {
        assert!(ignored_mutating_selectors(&args()).is_empty());
    }

    #[test]
    fn non_default_mutating_selectors_are_flagged() {
        let mut input = args();
        input.daemon = DaemonMode::Background;
        input.wire_mcp = WireMcpMode::All;

        let ignored = ignored_mutating_selectors(&input);
        assert!(ignored.iter().any(|entry| entry == "--daemon background"), "{ignored:?}");
        assert!(ignored.iter().any(|entry| entry == "--wire-mcp all"), "{ignored:?}");
        assert_eq!(ignored.len(), 2, "only non-default selectors are flagged: {ignored:?}");
    }
}
