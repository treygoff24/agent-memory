//! Normalization and validation for abstraction/cue frontmatter.

use unicode_casefold::UnicodeCaseFold;
use unicode_normalization::UnicodeNormalization;

use crate::error::ValidationError;
use crate::model::Frontmatter;

const ABSTRACTION_MAX_WORDS: usize = 8;
const ABSTRACTION_MAX_CHARS: usize = 120;
const CUE_MAX_WORDS: usize = 6;
const CUE_MAX_CHARS: usize = 64;
const CUE_MAX_COUNT: usize = 3;

/// Normalize all semantic frontmatter and enforce the write-time caps.
pub fn normalize_abstraction_cues(frontmatter: &mut Frontmatter) -> Result<(), ValidationError> {
    frontmatter.abstraction = normalize_abstraction_value(frontmatter.abstraction.take())?;
    frontmatter.cues = normalize_cue_values(std::mem::take(&mut frontmatter.cues))?;
    Ok(())
}

/// Normalize one optional abstraction under the ratified caps.
pub fn normalize_abstraction_value(value: Option<String>) -> Result<Option<String>, ValidationError> {
    value
        .map(|value| normalize_semantic_text(&value, "abstraction", ABSTRACTION_MAX_WORDS, ABSTRACTION_MAX_CHARS))
        .transpose()
        .map(|value| value.filter(|value| !value.is_empty()))
}

/// Normalize, order, and case-fold-deduplicate caller-supplied cues.
pub fn normalize_cue_values(cues: Vec<String>) -> Result<Vec<String>, ValidationError> {
    let mut cues = cues
        .into_iter()
        .map(|cue| normalize_semantic_text(&cue, "cues", CUE_MAX_WORDS, CUE_MAX_CHARS))
        .collect::<Result<Vec<_>, _>>()?;
    cues.retain(|cue| !cue.is_empty());
    cues.sort_by(|left, right| cue_key(left).cmp(&cue_key(right)));
    cues.dedup_by(|left, right| full_case_fold(left) == full_case_fold(right));
    if cues.len() > CUE_MAX_COUNT {
        return Err(ValidationError::BadShape("cues".to_string()));
    }
    Ok(cues)
}

/// NFC-normalize, trim, and collapse non-control whitespace in one semantic string.
pub fn normalize_semantic_text(
    value: &str,
    field: &'static str,
    max_words: usize,
    max_chars: usize,
) -> Result<String, ValidationError> {
    if value.chars().any(char::is_control) {
        return Err(ValidationError::BadShape(field.to_string()));
    }
    let normalized = value.nfc().collect::<String>();
    let collapsed = normalized.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() > max_chars || collapsed.split_whitespace().count() > max_words {
        return Err(ValidationError::BadShape(field.to_string()));
    }
    Ok(collapsed)
}

pub(crate) fn canonicalize_cue_union<'a>(values: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut cues = values
        .into_iter()
        .filter_map(|value| normalize_semantic_text(value, "cues", CUE_MAX_WORDS, CUE_MAX_CHARS).ok())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    cues.sort_by(|left, right| cue_key(left).cmp(&cue_key(right)));
    cues.dedup_by(|left, right| full_case_fold(left) == full_case_fold(right));
    cues.truncate(CUE_MAX_COUNT);
    cues
}

fn cue_key(value: &str) -> (String, &[u8]) {
    (full_case_fold(value), value.as_bytes())
}

fn full_case_fold(value: &str) -> String {
    value.case_fold().collect()
}

#[cfg(test)]
mod tests {
    use super::{canonicalize_cue_union, normalize_semantic_text};

    #[test]
    fn cue_order_is_side_independent_and_uses_full_unicode_case_folding() {
        let left = canonicalize_cue_union(["oauth", "OAuth", "Straße", "STRASSE", "İ", "I"]);
        let right = canonicalize_cue_union(["I", "İ", "STRASSE", "Straße", "OAuth", "oauth"]);
        assert_eq!(left, right);
        assert_eq!(left, vec!["I", "İ", "OAuth"]);
    }

    #[test]
    fn semantic_text_normalizes_nfc_and_collapses_whitespace_but_rejects_controls() {
        let normalized = normalize_semantic_text("  Cafe\u{301}   auth ", "cues", 6, 64)
            .unwrap_or_else(|error| panic!("normalization failed: {error}"));
        assert_eq!(normalized, "Café auth");
        assert!(normalize_semantic_text("two\nlines", "cues", 6, 64).is_err());
    }
}
