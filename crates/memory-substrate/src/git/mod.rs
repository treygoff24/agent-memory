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
    uncommitted_substrate_paths, CommitOutcome, LeaseCommitAction,
};
pub use init::{configure_merge_driver, init_git_repo};
pub use preflight::git_preflight;
pub use sync::{fetch_and_merge, fetch_inspect, push, QuarantineRecord, RemoteState, SyncOutcome};
