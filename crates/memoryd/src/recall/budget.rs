const TOKEN_ESTIMATOR_BYTES_PER_TOKEN: usize = 4;
const ELLIPSIS: &str = "…";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TruncatedText {
    pub value: String,
    pub truncated: bool,
}

pub fn estimated_tokens(value: &str) -> usize {
    value.len().div_ceil(TOKEN_ESTIMATOR_BYTES_PER_TOKEN)
}

pub fn truncate_utf8_bytes(value: &str, max_bytes: usize) -> TruncatedText {
    if value.len() <= max_bytes {
        return TruncatedText { value: value.to_owned(), truncated: false };
    }

    if max_bytes < ELLIPSIS.len() {
        return TruncatedText { value: String::new(), truncated: true };
    }

    let prefix_budget = max_bytes - ELLIPSIS.len();
    let prefix_len = largest_char_boundary_at_or_before(value, prefix_budget);
    let mut truncated = String::from(&value[..prefix_len]);
    truncated.push_str(ELLIPSIS);

    TruncatedText { value: truncated, truncated: true }
}

fn largest_char_boundary_at_or_before(value: &str, max_bytes: usize) -> usize {
    if max_bytes >= value.len() {
        return value.len();
    }

    let mut boundary = 0;
    for (index, character) in value.char_indices() {
        let character_end = index + character.len_utf8();
        if character_end > max_bytes {
            break;
        }
        boundary = character_end;
    }
    boundary
}
