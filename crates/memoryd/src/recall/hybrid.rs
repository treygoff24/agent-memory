use std::sync::Arc;
use std::time::Duration;

use memory_substrate::{HybridVectorQuery, Substrate, VectorError};

use crate::embedding::{EmbeddingProvider, EmbeddingProviderAcquire, EmbeddingProviderSlot, ProviderGuard};
use crate::recall::config::VectorRecallConfig;
use crate::recall::fusion::{fuse_rrf, FusedHybridCandidate};

pub(crate) const DEGRADED_NO_EMBEDDING_PROVIDER: &str = "no_embedding_provider";
pub(crate) const DEGRADED_EMBEDDING_DORMANT: &str = "embedding_dormant";
pub(crate) const DEGRADED_NO_ACTIVE_TRIPLE: &str = "no_active_triple";
pub(crate) const DEGRADED_TRIPLE_MISMATCH: &str = "triple_mismatch";
pub(crate) const DEGRADED_EMBEDDING_FAILED: &str = "embedding_failed";
pub(crate) const DEGRADED_NO_VECTOR_TABLE: &str = "no_vector_table";
pub(crate) const DEGRADED_KNN_FAILED: &str = "knn_failed";
pub(crate) const DEGRADED_EMBEDDING_TIMEOUT: &str = "embedding_timeout";

#[derive(Clone)]
pub struct VectorRecallContext {
    provider: VectorRecallProvider,
    pub config: VectorRecallConfig,
}

impl VectorRecallContext {
    pub fn new(provider: Option<Arc<dyn EmbeddingProvider>>, config: VectorRecallConfig) -> Self {
        Self { provider: VectorRecallProvider::Direct(provider), config }
    }

    pub fn from_lifecycle(provider_slot: EmbeddingProviderSlot, config: VectorRecallConfig) -> Self {
        Self { provider: VectorRecallProvider::Lifecycle(provider_slot), config }
    }
}

#[derive(Clone)]
enum VectorRecallProvider {
    Direct(Option<Arc<dyn EmbeddingProvider>>),
    Lifecycle(EmbeddingProviderSlot),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HydratedHybridCandidate {
    pub id: String,
    pub text: String,
    pub rrf_score: f64,
    pub final_score: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum HybridRecallDecision {
    FtsOnly { degraded: Option<&'static str> },
    Fused { candidates: Vec<HydratedHybridCandidate> },
}

pub(crate) async fn collect_hybrid_recall(
    substrate: &Substrate,
    message: &str,
    context: Option<&VectorRecallContext>,
) -> HybridRecallDecision {
    let Some(context) = context else {
        return HybridRecallDecision::FtsOnly { degraded: None };
    };
    if !context.config.enabled {
        return HybridRecallDecision::FtsOnly { degraded: None };
    }

    let active_triple = match active_triple_or_degradation(substrate.active_embedding_triple()) {
        Ok(triple) => triple,
        Err(marker) => return HybridRecallDecision::FtsOnly { degraded: Some(marker) },
    };
    let vector = match &context.provider {
        VectorRecallProvider::Direct(Some(provider)) => {
            if provider.triple() != &active_triple {
                tracing::warn!(
                    provider_triple = ?provider.triple(),
                    active_triple = ?active_triple,
                    "vector recall degraded: provider triple does not match active triple"
                );
                return HybridRecallDecision::FtsOnly { degraded: Some(DEGRADED_TRIPLE_MISMATCH) };
            }
            match embed_query_with_timeout(provider, message, context.config.embed_timeout_ms).await {
                Ok(vector) => vector,
                Err(marker) => return HybridRecallDecision::FtsOnly { degraded: Some(marker) },
            }
        }
        VectorRecallProvider::Direct(None) => {
            return HybridRecallDecision::FtsOnly { degraded: Some(DEGRADED_NO_EMBEDDING_PROVIDER) };
        }
        VectorRecallProvider::Lifecycle(provider_slot) => {
            let guard = match provider_slot.acquire_or_trigger_load() {
                EmbeddingProviderAcquire::Active(guard) => guard,
                EmbeddingProviderAcquire::Dormant | EmbeddingProviderAcquire::Loading => {
                    return HybridRecallDecision::FtsOnly { degraded: Some(DEGRADED_EMBEDDING_DORMANT) };
                }
                EmbeddingProviderAcquire::Failed { .. } => {
                    return HybridRecallDecision::FtsOnly { degraded: Some(DEGRADED_NO_EMBEDDING_PROVIDER) };
                }
            };
            if guard.triple() != &active_triple {
                tracing::warn!(
                    provider_triple = ?guard.triple(),
                    active_triple = ?active_triple,
                    "vector recall degraded: provider triple does not match active triple"
                );
                return HybridRecallDecision::FtsOnly { degraded: Some(DEGRADED_TRIPLE_MISMATCH) };
            }
            match embed_query_guard_with_timeout(guard, message, context.config.embed_timeout_ms).await {
                Ok(vector) => vector,
                Err(marker) => return HybridRecallDecision::FtsOnly { degraded: Some(marker) },
            }
        }
    };

    let candidates = match substrate
        .query_hybrid_chunks(
            message,
            Some(HybridVectorQuery { triple: &active_triple, vector: &vector }),
            context.config.knn_limit,
        )
        .await
    {
        Ok(candidates) => candidates,
        Err(VectorError::UnknownEmbeddingTriple(_)) => {
            return HybridRecallDecision::FtsOnly { degraded: Some(DEGRADED_NO_VECTOR_TABLE) };
        }
        Err(error) => {
            tracing::warn!(%error, "vector recall degraded: hybrid KNN query failed");
            return HybridRecallDecision::FtsOnly { degraded: Some(DEGRADED_KNN_FAILED) };
        }
    };

    let fused = fuse_rrf(
        candidates,
        context.config.rrf_k,
        context.config.recency_lambda,
        context.config.recency_half_life_days,
    );
    let hydrated = hydrate_fused_candidates(fused);
    HybridRecallDecision::Fused { candidates: hydrated }
}

fn active_triple_or_degradation(
    result: Result<memory_substrate::EmbeddingTriple, VectorError>,
) -> Result<memory_substrate::EmbeddingTriple, &'static str> {
    match result {
        Ok(triple) => Ok(triple),
        Err(error) => {
            tracing::warn!(%error, "vector recall degraded: active embedding triple unavailable");
            Err(DEGRADED_NO_ACTIVE_TRIPLE)
        }
    }
}

async fn embed_query_with_timeout(
    provider: &Arc<dyn EmbeddingProvider>,
    message: &str,
    timeout_ms: u64,
) -> Result<Vec<f32>, &'static str> {
    let embed_provider = Arc::clone(provider);
    let body = message.to_owned();
    let task = tokio::task::spawn_blocking(move || embed_provider.embed_query(&body));
    match tokio::time::timeout(Duration::from_millis(timeout_ms), task).await {
        Err(_) => {
            tracing::warn!(timeout_ms, "vector recall degraded: query embedding timed out");
            Err(DEGRADED_EMBEDDING_TIMEOUT)
        }
        Ok(Ok(Ok(vector))) => Ok(vector),
        Ok(Ok(Err(error))) => {
            tracing::warn!(%error, "vector recall degraded: query embedding failed");
            Err(DEGRADED_EMBEDDING_FAILED)
        }
        Ok(Err(join_error)) => {
            tracing::warn!(%join_error, "vector recall degraded: query embedding task failed");
            Err(DEGRADED_EMBEDDING_FAILED)
        }
    }
}

async fn embed_query_guard_with_timeout(
    provider: ProviderGuard,
    message: &str,
    timeout_ms: u64,
) -> Result<Vec<f32>, &'static str> {
    let body = message.to_owned();
    let task = tokio::task::spawn_blocking(move || provider.embed_query(&body));
    match tokio::time::timeout(Duration::from_millis(timeout_ms), task).await {
        Err(_) => {
            tracing::warn!(timeout_ms, "vector recall degraded: query embedding timed out");
            Err(DEGRADED_EMBEDDING_TIMEOUT)
        }
        Ok(Ok(Ok(vector))) => Ok(vector),
        Ok(Ok(Err(error))) => {
            tracing::warn!(%error, "vector recall degraded: query embedding failed");
            Err(DEGRADED_EMBEDDING_FAILED)
        }
        Ok(Err(join_error)) => {
            tracing::warn!(%join_error, "vector recall degraded: query embedding task failed");
            Err(DEGRADED_EMBEDDING_FAILED)
        }
    }
}

fn hydrate_fused_candidates(fused: Vec<FusedHybridCandidate>) -> Vec<HydratedHybridCandidate> {
    fused
        .into_iter()
        .map(|candidate| HydratedHybridCandidate {
            id: candidate.memory_id.as_str().to_owned(),
            text: candidate.text,
            rrf_score: candidate.rrf_score,
            final_score: candidate.final_score,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embedding::EmbeddingError;
    use memory_substrate::EmbeddingTriple;

    struct FailingProvider {
        triple: EmbeddingTriple,
    }

    impl EmbeddingProvider for FailingProvider {
        fn triple(&self) -> &EmbeddingTriple {
            &self.triple
        }

        fn embed_query(&self, _text: &str) -> Result<Vec<f32>, EmbeddingError> {
            Err(EmbeddingError::Inference("fixture failure".to_owned()))
        }

        fn embed_document(&self, _text: &str) -> Result<Vec<f32>, EmbeddingError> {
            Err(EmbeddingError::Inference("fixture failure".to_owned()))
        }
    }

    #[tokio::test]
    async fn embed_query_failure_maps_to_stable_marker() {
        let provider: Arc<dyn EmbeddingProvider> = Arc::new(FailingProvider {
            triple: EmbeddingTriple { provider: "p".to_owned(), model_ref: "m".to_owned(), dimension: 1 },
        });
        let marker = embed_query_with_timeout(&provider, "hello", 50).await.expect_err("failed");
        assert_eq!(marker, DEGRADED_EMBEDDING_FAILED);
    }

    #[test]
    fn active_triple_read_error_maps_to_no_active_triple_marker() {
        let marker = active_triple_or_degradation(Err(VectorError::IndexUnavailable("poisoned".to_owned())))
            .expect_err("degraded");
        assert_eq!(marker, DEGRADED_NO_ACTIVE_TRIPLE);
    }
}
