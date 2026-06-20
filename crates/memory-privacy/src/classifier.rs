use memory_substrate::config::PrivacyEnforcement;

use crate::decision::{
    PrivacyDecision, PrivacyLabel, PrivacyNamespace, PrivacySpan, PrivacyStorageAction, PrivacyTier,
    SafeFragmentDecision,
};
use crate::error::PrivacyResult;
use crate::policy::{current_enforcement, CallerSensitivity, PrivacyPolicy};
use crate::privacy_filter::PrivacyFilterProvider;
use crate::regex::label_regex_spans;
use crate::secret_only_scan::SecretOnlyScan;
use tracing::warn;

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
        let (storage_action, downgraded) =
            apply_encryption_downgrade(namespace, resolved.tier, resolved.storage_action, &spans, enforcement);
        let mut decision = PrivacyDecision::new(resolved.tier, storage_action, spans, model);
        decision.downgraded_by_enforcement = downgraded;
        Ok(decision)
    }
}

/// Apply the `enforcement.encryption=false` downgrade.
///
/// When encryption is disabled and the resolved action requires encryption, the
/// effective storage action is downgraded to `Plaintext` and a single warning is
/// emitted. Returns the effective storage action and whether a downgrade occurred.
#[allow(clippy::too_many_arguments)]
fn apply_encryption_downgrade(
    namespace: PrivacyNamespace,
    tier: PrivacyTier,
    resolved_action: PrivacyStorageAction,
    spans: &[PrivacySpan],
    enforcement: PrivacyEnforcement,
) -> (PrivacyStorageAction, bool) {
    if enforcement.encryption || !resolved_action.requires_encryption() {
        return (resolved_action, false);
    }
    let encryption_labels: Vec<_> = spans
        .iter()
        .filter(|s| s.label.storage_action().requires_encryption())
        .map(|s| s.label)
        .collect();
    warn!(
        namespace = ?namespace,
        tier = ?tier,
        encryption_span_count = encryption_labels.len(),
        encryption_labels = ?encryption_labels,
        "enforcement.encryption=false: span(s) requiring encryption downgraded to Plaintext; \
         set enforcement.encryption=true to protect this content at rest",
    );
    (PrivacyStorageAction::Plaintext, true)
}

/// Classify a short fragment for safe plaintext emission.
///
/// This helper never reveals or decrypts persisted memory. It classifies the
/// provided fragment under `PrivacyNamespace::Me`, then maps Stream D storage
/// semantics into the narrower Stream E emission decision.
pub fn safe_plaintext_fragment(classifier: &dyn PrivacyClassifier, fragment: &str) -> SafeFragmentDecision {
    let Ok(decision) = classifier.classify(fragment, PrivacyNamespace::Me, None) else {
        return SafeFragmentDecision::OmitEncryptedBodyHidden;
    };

    if decision.storage_action.refuses_storage() || decision.spans.iter().any(|span| span.label == PrivacyLabel::Secret)
    {
        return SafeFragmentDecision::OmitEncryptedBodyHidden;
    }

    if decision.spans.iter().any(label_requires_review) {
        return SafeFragmentDecision::OmitReviewPending;
    }

    SafeFragmentDecision::Allow
}

fn label_requires_review(span: &PrivacySpan) -> bool {
    matches!(
        span.label,
        PrivacyLabel::AccountNumber
            | PrivacyLabel::PrivateAddress
            | PrivacyLabel::PrivateEmail
            | PrivacyLabel::PrivatePerson
            | PrivacyLabel::PrivatePhone
    ) || span.label.storage_action().requires_encryption()
}
