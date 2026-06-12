use std::path::Path;

use serde::{Deserialize, Serialize};

pub const DEFAULT_VECTOR_RECALL_ENABLED: bool = true;
pub const DEFAULT_VECTOR_RECALL_KNN_LIMIT: usize = 20;
pub const DEFAULT_VECTOR_RECALL_RRF_K: u32 = 60;
/// Small additive recency prior on fused RRF scores. Twice the old ε band (0.00025) so
/// freshness can flip adjacent near-ties deep in the list without jumping a top-of-list gap.
/// Set to `0.0` to recover pure RRF ordering.
pub const DEFAULT_VECTOR_RECALL_RECENCY_LAMBDA: f64 = 0.0005;
/// Half-life for exponential recency decay in days. Age is measured from the newest
/// candidate's `recency_at` in the result set (not wall clock), keeping eval deterministic.
pub const DEFAULT_VECTOR_RECALL_RECENCY_HALF_LIFE_DAYS: f64 = 90.0;
pub const DEFAULT_VECTOR_RECALL_EMBED_TIMEOUT_MS: u64 = 50;

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct RecallConfig {
    #[serde(default)]
    pub vector_recall: VectorRecallConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct VectorRecallConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_knn_limit")]
    pub knn_limit: usize,
    #[serde(default = "default_rrf_k")]
    pub rrf_k: u32,
    #[serde(default = "default_recency_lambda")]
    pub recency_lambda: f64,
    #[serde(default = "default_recency_half_life_days")]
    pub recency_half_life_days: f64,
    #[serde(default = "default_embed_timeout_ms")]
    pub embed_timeout_ms: u64,
}

impl Default for VectorRecallConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            knn_limit: default_knn_limit(),
            rrf_k: default_rrf_k(),
            recency_lambda: default_recency_lambda(),
            recency_half_life_days: default_recency_half_life_days(),
            embed_timeout_ms: default_embed_timeout_ms(),
        }
    }
}

fn default_enabled() -> bool {
    DEFAULT_VECTOR_RECALL_ENABLED
}

fn default_knn_limit() -> usize {
    DEFAULT_VECTOR_RECALL_KNN_LIMIT
}

fn default_rrf_k() -> u32 {
    DEFAULT_VECTOR_RECALL_RRF_K
}

fn default_recency_lambda() -> f64 {
    DEFAULT_VECTOR_RECALL_RECENCY_LAMBDA
}

fn default_recency_half_life_days() -> f64 {
    DEFAULT_VECTOR_RECALL_RECENCY_HALF_LIFE_DAYS
}

fn default_embed_timeout_ms() -> u64 {
    DEFAULT_VECTOR_RECALL_EMBED_TIMEOUT_MS
}

#[derive(Debug, Default, Deserialize)]
struct ConfigRecallEnvelope {
    #[serde(default)]
    recall: Option<RecallConfig>,
}

pub fn load_recall_config(repo: &Path) -> Result<RecallConfig, String> {
    let path = repo.join("config.yaml");
    if !path.exists() {
        return Ok(RecallConfig::default());
    }
    let text = std::fs::read_to_string(&path).map_err(|error| format!("read {}: {error}", path.display()))?;
    let envelope: ConfigRecallEnvelope =
        serde_yaml::from_str(&text).map_err(|error| format!("parse {}: {error}", path.display()))?;
    Ok(envelope.recall.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vector_recall_config_defaults_match_spec() {
        assert_eq!(
            VectorRecallConfig::default(),
            VectorRecallConfig {
                enabled: true,
                knn_limit: 20,
                rrf_k: 60,
                recency_lambda: 0.0005,
                recency_half_life_days: 90.0,
                embed_timeout_ms: 50,
            }
        );
    }

    #[test]
    fn recall_config_parses_partial_section() {
        let yaml = "recall:\n  vector_recall:\n    enabled: false\n    rrf_k: 42\n";
        let envelope: ConfigRecallEnvelope = serde_yaml::from_str(yaml).expect("parse");
        let config = envelope.recall.expect("recall present");
        assert!(!config.vector_recall.enabled);
        assert_eq!(config.vector_recall.rrf_k, 42);
        assert_eq!(config.vector_recall.knn_limit, 20);
        assert_eq!(config.vector_recall.embed_timeout_ms, 50);
    }

    #[test]
    fn load_recall_config_defaults_when_file_absent() {
        let temp = tempfile::tempdir().expect("tempdir");
        assert_eq!(load_recall_config(temp.path()).expect("load"), RecallConfig::default());
    }

    #[test]
    fn load_recall_config_ignores_unrelated_config() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            temp.path().join("config.yaml"),
            "schema_version: 1\nactive_embedding:\n  provider: synthetic\n  model_ref: stream-a-test\n  dimension: 32\n",
        )
        .expect("write config");
        assert_eq!(load_recall_config(temp.path()).expect("load"), RecallConfig::default());
    }
}
