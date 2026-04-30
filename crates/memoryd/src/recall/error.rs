use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecallError {
    InvalidRequest { message: String },
    SubstrateError { message: String },
    RecallUnavailable { message: String },
    PrivacyError { message: String },
    NotImplemented { message: String },
}

impl RecallError {
    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::InvalidRequest { message: message.into() }
    }

    pub fn substrate_error(message: impl Into<String>) -> Self {
        Self::SubstrateError { message: message.into() }
    }

    pub fn recall_unavailable(message: impl Into<String>) -> Self {
        Self::RecallUnavailable { message: message.into() }
    }

    pub fn privacy_error(message: impl Into<String>) -> Self {
        Self::PrivacyError { message: message.into() }
    }

    pub fn not_implemented(message: impl Into<String>) -> Self {
        Self::NotImplemented { message: message.into() }
    }

    pub fn protocol_code(&self) -> &'static str {
        match self {
            Self::InvalidRequest { .. } => "invalid_request",
            Self::SubstrateError { .. } => "substrate_error",
            Self::RecallUnavailable { .. } => "recall_unavailable",
            Self::PrivacyError { .. } => "privacy_error",
            Self::NotImplemented { .. } => "not_implemented",
        }
    }

    pub fn retryable(&self) -> bool {
        matches!(self, Self::SubstrateError { .. } | Self::RecallUnavailable { .. })
    }

    pub fn exit_code(&self) -> i32 {
        match self {
            Self::InvalidRequest { .. } => 1,
            Self::SubstrateError { .. } | Self::RecallUnavailable { .. } => 2,
            Self::PrivacyError { .. } => 3,
            Self::NotImplemented { .. } => 4,
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::InvalidRequest { message }
            | Self::SubstrateError { message }
            | Self::RecallUnavailable { message }
            | Self::PrivacyError { message }
            | Self::NotImplemented { message } => message,
        }
    }
}

impl fmt::Display for RecallError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.protocol_code(), self.message())
    }
}

impl std::error::Error for RecallError {}
