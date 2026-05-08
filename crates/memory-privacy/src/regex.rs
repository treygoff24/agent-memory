use once_cell::sync::Lazy;
use regex::Regex;

use crate::decision::{PrivacyLabel, PrivacySpan};

struct Rule {
    label: PrivacyLabel,
    confidence: f32,
    regex: Regex,
}

#[allow(clippy::expect_used)]
static SECRET_RULES: Lazy<Vec<Rule>> = Lazy::new(|| {
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
static LABEL_RULES: Lazy<Vec<Rule>> = Lazy::new(|| {
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

/// Deterministic regex privacy spans.
pub fn regex_spans(text: &str) -> Vec<PrivacySpan> {
    let mut spans = Vec::new();
    for rule in SECRET_RULES.iter().chain(LABEL_RULES.iter()) {
        spans.extend(
            rule.regex
                .find_iter(text)
                .map(|matched| PrivacySpan::new(rule.label, matched.start(), matched.end(), rule.confidence)),
        );
    }
    spans.extend(credit_card_spans(text));
    spans.sort_by_key(|span| (span.start, span.end));
    spans
}

/// Regex spans that imply secret/refuse handling.
pub fn secret_regex_spans(text: &str) -> Vec<PrivacySpan> {
    let mut spans = Vec::new();
    for rule in SECRET_RULES.iter() {
        spans.extend(
            rule.regex
                .find_iter(text)
                .map(|matched| PrivacySpan::new(rule.label, matched.start(), matched.end(), rule.confidence)),
        );
    }
    spans.extend(credit_card_spans(text));
    spans.sort_by_key(|span| (span.start, span.end));
    spans
}

/// Regex spans used only by the full classifier.
pub fn label_regex_spans(text: &str) -> Vec<PrivacySpan> {
    let mut spans = Vec::new();
    for rule in LABEL_RULES.iter() {
        spans.extend(
            rule.regex
                .find_iter(text)
                .map(|matched| PrivacySpan::new(rule.label, matched.start(), matched.end(), rule.confidence)),
        );
    }
    spans.sort_by_key(|span| (span.start, span.end));
    spans
}

#[allow(clippy::expect_used)]
static CARD_CANDIDATE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b(?:\d[ -]?){13,19}\b").expect("credit card candidate regex literal"));

fn credit_card_spans(text: &str) -> Vec<PrivacySpan> {
    CARD_CANDIDATE
        .find_iter(text)
        .filter_map(|matched| {
            let digits = matched.as_str().chars().filter(|ch| ch.is_ascii_digit()).collect::<String>();
            luhn_valid(&digits).then(|| PrivacySpan::new(PrivacyLabel::Secret, matched.start(), matched.end(), 0.99))
        })
        .collect()
}

fn luhn_valid(digits: &str) -> bool {
    if !(13..=19).contains(&digits.len()) {
        return false;
    }
    let mut sum = 0_u32;
    let mut double = false;
    for digit in digits.bytes().rev() {
        if !digit.is_ascii_digit() {
            return false;
        }
        let mut value = u32::from(digit - b'0');
        if double {
            value *= 2;
            if value > 9 {
                value -= 9;
            }
        }
        sum += value;
        double = !double;
    }
    sum % 10 == 0
}
