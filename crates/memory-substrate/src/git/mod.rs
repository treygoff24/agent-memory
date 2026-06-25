//! Git wrapper.

mod adopt;
mod command;
mod commit;
mod init;
mod preflight;
mod sync;

pub use adopt::{adopt_clone, adopt_clone_explicit, AdoptError};
pub use command::run_git;
pub use commit::{
    auto_commit, auto_commit_with_outcome, commit_lease_file, commit_substrate_writes, count_substrate_write_changes,
    CommitOutcome, LeaseCommitAction,
};
pub use init::{configure_merge_driver, init_git_repo};
pub use preflight::git_preflight;
pub use sync::{
    fetch_and_merge as fetch_and_merge_with_driver, fetch_inspect, push, QuarantineRecord, RemoteState, SyncOutcome,
};

use crate::error::GitError;
use std::path::Path;

/// Fetch and merge the configured upstream.
///
/// Compatibility wrapper that resolves the merge driver binary via `which`.
/// Deferred: accept an explicit path from callers before merge.
pub fn fetch_and_merge(repo: &Path) -> Result<(), GitError> {
    let driver = which::which("memory-merge-driver").unwrap_or_default();
    sync::fetch_and_merge(repo, &driver).map(|_| ())
}
