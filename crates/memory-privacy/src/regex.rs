use regex::Regex;
use std::sync::LazyLock;

use crate::decision::{PrivacyLabel, PrivacySpan};

struct Rule {
    label: PrivacyLabel,
    confidence: f32,
    regex: Regex,
}

#[allow(clippy::expect_used)]
static SECRET_RULES: LazyLock<Vec<Rule>> = LazyLock::new(|| {
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
            label: PrivacyLabel::Secret,
            confidence: 0.99,
            regex: Regex::new(r"\b\d{3}-\d{2}-\d{4}\b").expect("ssn regex literal"),
        },
    ]
});

#[allow(clippy::expect_used)]
static LABEL_RULES: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    vec![
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

/// Regex spans that imply secret/refuse handling.
pub fn secret_regex_spans(text: &str) -> Vec<PrivacySpan> {
    let mut spans = rule_spans(SECRET_RULES.iter(), text);
    spans.extend(credit_card_spans(text));
    spans.sort_by_key(|span| (span.start, span.end));
    spans
}

/// Regex spans used only by the full classifier.
pub fn label_regex_spans(text: &str) -> Vec<PrivacySpan> {
    let mut spans = rule_spans(LABEL_RULES.iter(), text);
    spans.sort_by_key(|span| (span.start, span.end));
    spans
}

fn rule_spans<'a>(rules: impl Iterator<Item = &'a Rule>, text: &str) -> Vec<PrivacySpan> {
    rules
        .flat_map(|rule| {
            rule.regex
                .find_iter(text)
                .map(|matched| PrivacySpan::new(rule.label, matched.start(), matched.end(), rule.confidence))
        })
        .collect()
}

#[allow(clippy::expect_used)]
static CARD_CANDIDATE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(?:\d[ -]?){13,19}\b").expect("credit card candidate regex literal"));

fn credit_card_spans(text: &str) -> Vec<PrivacySpan> {
    CARD_CANDIDATE
        .find_iter(text)
        .filter(|matched| luhn_valid_candidate(matched.as_str()))
        .map(|matched| PrivacySpan::new(PrivacyLabel::Secret, matched.start(), matched.end(), 0.99))
        .collect()
}

fn luhn_valid_candidate(candidate: &str) -> bool {
    let mut sum = 0_u32;
    let mut double = false;
    let mut digits = 0_usize;
    for byte in candidate.bytes().rev() {
        if !byte.is_ascii_digit() {
            if matches!(byte, b' ' | b'-') {
                continue;
            }
            return false;
        }
        digits += 1;
        let mut value = u32::from(byte - b'0');
        if double {
            value *= 2;
            if value > 9 {
                value -= 9;
            }
        }
        sum += value;
        double = !double;
    }
    (13..=19).contains(&digits) && sum % 10 == 0
}
