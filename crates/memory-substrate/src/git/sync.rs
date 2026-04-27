//! Fetch/merge/push helpers per spec §13.5.

use std::path::Path;

use crate::error::GitError;
use crate::git::{git_preflight, run_git};

/// Result of a `fetch_and_merge` operation.
#[derive(Debug, Eq, PartialEq)]
pub enum SyncOutcome {
    /// Remote and local are identical; no merge performed.
    NothingToDo,
    /// Local is ahead of remote; no merge needed.
    LocalAhead,
    /// Merge succeeded; local may have new memories.
    Merged,
    /// Merge was attempted but produced quarantined files.
    Quarantined(Vec<QuarantineRecord>),
}

/// A quarantined file detected after merge.
#[derive(Debug, Eq, PartialEq)]
pub struct QuarantineRecord {
    /// Repository-relative path of the quarantined file.
    pub path: String,
}

/// Remote divergence state relative to local HEAD.
#[derive(Debug, Eq, PartialEq)]
pub enum RemoteState {
    /// Remote and local are at the same commit.
    UpToDate,
    /// Local is ahead by `n` commits.
    Ahead(u32),
    /// Remote is ahead by `n` commits; merge is needed.
    Behind(u32),
    /// Histories have diverged; merge is needed.
    Diverged { ahead: u32, behind: u32 },
}

/// Orchestrate the full §13.5 sync protocol.
///
/// Steps:
/// 1. Preflight check (merge driver binary present, repo initialised).
/// 2. `git fetch origin`.
/// 3. Classify ahead/behind/diverged state.
/// 4. Merge only when behind or diverged.
/// 5. Scan for quarantined memories post-merge.
/// 6. Deferred: auto-commit reconciliation work post-merge.
pub fn fetch_and_merge(repo: &Path, merge_driver_binary: &Path) -> Result<SyncOutcome, GitError> {
    git_preflight(repo, merge_driver_binary)?;
    fetch_origin(repo)?;

    match classify_remote_state(repo)? {
        RemoteState::UpToDate => return Ok(SyncOutcome::NothingToDo),
        RemoteState::Ahead(_) => return Ok(SyncOutcome::LocalAhead),
        RemoteState::Behind(_) | RemoteState::Diverged { .. } => {}
    }

    merge_no_ff(repo)?;

    let quarantines = scan_quarantine(repo);
    if !quarantines.is_empty() {
        // Deferred: emit MergeQuarantined events via event log.
        return Ok(SyncOutcome::Quarantined(quarantines));
    }

    // Deferred: emit GitFetched event via event log; auto_commit_reconciliation.

    Ok(SyncOutcome::Merged)
}

/// Fetch from origin without merging.
fn fetch_origin(repo: &Path) -> Result<(), GitError> {
    run_git(repo, &["fetch", "origin"]).map(|_| ())
}

/// Classify the local HEAD position relative to `origin/main`.
fn classify_remote_state(repo: &Path) -> Result<RemoteState, GitError> {
    let output = run_git(repo, &["rev-list", "--count", "--left-right", "HEAD...origin/main"])?;
    let (ahead_str, behind_str) = parse_rev_list_count(output.trim()).ok_or_else(|| GitError::CommandFailed {
        program: "git".to_string(),
        args: vec!["rev-list".to_string(), "--count".to_string(), "--left-right".to_string()],
        stderr: "unexpected rev-list output".to_string(),
    })?;

    Ok(match (ahead_str, behind_str) {
        (0, 0) => RemoteState::UpToDate,
        (a, 0) => RemoteState::Ahead(a),
        (0, b) => RemoteState::Behind(b),
        (a, b) => RemoteState::Diverged { ahead: a, behind: b },
    })
}

/// Parse `git rev-list --count --left-right` tab-separated output.
fn parse_rev_list_count(s: &str) -> Option<(u32, u32)> {
    let (left, right) = s.split_once('\t')?;
    Some((left.trim().parse().ok()?, right.trim().parse().ok()?))
}

/// Run `git merge --no-ff origin/main` per spec §13.5 step 5.
fn merge_no_ff(repo: &Path) -> Result<(), GitError> {
    run_git(repo, &["merge", "--no-ff", "origin/main"]).map(|_| ())
}

/// Walk the working tree for files with `status: quarantined` in their content.
fn scan_quarantine(repo: &Path) -> Vec<QuarantineRecord> {
    let mut records = Vec::new();
    let walker = walkdir::WalkDir::new(repo).follow_links(false);
    for entry in walker.into_iter().filter_map(Result::ok) {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "md") {
            continue;
        }
        if is_quarantined_memory(path) {
            let relative = path.strip_prefix(repo).map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
            records.push(QuarantineRecord { path: relative });
        }
    }
    records
}

/// Return true when the file contains `status: quarantined` in its frontmatter.
fn is_quarantined_memory(path: &Path) -> bool {
    let Ok(bytes) = std::fs::read(path) else { return false };
    let Ok(text) = std::str::from_utf8(&bytes) else { return false };
    // Fast textual scan — avoid full YAML parse on the hot path.
    text.contains("status: quarantined") || text.contains("status:quarantined")
}

/// Push the current branch to origin.
pub fn push(repo: &Path) -> Result<(), GitError> {
    run_git(repo, &["push"]).map(|_| ()).map_err(|err| GitError::GitPushFailed(err.to_string()))
}

/// Fetch without merging (dry-run inspection).
pub fn fetch_inspect(repo: &Path) -> Result<String, GitError> {
    run_git(repo, &["fetch", "--dry-run"])
}
