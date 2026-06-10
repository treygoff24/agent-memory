use memoryd::protocol::{ComponentScores, MemoryId};

/// The five drift-score components rendered in the Reality Check focus view,
/// mirrored from the daemon's `ComponentScores` payload. Each value is the
/// normalized [0.0, 1.0] component score; higher means more drift pressure
/// (except `corroboration`, where higher means better-grounded).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScoreBreakdown {
    pub recency: f64,
    pub recall_frequency: f64,
    pub corroboration: f64,
    pub confidence_decay: f64,
    pub sensitivity: f64,
}

impl ScoreBreakdown {
    pub const fn from_protocol(scores: &ComponentScores) -> Self {
        Self {
            recency: scores.days_since_observed_norm,
            recall_frequency: scores.recall_frequency_norm,
            corroboration: scores.cross_source_corroboration,
            confidence_decay: scores.confidence_decay,
            sensitivity: scores.sensitivity_weight,
        }
    }

    /// The five components paired with their display labels, in render order.
    pub const fn components(&self) -> [(&'static str, f64); 5] {
        [
            ("recency", self.recency),
            ("recall_frequency", self.recall_frequency),
            ("corroboration", self.corroboration),
            ("confidence_decay", self.confidence_decay),
            ("sensitivity", self.sensitivity),
        ]
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RealityCheckState {
    pub active_session_id: Option<String>,
    pub reviewed: usize,
    pub deferred: usize,
    pub items_total: usize,
    pub items_reviewed: usize,
    pub current_title: Option<String>,
    pub current_memory_id: Option<String>,
    pub current_score: Option<f64>,
    pub current_breakdown: Option<ScoreBreakdown>,
    pub current_encrypted: bool,
    pub transition_start_tick: Option<u64>,
}

impl RealityCheckState {
    pub fn progress_label(&self) -> String {
        format!("{} of {}", self.items_reviewed, self.items_total)
    }

    pub fn remaining(&self) -> usize {
        self.items_total.saturating_sub(self.items_reviewed + self.deferred)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FocusKind {
    None,
    RealityCheck { session: String },
    CorrectEditor { item_id: MemoryId },
}
