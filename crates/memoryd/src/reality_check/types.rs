use memory_substrate::{MemoryId, MemoryStatus, RecallIndexRow, Scope, Sensitivity};

use crate::protocol::ComponentScores;

pub const DEFAULT_TOP_N: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScoreWeights {
    pub staleness: f64,
    pub recall_frequency: f64,
    pub cross_source_corroboration: f64,
    pub confidence_decay: f64,
    pub sensitivity: f64,
}

impl ScoreWeights {
    pub const DEFAULT: Self = Self {
        staleness: 0.35,
        recall_frequency: 0.20,
        cross_source_corroboration: 0.20,
        confidence_decay: 0.15,
        sensitivity: 0.10,
    };

    pub fn normalized_or_default(self) -> Self {
        let sum = self.staleness
            + self.recall_frequency
            + self.cross_source_corroboration
            + self.confidence_decay
            + self.sensitivity;
        if components_are_valid(self) && (sum - 1.0).abs() <= 0.000_001 {
            self
        } else {
            Self::DEFAULT
        }
    }
}

impl Default for ScoreWeights {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScoringConfig {
    pub top_n: usize,
    pub weights: ScoreWeights,
}

impl ScoringConfig {
    pub fn with_top_n(top_n: usize) -> Self {
        Self { top_n, ..Self::default() }
    }
}

impl Default for ScoringConfig {
    fn default() -> Self {
        Self { top_n: DEFAULT_TOP_N, weights: ScoreWeights::DEFAULT }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScoredMemory {
    pub memory_id: MemoryId,
    pub summary: String,
    pub scope: Scope,
    pub canonical_namespace_id: Option<String>,
    pub status: MemoryStatus,
    pub sensitivity: Sensitivity,
    pub score: f64,
    pub component_scores: ComponentScores,
    pub recall_count_30d: u32,
    pub last_recalled_at: Option<chrono::DateTime<chrono::Utc>>,
    pub last_observed_at: chrono::DateTime<chrono::Utc>,
    pub encrypted: bool,
}

impl ScoredMemory {
    pub fn from_row(row: &RecallIndexRow, score: f64, component_scores: ComponentScores, facts: ScoreFacts) -> Self {
        Self {
            memory_id: row.id.clone(),
            summary: row.summary.clone(),
            scope: row.scope,
            canonical_namespace_id: row.canonical_namespace_id.clone(),
            status: row.status,
            sensitivity: row.sensitivity,
            score,
            component_scores,
            recall_count_30d: facts.recall_count_30d,
            last_recalled_at: facts.last_recalled_at,
            last_observed_at: facts.last_observed_at,
            encrypted: facts.encrypted,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScoreFacts {
    pub recall_count_30d: u32,
    pub last_recalled_at: Option<chrono::DateTime<chrono::Utc>>,
    pub last_observed_at: chrono::DateTime<chrono::Utc>,
    pub original_confidence: Option<f64>,
    pub distinct_sources: u32,
    pub max_recall_30d_active: u32,
    pub encrypted: bool,
}

fn components_are_valid(weights: ScoreWeights) -> bool {
    [
        weights.staleness,
        weights.recall_frequency,
        weights.cross_source_corroboration,
        weights.confidence_decay,
        weights.sensitivity,
    ]
    .into_iter()
    .all(|value| value.is_finite() && value >= 0.0)
}
