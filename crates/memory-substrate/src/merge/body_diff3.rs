//! Diff3-based body merge per Q8 (open-questions-resolved.md).

use imara_diff::{Algorithm, Diff, Hunk, InternedInput, Token};

/// Outcome of body diff3 merge.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum BodyMergeOutcome {
    /// Both edits applied cleanly; resulting body has no conflict markers.
    Clean(String),
    /// At least one hunk overlapped; resulting body carries `<<<<<<<` /
    /// `=======` / `>>>>>>>` markers and the document is quarantined.
    Conflict(String),
}

/// Three-way merge a memory body using diff3-style line semantics.
///
/// Phase 4 implementation:
///
/// - When `ours == base`, accept theirs wholesale.
/// - When `theirs == base`, accept ours wholesale.
/// - Otherwise, compute line-level hunks `(base→ours)` and `(base→theirs)`.
///   Hunks touching disjoint base regions merge cleanly; overlapping hunks
///   emit standard conflict markers.
///
/// Production-grade diff3 with whitespace-aware semantics is beyond the
/// substrate's scope; this implementation prioritises spec §13.6.1
/// convergence (deterministic, byte-stable output for identical inputs) and
/// surfaces overlapping conflicts honestly rather than silently dropping
/// data.
pub(super) fn merge_body_diff3(base: &str, ours: &str, theirs: &str) -> BodyMergeOutcome {
    if ours == theirs {
        return BodyMergeOutcome::Clean(ours.to_string());
    }
    if ours == base {
        return BodyMergeOutcome::Clean(theirs.to_string());
    }
    if theirs == base {
        return BodyMergeOutcome::Clean(ours.to_string());
    }

    let ours_hunks = compute_hunks(base, ours);
    let theirs_hunks = compute_hunks(base, theirs);

    if hunks_overlap(&ours_hunks, &theirs_hunks) {
        BodyMergeOutcome::Conflict(format_conflict_markers(ours, theirs))
    } else {
        BodyMergeOutcome::Clean(apply_disjoint_hunks(base, &ours_hunks, &theirs_hunks))
    }
}

/// Captured hunk: which range in `base` was replaced by which lines from
/// the side under diff.
#[derive(Clone, Debug, Eq, PartialEq)]
struct CapturedHunk {
    base_start: u32,
    base_end: u32,
    replacement: String,
}

fn compute_hunks(base: &str, side: &str) -> Vec<CapturedHunk> {
    let interned: InternedInput<&str> = InternedInput::new(base, side);
    let diff = Diff::compute(Algorithm::Histogram, &interned);
    let mut captured: Vec<CapturedHunk> = Vec::new();
    for hunk in diff.hunks() {
        captured.push(CapturedHunk {
            base_start: hunk.before.start,
            base_end: hunk.before.end,
            replacement: render_after_range(&interned, &hunk),
        });
    }
    captured
}

fn render_after_range(interned: &InternedInput<&str>, hunk: &Hunk) -> String {
    let mut out = String::new();
    let after_tokens = &interned.after[hunk.after.start as usize..hunk.after.end as usize];
    for token in after_tokens {
        out.push_str(token_text(interned, *token));
    }
    out
}

fn token_text<'a>(interned: &'a InternedInput<&'a str>, token: Token) -> &'a str {
    interned.interner[token]
}

fn hunks_overlap(left: &[CapturedHunk], right: &[CapturedHunk]) -> bool {
    for a in left {
        for b in right {
            if a.base_start < b.base_end && b.base_start < a.base_end {
                return true;
            }
        }
    }
    false
}

/// Apply two disjoint sets of hunks against `base` in base-position order.
/// Both sides' hunks merge so insertions/deletions from each apply
/// independently. Overlap is impossible at this point ([`hunks_overlap`]
/// returned false).
fn apply_disjoint_hunks(base: &str, ours: &[CapturedHunk], theirs: &[CapturedHunk]) -> String {
    let lines: Vec<&str> = base.split_inclusive('\n').collect();
    let mut events: Vec<&CapturedHunk> = ours.iter().chain(theirs.iter()).collect();
    events.sort_by_key(|hunk| hunk.base_start);
    let mut out = String::with_capacity(base.len());
    let mut cursor: u32 = 0;
    for hunk in events {
        while cursor < hunk.base_start && (cursor as usize) < lines.len() {
            out.push_str(lines[cursor as usize]);
            cursor += 1;
        }
        out.push_str(&hunk.replacement);
        cursor = hunk.base_end;
    }
    while (cursor as usize) < lines.len() {
        out.push_str(lines[cursor as usize]);
        cursor += 1;
    }
    out
}

fn format_conflict_markers(ours: &str, theirs: &str) -> String {
    format!(
        "<<<<<<< ours\n{}=======\n{}>>>>>>> theirs\n",
        ensure_trailing_newline(ours),
        ensure_trailing_newline(theirs),
    )
}

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
    fn ours_unchanged_takes_theirs() {
        let base = "alpha\nbeta\n";
        let ours = base;
        let theirs = "alpha\nGAMMA\n";
        let outcome = merge_body_diff3(base, ours, theirs);
        assert_eq!(outcome, BodyMergeOutcome::Clean(theirs.to_string()));
    }

    #[test]
    fn theirs_unchanged_takes_ours() {
        let base = "alpha\nbeta\n";
        let ours = "ALPHA\nbeta\n";
        let theirs = base;
        let outcome = merge_body_diff3(base, ours, theirs);
        assert_eq!(outcome, BodyMergeOutcome::Clean(ours.to_string()));
    }

    #[test]
    fn overlapping_edits_emit_conflict_markers() {
        let base = "alpha\nbeta\ngamma\n";
        let ours = "alpha\nBETA-ours\ngamma\n";
        let theirs = "alpha\nBETA-theirs\ngamma\n";
        let outcome = merge_body_diff3(base, ours, theirs);
        let BodyMergeOutcome::Conflict(text) = outcome else {
            panic!("expected conflict for overlapping edits");
        };
        assert!(text.contains("<<<<<<< ours"));
        assert!(text.contains(">>>>>>> theirs"));
    }

    #[test]
    fn disjoint_edits_merge_cleanly() {
        let base = "alpha\nbeta\ngamma\n";
        let ours = "ALPHA\nbeta\ngamma\n";
        let theirs = "alpha\nbeta\nGAMMA\n";
        let outcome = merge_body_diff3(base, ours, theirs);
        let BodyMergeOutcome::Clean(text) = outcome else {
            panic!("expected clean diff3 merge for disjoint edits");
        };
        assert_eq!(text, "ALPHA\nbeta\nGAMMA\n");
    }
}
