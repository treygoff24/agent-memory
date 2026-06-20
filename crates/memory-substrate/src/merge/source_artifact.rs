use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::{clean_fastpath, ensure_trailing_newline, MergeInput, MergeResult};
use crate::error::{MergeError, MergeSide};

pub fn merge_source_artifact(input: &MergeInput<'_>) -> Option<Result<MergeResult, MergeError>> {
    if is_source_manifest(input.path) {
        return Some(merge_manifest(input));
    }
    if is_source_excerpts(input.path) {
        return Some(merge_excerpts(input));
    }
    if is_source_extracted(input.path) {
        return Some(Ok(merge_extracted(input)));
    }
    None
}

fn is_source_manifest(path: &str) -> bool {
    path.starts_with("sources/web/") && path.ends_with("/manifest.json")
}

fn is_source_excerpts(path: &str) -> bool {
    path.starts_with("sources/web/") && path.ends_with("/excerpts.jsonl")
}

fn is_source_extracted(path: &str) -> bool {
    path.starts_with("sources/web/") && path.ends_with("/extracted.txt")
}

fn merge_manifest(input: &MergeInput<'_>) -> Result<MergeResult, MergeError> {
    if let Some(clean) = clean_fastpath(input) {
        return Ok(clean);
    }
    let ours = parse_json_object(MergeSide::Ours, input.ours)?;
    let theirs = parse_json_object(MergeSide::Theirs, input.theirs)?;
    let mut winner = if input.ours <= input.theirs { ours } else { theirs };
    let object = winner.as_object_mut().ok_or_else(|| MergeError::ParseSide {
        side: MergeSide::Ours,
        message: "source manifest must be a JSON object".to_string(),
    })?;
    object.insert("capture_status".to_string(), Value::String("partial".to_string()));
    let warnings = object.entry("warnings").or_insert_with(|| Value::Array(Vec::new()));
    if let Value::Array(values) = warnings {
        if !values.iter().any(|value| value == "source_artifact_merge_conflict") {
            values.push(Value::String("source_artifact_merge_conflict".to_string()));
        }
    }
    object.insert(
        "merge_conflict".to_string(),
        json!({
            "base_sha256": bounded_sha(input.base),
            "ours_sha256": bounded_sha(input.ours),
            "theirs_sha256": bounded_sha(input.theirs),
        }),
    );
    let merged = serde_json::to_string_pretty(&winner)
        .map_err(|err| MergeError::Serialize { message: format!("source manifest serialization failed: {err}") })?;
    Ok(MergeResult::Quarantine(format!("{merged}\n")))
}

fn merge_excerpts(input: &MergeInput<'_>) -> Result<MergeResult, MergeError> {
    if let Some(clean) = clean_fastpath(input) {
        return Ok(clean);
    }
    let mut by_id: BTreeMap<String, String> = BTreeMap::new();
    let mut conflicts = Vec::new();
    for (side, raw) in [(MergeSide::Base, input.base), (MergeSide::Ours, input.ours), (MergeSide::Theirs, input.theirs)]
    {
        for value in parse_jsonl(side, raw)? {
            let excerpt_id = value.get("excerpt_id").and_then(Value::as_str).unwrap_or("").to_string();
            if excerpt_id.is_empty() {
                return Err(MergeError::ParseSide {
                    side,
                    message: "source excerpt row missing excerpt_id".to_string(),
                });
            }
            let canonical = serde_json::to_string(&value).map_err(|err| MergeError::Serialize {
                message: format!("source excerpt row serialization failed: {err}"),
            })?;
            if let Some(existing) = by_id.get(&excerpt_id) {
                if existing != &canonical {
                    conflicts.push(excerpt_id);
                }
            } else {
                by_id.insert(excerpt_id, canonical);
            }
        }
    }
    let mut rows = by_id.into_values().collect::<Vec<_>>();
    if !conflicts.is_empty() {
        conflicts.sort();
        conflicts.dedup();
        rows.push(
            serde_json::to_string(&json!({
                "record_kind": "merge_conflict",
                "excerpt_ids": conflicts,
                "base_sha256": bounded_sha(input.base),
                "ours_sha256": bounded_sha(input.ours),
                "theirs_sha256": bounded_sha(input.theirs),
            }))
            .map_err(|err| MergeError::Serialize { message: err.to_string() })?,
        );
        return Ok(MergeResult::Quarantine(ensure_trailing_newline(&rows.join("\n"))));
    }
    Ok(MergeResult::Clean(ensure_trailing_newline(&rows.join("\n"))))
}

fn merge_extracted(input: &MergeInput<'_>) -> MergeResult {
    if let Some(clean) = clean_fastpath(input) {
        return clean;
    }
    MergeResult::Quarantine(format!(
        "source_artifact_merge_conflict\npath: {}\nbase_sha256: {}\nours_sha256: {}\ntheirs_sha256: {}\n",
        input.path,
        bounded_sha(input.base),
        bounded_sha(input.ours),
        bounded_sha(input.theirs)
    ))
}

fn parse_json_object(side: MergeSide, raw: &str) -> Result<Value, MergeError> {
    let value: Value = serde_json::from_str(raw.trim())
        .map_err(|err| MergeError::ParseSide { side, message: format!("invalid source manifest JSON: {err}") })?;
    if value.is_object() {
        Ok(value)
    } else {
        Err(MergeError::ParseSide { side, message: "source manifest must be a JSON object".to_string() })
    }
}

fn parse_jsonl(side: MergeSide, raw: &str) -> Result<Vec<Value>, MergeError> {
    let mut rows = Vec::new();
    let mut seen = BTreeSet::new();
    for (index, line) in raw.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(trimmed).map_err(|err| MergeError::ParseSide {
            side,
            message: format!("invalid source excerpts JSONL row {}: {err}", index + 1),
        })?;
        if !value.is_object() {
            return Err(MergeError::ParseSide {
                side,
                message: format!("source excerpts JSONL row {} must be an object", index + 1),
            });
        }
        let canonical =
            serde_json::to_string(&value).map_err(|err| MergeError::Serialize { message: err.to_string() })?;
        if seen.insert(canonical) {
            rows.push(value);
        }
    }
    Ok(rows)
}

fn bounded_sha(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Golden tests: byte-exact behavior locked before the merge-driver dedup refactor.

    #[test]
    fn clean_fastpath_ours_equals_theirs_returns_ours_bytes() {
        let input = MergeInput { base: "BASE", ours: "SAME", theirs: "SAME", path: "p" };
        assert_eq!(clean_fastpath(&input), Some(MergeResult::Clean("SAME".to_string())));
    }

    #[test]
    fn clean_fastpath_ours_equals_base_returns_theirs_bytes() {
        let input = MergeInput { base: "BASE", ours: "BASE", theirs: "THEIRS", path: "p" };
        assert_eq!(clean_fastpath(&input), Some(MergeResult::Clean("THEIRS".to_string())));
    }

    #[test]
    fn clean_fastpath_theirs_equals_base_returns_ours_bytes() {
        let input = MergeInput { base: "BASE", ours: "OURS", theirs: "BASE", path: "p" };
        assert_eq!(clean_fastpath(&input), Some(MergeResult::Clean("OURS".to_string())));
    }

    #[test]
    fn clean_fastpath_does_not_normalize_newlines() {
        // No trailing newline on any side: clean_fastpath must return the bytes
        // verbatim, never appending or stripping a newline.
        let input = MergeInput { base: "x", ours: "no-newline", theirs: "no-newline", path: "p" };
        assert_eq!(clean_fastpath(&input), Some(MergeResult::Clean("no-newline".to_string())));
        let input = MergeInput { base: "base\n", ours: "base\n", theirs: "theirs-no-nl", path: "p" };
        assert_eq!(clean_fastpath(&input), Some(MergeResult::Clean("theirs-no-nl".to_string())));
    }

    #[test]
    fn clean_fastpath_all_differ_returns_none() {
        let input = MergeInput { base: "BASE", ours: "OURS", theirs: "THEIRS", path: "p" };
        assert_eq!(clean_fastpath(&input), None);
    }

    #[test]
    fn shared_trailing_newline_appends_when_missing_and_noop_when_present() {
        // source_artifact now routes through the shared ensure_trailing_newline;
        // lock the byte behavior it relies on (was the local with_trailing_newline).
        assert_eq!(ensure_trailing_newline("no-nl"), "no-nl\n");
        assert_eq!(ensure_trailing_newline("has-nl\n"), "has-nl\n");
        assert_eq!(ensure_trailing_newline(""), "\n");
    }

    #[test]
    fn golden_source_extracted_quarantine_dispatch_bytes() {
        let input = MergeInput {
            base: "base text\n",
            ours: "ours text\n",
            theirs: "theirs text\n",
            path: "sources/web/example/extracted.txt",
        };
        let result = merge_source_artifact(&input).expect("extracted dispatches").expect("merge ok");
        // expect-justified: golden test asserts exact dispatch bytes
        assert_eq!(
            result,
            MergeResult::Quarantine(
                "source_artifact_merge_conflict\n\
                 path: sources/web/example/extracted.txt\n\
                 base_sha256: sha256:67859b52532ef63bed5cddf83081670406eeb184a75ea6aaaf077bf6ed78c7c4\n\
                 ours_sha256: sha256:c29fc27db082ca8c20af0b2b7af92c969c71ebae93e095e5f96546f93fed713d\n\
                 theirs_sha256: sha256:25bf381f2f4886054efdb58d5376d5cf31cbfd6fdca182a290fa374d9ae095ac\n"
                    .to_string()
            )
        );
    }

    #[test]
    fn golden_source_extracted_fastpath_is_clean_verbatim() {
        // base == theirs -> returns ours bytes verbatim through the shared fast path.
        let input = MergeInput {
            base: "same\n",
            ours: "ours-only\n",
            theirs: "same\n",
            path: "sources/web/example/extracted.txt",
        };
        let result = merge_source_artifact(&input).expect("extracted dispatches").expect("merge ok");
        // expect-justified: golden test asserts exact dispatch bytes
        assert_eq!(result, MergeResult::Clean("ours-only\n".to_string()));
    }

    #[test]
    fn golden_source_excerpts_clean_union_sorted_by_id_bytes() {
        let input = MergeInput {
            base: "",
            ours: "{\"excerpt_id\":\"e2\",\"text\":\"ours\"}\n",
            theirs: "{\"excerpt_id\":\"e1\",\"text\":\"theirs\"}\n",
            path: "sources/web/example/excerpts.jsonl",
        };
        let result = merge_source_artifact(&input).expect("excerpts dispatches").expect("merge ok");
        // expect-justified: golden test asserts exact dispatch bytes
        assert_eq!(
            result,
            MergeResult::Clean(
                "{\"excerpt_id\":\"e1\",\"text\":\"theirs\"}\n{\"excerpt_id\":\"e2\",\"text\":\"ours\"}\n".to_string()
            )
        );
    }

    #[test]
    fn golden_source_artifact_returns_none_for_non_source_path() {
        let input = MergeInput { base: "{}", ours: "{}", theirs: "{}", path: "substrate/dev/2026.jsonl" };
        assert!(merge_source_artifact(&input).is_none());
    }
}
