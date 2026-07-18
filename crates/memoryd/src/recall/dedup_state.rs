//! Per-runtime dedup state for startup recall.
//!
//! Tracks which reality-check pending-attention items and dream-attention
//! questions have already been surfaced, so the same item is not re-emitted
//! within its cool-down window. The state lives on each daemon's `HandlerState`
//! so daemons in the same process cannot bleed state into one another.
//!
//! Both fields are `Arc<Mutex<_>>` so a clone can be moved into the
//! `spawn_blocking` closure that runs the (synchronous, disk-reading) dream
//! question scan without holding a guard across an `.await`.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};

use crate::recall::dream_questions::RecentSurfacedQuestionStore;

/// Surfacing-dedup state for one daemon's startup recall.
///
/// Cloning is cheap (`Arc` bumps): clones share the same underlying state,
/// which is what lets the dream-question scan run on the blocking pool while
/// the owning `HandlerState` keeps a handle for the reality-check marker.
#[derive(Debug, Clone, Default)]
pub struct RecallDedupState {
    /// Last time the reality-check pending-attention item was surfaced, keyed
    /// by runtime root. Mirrors the on-disk marker, but cheap to consult and
    /// authoritative for the in-process single-session window.
    reality_check_surfaced_at: Arc<Mutex<BTreeMap<PathBuf, DateTime<Utc>>>>,
    /// Ring of recently-surfaced dream-attention question hashes, keyed by repo
    /// root, used to suppress re-asking the same question inside the recent
    /// window.
    recent_surfaced_questions: Arc<Mutex<RecentSurfacedQuestionStore>>,
}

impl RecallDedupState {
    /// Handle to the reality-check surfaced-at map, for the startup
    /// reality-check marker read/write.
    pub(crate) fn reality_check_surfaced_at(&self) -> &Arc<Mutex<BTreeMap<PathBuf, DateTime<Utc>>>> {
        &self.reality_check_surfaced_at
    }

    /// Handle to the dream-question dedup store. A clone of this `Arc` is moved
    /// into the blocking-pool closure that runs `select_pending_attention_questions`.
    pub(crate) fn recent_surfaced_questions(&self) -> &Arc<Mutex<RecentSurfacedQuestionStore>> {
        &self.recent_surfaced_questions
    }
}
