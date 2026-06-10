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
//! Metal GPU offload (`Device::new_metal(0)`) is the wired but
//! lightly-documented fastembed path; we smoke-test it at load and fall back to
//! CPU when it is unavailable or errors, recording which device won in
//! [`FastembedProvider::device`] so the daemon can log it. The `accelerate`
//! cargo feature gives an Apple-BLAS CPU path; this code does not require it but
//! benefits from it transparently when compiled in.
//!
//! ## Model acquisition
//!
//! `Qwen3TextEmbedding::from_hf` resolves weights through the hf-hub cache,
//! which honors `HF_HOME`. The caller points `HF_HOME` at
//! `<runtime_root>/models` (see [`FastembedProvider::load_for_runtime`]) so
//! weights land inside the runtime tree on first use and are reused thereafter.
//! Weights are never bundled. The model is Apache 2.0.

use std::path::Path;

use candle_core::{DType, Device};
use fastembed::Qwen3TextEmbedding;
use memory_substrate::EmbeddingTriple;

use super::prompts::query_prompt;
use super::{check_dimension, EmbeddingError, EmbeddingProvider};

/// Maximum tokenized sequence length. Memorum chunks are 50–500 tokens; 512
/// covers them with headroom while keeping per-call compute bounded.
const MAX_SEQUENCE_LENGTH: usize = 512;

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
    /// repo id to load. The loaded model's output dimension is validated against
    /// `triple.dimension` on the first embed call (invariant 3).
    pub fn load_for_runtime(runtime_root: &Path, triple: EmbeddingTriple) -> Result<Self, EmbeddingError> {
        let cache = runtime_root.join("models");
        std::fs::create_dir_all(&cache)
            .map_err(|err| EmbeddingError::Load(format!("create model cache {}: {err}", cache.display())))?;
        // hf-hub's sync API reads HF_HOME for its cache root. Point it at the
        // runtime tree so weights live with the rest of the per-device state.
        // SAFETY: set before any HF download on this process; the daemon loads
        // the model from a single task at startup.
        std::env::set_var("HF_HOME", &cache);
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
        match Self::load_on(repo_id, &triple, LoadedDevice::Metal) {
            Ok(provider) => Ok(provider),
            Err(metal_err) => {
                tracing::warn!(
                    error = %metal_err,
                    "Qwen3 embedding model failed to load on Metal; falling back to CPU"
                );
                Self::load_on(repo_id, &triple, LoadedDevice::Cpu)
            }
        }
    }

    fn load_on(repo_id: &str, triple: &EmbeddingTriple, lane: LoadedDevice) -> Result<Self, EmbeddingError> {
        // Device and dtype are one decision: Metal runs fp16, CPU runs fp32.
        let (device, dtype) = match lane {
            LoadedDevice::Metal => (
                Device::new_metal(0).map_err(|err| EmbeddingError::Load(format!("{} device: {err}", lane.label())))?,
                DType::F16,
            ),
            LoadedDevice::Cpu => (Device::Cpu, DType::F32),
        };
        let model = Qwen3TextEmbedding::from_hf(repo_id, &device, dtype, MAX_SEQUENCE_LENGTH)
            .map_err(|err| EmbeddingError::Load(format!("{repo_id} on {}: {err}", lane.label())))?;
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
}
