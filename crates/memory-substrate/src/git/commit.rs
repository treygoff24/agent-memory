//! Auto-commit helper.

use std::path::Path;

use crate::error::GitError;
use crate::git::run_git;

/// Spec §5.1 namespaces that `auto_commit` is allowed to stage.
///
/// Anything outside this list must not be staged (spec §13.4 step 3).
const STAGED_NAMESPACES: &[&str] = &[
    "me/",
    "projects/",
    "agent/",
    "dreams/",
    "encrypted/",
    "substrate/",
    "events/",
    "tombstones/",
    "policies/",
    "leases/",
];

/// Root-level bootstrap files that are always tracked.
const STAGED_ROOT_FILES: &[&str] = &["config.yaml", ".gitattributes", ".gitignore"];

/// Outcome of a git commit operation.
#[derive(Debug, Eq, PartialEq)]
pub enum CommitOutcome {
    /// Nothing to commit; index was clean.
    NoChanges,
    /// Commit succeeded with the returned SHA.
    Committed {
        /// Short SHA of the new commit.
        sha: String,
    },
}

/// Commit current substrate changes.
///
/// Returns `Ok(())` when the commit succeeds or when there is nothing to
/// commit. Propagates `Err(GitError::CommandFailed)` on real git failures
/// (signed-commit rejection, pre-commit hook, locked index, etc.).
///
/// This is the `api.rs`-compatible surface (returns `Result<(), GitError>`).
/// Internal callers that need the typed outcome use `auto_commit_with_outcome`.
pub fn auto_commit(repo: &Path, message: &str) -> Result<(), GitError> {
    auto_commit_with_outcome(repo, message).map(|_| ())
}

/// Commit and return a typed `CommitOutcome`.
///
/// Distinguishes "nothing to commit" (clean index) from real commit failures.
pub fn auto_commit_with_outcome(repo: &Path, message: &str) -> Result<CommitOutcome, GitError> {
    stage_spec_namespaces(repo)?;

    if nothing_to_commit(repo)? {
        return Ok(CommitOutcome::NoChanges);
    }

    let sha = run_commit(repo, message)?;
    Ok(CommitOutcome::Committed { sha })
}

/// Stage only the spec §5.1 namespaces and bootstrap root files.
fn stage_spec_namespaces(repo: &Path) -> Result<(), GitError> {
    let mut full_args = vec!["add", "--"];
    full_args.extend(staged_paths());
    // `git add` with non-existent paths exits non-zero only when the path was
    // required; with globs it succeeds silently. We pass explicit names that
    // exist on disk so failures are real failures.
    run_git(repo, &full_args).map(|_| ())
}

/// Return true when staging the spec namespaces produced no staged changes.
fn nothing_to_commit(repo: &Path) -> Result<bool, GitError> {
    let mut args = vec!["diff", "--cached", "--name-only", "--"];
    args.extend(staged_paths());
    let output = run_git(repo, &args)?;
    Ok(output.trim().is_empty())
}

/// Run `git commit` and return the short SHA of the new commit.
fn run_commit(repo: &Path, message: &str) -> Result<String, GitError> {
    run_git(repo, &["commit", "-m", message])?;
    run_git(repo, &["rev-parse", "--short", "HEAD"]).map(|sha| sha.trim().to_string())
}

fn staged_paths() -> Vec<&'static str> {
    let mut paths = Vec::with_capacity(STAGED_NAMESPACES.len() + STAGED_ROOT_FILES.len());
    paths.extend_from_slice(STAGED_NAMESPACES);
    paths.extend_from_slice(STAGED_ROOT_FILES);
    paths
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{auto_commit_with_outcome, CommitOutcome};
    use crate::tree::bootstrap_repo_tree;

    #[test]
    fn unrelated_dirty_files_do_not_force_a_commit() {
        let repo = tempdir().expect("tempdir"); // expect-justified: test setup
        bootstrap_repo_tree(repo.path()).expect("bootstrap"); // expect-justified: test setup
        git(repo.path(), &["init"]).expect("git init"); // expect-justified: test setup
        git(repo.path(), &["config", "user.email", "codex@example.com"]).expect("git email"); // expect-justified: test setup
        git(repo.path(), &["config", "user.name", "Codex"]).expect("git name"); // expect-justified: test setup
        let first = auto_commit_with_outcome(repo.path(), "Stream A auto-commit").expect("baseline commit"); // expect-justified: test setup
        let baseline_sha = match first {
            CommitOutcome::Committed { sha } => sha,
            CommitOutcome::NoChanges => panic!("baseline bootstrap commit should create HEAD"),
        };
        fs::write(repo.path().join("scratch.txt"), "noise").expect("scratch"); // expect-justified: test setup

        let outcome = auto_commit_with_outcome(repo.path(), "Stream A auto-commit").expect("auto commit"); // expect-justified: test assertion

        assert_eq!(outcome, CommitOutcome::NoChanges);
        assert_eq!(git(repo.path(), &["rev-parse", "--short", "HEAD"]).expect("head sha"), baseline_sha);
        // expect-justified: test assertion
    }

    #[test]
    fn staged_spec_changes_are_committed_even_with_unrelated_dirty_files() {
        let repo = tempdir().expect("tempdir"); // expect-justified: test setup
        bootstrap_repo_tree(repo.path()).expect("bootstrap"); // expect-justified: test setup
        git(repo.path(), &["init"]).expect("git init"); // expect-justified: test setup
        git(repo.path(), &["config", "user.email", "codex@example.com"]).expect("git email"); // expect-justified: test setup
        git(repo.path(), &["config", "user.name", "Codex"]).expect("git name"); // expect-justified: test setup
        let baseline = auto_commit_with_outcome(repo.path(), "Stream A auto-commit").expect("baseline commit"); // expect-justified: test setup
        assert!(matches!(baseline, CommitOutcome::Committed { .. }));
        fs::write(repo.path().join("config.yaml"), "schema_version: 1\nactive_embedding:\n  provider: synthetic\n  model_ref: stream-a-test\n  dimension: 64\n")
            .expect("config"); // expect-justified: test setup
        fs::write(repo.path().join("scratch.txt"), "noise").expect("scratch"); // expect-justified: test setup

        let outcome = auto_commit_with_outcome(repo.path(), "Stream A auto-commit").expect("auto commit"); // expect-justified: test assertion

        assert!(matches!(outcome, CommitOutcome::Committed { .. }));
    }

    fn git(repo: &std::path::Path, args: &[&str]) -> Result<String, String> {
        let output =
            std::process::Command::new("git").args(args).current_dir(repo).output().map_err(|err| err.to_string())?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).to_string())
        }
    }
}
