//! Explicit argv git execution with environment sanitization.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use crate::error::GitError;

/// Git environment variables that could override `--git-dir` / `--work-tree`
/// and must be cleared before every invocation (spec §13.1 footnote).
const GIT_ENV_CLEAR: &[&str] = &["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_OBJECT_DIRECTORY", "GIT_NAMESPACE"];

/// Absolute path to the `git` binary resolved once at first use.
///
/// `which::which("git")` is used rather than `"git"` so the subprocess does
/// not depend on the ambient `PATH` ordering of any parent shell.
static GIT_BINARY: OnceLock<PathBuf> = OnceLock::new();

fn git_binary() -> &'static Path {
    GIT_BINARY.get_or_init(|| which::which("git").unwrap_or_else(|_| PathBuf::from("git")))
}

/// Run git with explicit args in a validated repo root.
///
/// The following environment variables are cleared before invocation:
/// `GIT_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE`, `GIT_OBJECT_DIRECTORY`,
/// `GIT_NAMESPACE`. This prevents parent-shell overrides from redirecting git
/// operations to a different repository (spec §13.1 footnote).
pub fn run_git(repo: &Path, args: &[&str]) -> Result<String, GitError> {
    if !repo.is_dir() {
        return Err(GitError::InvalidRepoRoot(repo.display().to_string()));
    }
    let mut cmd = Command::new(git_binary());
    cmd.args(args).current_dir(repo);
    for var in GIT_ENV_CLEAR {
        cmd.env_remove(var);
    }
    let output = cmd.output()?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(GitError::CommandFailed {
            program: "git".to_string(),
            args: args.iter().map(|arg| (*arg).to_string()).collect(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }
}
