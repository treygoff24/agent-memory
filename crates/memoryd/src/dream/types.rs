use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const INVALID_REQUEST: &str = "invalid_request";
pub const DREAM_UNAVAILABLE: &str = "dream_unavailable";

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DreamError {
    #[error("invalid_request: {message}")]
    InvalidRequest { message: String },
    /// A caller-supplied `--cli` override that names no registered harness. Kept
    /// distinct from `InvalidRequest` because the dispatch sites treat it
    /// specially (manual CLI exits 1; scheduled maps it to `LeaseError::InvalidRequest`).
    #[error("invalid_request: unknown harness CLI override `{name}`")]
    UnknownHarnessOverride { name: String },
    /// No harness CLI could be selected (disabled, missing, or unauthenticated).
    /// The `dream_unavailable` category: retryable, and the manual CLI exits 2.
    #[error("dream_unavailable: {message}")]
    Unavailable { message: String },
}

impl DreamError {
    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::InvalidRequest { message: message.into() }
    }

    pub fn unknown_harness_override(name: impl Into<String>) -> Self {
        Self::UnknownHarnessOverride { name: name.into() }
    }

    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::Unavailable { message: message.into() }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidRequest { .. } | Self::UnknownHarnessOverride { .. } => INVALID_REQUEST,
            Self::Unavailable { .. } => DREAM_UNAVAILABLE,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DreamPass {
    Pass1,
    Pass2,
    Pass3,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessSelection {
    pub name: String,
    pub prompt_transport: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaskingContext {
    pub session_id: String,
    pub seed_surrogate: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubstrateFragment {
    pub id: String,
    pub kind: String,
    pub ts: String,
    pub entities: Vec<String>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveMemory {
    pub id: String,
    pub namespace: String,
    pub kind: String,
    pub entities: Vec<String>,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceCatalogEntry {
    pub kind: String,
    #[serde(rename = "ref")]
    pub reference: String,
    pub entities: Vec<String>,
    pub excerpt: String,
}
