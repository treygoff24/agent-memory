use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::{MergeInput, MergeResult};
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
        return Ok(MergeResult::Quarantine(with_trailing_newline(rows.join("\n"))));
    }
    Ok(MergeResult::Clean(with_trailing_newline(rows.join("\n"))))
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

fn clean_fastpath(input: &MergeInput<'_>) -> Option<MergeResult> {
    if input.ours == input.theirs {
        return Some(MergeResult::Clean(input.ours.to_string()));
    }
    if input.ours == input.base {
        return Some(MergeResult::Clean(input.theirs.to_string()));
    }
    if input.theirs == input.base {
        return Some(MergeResult::Clean(input.ours.to_string()));
    }
    None
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

fn with_trailing_newline(mut text: String) -> String {
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text
}
