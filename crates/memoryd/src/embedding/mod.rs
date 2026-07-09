//! Production embedding inference (Stream B, Task 3.0).
//!
//! Every write produces chunks and `pending_embedding_jobs` against the active
//! embedding triple, but until this module shipped nothing consumed them — the
//! only vector-write API (`Substrate::update_embedding`) was exercised solely by
//! benches and tests, so production recall silently degraded to FTS-only bm25.
//!
//! This module supplies:
//!
//! - [`EmbeddingProvider`], a small trait with **asymmetric** prompting:
//!   [`EmbeddingProvider::embed_query`] applies the model-card instruction
//!   prompt; [`EmbeddingProvider::embed_document`] embeds plain text. Collapsing
//!   the two measurably degrades retrieval, so both are part of the contract.
//! - [`FastembedProvider`], the production lane: Qwen3-Embedding-0.6B via the
//!   fastembed candle backend (Metal GPU, with an Apple-BLAS CPU fallback).
//! - [`FixtureProvider`], a deterministic test/CI lane backed by
//!   content-derived hashed vectors, implementing the same trait so the drain
//!   loop and e2e tests run with no model download.
//! - [`worker`], the daemon background task that drains the backlog.
//!
//! Invariant 3 (spec §10.2.2): the embedding triple `(provider, model_ref,
//! dimension)` is identity, never flavor. A provider whose output length does
//! not match its declared `dimension` is a bug surfaced as
//! [`EmbeddingError::DimensionMismatch`], never silently truncated or padded.
//!
//! Metal and CPU use different numeric lanes for the same triple: Metal loads
//! Qwen3 as fp16, CPU as fp32. The dtype is intentionally treated as a compute
//! flavor rather than identity, so small numeric drift can coexist in one vector
//! table across restarts without changing `(provider, model_ref, dimension)`.

mod api_provider;
mod fastembed_provider;
mod fixture_provider;
pub mod lifecycle;
mod prompts;
pub mod worker;

#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use api_provider::test_support as api_test_support;
pub use api_provider::{
    is_gemini_api_triple, read_gemini_api_key, write_gemini_api_key, ApiEmbeddingProvider, ApiKey,
    GEMINI_API_DEFAULT_MODEL_REF, GEMINI_API_PROVIDER, GEMINI_API_RECOMMENDED_DIMENSION,
};
pub use fastembed_provider::{is_fastembed_candle_triple, FastembedProvider, LoadedDevice, FASTEMBED_CANDLE_PROVIDER};
pub use fixture_provider::FixtureProvider;
pub use lifecycle::{
    EmbeddingIdleWindow, EmbeddingLifecycleSnapshot, EmbeddingProviderAcquire, EmbeddingProviderSlot, ProviderGuard,
};

use std::sync::Mutex;

use memory_substrate::{EmbeddingLaneEligibility, EmbeddingTriple};

static MODEL_LOAD_FAILURE: Mutex<Option<String>> = Mutex::new(None);

/// Record the last provider-load failure so `doctor` can explain why the worker
/// is retrying instead of silently leaving recall FTS-only.
pub(crate) fn record_model_load_failure(error: impl Into<String>) {
    match MODEL_LOAD_FAILURE.lock() {
        Ok(mut guard) => {
            *guard = Some(error.into());
        }
        Err(error) => tracing::error!(%error, "embedding model-load status lock poisoned while recording failure"),
    }
}

/// Clear the provider-load failure once a retry succeeds.
pub(crate) fn clear_model_load_failure() {
    match MODEL_LOAD_FAILURE.lock() {
        Ok(mut guard) => {
            *guard = None;
        }
        Err(error) => tracing::error!(%error, "embedding model-load status lock poisoned while clearing failure"),
    }
}

/// Last provider-load failure recorded by the server load loop.
pub(crate) fn model_load_failure() -> Option<String> {
    match MODEL_LOAD_FAILURE.lock() {
        Ok(guard) => guard.clone(),
        Err(error) => {
            tracing::error!(%error, "embedding model-load status lock poisoned while reading failure");
            None
        }
    }
}

/// Whether the active triple is served by an off-device API embedding lane.
///
/// The single product-policy predicate for "does content leave the machine to be
/// embedded?" — every fence site (governance write-path, worker drain, status,
/// doctor) routes through this so a future second API provider is enabled by
/// extending one function, not by editing each fence in lockstep.
pub fn is_api_embedding_lane(triple: &EmbeddingTriple) -> bool {
    is_gemini_api_triple(triple)
}

/// Product policy for deciding which queued embedding jobs may transit the
/// active embedding lane.
pub fn embedding_lane_eligibility(triple: &EmbeddingTriple) -> EmbeddingLaneEligibility {
    if is_api_embedding_lane(triple) {
        EmbeddingLaneEligibility::PlaintextOnly
    } else {
        EmbeddingLaneEligibility::AllTiers
    }
}

/// Failure modes for embedding inference.
#[derive(Debug, thiserror::Error)]
pub enum EmbeddingError {
    /// The model could not be loaded (download, weights, tokenizer, device).
    #[error("embedding model load failed: {0}")]
    Load(String),
    /// Inference failed for a specific input.
    #[error("embedding inference failed: {0}")]
    Inference(String),
    /// The API provider is missing credentials or the remote rejected them.
    #[error("embedding API authentication failed: {0}")]
    Auth(String),
    /// The API provider asked the caller to back off.
    #[error("embedding API rate-limited: {message}")]
    RateLimit {
        /// Parsed `Retry-After` delay, when the server supplied one.
        retry_after: Option<std::time::Duration>,
        /// Human-readable remote error context.
        message: String,
    },
    /// The API request could not be delivered or completed.
    #[error("embedding API transport failed: {0}")]
    Transport(String),
    /// The API response did not match the provider contract.
    #[error("embedding API contract failed: {0}")]
    Contract(String),
    /// The produced vector length disagreed with the provider's declared
    /// dimension. Per invariant 3 this is a hard error, never a silent
    /// truncate/pad — a mismatch means the configured triple does not describe
    /// the model that produced the vector.
    #[error("embedding dimension mismatch: triple declares {expected}, model produced {found}")]
    DimensionMismatch {
        /// Dimension declared by the active triple.
        expected: u32,
        /// Vector length the model actually produced.
        found: u32,
    },
}

/// A local embedding model behind asymmetric query/document prompting.
///
/// Implementations are synchronous: the fastembed candle path blocks on CPU/GPU
/// compute, so callers on the async runtime must invoke it under
/// `spawn_blocking` (the [`worker`] drain loop does this).
pub trait EmbeddingProvider: Send + Sync {
    /// The embedding triple this provider produces vectors for.
    fn triple(&self) -> &EmbeddingTriple;

    /// Embed a query string with the model-card instruction prompt applied.
    fn embed_query(&self, text: &str) -> Result<Vec<f32>, EmbeddingError>;

    /// Embed a document/chunk string as plain text (no instruction prompt).
    fn embed_document(&self, text: &str) -> Result<Vec<f32>, EmbeddingError>;

    /// Embed a batch of document/chunk strings as plain text, returning one
    /// vector per input in positional order.
    ///
    /// The default implementation loops over [`Self::embed_document`], so a
    /// provider gets correct batch behavior for free; transformer-backed
    /// providers (fastembed/candle) override this to amortize the forward pass
    /// over the whole slice, which is several times faster per item at batch
    /// sizes typical of a cold reindex. The per-item results are byte-identical
    /// to calling `embed_document` once per text, so the drain worker's
    /// stale-chunk keying is unaffected.
    fn embed_documents(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        texts.iter().map(|text| self.embed_document(text)).collect()
    }
}

/// Validate a produced vector against the provider's declared dimension.
///
/// Shared by every provider so the invariant-3 check is spelled once.
fn check_dimension(triple: &EmbeddingTriple, vector: &[f32]) -> Result<(), EmbeddingError> {
    if vector.len() == triple.dimension as usize {
        Ok(())
    } else {
        Err(EmbeddingError::DimensionMismatch { expected: triple.dimension, found: vector.len() as u32 })
    }
}

#[cfg(test)]
pub(crate) mod lane_test_support {
    use chrono::{DateTime, Utc};
    use memory_substrate::index::{open_index, Index};
    use memory_substrate::{
        Author, AuthorKind, Frontmatter, Memory, MemoryId, MemoryStatus, MemoryType, RepoPath, RetrievalPolicy, Roots,
        Scope, Sensitivity, Source, SourceKind, Substrate, TrustLevel, WritePolicy,
    };

    pub(crate) fn gemini_test_triple() -> memory_substrate::EmbeddingTriple {
        memory_substrate::EmbeddingTriple {
            provider: super::GEMINI_API_PROVIDER.to_string(),
            model_ref: "gemini-embedding-2".to_string(),
            dimension: 768,
        }
    }

    pub(crate) fn local_test_triple() -> memory_substrate::EmbeddingTriple {
        memory_substrate::EmbeddingTriple {
            provider: super::FASTEMBED_CANDLE_PROVIDER.to_string(),
            model_ref: memory_substrate::tree::DEFAULT_ACTIVE_EMBEDDING_MODEL_REF.to_string(),
            dimension: memory_substrate::tree::DEFAULT_ACTIVE_EMBEDDING_DIMENSION,
        }
    }

    pub(crate) async fn init_substrate_with_active_embedding(
        triple: memory_substrate::EmbeddingTriple,
        device_id: &str,
    ) -> (tempfile::TempDir, Substrate) {
        let temp = tempfile::tempdir().expect("tempdir");
        let repo = temp.path().join("repo");
        let runtime = temp.path().join("runtime");
        std::fs::create_dir_all(&repo).expect("repo dir");
        std::fs::write(
            repo.join("config.yaml"),
            format!(
                "schema_version: 1\nactive_embedding:\n  provider: {}\n  model_ref: {}\n  dimension: {}\n",
                triple.provider, triple.model_ref, triple.dimension
            ),
        )
        .expect("write config");
        let substrate = Substrate::init(
            Roots::new(&repo, &runtime),
            memory_substrate::InitOptions { force_unsafe_durability: true, device_id: Some(device_id.to_string()) },
        )
        .await
        .expect("substrate init");
        (temp, substrate)
    }

    pub(crate) fn seed_indexed_memory(substrate: &Substrate, id: &str, sensitivity: Sensitivity, body: &str) {
        let triple = substrate.active_embedding_triple().expect("active triple");
        let connection = open_index(&substrate.roots().runtime.join("index.sqlite")).expect("open index");
        let mut index = Index::with_active_embedding(connection, triple);
        index.upsert_memory(&memory(id, sensitivity, body), false).expect("upsert indexed memory");
    }

    fn memory(id: &str, sensitivity: Sensitivity, body: &str) -> Memory {
        let now = instant("2026-07-09T12:00:00Z");
        Memory {
            frontmatter: Frontmatter {
                schema_version: memory_substrate::SUBSTRATE_SCHEMA_VERSION,
                id: MemoryId::new(id),
                memory_type: MemoryType::Pattern,
                scope: Scope::Agent,
                summary: "embedding lane accounting fixture".to_string(),
                confidence: 1.0,
                original_confidence: None,
                trust_level: TrustLevel::Trusted,
                sensitivity,
                status: MemoryStatus::Active,
                created_at: now,
                updated_at: now,
                observed_at: None,
                author: Author {
                    kind: AuthorKind::System,
                    user_handle: None,
                    harness: None,
                    harness_version: None,
                    session_id: None,
                    subagent_id: None,
                    phase: None,
                    component: Some("memoryd-test".to_string()),
                },
                namespace: None,
                canonical_namespace_id: None,
                tags: Vec::new(),
                entities: Vec::new(),
                aliases: Vec::new(),
                source: Source {
                    kind: SourceKind::Import,
                    reference: None,
                    harness: None,
                    harness_version: None,
                    session_id: None,
                    subagent_id: None,
                    device: None,
                },
                evidence: Vec::new(),
                requires_user_confirmation: false,
                review_state: None,
                supersedes: Vec::new(),
                superseded_by: Vec::new(),
                related: Vec::new(),
                tombstone_events: Vec::new(),
                retrieval_policy: RetrievalPolicy {
                    passive_recall: true,
                    max_scope: Scope::Agent,
                    mask_personal_for_synthesis: matches!(
                        sensitivity,
                        Sensitivity::Confidential | Sensitivity::Personal
                    ),
                    index_body: true,
                    index_embeddings: true,
                },
                write_policy: WritePolicy {
                    human_review_required: false,
                    policy_applied: "memoryd-test".to_string(),
                    expected_base_hash: None,
                },
                merge_diagnostics: None,
                extras: Default::default(),
            },
            body: body.to_string(),
            path: Some(RepoPath::new(format!("agent/patterns/{id}.md"))),
        }
    }

    fn instant(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value).expect("date").with_timezone(&Utc)
    }
}
