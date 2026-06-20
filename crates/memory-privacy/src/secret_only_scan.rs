//! Always-on secret scanner used even when the full classifier is disabled.

use crate::decision::PrivacySpan;
use crate::entropy::high_entropy_spans;
use crate::regex::secret_regex_spans;

/// Always-on scanner for credentials, SSNs, Luhn-valid card numbers, and
/// credential-like high-entropy tokens.
#[derive(Clone, Copy, Debug, Default)]
pub struct SecretOnlyScan;

impl SecretOnlyScan {
    /// Return all secret spans for classifier audit metadata.
    pub(crate) fn spans(text: &str) -> Vec<PrivacySpan> {
        let mut spans = secret_regex_spans(text);
        spans.extend(high_entropy_spans(text));
        spans.sort_by_key(|span| (span.start, span.end));
        spans
    }
}
