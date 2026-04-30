use crate::decision::{PrivacyDecision, PrivacyNamespace};
use crate::entropy::high_entropy_spans;
use crate::error::PrivacyResult;
use crate::policy::{CallerSensitivity, PrivacyPolicy};
use crate::privacy_filter::PrivacyFilterProvider;
use crate::regex::regex_spans;

/// Privacy classifier boundary.
pub trait PrivacyClassifier: Send + Sync {
    /// Classify a candidate text for storage routing.
    fn classify(
        &self,
        text: &str,
        namespace: PrivacyNamespace,
        caller: Option<CallerSensitivity>,
    ) -> PrivacyResult<PrivacyDecision>;
}

/// Deterministic Layer 1 classifier with optional provider spans.
#[derive(Default)]
pub struct DeterministicPrivacyClassifier {
    provider: Option<Box<dyn PrivacyFilterProvider>>,
}

impl DeterministicPrivacyClassifier {
    /// Construct the always-on deterministic classifier.
    pub fn new() -> Self {
        Self { provider: None }
    }

    /// Construct with an optional Privacy Filter provider.
    pub fn with_provider(provider: Box<dyn PrivacyFilterProvider>) -> Self {
        Self { provider: Some(provider) }
    }
}

impl PrivacyClassifier for DeterministicPrivacyClassifier {
    fn classify(
        &self,
        text: &str,
        namespace: PrivacyNamespace,
        caller: Option<CallerSensitivity>,
    ) -> PrivacyResult<PrivacyDecision> {
        let mut spans = regex_spans(text);
        spans.extend(high_entropy_spans(text));
        let mut model = "memory-privacy/layer1@v0.1".to_string();
        if let Some(provider) = &self.provider {
            let provider_spans = provider.detect(text)?;
            if !provider_spans.is_empty() {
                model = format!("{model}+{}", provider.model_name());
            }
            spans.extend(provider_spans);
        }
        spans.sort_by_key(|span| (span.start, span.end));
        let resolved = PrivacyPolicy::resolve(namespace, caller, &spans)?;
        Ok(PrivacyDecision::new(resolved.tier, resolved.storage_action, spans, model))
    }
}
