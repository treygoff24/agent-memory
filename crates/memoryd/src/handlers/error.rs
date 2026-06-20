//! The `HandlerError` type and its typed constructors.
//!
//! Owns the daemon's request-handler error envelope: a `(code, message,
//! retryable)` triple that `handle_request_with_state` (in `handlers::mod`)
//! unwraps into the wire `ResponseEnvelope::error`. The `code` strings are part
//! of the protocol contract and must stay byte-identical. The fields and
//! constructors are `pub(crate)` so the sibling handler modules
//! (`memory_ops`, `source`, `review`, `status`, `reality_check`, `web_dashboard`,
//! `peer`, `dream`, `inspect`, and `governance::*`) can build and inspect them.

use memory_source::SourceError;
use memory_substrate::MemoryId;

use super::bounded;
use crate::recall::RecallError;

#[derive(Debug)]
pub(crate) struct HandlerError {
    pub(crate) code: String,
    pub(crate) message: String,
    pub(crate) retryable: bool,
}

impl HandlerError {
    pub(crate) fn invalid_request(message: impl Into<String>) -> Self {
        Self { code: "invalid_request".to_string(), message: message.into(), retryable: false }
    }

    /// Parse a caller-supplied id string into a canonical [`MemoryId`], mapping
    /// the validation error to an `invalid_request` handler error.
    pub(crate) fn parse_memory_id(id: impl Into<String>) -> Result<MemoryId, Self> {
        MemoryId::try_new(id.into()).map_err(|err| Self::invalid_request(err.to_string()))
    }

    pub(crate) fn dream_unavailable(message: impl Into<String>) -> Self {
        Self { code: "dream_unavailable".to_string(), message: message.into(), retryable: true }
    }

    pub(crate) fn dream_disabled(message: impl Into<String>) -> Self {
        Self { code: "dream_disabled".to_string(), message: message.into(), retryable: false }
    }

    pub(crate) fn web_unavailable(message: impl Into<String>) -> Self {
        Self { code: "web_unavailable".to_string(), message: message.into(), retryable: false }
    }

    pub(crate) fn port_in_use(message: impl Into<String>) -> Self {
        Self { code: "port_in_use".to_string(), message: message.into(), retryable: false }
    }

    pub(crate) fn substrate(error: impl std::fmt::Display) -> Self {
        Self { code: "substrate_error".to_string(), message: error.to_string(), retryable: true }
    }

    /// Typed refusal for a review approval blocked by grounding rehydration: the
    /// dream candidate's cited evidence drifted, aged out, or went missing since
    /// capture, so the approval is refused (and the memory quarantined) instead of
    /// promoting stale evidence to Active. Carries the stable
    /// `grounding_rehydration_failed` code so the review UI can show *why*.
    pub(crate) fn grounding_rehydration(error: &crate::dream::rehydration::GroundingRehydrationError) -> Self {
        Self { code: error.code().to_string(), message: bounded(&error.to_string(), 240), retryable: false }
    }

    pub(crate) fn privacy(error: impl std::fmt::Display) -> Self {
        Self { code: "privacy_error".to_string(), message: bounded(&error.to_string(), 240), retryable: false }
    }

    pub(crate) fn source_capture(error: SourceError) -> Self {
        let code = match &error {
            SourceError::InvalidId(_)
            | SourceError::InvalidSourceRef(_)
            | SourceError::UrlSafety(_)
            | SourceError::Privacy(_)
            | SourceError::ExcerptNotFound(_) => "invalid_request",
            SourceError::Unsupported(_) => "unsupported",
            SourceError::Io(_) | SourceError::Json(_) | SourceError::Integrity(_) | SourceError::CaptureFailed(_) => {
                "source_capture_failed"
            }
        };
        Self { code: code.to_string(), message: bounded(&error.to_string(), 240), retryable: false }
    }

    pub(crate) fn trust_artifact(error: crate::trust_artifact::TrustArtifactError) -> Self {
        match error {
            crate::trust_artifact::TrustArtifactError::MemoryNotFound(memory_id) => Self {
                code: "not_found".to_string(),
                message: format!("memory {} was not found", memory_id.as_str()),
                retryable: false,
            },
            crate::trust_artifact::TrustArtifactError::ReadMemory {
                id,
                source: memory_substrate::ReadError::NotFound(_),
            } => Self {
                code: "not_found".to_string(),
                message: format!("memory {} was not found", id.as_str()),
                retryable: false,
            },
            other => Self {
                code: "trust_artifact_error".to_string(),
                message: bounded(&other.to_string(), 240),
                retryable: true,
            },
        }
    }

    pub(crate) fn from_recall(error: RecallError) -> Self {
        Self {
            code: error.protocol_code().to_owned(),
            message: bounded(error.message(), 240),
            retryable: error.retryable(),
        }
    }

    pub(crate) fn from_dream(error: crate::dream::types::DreamError) -> Self {
        Self { code: error.code().to_string(), message: bounded(&error.to_string(), 240), retryable: false }
    }

    pub(crate) fn from_lease(error: crate::dream::lease::LeaseError) -> Self {
        let retryable = matches!(
            error,
            crate::dream::lease::LeaseError::Held { .. } | crate::dream::lease::LeaseError::Unavailable { .. }
        );
        Self { code: error.code().to_string(), message: bounded(&error.to_string(), 240), retryable }
    }
}
