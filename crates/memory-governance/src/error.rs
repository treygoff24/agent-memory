//! Typed errors for governance decisions.

/// Result alias for governance operations.
pub type GovernanceResult<T> = Result<T, GovernanceError>;

/// Public governance error taxonomy.
#[derive(Debug, thiserror::Error)]
pub enum GovernanceError {
    /// Refusal reason code is not part of the stable public contract.
    #[error("unknown governance refusal reason: {reason_code}")]
    UnknownRefusalReason {
        /// Unsupported reason code supplied by a caller.
        reason_code: String,
    },
}
