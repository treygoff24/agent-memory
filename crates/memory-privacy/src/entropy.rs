use crate::decision::{PrivacyLabel, PrivacySpan};

const MIN_SECRET_LEN: usize = 32;
const MIN_ENTROPY_BITS_PER_CHAR: f64 = 4.2;

/// Detect high-entropy credential-like tokens.
pub fn high_entropy_spans(text: &str) -> Vec<PrivacySpan> {
    let mut spans = Vec::new();
    let mut offset = 0;
    for token in text.split_whitespace() {
        if token.len() >= MIN_SECRET_LEN
            && looks_token_like(token)
            && shannon_entropy(token) >= MIN_ENTROPY_BITS_PER_CHAR
        {
            if let Some(start) = text[offset..].find(token).map(|relative| offset + relative) {
                spans.push(PrivacySpan::new(PrivacyLabel::Secret, start, start + token.len(), 0.80));
                offset = start + token.len();
            }
        }
    }
    spans
}

fn looks_token_like(token: &str) -> bool {
    let alnum = token.chars().filter(|ch| ch.is_ascii_alphanumeric()).count();
    alnum >= token.len().saturating_sub(4) && token.chars().any(|ch| ch.is_ascii_digit())
}

fn shannon_entropy(value: &str) -> f64 {
    let len = value.chars().count() as f64;
    if len == 0.0 {
        return 0.0;
    }
    let mut counts = std::collections::BTreeMap::new();
    for ch in value.chars() {
        *counts.entry(ch).or_insert(0usize) += 1;
    }
    counts
        .values()
        .map(|count| {
            let p = *count as f64 / len;
            -p * p.log2()
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::shannon_entropy;

    #[test]
    fn entropy_increases_for_mixed_tokens() {
        assert!(
            shannon_entropy("abcdefghijklmnopqrstuvwxyz0123456789") > shannon_entropy("aaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );
    }
}
