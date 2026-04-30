use thiserror::Error;

/// Result type used by Stream D privacy code.
pub type PrivacyResult<T> = Result<T, PrivacyError>;

/// Stream D failure modes. Callers must fail closed for these errors.
#[derive(Debug, Error)]
pub enum PrivacyError {
    /// Privacy Filter is disabled or cannot be reached.
    #[error("privacy filter unavailable: {0}")]
    PrivacyFilterUnavailable(String),
    /// The encrypted tier cannot be used because key material is absent.
    #[error("privacy key unavailable: {0}")]
    KeyUnavailable(String),
    /// Encryption or decryption failed.
    #[error("privacy crypto error: {0}")]
    Crypto(String),
    /// Caller supplied an unsupported or unsafe policy value.
    #[error("privacy policy error: {0}")]
    Policy(String),
    /// A masked token cannot be restored in the active session.
    #[error("masking error: {0}")]
    Masking(String),
}
