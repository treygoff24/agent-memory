use std::sync::Arc;
use std::time::Duration;

use memory_substrate::{HybridVectorQuery, MemoryContent, Substrate, VectorError};

use crate::embedding::EmbeddingProvider;
use crate::recall::config::VectorRecallConfig;
use crate::recall::fusion::{fuse_rrf, FusedHybridCandidate};

pub(crate) const DEGRADED_NO_EMBEDDING_PROVIDER: &str = "no_embedding_provider";
pub(crate) const DEGRADED_NO_ACTIVE_TRIPLE: &str = "no_active_triple";
pub(crate) const DEGRADED_TRIPLE_MISMATCH: &str = "triple_mismatch";
pub(crate) const DEGRADED_EMBEDDING_FAILED: &str = "embedding_failed";
pub(crate) const DEGRADED_NO_VECTOR_TABLE: &str = "no_vector_table";
pub(crate) const DEGRADED_KNN_FAILED: &str = "knn_failed";
pub(crate) const DEGRADED_EMBEDDING_TIMEOUT: &str = "embedding_timeout";

#[derive(Clone)]
pub struct VectorRecallContext {
    pub provider: Option<Arc<dyn EmbeddingProvider>>,
    pub config: VectorRecallConfig,
}

impl VectorRecallContext {
    pub fn new(provider: Option<Arc<dyn EmbeddingProvider>>, config: VectorRecallConfig) -> Self {
        Self { provider, config }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HydratedHybridCandidate {
    pub id: String,
    pub text: String,
    pub rrf_score: f64,
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

    let Some(provider) = context.provider.as_ref() else {
        return HybridRecallDecision::FtsOnly { degraded: Some(DEGRADED_NO_EMBEDDING_PROVIDER) };
    };

    let active_triple = match active_triple_or_degradation(substrate.active_embedding_triple()) {
        Ok(triple) => triple,
        Err(marker) => return HybridRecallDecision::FtsOnly { degraded: Some(marker) },
    };
    if provider.triple() != &active_triple {
        tracing::warn!(
            provider_triple = ?provider.triple(),
            active_triple = ?active_triple,
            "vector recall degraded: provider triple does not match active triple"
        );
        return HybridRecallDecision::FtsOnly { degraded: Some(DEGRADED_TRIPLE_MISMATCH) };
    }

    let vector = match embed_query_with_timeout(provider, message, context.config.embed_timeout_ms).await {
        Ok(vector) => vector,
        Err(marker) => return HybridRecallDecision::FtsOnly { degraded: Some(marker) },
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

    let fused = fuse_rrf(&candidates, context.config.rrf_k);
    let hydrated = hydrate_fused_candidates(substrate, fused).await;
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

async fn hydrate_fused_candidates(
    substrate: &Substrate,
    fused: Vec<FusedHybridCandidate>,
) -> Vec<HydratedHybridCandidate> {
    let mut hydrated = Vec::with_capacity(fused.len());
    for candidate in fused {
        let memory_id = candidate.memory_id.clone();
        let substrate = substrate.clone();
        let envelope = match tokio::task::spawn_blocking(move || substrate.read_memory_envelope_blocking(&memory_id))
            .await
        {
            Ok(Ok(envelope)) => envelope,
            Ok(Err(error)) => {
                tracing::warn!(memory_id = %candidate.memory_id.as_str(), %error, "vector recall candidate hydration failed");
                continue;
            }
            Err(join_error) => {
                tracing::warn!(memory_id = %candidate.memory_id.as_str(), %join_error, "vector recall hydration task failed");
                continue;
            }
        };
        let text = match envelope.content {
            MemoryContent::Plaintext(body) if !body.trim().is_empty() => body,
            MemoryContent::Plaintext(_) => envelope.metadata.frontmatter.summary,
            MemoryContent::Ciphertext { .. } | MemoryContent::MetadataOnly => {
                tracing::warn!(memory_id = %candidate.memory_id.as_str(), "vector recall hydration skipped non-plaintext memory");
                continue;
            }
        };
        hydrated.push(HydratedHybridCandidate {
            id: candidate.memory_id.as_str().to_owned(),
            text,
            rrf_score: candidate.rrf_score,
        });
    }
    hydrated
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
