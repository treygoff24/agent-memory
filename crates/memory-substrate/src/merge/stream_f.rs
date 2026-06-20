//! Stream F noncanonical merge-driver rules.
//!
//! These paths (substrate JSONL fragments, dream question/journal/lease files,
//! and cleanup JSON) are not §14 canonical Markdown memories; they dispatch out
//! of the Markdown orchestrator before any frontmatter handling. Mirrors the
//! `source_artifact` module's shape.

use std::collections::BTreeSet;

use serde_json::Value;

use super::{clean_fastpath, ensure_trailing_newline, MergeInput, MergeResult};
use crate::error::{MergeError, MergeSide};

pub(super) fn merge_stream_f_file(input: &MergeInput<'_>) -> Option<Result<MergeResult, MergeError>> {
    let path = input.path;
    if is_substrate_jsonl(path) {
        return Some(merge_jsonl(input, substrate_jsonl_sort_key));
    }
    if is_dream_question_jsonl(path) || is_journal_lease(path) {
        return Some(merge_jsonl(input, scope_ts_id_sort_key));
    }
    if is_dream_journal_markdown(path) {
        return Some(Ok(merge_dream_journal_markdown(input)));
    }
    if is_cleanup_json(path) {
        return Some(merge_cleanup_json(input));
    }
    None
}

fn is_substrate_jsonl(path: &str) -> bool {
    path.ends_with(".jsonl") && (path.starts_with("substrate/") || path.starts_with("encrypted/substrate/"))
}

fn is_dream_question_jsonl(path: &str) -> bool {
    path.starts_with("dreams/questions/") && path.ends_with(".jsonl")
}

fn is_journal_lease(path: &str) -> bool {
    path == "leases/journal.lease"
}

fn is_dream_journal_markdown(path: &str) -> bool {
    path.starts_with("dreams/journal/") && path.ends_with(".md")
}

fn is_cleanup_json(path: &str) -> bool {
    path.starts_with("dreams/cleanup/") && path.ends_with(".json")
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct JsonlSortKey {
    primary: String,
    secondary: String,
    tertiary: String,
    canonical: String,
}

fn merge_jsonl(
    input: &MergeInput<'_>,
    sort_key: fn(&Value, &str, &str) -> JsonlSortKey,
) -> Result<MergeResult, MergeError> {
    let mut seen = BTreeSet::new();
    let mut records = Vec::new();
    for (side, raw) in [(MergeSide::Base, input.base), (MergeSide::Ours, input.ours), (MergeSide::Theirs, input.theirs)]
    {
        for value in parse_jsonl_side(side, raw)? {
            let canonical = serde_json::to_string(&value).map_err(|err| MergeError::Serialize {
                message: format!("stream-f JSONL row serialization failed: {err}"),
            })?;
            if seen.insert(canonical.clone()) {
                let key = sort_key(&value, input.path, &canonical);
                records.push((key, canonical));
            }
        }
    }
    records.sort_by(|left, right| left.0.cmp(&right.0));
    let mut merged = records.into_iter().map(|(_, canonical)| canonical).collect::<Vec<_>>().join("\n");
    if !merged.is_empty() {
        merged.push('\n');
    }
    Ok(MergeResult::Clean(merged))
}

fn parse_jsonl_side(side: MergeSide, raw: &str) -> Result<Vec<Value>, MergeError> {
    let mut rows = Vec::new();
    for (index, line) in raw.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(trimmed).map_err(|err| MergeError::ParseSide {
            side,
            message: format!("invalid Stream F JSONL row {}: {err}", index + 1),
        })?;
        if !value.is_object() {
            return Err(MergeError::ParseSide {
                side,
                message: format!("Stream F JSONL row {} must be a JSON object", index + 1),
            });
        }
        rows.push(value);
    }
    Ok(rows)
}

fn substrate_jsonl_sort_key(value: &Value, _path: &str, canonical: &str) -> JsonlSortKey {
    JsonlSortKey {
        primary: json_field_text(value, "id").unwrap_or_default(),
        secondary: String::new(),
        tertiary: String::new(),
        canonical: canonical.to_string(),
    }
}

fn scope_ts_id_sort_key(value: &Value, path: &str, canonical: &str) -> JsonlSortKey {
    JsonlSortKey {
        primary: json_field_text(value, "scope").unwrap_or_else(|| dream_scope_from_path(path).unwrap_or_default()),
        secondary: json_field_text(value, "ts")
            .or_else(|| json_field_text(value, "acquired_at"))
            .or_else(|| json_field_text(value, "expires_at"))
            .or_else(|| dream_date_from_path(path))
            .unwrap_or_default(),
        tertiary: json_field_text(value, "id")
            .or_else(|| json_field_text(value, "run_id"))
            .unwrap_or_else(|| canonical.to_string()),
        canonical: canonical.to_string(),
    }
}

fn json_field_text(value: &Value, field: &str) -> Option<String> {
    match value.get(field)? {
        Value::String(text) => Some(text.clone()),
        other => Some(other.to_string()),
    }
}

fn merge_dream_journal_markdown(input: &MergeInput<'_>) -> MergeResult {
    if let Some(fastpath) = clean_fastpath(input) {
        return fastpath;
    }

    let (scope_path, date) = dream_scope_date_from_path(input.path);
    let marker = format!(
        "<!-- stream-f-merge: quarantine contested dream journal\npath: {}\nscope_path: {}\ndate: {}\n-->\n",
        input.path, scope_path, date
    );
    let quarantined = format!(
        "{marker}\n# Contested Stream F dream journal\n\nTwo devices wrote the same dream journal scope/date. Choose the surviving Pass 1 narrative manually.\n\n<<<<<<< ours\n{}=======\n{}>>>>>>> theirs\n",
        ensure_trailing_newline(input.ours),
        ensure_trailing_newline(input.theirs)
    );
    MergeResult::Quarantine(quarantined)
}

fn merge_cleanup_json(input: &MergeInput<'_>) -> Result<MergeResult, MergeError> {
    if let Some(fastpath) = clean_fastpath(input) {
        return Ok(fastpath);
    }
    let ours = parse_json_object_side(MergeSide::Ours, input.ours)?;
    let theirs = parse_json_object_side(MergeSide::Theirs, input.theirs)?;
    let winner =
        if cleanup_sort_key(&theirs, input.path) >= cleanup_sort_key(&ours, input.path) { theirs } else { ours };
    let merged = serde_json::to_string(&winner).map_err(|err| MergeError::Serialize {
        message: format!("stream-f cleanup JSON serialization failed: {err}"),
    })?;
    Ok(MergeResult::Clean(merged))
}

fn parse_json_object_side(side: MergeSide, raw: &str) -> Result<Value, MergeError> {
    let value: Value = serde_json::from_str(raw.trim())
        .map_err(|err| MergeError::ParseSide { side, message: format!("invalid Stream F JSON object: {err}") })?;
    if value.is_object() {
        Ok(value)
    } else {
        Err(MergeError::ParseSide { side, message: "Stream F cleanup file must be a JSON object".to_string() })
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct CleanupSortKey {
    device_id: String,
    date: String,
    last_write: String,
    canonical: String,
}

fn cleanup_sort_key(value: &Value, path: &str) -> CleanupSortKey {
    let (path_device, path_date) = cleanup_device_date_from_path(path);
    CleanupSortKey {
        device_id: json_field_text(value, "device_id").unwrap_or(path_device),
        date: json_field_text(value, "date").unwrap_or(path_date),
        last_write: json_field_text(value, "completed_at")
            .or_else(|| json_field_text(value, "updated_at"))
            .or_else(|| json_field_text(value, "ts"))
            .unwrap_or_default(),
        canonical: value.to_string(),
    }
}

fn dream_scope_date_from_path(path: &str) -> (String, String) {
    let Some(rest) = path.strip_prefix("dreams/journal/").or_else(|| path.strip_prefix("dreams/questions/")) else {
        return (String::new(), String::new());
    };
    let mut parts = rest.rsplitn(2, '/');
    let file = parts.next().unwrap_or_default();
    let scope_path = parts.next().unwrap_or_default();
    (scope_path.to_string(), file_stem(file).to_string())
}

fn dream_scope_from_path(path: &str) -> Option<String> {
    let (scope_path, _) = dream_scope_date_from_path(path);
    if scope_path.is_empty() {
        None
    } else {
        Some(scope_path)
    }
}

fn dream_date_from_path(path: &str) -> Option<String> {
    let (_, date) = dream_scope_date_from_path(path);
    if date.is_empty() {
        None
    } else {
        Some(date)
    }
}

fn cleanup_device_date_from_path(path: &str) -> (String, String) {
    let Some(rest) = path.strip_prefix("dreams/cleanup/") else {
        return (String::new(), String::new());
    };
    let mut parts = rest.split('/');
    let device = parts.next().unwrap_or_default();
    let date_file = parts.next().unwrap_or_default();
    (device.to_string(), file_stem(date_file).to_string())
}

fn file_stem(file: &str) -> &str {
    file.rsplit_once('.').map_or(file, |(stem, _)| stem)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jsonl<const N: usize>(lines: [&str; N]) -> String {
        let mut text = lines.join("\n");
        text.push('\n');
        text
    }

    #[test]
    fn golden_stream_f_substrate_jsonl_dispatch_bytes() {
        let base = jsonl([r#"{"id":"frag_b","ts":"2026-04-30T03:00:00Z","text":"base"}"#]);
        let ours = jsonl([
            r#"{"id":"frag_b","ts":"2026-04-30T03:00:00Z","text":"base"}"#,
            r#"{"id":"frag_d","ts":"2026-04-30T03:00:03Z","text":"ours"}"#,
        ]);
        let theirs = jsonl([
            r#"{"id":"frag_b","ts":"2026-04-30T03:00:00Z","text":"base"}"#,
            r#"{"id":"frag_a","ts":"2026-04-30T03:00:01Z","text":"theirs"}"#,
        ]);
        let input =
            MergeInput { base: &base, ours: &ours, theirs: &theirs, path: "substrate/dev_local/2026-04-30.jsonl" };
        let result = merge_stream_f_file(&input).expect("substrate JSONL dispatches").expect("merge ok");
        // expect-justified: golden test asserts exact dispatch bytes
        assert_eq!(
            result,
            MergeResult::Clean(
                "{\"id\":\"frag_a\",\"text\":\"theirs\",\"ts\":\"2026-04-30T03:00:01Z\"}\n\
                 {\"id\":\"frag_b\",\"text\":\"base\",\"ts\":\"2026-04-30T03:00:00Z\"}\n\
                 {\"id\":\"frag_d\",\"text\":\"ours\",\"ts\":\"2026-04-30T03:00:03Z\"}\n"
                    .to_string()
            )
        );
    }

    #[test]
    fn golden_stream_f_dream_journal_markdown_dispatch_bytes() {
        let input = MergeInput {
            base: "base journal\n",
            ours: "journal from dev_a\n",
            theirs: "journal from dev_b\n",
            path: "dreams/journal/me/2026-04-30.md",
        };
        let result = merge_stream_f_file(&input).expect("dream journal dispatches").expect("merge ok");
        // expect-justified: golden test asserts exact dispatch bytes
        assert_eq!(
            result,
            MergeResult::Quarantine(
                "<!-- stream-f-merge: quarantine contested dream journal\n\
                 path: dreams/journal/me/2026-04-30.md\n\
                 scope_path: me\n\
                 date: 2026-04-30\n\
                 -->\n\n\
                 # Contested Stream F dream journal\n\n\
                 Two devices wrote the same dream journal scope/date. Choose the surviving Pass 1 narrative manually.\n\n\
                 <<<<<<< ours\njournal from dev_a\n=======\njournal from dev_b\n>>>>>>> theirs\n"
                    .to_string()
            )
        );
    }

    #[test]
    fn golden_stream_f_cleanup_json_dispatch_bytes() {
        let base = r#"{"device_id":"dev_local","date":"2026-04-30","completed_at":"2026-04-30T03:00:00Z","fragments_archived":1}"#;
        let ours = r#"{"device_id":"dev_local","date":"2026-04-30","completed_at":"2026-04-30T03:10:00Z","fragments_archived":2}"#;
        let theirs = r#"{"device_id":"dev_local","date":"2026-04-30","completed_at":"2026-04-30T03:20:00Z","fragments_archived":3}"#;
        let input = MergeInput { base, ours, theirs, path: "dreams/cleanup/dev_local/2026-04-30.json" };
        let result = merge_stream_f_file(&input).expect("cleanup JSON dispatches").expect("merge ok");
        // expect-justified: golden test asserts exact dispatch bytes
        assert_eq!(
            result,
            MergeResult::Clean(
                "{\"completed_at\":\"2026-04-30T03:20:00Z\",\"date\":\"2026-04-30\",\"device_id\":\"dev_local\",\"fragments_archived\":3}"
                    .to_string()
            )
        );
    }

    #[test]
    fn golden_stream_f_returns_none_for_source_artifact_path() {
        // Source-artifact paths are dispatched earlier in merge_markdown; the
        // Stream F gate must decline them so dispatch order is preserved.
        let input = MergeInput { base: "{}", ours: "{}", theirs: "{}", path: "sources/web/example/manifest.json" };
        assert!(merge_stream_f_file(&input).is_none());
    }
}
