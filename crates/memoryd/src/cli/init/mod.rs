//! `memoryd init` dispatch.
//!
//! Two frontends share the substrate setup engine:
//!
//! - [`agent`] — non-interactive / machine path. Drives the engine from flags
//!   and emits a JSON `SetupReport` on stdout (diagnostics on stderr).
//! - [`interactive`] — TTY path. Drives the shared engine through
//!   `InteractiveIo`, presenting `dialoguer` prompts for each setup decision.
//!   Explicitly passed selector flags (`--import`, `--harness`, `--wire-mcp`,
//!   `--daemon`, `--non-git-cwd-default`, `--print-only`) pre-answer their
//!   prompt instead of being re-asked.
//!
//! Dispatch rules:
//! 1. `--detect-only` always runs the side-effect-free detection path (agent).
//! 2. `--non-interactive` / `--json` force the machine path (agent).
//! 3. Otherwise, when stdin and stderr are TTYs, run the interactive wizard.
//!    Declining every prompt is a guaranteed no-op (see `InteractiveIo`).
//! 4. When stdin or stderr is not a TTY and no machine mode was selected, refuse with
//!    guidance instead of mutating anything. A piped/CI caller must opt into
//!    the scripted path explicitly; silent provisioning from a bare `init` in
//!    a non-TTY shell is exactly the trap this rule exists to close.

pub mod agent;
pub mod interactive;

use std::io::IsTerminal;
use std::path::PathBuf;

use super::InitArgs;

/// Dispatch `memoryd init` to the right frontend.
pub async fn run(args: InitArgs) -> anyhow::Result<()> {
    // Detection is explicit and side-effect-free, so it bypasses TTY routing
    // and always uses the agent (JSON) path.
    if args.detect_only {
        return agent::run(args).await;
    }

    // Explicit machine output requested. `--non-interactive` and `--json` are
    // interchangeable here: both force the agent path, which always emits JSON.
    if args.non_interactive || args.json {
        return agent::run(args).await;
    }

    if std::io::stdin().is_terminal() && std::io::stderr().is_terminal() {
        return interactive::run(args).await;
    }

    // Non-TTY without an explicit machine mode: refuse rather than guess. The
    // agent path provisions a substrate from flag defaults, which is never an
    // acceptable side effect of a bare `memoryd init` in a pipe or CI job.
    anyhow::bail!(
        "memoryd init: this terminal cannot run the interactive wizard (stdin and stderr must both be a TTY).\n\
         \n\
         Pick an explicit mode instead:\n\
         \n\
         \x20 memoryd init --detect-only\n\
         \x20     Inspect what is present (read-only, JSON on stdout).\n\
         \n\
         \x20 memoryd init --non-interactive --json [flags]\n\
         \x20     Scripted setup driven entirely by flags; emits a JSON SetupReport.\n\
         \x20     Example: memoryd init --non-interactive --json --import --wire-mcp current --daemon on-demand\n\
         \n\
         AI agents installing Memorum for a user should follow docs/agent-onboarding.md."
    );
}

/// Resolve the canonical repo root and per-device runtime directory.
///
/// Mirrors `scripts/install-memorum.sh`: `--repo` flag → `$MEMORUM_REPO` →
/// `~/memorum`, with runtime defaulting to `<repo>/.memoryd`.
pub(crate) fn resolve_repo_runtime(args: &InitArgs) -> (PathBuf, PathBuf) {
    crate::cli::paths::resolve_repo_runtime_paths(args.repo.clone(), args.runtime.clone())
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
            harness: None,
            non_git_cwd_default: Some(NonGitCwdDefault::Skip),
            wire_mcp: None,
            daemon: None,
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
}
