use memoryd::recall::{
    bounded_omissions, estimated_tokens, render_memory_entry, render_startup_frame, truncate_utf8_bytes,
    OmissionReason, ProjectBinding, ProjectBindingSource, RecallEntry, RecallError, RecallExplanation, RecallOmission,
    RecallSectionName, SessionBinding, STREAM_E_POLICY,
};
use serde_json::{json, Value};

#[test]
fn estimated_tokens_uses_exact_ceil_utf8_bytes_over_four() {
    assert_eq!(estimated_tokens(""), 0);
    assert_eq!(estimated_tokens("abcd"), 1);
    assert_eq!(estimated_tokens("abcde"), 2);
    assert_eq!(estimated_tokens("é🙂"), 2);
}

#[test]
fn truncate_utf8_bytes_preserves_character_boundaries_and_marks_only_truncation() {
    let value = "東京🙂abc";

    let truncated = truncate_utf8_bytes(value, 10);
    assert_eq!(truncated.value, "東京…");
    assert!(truncated.truncated);

    let unchanged = truncate_utf8_bytes(value, value.len());
    assert_eq!(unchanged.value, value);
    assert!(!unchanged.truncated);
    assert!(!unchanged.value.contains('…'));
}

#[test]
fn rendered_entry_truncates_summary_and_snippet_inside_xml_fields() {
    let entry = RecallEntry {
        id: "mem_1".to_owned(),
        summary: "a".repeat(241),
        snippet: Some("b".repeat(361)),
        updated: "2026-04-30".to_owned(),
        source_kind: "agent_primary".to_owned(),
        confidence: "0.93".to_owned(),
    };

    let rendered = render_memory_entry(&entry);

    assert!(rendered.contains(&format!("<summary>{}…</summary>", "a".repeat(237))));
    assert!(rendered.contains(&format!("<snippet>{}…</snippet>", "b".repeat(357))));
    assert!(rendered.contains("updated=\"2026-04-30\""));
    assert!(rendered.contains("source=\"agent_primary\""));
    assert!(rendered.contains("confidence=\"0.93\""));
    assert!(rendered.ends_with("</memory>"));
}

#[test]
fn empty_startup_frame_contains_required_sections_in_spec_order() {
    assert_eq!(STREAM_E_POLICY, "stream-e-v0.5");

    let binding = session_binding();
    let explanation = RecallExplanation::empty(3600);

    let frame = render_startup_frame(&binding, &explanation, &[]);

    assert_in_order(
        &frame,
        &[
            "<memory-recall version=\"stream-e-v0.5\" harness=\"codex\" session=\"sess_abc123\">",
            "<identity>",
            "</identity>",
            "<project-state project=\"agent-memory\" resolved-via=\"git_remote\">",
            "</project-state>",
            "<entity-recall entities=\"\">",
            "</entity-recall>",
            "<recent-memory>",
            "</recent-memory>",
            "<pending-attention>",
            "</pending-attention>",
            "<recall-explanation policy=\"stream-e-v0.5\" budget-tokens=\"3600\" used-tokens=\"0\">",
            "</recall-explanation>",
            "</memory-recall>",
        ],
    );
}

#[test]
fn project_binding_serde_and_rendering_match_stream_e_contract() {
    let yaml_override = ProjectBinding {
        canonical_id: "proj_canonical".to_owned(),
        alias: Some("project alias".to_owned()),
        concurrent_session_mode: None,
        resolved_via: ProjectBindingSource::YamlOverride,
    };

    let encoded = serde_json::to_value(&yaml_override).expect("project binding serializes");
    assert_eq!(encoded["resolved_via"], "yaml_override");
    assert_eq!(encoded["alias"], "project alias");

    let missing_alias: ProjectBinding = serde_json::from_value(json!({
        "canonical_id": "proj_without_alias",
        "resolved_via": "git_remote"
    }))
    .expect("project binding with absent alias deserializes");
    assert_eq!(missing_alias.alias, None);

    let null_alias: ProjectBinding = serde_json::from_value(json!({
        "canonical_id": "proj_null_alias",
        "alias": null,
        "resolved_via": "yaml_override"
    }))
    .expect("project binding with null alias deserializes");
    assert_eq!(null_alias.alias, None);

    let mut binding = session_binding();
    binding.project = Some(missing_alias);
    let frame = render_startup_frame(&binding, &RecallExplanation::empty(3600), &[]);

    assert!(frame.contains("<project-state project=\"proj_without_alias\" resolved-via=\"git_remote\">"));
}

#[test]
fn rendering_is_byte_identical_and_uses_stable_xml_escaping() {
    let mut binding = session_binding();
    binding.session_id = "sess<&\"'".to_owned();
    binding.harness = "codex<&\"'".to_owned();
    binding.project.as_mut().expect("project binding exists").alias = Some("agent<memory&>".to_owned());
    let explanation = RecallExplanation::empty(3600);

    let first = render_startup_frame(&binding, &explanation, &[]);
    let second = render_startup_frame(&binding, &explanation, &[]);

    assert_eq!(first.as_bytes(), second.as_bytes());
    assert!(first.contains("harness=\"codex&lt;&amp;&quot;&apos;\""));
    assert!(first.contains("session=\"sess&lt;&amp;&quot;&apos;\""));
    assert!(first.contains("project=\"agent&lt;memory&amp;&gt;\""));

    let entry = render_memory_entry(&RecallEntry {
        id: "mem<&>".to_owned(),
        summary: "use <xml> & plain text".to_owned(),
        snippet: None,
        updated: "2026-04-30".to_owned(),
        source_kind: "agent&tool".to_owned(),
        confidence: "1.00".to_owned(),
    });
    assert_eq!(
        entry,
        "<memory ref=\"mem&lt;&amp;&gt;\" updated=\"2026-04-30\" source=\"agent&amp;tool\" confidence=\"1.00\">\n  <summary>use &lt;xml&gt; &amp; plain text</summary>\n  <snippet></snippet>\n</memory>"
    );
}

#[test]
fn omissions_truncate_to_sixty_four_and_record_dropped_count() {
    let omissions = (0..70)
        .map(|index| RecallOmission {
            id: Some(format!("mem_{index:03}")),
            section: RecallSectionName::RecentMemory,
            reason: OmissionReason::BudgetExhausted,
            alias: None,
            colliding_ids: Vec::new(),
        })
        .collect();

    let bounded = bounded_omissions(omissions);

    assert_eq!(bounded.omitted.len(), 64);
    assert_eq!(bounded.omitted_truncated_count, 6);
    assert_eq!(bounded.omitted.first().and_then(|omission| omission.id.as_deref()), Some("mem_000"));
    assert_eq!(bounded.omitted.last().and_then(|omission| omission.id.as_deref()), Some("mem_063"));
}

#[test]
fn recall_omission_alias_collision_roundtrips_and_budget_omission_skips_default_fields() {
    let collision = RecallOmission {
        id: None,
        section: RecallSectionName::EntityRecall,
        reason: OmissionReason::AmbiguousAlias,
        alias: Some("foo".to_owned()),
        colliding_ids: vec!["a".to_owned(), "b".to_owned()],
    };

    let encoded = serde_json::to_value(&collision).expect("collision omission serializes");
    assert_eq!(encoded["alias"], "foo");
    assert_eq!(encoded["colliding_ids"], json!(["a", "b"]));
    let decoded: RecallOmission = serde_json::from_value(encoded).expect("collision omission deserializes");
    assert_eq!(decoded, collision);

    let budget = RecallOmission {
        id: Some("mem_budget".to_owned()),
        section: RecallSectionName::RecentMemory,
        reason: OmissionReason::BudgetExhausted,
        alias: None,
        colliding_ids: Vec::new(),
    };
    let encoded = serde_json::to_value(&budget).expect("budget omission serializes");
    assert_absent(&encoded, "alias");
    assert_absent(&encoded, "colliding_ids");

    let legacy_shape: RecallOmission = serde_json::from_value(json!({
        "id": "mem_legacy",
        "section": "identity",
        "reason": "budget_exhausted"
    }))
    .expect("legacy omission shape deserializes");
    assert_eq!(legacy_shape.alias, None);
    assert!(legacy_shape.colliding_ids.is_empty());
}

#[test]
fn omissions_sort_by_section_reason_alias_then_id() {
    let bounded = bounded_omissions(vec![
        omission(Some("mem_b"), RecallSectionName::RecentMemory, OmissionReason::BudgetExhausted, None),
        omission(None, RecallSectionName::EntityRecall, OmissionReason::AmbiguousAlias, Some("zeta")),
        omission(Some("mem_a"), RecallSectionName::RecentMemory, OmissionReason::BudgetExhausted, None),
        omission(None, RecallSectionName::EntityRecall, OmissionReason::AmbiguousAlias, Some("alpha")),
        omission(Some("mem_identity"), RecallSectionName::Identity, OmissionReason::ReviewPending, None),
    ]);

    let sort_projection = bounded
        .omitted
        .iter()
        .map(|omission| {
            (
                omission.section.as_str(),
                omission.reason.as_str(),
                omission.alias.as_deref().unwrap_or(""),
                omission.id.as_deref().unwrap_or(""),
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(
        sort_projection,
        vec![
            ("entity-recall", "ambiguous_alias", "alpha", ""),
            ("entity-recall", "ambiguous_alias", "zeta", ""),
            ("identity", "review_pending", "", "mem_identity"),
            ("recent-memory", "budget_exhausted", "", "mem_a"),
            ("recent-memory", "budget_exhausted", "", "mem_b"),
        ]
    );
}

#[test]
fn recall_errors_map_to_protocol_retryability_and_cli_exit_codes() {
    let cases = [
        (RecallError::invalid_request("bad cwd"), "invalid_request", false, 1),
        (RecallError::substrate_error("index busy"), "substrate_error", true, 2),
        (RecallError::recall_unavailable("repair pending"), "recall_unavailable", true, 2),
        (RecallError::privacy_error("unsafe metadata"), "privacy_error", false, 3),
        (RecallError::not_implemented("event deltas"), "not_implemented", false, 4),
    ];

    for (error, code, retryable, exit_code) in cases {
        assert_eq!(error.protocol_code(), code);
        assert_eq!(error.retryable(), retryable);
        assert_eq!(error.exit_code(), exit_code);
        assert!(error.to_string().starts_with(code));
    }
}

fn session_binding() -> SessionBinding {
    SessionBinding {
        session_id: "sess_abc123".to_owned(),
        harness: "codex".to_owned(),
        harness_version: Some("0.0.0".to_owned()),
        cwd: "/Users/treygoff/Code/agent-memory".to_owned(),
        project: Some(ProjectBinding {
            canonical_id: "proj_test".to_owned(),
            alias: Some("agent-memory".to_owned()),
            concurrent_session_mode: None,
            resolved_via: ProjectBindingSource::GitRemote,
        }),
        namespaces_in_scope: vec!["me".to_owned(), "project:proj_test".to_owned(), "agent".to_owned()],
    }
}

fn omission(
    id: Option<&str>,
    section: RecallSectionName,
    reason: OmissionReason,
    alias: Option<&str>,
) -> RecallOmission {
    RecallOmission {
        id: id.map(str::to_owned),
        section,
        reason,
        alias: alias.map(str::to_owned),
        colliding_ids: Vec::new(),
    }
}

fn assert_in_order(haystack: &str, needles: &[&str]) {
    let mut cursor = 0;
    for needle in needles {
        let offset = haystack[cursor..].find(needle).unwrap_or_else(|| panic!("missing ordered fragment: {needle}"));
        cursor += offset + needle.len();
    }
}

fn assert_absent(value: &Value, key: &str) {
    assert!(value.get(key).is_none(), "expected key {key:?} to be absent in {value}");
}
