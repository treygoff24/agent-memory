use thiserror::Error;

pub type SourceResult<T> = Result<T, SourceError>;

#[derive(Debug, Error)]
pub enum SourceError {
    #[error("invalid source artifact id `{0}`")]
    InvalidId(String),
    #[error("invalid source ref `{0}`")]
    InvalidSourceRef(String),
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("source artifact integrity error: {0}")]
    Integrity(String),
    #[error("unsupported source content: {0}")]
    Unsupported(String),
    #[error("url safety error: {0}")]
    UrlSafety(String),
    #[error("capture failed: {0}")]
    CaptureFailed(String),
    #[error("privacy policy rejected source storage: {0}")]
    Privacy(String),
    #[error("excerpt not found: {0}")]
    ExcerptNotFound(String),
}

impl SourceError {
    pub fn integrity(message: impl Into<String>) -> Self {
        Self::Integrity(message.into())
    }

    pub fn url_safety(message: impl Into<String>) -> Self {
        Self::UrlSafety(message.into())
    }

    pub fn privacy(message: impl Into<String>) -> Self {
        Self::Privacy(message.into())
    }
}
