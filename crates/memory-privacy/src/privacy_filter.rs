use crate::decision::PrivacySpan;
use crate::error::{PrivacyError, PrivacyResult};

/// Optional OpenAI Privacy Filter provider boundary.
pub trait PrivacyFilterProvider: Send + Sync {
    /// Return model/provider name for audit.
    fn model_name(&self) -> &str;

    /// Detect spans using the optional provider.
    fn detect(&self, text: &str) -> PrivacyResult<Vec<PrivacySpan>>;
}

/// Disabled provider used by default. Layer 1 still runs outside this provider.
#[derive(Clone, Debug, Default)]
pub struct DisabledPrivacyFilter;

impl PrivacyFilterProvider for DisabledPrivacyFilter {
    fn model_name(&self) -> &str {
        "openai/privacy-filter@disabled"
    }

    fn detect(&self, _text: &str) -> PrivacyResult<Vec<PrivacySpan>> {
        Err(PrivacyError::PrivacyFilterUnavailable("privacy filter is disabled".to_string()))
    }
}

/// Deterministic fixture provider for tests; no model download is required.
#[derive(Clone, Debug)]
pub struct FixturePrivacyFilter {
    spans: Vec<PrivacySpan>,
}

impl FixturePrivacyFilter {
    /// Create a fixture provider with fixed spans.
    pub fn new(spans: Vec<PrivacySpan>) -> Self {
        Self { spans }
    }
}

impl PrivacyFilterProvider for FixturePrivacyFilter {
    fn model_name(&self) -> &str {
        "openai/privacy-filter@fixture"
    }

    fn detect(&self, _text: &str) -> PrivacyResult<Vec<PrivacySpan>> {
        Ok(self.spans.clone())
    }
}
