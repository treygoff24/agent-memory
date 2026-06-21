//! Production embedding lane: Qwen3-Embedding-0.6B via the fastembed candle
//! backend.
//!
//! Loaded once at daemon start and shared across the drain worker and the query
//! path. Loading is the expensive step (weights mmap + tokenizer); inference is
//! synchronous candle compute, so callers on the async runtime invoke through
//! `spawn_blocking`.
//!
//! ## Device selection
//!
//! On macOS, Metal GPU offload (`Device::new_metal(0)`) is the wired but
//! lightly-documented fastembed path; we smoke-test it at load and fall back to
//! CPU when it is unavailable or errors, recording which device won in
//! [`FastembedProvider::device`] so the daemon can log it. Other targets load
//! CPU directly so Linux CI can run `--all-features` without Apple-only deps.
//! The `accelerate` cargo feature gives an Apple-BLAS CPU path; this code does
//! not require it but benefits from it transparently when compiled in.
//!
//! ## Model acquisition
//!
//! `Qwen3TextEmbedding::from_hf` resolves weights through hf-hub. In fastembed
//! 5.16.0 the Qwen3 candle loader does not expose the generic `cache_dir`
//! option, so the CLI `serve` entrypoint sets `HF_HOME=<runtime>/models` before
//! constructing the Tokio runtime. This module never mutates process
//! environment while the daemon is live. Weights are never bundled. The model is
//! Apache 2.0.

use std::path::Path;

use candle_core::{DType, Device};
use fastembed::Qwen3TextEmbedding;
use memory_substrate::EmbeddingTriple;

use super::prompts::query_prompt;
use super::{check_dimension, EmbeddingError, EmbeddingProvider};

/// Maximum tokenized sequence length passed to fastembed truncation.
///
/// Memorum document chunks are capped at 500 tokens. Query-side paths
/// (`embed_query`, including governance contradiction detection) prepend the
/// tuned Qwen3 instruction prefix from `prompts::query_prompt`, measured at 67
/// tokens with the real tokenizer. 640 = 500 + 67 + headroom so a max-sized
/// chunk plus the prefix is not silently truncated before similarity matching.
///
/// fastembed pads with `PaddingStrategy::BatchLongest`, so this cap does not
/// force every call to 640 tokens — only sequences that would exceed it are
/// clipped, and batch tensors pad to the longest post-truncation sequence in the batch.
const MAX_SEQUENCE_LENGTH: usize = 640;

/// The only active-triple provider lane this process can satisfy with the
/// fastembed candle worker.
pub const FASTEMBED_CANDLE_PROVIDER: &str = memory_substrate::tree::DEFAULT_ACTIVE_EMBEDDING_PROVIDER;

/// Whether an active embedding triple belongs to the fastembed candle lane.
pub fn is_fastembed_candle_triple(triple: &EmbeddingTriple) -> bool {
    triple.provider == FASTEMBED_CANDLE_PROVIDER
}

/// Which compute device the model actually loaded onto.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadedDevice {
    /// Apple Metal GPU.
    Metal,
    /// CPU (plain, or Apple-BLAS accelerated when the `accelerate` feature is
    /// compiled in).
    Cpu,
}

impl LoadedDevice {
    /// Human label for logs and init output.
    pub fn label(self) -> &'static str {
        match self {
            LoadedDevice::Metal => "Metal GPU",
            LoadedDevice::Cpu => "CPU",
        }
    }
}

/// A loaded Qwen3 embedding model behind the asymmetric provider trait.
pub struct FastembedProvider {
    model: Qwen3TextEmbedding,
    triple: EmbeddingTriple,
    device: LoadedDevice,
}

impl FastembedProvider {
    /// Load the model for a runtime root, directing the hf-hub cache at
    /// `<runtime_root>/models` and trying Metal before CPU.
    ///
    /// `triple` is the configured active triple; `triple.model_ref` is the HF
    /// repo id to load. The loaded model's output dimension is probed before the
    /// provider is returned (invariant 3), so a bad dimension config disables
    /// the worker once instead of being rediscovered per queued job.
    pub fn load_for_runtime(runtime_root: &Path, triple: EmbeddingTriple) -> Result<Self, EmbeddingError> {
        let cache = runtime_root.join("models");
        std::fs::create_dir_all(&cache)
            .map_err(|err| EmbeddingError::Load(format!("create model cache {}: {err}", cache.display())))?;
        if std::env::var_os("HF_HOME").is_none() {
            tracing::warn!(
                cache = %cache.display(),
                "HF_HOME was not configured before runtime startup; fastembed Qwen3 will use its default cache"
            );
        }
        let repo_id = triple.model_ref.clone();
        Self::load(&repo_id, triple)
    }

    /// Load `triple.model_ref` for `triple` from the ambient hf-hub cache
    /// (`~/.cache/huggingface` unless `HF_HOME` is already set), trying Metal
    /// then CPU.
    ///
    /// Used by the real-model smoke test and any caller that wants the shared
    /// cache rather than the per-runtime `<runtime>/models` directory.
    pub fn load_from_repo(repo_id: &str, triple: EmbeddingTriple) -> Result<Self, EmbeddingError> {
        Self::load(repo_id, triple)
    }

    /// Load `repo_id` for `triple`, trying Metal then CPU.
    fn load(repo_id: &str, triple: EmbeddingTriple) -> Result<Self, EmbeddingError> {
        #[cfg(target_os = "macos")]
        match Self::load_on(repo_id, &triple, LoadedDevice::Metal) {
            Ok(provider) => Ok(provider),
            Err(error @ EmbeddingError::DimensionMismatch { .. }) => Err(error),
            Err(metal_err) => {
                tracing::warn!(
                    error = %metal_err,
                    "Qwen3 embedding model failed to load on Metal; falling back to CPU"
                );
                Self::load_on(repo_id, &triple, LoadedDevice::Cpu)
            }
        }

        #[cfg(not(target_os = "macos"))]
        {
            Self::load_on(repo_id, &triple, LoadedDevice::Cpu)
        }
    }

    fn load_on(repo_id: &str, triple: &EmbeddingTriple, lane: LoadedDevice) -> Result<Self, EmbeddingError> {
        // Device and dtype are one decision: Metal runs fp16, CPU runs fp32.
        let (device, dtype) = match lane {
            LoadedDevice::Metal => metal_device(lane)?,
            LoadedDevice::Cpu => (Device::Cpu, DType::F32),
        };
        let model = Qwen3TextEmbedding::from_hf(repo_id, &device, dtype, MAX_SEQUENCE_LENGTH)
            .map_err(|err| EmbeddingError::Load(format!("{repo_id} on {}: {err}", lane.label())))?;
        probe_model_dimension(&model, triple)?;
        Ok(Self { model, triple: triple.clone(), device: lane })
    }

    /// The compute device the model loaded onto.
    pub fn device(&self) -> LoadedDevice {
        self.device
    }

    fn embed_one(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        let mut vectors = self.model.embed(&[text]).map_err(|err| EmbeddingError::Inference(err.to_string()))?;
        let vector = vectors.pop().ok_or_else(|| EmbeddingError::Inference("model returned no vector".into()))?;
        check_dimension(&self.triple, &vector)?;
        Ok(vector)
    }

    /// Embed a whole slice of document texts in one fastembed forward pass.
    ///
    /// fastembed amortizes the candle matmuls over the batch slice (and pads to
    /// the longest post-truncation sequence, see [`MAX_SEQUENCE_LENGTH`]), so a
    /// single `self.model.embed(texts)` is several times faster per item than
    /// looping `embed_one`. Each returned vector is dimension-checked exactly as
    /// `embed_one` does, so per-item results are byte-identical to the per-text
    /// path.
    fn embed_documents_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        let vectors = self.model.embed(texts).map_err(|err| EmbeddingError::Inference(err.to_string()))?;
        if vectors.len() != texts.len() {
            return Err(EmbeddingError::Inference(format!(
                "model returned {} vectors for {} inputs",
                vectors.len(),
                texts.len()
            )));
        }
        for vector in &vectors {
            check_dimension(&self.triple, vector)?;
        }
        Ok(vectors)
    }
}

#[cfg(target_os = "macos")]
fn metal_device(lane: LoadedDevice) -> Result<(Device, DType), EmbeddingError> {
    Ok((
        Device::new_metal(0).map_err(|err| EmbeddingError::Load(format!("{} device: {err}", lane.label())))?,
        DType::F16,
    ))
}

#[cfg(not(target_os = "macos"))]
fn metal_device(lane: LoadedDevice) -> Result<(Device, DType), EmbeddingError> {
    Err(EmbeddingError::Load(format!("{} device is only supported on macOS", lane.label())))
}

fn probe_model_dimension(model: &Qwen3TextEmbedding, triple: &EmbeddingTriple) -> Result<(), EmbeddingError> {
    let mut vectors = model
        .embed(&["Memorum embedding dimension probe."])
        .map_err(|err| EmbeddingError::Load(format!("dimension probe failed: {err}")))?;
    let vector = vectors.pop().ok_or_else(|| EmbeddingError::Load("dimension probe returned no vector".into()))?;
    check_dimension(triple, &vector)
}

impl EmbeddingProvider for FastembedProvider {
    fn triple(&self) -> &EmbeddingTriple {
        &self.triple
    }

    fn embed_query(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        self.embed_one(&query_prompt(text))
    }

    fn embed_document(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        self.embed_one(text)
    }

    fn embed_documents(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        self.embed_documents_batch(texts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fastembed_lane_accepts_only_fastembed_candle_provider() {
        let supported = EmbeddingTriple {
            provider: FASTEMBED_CANDLE_PROVIDER.to_string(),
            model_ref: "Qwen/Qwen3-Embedding-0.6B".to_string(),
            dimension: 1024,
        };
        let unsupported = EmbeddingTriple {
            provider: "synthetic".to_string(),
            model_ref: "stream-a-test".to_string(),
            dimension: 32,
        };

        assert!(is_fastembed_candle_triple(&supported));
        assert!(!is_fastembed_candle_triple(&unsupported));
    }
}
