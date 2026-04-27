//! Per-field 3-way merge rules for spec §14.4.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::model::{Entity, Evidence, Frontmatter, Memory, Scope, Sensitivity, TombstoneEvent};

use super::quarantine::EvidenceNearDuplicate;

/// A single conflicting-field diagnostic emitted by per-field merges.
///
/// Per-field merges emit one diagnostic per conflict; the orchestrator
/// routes them into the appropriate spec §6.10 array
/// (`preserved_sources`, `lifecycle_notes`, etc.).
#[derive(Clone, Debug, PartialEq)]
pub(super) struct FieldDiagnostic {
    /// Field name (e.g. `sensitivity`).
    pub field: String,
    /// Free-form details serialized into `preserved_sources[]`.
    pub note: Value,
    /// Bucket: which top-level array this diagnostic belongs in.
    pub bucket: DiagnosticBucket,
}

/// Top-level diagnostic buckets defined by spec §6.10.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)] // `LifecycleNotes` / `EvidenceNearDuplicates` are reserved
                    // for future per-field diagnostics; orchestrator currently
                    // populates them via direct `Vec` accumulation.
pub(super) enum DiagnosticBucket {
    /// `_merge_diagnostics.preserved_sources[]`.
    PreservedSources,
    /// `_merge_diagnostics.lifecycle_notes[]`.
    LifecycleNotes,
    /// `_merge_diagnostics.evidence_near_duplicates[]`.
    EvidenceNearDuplicates,
}

/// Reasons a field merge can force a whole-document quarantine.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct QuarantineReason {
    /// Names of fields that conflict.
    pub conflicting_fields: Vec<String>,
    /// Operator-facing reason rendered into `_merge_diagnostics.human_reason`.
    pub human_reason: String,
}

/// Top-level merge for [`Frontmatter`] scalars and arrays per spec §14.4.
///
/// Returns merged frontmatter plus accumulated diagnostics. A
/// [`QuarantineReason`] short-circuits the orchestrator into the
/// quarantine-retry path; clean returns surface `lifecycle_notes` etc.
pub(super) struct ScalarMergeReport {
    /// Merged frontmatter (with arrays canonically sorted).
    pub merged: Frontmatter,
    /// Conflicting field names accumulated during merge.
    pub conflicting_fields: Vec<String>,
    /// Field-level diagnostic entries, bucketed by spec §6.10 array.
    pub diagnostics: Vec<FieldDiagnostic>,
    /// `evidence_near_duplicates[]` entries surfaced from the evidence merge.
    pub evidence_near_duplicates: Vec<EvidenceNearDuplicate>,
    /// Quarantine reason if any immutable/conflicting field tripped quarantine.
    pub quarantine: Option<QuarantineReason>,
}

/// Merge frontmatter scalar and array fields per spec §14.4.
pub(super) fn merge_frontmatter_scalars(base: &Memory, ours: &Memory, theirs: &Memory) -> ScalarMergeReport {
    let mut merged = ours.frontmatter.clone();
    let mut conflicting_fields: Vec<String> = Vec::new();
    let mut diagnostics: Vec<FieldDiagnostic> = Vec::new();
    let mut evidence_near_duplicates: Vec<EvidenceNearDuplicate> = Vec::new();

    if let Some(reason) =
        check_immutable_fields(&base.frontmatter, &ours.frontmatter, &theirs.frontmatter, &mut conflicting_fields)
    {
        return ScalarMergeReport {
            merged,
            conflicting_fields,
            diagnostics,
            evidence_near_duplicates,
            quarantine: Some(reason),
        };
    }

    apply_scalar_rules(&base.frontmatter, &ours.frontmatter, &theirs.frontmatter, &mut merged, &mut diagnostics);

    if let Some(reason) = merge_confidence_three_way(
        &base.frontmatter,
        &ours.frontmatter,
        &theirs.frontmatter,
        &mut merged,
        &mut diagnostics,
    ) {
        conflicting_fields.push("confidence".to_string());
        return ScalarMergeReport {
            merged,
            conflicting_fields,
            diagnostics,
            evidence_near_duplicates,
            quarantine: Some(reason),
        };
    }
    if let Some(reason) = merge_sensitivity_three_way(
        &base.frontmatter,
        &ours.frontmatter,
        &theirs.frontmatter,
        &mut merged,
        &mut diagnostics,
        &mut conflicting_fields,
    ) {
        return ScalarMergeReport {
            merged,
            conflicting_fields,
            diagnostics,
            evidence_near_duplicates,
            quarantine: Some(reason),
        };
    }

    if let Some(reason) = merge_review_state(
        &base.frontmatter,
        &ours.frontmatter,
        &theirs.frontmatter,
        &mut merged,
        &mut diagnostics,
        &mut conflicting_fields,
    ) {
        return ScalarMergeReport {
            merged,
            conflicting_fields,
            diagnostics,
            evidence_near_duplicates,
            quarantine: Some(reason),
        };
    }

    merge_required_user_confirmation(&base.frontmatter, &ours.frontmatter, &theirs.frontmatter, &mut merged);

    merge_timestamps(&base.frontmatter, &ours.frontmatter, &theirs.frontmatter, &mut merged);

    merge_array_unions(
        &base.frontmatter,
        &ours.frontmatter,
        &theirs.frontmatter,
        &mut merged,
        &mut evidence_near_duplicates,
    );

    merge_extras(&base.frontmatter.extras, &ours.frontmatter.extras, &theirs.frontmatter.extras, &mut merged);

    merge_retrieval_policy_per_key(&base.frontmatter, &ours.frontmatter, &theirs.frontmatter, &mut merged);
    merge_write_policy_per_key(&base.frontmatter, &ours.frontmatter, &theirs.frontmatter, &mut merged);
    apply_sensitivity_index_clamp(&mut merged);

    ScalarMergeReport { merged, conflicting_fields, diagnostics, evidence_near_duplicates, quarantine: None }
}

/// Spec §14.4 row 3: `type`, `scope`, `namespace`, `canonical_namespace_id`
/// are immutable. Same-field divergence quarantines.
fn is_immutable_field(name: &str) -> bool {
    matches!(name, "type" | "scope" | "namespace" | "canonical_namespace_id")
}

fn check_immutable_fields(
    base: &Frontmatter,
    ours: &Frontmatter,
    theirs: &Frontmatter,
    conflicting_fields: &mut Vec<String>,
) -> Option<QuarantineReason> {
    let mut conflicts: Vec<String> = Vec::new();
    if immutable_conflict_typed(&base.memory_type, &ours.memory_type, &theirs.memory_type) {
        conflicts.push("type".to_string());
    }
    if immutable_conflict_typed(&base.scope, &ours.scope, &theirs.scope) {
        conflicts.push("scope".to_string());
    }
    if immutable_conflict_typed(&base.namespace, &ours.namespace, &theirs.namespace) {
        conflicts.push("namespace".to_string());
    }
    if immutable_conflict_typed(
        &base.canonical_namespace_id,
        &ours.canonical_namespace_id,
        &theirs.canonical_namespace_id,
    ) {
        conflicts.push("canonical_namespace_id".to_string());
    }
    debug_assert!(conflicts.iter().all(|name| is_immutable_field(name)));
    if conflicts.is_empty() {
        None
    } else {
        let reason = QuarantineReason {
            conflicting_fields: conflicts.clone(),
            human_reason: format!("immutable field divergence - {}", conflicts.join(", ")),
        };
        for field in &conflicts {
            conflicting_fields.push(field.clone());
        }
        Some(reason)
    }
}

fn immutable_conflict_typed<T: PartialEq>(base: &T, ours: &T, theirs: &T) -> bool {
    ours != theirs && base != ours && base != theirs
}

/// Apply the asymmetric "ours unchanged → take theirs" rule for scalar fields
/// that don't need quarantine handling on conflict (summary, author, source).
#[allow(clippy::too_many_arguments)]
fn apply_scalar_rules(
    base: &Frontmatter,
    ours: &Frontmatter,
    theirs: &Frontmatter,
    merged: &mut Frontmatter,
    diagnostics: &mut Vec<FieldDiagnostic>,
) {
    take_theirs_if_ours_unchanged(&base.summary, &ours.summary, &theirs.summary, &mut merged.summary);
    take_theirs_if_ours_unchanged(&base.author, &ours.author, &theirs.author, &mut merged.author);
    take_theirs_if_ours_unchanged(&base.source, &ours.source, &theirs.source, &mut merged.source);

    if ours.summary != theirs.summary && ours.summary != base.summary && theirs.summary != base.summary {
        let later_side = if theirs.updated_at > ours.updated_at { "theirs" } else { "ours" };
        let later = if later_side == "theirs" { theirs } else { ours };
        merged.summary = later.summary.clone();
        diagnostics.push(FieldDiagnostic {
            field: "summary".to_string(),
            note: serde_json::json!({
                "field": "summary",
                "winner": later_side,
                "loser_side": if later_side == "theirs" { "ours" } else { "theirs" },
                "loser_value": if later_side == "theirs" { ours.summary.clone() } else { theirs.summary.clone() },
            }),
            bucket: DiagnosticBucket::PreservedSources,
        });
    }
}

fn take_theirs_if_ours_unchanged<T: PartialEq + Clone>(base: &T, ours: &T, theirs: &T, merged: &mut T) {
    if ours == base && theirs != base {
        *merged = theirs.clone();
    }
}

/// Spec §14.4: `confidence` 3-way; conflict picks later `updated_at`; delta
/// > 0.25 quarantines.
#[allow(clippy::too_many_arguments)]
fn merge_confidence_three_way(
    base: &Frontmatter,
    ours: &Frontmatter,
    theirs: &Frontmatter,
    merged: &mut Frontmatter,
    diagnostics: &mut Vec<FieldDiagnostic>,
) -> Option<QuarantineReason> {
    let ours_changed = (ours.confidence - base.confidence).abs() >= f64::EPSILON;
    let theirs_changed = (theirs.confidence - base.confidence).abs() >= f64::EPSILON;
    match (ours_changed, theirs_changed) {
        (false, true) => {
            merged.confidence = theirs.confidence;
            None
        }
        (true, false) => {
            merged.confidence = ours.confidence;
            None
        }
        (true, true) if (ours.confidence - theirs.confidence).abs() < f64::EPSILON => None,
        (true, true) => {
            if (ours.confidence - theirs.confidence).abs() > 0.25 {
                Some(QuarantineReason {
                    conflicting_fields: vec!["confidence".to_string()],
                    human_reason: format!(
                        "confidence delta {:.3} exceeds 0.25 quarantine threshold",
                        (ours.confidence - theirs.confidence).abs()
                    ),
                })
            } else {
                let later_side = if theirs.updated_at > ours.updated_at { "theirs" } else { "ours" };
                merged.confidence = if later_side == "theirs" { theirs.confidence } else { ours.confidence };
                diagnostics.push(FieldDiagnostic {
                    field: "confidence".to_string(),
                    note: serde_json::json!({
                        "field": "confidence",
                        "winner": later_side,
                        "ours": ours.confidence,
                        "theirs": theirs.confidence,
                    }),
                    bucket: DiagnosticBucket::PreservedSources,
                });
                None
            }
        }
        (false, false) => None,
    }
}

/// Spec §14.4 sensitivity row: 3-way; conflict resolves to maximum order
/// `personal > confidential > internal > public` and records loser.
#[allow(clippy::too_many_arguments)]
fn merge_sensitivity_three_way(
    base: &Frontmatter,
    ours: &Frontmatter,
    theirs: &Frontmatter,
    merged: &mut Frontmatter,
    diagnostics: &mut Vec<FieldDiagnostic>,
    conflicting_fields: &mut Vec<String>,
) -> Option<QuarantineReason> {
    if ours.sensitivity == base.sensitivity && theirs.sensitivity != base.sensitivity {
        merged.sensitivity = theirs.sensitivity;
        return None;
    }
    if theirs.sensitivity == base.sensitivity && ours.sensitivity != base.sensitivity {
        merged.sensitivity = ours.sensitivity;
        return None;
    }
    if ours.sensitivity == theirs.sensitivity {
        return None;
    }
    let resolved = stricter_sensitivity(ours.sensitivity, theirs.sensitivity);
    let (winning_side, losing_side, losing_value) = if resolved == ours.sensitivity {
        ("ours", "theirs", theirs.sensitivity)
    } else {
        ("theirs", "ours", ours.sensitivity)
    };
    merged.sensitivity = resolved;
    diagnostics.push(FieldDiagnostic {
        field: "sensitivity".to_string(),
        note: serde_json::json!({
            "field": "sensitivity",
            "base": base.sensitivity,
            "ours": ours.sensitivity,
            "theirs": theirs.sensitivity,
            "resolved": resolved,
            "winning_side": winning_side,
            "losing_side": losing_side,
            "losing_value": losing_value,
        }),
        bucket: DiagnosticBucket::PreservedSources,
    });
    conflicting_fields.push("sensitivity".to_string());
    None
}

/// Sensitivity is `Ord`-derived; the `personal > confidential > internal >
/// public` ordering is enforced by the enum's declaration order in `model.rs`.
fn stricter_sensitivity(a: Sensitivity, b: Sensitivity) -> Sensitivity {
    a.max(b)
}

/// Spec §14.4: `review_state` stricter wins (`pending > rejected > approved >
/// null`); `approved` vs `rejected` quarantines.
#[allow(clippy::too_many_arguments)]
fn merge_review_state(
    base: &Frontmatter,
    ours: &Frontmatter,
    theirs: &Frontmatter,
    merged: &mut Frontmatter,
    diagnostics: &mut Vec<FieldDiagnostic>,
    conflicting_fields: &mut Vec<String>,
) -> Option<QuarantineReason> {
    let ours_state = review_state_rank(ours.review_state.as_deref());
    let theirs_state = review_state_rank(theirs.review_state.as_deref());
    let approved_vs_rejected = matches!(
        (ours.review_state.as_deref(), theirs.review_state.as_deref()),
        (Some("approved"), Some("rejected")) | (Some("rejected"), Some("approved"))
    );
    if approved_vs_rejected {
        conflicting_fields.push("review_state".to_string());
        return Some(QuarantineReason {
            conflicting_fields: vec!["review_state".to_string()],
            human_reason: "review_state collision - approved vs rejected".to_string(),
        });
    }
    if ours.review_state == base.review_state && theirs.review_state != base.review_state {
        merged.review_state = theirs.review_state.clone();
    } else if theirs.review_state == base.review_state && ours.review_state != base.review_state {
        merged.review_state = ours.review_state.clone();
    } else if ours.review_state != theirs.review_state {
        let stricter = ours_state.max(theirs_state);
        merged.review_state = match stricter {
            3 => Some("pending".to_string()),
            2 => Some("rejected".to_string()),
            1 => Some("approved".to_string()),
            _ => None,
        };
        diagnostics.push(FieldDiagnostic {
            field: "review_state".to_string(),
            note: serde_json::json!({
                "field": "review_state",
                "ours": ours.review_state,
                "theirs": theirs.review_state,
                "resolved": merged.review_state,
            }),
            bucket: DiagnosticBucket::PreservedSources,
        });
    }
    None
}

fn review_state_rank(state: Option<&str>) -> u8 {
    match state {
        Some("pending") => 3,
        Some("rejected") => 2,
        Some("approved") => 1,
        _ => 0,
    }
}

/// Spec §14.4: `requires_user_confirmation` true wins on conflict.
fn merge_required_user_confirmation(
    base: &Frontmatter,
    ours: &Frontmatter,
    theirs: &Frontmatter,
    merged: &mut Frontmatter,
) {
    if ours.requires_user_confirmation == base.requires_user_confirmation
        && theirs.requires_user_confirmation != base.requires_user_confirmation
    {
        merged.requires_user_confirmation = theirs.requires_user_confirmation;
    } else if ours.requires_user_confirmation != theirs.requires_user_confirmation {
        merged.requires_user_confirmation = ours.requires_user_confirmation || theirs.requires_user_confirmation;
    }
}

/// Spec §14.4: `updated_at = max`, `created_at = min`.
fn merge_timestamps(base: &Frontmatter, ours: &Frontmatter, theirs: &Frontmatter, merged: &mut Frontmatter) {
    let _ = base; // base is only consulted for individual scalar rules; min/max suffice here
    merged.updated_at = max_datetime(ours.updated_at, theirs.updated_at);
    merged.created_at = min_datetime(ours.created_at, theirs.created_at);
}

fn max_datetime(a: DateTime<Utc>, b: DateTime<Utc>) -> DateTime<Utc> {
    if a >= b {
        a
    } else {
        b
    }
}

fn min_datetime(a: DateTime<Utc>, b: DateTime<Utc>) -> DateTime<Utc> {
    if a <= b {
        a
    } else {
        b
    }
}

/// Spec §14.4 array unions; each is keyed and sorted to keep merges
/// commutative (B-MG-6).
#[allow(clippy::too_many_arguments)]
fn merge_array_unions(
    _base: &Frontmatter,
    ours: &Frontmatter,
    theirs: &Frontmatter,
    merged: &mut Frontmatter,
    near_duplicates: &mut Vec<EvidenceNearDuplicate>,
) {
    merged.tags = union_strings_sorted(&ours.tags, &theirs.tags);
    merged.aliases = union_strings_sorted(&ours.aliases, &theirs.aliases);
    merged.supersedes = union_memory_ids_sorted(&ours.supersedes, &theirs.supersedes);
    merged.superseded_by = union_memory_ids_sorted(&ours.superseded_by, &theirs.superseded_by);
    merged.related = union_memory_ids_sorted(&ours.related, &theirs.related);
    merged.entities = merge_entities_id_keyed(&ours.entities, &theirs.entities);
    merged.tombstone_events = merge_tombstone_events_id_keyed(&ours.tombstone_events, &theirs.tombstone_events);
    let (evidence, near) = merge_evidence_id_keyed(&ours.evidence, &theirs.evidence);
    merged.evidence = evidence;
    *near_duplicates = near;
}

fn union_strings_sorted(left: &[String], right: &[String]) -> Vec<String> {
    let mut set: std::collections::BTreeSet<String> = left.iter().cloned().collect();
    set.extend(right.iter().cloned());
    set.into_iter().collect()
}

fn union_memory_ids_sorted(
    left: &[crate::model::MemoryId],
    right: &[crate::model::MemoryId],
) -> Vec<crate::model::MemoryId> {
    let set: std::collections::BTreeSet<String> =
        left.iter().map(|id| id.as_str().to_string()).chain(right.iter().map(|id| id.as_str().to_string())).collect();
    set.into_iter().filter_map(|raw| crate::model::MemoryId::try_new(raw).ok()).collect()
}

/// Spec §14.4: `entities` union by `id`; label conflict preserves newer label
/// (Phase 4 keeps simple `last-wins` because there is no per-entry timestamp).
fn merge_entities_id_keyed(ours: &[Entity], theirs: &[Entity]) -> Vec<Entity> {
    let mut by_id: BTreeMap<String, Entity> = BTreeMap::new();
    for entity in ours.iter().chain(theirs.iter()) {
        by_id.insert(entity.id.clone(), entity.clone());
    }
    by_id.into_values().collect()
}

/// Spec §14.4: `tombstone_events` union by event `id`; deterministic sort by
/// `id` ASC for two-clone convergence.
fn merge_tombstone_events_id_keyed(ours: &[TombstoneEvent], theirs: &[TombstoneEvent]) -> Vec<TombstoneEvent> {
    let mut by_id: BTreeMap<String, TombstoneEvent> = BTreeMap::new();
    for event in ours.iter().chain(theirs.iter()) {
        by_id.insert(event.id.clone(), event.clone());
    }
    by_id.into_values().collect()
}

/// Spec §14.4: `evidence` union by `id`; fallback to
/// `(quote_norm_hash, ref)`; near-duplicates surfaced in diagnostics.
pub(super) fn merge_evidence_id_keyed(
    ours: &[Evidence],
    theirs: &[Evidence],
) -> (Vec<Evidence>, Vec<EvidenceNearDuplicate>) {
    let mut by_id: BTreeMap<String, Evidence> = BTreeMap::new();
    let mut near_duplicates: Vec<EvidenceNearDuplicate> = Vec::new();
    for evidence in ours.iter().chain(theirs.iter()) {
        if let Some(existing) = by_id.get(&evidence.id) {
            if existing.quote != evidence.quote || existing.reference != evidence.reference {
                near_duplicates.push(EvidenceNearDuplicate {
                    evidence_id: evidence.id.clone(),
                    primary_quote: existing.quote.clone(),
                    near_duplicate_quote: evidence.quote.clone(),
                });
            }
        } else if let Some((existing_id, _)) = by_id.iter().find(|(_, candidate)| {
            // secondary_key returns (String, String); require at least one non-empty
            // component before treating as a match (avoids false positives on empty keys).
            let key = secondary_key(evidence);
            secondary_key(candidate) == key && !(key.0.is_empty() && key.1.is_empty())
        }) {
            near_duplicates.push(EvidenceNearDuplicate {
                evidence_id: existing_id.clone(),
                primary_quote: by_id[existing_id].quote.clone(),
                near_duplicate_quote: evidence.quote.clone(),
            });
        } else {
            by_id.insert(evidence.id.clone(), evidence.clone());
        }
    }
    (by_id.into_values().collect(), near_duplicates)
}

fn secondary_key(evidence: &Evidence) -> (String, String) {
    let normalized = evidence.quote.split_whitespace().collect::<Vec<_>>().join(" ");
    (evidence.quote_norm_hash.clone().unwrap_or(normalized), evidence.reference.clone())
}

/// Spec §14.4: per-key 3-way merge of `extras`. `regression.occurrences[]`
/// gets the special G-counter union path.
pub(super) fn merge_extras(
    base: &BTreeMap<String, Value>,
    ours: &BTreeMap<String, Value>,
    theirs: &BTreeMap<String, Value>,
    merged: &mut Frontmatter,
) {
    let keys: std::collections::BTreeSet<String> =
        base.keys().chain(ours.keys()).chain(theirs.keys()).cloned().collect();
    let mut extras: BTreeMap<String, Value> = BTreeMap::new();
    for key in keys {
        if key == "regression" {
            if let Some(value) = merge_regression(ours.get(&key), theirs.get(&key)) {
                extras.insert(key, value);
            }
            continue;
        }
        if let Some(value) = three_way_value(base.get(&key), ours.get(&key), theirs.get(&key)) {
            extras.insert(key, value);
        }
    }
    merged.extras = extras;
}

fn three_way_value(base: Option<&Value>, ours: Option<&Value>, theirs: Option<&Value>) -> Option<Value> {
    match (base, ours, theirs) {
        (_, Some(o), Some(t)) if values_equivalent(o, t) => Some(o.clone()),
        (Some(b), Some(o), Some(t)) if values_equivalent(o, b) => Some(t.clone()),
        (Some(b), Some(o), Some(t)) if values_equivalent(t, b) => Some(o.clone()),
        (None, Some(o), None) | (Some(_), Some(o), None) => Some(o.clone()),
        (None, None, Some(t)) | (Some(_), None, Some(t)) => Some(t.clone()),
        // True 3-way conflict: ours wins on the file, conflict surfaced via
        // diagnostics in the orchestrator. Phase 4 punts a per-key diagnostic
        // for unknown extras — they remain rare in practice and the spec
        // §14.4 text places them in `clean_with_warnings` rather than
        // quarantining.
        (_, Some(o), Some(_)) => Some(o.clone()),
        _ => None,
    }
}

fn values_equivalent(a: &Value, b: &Value) -> bool {
    a == b
}

/// Spec §14.4 regression row: occurrences union by id, max count.
fn merge_regression(ours: Option<&Value>, theirs: Option<&Value>) -> Option<Value> {
    let mut merged = ours.cloned().or_else(|| theirs.cloned())?;
    let object = merged.as_object_mut()?;
    let mut occurrences: BTreeMap<String, Value> = BTreeMap::new();
    for value in [ours, theirs].into_iter().flatten() {
        let Some(values) = value.get("occurrences").and_then(|value| value.as_array()) else {
            continue;
        };
        for occurrence in values {
            let Some(id) = occurrence.get("id").and_then(|value| value.as_str()) else {
                continue;
            };
            match occurrences.get(id) {
                Some(existing)
                    if existing.get("count").and_then(|v| v.as_u64())
                        >= occurrence.get("count").and_then(|v| v.as_u64()) => {}
                _ => {
                    occurrences.insert(id.to_string(), occurrence.clone());
                }
            }
        }
    }
    object.insert("occurrences".to_string(), Value::Array(occurrences.into_values().collect()));
    Some(merged)
}

/// Spec §14.4: `retrieval_policy` recursive per-key 3-way; safety keys
/// (`index_body`, `index_embeddings`, `mask_personal_for_synthesis`,
/// `passive_recall`, `max_scope`) get stricter-wins on conflict.
fn merge_retrieval_policy_per_key(
    base: &Frontmatter,
    ours: &Frontmatter,
    theirs: &Frontmatter,
    merged: &mut Frontmatter,
) {
    merged.retrieval_policy.passive_recall = stricter_bool_off(
        base.retrieval_policy.passive_recall,
        ours.retrieval_policy.passive_recall,
        theirs.retrieval_policy.passive_recall,
    );
    merged.retrieval_policy.index_body = stricter_bool_off(
        base.retrieval_policy.index_body,
        ours.retrieval_policy.index_body,
        theirs.retrieval_policy.index_body,
    );
    merged.retrieval_policy.index_embeddings = stricter_bool_off(
        base.retrieval_policy.index_embeddings,
        ours.retrieval_policy.index_embeddings,
        theirs.retrieval_policy.index_embeddings,
    );
    merged.retrieval_policy.mask_personal_for_synthesis = stricter_bool_on(
        base.retrieval_policy.mask_personal_for_synthesis,
        ours.retrieval_policy.mask_personal_for_synthesis,
        theirs.retrieval_policy.mask_personal_for_synthesis,
    );
    merged.retrieval_policy.max_scope = narrower_scope(
        base.retrieval_policy.max_scope,
        ours.retrieval_policy.max_scope,
        theirs.retrieval_policy.max_scope,
    );
}

fn merge_write_policy_per_key(base: &Frontmatter, ours: &Frontmatter, theirs: &Frontmatter, merged: &mut Frontmatter) {
    merged.write_policy.human_review_required = stricter_bool_on(
        base.write_policy.human_review_required,
        ours.write_policy.human_review_required,
        theirs.write_policy.human_review_required,
    );
    if ours.write_policy.policy_applied == base.write_policy.policy_applied
        && theirs.write_policy.policy_applied != base.write_policy.policy_applied
    {
        merged.write_policy.policy_applied = theirs.write_policy.policy_applied.clone();
    }
    if ours.write_policy.expected_base_hash == base.write_policy.expected_base_hash
        && theirs.write_policy.expected_base_hash != base.write_policy.expected_base_hash
    {
        merged.write_policy.expected_base_hash = theirs.write_policy.expected_base_hash.clone();
    }
}

/// Stricter-wins where `false` is more restrictive (e.g. `index_body=false`
/// is stricter). Keeps a deliberate downgrade by one side when the other
/// matches base.
fn stricter_bool_off(base: bool, ours: bool, theirs: bool) -> bool {
    if ours == base && theirs != base {
        return theirs;
    }
    if theirs == base && ours != base {
        return ours;
    }
    ours && theirs
}

/// Stricter-wins where `true` is more restrictive (e.g.
/// `mask_personal_for_synthesis=true`).
fn stricter_bool_on(base: bool, ours: bool, theirs: bool) -> bool {
    if ours == base && theirs != base {
        return theirs;
    }
    if theirs == base && ours != base {
        return ours;
    }
    ours || theirs
}

fn narrower_scope(_base: Scope, ours: Scope, theirs: Scope) -> Scope {
    if scope_rank(ours) <= scope_rank(theirs) {
        ours
    } else {
        theirs
    }
}

/// Lower rank = narrower. Subagent < Agent < User < Project < Org.
fn scope_rank(scope: Scope) -> u8 {
    match scope {
        Scope::Subagent => 0,
        Scope::Agent => 1,
        Scope::User => 2,
        Scope::Project => 3,
        Scope::Org => 4,
    }
}

/// Defense-in-depth: if merged sensitivity is confidential/personal, force
/// indexing flags off. Mirrors validator §6.11 #13.
fn apply_sensitivity_index_clamp(merged: &mut Frontmatter) {
    if matches!(merged.sensitivity, Sensitivity::Confidential | Sensitivity::Personal) {
        merged.retrieval_policy.index_body = false;
        merged.retrieval_policy.index_embeddings = false;
        merged.retrieval_policy.mask_personal_for_synthesis = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stricter_bool_off_keeps_unilateral_downgrade() {
        // base=true, ours=false (downgrade), theirs=true → false survives
        assert!(!stricter_bool_off(true, false, true));
    }

    #[test]
    fn stricter_bool_off_picks_false_on_full_conflict() {
        // base=true, ours=false, theirs=false → false (both stricter)
        assert!(!stricter_bool_off(true, false, false));
    }

    #[test]
    fn stricter_bool_on_keeps_unilateral_upgrade() {
        // base=false, ours=true (upgrade), theirs=false → true survives
        assert!(stricter_bool_on(false, true, false));
    }

    #[test]
    fn review_state_rank_orders_by_strictness() {
        assert!(review_state_rank(Some("pending")) > review_state_rank(Some("rejected")));
        assert!(review_state_rank(Some("rejected")) > review_state_rank(Some("approved")));
        assert!(review_state_rank(Some("approved")) > review_state_rank(None));
    }

    #[test]
    fn evidence_id_collision_with_different_quote_records_near_duplicate() {
        let ev_a = Evidence {
            id: "ev_001".to_string(),
            quote: "alpha".to_string(),
            quote_norm_hash: None,
            reference: "file://a".to_string(),
            weight: 1.0,
            observed_at: None,
            source: None,
        };
        let ev_b = Evidence { quote: "beta".to_string(), ..ev_a.clone() };
        let (merged, near) = merge_evidence_id_keyed(&[ev_a], &[ev_b]);
        assert_eq!(merged.len(), 1);
        assert_eq!(near.len(), 1);
        assert_eq!(near[0].evidence_id, "ev_001");
    }

    #[test]
    fn evidence_id_distinct_unions_both_sorted() {
        // Two evidence entries with distinct ids and distinct secondary keys
        // (both quote and ref differ) should both survive the merge,
        // ordered by id ascending for two-clone convergence.
        let ev_a = Evidence {
            id: "ev_002".to_string(),
            quote: "alpha".to_string(),
            quote_norm_hash: None,
            reference: "file://a".to_string(),
            weight: 1.0,
            observed_at: None,
            source: None,
        };
        let ev_b = Evidence {
            id: "ev_001".to_string(),
            quote: "beta".to_string(),
            quote_norm_hash: None,
            reference: "file://b".to_string(),
            weight: 1.0,
            observed_at: None,
            source: None,
        };
        let (merged, _) = merge_evidence_id_keyed(&[ev_a], &[ev_b]);
        assert_eq!(merged.iter().map(|e| e.id.clone()).collect::<Vec<_>>(), vec!["ev_001", "ev_002"]);
    }
}
