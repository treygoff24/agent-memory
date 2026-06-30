//! Auto-commit helper.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::GitError;
use crate::git::{command::run_git_with_env, run_git};

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

/// Git exclude pathspec that keeps in-flight atomic-write temps
/// (`markdown/atomic.rs`'s nested `.<basename>.<op_id>.tmp`) out of every staging
/// pass, independent of `.gitignore`. The de-anchored `.*.tmp` gitignore entry
/// (`tree/layout.rs`) already ignores these on a freshly-bootstrapped repo, but a
/// repo whose `.gitignore` predates that entry — or carries a stale root-anchored
/// `/.*.tmp` that misses nested temps — would otherwise let `git add -- me/ …`
/// stage a possibly-torn temp into the canonical tree. `glob` magic makes `**/`
/// match any depth while `*` stays within a path component, so this matches a
/// dot-prefixed `.tmp` basename at any depth and never a real canonical file
/// (none of `*.md`/`config.yaml`/`*.jsonl`/`.keep`/… both start with `.` and end
/// with `.tmp`).
const ATOMIC_TEMP_EXCLUDE_PATHSPEC: &str = ":(exclude,glob)**/.*.tmp";
const LEASE_BOT_NAME: &str = "memoryd lease-bot";
const WRITE_BOT_NAME: &str = "memoryd write-bot";
const BOT_EMAIL: &str = "noreply@memoryd.local";

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

    let paths = staged_substrate_write_paths(repo)?;
    if paths.is_empty() {
        return Ok(CommitOutcome::NoChanges);
    }

    // `write_count` is counted by the caller OUTSIDE the git lock, so a write that
    // lands between that count and this commit can make `<n>` stale — it is advisory
    // commit-message text only and never gates what is actually staged.
    let message = format!("substrate: commit {write_count} write(s)");
    run_write_bot_commit(repo, &message, &paths)?;
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
        .filter(|path| *path != "leases/journal.lease" && !is_atomic_temp(path))
        // ^ `path: &&str`; `is_atomic_temp` takes `&str` via deref coercion.
        .map(str::to_string)
        .collect())
}

/// An in-flight atomic-write temp file (`markdown/atomic.rs`'s nested
/// `.<basename>.<op_id>.tmp`). Staging already excludes these at `git add` time via
/// [`ATOMIC_TEMP_EXCLUDE_PATHSPEC`]; this predicate keeps the count/D3 status path
/// ([`uncommitted_substrate_paths`]) consistent with that staging filter, so a torn
/// temp surfaced by `git status` (e.g. on a repo with a stale `.*.tmp` gitignore) is
/// never counted as a pending substrate write.
fn is_atomic_temp(path: &str) -> bool {
    path.rsplit('/').next().is_some_and(|name| name.starts_with('.') && name.ends_with(".tmp"))
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
    // Stage the canonical namespaces but never an in-flight atomic temp: this
    // exclude pathspec filters torn `.<name>.tmp` files at `git add` time, so the
    // result is correct even on a repo whose `.gitignore` lacks (or root-anchors)
    // the `.*.tmp` entry. The guard is gitignore-independent by construction.
    full_args.push(ATOMIC_TEMP_EXCLUDE_PATHSPEC);
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

fn staged_substrate_write_paths(repo: &Path) -> Result<Vec<String>, GitError> {
    let mut args = vec!["diff", "--cached", "--name-only", "--"];
    args.extend(staged_paths());
    let output = run_git(repo, &args)?;
    Ok(output
        .lines()
        .filter(|path| *path != "leases/journal.lease")
        .filter(|path| !is_atomic_temp(path))
        .map(str::to_string)
        .collect())
}

/// Run `git commit` and return the short SHA of the new commit.
fn run_commit(repo: &Path, message: &str) -> Result<String, GitError> {
    run_git(repo, &["commit", "-m", message])?;
    run_git(repo, &["rev-parse", "--short", "HEAD"]).map(|sha| sha.trim().to_string())
}

fn run_lease_commit(repo: &Path, message: &str) -> Result<(), GitError> {
    run_bot_commit(
        repo,
        BotCommit { bot_name: LEASE_BOT_NAME, message, paths: &["leases/journal.lease"], extra_env: &[] },
    )
}

fn run_write_bot_commit(repo: &Path, message: &str, paths: &[String]) -> Result<(), GitError> {
    let temp_index = tempfile::NamedTempFile::new()?;
    fs::copy(git_index_path(repo)?, temp_index.path())?;

    let path_set = paths.iter().map(String::as_str).collect::<BTreeSet<_>>();
    let unrelated_paths =
        staged_index_paths(repo)?.into_iter().filter(|path| !path_set.contains(path.as_str())).collect::<Vec<_>>();
    let index_env = temp_index.path().to_string_lossy().to_string();
    if !unrelated_paths.is_empty() {
        let mut args = vec!["reset", "-q", "--"];
        args.extend(unrelated_paths.iter().map(String::as_str));
        run_git_with_env(repo, &args, &[("GIT_INDEX_FILE", index_env.as_str())])?;
    }

    run_bot_commit(
        repo,
        BotCommit {
            bot_name: WRITE_BOT_NAME,
            message,
            paths: &[],
            extra_env: &[("GIT_INDEX_FILE", index_env.as_str())],
        },
    )
}

struct BotCommit<'a> {
    bot_name: &'static str,
    message: &'a str,
    paths: &'a [&'a str],
    extra_env: &'a [(&'a str, &'a str)],
}

fn run_bot_commit(repo: &Path, commit: BotCommit<'_>) -> Result<(), GitError> {
    let author = format!("{} <{}>", commit.bot_name, BOT_EMAIL);
    let mut args = vec!["commit", "--author", author.as_str(), "-m", commit.message];
    if !commit.paths.is_empty() {
        args.push("--");
        args.extend(commit.paths.iter().copied());
    }
    let mut envs = vec![
        ("GIT_AUTHOR_NAME", commit.bot_name),
        ("GIT_AUTHOR_EMAIL", BOT_EMAIL),
        ("GIT_COMMITTER_NAME", commit.bot_name),
        ("GIT_COMMITTER_EMAIL", BOT_EMAIL),
    ];
    envs.extend(commit.extra_env.iter().copied());
    run_git_with_env(repo, &args, &envs).map(|_| ())
}

fn staged_index_paths(repo: &Path) -> Result<Vec<String>, GitError> {
    let output = run_git(repo, &["diff", "--cached", "--name-only"])?;
    Ok(output.lines().map(str::to_string).collect())
}

fn git_index_path(repo: &Path) -> Result<PathBuf, GitError> {
    let path = PathBuf::from(run_git(repo, &["rev-parse", "--git-path", "index"])?.trim());
    Ok(if path.is_absolute() { path } else { repo.join(path) })
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
        auto_commit_with_outcome, commit_lease_file, commit_substrate_writes, count_substrate_write_changes,
        CommitOutcome, LeaseCommitAction,
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
    fn write_bot_commit_preserves_unrelated_prestaged_files() {
        let repo = tempdir().expect("tempdir"); // expect-justified: test setup
        bootstrap_repo_tree(repo.path()).expect("bootstrap"); // expect-justified: test setup
        git(repo.path(), &["init"]).expect("git init"); // expect-justified: test setup
        commit_substrate_writes(repo.path(), 1).expect("baseline commit"); // expect-justified: test setup
        fs::write(repo.path().join("scratch.txt"), "pre-staged\n").expect("scratch"); // expect-justified: test setup
        git(repo.path(), &["add", "--", "scratch.txt"]).expect("stage scratch"); // expect-justified: test setup
        fs::create_dir_all(repo.path().join("me/identity")).expect("identity dir"); // expect-justified: test setup
        fs::write(repo.path().join("me/identity/fact.md"), "---\nsummary: fact\n---\nbody\n").expect("memory write"); // expect-justified: test setup

        let outcome = commit_substrate_writes(repo.path(), 1).expect("write commit"); // expect-justified: test assertion

        assert!(matches!(outcome, CommitOutcome::Committed { .. }));
        let committed_files =
            git(repo.path(), &["diff-tree", "--no-commit-id", "--name-only", "-r", "HEAD"]).expect("committed files"); // expect-justified: test assertion
        assert!(committed_files.lines().any(|path| path == "me/identity/fact.md"), "{committed_files}");
        assert!(!committed_files.lines().any(|path| path == "scratch.txt"), "{committed_files}");
        assert_eq!(
            git(repo.path(), &["status", "--porcelain", "--", "scratch.txt"]).expect("scratch status"), // expect-justified: test assertion
            "A  scratch.txt"
        );
        assert!(
            git(repo.path(), &["cat-file", "-e", "HEAD:scratch.txt"]).is_err(),
            "scratch.txt must remain staged and absent from HEAD"
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
    fn commit_failure_does_not_lose_write_and_surfaces_to_doctor() {
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

    #[test]
    fn worker_never_commits_a_nested_atomic_temp() {
        let repo = tempdir().expect("tempdir"); // expect-justified: test setup
        bootstrap_repo_tree(repo.path()).expect("bootstrap"); // expect-justified: test setup
        git(repo.path(), &["init"]).expect("git init"); // expect-justified: test setup
        commit_substrate_writes(repo.path(), 1).expect("baseline commit"); // expect-justified: test setup
        fs::create_dir_all(repo.path().join("me/identity")).expect("dir"); // expect-justified: test setup
        fs::write(repo.path().join("me/identity/fact.md"), "---\nsummary: f\n---\nbody\n").expect("fact"); // expect-justified: test setup
        fs::write(repo.path().join("me/identity/.fact.md.op1.tmp"), "torn").expect("temp"); // expect-justified: test setup

        // The temp is not counted, and the commit holds the real file but never the temp.
        assert_eq!(count_substrate_write_changes(repo.path()).expect("count"), 1, "atomic temp must not be counted"); // expect-justified: test assertion
        let outcome = commit_substrate_writes(repo.path(), 1).expect("commit"); // expect-justified: test assertion
        assert!(matches!(outcome, CommitOutcome::Committed { .. }));
        let files =
            git(repo.path(), &["diff-tree", "--no-commit-id", "--name-only", "-r", "HEAD"]).expect("committed files"); // expect-justified: test assertion
        assert!(files.lines().any(|path| path == "me/identity/fact.md"), "{files}");
        assert!(!files.lines().any(|path| path.ends_with(".tmp")), "atomic temp must never be committed: {files}");
    }

    #[test]
    fn worker_never_stages_a_nested_atomic_temp_with_stale_gitignore() {
        let repo = tempdir().expect("tempdir"); // expect-justified: test setup
        bootstrap_repo_tree(repo.path()).expect("bootstrap"); // expect-justified: test setup
                                                              // Simulate a repo whose `.gitignore` predates the de-anchored `.*.tmp`
                                                              // entry: the brief intermediate F1 build emitted a ROOT-anchored `/.*.tmp`,
                                                              // which ignores only top-level temps and leaves NESTED atomic temps
                                                              // untracked-and-unignored. Overwriting after bootstrap is durable because
                                                              // `reconcile_gitignore` only runs during bootstrap, so the stale entry
                                                              // survives to the commit — only the staging pathspec exclude keeps the
                                                              // nested temp out of the canonical tree here.
        fs::write(repo.path().join(".gitignore"), "/.memoryd/\n/.memorum/\n/.*.tmp\n").expect("stale gitignore"); // expect-justified: test setup
        git(repo.path(), &["init"]).expect("git init"); // expect-justified: test setup
        commit_substrate_writes(repo.path(), 1).expect("baseline commit"); // expect-justified: test setup
        fs::create_dir_all(repo.path().join("me/identity")).expect("dir"); // expect-justified: test setup
        fs::write(repo.path().join("me/identity/fact.md"), "---\nsummary: f\n---\nbody\n").expect("fact"); // expect-justified: test setup
        fs::write(repo.path().join("me/identity/.fact.md.op1.tmp"), "torn").expect("temp"); // expect-justified: test setup

        // Confirm the stale gitignore really does NOT ignore the nested temp — so
        // the staging pathspec exclude (not gitignore) is what this test exercises.
        assert!(
            git(repo.path(), &["status", "--porcelain", "--", "me/identity/.fact.md.op1.tmp"])
                .expect("temp status") // expect-justified: test assertion
                .contains("??"),
            "stale root-anchored /.*.tmp must leave the nested temp untracked-and-unignored"
        );

        let outcome = commit_substrate_writes(repo.path(), 1).expect("commit"); // expect-justified: test assertion
        assert!(matches!(outcome, CommitOutcome::Committed { .. }));
        let files =
            git(repo.path(), &["diff-tree", "--no-commit-id", "--name-only", "-r", "HEAD"]).expect("committed files"); // expect-justified: test assertion
        assert!(files.lines().any(|path| path == "me/identity/fact.md"), "{files}");
        assert!(
            !files.lines().any(|path| path.ends_with(".tmp")),
            "staging exclude must keep the nested temp out of the commit even with a stale gitignore: {files}"
        );
        // The temp is never deleted; it stays on disk for the in-flight writer.
        assert!(repo.path().join("me/identity/.fact.md.op1.tmp").is_file(), "atomic temp must remain on disk");
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
