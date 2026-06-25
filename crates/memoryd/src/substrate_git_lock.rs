use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use fs2::FileExt;
use memory_substrate::git::{commit_substrate_writes, count_substrate_write_changes, CommitOutcome};

/// Held-open advisory lock for substrate git operations.
pub struct SubstrateGitLockGuard {
    file: File,
    path: PathBuf,
}

impl SubstrateGitLockGuard {
    /// Lock file path: `<runtime>/.memoryd/substrate-git.lock`.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for SubstrateGitLockGuard {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.file);
    }
}

/// Acquire the repo-level substrate git lock at `<runtime>/.memoryd/substrate-git.lock`.
pub fn acquire_substrate_git_lock(runtime: &Path) -> std::io::Result<SubstrateGitLockGuard> {
    let dir = runtime.join(".memoryd");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("substrate-git.lock");
    let file = OpenOptions::new().create(true).write(true).truncate(false).open(&path)?;
    file.lock_exclusive()?;
    Ok(SubstrateGitLockGuard { file, path })
}

/// Commit any pending daemon substrate writes under the repo-level lock.
///
/// A no-op when there is nothing to commit, so the lock is only taken when there
/// is work — and so a caller can flush unconditionally before acquiring a dream
/// lease without contending on an empty tree. Shared by every out-of-worker
/// flush site (the dream CLI and the in-daemon dream handler); the daemon commit
/// worker has its own count-then-commit loop.
pub(crate) fn flush_substrate_writes(repo: &Path, runtime: &Path) -> anyhow::Result<()> {
    let write_count = count_substrate_write_changes(repo)?;
    if write_count == 0 {
        return Ok(());
    }
    let _lock = acquire_substrate_git_lock(runtime)?;
    match commit_substrate_writes(repo, write_count)? {
        CommitOutcome::Committed { .. } | CommitOutcome::NoChanges => Ok(()),
    }
}
