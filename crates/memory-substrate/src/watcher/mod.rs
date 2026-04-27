//! Filesystem watcher and suppression ledger.

mod filter;
mod subscription;
mod suppression;

pub use filter::{is_memory_path, should_watch};
pub use subscription::{watch_root, watch_root_with_suppression, FileEvent, WatchEventKind, WatchSubscription};
pub use suppression::{SuppressionLedger, SuppressionState};
