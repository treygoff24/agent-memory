//! Watch subscription handle.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use crate::error::WatchError;
use crate::model::{RepoPath, Sha256};
use crate::watcher::filter::should_watch;
use crate::watcher::SuppressionLedger;

/// File event delivered by a subscription.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileEvent {
    /// Changed path.
    pub path: PathBuf,
    /// Event kind.
    pub kind: WatchEventKind,
    /// Hash of the file contents observed by the watcher callback.
    ///
    /// This is populated for concrete path-change events when the path can be
    /// read as a file. It lets subscribers and tests distinguish a queued stale
    /// event from an event observed after different bytes were written.
    pub content_hash: Option<Sha256>,
}

/// Watch event kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WatchEventKind {
    /// A concrete path changed.
    PathChanged,
    /// Watcher overflowed and callers must run a full rescan.
    RescanRequired,
    /// Watcher encountered an internal error; subscriber should log and continue.
    WatcherError,
}

impl FileEvent {
    /// Build a full-rescan marker event for deterministic overflow handling.
    pub fn rescan_required(root: impl Into<PathBuf>) -> Self {
        Self { path: root.into(), kind: WatchEventKind::RescanRequired, content_hash: None }
    }

    fn path_changed(path: PathBuf, content_hash: Option<Sha256>) -> Self {
        Self { path, kind: WatchEventKind::PathChanged, content_hash }
    }

    fn watcher_error(path: PathBuf) -> Self {
        Self { path, kind: WatchEventKind::WatcherError, content_hash: None }
    }
}

/// Watch subscription. Dropping the handle releases OS resources.
pub struct WatchSubscription {
    receiver: Receiver<FileEvent>,
    watcher: Option<RecommendedWatcher>,
}

impl WatchSubscription {
    /// Blocking receive for tests/daemon integration.
    pub fn recv(&self) -> Result<FileEvent, WatchError> {
        self.receiver.recv().map_err(|_| WatchError::Closed)
    }

    /// Receive with timeout. Returns `WatchError::Timeout` when the deadline
    /// elapses without an event; `WatchError::Closed` when the channel is gone.
    pub fn recv_timeout(&self, timeout: Duration) -> Result<FileEvent, WatchError> {
        self.receiver.recv_timeout(timeout).map_err(|err| match err {
            RecvTimeoutError::Timeout => WatchError::Timeout,
            RecvTimeoutError::Disconnected => WatchError::Closed,
        })
    }

    /// Explicitly unsubscribe.
    pub fn unsubscribe(mut self) {
        self.watcher.take();
    }
}

/// Watch a root.
pub fn watch_root(root: &Path) -> Result<WatchSubscription, WatchError> {
    watch_root_with_suppression(root, None)
}

/// Watch a root with an optional self-event suppression ledger.
pub fn watch_root_with_suppression(
    root: &Path,
    suppression: Option<Arc<Mutex<SuppressionLedger>>>,
) -> Result<WatchSubscription, WatchError> {
    let (sender, receiver) = channel();
    let root = root.to_path_buf();
    let callback_root = root.canonicalize().unwrap_or_else(|_| root.clone());

    let overflow_sender = sender.clone();
    let error_sender = sender.clone();

    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        match event {
            Ok(event) => {
                // Overflow: OS signalled that events were dropped and a full
                // rescan is needed (spec §11.1, §11.4).
                if event.need_rescan() {
                    let _ = overflow_sender.send(FileEvent::rescan_required(&callback_root));
                    return;
                }
                for path in event.paths {
                    // Apply spec §11.2 path filters before forwarding.
                    if !should_watch(&path) {
                        continue;
                    }
                    let content_hash = observed_content_hash(&path);
                    if should_suppress(&callback_root, &path, content_hash.as_ref(), suppression.as_ref()) {
                        continue;
                    }
                    let _ = sender.send(FileEvent::path_changed(path, content_hash));
                }
            }
            Err(err) => {
                // Log the error and emit an event so subscribers know the
                // watcher has degraded — never silently discard (B-RT-5).
                tracing::warn!("watcher error: {err}");
                let _ = error_sender.send(FileEvent::watcher_error(callback_root.clone()));
            }
        }
    })
    .map_err(|err| WatchError::Setup(err.to_string()))?;

    watcher.watch(&root, RecursiveMode::Recursive).map_err(|err| WatchError::Setup(err.to_string()))?;
    Ok(WatchSubscription { receiver, watcher: Some(watcher) })
}

fn should_suppress(
    root: &Path,
    path: &Path,
    content_hash: Option<&Sha256>,
    suppression: Option<&Arc<Mutex<SuppressionLedger>>>,
) -> bool {
    let Some(suppression) = suppression else {
        return false;
    };
    let Ok(relative) = path.strip_prefix(root) else {
        return false;
    };
    let relative = relative.to_string_lossy().replace('\\', "/");
    let repo_path = RepoPath::new(relative);
    let Some(hash) = content_hash else {
        return false;
    };
    // Propagate mutex poisoning rather than silently failing open (R-RT-5).
    let Ok(mut ledger) = suppression.lock() else {
        panic!("suppression ledger mutex not poisoned");
    };
    ledger.should_suppress(&repo_path, hash)
}

fn observed_content_hash(path: &Path) -> Option<Sha256> {
    let bytes = std::fs::read(path).ok()?;
    Some(crate::markdown::hash_bytes(&bytes))
}
