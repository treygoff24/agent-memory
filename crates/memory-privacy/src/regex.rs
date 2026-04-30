use once_cell::sync::Lazy;
use regex::Regex;

use crate::decision::{PrivacyLabel, PrivacySpan};

struct Rule {
    label: PrivacyLabel,
    confidence: f32,
    regex: Regex,
}

#[allow(clippy::expect_used)]
static RULES: Lazy<Vec<Rule>> = Lazy::new(|| {
    vec![
        Rule {
            label: PrivacyLabel::Secret,
            confidence: 0.99,
            regex: Regex::new(r"AKIA[0-9A-Z]{16}").expect("aws regex literal"),
        },
        Rule {
            label: PrivacyLabel::Secret,
            confidence: 0.99,
            regex: Regex::new(r"gh[pousr]_[A-Za-z0-9_]{20,}").expect("github token regex literal"),
        },
        Rule {
            label: PrivacyLabel::Secret,
            confidence: 0.99,
            regex: Regex::new(r"sk_(live|test)_[A-Za-z0-9]{16,}").expect("stripe token regex literal"),
        },
        Rule {
            label: PrivacyLabel::Secret,
            confidence: 0.99,
            regex: Regex::new(r"-----BEGIN [A-Z ]*PRIVATE KEY-----").expect("private key regex literal"),
        },
        Rule {
            label: PrivacyLabel::Secret,
            confidence: 0.92,
            regex: Regex::new(r"\beyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\b")
                .expect("jwt regex literal"),
        },
        Rule {
            label: PrivacyLabel::PrivateEmail,
            confidence: 0.95,
            regex: Regex::new(r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b").expect("email regex literal"),
        },
        Rule {
            label: PrivacyLabel::PrivatePhone,
            confidence: 0.85,
            regex: Regex::new(r"\b(?:\+?1[-.\s]?)?\(?[0-9]{3}\)?[-.\s]?[0-9]{3}[-.\s]?[0-9]{4}\b")
                .expect("phone regex literal"),
        },
        Rule {
            label: PrivacyLabel::PrivateUrl,
            confidence: 0.70,
            regex: Regex::new(r"https?://[^\s]+").expect("url regex literal"),
        },
        Rule {
            label: PrivacyLabel::PrivateAddress,
            confidence: 0.70,
            regex: Regex::new(r"\b\d{1,6}\s+[A-Z][A-Za-z0-9.\-]*(?:\s+[A-Z][A-Za-z0-9.\-]*)*\s+(?:St|Street|Ave|Avenue|Rd|Road|Blvd|Lane|Ln|Drive|Dr)\b")
                .expect("address regex literal"),
        },
        Rule {
            label: PrivacyLabel::PrivateDate,
            confidence: 0.55,
            regex: Regex::new(r"\b\d{4}-\d{2}-\d{2}\b").expect("date regex literal"),
        },
    ]
});

/// Deterministic regex privacy spans.
pub fn regex_spans(text: &str) -> Vec<PrivacySpan> {
    let mut spans = Vec::new();
    for rule in RULES.iter() {
        spans.extend(
            rule.regex
                .find_iter(text)
                .map(|matched| PrivacySpan::new(rule.label, matched.start(), matched.end(), rule.confidence)),
        );
    }
    spans.sort_by_key(|span| (span.start, span.end));
    spans
}
