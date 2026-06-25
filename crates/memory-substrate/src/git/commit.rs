//! Auto-commit helper.

use std::path::Path;
use std::process::Command;

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
    "sources/",
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

/// Lease commit action used in the fixed Stream F lease commit message.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum LeaseCommitAction {
    /// A lease record was acquired.
    Acquire,
    /// A lease record was released.
    Release,
}

impl LeaseCommitAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::Acquire => "acquire",
            Self::Release => "release",
        }
    }
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

/// Commit daemon substrate writes with the fixed write-bot identity.
///
/// This stages only daemon-managed substrate namespaces, explicitly excludes the
/// lease journal, and never pushes.
pub fn commit_substrate_writes(repo: &Path, write_count: usize) -> Result<CommitOutcome, GitError> {
    stage_spec_namespaces(repo)?;
    run_git(repo, &["reset", "-q", "--", "leases/journal.lease"])?;

    if nothing_to_commit(repo)? {
        return Ok(CommitOutcome::NoChanges);
    }

    let message = format!("substrate: commit {write_count} write(s)");
    run_write_bot_commit(repo, &message)?;
    let sha = run_git(repo, &["rev-parse", "--short", "HEAD"])?.trim().to_string();
    Ok(CommitOutcome::Committed { sha })
}

/// Repo-relative daemon-managed paths a substrate write commit would stage —
/// the staged namespaces, untracked included, with `leases/journal.lease` excluded
/// (it is committed only by `commit_lease_file`). Returned so callers like the
/// doctor's D3 stale-uncommitted check can stat their mtimes without re-deriving
/// the namespace + journal-exclusion logic.
pub fn uncommitted_substrate_paths(repo: &Path) -> Result<Vec<String>, GitError> {
    let mut args = vec!["status", "--porcelain=v1", "--untracked-files=all", "--"];
    args.extend(staged_paths());
    let output = run_git(repo, &args)?;
    Ok(output
        .lines()
        .filter_map(status_path)
        .filter(|path| *path != "leases/journal.lease")
        .map(str::to_string)
        .collect())
}

/// Count changed paths a substrate write commit would be allowed to stage.
pub fn count_substrate_write_changes(repo: &Path) -> Result<usize, GitError> {
    Ok(uncommitted_substrate_paths(repo)?.len())
}

/// Commit only `leases/journal.lease` with the fixed Stream F lease-bot identity.
///
/// This helper intentionally does not reuse [`auto_commit_with_outcome`]: lease
/// acquisition must never stage broad daemon-owned namespaces, because the
/// lease commit is the concurrency primitive that protects those later writes.
pub fn commit_lease_file(
    repo: &Path,
    action: LeaseCommitAction,
    scope: &str,
    device_id: &str,
) -> Result<CommitOutcome, GitError> {
    const LEASE_FILE: &str = "leases/journal.lease";

    run_git(repo, &["add", "--", LEASE_FILE])?;
    let changed = run_git(repo, &["diff", "--cached", "--name-only", "--", LEASE_FILE])?;
    if changed.trim().is_empty() {
        return Ok(CommitOutcome::NoChanges);
    }

    let message = format!("dream: lease {} {scope} on {device_id}", action.as_str());
    run_lease_commit(repo, &message)?;
    let sha = run_git(repo, &["rev-parse", "--short", "HEAD"])?.trim().to_string();
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

fn run_lease_commit(repo: &Path, message: &str) -> Result<(), GitError> {
    let args = vec![
        "commit".to_string(),
        "--author".to_string(),
        "memoryd lease-bot <noreply@memoryd.local>".to_string(),
        "-m".to_string(),
        message.to_string(),
        "--".to_string(),
        "leases/journal.lease".to_string(),
    ];
    let mut command = Command::new("git");
    command
        .args(&args)
        .current_dir(repo)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_NAMESPACE")
        .env("GIT_AUTHOR_NAME", "memoryd lease-bot")
        .env("GIT_AUTHOR_EMAIL", "noreply@memoryd.local")
        .env("GIT_COMMITTER_NAME", "memoryd lease-bot")
        .env("GIT_COMMITTER_EMAIL", "noreply@memoryd.local");

    let output = command.output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(GitError::CommandFailed {
            program: "git".to_string(),
            args,
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }
}

fn run_write_bot_commit(repo: &Path, message: &str) -> Result<(), GitError> {
    let args = vec![
        "commit".to_string(),
        "--author".to_string(),
        "memoryd write-bot <noreply@memoryd.local>".to_string(),
        "-m".to_string(),
        message.to_string(),
    ];
    let mut command = Command::new("git");
    command
        .args(&args)
        .current_dir(repo)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_NAMESPACE")
        .env("GIT_AUTHOR_NAME", "memoryd write-bot")
        .env("GIT_AUTHOR_EMAIL", "noreply@memoryd.local")
        .env("GIT_COMMITTER_NAME", "memoryd write-bot")
        .env("GIT_COMMITTER_EMAIL", "noreply@memoryd.local");

    let output = command.output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(GitError::CommandFailed {
            program: "git".to_string(),
            args,
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }
}

fn staged_paths() -> Vec<&'static str> {
    let mut paths = Vec::with_capacity(STAGED_NAMESPACES.len() + STAGED_ROOT_FILES.len());
    paths.extend_from_slice(STAGED_NAMESPACES);
    paths.extend_from_slice(STAGED_ROOT_FILES);
    paths
}

fn status_path(line: &str) -> Option<&str> {
    let path = line.get(3..)?.trim();
    if path.is_empty() {
        return None;
    }
    Some(path.rsplit(" -> ").next().unwrap_or(path).trim_matches('"'))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{
        auto_commit_with_outcome, commit_lease_file, commit_substrate_writes, CommitOutcome, LeaseCommitAction,
    };
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
        assert_eq!(
            git(repo.path(), &["rev-parse", "--short", "HEAD"]).expect("head sha"), // expect-justified: test assertion
            baseline_sha
        );
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

    #[test]
    fn commit_succeeds_on_unconfigured_git_identity() {
        let repo = tempdir().expect("tempdir"); // expect-justified: test setup
        bootstrap_repo_tree(repo.path()).expect("bootstrap"); // expect-justified: test setup
        git(repo.path(), &["init"]).expect("git init"); // expect-justified: test setup

        let outcome = commit_substrate_writes(repo.path(), 1).expect("write-bot commit succeeds without repo identity"); // expect-justified: test assertion

        assert!(matches!(outcome, CommitOutcome::Committed { .. }));
        assert_eq!(
            git(repo.path(), &["log", "-1", "--format=%an <%ae>"]).expect("author"), // expect-justified: test assertion
            "memoryd write-bot <noreply@memoryd.local>"
        );
        assert_eq!(
            git(repo.path(), &["log", "-1", "--format=%cn <%ce>"]).expect("committer"), // expect-justified: test assertion
            "memoryd write-bot <noreply@memoryd.local>"
        );
        assert_eq!(
            git(repo.path(), &["log", "-1", "--format=%s"]).expect("subject"), // expect-justified: test assertion
            "substrate: commit 1 write(s)"
        );
    }

    #[test]
    fn sources_web_write_is_tracked_after_commit() {
        let repo = tempdir().expect("tempdir"); // expect-justified: test setup
        bootstrap_repo_tree(repo.path()).expect("bootstrap"); // expect-justified: test setup
        git(repo.path(), &["init"]).expect("git init"); // expect-justified: test setup
        commit_substrate_writes(repo.path(), 1).expect("baseline commit"); // expect-justified: test setup
        let source_dir = repo.path().join("sources/web/example");
        fs::create_dir_all(&source_dir).expect("source dir"); // expect-justified: test setup
        fs::write(source_dir.join("manifest.json"), "{}\n").expect("source manifest"); // expect-justified: test setup

        let outcome = commit_substrate_writes(repo.path(), 1).expect("source commit"); // expect-justified: test assertion

        assert!(matches!(outcome, CommitOutcome::Committed { .. }));
        git(repo.path(), &["ls-files", "--error-unmatch", "sources/web/example/manifest.json"])
            .expect("source file tracked"); // expect-justified: test assertion
        assert_eq!(
            git(repo.path(), &["status", "--porcelain", "--", "sources/"]).expect("source status"), // expect-justified: test assertion
            ""
        );
    }

    #[test]
    fn broad_flush_between_lease_append_and_commit_does_not_corrupt_lease() {
        let repo = tempdir().expect("tempdir"); // expect-justified: test setup
        bootstrap_repo_tree(repo.path()).expect("bootstrap"); // expect-justified: test setup
        git(repo.path(), &["init"]).expect("git init"); // expect-justified: test setup
        commit_substrate_writes(repo.path(), 1).expect("baseline commit"); // expect-justified: test setup
        let lease_record = r#"{"device":"dev_local","scope":"me","run_id":"run_1"}"#;
        fs::write(repo.path().join("leases/journal.lease"), format!("{lease_record}\n")).expect("lease write"); // expect-justified: test setup
        fs::create_dir_all(repo.path().join("me/identity")).expect("identity dir"); // expect-justified: test setup
        fs::write(repo.path().join("me/identity/fact.md"), "---\nsummary: fact\n---\nbody\n").expect("memory write"); // expect-justified: test setup

        let outcome = commit_substrate_writes(repo.path(), 2).expect("broad substrate commit"); // expect-justified: test assertion

        assert!(matches!(outcome, CommitOutcome::Committed { .. }));
        let files =
            git(repo.path(), &["diff-tree", "--no-commit-id", "--name-only", "-r", "HEAD"]).expect("last commit files"); // expect-justified: test assertion
        assert!(files.lines().any(|path| path == "me/identity/fact.md"), "{files}");
        assert!(!files.lines().any(|path| path == "leases/journal.lease"), "{files}");
        assert_eq!(
            fs::read_to_string(repo.path().join("leases/journal.lease")).expect("lease text"), // expect-justified: test assertion
            format!("{lease_record}\n")
        );
        assert!(
            !git(repo.path(), &["status", "--porcelain", "--", "leases/journal.lease"])
                .expect("lease dirty status") // expect-justified: test assertion
                .is_empty(),
            "lease append must stay uncommitted for the lease-specific commit"
        );

        let lease_outcome =
            commit_lease_file(repo.path(), LeaseCommitAction::Acquire, "me", "dev_local").expect("lease commit"); // expect-justified: test assertion
        assert!(matches!(lease_outcome, CommitOutcome::Committed { .. }));
        assert_eq!(
            fs::read_to_string(repo.path().join("leases/journal.lease")).expect("lease text"), // expect-justified: test assertion
            format!("{lease_record}\n")
        );
    }

    #[test]
    #[cfg(unix)]
    fn commit_failure_does_not_lose_write() {
        use std::os::unix::fs::PermissionsExt;

        let repo = tempdir().expect("tempdir"); // expect-justified: test setup
        bootstrap_repo_tree(repo.path()).expect("bootstrap"); // expect-justified: test setup
        git(repo.path(), &["init"]).expect("git init"); // expect-justified: test setup
        commit_substrate_writes(repo.path(), 1).expect("baseline commit"); // expect-justified: test setup
        let hook = repo.path().join(".git/hooks/pre-commit");
        fs::write(&hook, "#!/bin/sh\necho blocked >&2\nexit 1\n").expect("hook write"); // expect-justified: test setup
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).expect("hook chmod"); // expect-justified: test setup
        let path = repo.path().join("me/identity/retry.md");
        let parent = path.parent().expect("path has parent"); // expect-justified: test setup
        fs::create_dir_all(parent).expect("identity dir"); // expect-justified: test setup
        fs::write(&path, "---\nsummary: retry\n---\nbody\n").expect("memory write"); // expect-justified: test setup

        let error = commit_substrate_writes(repo.path(), 1).expect_err("hook blocks commit");

        assert!(error.to_string().contains("blocked"), "{error}");
        assert!(path.is_file(), "failed commit must not delete the write");
        assert!(
            !git(repo.path(), &["status", "--porcelain", "--", "me/identity/retry.md"])
                .expect("dirty status") // expect-justified: test assertion
                .is_empty(),
            "failed commit must leave the write retryable"
        );

        fs::remove_file(hook).expect("remove hook"); // expect-justified: test setup
        let retry = commit_substrate_writes(repo.path(), 1).expect("retry commit"); // expect-justified: test assertion
        assert!(matches!(retry, CommitOutcome::Committed { .. }));
        assert_eq!(
            git(repo.path(), &["status", "--porcelain", "--", "me/identity/retry.md"]).expect("clean"), // expect-justified: test assertion
            ""
        );
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
