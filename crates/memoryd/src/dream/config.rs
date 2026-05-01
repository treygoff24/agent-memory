use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DreamConfig {
    pub enabled: bool,
    pub default_cli_priority: Vec<String>,
    pub scope_overrides: BTreeMap<String, Vec<String>>,
    pub per_pass_timeout_seconds: u64,
    pub pass_1_window_days: u16,
    pub pass_2_max_candidates: u16,
    pub pass_2_drift_threshold: f64,
    pub pass_3_max_questions: u16,
    pub pending_attention_per_scope_cap: u16,
    pub pending_attention_total_cap: u16,
    pub pending_attention_recent_window_days: u16,
    pub fragment_lifetime_days: u16,
    pub candidate_stale_days: u16,
    pub cleanup_run_hour_utc: u8,
    pub lease_window_seconds: u64,
    pub dream_retry_window_minutes: u16,
    pub events: DreamEventsConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DreamEventsConfig {
    pub compaction_days: u16,
}

impl Default for DreamConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            default_cli_priority: vec!["claude".to_owned(), "codex".to_owned()],
            scope_overrides: BTreeMap::new(),
            per_pass_timeout_seconds: 300,
            pass_1_window_days: 7,
            pass_2_max_candidates: 8,
            pass_2_drift_threshold: 0.30,
            pass_3_max_questions: 12,
            pending_attention_per_scope_cap: 2,
            pending_attention_total_cap: 6,
            pending_attention_recent_window_days: 7,
            fragment_lifetime_days: 14,
            candidate_stale_days: 30,
            cleanup_run_hour_utc: 3,
            lease_window_seconds: 3_600,
            dream_retry_window_minutes: 180,
            events: DreamEventsConfig::default(),
        }
    }
}

impl Default for DreamEventsConfig {
    fn default() -> Self {
        Self { compaction_days: 90 }
    }
}
