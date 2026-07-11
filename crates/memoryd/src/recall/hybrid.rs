use std::sync::Arc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use memory_substrate::{HybridVectorQuery, MemoryContent, Substrate, VectorError};

use crate::embedding::{EmbeddingProvider, EmbeddingProviderAcquire, EmbeddingProviderSlot, ProviderGuard};
use crate::recall::config::{VectorRecallConfig, HOOK_DEADLINE_MS};
use crate::recall::fusion::{
    apply_recency_prior_and_sort, fuse_four_lane_rrf, fuse_rrf, FourLaneFusionConfig, FourLaneWeights,
    FusedHybridCandidate,
};

pub(crate) const DEGRADED_NO_EMBEDDING_PROVIDER: &str = "no_embedding_provider";
pub(crate) const DEGRADED_EMBEDDING_DORMANT: &str = "embedding_dormant";
pub(crate) const DEGRADED_NO_ACTIVE_TRIPLE: &str = "no_active_triple";
pub(crate) const DEGRADED_TRIPLE_MISMATCH: &str = "triple_mismatch";
pub(crate) const DEGRADED_EMBEDDING_FAILED: &str = "embedding_failed";
pub(crate) const DEGRADED_NO_VECTOR_TABLE: &str = "no_vector_table";
pub(crate) const DEGRADED_KNN_FAILED: &str = "knn_failed";
pub(crate) const DEGRADED_EMBEDDING_TIMEOUT: &str = "embedding_timeout";
pub(crate) const DEGRADED_FOUR_LANE_TIMEOUT: &str = "four_lane_timeout";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FusionMode {
    Legacy,
    FourLaneHook,
    FourLaneSearch,
}

#[derive(Clone)]
pub struct VectorRecallContext {
    provider: VectorRecallProvider,
    pub config: VectorRecallConfig,
    mode: FusionMode,
}

impl VectorRecallContext {
    pub fn new(provider: Option<Arc<dyn EmbeddingProvider>>, config: VectorRecallConfig) -> Self {
        Self { provider: VectorRecallProvider::Direct(provider), config, mode: FusionMode::Legacy }
    }

    pub fn from_lifecycle(provider_slot: EmbeddingProviderSlot, config: VectorRecallConfig) -> Self {
        Self { provider: VectorRecallProvider::Lifecycle(provider_slot), config, mode: FusionMode::Legacy }
    }

    pub fn with_mode(mut self, mode: FusionMode) -> Self {
        self.mode = mode;
        self
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
    Fused { candidates: Vec<HydratedHybridCandidate>, degraded: Option<&'static str> },
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
            let timeout_ms = effective_embed_budget_ms(context, &active_triple);
            match embed_query_with_timeout(provider, message, timeout_ms).await {
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
            let timeout_ms = effective_embed_budget_ms(context, &active_triple);
            match embed_query_guard_with_timeout(guard, message, timeout_ms).await {
                Ok(vector) => vector,
                Err(marker) => return HybridRecallDecision::FtsOnly { degraded: Some(marker) },
            }
        }
    };

    if context.mode != FusionMode::Legacy && context.config.four_lane_enabled {
        let started = Instant::now();
        let deadline_ms = match context.mode {
            FusionMode::FourLaneHook => HOOK_DEADLINE_MS,
            FusionMode::FourLaneSearch => context.config.search_timeout_ms,
            FusionMode::Legacy => unreachable!("legacy mode is handled above"),
        };
        let deadline = Duration::from_millis(deadline_ms);
        let remaining = deadline.saturating_sub(started.elapsed());
        return collect_four_lane_recall(
            substrate,
            message,
            context,
            HybridVectorQuery { triple: &active_triple, vector: &vector },
            remaining,
        )
        .await;
    }

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
    HybridRecallDecision::Fused { candidates: hydrated, degraded: None }
}

#[allow(clippy::too_many_arguments)]
async fn collect_four_lane_recall(
    substrate: &Substrate,
    message: &str,
    context: &VectorRecallContext,
    vector_query: HybridVectorQuery<'_>,
    remaining: Duration,
) -> HybridRecallDecision {
    // Run the four primitive lanes concurrently. Each is timed by its own per-
    // lane budget; the whole set is also bounded by the remaining post-embed
    // envelope. The blocking threads underlying `spawn_blocking` queries stay
    // parked until their SQLite query returns, so index-connection contention
    // can still serialize them past the wall budget; the timeout surfaces that
    // as a degraded lane rather than a hang.
    let lane_timeout = std::cmp::min(Duration::from_millis(context.config.four_lane_timeout_ms), remaining);
    let (fts, chunk, abstractions, cues) = tokio::join!(
        lane_result("fts", lane_timeout, substrate.query_hybrid_chunks(message, None, context.config.knn_limit)),
        lane_result(
            "chunk-vector",
            lane_timeout,
            substrate.query_hybrid_chunks(message, Some(vector_query), context.config.knn_limit)
        ),
        lane_result(
            "abstraction-vector",
            lane_timeout,
            substrate.query_abstraction_vectors(vector_query.triple, vector_query.vector, context.config.knn_limit)
        ),
        lane_result(
            "cue-vector",
            lane_timeout,
            substrate.query_cue_vectors(
                vector_query.triple,
                vector_query.vector,
                context.config.knn_limit.saturating_mul(3)
            )
        )
    );

    let timed_out = fts.timed_out || chunk.timed_out || abstractions.timed_out || cues.timed_out;
    let mut candidates = fts.value.unwrap_or_default();
    if let Some(chunk_candidates) = chunk.value {
        merge_chunk_candidates(&mut candidates, chunk_candidates);
    }
    let mut fused = fuse_four_lane_rrf(
        candidates,
        abstractions.value.unwrap_or_default(),
        cues.value.unwrap_or_default(),
        FourLaneFusionConfig {
            rrf_k: context.config.rrf_k,
            weights: FourLaneWeights {
                chunk_vector: context.config.chunk_vector_weight,
                bm25: context.config.bm25_weight,
                abstraction_vector: context.config.abstraction_vector_weight,
                cue_vector: context.config.cue_vector_weight,
            },
            recency_lambda: context.config.recency_lambda,
            recency_half_life_days: context.config.recency_half_life_days,
        },
    );
    hydrate_aux_only_candidates(substrate, &mut fused).await;
    apply_recency_prior_and_sort(&mut fused, context.config.recency_lambda, context.config.recency_half_life_days);
    HybridRecallDecision::Fused {
        candidates: hydrate_fused_candidates(fused),
        degraded: timed_out.then_some(DEGRADED_FOUR_LANE_TIMEOUT),
    }
}

struct LaneResult<T> {
    value: Option<T>,
    timed_out: bool,
}

async fn lane_result<T>(
    lane: &'static str,
    timeout: Duration,
    future: impl std::future::Future<Output = Result<T, VectorError>>,
) -> LaneResult<T> {
    match tokio::time::timeout(timeout, future).await {
        Ok(Ok(value)) => LaneResult { value: Some(value), timed_out: false },
        Ok(Err(error)) => {
            tracing::warn!(lane, %error, "four-lane recall lane unavailable; fusing remaining lanes");
            LaneResult { value: None, timed_out: false }
        }
        Err(_) => {
            tracing::warn!(
                lane,
                timeout_ms = timeout.as_millis(),
                "four-lane recall lane timed out; fusing remaining lanes"
            );
            LaneResult { value: None, timed_out: true }
        }
    }
}

fn merge_chunk_candidates(
    candidates: &mut Vec<memory_substrate::HybridMemoryCandidate>,
    chunk_candidates: Vec<memory_substrate::HybridMemoryCandidate>,
) {
    let mut by_id = candidates
        .drain(..)
        .map(|candidate| (candidate.memory_id.clone(), candidate))
        .collect::<std::collections::BTreeMap<_, _>>();
    for incoming in chunk_candidates {
        by_id
            .entry(incoming.memory_id.clone())
            .and_modify(|candidate| {
                candidate.score_breakdown.cosine_similarity = incoming.score_breakdown.cosine_similarity;
                if candidate.text.is_empty() {
                    candidate.text.clone_from(&incoming.text);
                }
                candidate.recency_at = candidate.recency_at.max(incoming.recency_at);
            })
            .or_insert(incoming);
    }
    candidates.extend(by_id.into_values());
}

async fn hydrate_aux_only_candidates(substrate: &Substrate, candidates: &mut Vec<FusedHybridCandidate>) {
    for candidate in candidates.iter_mut().filter(|candidate| candidate.text.is_empty()) {
        match substrate.read_memory_envelope(&candidate.memory_id).await {
            Ok(envelope) => match envelope.content {
                MemoryContent::Plaintext(body) => {
                    let frontmatter = envelope.metadata.frontmatter;
                    candidate.recency_at =
                        Some(frontmatter.observed_at.unwrap_or(frontmatter.updated_at).max(frontmatter.updated_at));
                    candidate.text = body;
                }
                MemoryContent::Ciphertext { .. } => {
                    tracing::debug!(memory_id = %candidate.memory_id, "dropping auxiliary-only ciphertext hit from recall");
                }
                MemoryContent::MetadataOnly => {
                    tracing::debug!(memory_id = %candidate.memory_id, "dropping auxiliary-only metadata-only hit from recall");
                }
            },
            Err(error) => {
                tracing::warn!(memory_id = %candidate.memory_id, %error, "four-lane recall could not hydrate auxiliary-only hit")
            }
        }
    }
    candidates.retain(|candidate| !candidate.text.is_empty());
}

fn effective_embed_budget_ms(context: &VectorRecallContext, triple: &memory_substrate::EmbeddingTriple) -> u64 {
    let lane_budget = context.config.effective_embed_timeout_ms(triple);
    let deadline_ms = match context.mode {
        FusionMode::Legacy => return lane_budget,
        FusionMode::FourLaneHook => HOOK_DEADLINE_MS,
        FusionMode::FourLaneSearch => context.config.search_timeout_ms,
    };
    let reserve = measured_fusion_reserve();
    let residual =
        Duration::from_millis(deadline_ms).saturating_sub(reserve).as_millis().min(u128::from(u64::MAX)) as u64;
    // reserve >= deadline saturates residual to zero, so embedding gets a zero-
    // budget timeout and the caller falls back to FTS-only.
    lane_budget.min(residual)
}

/// Advisory lower-bound on fusion overhead, measured on empty inputs.
///
/// The hook embed budget uses this as a proxy, but the real bound is the F2
/// envelope (lanes + fusion + hydration). Empty-input calibration is a cheap
/// proxy, not a guarantee on real workloads.
fn measured_fusion_reserve() -> Duration {
    static RESERVE: OnceLock<Duration> = OnceLock::new();
    *RESERVE.get_or_init(|| {
        let started = Instant::now();
        for _ in 0..32 {
            let _ = fuse_four_lane_rrf(
                Vec::new(),
                Vec::new(),
                Vec::new(),
                FourLaneFusionConfig {
                    rrf_k: 60,
                    weights: FourLaneWeights { chunk_vector: 1.0, bm25: 1.0, abstraction_vector: 2.0, cue_vector: 1.0 },
                    recency_lambda: 0.0,
                    recency_half_life_days: 90.0,
                },
            );
        }
        started.elapsed() / 32
    })
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
    use crate::recall::config::API_EMBED_TIMEOUT_MS;
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

    /// W4-F4 pin: hydrate_aux_only_candidates surfaces content ONLY from
    /// Plaintext canonical envelopes. A candidate whose canonical memory is
    /// missing (the stale-index race class — the same fail-closed match family
    /// as Ciphertext/MetadataOnly) is dropped; a plaintext one hydrates.
    #[tokio::test]
    async fn aux_only_hydration_is_plaintext_fail_closed() {
        use memory_substrate::{
            ClassificationOutcome, EventContext, InitOptions, Roots, Substrate, WriteMode, WriteRequest,
        };
        let temp = tempfile::tempdir().expect("tempdir");
        let substrate = Substrate::init(
            Roots::new(temp.path().join("repo"), temp.path().join("runtime")),
            InitOptions { force_unsafe_durability: true, device_id: Some("dev_hydrate".into()) },
        )
        .await
        .expect("init substrate");
        let memory = memory_substrate::frontmatter::parse_document(
            "---\nschema_version: 1\nid: mem_20260610_00000000000000aa_000001\ntype: pattern\nscope: agent\nsummary: hydratable\nconfidence: 1.0\ntrust_level: trusted\nsensitivity: internal\nstatus: active\ncreated_at: 2026-06-10T00:00:00Z\nupdated_at: 2026-06-10T00:00:00Z\nauthor:\n  kind: system\n  component: test\n---\nplaintext body\n",
            Some(memory_substrate::RepoPath::new("agent/patterns/mem_20260610_00000000000000aa_000001.md")),
        )
        .expect("parse")
        .memory;
        substrate
            .write_memory(WriteRequest {
                operation_id: None,
                memory,
                expected_base_hash: None,
                write_mode: WriteMode::CreateNew,
                index_projection: None,
                event_context: EventContext::default(),
                allow_best_effort_durability: true,
                classification: ClassificationOutcome::Trusted,
            })
            .await
            .expect("write plaintext");

        let aux_only = |id: &str| FusedHybridCandidate {
            memory_id: memory_substrate::MemoryId::new(id),
            text: String::new(),
            score_breakdown: Default::default(),
            rrf_score: 1.0,
            recency_at: None,
            final_score: 1.0,
        };
        let mut candidates = vec![
            aux_only("mem_20260610_00000000000000aa_000001"),
            aux_only("mem_20260610_00000000000000bb_000002"), // no canonical file — the race class
        ];
        hydrate_aux_only_candidates(&substrate, &mut candidates).await;
        assert_eq!(candidates.len(), 1, "unreadable aux-only hit must be dropped");
        assert_eq!(candidates[0].memory_id.as_str(), "mem_20260610_00000000000000aa_000001");
        assert_eq!(candidates[0].text.trim_end(), "plaintext body");
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

    #[tokio::test]
    async fn timed_out_lane_is_omitted_without_panicking() {
        let result = lane_result::<Vec<()>>(
            "fixture",
            Duration::from_millis(1),
            std::future::pending::<Result<Vec<()>, VectorError>>(),
        )
        .await;
        assert!(result.timed_out);
        assert!(result.value.is_none());
    }

    #[test]
    fn legacy_embed_budget_is_uncapped() {
        let api = EmbeddingTriple {
            provider: "gemini-api".to_owned(),
            model_ref: "gemini-embedding-2".to_owned(),
            dimension: 768,
        };
        let context = VectorRecallContext::new(None, VectorRecallConfig::default()).with_mode(FusionMode::Legacy);
        assert_eq!(effective_embed_budget_ms(&context, &api), API_EMBED_TIMEOUT_MS);
    }

    #[test]
    fn search_embed_budget_saturates_at_zero_deadline() {
        let api = EmbeddingTriple {
            provider: "gemini-api".to_owned(),
            model_ref: "gemini-embedding-2".to_owned(),
            dimension: 768,
        };
        let config = VectorRecallConfig {
            search_timeout_ms: 0,
            embed_timeout_ms: Some(10_000),
            ..VectorRecallConfig::default()
        };
        let context = VectorRecallContext::new(None, config).with_mode(FusionMode::FourLaneSearch);
        // reserve >= deadline (0 ms) saturates residual to 0, so the API lane
        // gets a zero budget and the call will fall back to FTS-only.
        assert_eq!(effective_embed_budget_ms(&context, &api), 0);
    }

    #[test]
    fn hook_embed_budget_caps_at_hook_deadline() {
        let local = EmbeddingTriple {
            provider: "fastembed-candle".to_owned(),
            model_ref: "all-minilm-l6-v2".to_owned(),
            dimension: 384,
        };
        let config = VectorRecallConfig { embed_timeout_ms: Some(10_000), ..VectorRecallConfig::default() };
        let context = VectorRecallContext::new(None, config).with_mode(FusionMode::FourLaneHook);
        let budget = effective_embed_budget_ms(&context, &local);
        assert!(budget <= HOOK_DEADLINE_MS, "hook budget {budget} should not exceed {HOOK_DEADLINE_MS} ms");
        assert!(budget < 10_000, "hook budget should be capped by the deadline, not the lane timeout");
    }
}
