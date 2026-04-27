//! Git preflight.

use std::path::Path;

use crate::error::GitError;

/// Check merge-driver prerequisites.
pub fn git_preflight(repo: &Path, merge_driver_binary: &Path) -> Result<(), GitError> {
    if !repo.join(".git").exists() {
        return Err(GitError::InvalidRepoRoot(format!("{}; run git::adopt_clone before sync", repo.display())));
    }
    if !merge_driver_binary.exists() {
        return Err(GitError::MergeDriverMissing(merge_driver_binary.display().to_string()));
    }
    Ok(())
}
