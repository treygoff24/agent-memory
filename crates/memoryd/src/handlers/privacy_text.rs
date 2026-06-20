//! Plaintext privacy gates shared across the handler modules.
//!
//! These helpers decide what caller-supplied or memory-derived text is safe to
//! persist or index in cleartext. They are the single policy that keeps secrets
//! and PII out of the plaintext audit trail and the recall index (invariant 1).
//! The cross-module surface (`sanitize_reason`, `contains_secret_or_pii_marker`,
//! `is_safe_plaintext_for_indexing`, `insert_safe_descriptor`,
//! `safe_index_projection`) is `pub(crate)` so the sibling handler modules and
//! `governance::*` reach it through the re-exports in `handlers::mod`.

use memory_privacy::{safe_plaintext_fragment, DeterministicPrivacyClassifier, SafeFragmentDecision};
use memory_substrate::{IndexProjection, Memory};
use serde_json::Value;

use super::REDACTED_REASON;

/// Redact a caller-supplied reason field (forget, reveal, …) to `[redacted]` when it is
/// empty or carries secret/PII content; otherwise return it trimmed and bounded to
/// `max_chars`. Reason fields are persisted verbatim into the canonical event log, so this
/// is the single policy that keeps a secret out of the plaintext audit trail (invariant 1).
///
/// The primary gate is the privacy classifier's entropy/structure detector
/// (`is_safe_plaintext_for_indexing` → `SecretOnlyScan`: credential regexes,
/// Luhn-valid cards, SSNs, and high-entropy tokens). That catches credential-shaped
/// content regardless of whether the operator happened to type a marker word like
/// "secret" or "token" alongside it. `contains_secret_or_pii_marker` is kept only as
/// an additive belt for short marker-laden phrases the structural detector underweights;
/// it is never the sole line of defense.
pub(crate) fn sanitize_reason(reason: &str, max_chars: usize) -> String {
    let trimmed = reason.trim();
    if trimmed.is_empty() {
        return REDACTED_REASON.to_owned();
    }
    // Primary: structural/entropy classifier. Belt: keyword denylist.
    if !is_safe_plaintext_for_indexing(trimmed) || contains_secret_or_pii_marker(trimmed) {
        return REDACTED_REASON.to_owned();
    }
    trimmed.chars().take(max_chars).collect()
}

pub(crate) fn contains_secret_or_pii_marker(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("sk-")
        || lower.contains("api key")
        || lower.contains("secret")
        || lower.contains("token")
        || contains_email_like_token(text)
        || contains_phone_like_token(text)
}

fn contains_email_like_token(text: &str) -> bool {
    text.split_whitespace().any(|token| {
        let token = token.trim_matches(|ch: char| ch.is_ascii_punctuation() && ch != '@' && ch != '.');
        token.contains('@') && token.contains('.')
    })
}

fn contains_phone_like_token(text: &str) -> bool {
    let digit_count = text.chars().filter(|ch| ch.is_ascii_digit()).count();
    digit_count >= 7 && text.chars().any(|ch| matches!(ch, '-' | '(' | ')' | '+' | '.'))
}

pub(crate) fn insert_safe_descriptor(object: &mut serde_json::Map<String, Value>, key: &str, value: Option<&str>) {
    if let Some(value) = value.filter(|value| is_safe_plaintext_for_indexing(value)) {
        object.insert(key.to_string(), Value::String(value.to_string()));
    }
}

pub(crate) fn is_safe_plaintext_for_indexing(text: &str) -> bool {
    matches!(safe_plaintext_fragment(&DeterministicPrivacyClassifier::new(), text), SafeFragmentDecision::Allow)
}

pub(crate) fn safe_index_projection(memory: &Memory) -> Option<IndexProjection> {
    let mut fragments = Vec::new();
    if !memory.frontmatter.summary.starts_with("encrypted ") {
        fragments.push(memory.frontmatter.summary.clone());
    }
    fragments.extend(memory.frontmatter.tags.iter().cloned());
    if let Some(reference) = &memory.frontmatter.source.reference {
        if reference != "memoryd.governance" && reference != "memoryd.write_note" {
            fragments.push(reference.clone());
        }
    }
    if let Some(descriptors) = memory.frontmatter.extras.get("privacy_descriptors") {
        collect_descriptor_strings(descriptors, &mut fragments);
    }
    let safe_body = fragments
        .into_iter()
        .map(|fragment| fragment.trim().to_string())
        .filter(|fragment| !fragment.is_empty() && is_safe_plaintext_for_indexing(fragment))
        .collect::<Vec<_>>()
        .join("\n");
    (!safe_body.is_empty()).then_some(IndexProjection { safe_body: Some(safe_body) })
}

fn collect_descriptor_strings(value: &Value, output: &mut Vec<String>) {
    match value {
        Value::String(value) => output.push(value.clone()),
        Value::Array(values) => values.iter().for_each(|value| collect_descriptor_strings(value, output)),
        Value::Object(values) => values.values().for_each(|value| collect_descriptor_strings(value, output)),
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}
