//! Semantic frontmatter merge library.

mod body_diff3;
mod field_rules;
mod lifecycle;
mod quarantine;
mod source_artifact;
mod stream_f;
mod three_way;

pub use three_way::{merge_markdown, MergeInput, MergeResult};

/// Generic 3-way fast paths from spec §14.3, shared by the canonical Markdown
/// orchestrator, the source-artifact merger, and the Stream F merger. Returns
/// the surviving side's bytes verbatim with no newline normalization.
fn clean_fastpath(input: &MergeInput<'_>) -> Option<MergeResult> {
    if input.ours == input.theirs {
        Some(MergeResult::Clean(input.ours.to_string()))
    } else if input.base == input.ours {
        Some(MergeResult::Clean(input.theirs.to_string()))
    } else if input.base == input.theirs {
        Some(MergeResult::Clean(input.ours.to_string()))
    } else {
        None
    }
}

/// Re-export merge errors so embedders don't need to reach into
/// `crate::error` to handle exit codes.
pub use crate::error::{MergeError, MergeSide};

/// Merge-driver supported frontmatter schema version. Routed through the
/// canonical [`crate::SUBSTRATE_SCHEMA_VERSION`] so the driver and substrate
/// stay in lockstep — see CLAUDE.md invariant 5.
pub use crate::SUBSTRATE_SCHEMA_VERSION as MERGE_DRIVER_SUPPORTED_SCHEMA_VERSION;

/// Return `text` with a single guaranteed trailing newline. Shared by the body
/// and three-way merge paths, which normalize line endings before diffing.
fn ensure_trailing_newline(text: &str) -> String {
    if text.ends_with('\n') {
        text.to_string()
    } else {
        format!("{text}\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_trailing_newline_appends_when_missing_and_noop_when_present() {
        assert_eq!(ensure_trailing_newline("no-nl"), "no-nl\n");
        assert_eq!(ensure_trailing_newline("has-nl\n"), "has-nl\n");
        assert_eq!(ensure_trailing_newline(""), "\n");
    }
}
