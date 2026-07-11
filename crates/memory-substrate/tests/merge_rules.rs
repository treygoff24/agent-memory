//! Merge driver behavioral tests.
//!
//! Tests parse the merged YAML and assert on structured values rather than
//! using substring matches; this catches diagnostic-shape regressions that
//! the old `text.contains(...)` style cannot.

use memory_substrate::frontmatter::parse_document;
use memory_substrate::merge::{merge_markdown, MergeError, MergeInput, MergeResult, MergeSide};
use memory_substrate::MemoryStatus;
use proptest::prelude::*;
use serde_json::Value;

#[test]
fn independent_scalar_edits_both_survive() {
    let base = doc("base summary", "base body");
    let ours = doc("ours summary", "base body");
    let theirs = doc("base summary", "theirs body");
    let MergeResult::Clean(text) = merge(&base, &ours, &theirs) else {
        panic!("expected clean merge");
    };
    let parsed = parse_document(&text, None).expect("parse merged");
    assert_eq!(parsed.memory.frontmatter.summary, "ours summary");
    assert!(text.ends_with("theirs body\n"), "merged body keeps theirs edit");
}

#[test]
fn conflicting_body_edits_quarantine_instead_of_dropping_theirs() {
    let base = doc("base summary", "alpha\nbeta\ngamma\n");
    let ours = doc("ours summary", "alpha\nBETA-ours\ngamma\n");
    let theirs = doc("theirs summary", "alpha\nBETA-theirs\ngamma\n");
    let MergeResult::Quarantine(text) = merge(&base, &ours, &theirs) else {
        panic!("expected quarantine");
    };
    let parsed = parse_document(&text, None).expect("parse quarantine");
    assert_eq!(parsed.memory.frontmatter.status, MemoryStatus::Quarantined);
    let diag = first_diagnostic(&parsed.memory.frontmatter.merge_diagnostics);
    assert_eq!(diag["status"], Value::String("quarantined".to_string()));
    assert!(diag["conflicting_fields"].as_array().expect("array").iter().any(|v| v == "body"));
}

#[test]
fn sensitivity_downgrade_on_one_side_survives() {
    let base = doc_with_sensitivity("base summary", "base body", "confidential");
    let ours = doc_with_sensitivity("base summary", "base body", "internal");
    let theirs = doc_with_sensitivity("base summary", "base body", "confidential");
    let MergeResult::Clean(text) = merge(&base, &ours, &theirs) else {
        panic!("expected clean merge");
    };
    let parsed = parse_document(&text, None).expect("parse");
    assert!(matches!(parsed.memory.frontmatter.sensitivity, memory_substrate::Sensitivity::Internal));

    let MergeResult::Clean(text) = merge(&base, &theirs, &ours) else {
        panic!("expected clean merge inverse");
    };
    let parsed = parse_document(&text, None).expect("parse inverse");
    assert!(matches!(parsed.memory.frontmatter.sensitivity, memory_substrate::Sensitivity::Internal));
}

#[test]
fn sensitivity_conflict_resolves_to_more_restrictive_with_diagnostics() {
    let base = doc_with_sensitivity("base summary", "base body", "internal");
    let ours = doc_with_sensitivity("base summary", "base body", "personal");
    let theirs = doc_with_sensitivity("base summary", "base body", "confidential");
    let MergeResult::Clean(text) = merge(&base, &ours, &theirs) else {
        panic!("expected clean merge");
    };
    let parsed = parse_document(&text, None).expect("parse");
    assert!(matches!(parsed.memory.frontmatter.sensitivity, memory_substrate::Sensitivity::Personal));
    assert!(!parsed.memory.frontmatter.retrieval_policy.index_body);
    assert!(!parsed.memory.frontmatter.retrieval_policy.index_embeddings);

    let diag = first_diagnostic(&parsed.memory.frontmatter.merge_diagnostics);
    assert_eq!(diag["status"], Value::String("clean_with_warnings".to_string()));
    let preserved = diag["preserved_sources"].as_array().expect("preserved_sources");
    assert!(preserved.iter().any(|note| note["field"] == "sensitivity"));
    let sensitivity_note = preserved.iter().find(|note| note["field"] == "sensitivity").expect("sensitivity note");
    assert_eq!(sensitivity_note["losing_side"], "theirs");
    assert_eq!(sensitivity_note["losing_value"], "confidential");
}

#[test]
fn diagnostic_carries_merge_id_and_created_at() {
    let base = doc_with_sensitivity("base summary", "base body", "internal");
    let ours = doc_with_sensitivity("base summary", "base body", "personal");
    let theirs = doc_with_sensitivity("base summary", "base body", "confidential");
    let MergeResult::Clean(text) = merge(&base, &ours, &theirs) else {
        panic!("expected clean merge");
    };
    let parsed = parse_document(&text, None).expect("parse");
    let diag = first_diagnostic(&parsed.memory.frontmatter.merge_diagnostics);
    let merge_id = diag["merge_id"].as_str().expect("merge_id");
    assert!(merge_id.starts_with("merge_"));
    assert!(diag["created_at"].is_string(), "created_at present");
}

#[test]
fn evidence_id_collision_emits_near_duplicate_diagnostic() {
    let base = doc("base summary", "base body");
    let ours = doc_with_extra_yaml(
        "base summary",
        "base body",
        r#"evidence:
  -
    id: ev_001
    quote: "alpha quote"
    ref: file://a
"#,
    );
    let theirs = doc_with_extra_yaml(
        "base summary",
        "base body",
        r#"evidence:
  -
    id: ev_001
    quote: "alpha quote with extra words"
    ref: file://a
"#,
    );
    let MergeResult::Clean(text) = merge(&base, &ours, &theirs) else {
        panic!("expected clean merge");
    };
    let parsed = parse_document(&text, None).expect("parse");
    assert_eq!(parsed.memory.frontmatter.evidence.len(), 1);
    let diag = first_diagnostic(&parsed.memory.frontmatter.merge_diagnostics);
    let near = diag["evidence_near_duplicates"].as_array().expect("array");
    assert_eq!(near.len(), 1);
    assert_eq!(near[0]["evidence_id"], "ev_001");
}

#[test]
fn regression_occurrence_counts_merge_by_id_with_max_count() {
    let base = doc("base summary", "base body");
    let ours = doc_with_extra_yaml(
        "base summary",
        "base body",
        r#"regression:
  occurrences:
    -
      count: 2
      id: occ-1
"#,
    );
    let theirs = doc_with_extra_yaml(
        "base summary",
        "base body",
        r#"regression:
  occurrences:
    -
      count: 3
      id: occ-1
"#,
    );

    let MergeResult::Clean(text) = merge(&base, &ours, &theirs) else {
        panic!("expected clean merge");
    };
    let parsed = parse_document(&text, None).expect("parse merged regression");
    let occurrences = parsed.memory.frontmatter.extras["regression"]["occurrences"].as_array().expect("occurrences");
    assert_eq!(occurrences.len(), 1);
    assert_eq!(occurrences[0]["count"], 3);
}

#[test]
fn unknown_fields_use_true_three_way_per_key() {
    let base = doc_with_extra_yaml("base summary", "base body", "custom_unknown: base\n");
    let ours = doc_with_extra_yaml("base summary", "base body", "custom_unknown: ours\n");
    let theirs = doc_with_extra_yaml("base summary", "base body", "custom_unknown: base\ntheirs_only: yes\n");

    let MergeResult::Clean(text) = merge(&base, &ours, &theirs) else {
        panic!("expected clean merge");
    };
    let parsed = parse_document(&text, None).expect("parse merged unknown fields");
    assert_eq!(parsed.memory.frontmatter.extras["custom_unknown"], "ours");
    assert_eq!(parsed.memory.frontmatter.extras["theirs_only"], "yes");
}

#[test]
fn add_add_same_path_quarantine_preserves_alternates_with_raw_bytes() {
    let ours = doc_with_id("mem_20260424_a1b2c3d4e5f60718_000031", "ours summary", "ours body");
    let theirs = doc_with_id("mem_20260424_a1b2c3d4e5f60718_000032", "theirs summary", "theirs body");

    let MergeResult::Quarantine(text) = merge_at("", &ours, &theirs) else {
        panic!("expected quarantine for add/add");
    };
    let parsed = parse_document(&text, None).expect("parse add/add");
    assert_eq!(parsed.memory.frontmatter.status, MemoryStatus::Quarantined);
    let diag = first_diagnostic(&parsed.memory.frontmatter.merge_diagnostics);
    let alternates = diag["add_add_alternates"].as_array().expect("alternates");
    assert_eq!(alternates.len(), 2);
    let theirs_alt = alternates
        .iter()
        .find(|alt| alt["id"] == "mem_20260424_a1b2c3d4e5f60718_000032")
        .expect("theirs alternate present");
    assert!(theirs_alt["frontmatter_yaml_b64"].is_string());
    assert!(theirs_alt["body_b64"].is_string());
    assert!(theirs_alt["body_sha256"].as_str().expect("sha").starts_with("sha256:"));

    // Mechanical recovery: decode and confirm the stored body matches the input.
    let body_b64 = theirs_alt["body_b64"].as_str().expect("body");
    let decoded = base64::engine::general_purpose::STANDARD.decode(body_b64).expect("decode");
    assert_eq!(std::str::from_utf8(&decoded).unwrap(), "theirs body\n");
}

#[test]
fn add_add_id_collision_marks_duplicate_id_repair() {
    let ours = doc_with_id("mem_20260424_a1b2c3d4e5f60718_000040", "ours summary", "ours body");
    let theirs = doc_with_id("mem_20260424_a1b2c3d4e5f60718_000040", "theirs summary", "theirs body");

    let MergeResult::Quarantine(text) = merge_at("", &ours, &theirs) else {
        panic!("expected quarantine for add/add same id");
    };
    let parsed = parse_document(&text, None).expect("parse");
    let diag = first_diagnostic(&parsed.memory.frontmatter.merge_diagnostics);
    let human_reason = diag["human_reason"].as_str().expect("reason");
    assert!(human_reason.contains("duplicate-ID repair required"));
    let alternates = diag["add_add_alternates"].as_array().map(|a| a.len()).unwrap_or(0);
    assert_eq!(alternates, 0, "id-collision case should not invent alternates");
}

#[test]
fn unparsed_side_quarantine_emits_typed_unparsed_sides() {
    let base = doc("base summary", "base body");
    let ours = base.replace("summary: base summary", "summary: [unterminated");
    let theirs = base.replace("base body", "theirs body");

    let MergeResult::Quarantine(text) = merge(&base, &ours, &theirs) else {
        panic!("expected quarantine");
    };
    let parsed = parse_document(&text, None).expect("parse unparsed-side quarantine");
    assert_eq!(parsed.memory.frontmatter.status, MemoryStatus::Quarantined);
    let diag = first_diagnostic(&parsed.memory.frontmatter.merge_diagnostics);
    let unparsed = diag["unparsed_sides"].as_array().expect("unparsed_sides");
    assert!(!unparsed.is_empty());
    let first = &unparsed[0];
    assert!(first["frontmatter_raw_b64"].is_string());
    assert!(first["body_b64"].is_string());
    assert!(first["parse_error"].is_string());
}

#[test]
fn lifecycle_pair_fixture_matrix_outputs_valid_markdown() {
    let statuses = ["candidate", "active", "pinned", "superseded", "archived", "tombstoned", "quarantined"];
    let base = doc_with_status("active", "base body");
    for ours_status in statuses {
        for theirs_status in statuses {
            let ours = doc_with_status(ours_status, "base body");
            let theirs = doc_with_status(theirs_status, "base body");
            let result = merge(&base, &ours, &theirs);
            let markdown = match result {
                MergeResult::Clean(markdown) | MergeResult::Quarantine(markdown) => markdown,
            };
            parse_document(&markdown, None).unwrap_or_else(|err| {
                panic!("invalid lifecycle merge for ours={ours_status} theirs={theirs_status}: {err}\n{markdown}");
            });
        }
    }
}

#[test]
fn lifecycle_archived_vs_superseded_quarantines() {
    let base = doc_with_status("active", "base body");
    let ours = doc_with_status("archived", "base body");
    let theirs = doc_with_status("superseded", "base body");
    let MergeResult::Quarantine(text) = merge(&base, &ours, &theirs) else {
        panic!("expected quarantine for archived vs superseded");
    };
    let parsed = parse_document(&text, None).expect("parse");
    assert_eq!(parsed.memory.frontmatter.status, MemoryStatus::Quarantined);
}

#[test]
fn lifecycle_tombstone_clears_superseded_by() {
    let base = doc_with_status("active", "base body");
    let theirs = doc_with_status("superseded", "base body");
    let ours = doc_with_status("tombstoned", "base body");
    let result = merge(&base, &ours, &theirs);
    let text = match result {
        MergeResult::Clean(text) | MergeResult::Quarantine(text) => text,
    };
    let parsed = parse_document(&text, None).expect("parse");
    assert_eq!(parsed.memory.frontmatter.status, MemoryStatus::Tombstoned);
    assert!(parsed.memory.frontmatter.superseded_by.is_empty(), "tombstone clears superseded_by per §14.5 #1");
}

#[test]
fn schema_version_gate_returns_typed_error_without_writing() {
    let base = doc_with_yaml_fields_explicit_schema(2, "base summary", "base body");
    let ours = doc("ours summary", "ours body");
    let theirs = doc("theirs summary", "theirs body");
    let err = merge_markdown(MergeInput { base: &base, ours: &ours, theirs: &theirs, path: "agent/patterns/m.md" })
        .expect_err("schema gate");
    assert!(matches!(err, MergeError::UnsupportedSchema { found: 2, supported: 1 }));
}

#[test]
fn secret_sensitivity_refuses() {
    let base = doc("base", "body");
    let secret_doc = doc_with_sensitivity("secret-flagged", "body", "secret");
    let theirs = doc("theirs", "body");
    let err =
        merge_markdown(MergeInput { base: &base, ours: &secret_doc, theirs: &theirs, path: "agent/patterns/x.md" })
            .expect_err("secret refused");
    assert!(matches!(err, MergeError::SecretSensitivityRefused { side: MergeSide::Ours }));
}

#[test]
fn confidence_delta_over_threshold_quarantines() {
    let base = doc_with_confidence("base", "base body", 0.5);
    let ours = doc_with_confidence("base", "base body", 0.9);
    let theirs = doc_with_confidence("base", "base body", 0.2);
    let MergeResult::Quarantine(text) = merge(&base, &ours, &theirs) else {
        panic!("expected quarantine for confidence delta > 0.25");
    };
    let parsed = parse_document(&text, None).expect("parse");
    let diag = first_diagnostic(&parsed.memory.frontmatter.merge_diagnostics);
    assert!(diag["conflicting_fields"].as_array().expect("array").iter().any(|v| v == "confidence"));
}

#[test]
fn updated_at_takes_max_created_at_takes_min() {
    let base = doc("base", "base body");
    let ours = base
        .replace("created_at: 2026-04-24T12:00:00Z", "created_at: 2026-04-23T12:00:00Z")
        .replace("base summary", "ours summary");
    let theirs = base
        .replace("created_at: 2026-04-24T12:00:00Z", "created_at: 2026-04-25T12:00:00Z")
        .replace("updated_at: 2026-04-24T12:00:00Z", "updated_at: 2026-04-26T12:00:00Z");
    let MergeResult::Clean(text) = merge(&base, &ours, &theirs) else {
        panic!("expected clean merge");
    };
    let parsed = parse_document(&text, None).expect("parse");
    assert_eq!(parsed.memory.frontmatter.created_at.to_rfc3339(), "2026-04-23T12:00:00+00:00");
    assert_eq!(parsed.memory.frontmatter.updated_at.to_rfc3339(), "2026-04-26T12:00:00+00:00");
}

#[test]
fn abstraction_conflict_uses_updated_at_then_side_independent_hash_and_preserves_loser() {
    let base = doc_with_extra_yaml("base", "body", "abstraction: Base concept\n");
    let ours = doc_with_extra_yaml("base", "body", "abstraction: OAuth policy\n");
    let theirs = doc_with_extra_yaml("base", "body", "abstraction: Token policy\n")
        .replace("updated_at: 2026-04-24T12:00:00Z", "updated_at: 2026-04-25T12:00:00Z");
    let MergeResult::Clean(text) = merge(&base, &ours, &theirs) else { panic!("clean merge") };
    let parsed = parse_document(&text, None).expect("parse");
    assert_eq!(parsed.memory.frontmatter.abstraction.as_deref(), Some("Token policy"));
    assert!(first_diagnostic(&parsed.memory.frontmatter.merge_diagnostics)["preserved_sources"]
        .as_array()
        .expect("preserved")
        .iter()
        .any(|entry| entry["field"] == "abstraction" && entry["loser_value"] == "OAuth policy"));

    let ours_equal_time = ours;
    let theirs_equal_time = doc_with_extra_yaml("base", "body", "abstraction: Token policy\n");
    let forward = merge(&base, &ours_equal_time, &theirs_equal_time);
    let reverse = merge(&base, &theirs_equal_time, &ours_equal_time);
    assert_eq!(merge_text(&forward), merge_text(&reverse));
    let forward = parse_document(merge_text(&forward), None).expect("forward parse");
    let reverse = parse_document(merge_text(&reverse), None).expect("reverse parse");
    assert_eq!(forward.memory.frontmatter.abstraction, reverse.memory.frontmatter.abstraction);
    for parsed in [forward, reverse] {
        assert!(first_diagnostic(&parsed.memory.frontmatter.merge_diagnostics)["preserved_sources"]
            .as_array()
            .expect("preserved")
            .iter()
            .any(|entry| entry["field"] == "abstraction" && entry["loser_value"] == "Token policy"));
    }
}

#[test]
fn cue_union_converges_with_overflow_and_casing_only_duplicates() {
    let base = doc("base", "body");
    let ours = doc_with_extra_yaml("base", "body", "cues:\n  - oauth\n  - Straße auth\n  - Zebra state\n");
    let theirs = doc_with_extra_yaml("base", "body", "cues:\n  - OAuth\n  - STRASSE auth\n  - Alpha state\n");
    let forward = merge(&base, &ours, &theirs);
    let reverse = merge(&base, &theirs, &ours);
    assert_eq!(merge_text(&forward), merge_text(&reverse));
    let parsed = parse_document(merge_text(&forward), None).expect("parse");
    assert_eq!(parsed.memory.frontmatter.cues, vec!["Alpha state", "OAuth", "STRASSE auth"]);
}

#[test]
fn swap_order_yields_identical_output_curated_fixtures() {
    // Smoke fixtures asserting commutativity of `(ours, theirs)` swaps.
    // The fuzz target covers the property at scale; this test ensures the
    // happy path holds for hand-rolled cases used in spec acceptance.
    let base = doc("base summary", "base body");
    let ours = doc_with_extra_yaml(
        "base summary",
        "base body",
        r#"evidence:
  -
    id: ev_002
    quote: "alpha"
    ref: file://a
"#,
    );
    let theirs = doc_with_extra_yaml(
        "base summary",
        "base body",
        r#"evidence:
  -
    id: ev_001
    quote: "beta"
    ref: file://b
"#,
    );
    let ab = merge(&base, &ours, &theirs);
    let ba = merge(&base, &theirs, &ours);
    let (ab_text, ba_text) = match (ab, ba) {
        (MergeResult::Clean(a), MergeResult::Clean(b)) => (a, b),
        _ => panic!("swap-order convergence requires both clean merges"),
    };
    let ab_parsed = parse_document(&ab_text, None).expect("parse ab");
    let ba_parsed = parse_document(&ba_text, None).expect("parse ba");
    let ab_ids: Vec<_> = ab_parsed.memory.frontmatter.evidence.iter().map(|e| &e.id).cloned().collect();
    let ba_ids: Vec<_> = ba_parsed.memory.frontmatter.evidence.iter().map(|e| &e.id).cloned().collect();
    assert_eq!(ab_ids, ba_ids, "evidence ordering must be commutative");
    assert_eq!(ab_ids, vec!["ev_001".to_string(), "ev_002".to_string()], "evidence sorted by id ascending");
}

proptest! {
    #![proptest_config(ProptestConfig { failure_persistence: None, .. ProptestConfig::default() })]

    #[test]
    fn merge_driver_fuzz_smoke_never_panics_and_outputs_valid_yaml(
        ours in "[a-zA-Z0-9 .,]{0,80}",
        theirs in "[a-zA-Z0-9 .,]{0,80}",
    ) {
        let base = doc("base summary", "base body");
        let ours_doc = doc("base summary", &ours);
        let theirs_doc = doc("base summary", &theirs);
        let result = merge_markdown(MergeInput { base: &base, ours: &ours_doc, theirs: &theirs_doc, path: "agent/patterns/m.md" });
        prop_assert!(result.is_ok());
        let markdown = match result.expect("merge result") {
            MergeResult::Clean(markdown) | MergeResult::Quarantine(markdown) => markdown,
        };
        prop_assert!(parse_document(&markdown, None).is_ok());
    }
}

fn merge(base: &str, ours: &str, theirs: &str) -> MergeResult {
    merge_markdown(MergeInput { base, ours, theirs, path: "agent/patterns/m.md" }).expect("merge")
}

fn merge_at(base: &str, ours: &str, theirs: &str) -> MergeResult {
    merge_markdown(MergeInput { base, ours, theirs, path: "agent/patterns/m.md" }).expect("merge")
}

fn merge_text(result: &MergeResult) -> &str {
    match result {
        MergeResult::Clean(text) | MergeResult::Quarantine(text) => text,
    }
}

fn first_diagnostic(slot: &Option<Value>) -> Value {
    let arr = slot.as_ref().expect("diagnostics present").as_array().expect("array");
    assert!(!arr.is_empty(), "diagnostics array non-empty");
    arr.last().expect("at least one diagnostic").clone()
}

fn doc(summary: &str, body: &str) -> String {
    doc_with_sensitivity(summary, body, "internal")
}

fn doc_with_id(id: &str, summary: &str, body: &str) -> String {
    doc_with_yaml(id, summary, body, "internal", "")
}

fn doc_with_sensitivity(summary: &str, body: &str, sensitivity: &str) -> String {
    doc_with_yaml("mem_20260424_a1b2c3d4e5f60718_000031", summary, body, sensitivity, "")
}

fn doc_with_extra_yaml(summary: &str, body: &str, extra_yaml: &str) -> String {
    doc_with_yaml("mem_20260424_a1b2c3d4e5f60718_000031", summary, body, "internal", extra_yaml)
}

fn doc_with_confidence(summary: &str, body: &str, confidence: f64) -> String {
    let raw = doc_with_yaml("mem_20260424_a1b2c3d4e5f60718_000031", summary, body, "internal", "");
    raw.replace("confidence: 1.0", &format!("confidence: {confidence}"))
}

#[allow(clippy::too_many_arguments)]
fn doc_with_yaml(id: &str, summary: &str, body: &str, sensitivity: &str, extra_yaml: &str) -> String {
    doc_with_yaml_fields(id, summary, body, sensitivity, "active", "trusted", extra_yaml)
}

fn doc_with_status(status: &str, body: &str) -> String {
    let (trust_level, extra_yaml) = match status {
        "candidate" => ("candidate", ""),
        "active" => ("trusted", ""),
        "pinned" => ("pinned", ""),
        "superseded" => ("trusted", "superseded_by:\n  - mem_20260424_a1b2c3d4e5f60718_999999\n"),
        "archived" => ("trusted", ""),
        "tombstoned" => (
            "trusted",
            r#"tombstone_events:
  -
    id: tomb_001
    applied_at: 2026-04-24T12:00:00Z
    actor:
      kind: user
      ref: trey
    reason: stale
    prior_status: active
"#,
        ),
        "quarantined" => (
            "quarantined",
            r#"_merge_diagnostics:
  - merge_id: merge_legacy
    created_at: 2026-04-24T12:00:00Z
    status: quarantined
    conflicting_fields:
      - status
    human_reason: pre-existing quarantine
"#,
        ),
        _ => ("trusted", ""),
    };
    doc_with_yaml_fields(
        "mem_20260424_a1b2c3d4e5f60718_000031",
        "base summary",
        body,
        "internal",
        status,
        trust_level,
        extra_yaml,
    )
}

#[allow(clippy::too_many_arguments)]
fn doc_with_yaml_fields(
    id: &str,
    summary: &str,
    body: &str,
    sensitivity: &str,
    status: &str,
    trust_level: &str,
    extra_yaml: &str,
) -> String {
    let indexable = if matches!(sensitivity, "confidential" | "personal") { "false" } else { "true" };
    format!(
        r#"---
schema_version: 1
id: {id}
type: pattern
scope: agent
summary: {summary}
confidence: 1.0
trust_level: {trust_level}
sensitivity: {sensitivity}
status: {status}
created_at: 2026-04-24T12:00:00Z
updated_at: 2026-04-24T12:00:00Z
author:
  kind: system
  user_handle: null
  harness: null
  harness_version: null
  session_id: null
  subagent_id: null
  phase: null
  component: test
retrieval_policy:
  index_body: {indexable}
  index_embeddings: {indexable}
  mask_personal_for_synthesis: false
  max_scope: agent
  passive_recall: true
{extra_yaml}---
{body}
"#
    )
}

fn doc_with_yaml_fields_explicit_schema(schema: u32, summary: &str, body: &str) -> String {
    let raw = doc_with_yaml_fields(
        "mem_20260424_a1b2c3d4e5f60718_000031",
        summary,
        body,
        "internal",
        "active",
        "trusted",
        "",
    );
    raw.replace("schema_version: 1", &format!("schema_version: {schema}"))
}

// Pull the base64 engine into scope for the add_add round-trip test.
use base64::Engine;
