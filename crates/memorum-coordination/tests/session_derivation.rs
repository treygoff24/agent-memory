use chrono::{DateTime, TimeZone, Utc};
use memorum_coordination::gate::{path_fraction, PeerWriteCandidate};
use memorum_coordination::session::StartupRecallEntityInput;
use memorum_coordination::{ConcurrentSessionMode, CoordinationConfig, ProjectBinding, RelevanceGate, SessionContext};
use memory_substrate::{Entity, MemoryId, MemoryStatus, RecallIndexRow, RepoPath, Scope, Sensitivity, SourceKind};
use std::collections::HashSet;

#[test]
fn test_salient_entities_from_startup_recall() {
    let recall = mock_recall_explanation_with_entity_sections();
    let session = SessionContext::from_startup_recall(
        "sess_tier1",
        "codex",
        StartupRecallEntityInput {
            recall_block: &recall,
            last_three_turn_fts5_entity_ids: &["ent_from_fts5", " ent_beta ", ""],
        },
    );

    assert_eq!(session.salient_entities, set(["ent_alpha", "ent_beta", "ent_gamma", "ent_from_fts5"]));
    assert!(session.recent_query_embedding.is_none());
    assert!(session.is_full_coordination_harness());
}

#[test]
fn test_salient_entities_tier3_from_binding_only() {
    let session = SessionContext::from_tier3_binding("sess_tier3", "cursor", project_binding());

    assert_eq!(session.salient_entities, set(["proj_abc", "my-project", "code"]));
    assert!(session.salient_paths.is_empty());
    assert!(session.recent_query_embedding.is_none());
    assert!(session.is_observe_only_harness());
}

#[test]
fn known_full_coordination_harness_names_are_allowlisted() {
    for harness in ["codex", "codex-cli", "claude-code", " CODEX "] {
        let session = SessionContext::from_startup_recall(
            "sess_full",
            harness,
            StartupRecallEntityInput { recall_block: "", last_three_turn_fts5_entity_ids: &[] },
        );

        assert!(session.is_full_coordination_harness(), "{harness} should be full coordination");
        assert!(!session.is_observe_only_harness(), "{harness} should not be observe-only");
    }
}

#[test]
fn unknown_harness_names_default_to_observe_only() {
    for harness in ["cursor", "claude-code-v2", "opencode"] {
        let session = SessionContext::from_startup_recall(
            "sess_observe",
            harness,
            StartupRecallEntityInput { recall_block: "", last_three_turn_fts5_entity_ids: &[] },
        );

        assert!(!session.is_full_coordination_harness(), "{harness} must not silently gain coordination");
        assert!(session.is_observe_only_harness(), "{harness} should default observe-only");
    }
}

#[test]
fn test_relevance_gate_skipped_for_tier3() {
    let now = fixture_now();
    let candidate = candidate("mem_20260501_a1b2c3d4e5f60718_000101", ["proj_abc"], ["project:proj/shared.md"], now);
    let mut tier1_session = SessionContext::from_startup_recall(
        "sess_tier1",
        "codex",
        StartupRecallEntityInput {
            recall_block: r#"<entity-recall entities="proj_abc">"#,
            last_three_turn_fts5_entity_ids: &[],
        },
    );
    tier1_session.salient_paths = set(["project:proj/shared.md"]);
    let mut tier3_session = SessionContext::from_tier3_binding("sess_tier3", "cursor", project_binding());
    tier3_session.salient_paths = set(["project:proj/shared.md"]);
    let gate = RelevanceGate::new(CoordinationConfig::default());

    assert_eq!(gate.evaluate(&mut tier1_session, std::slice::from_ref(&candidate), now).peer_updates.len(), 1);
    assert!(gate.evaluate(&mut tier3_session, &[candidate], now).peer_updates.is_empty());
}

#[test]
fn test_salient_paths_from_selected_ids() {
    let recall = mock_startup_recall_with_refs();
    let mut session = SessionContext::from_startup_recall(
        "sess_tier1",
        "codex",
        StartupRecallEntityInput { recall_block: &recall, last_three_turn_fts5_entity_ids: &[] },
    );

    session.add_session_paths(["project:proj/tool-session.md", "project:proj/state/from-project.md", ""]);

    assert_eq!(
        session.salient_paths,
        set([
            "project:proj/memories/from-entity.md",
            "project:proj/state/from-project.md",
            "project:proj/tool-session.md",
        ])
    );
}

#[test]
fn test_salient_paths_tier3_from_mcp_startup_paths() {
    let mut session = SessionContext::from_tier3_binding("sess_tier3", "cursor", project_binding());

    session.populate_salient_paths_from_recall(&mock_mcp_startup_response_with_refs());
    session.add_session_paths(["project:proj/tool-session-ignored.md"]);

    assert_eq!(
        session.salient_paths,
        set(["project:proj/memories/from-entity.md", "project:proj/state/from-project.md",])
    );
}

#[test]
fn test_salient_paths_tier3_no_startup_empty() {
    let session = SessionContext::from_tier3_binding("sess_tier3", "cursor", project_binding());

    assert!(session.salient_paths.is_empty());
}

#[test]
fn test_path_matching_component_boundary_semantics() {
    // Trailing slash is a formatting artifact, not a distinct path: same
    // components ⇒ intersects (paths_intersect upgrade, 2026-06-09).
    let session_paths = set(["project:proj/decision.md/"]);
    let candidate_paths = vec!["project:proj/decision.md".to_string()];
    assert_eq!(path_fraction(&candidate_paths, &session_paths), 1.0);

    // Component boundaries still protect against lookalike prefixes.
    let session_paths = set(["project:proj/decision.md"]);
    let candidate_paths = vec!["project:proj/decision.md.bak".to_string()];
    assert_eq!(path_fraction(&candidate_paths, &session_paths), 0.0);
}

#[test]
fn test_ref_attribute_parser_ignores_non_ref_attribute_names() {
    let recall = [
        r#"<memory-recall version="stream-e-v0.5">"#,
        r#"  <entity-recall entities="ent_alpha">"#,
        r#"    <memory data-ref="project:proj/not-ref.md" xref="project:proj/also-not-ref.md" />"#,
        r#"    <memory ref="project:proj/is-ref.md" />"#,
        r#"  </entity-recall>"#,
        r#"</memory-recall>"#,
    ]
    .join("\n");
    let session = SessionContext::from_startup_recall(
        "sess_tier1",
        "codex",
        StartupRecallEntityInput { recall_block: &recall, last_three_turn_fts5_entity_ids: &[] },
    );

    assert_eq!(session.salient_paths, set(["project:proj/is-ref.md"]));
}

#[test]
fn test_concurrent_session_mode_project_vocabulary() {
    assert_eq!(ConcurrentSessionMode::from_project_value("minimal"), Some(ConcurrentSessionMode::Minimal));
    assert_eq!(ConcurrentSessionMode::from_project_value("default"), Some(ConcurrentSessionMode::Default));
    assert_eq!(ConcurrentSessionMode::from_project_value("collaborative"), Some(ConcurrentSessionMode::Collaborative));
    assert_eq!(ConcurrentSessionMode::Default.project_value(), "default");
}

fn mock_recall_explanation_with_entity_sections() -> String {
    [
        r#"<memory-recall version="stream-e-v0.5">"#,
        r#"  <entity-recall entities="ent_alpha, ent_beta">"#,
        r#"  </entity-recall>"#,
        r#"  <project-state>"#,
        r#"  </project-state>"#,
        r#"  <entity-recall entities="ent_beta,ent_gamma">"#,
        r#"  </entity-recall>"#,
        r#"</memory-recall>"#,
    ]
    .join("\n")
}

fn mock_startup_recall_with_refs() -> String {
    [
        r#"<memory-recall version="stream-e-v0.5">"#,
        r#"  <entity-recall entities="ent_alpha">"#,
        r#"    <memory ref="project:proj/memories/from-entity.md" />"#,
        r#"  </entity-recall>"#,
        r#"  <project-state>"#,
        r#"    <memory ref="project:proj/state/from-project.md" />"#,
        r#"  </project-state>"#,
        r#"  <recent-memory>"#,
        r#"    <memory ref="project:proj/recent/not-salient.md" />"#,
        r#"  </recent-memory>"#,
        r#"</memory-recall>"#,
    ]
    .join("\n")
}

fn mock_mcp_startup_response_with_refs() -> String {
    [
        r#"<memory_startup>"#,
        r#"  <memory-recall version="stream-e-v0.5">"#,
        r#"    <entity-recall entities="ent_alpha">"#,
        r#"      <memory ref="project:proj/memories/from-entity.md" />"#,
        r#"    </entity-recall>"#,
        r#"    <project-state>"#,
        r#"      <memory ref="project:proj/state/from-project.md" />"#,
        r#"    </project-state>"#,
        r#"  </memory-recall>"#,
        r#"</memory_startup>"#,
    ]
    .join("\n")
}

fn project_binding() -> ProjectBinding {
    ProjectBinding {
        canonical_id: "proj_abc".to_string(),
        alias: Some("my-project".to_string()),
        cwd: Some("/Users/trey/code/my-project".to_string()),
        concurrent_session_mode: Some(ConcurrentSessionMode::Collaborative),
    }
}

fn candidate<const E: usize, const P: usize>(
    id: &str,
    entities: [&str; E],
    paths: [&str; P],
    now: DateTime<Utc>,
) -> PeerWriteCandidate {
    PeerWriteCandidate {
        memory_id: MemoryId::new(id),
        row: row(id, entities, now),
        paths: paths.into_iter().map(String::from).collect(),
        harness: "claude-code".to_string(),
        session_id: "peer_session".to_string(),
        namespace: "project:proj".to_string(),
        embedding: None,
    }
}

fn row<const N: usize>(id: &str, entity_ids: [&str; N], now: DateTime<Utc>) -> RecallIndexRow {
    RecallIndexRow {
        id: MemoryId::new(id),
        path: RepoPath::from_unchecked(format!("project:proj/{id}.md")),
        summary: format!("summary for {id}"),
        status: MemoryStatus::Active,
        scope: Scope::Project,
        canonical_namespace_id: Some("proj".to_string()),
        updated_at: now,
        indexed_at: now,
        confidence: 1.0,
        source_kind: SourceKind::AgentPrimary,
        source_device: Some("device_peer".to_string()),
        source_harness: None,
        source_session_id: None,
        author_harness: None,
        author_session_id: None,
        sensitivity: Sensitivity::Internal,
        passive_recall: true,
        index_body: true,
        requires_user_confirmation: false,
        review_state: None,
        human_review_required: false,
        max_scope: Scope::Project,
        merge_diagnostics_json: None,
        tags: Vec::new(),
        aliases: Vec::new(),
        entities: entity_ids
            .into_iter()
            .map(|id| Entity { id: id.to_string(), label: id.to_string(), aliases: Vec::new() })
            .collect(),
    }
}

fn fixture_now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 5, 1, 12, 0, 0).unwrap()
}

fn set<const N: usize>(values: [&str; N]) -> HashSet<String> {
    values.into_iter().map(String::from).collect()
}
