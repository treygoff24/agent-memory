use std::collections::{HashSet, VecDeque};
use std::path::Path;

use memory_substrate::git::{
    commit_lease_file, push, run_git, uncommitted_substrate_paths, CommitOutcome, LeaseCommitAction,
};
use thiserror::Error;

/// A single lease commit to apply to the journal. Lives next to the
/// [`LeaseGit`] trait that consumes it so the git layer carries its own
/// contract types without depending on the lease orchestration module.
#[derive(Debug, Clone, Copy)]
pub struct LeaseCommit<'a> {
    pub action: LeaseCommitAction,
    pub scope: &'a str,
    pub device_id: &'a str,
}

/// Error surface for lease election. Shared between the git layer (which
/// produces it) and the lease orchestration layer (which maps it to CLI and
/// protocol surfaces).
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum LeaseError {
    #[error("lease_held: active lease for {scope} is held by {by_device}")]
    Held { scope: String, by_device: String },
    #[error("lease_unavailable: {message}")]
    Unavailable { message: String },
    #[error("lease_dirty_tree: {message}")]
    DirtyTree { message: String },
    #[error("invalid_request: {message}")]
    InvalidRequest { message: String },
}

impl LeaseError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Held { .. } => "lease_held",
            Self::Unavailable { .. } => "lease_unavailable",
            Self::DirtyTree { .. } => "lease_dirty_tree",
            Self::InvalidRequest { .. } => "invalid_request",
        }
    }

    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::Unavailable { message: message.into() }
    }

    pub fn cli_exit_code(&self) -> i32 {
        match self {
            Self::Held { .. } | Self::Unavailable { .. } | Self::DirtyTree { .. } => 5,
            Self::InvalidRequest { .. } => 1,
        }
    }
}

/// Git operations needed for lease election.
///
/// Tests script this trait directly so lease retry behavior can be proven
/// without network remotes or racy real repositories.
pub trait LeaseGit {
    /// Probe whether an origin remote is configured. Called once per acquire attempt /
    /// release transaction; the result is threaded into [`LeaseGit::fetch_origin`] and
    /// [`LeaseGit::push`] so a single probe covers the whole attempt. Goes through the
    /// trait (rather than the free `origin_remote_configured`) so scripted tests can drive
    /// it without a real repo. `Ok(false)` means "no origin"; a probe *failure* is `Err`
    /// and must never be collapsed into "no remote" (I-F2.4).
    fn origin_configured(&mut self, repo: &Path) -> Result<bool, LeaseError>;
    /// Fetch from origin when a remote is configured. `has_origin` is the caller's
    /// single per-attempt [`LeaseGit::origin_configured`] probe result, threaded in so one
    /// `git remote` subprocess covers the whole attempt — fetch, the stale-lease eviction
    /// discriminator, and push. A probe *failure* surfaces as `Err` at the caller before
    /// this is reached, so it is never collapsed into "no remote" (I-F2.4).
    fn fetch_origin(&mut self, repo: &Path, has_origin: bool) -> Result<(), LeaseError>;
    /// Return the list of paths in the working tree that count as "dirty user work" —
    /// anything `git status --porcelain --untracked-files=all` reports that is not
    /// `leases/journal.lease`. Empty list means the tree is clean for lease purposes.
    /// Callers should use `paths.is_empty()` for the boolean check and surface the
    /// path list in operator-visible errors so the offending paths are diagnosable.
    fn dirty_user_work_paths(&mut self, repo: &Path) -> Result<Vec<String>, LeaseError>;
    fn commit_lease(&mut self, repo: &Path, commit: &LeaseCommit<'_>) -> Result<(), LeaseError>;
    /// Push to origin when a remote is configured. `has_origin` is the same per-attempt
    /// probe result threaded into [`LeaseGit::fetch_origin`] (see there).
    fn push(&mut self, repo: &Path, has_origin: bool) -> Result<(), LeaseError>;
    fn rollback_failed_lease_attempt(&mut self, repo: &Path) -> Result<(), LeaseError>;
}

#[derive(Debug, Default)]
pub struct NativeLeaseGit;

impl LeaseGit for NativeLeaseGit {
    fn origin_configured(&mut self, repo: &Path) -> Result<bool, LeaseError> {
        origin_remote_configured(repo)
    }

    fn fetch_origin(&mut self, repo: &Path, has_origin: bool) -> Result<(), LeaseError> {
        // Local-first (spec §2/F2): with no origin remote there is nothing to fetch,
        // so the lease election runs entirely locally. A *configured* remote that
        // fails still errors below (I-F2.4) — only the no-remote case no-ops. `has_origin`
        // is the caller's once-per-attempt probe; a probe *failure* already surfaced there
        // as `Err`, so it is never silently collapsed into "no remote".
        if !has_origin {
            return Ok(());
        }
        run_git(repo, &["fetch", "origin"]).map(|_| ()).map_err(|err| LeaseError::unavailable(err.to_string()))
    }

    fn dirty_user_work_paths(&mut self, repo: &Path) -> Result<Vec<String>, LeaseError> {
        dirty_user_work_paths(repo)
    }

    fn commit_lease(&mut self, repo: &Path, commit: &LeaseCommit<'_>) -> Result<(), LeaseError> {
        let outcome = commit_lease_file(repo, commit.action, commit.scope, commit.device_id)
            .map_err(|err| LeaseError::unavailable(err.to_string()))?;
        match outcome {
            CommitOutcome::Committed { .. } | CommitOutcome::NoChanges => Ok(()),
        }
    }

    fn push(&mut self, repo: &Path, has_origin: bool) -> Result<(), LeaseError> {
        // Local-first (spec §2/F2): no origin remote → the local commit is the
        // durable record, so the push no-ops. A configured remote still pushes
        // and surfaces failures (I-F2.4). `has_origin` is threaded from the caller's
        // single per-attempt probe (see `fetch_origin`).
        if !has_origin {
            return Ok(());
        }
        push(repo).map_err(|err| LeaseError::unavailable(err.to_string()))
    }

    fn rollback_failed_lease_attempt(&mut self, repo: &Path) -> Result<(), LeaseError> {
        run_git(repo, &["reset", "--hard", "HEAD~1"])
            .map(|_| ())
            .map_err(|err| LeaseError::unavailable(err.to_string()))
    }
}

#[derive(Debug, Default)]
pub struct ScriptedLeaseGit {
    fetch_results: VecDeque<Result<(), String>>,
    push_results: VecDeque<Result<(), String>>,
    fetch_calls: usize,
    push_calls: usize,
    dirty_user_work: bool,
}

impl ScriptedLeaseGit {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_fetch_results<const N: usize>(mut self, results: [Result<(), String>; N]) -> Self {
        self.fetch_results = results.into();
        self
    }

    pub fn with_push_results<const N: usize>(mut self, results: [Result<(), String>; N]) -> Self {
        self.push_results = results.into();
        self
    }

    pub fn fetch_calls(&self) -> usize {
        self.fetch_calls
    }

    pub fn push_calls(&self) -> usize {
        self.push_calls
    }
}

impl LeaseGit for ScriptedLeaseGit {
    fn origin_configured(&mut self, _repo: &Path) -> Result<bool, LeaseError> {
        // Scripted lease tests exercise retry/race logic against an assumed remote, so the
        // probe reports "configured" and lets the scripted fetch/push outcomes drive the
        // run. No-remote behavior is covered separately by NativeLeaseGit + real repos.
        Ok(true)
    }

    fn fetch_origin(&mut self, _repo: &Path, _has_origin: bool) -> Result<(), LeaseError> {
        self.fetch_calls += 1;
        self.fetch_results.pop_front().unwrap_or(Ok(())).map_err(LeaseError::unavailable)
    }

    fn dirty_user_work_paths(&mut self, _repo: &Path) -> Result<Vec<String>, LeaseError> {
        if self.dirty_user_work {
            Ok(vec!["<scripted-dirty>".to_string()])
        } else {
            Ok(Vec::new())
        }
    }

    fn commit_lease(&mut self, _repo: &Path, _commit: &LeaseCommit<'_>) -> Result<(), LeaseError> {
        Ok(())
    }

    fn push(&mut self, _repo: &Path, _has_origin: bool) -> Result<(), LeaseError> {
        self.push_calls += 1;
        self.push_results.pop_front().unwrap_or(Ok(())).map_err(LeaseError::unavailable)
    }

    fn rollback_failed_lease_attempt(&mut self, repo: &Path) -> Result<(), LeaseError> {
        let lease_path = repo.join("leases/journal.lease");
        let text = std::fs::read_to_string(&lease_path).map_err(|err| LeaseError::unavailable(err.to_string()))?;
        let mut records = text.lines().filter(|line| !line.trim().is_empty()).collect::<Vec<_>>();
        records.pop();
        let new_text = if records.is_empty() { String::new() } else { format!("{}\n", records.join("\n")) };
        std::fs::write(lease_path, new_text).map_err(|err| LeaseError::unavailable(err.to_string()))
    }
}

/// Whether `repo` has an `origin` remote configured — the synchronous sibling of
/// the async `git_origin_remote` in `recall/project.rs` (not extracted from it
/// because [`LeaseGit`] is sync and an async helper would block a runtime thread).
///
/// Probes with `git remote`, which lists configured remote names and exits 0 even
/// when there are none. Membership of `origin` is therefore a version-robust signal,
/// unlike `git remote get-url origin`, whose "no such remote" exit code is not
/// stable across git builds. Runs through [`run_git`] so the probe inherits the same
/// `GIT_DIR`/`GIT_WORK_TREE` sanitization as every other git call, and a real failure
/// (not a git repo, unreadable config) surfaces as `Err` — a broken remote is never
/// silently collapsed into "local-only" (I-F2.4).
pub(crate) fn origin_remote_configured(repo: &Path) -> Result<bool, LeaseError> {
    let remotes = run_git(repo, &["remote"]).map_err(|err| LeaseError::unavailable(err.to_string()))?;
    Ok(remotes.lines().any(|name| name.trim() == "origin"))
}

fn dirty_user_work_paths(repo: &Path) -> Result<Vec<String>, LeaseError> {
    let output = run_git(repo, &["status", "--porcelain=v1", "--untracked-files=all"])
        .map_err(|err| LeaseError::unavailable(err.to_string()))?;
    let daemon_managed_paths = uncommitted_substrate_paths(repo)
        .map_err(|err| LeaseError::unavailable(err.to_string()))?
        .into_iter()
        .collect::<HashSet<_>>();
    Ok(output
        .lines()
        .filter_map(status_path)
        .filter(|path| !is_lease_file(path))
        .filter(|path| !is_runtime_managed_path(path))
        .filter(|path| !daemon_managed_paths.contains(*path))
        .map(str::to_string)
        .collect())
}

fn is_lease_file(path: &str) -> bool {
    path == "leases/journal.lease"
}

fn is_runtime_managed_path(path: &str) -> bool {
    const RUNTIME_MANAGED_PREFIXES: &[&str] = &[".memorum/", ".memoryd/"];
    RUNTIME_MANAGED_PREFIXES.iter().any(|prefix| path.starts_with(prefix))
}

fn status_path(line: &str) -> Option<&str> {
    let path = line.get(3..)?.trim();
    if path.is_empty() {
        return None;
    }
    Some(path.rsplit(" -> ").next().unwrap_or(path).trim_matches('"'))
}
