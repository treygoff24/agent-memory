//! Git init.

use std::path::Path;

use crate::error::GitError;
use crate::git::commit::{auto_commit_with_outcome, CommitOutcome};
use crate::git::run_git;
use crate::tree::bootstrap_repo_tree;

/// Initialize repo tree and git config, then commit the bootstrap files.
pub fn init_git_repo(repo: &Path, merge_driver_binary: &Path) -> Result<(), GitError> {
    bootstrap_repo_tree(repo)?;
    if !repo.join(".git").exists() {
        run_git(repo, &["init"])?;
    }
    configure_merge_driver(repo, merge_driver_binary)?;
    run_git(
        repo,
        &["add", ".gitattributes", ".gitignore", "config.yaml", "events/.keep", "policies/.keep", "leases/.keep"],
    )?;
    match auto_commit_with_outcome(repo, "Initialize Stream A memory substrate")? {
        CommitOutcome::NoChanges => {}
        CommitOutcome::Committed { .. } => {}
    }
    Ok(())
}

/// Configure the `memory-merge-driver` git merge driver in the local repo config.
pub fn configure_merge_driver(repo: &Path, merge_driver_binary: &Path) -> Result<(), GitError> {
    run_git(repo, &["config", "merge.memory-merge-driver.name", "Stream A memory merge driver"])?;
    run_git(
        repo,
        &[
            "config",
            "merge.memory-merge-driver.driver",
            &format!("\"{}\" --base %O --ours %A --theirs %B --path %P", merge_driver_binary.display()),
        ],
    )
    .map(|_| ())
}
