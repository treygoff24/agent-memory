use std::collections::VecDeque;
use std::path::Path;

use memory_substrate::git::{commit_lease_file, push, run_git, CommitOutcome};

use crate::dream::lease::{LeaseCommit, LeaseError};

/// Git operations needed for lease election.
///
/// Tests script this trait directly so lease retry behavior can be proven
/// without network remotes or racy real repositories.
pub trait LeaseGit {
    fn fetch_origin(&mut self, repo: &Path) -> Result<(), LeaseError>;
    /// Return the list of paths in the working tree that count as "dirty user work" —
    /// anything `git status --porcelain --untracked-files=all` reports that is not
    /// `leases/journal.lease`. Empty list means the tree is clean for lease purposes.
    /// Callers should use `paths.is_empty()` for the boolean check and surface the
    /// path list in operator-visible errors so the offending paths are diagnosable.
    fn dirty_user_work_paths(&mut self, repo: &Path) -> Result<Vec<String>, LeaseError>;
    fn commit_lease(&mut self, repo: &Path, commit: &LeaseCommit<'_>) -> Result<(), LeaseError>;
    fn push(&mut self, repo: &Path) -> Result<(), LeaseError>;
    fn rollback_failed_lease_attempt(&mut self, repo: &Path) -> Result<(), LeaseError>;
}

#[derive(Debug, Default)]
pub struct NativeLeaseGit;

impl LeaseGit for NativeLeaseGit {
    fn fetch_origin(&mut self, repo: &Path) -> Result<(), LeaseError> {
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

    fn push(&mut self, repo: &Path) -> Result<(), LeaseError> {
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
    fn fetch_origin(&mut self, _repo: &Path) -> Result<(), LeaseError> {
        self.fetch_calls += 1;
        self.fetch_results.pop_front().unwrap_or(Ok(())).map_err(LeaseError::unavailable)
    }

    fn dirty_user_work_paths(&mut self, _repo: &Path) -> Result<Vec<String>, LeaseError> {
        if self.dirty_user_work { Ok(vec!["<scripted-dirty>".to_string()]) } else { Ok(Vec::new()) }
    }

    fn commit_lease(&mut self, _repo: &Path, _commit: &LeaseCommit<'_>) -> Result<(), LeaseError> {
        Ok(())
    }

    fn push(&mut self, _repo: &Path) -> Result<(), LeaseError> {
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

fn dirty_user_work_paths(repo: &Path) -> Result<Vec<String>, LeaseError> {
    let output = run_git(repo, &["status", "--porcelain=v1", "--untracked-files=all"])
        .map_err(|err| LeaseError::unavailable(err.to_string()))?;
    Ok(output.lines().filter_map(status_path).filter(|path| !is_substrate_managed_path(path)).map(str::to_string).collect())
}

/// Paths the substrate, daemon runtime, or lease subsystem manages on the user's behalf —
/// the dirty-tree check refuses lease acquisition on user work, not on transient substrate
/// state. Without this filter the daemon's own writes (per-device events log, substrate
/// state, runtime artifacts) race the lease check under parallel load and flake T17.
///
/// Keep this list aligned with the auto-generated `.gitignore` written by
/// `memory_substrate::tree::layout::bootstrap_repo_layout` so any path the substrate
/// is allowed to author is also tolerated by the lease dirty-tree check.
fn is_substrate_managed_path(path: &str) -> bool {
    if path == "leases/journal.lease" {
        return true;
    }
    // Substrate marker dir (.memorum), daemon runtime (.memoryd), and the per-device
    // events log are all written by the daemon between substrate commit cycles.
    const SUBSTRATE_MANAGED_PREFIXES: &[&str] = &[".memorum/", ".memoryd/", "events/"];
    SUBSTRATE_MANAGED_PREFIXES.iter().any(|prefix| path.starts_with(prefix))
}

fn status_path(line: &str) -> Option<&str> {
    let path = line.get(3..)?.trim();
    if path.is_empty() {
        return None;
    }
    Some(path.rsplit(" -> ").next().unwrap_or(path).trim_matches('"'))
}
