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
//!   `memory-test-support`'s sanctioned synthetic vectors, implementing the same
//!   trait so the drain loop and e2e tests run with no model download.
//! - [`worker`], the daemon background task that drains the backlog.
//!
//! Invariant 3 (spec §10.2.2): the embedding triple `(provider, model_ref,
//! dimension)` is identity, never flavor. A provider whose output length does
//! not match its declared `dimension` is a bug surfaced as
//! [`EmbeddingError::DimensionMismatch`], never silently truncated or padded.

mod fastembed_provider;
mod fixture_provider;
mod prompts;
pub mod worker;

pub use fastembed_provider::{FastembedProvider, LoadedDevice};
pub use fixture_provider::FixtureProvider;

use std::sync::{Arc, RwLock};

use memory_substrate::EmbeddingTriple;

/// A late-initialized, shareable handle to the active embedding provider.
///
/// The provider loads asynchronously a moment after daemon startup (the
/// fastembed model load is CPU/GPU-heavy and must not gate socket binding), so
/// the slot starts empty and the embedding worker publishes the provider into it
/// once loaded. Consumers that need to embed on a request path — governance
/// contradiction detection embeds the candidate text — clone this slot from
/// `HandlerState` and read the current provider.
///
/// An empty slot is not an error: it means embedding inference is unavailable
/// (model not yet loaded, load failed, or the worker is disabled). Governance
/// treats that as "no similarity candidates" and records the degradation in the
/// decision trace rather than silently behaving as if nothing was similar
/// (invariant 3: no silent fallback).
#[derive(Clone, Default)]
pub struct EmbeddingProviderSlot {
    inner: Arc<RwLock<Option<Arc<dyn EmbeddingProvider>>>>,
}

impl EmbeddingProviderSlot {
    /// An empty slot — no provider published yet.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Publish the loaded provider so request-path consumers can embed with it.
    pub fn set(&self, provider: Arc<dyn EmbeddingProvider>) {
        if let Ok(mut guard) = self.inner.write() {
            *guard = Some(provider);
        }
    }

    /// The current provider, or `None` if none has been published yet.
    pub fn get(&self) -> Option<Arc<dyn EmbeddingProvider>> {
        self.inner.read().ok().and_then(|guard| guard.clone())
    }
}

impl std::fmt::Debug for EmbeddingProviderSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let loaded = self.get().is_some();
        f.debug_struct("EmbeddingProviderSlot").field("loaded", &loaded).finish()
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
