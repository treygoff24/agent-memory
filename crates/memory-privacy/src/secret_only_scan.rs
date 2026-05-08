//! Always-on secret scanner used even when the full classifier is disabled.

use crate::decision::{PrivacyLabel, PrivacySpan};
use crate::entropy::high_entropy_spans;
use crate::regex::secret_regex_spans;

/// A smallest-surface secret finding that must refuse storage.
#[derive(Clone, Debug, PartialEq)]
pub struct SecretFinding {
    /// Span that triggered the refusal.
    pub span: PrivacySpan,
}

/// Always-on scanner for credentials, SSNs, Luhn-valid card numbers, and
/// credential-like high-entropy tokens.
#[derive(Clone, Copy, Debug, Default)]
pub struct SecretOnlyScan;

impl SecretOnlyScan {
    /// Return the earliest secret finding, if any.
    pub fn scan(text: &str) -> Option<SecretFinding> {
        let mut spans = secret_regex_spans(text);
        spans.extend(high_entropy_spans(text));
        spans.sort_by_key(|span| (span.start, span.end));
        spans.into_iter().find(|span| span.label == PrivacyLabel::Secret).map(|span| SecretFinding { span })
    }

    /// Return all secret spans for classifier audit metadata.
    pub(crate) fn spans(text: &str) -> Vec<PrivacySpan> {
        let mut spans = secret_regex_spans(text);
        spans.extend(high_entropy_spans(text));
        spans.sort_by_key(|span| (span.start, span.end));
        spans
    }
}
