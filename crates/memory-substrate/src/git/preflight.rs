//! Git preflight.

use std::path::Path;

use crate::error::GitError;
use crate::git::run_git;

/// Check merge-driver prerequisites.
pub fn git_preflight(repo: &Path, merge_driver_binary: &Path) -> Result<(), GitError> {
    if !repo.join(".git").exists() {
        return Err(GitError::InvalidRepoRoot(format!("{}; run git::adopt_clone before sync", repo.display())));
    }
    if !merge_driver_binary.exists() {
        return Err(GitError::MergeDriverMissing(merge_driver_binary.display().to_string()));
    }
    run_git(repo, &["config", "--get", "merge.memory-merge-driver.driver"]).map_err(|_| {
        GitError::InvalidRepoRoot(format!(
            "{} is missing merge driver config; run git::adopt_clone before sync",
            repo.display()
        ))
    })?;
    Ok(())
}
