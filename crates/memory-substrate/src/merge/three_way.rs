//! Top-level Markdown merge orchestrator (spec §14).

use std::collections::BTreeSet;

use crate::error::{MergeError, MergeSide};
use crate::frontmatter::{parse_document, serialize_document, ParsedMemory};
use crate::model::{Memory, MemoryStatus, TrustLevel};
use serde_json::Value;

use super::body_diff3::{merge_body_diff3, BodyMergeOutcome};
use super::field_rules::{merge_frontmatter_scalars, QuarantineReason, ScalarMergeReport};
use super::lifecycle::{apply_lifecycle_take, merge_lifecycle, LifecycleOutcome};
use super::quarantine::{
    build_add_add_alternate, build_unparsed_side, fresh_diagnostic, splice_diagnostic, union_diagnostics,
    AddAddAlternate, MergeStatus, UnparsedSide,
};
use super::source_artifact::merge_source_artifact;
use super::MERGE_DRIVER_SUPPORTED_SCHEMA_VERSION;

/// Merge input blobs.
pub struct MergeInput<'a> {
    /// Base blob.
    pub base: &'a str,
    /// Ours blob.
    pub ours: &'a str,
    /// Theirs blob.
    pub theirs: &'a str,
    /// Repo path.
    pub path: &'a str,
}

/// Merge result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MergeResult {
    /// Clean merged markdown.
    Clean(String),
    /// Semantically valid quarantine markdown.
    Quarantine(String),
}

/// Merge three Markdown blobs per spec §14.
pub fn merge_markdown(input: MergeInput<'_>) -> Result<MergeResult, MergeError> {
    if let Some(result) = merge_source_artifact(&input) {
        return result;
    }
    if let Some(result) = merge_stream_f_file(&input) {
        return result;
    }
    refuse_secret_sensitivity(&input)?;
    enforce_schema_version_gate(&input)?;
    if input.base.trim().is_empty() {
        return add_add_quarantine(&input);
    }

    let parsed = parse_three_sides(&input)?;
    let ParsedSides { base, ours, theirs } = match parsed {
        ParseOutcome::AllParsed(sides) => sides,
        ParseOutcome::Unparsed { recoverable } => {
            return quarantine_unparsed_sides(&input, recoverable);
        }
    };

    enforce_post_parse_schema_gate(&ours, &theirs)?;

    if let Some(fastpath) = clean_fastpath(&input) {
        return Ok(fastpath);
    }

    let scalar_report = merge_frontmatter_scalars(&base.memory, &ours.memory, &theirs.memory);
    if let Some(reason) = scalar_report.quarantine.clone() {
        return quarantine_with_reason(scalar_report, &base.memory, &ours.memory, &theirs.memory, reason);
    }

    let body_outcome = merge_body_diff3(&base.memory.body, &ours.memory.body, &theirs.memory.body);
    finalize_merge(scalar_report, body_outcome, &base.memory, &ours.memory, &theirs.memory)
}

fn merge_stream_f_file(input: &MergeInput<'_>) -> Option<Result<MergeResult, MergeError>> {
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

fn ensure_trailing_newline(text: &str) -> String {
    if text.ends_with('\n') {
        text.to_string()
    } else {
        format!("{text}\n")
    }
}

/// Reject any side that carries `sensitivity: secret` per Q9.
///
/// Textual prefilter rather than enum-variant: spec §6.11 #10 forbids
/// `secret` from being a persisted value, and adding `Sensitivity::Secret`
/// would let other code paths construct it accidentally.
fn refuse_secret_sensitivity(input: &MergeInput<'_>) -> Result<(), MergeError> {
    for (side, raw) in [(MergeSide::Base, input.base), (MergeSide::Ours, input.ours), (MergeSide::Theirs, input.theirs)]
    {
        if frontmatter_carries_secret_sensitivity(raw) {
            return Err(MergeError::SecretSensitivityRefused { side });
        }
    }
    Ok(())
}

fn frontmatter_carries_secret_sensitivity(raw: &str) -> bool {
    let Some(after_open) = raw.strip_prefix("---\n") else {
        return false;
    };
    let Some(end) = after_open.find("\n---\n") else {
        return false;
    };
    let frontmatter = &after_open[..end];
    frontmatter.lines().any(|line| {
        let trimmed = line.trim_start();
        trimmed.eq_ignore_ascii_case("sensitivity: secret")
            || trimmed.eq_ignore_ascii_case("sensitivity:secret")
            || trimmed.eq_ignore_ascii_case("sensitivity:  secret")
            || matches_sensitivity_secret(trimmed)
    })
}

fn matches_sensitivity_secret(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    let Some(rest) = lower.strip_prefix("sensitivity:") else {
        return false;
    };
    rest.trim() == "secret"
}

/// Spec §14.2 #2: any side whose `schema_version` exceeds supported aborts
/// with a typed error before any disk effects.
fn enforce_schema_version_gate(input: &MergeInput<'_>) -> Result<(), MergeError> {
    for raw in [input.base, input.ours, input.theirs] {
        let version = raw_schema_version(raw).unwrap_or(MERGE_DRIVER_SUPPORTED_SCHEMA_VERSION);
        if version > MERGE_DRIVER_SUPPORTED_SCHEMA_VERSION {
            return Err(MergeError::UnsupportedSchema {
                found: version,
                supported: MERGE_DRIVER_SUPPORTED_SCHEMA_VERSION,
            });
        }
    }
    Ok(())
}

fn raw_schema_version(input: &str) -> Option<u32> {
    let after = input.strip_prefix("---\n")?;
    after.lines().take_while(|line| *line != "---").find_map(|line| {
        let raw = line.strip_prefix("schema_version:")?.trim();
        raw.parse().ok()
    })
}

fn enforce_post_parse_schema_gate(ours: &ParsedMemory, theirs: &ParsedMemory) -> Result<(), MergeError> {
    for version in [ours.memory.frontmatter.schema_version, theirs.memory.frontmatter.schema_version] {
        if version > MERGE_DRIVER_SUPPORTED_SCHEMA_VERSION {
            return Err(MergeError::UnsupportedSchema {
                found: version,
                supported: MERGE_DRIVER_SUPPORTED_SCHEMA_VERSION,
            });
        }
    }
    Ok(())
}

/// Generic 3-way fast paths from spec §14.3.
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

struct ParsedSides {
    base: ParsedMemory,
    ours: ParsedMemory,
    theirs: ParsedMemory,
}

#[allow(clippy::large_enum_variant)]
enum ParseOutcome {
    AllParsed(ParsedSides),
    Unparsed { recoverable: Vec<UnparsedSide> },
}

fn parse_three_sides(input: &MergeInput<'_>) -> Result<ParseOutcome, MergeError> {
    let base = parse_document(input.base, None);
    let ours = parse_document(input.ours, None);
    let theirs = parse_document(input.theirs, None);
    let any_failed = base.is_err() || ours.is_err() || theirs.is_err();
    if !any_failed {
        return Ok(ParseOutcome::AllParsed(ParsedSides {
            base: base.expect("ok"),     // expect-justified: any_failed=false guarantees Ok
            ours: ours.expect("ok"),     // expect-justified: any_failed=false guarantees Ok
            theirs: theirs.expect("ok"), // expect-justified: any_failed=false guarantees Ok
        }));
    }
    let attempts = [
        ("base", input.base, base.as_ref().err().map(|err| err.to_string())),
        ("ours", input.ours, ours.as_ref().err().map(|err| err.to_string())),
        ("theirs", input.theirs, theirs.as_ref().err().map(|err| err.to_string())),
    ];
    if attempts.iter().any(|(_, raw, err)| err.is_some() && !has_frontmatter_delimiters(raw)) {
        return Err(MergeError::MissingDelimiters);
    }
    let unparsed: Vec<UnparsedSide> = attempts
        .iter()
        .filter_map(|(side, raw, err)| {
            err.as_ref().map(|message| build_unparsed_side(side, input.path, raw, message.clone()))
        })
        .collect();
    Ok(ParseOutcome::Unparsed { recoverable: unparsed })
}

fn has_frontmatter_delimiters(input: &str) -> bool {
    input.starts_with("---\n") && input[4..].contains("\n---")
}

fn quarantine_unparsed_sides(
    input: &MergeInput<'_>,
    unparsed_sides: Vec<UnparsedSide>,
) -> Result<MergeResult, MergeError> {
    // Pick a parseable side as the carrier so the quarantined file validates.
    let carrier = parse_document(input.ours, None)
        .or_else(|_| parse_document(input.theirs, None))
        .or_else(|_| parse_document(input.base, None))
        .map_err(|err| MergeError::Parse(err.to_string()))?;
    let mut memory = carrier.memory;
    set_quarantined_lifecycle(&mut memory);
    let mut diagnostic =
        fresh_diagnostic(MergeStatus::Quarantined, "frontmatter parse failed on at least one merge side");
    diagnostic.unparsed_sides = unparsed_sides;
    diagnostic.conflicting_fields = vec!["frontmatter".to_string()];
    splice_diagnostic(&mut memory.frontmatter.merge_diagnostics, &diagnostic)
        .map_err(|message| MergeError::Serialize { message })?;
    memory.body = format!(
        "{}\n\n<!-- merge quarantine: unparsed side preserved in _merge_diagnostics -->\n",
        memory.body.trim_end()
    );
    serialize_or_quarantine_retry(memory)
}

fn set_quarantined_lifecycle(memory: &mut Memory) {
    memory.frontmatter.status = MemoryStatus::Quarantined;
    memory.frontmatter.trust_level = TrustLevel::Quarantined;
    memory.frontmatter.review_state = Some("pending".to_string());
}

/// Apply the lifecycle pair table, run the body diff3, splice diagnostics,
/// and serialize. Falls back to quarantine on validation failure (B-MG-15).
#[allow(clippy::too_many_arguments)]
fn finalize_merge(
    mut report: ScalarMergeReport,
    body_outcome: BodyMergeOutcome,
    base: &Memory,
    ours: &Memory,
    theirs: &Memory,
) -> Result<MergeResult, MergeError> {
    let mut lifecycle_notes: Vec<String> = Vec::new();
    let outcome = merge_lifecycle(
        base.frontmatter.status,
        ours.frontmatter.status,
        theirs.frontmatter.status,
        superseded_by_validates(ours),
        superseded_by_validates(theirs),
    );
    let lifecycle_quarantine = match outcome {
        LifecycleOutcome::Continue => None,
        LifecycleOutcome::Take { side, clear_superseded_by, note } => {
            apply_lifecycle_take(&mut report.merged, side, &ours.frontmatter, &theirs.frontmatter, clear_superseded_by);
            if let Some(note) = note {
                lifecycle_notes.push(note);
            }
            None
        }
        LifecycleOutcome::Quarantine(reason) => Some(reason),
    };
    if let Some(reason) = lifecycle_quarantine {
        return quarantine_with_reason(report, base, ours, theirs, reason);
    }

    let mut conflicting_fields = report.conflicting_fields;
    let mut preserved_sources: Vec<serde_json::Value> = Vec::new();
    for diag in &report.diagnostics {
        if !conflicting_fields.contains(&diag.field) {
            conflicting_fields.push(diag.field.clone());
        }
        preserved_sources.push(diag.note.clone());
    }

    let (merged_body, body_conflict) = match body_outcome {
        BodyMergeOutcome::Clean(body) => (body, false),
        BodyMergeOutcome::Conflict(body) => {
            conflicting_fields.push("body".to_string());
            lifecycle_notes.push("body diff3 conflict; manual resolution required".to_string());
            (body, true)
        }
    };

    let merged_memory = Memory { frontmatter: report.merged, body: merged_body, path: ours.path.clone() };

    if body_conflict {
        return quarantine_body_conflict(
            merged_memory,
            base,
            ours,
            theirs,
            conflicting_fields,
            preserved_sources,
            report.evidence_near_duplicates,
            lifecycle_notes,
        );
    }

    splice_clean_diagnostics(
        merged_memory,
        base,
        ours,
        theirs,
        conflicting_fields,
        preserved_sources,
        report.evidence_near_duplicates,
        lifecycle_notes,
    )
}

/// Spec §6.11 #4: superseded requires non-empty `superseded_by`.
fn superseded_by_validates(memory: &Memory) -> bool {
    !memory.frontmatter.superseded_by.is_empty()
}

#[allow(clippy::too_many_arguments)]
fn splice_clean_diagnostics(
    mut memory: Memory,
    base: &Memory,
    ours: &Memory,
    theirs: &Memory,
    conflicting_fields: Vec<String>,
    preserved_sources: Vec<serde_json::Value>,
    near_duplicates: Vec<super::quarantine::EvidenceNearDuplicate>,
    lifecycle_notes: Vec<String>,
) -> Result<MergeResult, MergeError> {
    if conflicting_fields.is_empty()
        && preserved_sources.is_empty()
        && near_duplicates.is_empty()
        && lifecycle_notes.is_empty()
    {
        // Nothing to emit beyond the prior diagnostic union (which preserves
        // older-merge history). Spec §14.4: still union prior diagnostics.
        let unioned = union_diagnostics(
            base.frontmatter.merge_diagnostics.as_ref(),
            ours.frontmatter.merge_diagnostics.as_ref(),
            theirs.frontmatter.merge_diagnostics.as_ref(),
            None,
        );
        memory.frontmatter.merge_diagnostics = unioned;
        return serialize_or_quarantine_retry(memory);
    }
    let mut diagnostic =
        fresh_diagnostic(MergeStatus::CleanWithWarnings, build_human_reason(&conflicting_fields, false));
    diagnostic.conflicting_fields = conflicting_fields;
    diagnostic.preserved_sources = preserved_sources;
    diagnostic.evidence_near_duplicates = near_duplicates;
    diagnostic.lifecycle_notes = lifecycle_notes;
    let unioned = union_diagnostics(
        base.frontmatter.merge_diagnostics.as_ref(),
        ours.frontmatter.merge_diagnostics.as_ref(),
        theirs.frontmatter.merge_diagnostics.as_ref(),
        Some(diagnostic),
    );
    memory.frontmatter.merge_diagnostics = unioned;
    serialize_or_quarantine_retry(memory)
}

#[allow(clippy::too_many_arguments)]
fn quarantine_body_conflict(
    mut memory: Memory,
    base: &Memory,
    ours: &Memory,
    theirs: &Memory,
    conflicting_fields: Vec<String>,
    preserved_sources: Vec<serde_json::Value>,
    near_duplicates: Vec<super::quarantine::EvidenceNearDuplicate>,
    lifecycle_notes: Vec<String>,
) -> Result<MergeResult, MergeError> {
    set_quarantined_lifecycle(&mut memory);
    let mut diagnostic = fresh_diagnostic(MergeStatus::Quarantined, build_human_reason(&conflicting_fields, true));
    diagnostic.conflicting_fields = conflicting_fields;
    diagnostic.preserved_sources = preserved_sources;
    diagnostic.evidence_near_duplicates = near_duplicates;
    diagnostic.lifecycle_notes = lifecycle_notes;
    let unioned = union_diagnostics(
        base.frontmatter.merge_diagnostics.as_ref(),
        ours.frontmatter.merge_diagnostics.as_ref(),
        theirs.frontmatter.merge_diagnostics.as_ref(),
        Some(diagnostic),
    );
    memory.frontmatter.merge_diagnostics = unioned;
    memory.body = format!("{}\n\n<!-- body merge conflict; manual review required -->\n", memory.body.trim_end());
    serialize_or_quarantine_retry(memory)
}

fn build_human_reason(fields: &[String], body_conflict: bool) -> String {
    // Stay within the plain-YAML scalar set that the canonical serializer
    // emits unquoted (alphanumerics + `_-./@`); `:` would be reinterpreted as
    // a YAML mapping value on the next round-trip.
    let joined = fields.join(", ");
    if body_conflict {
        format!("body diff3 conflict - fields {joined}")
    } else if fields.is_empty() {
        "merged with diagnostics".to_string()
    } else {
        format!("conflicting fields - {joined}")
    }
}

#[allow(clippy::too_many_arguments)]
fn quarantine_with_reason(
    report: ScalarMergeReport,
    base: &Memory,
    ours: &Memory,
    theirs: &Memory,
    reason: QuarantineReason,
) -> Result<MergeResult, MergeError> {
    let mut memory = Memory { frontmatter: report.merged, body: ours.body.clone(), path: ours.path.clone() };
    set_quarantined_lifecycle(&mut memory);
    let mut diagnostic = fresh_diagnostic(MergeStatus::Quarantined, reason.human_reason);
    diagnostic.conflicting_fields = reason.conflicting_fields;
    for diag in report.diagnostics {
        diagnostic.preserved_sources.push(diag.note);
    }
    diagnostic.evidence_near_duplicates = report.evidence_near_duplicates;
    let unioned = union_diagnostics(
        base.frontmatter.merge_diagnostics.as_ref(),
        ours.frontmatter.merge_diagnostics.as_ref(),
        theirs.frontmatter.merge_diagnostics.as_ref(),
        Some(diagnostic),
    );
    memory.frontmatter.merge_diagnostics = unioned;
    memory.body = format!("{}\n\n<!-- merge quarantine; admin review required -->\n", memory.body.trim_end());
    serialize_or_quarantine_retry(memory)
}

/// Add/add same-path: build a typed quarantine carrying both alternates.
///
/// Spec §14.6: when ids match, surface duplicate-id repair without inventing
/// suffix ids. When ids differ, emit two `add_add_alternates[]` entries with
/// raw bytes captured for mechanical recovery (B-MG-4).
fn add_add_quarantine(input: &MergeInput<'_>) -> Result<MergeResult, MergeError> {
    let ours = parse_document(input.ours, None)
        .map_err(|err| MergeError::ParseSide { side: MergeSide::Ours, message: err.to_string() })?;
    let theirs = parse_document(input.theirs, None)
        .map_err(|err| MergeError::ParseSide { side: MergeSide::Theirs, message: err.to_string() })?;
    let id_collision = ours.memory.frontmatter.id == theirs.memory.frontmatter.id;
    let mut memory = ours.memory.clone();
    set_quarantined_lifecycle(&mut memory);
    let alternates: Vec<AddAddAlternate> = if id_collision {
        Vec::new()
    } else {
        vec![
            build_add_add_alternate(
                ours.memory.frontmatter.id.as_str().to_string(),
                input.path.to_string(),
                input.ours,
            ),
            build_add_add_alternate(
                theirs.memory.frontmatter.id.as_str().to_string(),
                input.path.to_string(),
                input.theirs,
            ),
        ]
    };
    let human_reason = if id_collision {
        "duplicate-ID repair required (add/add same-path collision)".to_string()
    } else {
        "add/add same-path conflict; alternate memory preserved".to_string()
    };
    let mut diagnostic = fresh_diagnostic(MergeStatus::Quarantined, human_reason);
    diagnostic.add_add_alternates = alternates;
    diagnostic.conflicting_fields = vec!["add_add".to_string()];
    splice_diagnostic(&mut memory.frontmatter.merge_diagnostics, &diagnostic)
        .map_err(|message| MergeError::Serialize { message })?;
    memory.body = format!(
        "{}\n\n<!-- add/add conflict: alternate memory preserved in _merge_diagnostics -->\n",
        memory.body.trim_end()
    );
    serialize_or_quarantine_retry(memory)
}

/// Spec §14.2 #7: try clean serialize first; on validation failure fall
/// back to a quarantined retry; if even that won't validate, surface
/// [`MergeError::QuarantineWillNotValidate`] so the CLI can exit 1.
fn serialize_or_quarantine_retry(memory: Memory) -> Result<MergeResult, MergeError> {
    let was_quarantined = matches!(memory.frontmatter.status, MemoryStatus::Quarantined);
    match serialize_document(&memory) {
        Ok(text) if was_quarantined => Ok(MergeResult::Quarantine(text)),
        Ok(text) => Ok(MergeResult::Clean(text)),
        Err(err) => {
            if was_quarantined {
                return Err(MergeError::QuarantineWillNotValidate { message: err.to_string() });
            }
            quarantine_validation_retry(memory, err.to_string())
        }
    }
}

fn quarantine_validation_retry(mut memory: Memory, validator_error: String) -> Result<MergeResult, MergeError> {
    set_quarantined_lifecycle(&mut memory);
    let prior = memory.frontmatter.merge_diagnostics.take();
    let mut diagnostic = fresh_diagnostic(
        MergeStatus::Quarantined,
        format!("validation failed after merge - {}", validator_error.replace(':', " ")),
    );
    diagnostic.conflicting_fields = vec!["validation".to_string()];
    let unioned = union_diagnostics(None, prior.as_ref(), None, Some(diagnostic));
    memory.frontmatter.merge_diagnostics = unioned;
    match serialize_document(&memory) {
        Ok(text) => Ok(MergeResult::Quarantine(text)),
        Err(err) => Err(MergeError::QuarantineWillNotValidate { message: err.to_string() }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_sensitivity_prefilter_rejects_ours_side() {
        let secret_doc = "---\nschema_version: 1\nid: x\nsensitivity: secret\n---\nbody\n";
        let input = MergeInput { base: "", ours: secret_doc, theirs: "", path: "p" };
        let err = refuse_secret_sensitivity(&input).expect_err("secret rejected");
        assert!(matches!(err, MergeError::SecretSensitivityRefused { side: MergeSide::Ours }));
    }

    #[test]
    fn secret_sensitivity_prefilter_is_case_insensitive() {
        let raw = "---\nschema_version: 1\nSensitivity: Secret\n---\nbody\n";
        assert!(frontmatter_carries_secret_sensitivity(raw));
    }

    #[test]
    fn secret_sensitivity_prefilter_skips_internal_value() {
        let raw = "---\nschema_version: 1\nsensitivity: internal\n---\nbody\n";
        assert!(!frontmatter_carries_secret_sensitivity(raw));
    }
}
