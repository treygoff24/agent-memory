//! Quarantine helpers and spec §6.10 `_merge_diagnostics` shape.

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256 as Sha256Hasher};

/// Top-level merge-diagnostics object per spec §6.10.
///
/// Empty arrays are skipped on serialize so the on-disk file does not carry
/// noise for clean merges. `human_reason` is always present (spec mandates a
/// short operator-readable string).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(super) struct MergeDiagnostic {
    /// Stable merge id (`merge_<ulid>`).
    pub merge_id: String,
    /// RFC3339 UTC timestamp.
    pub created_at: DateTime<Utc>,
    /// `clean_with_warnings | quarantined`.
    pub status: MergeStatus,
    /// Field names that conflicted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conflicting_fields: Vec<String>,
    /// Per-field winner/loser notes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub preserved_sources: Vec<Value>,
    /// Evidence near-duplicates surfaced from id-keyed evidence union.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_near_duplicates: Vec<EvidenceNearDuplicate>,
    /// Privacy-scan models retained from each side per spec §6.9.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub privacy_scans_preserved: Vec<Value>,
    /// Add/add alternates with mechanically recoverable raw bytes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub add_add_alternates: Vec<AddAddAlternate>,
    /// Sides that failed to parse but had identifiable frontmatter delimiters.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unparsed_sides: Vec<UnparsedSide>,
    /// Lifecycle pair-table notes (e.g. "tombstone clears superseded_by").
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lifecycle_notes: Vec<String>,
    /// Operator-readable summary of why this merge produced diagnostics.
    pub human_reason: String,
}

/// Spec §6.10 `status` enumeration. Only `clean_with_warnings` and
/// `quarantined` are valid — Phase 4 deletes the legacy
/// `clean_with_diagnostics` value (B-MG-3).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum MergeStatus {
    /// Clean merge with non-fatal conflict notes.
    CleanWithWarnings,
    /// Document was quarantined; admin review required.
    Quarantined,
}

impl Default for MergeStatus {
    fn default() -> Self {
        Self::CleanWithWarnings
    }
}

/// Add/add alternate per spec §6.10. Round-trip lossless via raw byte capture.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct AddAddAlternate {
    /// Memory id of the alternate.
    pub id: String,
    /// Original repo path the alternate occupied.
    pub original_path: String,
    /// Base64-encoded raw frontmatter YAML (between the `---` delimiters).
    pub frontmatter_yaml_b64: String,
    /// `sha256:<hex64>` of the raw body bytes.
    pub body_sha256: String,
    /// Base64-encoded body bytes when small enough to inline (≤ 1 MiB).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_b64: Option<String>,
    /// Reference to a body artifact when the body exceeds the inline cutoff.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_artifact_ref: Option<String>,
}

/// Spec §6.10 `unparsed_sides[]` shape; frontmatter and body separated.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct UnparsedSide {
    /// Side label.
    pub side: String,
    /// Repo path.
    pub path: String,
    /// Base64-encoded raw frontmatter region (empty when delimiters missing).
    pub frontmatter_raw_b64: String,
    /// Base64-encoded body bytes.
    pub body_b64: String,
    /// Rendered parser error.
    pub parse_error: String,
}

/// Spec §6.10 `evidence_near_duplicates[]` row.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct EvidenceNearDuplicate {
    /// Evidence id of the surviving entry.
    pub evidence_id: String,
    /// Quote that won.
    pub primary_quote: String,
    /// Near-duplicate quote that was elided.
    pub near_duplicate_quote: String,
}

/// 1 MiB cutoff for inlining body bytes per spec §6.10.
pub(super) const BODY_INLINE_LIMIT: usize = 1 << 20;

/// Construct a fresh diagnostic with `merge_id`/`created_at` populated.
pub(super) fn fresh_diagnostic(status: MergeStatus, human_reason: impl Into<String>) -> MergeDiagnostic {
    MergeDiagnostic {
        merge_id: format!("merge_{}", ulid::Ulid::new()),
        created_at: Utc::now(),
        status,
        conflicting_fields: Vec::new(),
        preserved_sources: Vec::new(),
        evidence_near_duplicates: Vec::new(),
        privacy_scans_preserved: Vec::new(),
        add_add_alternates: Vec::new(),
        unparsed_sides: Vec::new(),
        lifecycle_notes: Vec::new(),
        human_reason: human_reason.into(),
    }
}

/// Splice a typed [`MergeDiagnostic`] into a frontmatter's
/// `_merge_diagnostics` slot.
///
/// Spec §14.4 last semantic row: `_merge_diagnostics` is unioned across
/// ours/theirs/base by `merge_id` and sorted by `created_at` ASC. Phase 4
/// implements the union via the [`union_diagnostics`] helper called from
/// the orchestrator before the final splice.
pub(super) fn splice_diagnostic(slot: &mut Option<Value>, diagnostic: &MergeDiagnostic) -> Result<(), String> {
    let value = serde_json::to_value(diagnostic).map_err(|err| err.to_string())?;
    *slot = Some(Value::Array(vec![value]));
    Ok(())
}

/// Union prior `_merge_diagnostics` arrays from base/ours/theirs with the
/// fresh diagnostic emitted by this merge.
///
/// Each side's diagnostic is treated as either a single object (legacy) or
/// an array of objects (new). Dedupe on `merge_id`; sort by `created_at` ASC.
pub(super) fn union_diagnostics(
    base: Option<&Value>,
    ours: Option<&Value>,
    theirs: Option<&Value>,
    fresh: Option<MergeDiagnostic>,
) -> Option<Value> {
    let mut by_id: std::collections::BTreeMap<String, MergeDiagnostic> = std::collections::BTreeMap::new();
    for source in [base, ours, theirs].into_iter().flatten() {
        for entry in normalize_diagnostic_array(source) {
            by_id.insert(entry.merge_id.clone(), entry);
        }
    }
    if let Some(fresh) = fresh {
        by_id.insert(fresh.merge_id.clone(), fresh);
    }
    if by_id.is_empty() {
        return None;
    }
    let mut entries: Vec<MergeDiagnostic> = by_id.into_values().collect();
    entries.sort_by_key(|entry| entry.created_at);
    Some(serde_json::to_value(entries).expect("merge diagnostics serialize")) // expect-justified: typed fields only
}

fn normalize_diagnostic_array(value: &Value) -> Vec<MergeDiagnostic> {
    match value {
        Value::Array(items) => items.iter().filter_map(|item| serde_json::from_value(item.clone()).ok()).collect(),
        Value::Object(_) => {
            serde_json::from_value::<MergeDiagnostic>(value.clone()).map(|d| vec![d]).unwrap_or_default()
        }
        _ => Vec::new(),
    }
}

/// Split a raw merge-input blob into `(frontmatter_bytes, body_bytes)` on
/// the first `---\n.../---\n` boundary. When delimiters are absent (or the
/// closing one is), `frontmatter_bytes` is empty and the entire input lands
/// in `body_bytes` so callers can still emit `body_b64` for forensics.
pub(super) fn split_raw_document(raw: &str) -> (String, String) {
    let Some(after_open) = raw.strip_prefix("---\n") else {
        return (String::new(), raw.to_string());
    };
    let Some(end) = after_open.find("\n---\n") else {
        return (String::new(), raw.to_string());
    };
    let frontmatter = &after_open[..end];
    let body = &after_open[end + "\n---\n".len()..];
    (frontmatter.to_string(), body.to_string())
}

/// Build an [`UnparsedSide`] entry for the side that failed to parse. The
/// raw frontmatter and body are captured separately per spec §6.10.
pub(super) fn build_unparsed_side(side: &str, path: &str, raw: &str, parse_error: String) -> UnparsedSide {
    let (frontmatter, body) = split_raw_document(raw);
    UnparsedSide {
        side: side.to_string(),
        path: path.to_string(),
        frontmatter_raw_b64: BASE64_STANDARD.encode(frontmatter.as_bytes()),
        body_b64: BASE64_STANDARD.encode(body.as_bytes()),
        parse_error,
    }
}

/// Build an [`AddAddAlternate`] with raw bytes captured from the loser's
/// merge-input string.
pub(super) fn build_add_add_alternate(id: String, original_path: String, raw: &str) -> AddAddAlternate {
    let (frontmatter, body) = split_raw_document(raw);
    let body_bytes = body.as_bytes();
    let body_sha256 = format!("sha256:{}", hex::encode(Sha256Hasher::digest(body_bytes)));
    let (body_b64, body_artifact_ref) = if body_bytes.len() > BODY_INLINE_LIMIT {
        // Deferred: wire body artifact store for large bodies. Slot stays empty.
        (None, None)
    } else {
        (Some(BASE64_STANDARD.encode(body_bytes)), None)
    };
    AddAddAlternate {
        id,
        original_path,
        frontmatter_yaml_b64: BASE64_STANDARD.encode(frontmatter.as_bytes()),
        body_sha256,
        body_b64,
        body_artifact_ref,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_raw_document_strips_delimiters() {
        let raw = "---\nschema_version: 1\nid: x\n---\nbody body\n";
        let (fm, body) = split_raw_document(raw);
        assert_eq!(fm, "schema_version: 1\nid: x");
        assert_eq!(body, "body body\n");
    }

    #[test]
    fn split_raw_document_handles_missing_delimiters() {
        let raw = "no frontmatter at all\n";
        let (fm, body) = split_raw_document(raw);
        assert!(fm.is_empty());
        assert_eq!(body, raw);
    }

    #[test]
    fn build_add_add_alternate_round_trips_bytes() {
        let raw = "---\nid: mem_x\n---\nhello body\n";
        let alt = build_add_add_alternate("mem_x".to_string(), "agent/patterns/x.md".to_string(), raw);
        let frontmatter_bytes = BASE64_STANDARD.decode(alt.frontmatter_yaml_b64).expect("decode"); // expect-justified: test
        let body_bytes = BASE64_STANDARD.decode(alt.body_b64.expect("inline body")).expect("decode"); // expect-justified: test
        assert_eq!(std::str::from_utf8(&frontmatter_bytes).unwrap(), "id: mem_x"); // unwrap-justified: test
        assert_eq!(std::str::from_utf8(&body_bytes).unwrap(), "hello body\n"); // unwrap-justified: test
        assert!(alt.body_sha256.starts_with("sha256:"));
    }

    #[test]
    fn fresh_diagnostic_emits_ulid_merge_id() {
        let diag = fresh_diagnostic(MergeStatus::CleanWithWarnings, "test");
        assert!(diag.merge_id.starts_with("merge_"));
        assert!(diag.merge_id.len() > "merge_".len());
    }

    #[test]
    fn union_diagnostics_dedupes_by_merge_id_sorts_by_created_at() {
        let earlier = MergeDiagnostic {
            merge_id: "merge_001".to_string(),
            created_at: Utc::now() - chrono::Duration::seconds(10),
            status: MergeStatus::CleanWithWarnings,
            human_reason: "older".to_string(),
            ..Default::default()
        };
        let later = MergeDiagnostic {
            merge_id: "merge_002".to_string(),
            created_at: Utc::now(),
            status: MergeStatus::Quarantined,
            human_reason: "newer".to_string(),
            ..Default::default()
        };
        let ours = serde_json::to_value(vec![earlier.clone()]).unwrap(); // unwrap-justified: test
        let theirs = serde_json::to_value(vec![earlier.clone(), later.clone()]).unwrap(); // unwrap-justified: test
        let unioned = union_diagnostics(None, Some(&ours), Some(&theirs), None).expect("non-empty"); // expect-justified: test
        let entries: Vec<MergeDiagnostic> = serde_json::from_value(unioned).expect("decode"); // expect-justified: test
        assert_eq!(entries.len(), 2);
        assert!(entries[0].created_at <= entries[1].created_at);
    }
}
