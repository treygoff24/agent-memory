use memory_substrate::error::{ValidationError, ValidationWarning};
use memory_substrate::frontmatter::{parse_document, serialize_document};
use memory_substrate::merge::{merge_markdown, MergeInput, MergeResult};

#[test]
fn parses_missing_nullable_fields_with_typed_defaults_and_warnings() {
    let parsed = parse_document(minimal_doc(), None).expect("minimal document parses");
    assert!(parsed.warnings.contains(&ValidationWarning::AutoPopulatedNullableField { field: "tags".to_string() }));
    assert!(parsed.memory.frontmatter.tags.is_empty());
    assert!(parsed.memory.frontmatter.retrieval_policy.index_body);
}

#[test]
fn rejects_invalid_lifecycle_matrix_pair() {
    let err = parse_document(&minimal_doc().replace("trust_level: trusted", "trust_level: pinned"), None)
        .expect_err("active/pinned is invalid");
    assert!(matches!(err, ValidationError::InvalidLifecyclePair));
}

#[test]
fn serializes_canonical_key_order_byte_stably() {
    let parsed = parse_document(minimal_doc(), None).expect("minimal document parses");
    let first = serialize_document(&parsed.memory).expect("serialize");
    let second = serialize_document(&parse_document(&first, None).expect("reparse").memory).expect("serialize again");
    assert_eq!(first, second);
    assert!(first.starts_with("---\nschema_version:"));
}

#[test]
fn grounding_rehydration_required_defaults_false_and_round_trips() {
    let parsed = parse_document(minimal_doc(), None).expect("minimal document parses");
    assert!(!parsed.memory.frontmatter.grounding_rehydration_required());

    let serialized = serialize_document(&parsed.memory).expect("serialize");
    assert!(serialized.contains("grounding_rehydration_required: false"));

    let with_marker = minimal_doc().replace("author:\n", "grounding_rehydration_required: true\nauthor:\n");
    let reparsed = parse_document(&with_marker, None).expect("explicit marker parses");
    assert!(reparsed.memory.frontmatter.grounding_rehydration_required());

    let serialized = serialize_document(&reparsed.memory).expect("serialize explicit marker");
    assert!(serialized.contains("grounding_rehydration_required: true"));
    assert!(parse_document(&serialized, None)
        .expect("reparse explicit marker")
        .memory
        .frontmatter
        .grounding_rehydration_required());
}

#[test]
fn dream_authored_candidate_frontmatter_supports_grounding_marker() {
    let text = minimal_doc()
        .replace("status: active", "status: candidate")
        .replace("trust_level: trusted", "trust_level: candidate")
        .replace("author:\n", "grounding_rehydration_required: true\nauthor:\n")
        .replace("kind: system", "kind: dreaming")
        .replace("component: test", "component: null")
        .replace("phase: null", "phase: pass_2");

    let parsed = parse_document(&text, None).expect("dream-authored candidate parses");

    assert!(parsed.memory.frontmatter.grounding_rehydration_required());
}

#[test]
fn preserves_unknown_v1_extras() {
    let parsed = parse_document(&minimal_doc().replace("author:\n", "custom_future: yes\nauthor:\n"), None)
        .expect("parse with extra");
    assert!(parsed.memory.frontmatter.extras.contains_key("custom_future"));
    assert!(parsed.warnings.contains(&ValidationWarning::UnknownFieldPreserved { field: "custom_future".to_string() }));
}

#[test]
fn unknown_extras_round_trip_byte_stably() {
    // B-API-5: `Frontmatter::extras` must round-trip; unknown future fields
    // survive parse → serialize → parse without bleed across canonical keys.
    let with_extra = minimal_doc().replace("author:\n", "future_field: tomorrow\nauthor:\n");
    let parsed = parse_document(&with_extra, None).expect("parse with future field");
    assert_eq!(parsed.memory.frontmatter.extras.get("future_field").and_then(|v| v.as_str()), Some("tomorrow"));

    let first_serialized = serialize_document(&parsed.memory).expect("first serialize");
    let reparsed = parse_document(&first_serialized, None).expect("reparse round-trip");
    let second_serialized = serialize_document(&reparsed.memory).expect("second serialize");

    assert_eq!(first_serialized, second_serialized, "byte-stable round trip including extras");
    assert!(first_serialized.contains("future_field: tomorrow"), "extras re-emitted in canonical order");
}

#[test]
fn prospective_memory_with_time_event_and_conditional_triggers_validates() {
    let text = minimal_doc().replace(
        "type: pattern",
        r#"type: prospective
prospective:
  triggers:
    - kind: time
      at: 2026-04-25T09:00:00Z
    - kind: event
      name: meeting_started
    - kind: conditional
      expression: user_is_available"#,
    );

    let parsed = parse_document(&text, None).expect("prospective memory validates");

    assert_eq!(parsed.memory.frontmatter.extras["prospective"]["triggers"].as_array().expect("triggers").len(), 3);
}

#[test]
fn privacy_scan_private_credential_requires_quarantine() {
    let text = minimal_doc()
        .replace("author:\n", "privacy_scan:\n  labels:\n    - private_credential\n    - low_entropy\nauthor:\n");

    let err = parse_document(&text, None).expect_err("credential-bearing plaintext must be quarantined");

    assert!(matches!(err, ValidationError::BadShape(field) if field == "privacy_scan.private_credential"));
}

#[test]
fn tombstoned_memory_with_two_events_validates_and_round_trips() {
    let text = minimal_doc().replace(
        "status: active",
        r#"status: tombstoned
tombstone_events:
  - id: tomb_01HZX0YA0
    applied_at: 2026-04-24T12:10:00Z
    actor:
      kind: agent
      ref: claude-code
    reason: stale
    prior_status: active
  - id: tomb_01HZX0YA1
    applied_at: 2026-04-24T12:20:00Z
    actor:
      kind: user
      ref: trey
    reason: user-request
    prior_status: active"#,
    );

    let parsed = parse_document(&text, None).expect("tombstoned memory validates");
    let serialized = serialize_document(&parsed.memory).expect("serialize tombstone");
    let reparsed = parse_document(&serialized, None).expect("reparse tombstone");

    assert_eq!(reparsed.memory.frontmatter.tombstone_events.len(), 2);
}

#[test]
fn higher_schema_version_is_rejected_before_mutation() {
    let text = minimal_doc().replace("schema_version: 1", "schema_version: 2");

    let err = parse_document(&text, None).expect_err("higher schema is read-only to v1 parser");

    assert!(matches!(err, ValidationError::UnsupportedSchemaVersion { found: 2, supported: 1 }));
}

#[test]
fn merge_driver_quarantine_output_validates() {
    let base = minimal_doc();
    let ours = base.replace("Body text.", "Body text from ours.");
    let theirs = base.replace("Body text.", "Body text from theirs.");

    let merged = merge_markdown(MergeInput { base, ours: &ours, theirs: &theirs, path: "agent/patterns/test.md" })
        .expect("merge succeeds with semantic quarantine");
    let MergeResult::Quarantine(markdown) = merged else {
        panic!("body conflict should produce semantic quarantine");
    };
    let parsed = parse_document(&markdown, None).expect("quarantine output validates");

    assert_eq!(parsed.memory.frontmatter.review_state.as_deref(), Some("pending"));
}

#[test]
fn frontmatter_field_rule_matrix_accepts_and_rejects_known_shapes() {
    let positive = [
        minimal_doc().to_string(),
        minimal_doc().replace(
            "scope: agent",
            "scope: project\nnamespace: agent-memory\ncanonical_namespace_id: project:agent-memory",
        ),
    ];
    for text in positive {
        parse_document(&text, None).expect("positive matrix fixture validates");
    }

    let negative = [
        minimal_doc().replace("id: mem_20260424_a1b2c3d4e5f60718_000001", "id: bad"),
        minimal_doc().replace("summary: A useful pattern", "summary: "),
        minimal_doc().replace("confidence: 1.0", "confidence: 1.5"),
        minimal_doc().replace("updated_at: 2026-04-24T12:00:00Z", "updated_at: 2026-04-23T12:00:00Z"),
        minimal_doc().replace("scope: agent", "scope: project"),
        minimal_doc().replace("component: test", "component: null"),
        minimal_doc().replace(
            "sensitivity: internal",
            "sensitivity: confidential\nretrieval_policy:\n  passive_recall: true\n  max_scope: agent\n  mask_personal_for_synthesis: false\n  index_body: true\n  index_embeddings: true",
        ),
    ];
    for text in negative {
        parse_document(&text, None).expect_err("negative matrix fixture is rejected");
    }
}

#[test]
fn rejects_superseded_without_superseded_by() {
    let err = parse_document(&minimal_doc().replace("status: active", "status: superseded"), None)
        .expect_err("superseded requires superseded_by");
    assert!(matches!(err, ValidationError::BadShape(field) if field == "superseded_by/status"));
}

#[test]
fn rejects_quarantined_without_merge_diagnostics() {
    let text = minimal_doc()
        .replace("trust_level: trusted", "trust_level: quarantined")
        .replace("status: active", "status: quarantined");
    let err = parse_document(&text, None).expect_err("quarantine requires diagnostics");
    assert!(matches!(err, ValidationError::BadShape(field) if field == "_merge_diagnostics"));
}

/// Regression: write-note text containing ": " (the YAML mapping indicator)
/// must round-trip through `serialize_document` and back. Before the fix in
/// `serialize.rs::plain_yaml_string`, the serializer emitted such summaries
/// unquoted, producing `summary: Useful: memoryd doctor ...` which the
/// substrate's reindex phase then rejected as "mapping values are not allowed
/// in this context", forcing the daemon into operator-repair-required state.
#[test]
fn summary_with_colon_space_round_trips_through_serialize_and_parse() {
    let trapped_summary = "Useful: memoryd doctor --reindex rebuilds the SQLite events_log mirror from JSONL";
    let trailing_colon = "Followup TBD:";
    let leading_dash = "- listed item";

    for summary in [trapped_summary, trailing_colon, leading_dash] {
        let text = minimal_doc().replace("summary: A useful pattern", &format!("summary: {summary:?}"));
        let parsed = parse_document(&text, None).unwrap_or_else(|err| panic!("parse {summary:?}: {err:?}"));
        assert_eq!(parsed.memory.frontmatter.summary, summary);

        let serialized = serialize_document(&parsed.memory).expect("serialize");
        assert!(
            !serialized.contains(&format!("summary: {summary}\n")),
            "summary {summary:?} must be quoted in serialized output:\n{serialized}"
        );
        // Re-parse to confirm the round-trip preserved the value.
        let reparsed = parse_document(&serialized, None).expect("reparse");
        assert_eq!(reparsed.memory.frontmatter.summary, summary);
    }
}

#[test]
fn rejects_supersedes_and_superseded_by_overlap() {
    let id = "mem_20260424_a1b2c3d4e5f60718_000099";
    let text = minimal_doc()
        .replace("status: active", "status: superseded")
        .replace("---\nBody text.", &format!("supersedes: [{id}]\nsuperseded_by: [{id}]\n---\nBody text."));
    let err = parse_document(&text, None).expect_err("overlap rejected");
    assert!(matches!(err, ValidationError::BadShape(field) if field == "supersession overlap"));
}

fn minimal_doc() -> &'static str {
    r#"---
schema_version: 1
id: mem_20260424_a1b2c3d4e5f60718_000001
type: pattern
scope: agent
summary: A useful pattern
confidence: 1.0
trust_level: trusted
sensitivity: internal
status: active
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
---
Body text.
"#
}
