use memory_substrate::config::PrivacyEnforcement;

use crate::decision::{PrivacyDecision, PrivacyNamespace, PrivacyStorageAction, PrivacyTier};
use crate::error::PrivacyResult;
use crate::policy::{current_enforcement, CallerSensitivity, PrivacyPolicy};
use crate::privacy_filter::PrivacyFilterProvider;
use crate::regex::label_regex_spans;
use crate::secret_only_scan::SecretOnlyScan;

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
    enforcement: Option<PrivacyEnforcement>,
}

impl DeterministicPrivacyClassifier {
    /// Construct the always-on deterministic classifier.
    pub fn new() -> Self {
        Self { provider: None, enforcement: None }
    }

    /// Construct with an optional Privacy Filter provider.
    pub fn with_provider(provider: Box<dyn PrivacyFilterProvider>) -> Self {
        Self { provider: Some(provider), enforcement: None }
    }

    /// Construct with explicit enforcement, bypassing process-global runtime state.
    pub fn with_enforcement(enforcement: PrivacyEnforcement) -> Self {
        Self { provider: None, enforcement: Some(enforcement) }
    }

    fn enforcement(&self) -> PrivacyEnforcement {
        self.enforcement.unwrap_or_else(current_enforcement)
    }
}

impl PrivacyClassifier for DeterministicPrivacyClassifier {
    fn classify(
        &self,
        text: &str,
        namespace: PrivacyNamespace,
        caller: Option<CallerSensitivity>,
    ) -> PrivacyResult<PrivacyDecision> {
        let enforcement = self.enforcement();
        let spans = SecretOnlyScan::spans(text);
        if !spans.is_empty() {
            let resolved = PrivacyPolicy::resolve(namespace, caller, &spans)?;
            return Ok(PrivacyDecision::new(
                resolved.tier,
                PrivacyStorageAction::Refuse,
                spans,
                "memory-privacy/secret-only@v0.1",
            ));
        }
        if !enforcement.classifier {
            return Ok(PrivacyDecision::new(
                PrivacyTier::Internal,
                PrivacyStorageAction::Plaintext,
                Vec::new(),
                "memory-privacy/classifier-disabled@v0.1",
            ));
        }

        let mut spans = label_regex_spans(text);
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
        let storage_action = if enforcement.encryption {
            resolved.storage_action
        } else if resolved.storage_action.requires_encryption() {
            PrivacyStorageAction::Plaintext
        } else {
            resolved.storage_action
        };
        Ok(PrivacyDecision::new(resolved.tier, storage_action, spans, model))
    }
}
