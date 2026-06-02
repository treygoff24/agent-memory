//! `memoryd init` dispatch.
//!
//! Three frontends share the substrate setup engine:
//!
//! - [`agent`] — non-interactive / machine path. Drives the engine from flags
//!   and emits a JSON `SetupReport` on stdout (diagnostics on stderr).
//! - [`interactive`] — TTY path. STUB until T05; today it short-circuits via
//!   `InteractiveIo` (`SetupError::Unsupported`).
//! - [`detect_and_advise`] — the legacy detect-and-advise advisory output kept
//!   for backward compatibility on a bare interactive `memoryd init`.
//!
//! Dispatch rules:
//! 1. `--detect-only` always runs the side-effect-free detection path (agent).
//! 2. `--non-interactive` / `--json` force the machine path (agent).
//! 3. Otherwise, when stdin is a TTY: a bare invocation (no action flags) keeps
//!    today's detect-and-advise output; an invocation that requests engine
//!    action (e.g. `--import`, `--print-only`) routes to the interactive stub.
//! 4. When stdin is not a TTY, route to the agent path.

pub mod agent;
pub mod interactive;

use std::io::IsTerminal;
use std::path::PathBuf;

use super::InitArgs;
use crate::import::discovery::{discover_claude_memory_root, discover_codex_memory_root};

/// Dispatch `memoryd init` to the right frontend.
pub async fn run(args: InitArgs) -> anyhow::Result<()> {
    // Detection is explicit and side-effect-free, so it bypasses TTY routing
    // and always uses the agent (JSON) path.
    if args.detect_only {
        return agent::run(args).await;
    }

    // Explicit machine output requested.
    if args.non_interactive || args.json {
        return agent::run(args).await;
    }

    if std::io::stdin().is_terminal() {
        if requests_engine_action(&args) {
            // TTY + action flags: hand off to the interactive frontend. T05
            // implements the prompts; until then `InteractiveIo` rejects the
            // decision prompts with `SetupError::Unsupported`.
            return interactive::run(args).await;
        }
        // Bare interactive invocation keeps the legacy advisory output.
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
