use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const INVALID_REQUEST: &str = "invalid_request";

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DreamError {
    #[error("invalid_request: {message}")]
    InvalidRequest { message: String },
}

impl DreamError {
    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::InvalidRequest { message: message.into() }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidRequest { .. } => INVALID_REQUEST,
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
