//! Free-text search (FTS5) query sanitization for the index read paths.
//!
//! Transforms free-form user text into FTS5-safe phrase tokens (strict lane) and
//! a bounded identifier-aware OR expression (relaxed fallback lane).

/// Sanitize a free-form user query for FTS5.
///
/// FTS5 has its own query syntax — `NOT`, `AND`, `OR`, `"phrase"`, column
/// qualifiers `col:term`, and the bare `-` prefix that means NOT. Forwarding
/// raw user text into MATCH means a query like `end-to-end` is parsed as
/// `end NOT to NOT end`, where `to` is then misread as a column qualifier and
/// the whole thing returns `sqlite error: no such column: to`.
///
/// The substrate's contract with callers is that `query.text` is a search
/// string, not an FTS5 expression. So at this boundary we transform the input
/// into a sequence of FTS5 phrase tokens — one quoted phrase per
/// whitespace-separated chunk, double-quotes escaped by doubling. Multiple
/// phrases are AND-ed by FTS5's default expression semantics.
///
/// Tokens with no alphanumeric content are dropped because FTS5's tokenizer
/// would reduce them to zero terms inside the phrase, which is a syntax error
/// in some FTS5 builds. An input that produces no usable tokens yields an
/// empty string; the caller short-circuits to an empty result set.
pub(super) fn sanitize_fts_query(input: &str) -> String {
    input
        .split_whitespace()
        .filter(|token| token.chars().any(|character| character.is_alphanumeric()))
        .map(|token| format!("\"{}\"", token.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ")
}

const RELAXED_FTS_MAX_TERMS: usize = 8;

/// Rank penalty applied to relaxed OR-fallback hits before RRF fusion.
///
/// Strict AND hits keep contiguous ranks `1..=S`. Relaxed-only hits receive
/// `S + i + RELAXED_RANK_OFFSET` (i = 1-based position among appended relaxed
/// hits), demoting OR-matches to tie-breakers of last resort because their BM25
/// scores come from a different query expression and are not rank-comparable
/// with strict AND hits.
///
/// 15 was chosen by a deterministic sweep on the recall-quality corpus
/// (2026-06-12, offsets 0/15/30/60): 15 was the only value that beat the
/// undiscounted behavior on nDCG@5 (0.7776 vs 0.7754) while recall@5 gave back
/// only 0.003 — heavier discounts (30, 60) lost real answers the OR fallback
/// was legitimately surfacing, not just noise (trap rate was flat across the
/// whole sweep).
pub(super) const RELAXED_RANK_OFFSET: usize = 15;

/// Build a bounded OR query for the hybrid BM25 lane's fallback pass.
///
/// The primary BM25 pass remains strict (`term term term`, implicit AND). This
/// relaxed expression only fills unused lane slots, so exact all-term matches
/// keep better BM25 ranks while memories sharing distinctive query anchors can
/// still corroborate the vector lane.
///
/// Short tokens (1–3 alphanumeric characters) are kept only when they look
/// like identifiers (digits, all-caps acronyms, or mixed alnum); lone letters and
/// short lowercase filler are dropped. Longer tokens still pass through the
/// low-signal stopword filter.
pub(super) fn sanitize_relaxed_fts_query(input: &str) -> String {
    input
        .split_whitespace()
        .filter_map(relaxed_fts_token)
        .take(RELAXED_FTS_MAX_TERMS)
        .map(|token| format!("\"{}\"", token.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" OR ")
}

/// Retain a whitespace token for the relaxed OR fallback when it survives
/// identifier-aware short-token filtering and the low-signal stopword list.
///
/// Tokens with fewer than four alphanumeric characters are kept only when they
/// look like recall anchors: they contain a digit, are an all-uppercase acronym
/// (two or more letters), or mix letters and digits. Lone letters and short
/// lowercase pure-alpha filler are dropped.
pub(super) fn relaxed_fts_token(token: &str) -> Option<&str> {
    let trimmed = token.trim_matches(|character: char| !character.is_alphanumeric());
    if trimmed.is_empty() {
        return None;
    }

    let alnum_count = trimmed.chars().filter(|character| character.is_alphanumeric()).count();
    if alnum_count < 4 && !should_keep_short_identifier(trimmed) {
        return None;
    }

    let lower = trimmed.to_ascii_lowercase();
    if is_low_signal_query_term(&lower) {
        return None;
    }

    Some(trimmed)
}

fn should_keep_short_identifier(trimmed: &str) -> bool {
    let alnum = trimmed.chars().filter(|character| character.is_alphanumeric()).collect::<Vec<_>>();
    let count = alnum.len();
    if count == 0 {
        return false;
    }

    // Lone letters (any case) are noise; lone digits are anchors.
    if count == 1 {
        return alnum[0].is_ascii_digit();
    }

    let has_digit = alnum.iter().any(|character| character.is_ascii_digit());
    if has_digit {
        return true;
    }

    if count >= 2 && alnum.iter().all(|character| character.is_ascii_uppercase()) {
        return true;
    }

    false
}

fn is_low_signal_query_term(term: &str) -> bool {
    matches!(
        term,
        "about"
            | "after"
            | "again"
            | "also"
            | "before"
            | "being"
            | "could"
            | "does"
            | "doing"
            | "from"
            | "have"
            | "into"
            | "memory"
            | "memories"
            | "should"
            | "that"
            | "their"
            | "there"
            | "these"
            | "this"
            | "those"
            | "user"
            | "what"
            | "when"
            | "where"
            | "which"
            | "with"
            | "would"
            | "your"
    )
}

#[cfg(test)]
mod tests {
    use super::{relaxed_fts_token, sanitize_fts_query, sanitize_relaxed_fts_query};

    #[test]
    fn sanitize_plain_word_wraps_as_single_phrase() {
        assert_eq!(sanitize_fts_query("needle"), "\"needle\"");
    }

    #[test]
    fn sanitize_multiple_words_ands_via_separate_phrases() {
        assert_eq!(sanitize_fts_query("daemon socket protocol"), "\"daemon\" \"socket\" \"protocol\"");
    }

    #[test]
    fn sanitize_hyphenated_word_stays_intact_inside_phrase() {
        // Inside FTS5 phrase quoting the tokenizer splits on `-`, so this
        // matches a body indexed as `end to end` — exactly what we want for
        // hyphenated agent queries. The key property is no MATCH error.
        assert_eq!(sanitize_fts_query("end-to-end"), "\"end-to-end\"");
    }

    #[test]
    fn sanitize_escapes_internal_double_quotes() {
        assert_eq!(sanitize_fts_query("say\"hi"), "\"say\"\"hi\"");
    }

    #[test]
    fn sanitize_drops_punctuation_only_tokens() {
        assert_eq!(sanitize_fts_query("hello -- world"), "\"hello\" \"world\"");
    }

    #[test]
    fn sanitize_empty_input_yields_empty_string() {
        assert_eq!(sanitize_fts_query(""), "");
        assert_eq!(sanitize_fts_query("   "), "");
        assert_eq!(sanitize_fts_query("--- !@#"), "");
    }

    #[test]
    fn sanitize_strips_fts5_operator_intent() {
        // `NOT to` is operator syntax in FTS5; after sanitization it becomes
        // two phrase matches, both required, neither one a NOT.
        assert_eq!(sanitize_fts_query("foo NOT bar"), "\"foo\" \"NOT\" \"bar\"");
    }

    #[test]
    fn relaxed_sanitize_ors_distinctive_terms_for_fallback() {
        assert_eq!(
            sanitize_relaxed_fts_query("what language preference should the user use"),
            "\"language\" OR \"preference\""
        );
    }

    #[test]
    fn relaxed_sanitize_bounds_terms_and_keeps_fts_escaping() {
        assert_eq!(
            sanitize_relaxed_fts_query("alpha beta gamma delta epsilon zeta eta theta iota kappa say\"hi"),
            "\"alpha\" OR \"beta\" OR \"gamma\" OR \"delta\" OR \"epsilon\" OR \"zeta\" OR \"theta\" OR \"iota\""
        );
    }

    #[test]
    fn relaxed_token_keeps_short_identifier_anchors() {
        assert_eq!(relaxed_fts_token("v2"), Some("v2"));
        assert_eq!(relaxed_fts_token("PR"), Some("PR"));
        // `trim_matches` strips only leading/trailing non-alnum; interior hyphens stay.
        assert_eq!(relaxed_fts_token("B-7"), Some("B-7"));
        assert_eq!(relaxed_fts_token("7"), Some("7"));
        assert_eq!(relaxed_fts_token("Rust"), Some("Rust"));
    }

    #[test]
    fn relaxed_token_drops_short_low_signal_filler() {
        assert_eq!(relaxed_fts_token("at"), None);
        assert_eq!(relaxed_fts_token("a"), None);
        assert_eq!(relaxed_fts_token("I"), None);
    }

    #[test]
    fn relaxed_sanitize_keeps_identifier_tokens_in_or_fallback() {
        assert_eq!(sanitize_relaxed_fts_query("what is the PR for v2 B-7"), "\"PR\" OR \"v2\" OR \"B-7\"");
    }
}
