use thiserror::Error;

#[derive(Debug, Error)]
pub enum LoaderError {
    #[error("missing theme token: {0}")]
    MissingToken(String),
    #[error("failed to parse theme: {0}")]
    ParseFailed(String),
    #[error("unknown theme preset: {0}")]
    UnknownPreset(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
