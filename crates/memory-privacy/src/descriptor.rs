use std::collections::BTreeSet;

use crate::decision::{safe_plaintext_fragment, SafeFragmentDecision};
use crate::{PrivacyClassifier, PrivacySpan};

const MAX_SUMMARY_BYTES: usize = 160;
const MAX_TAGS: usize = 8;

/// Safe plaintext projection for encrypted records that may be indexed or used
/// as synthesis input without revealing encrypted body spans.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SafeDescriptorProjection {
    /// Classifier-trusted, bounded summary with private spans removed.
    pub summary_safe: String,
    /// Classifier-trusted lookup tags derived from the safe summary.
    pub tag_safe: Vec<String>,
}

/// Build a safe descriptor projection for encrypted plaintext.
///
/// The projection removes classifier spans before deriving summary/tags, then
/// re-runs the safe-fragment guard over every emitted string. If the remaining
/// text is empty or still unsafe, the caller-provided fallback is emitted.
pub fn safe_descriptor_projection(
    classifier: &dyn PrivacyClassifier,
    text: &str,
    fallback_summary: &str,
    fallback_tags: &[String],
) -> SafeDescriptorProjection {
    let safe_text = classifier
        .classify(text, crate::PrivacyNamespace::Me, None)
        .ok()
        .map(|decision| remove_spans(text, &decision.spans))
        .unwrap_or_default();
    let candidate_summary = bounded_summary(&safe_text);
    let used_candidate_summary = !candidate_summary.is_empty()
        && safe_plaintext_fragment(classifier, &candidate_summary) == SafeFragmentDecision::Allow;
    let summary_safe = if used_candidate_summary {
        candidate_summary
    } else if safe_plaintext_fragment(classifier, fallback_summary) == SafeFragmentDecision::Allow {
        fallback_summary.to_string()
    } else {
        "encrypted record".to_string()
    };

    let mut tag_safe = if used_candidate_summary { tags_from_summary(classifier, &summary_safe) } else { Vec::new() };
    if tag_safe.is_empty() {
        tag_safe = fallback_tags
            .iter()
            .filter(|tag| safe_plaintext_fragment(classifier, tag) == SafeFragmentDecision::Allow)
            .cloned()
            .collect();
    }
    SafeDescriptorProjection { summary_safe, tag_safe }
}

fn remove_spans(text: &str, spans: &[PrivacySpan]) -> String {
    let mut output = String::with_capacity(text.len());
    let mut cursor = 0usize;
    for span in spans {
        if span.start < cursor
            || span.start > text.len()
            || span.end > text.len()
            || span.start >= span.end
            || !text.is_char_boundary(span.start)
            || !text.is_char_boundary(span.end)
        {
            continue;
        }
        output.push_str(&text[cursor..span.start]);
        output.push(' ');
        cursor = span.end;
    }
    output.push_str(&text[cursor..]);
    normalize_whitespace(&output)
}

fn bounded_summary(text: &str) -> String {
    let normalized = normalize_whitespace(text);
    if normalized.is_empty() {
        return String::new();
    }
    if normalized.len() <= MAX_SUMMARY_BYTES {
        return normalized;
    }
    let mut end = 0usize;
    for (index, _) in normalized.char_indices() {
        if index <= MAX_SUMMARY_BYTES.saturating_sub(3) {
            end = index;
        } else {
            break;
        }
    }
    format!("{}...", normalized[..end].trim_end())
}

fn normalize_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").trim().to_string()
}

fn tags_from_summary(classifier: &dyn PrivacyClassifier, summary: &str) -> Vec<String> {
    let mut tags = BTreeSet::new();
    for token in summary.split(|ch: char| !ch.is_ascii_alphanumeric()) {
        let token = token.to_ascii_lowercase();
        if token.len() < 4 || is_stopword(&token) {
            continue;
        }
        if safe_plaintext_fragment(classifier, &token) == SafeFragmentDecision::Allow {
            tags.insert(token);
        }
        if tags.len() >= MAX_TAGS {
            break;
        }
    }
    tags.into_iter().collect()
}

fn is_stopword(token: &str) -> bool {
    matches!(token, "about" | "after" | "before" | "from" | "into" | "must" | "that" | "this" | "with" | "without")
}
