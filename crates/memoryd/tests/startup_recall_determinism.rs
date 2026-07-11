mod common;

use chrono::{DateTime, Utc};
use common::trust_for_status;
use memory_substrate::events::{append_event, Event, EventKind, EVENT_SCHEMA_VERSION};
use memory_substrate::{
    Author, AuthorKind, ClassificationOutcome, DeviceId, EventContext, EventId, Frontmatter, InitOptions, Memory,
    MemoryId, MemoryStatus, MemoryType, OperationId, RepoPath, RetrievalPolicy, Roots, Scope, Sensitivity, Source,
    SourceKind, Substrate, WriteMode, WritePolicy, WriteRequest,
};
use memoryd::recall::{
    bounded_omissions, build_startup_response, estimated_tokens, render_memory_entry, render_startup_frame,
    truncate_utf8_bytes, OmissionReason, ProjectBinding, ProjectBindingSource, RecallEntry, RecallError,
    RecallExplanation, RecallOmission, RecallSectionName, SessionBinding, StartupRequest, STREAM_E_POLICY,
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
    assert_eq!(STREAM_E_POLICY, "stream-e-v0.7");

    let binding = session_binding();
    let explanation = RecallExplanation::empty(3600);

    let frame = render_startup_frame(&binding, &explanation, &[]);

    assert_in_order(
        &frame,
        &[
            "<memory-recall version=\"stream-e-v0.7\" harness=\"codex\" session=\"sess_abc123\">",
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
            "<recall-explanation policy=\"stream-e-v0.7\" budget-tokens=\"3600\" used-tokens=\"0\">",
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
        "<memory ref=\"mem&lt;&amp;&gt;\" updated=\"2026-04-30\" source=\"agent&amp;tool\" confidence=\"1.00\">\n  <summary>use &lt;xml&gt; &amp; plain text</summary>\n</memory>"
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

/// memory-dynamics-v0.1 §9 #2c: with dynamics off, a non-empty startup block with
/// pre-existing recall usage stays byte-pinned to the committed pre-dynamics
/// golden except for the stream policy token. This is intentionally an
/// integration test: it builds the block through the production startup path and
/// seeds recall events such that strengths would be non-zero if dynamics were on.
#[tokio::test]
async fn dynamics_off_block_is_byte_identical_to_pre_dynamics_golden() {
    let fixture = StartupDynamicsFixture::new("dev_dynamicsoff").await;
    fixture.write_dynamics_config(false);
    fixture
        .write_memory(fixture.memory(StartupMemorySpec {
            id: "mem_20260501_aaaaaaaaaaaaaaaa_000001",
            summary: "Pinned operational invariant remains in off mode.",
            status: MemoryStatus::Pinned,
            scope: Scope::User,
            path: "me/mem_20260501_aaaaaaaaaaaaaaaa_000001.md",
        }))
        .await;
    fixture
        .write_memory(fixture.memory(StartupMemorySpec {
            id: "mem_20260501_bbbbbbbbbbbbbbbb_000002",
            summary: "Frequently recalled agent note is still structurally rendered.",
            status: MemoryStatus::Active,
            scope: Scope::Agent,
            path: "agent/patterns/mem_20260501_bbbbbbbbbbbbbbbb_000002.md",
        }))
        .await;
    fixture
        .write_memory(fixture.memory(StartupMemorySpec {
            id: "mem_20260501_cccccccccccccccc_000003",
            summary: "Lightly recalled user note keeps the off-state fixture non-empty.",
            status: MemoryStatus::Active,
            scope: Scope::User,
            path: "me/mem_20260501_cccccccccccccccc_000003.md",
        }))
        .await;
    fixture.append_recall_hits("mem_20260501_bbbbbbbbbbbbbbbb_000002", 3);
    fixture.append_recall_hits("mem_20260501_cccccccccccccccc_000003", 1);
    fixture.reindex_events();

    let response = fixture.startup().await;
    let selected_recent = response.recall_explanation.sections[3].selected_ids.clone();
    assert_eq!(
        selected_recent,
        vec![
            "mem_20260501_aaaaaaaaaaaaaaaa_000001",
            "mem_20260501_cccccccccccccccc_000003",
            "mem_20260501_bbbbbbbbbbbbbbbb_000002",
        ],
        "fixture must render a non-empty recent-memory block"
    );

    let masked_current = mask_startup_frame(&response.recall_block, fixture.repo_path());
    if std::env::var_os("MEMORYD_UPDATE_DYNAMICS_OFF_GOLDEN").is_some() {
        std::fs::write(golden_path(), &masked_current).expect("update dynamics-off golden");
    }
    let golden = include_str!("fixtures/startup_dynamics_off_golden.xml");
    assert_eq!(
        masked_current.as_bytes(),
        golden.as_bytes(),
        "dynamics-off block differs from committed pre-dynamics golden by more than the masked policy token"
    );
    assert_no_dynamics_markers(&response.recall_block);
    assert!(response.recall_explanation.strengths.is_empty(), "dynamics-off explanation must not carry strengths");
    assert!(!response.recall_explanation.dynamics_degraded);
    let explanation_json = serde_json::to_string(&response.recall_explanation).expect("explanation serializes");
    assert_no_dynamics_markers(&explanation_json);

    fixture.write_dynamics_config(true);
    let dynamics_on = fixture.startup().await;
    assert!(
        dynamics_on.recall_explanation.strengths.iter().any(|entry| entry.strength > 0.0),
        "same fixture should hydrate non-zero strengths when dynamics is enabled"
    );
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

fn mask_startup_frame(frame: &str, repo: &std::path::Path) -> String {
    let mut masked = frame.replace(STREAM_E_POLICY, "STREAM_E_POLICY");
    if let Ok(canonical) = std::fs::canonicalize(repo) {
        masked = masked.replace(&canonical.to_string_lossy().into_owned(), "TEST_REPO");
    }
    masked = masked.replace(&repo.to_string_lossy().into_owned(), "TEST_REPO");
    normalize_masked_used_tokens(&masked)
}

fn normalize_masked_used_tokens(frame: &str) -> String {
    let mut normalized = frame.to_owned();
    for _ in 0..4 {
        let next = replace_used_tokens(&normalized, estimated_tokens(&normalized));
        if next == normalized {
            return normalized;
        }
        normalized = next;
    }
    replace_used_tokens(&normalized, estimated_tokens(&normalized))
}

fn replace_used_tokens(frame: &str, tokens: usize) -> String {
    let Some(attribute_start) = frame.find(r#" used-tokens=""#) else {
        return frame.to_owned();
    };
    let value_start = attribute_start + r#" used-tokens=""#.len();
    let Some(value_len) = frame[value_start..].find('"') else {
        return frame.to_owned();
    };
    let value_end = value_start + value_len;

    let mut updated = String::with_capacity(frame.len());
    updated.push_str(&frame[..value_start]);
    updated.push_str(&tokens.to_string());
    updated.push_str(&frame[value_end..]);
    updated
}

fn assert_no_dynamics_markers(text: &str) {
    assert!(!text.contains("strength"), "unexpected strength marker in {text}");
    assert!(!text.contains("dynamics"), "unexpected dynamics marker in {text}");
}

fn golden_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/startup_dynamics_off_golden.xml")
}

struct StartupDynamicsFixture {
    _temp: tempfile::TempDir,
    roots: Roots,
    substrate: Substrate,
    device_id: String,
}

impl StartupDynamicsFixture {
    async fn new(device_id: &str) -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
        let substrate = Substrate::init(
            roots.clone(),
            InitOptions { force_unsafe_durability: true, device_id: Some(device_id.to_owned()) },
        )
        .await
        .expect("substrate init");
        Self { _temp: temp, roots, substrate, device_id: device_id.to_owned() }
    }

    fn write_dynamics_config(&self, enabled: bool) {
        let body = format!(
            "schema_version: 1\ndynamics:\n  enabled: {enabled}\n  alpha_points: 12\n  weights:\n    frequency: 1.0\n    recency: 0.0\n    corroboration: 0.0\n"
        );
        std::fs::write(self.roots.repo.join("config.yaml"), body).expect("write dynamics config");
    }

    async fn startup(&self) -> memoryd::recall::StartupResponse {
        build_startup_response(
            &self.substrate,
            StartupRequest {
                cwd: self.roots.repo.to_string_lossy().into_owned(),
                session_id: "sess_off_pin".to_owned(),
                harness: "codex".to_owned(),
                harness_version: None,
                include_recent: true,
                since_event_id: None,
                budget_tokens: Some(1024),
                passive: false,
            },
        )
        .await
        .expect("startup recall")
    }

    async fn write_memory(&self, memory: Memory) {
        self.substrate
            .write_memory(WriteRequest {
                operation_id: None,
                memory,
                expected_base_hash: None,
                write_mode: WriteMode::CreateNew,
                index_projection: None,
                event_context: EventContext::default(),
                allow_best_effort_durability: true,
                classification: ClassificationOutcome::Trusted,
            })
            .await
            .expect("write memory");
    }

    fn append_recall_hits(&self, memory_id: &str, count: u64) {
        let path = self.roots.repo.join(format!("events/{}.jsonl", self.device_id));
        for seq in 1..=count {
            append_event(
                &path,
                &recall_event(RecallEventSpec {
                    event_id: &format!("evt_{}_{}", memory_id, seq),
                    device_id: &self.device_id,
                    seq,
                    memory_id,
                    timestamp: instant(&format!("2026-05-01T12:00:0{seq}Z")),
                }),
            )
            .expect("append recall hit");
        }
    }

    fn reindex_events(&self) {
        self.substrate.doctor_reindex_events_log().expect("reindex events");
    }

    fn repo_path(&self) -> &std::path::Path {
        self.roots.repo.as_path()
    }

    fn memory(&self, spec: StartupMemorySpec<'_>) -> Memory {
        let updated_at = instant("2026-05-01T12:00:00Z");
        Memory {
            frontmatter: Frontmatter {
                schema_version: 1,
                id: MemoryId::new(spec.id),
                memory_type: MemoryType::Pattern,
                scope: spec.scope,
                summary: spec.summary.to_owned(),
                confidence: 0.8,
                original_confidence: None,
                trust_level: trust_for_status(spec.status),
                sensitivity: Sensitivity::Internal,
                status: spec.status,
                created_at: updated_at,
                updated_at,
                observed_at: None,
                author: Author {
                    kind: AuthorKind::Agent,
                    user_handle: None,
                    harness: Some("codex".to_owned()),
                    harness_version: None,
                    session_id: Some("sess_off_pin".to_owned()),
                    subagent_id: None,
                    phase: None,
                    component: Some("startup-dynamics-off-test".to_owned()),
                },
                namespace: None,
                canonical_namespace_id: None,
                tags: Vec::new(),
                entities: Vec::new(),
                aliases: Vec::new(),
                source: Source {
                    kind: SourceKind::AgentPrimary,
                    reference: None,
                    harness: Some("codex".to_owned()),
                    harness_version: None,
                    session_id: Some("sess_off_pin".to_owned()),
                    subagent_id: None,
                    device: Some(self.device_id.clone()),
                },
                evidence: Vec::new(),
                requires_user_confirmation: false,
                review_state: None,
                supersedes: Vec::new(),
                superseded_by: Vec::new(),
                related: Vec::new(),
                tombstone_events: Vec::new(),
                retrieval_policy: RetrievalPolicy {
                    passive_recall: true,
                    max_scope: spec.scope,
                    mask_personal_for_synthesis: false,
                    index_body: true,
                    index_embeddings: false,
                },
                write_policy: WritePolicy {
                    human_review_required: false,
                    policy_applied: "startup-dynamics-off-test".to_owned(),
                    expected_base_hash: None,
                },
                merge_diagnostics: None,
                abstraction: None,
                cues: Vec::new(),
                extras: Default::default(),
            },
            body: spec.summary.to_owned(),
            path: Some(RepoPath::new(spec.path)),
        }
    }
}

struct StartupMemorySpec<'a> {
    id: &'a str,
    summary: &'a str,
    status: MemoryStatus,
    scope: Scope,
    path: &'a str,
}

struct RecallEventSpec<'a> {
    event_id: &'a str,
    device_id: &'a str,
    seq: u64,
    memory_id: &'a str,
    timestamp: DateTime<Utc>,
}

fn recall_event(spec: RecallEventSpec<'_>) -> Event {
    Event {
        schema: EVENT_SCHEMA_VERSION,
        id: EventId::new(spec.event_id),
        at: spec.timestamp,
        device: DeviceId::new(spec.device_id),
        seq: spec.seq,
        operation_id: Some(OperationId::new(format!("op_{}", spec.event_id))),
        kind: EventKind::RecallHit { id: MemoryId::new(spec.memory_id), recalled_at: spec.timestamp },
        crc32c: 0,
    }
}

fn instant(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value).expect("fixture timestamp parses").with_timezone(&Utc)
}
