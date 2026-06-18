use chrono::Utc;
use memory_substrate::index::{open_index, Index};
use memory_substrate::{
    Author, AuthorKind, Entity, Frontmatter, Memory, MemoryId, MemoryStatus, MemoryType, RecallIndexQuery, RepoPath,
    RetrievalPolicy, Scope, Sensitivity, Source, SourceKind, TrustLevel, WritePolicy,
};

// Stream I peer-write attribution reads harness/session identity straight off
// the recall-index row instead of re-reading canonical files. The session/author
// identity and merge-diagnostics fields are projected from `frontmatter_json` via
// `json_extract` only when `RecallIndexQuery::source_identity` is set. The public
// default remains compatibility-full; hot callers opt into the cheap projection.
#[test]
fn recall_index_row_projects_source_and_author_identity_when_requested() {
    let temp = tempfile::tempdir().expect("tempdir");
    let mut index = Index::new(open_index(&temp.path().join("index.sqlite")).expect("open index"));

    index
        .upsert_memory(
            &sample_memory(
                "mem_20260501_a1b2c3d4e5f60718_000501",
                Some("claude-code"),
                Some("sess-source"),
                Some("codex"),
                Some("sess-author"),
            ),
            false,
        )
        .expect("upsert with identity");
    index
        .upsert_memory(&sample_memory("mem_20260501_a1b2c3d4e5f60718_000502", None, None, None, None), false)
        .expect("upsert without identity");

    let mut rows = index.query_recall_index(&RecallIndexQuery::default()).expect("query recall index");
    rows.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));

    assert_eq!(rows[0].source_harness.as_deref(), Some("claude-code"));
    assert_eq!(rows[0].source_session_id.as_deref(), Some("sess-source"));
    assert_eq!(rows[0].author_harness.as_deref(), Some("codex"));
    assert_eq!(rows[0].author_session_id.as_deref(), Some("sess-author"));

    assert_eq!(rows[1].source_harness, None);
    assert_eq!(rows[1].source_session_id, None);
    assert_eq!(rows[1].author_harness, None);
    assert_eq!(rows[1].author_session_id, None);
}

// The hot recall/ranking path uses an explicit cheap projection: `source_harness`
// is still served straight from its materialized column (one fewer JSON parse than
// the prior `json_extract`), while the session/author identity fields the ranking
// path never reads are left `None` and cost no per-row `json_extract`.
#[test]
fn recall_index_row_cheap_projection_keeps_source_harness_column_only() {
    let temp = tempfile::tempdir().expect("tempdir");
    let mut index = Index::new(open_index(&temp.path().join("index.sqlite")).expect("open index"));

    index
        .upsert_memory(
            &sample_memory(
                "mem_20260501_a1b2c3d4e5f60718_000511",
                Some("claude-code"),
                Some("sess-source"),
                Some("codex"),
                Some("sess-author"),
            ),
            false,
        )
        .expect("upsert with identity");

    let rows = index
        .query_recall_index(&RecallIndexQuery { source_identity: false, ..RecallIndexQuery::default() })
        .expect("query recall index");

    // `source_harness` comes from the materialized column, identical to the value
    // `json_extract($.source.harness)` would have returned.
    assert_eq!(rows[0].source_harness.as_deref(), Some("claude-code"));
    // The gated identity fields are unread by ranking and stay `None` (not parsed).
    assert_eq!(rows[0].source_session_id, None);
    assert_eq!(rows[0].author_harness, None);
    assert_eq!(rows[0].author_session_id, None);
    assert_eq!(rows[0].merge_diagnostics_json, None);
}

// Stream I claim-lock conflict detection resolves entities from the index in a
// single batched query rather than reading each locked memory's file.
#[test]
fn entities_for_memories_batches_entity_projection() {
    let temp = tempfile::tempdir().expect("tempdir");
    let mut index = Index::new(open_index(&temp.path().join("index.sqlite")).expect("open index"));

    let mut with_entities = sample_memory("mem_20260501_a1b2c3d4e5f60718_000601", None, None, None, None);
    with_entities.frontmatter.entities =
        vec![Entity { id: "ent_alpha".to_string(), label: "Alpha".to_string(), aliases: vec!["a-1".to_string()] }];
    index.upsert_memory(&with_entities, false).expect("upsert with entities");
    index
        .upsert_memory(&sample_memory("mem_20260501_a1b2c3d4e5f60718_000602", None, None, None, None), false)
        .expect("upsert without entities");

    let result = index
        .entities_for_memories(&[
            "mem_20260501_a1b2c3d4e5f60718_000601".to_string(),
            "mem_20260501_a1b2c3d4e5f60718_000602".to_string(),
            "mem_20260501_a1b2c3d4e5f60718_000603".to_string(), // absent id is omitted
        ])
        .expect("entities_for_memories");

    let alpha = result.get("mem_20260501_a1b2c3d4e5f60718_000601").expect("entities for memory 601");
    assert_eq!(alpha.len(), 1);
    assert_eq!(alpha[0].id, "ent_alpha");
    assert_eq!(alpha[0].aliases, vec!["a-1".to_string()]);
    assert!(!result.contains_key("mem_20260501_a1b2c3d4e5f60718_000602"));
    assert!(!result.contains_key("mem_20260501_a1b2c3d4e5f60718_000603"));

    assert!(index.entities_for_memories(&[]).expect("empty query").is_empty());
}

#[allow(clippy::too_many_arguments)]
fn sample_memory(
    id: &str,
    source_harness: Option<&str>,
    source_session_id: Option<&str>,
    author_harness: Option<&str>,
    author_session_id: Option<&str>,
) -> Memory {
    let now = Utc::now();
    Memory {
        frontmatter: Frontmatter {
            schema_version: 1,
            id: MemoryId::new(id),
            memory_type: MemoryType::Pattern,
            scope: Scope::Agent,
            summary: "source identity fixture".to_string(),
            confidence: 0.7,
            original_confidence: None,
            trust_level: TrustLevel::Trusted,
            sensitivity: Sensitivity::Internal,
            status: MemoryStatus::Active,
            created_at: now,
            updated_at: now,
            observed_at: None,
            author: Author {
                kind: AuthorKind::Agent,
                user_handle: None,
                harness: author_harness.map(str::to_string),
                harness_version: None,
                session_id: author_session_id.map(str::to_string),
                subagent_id: None,
                phase: None,
                component: None,
            },
            namespace: None,
            canonical_namespace_id: None,
            tags: Vec::new(),
            entities: Vec::new(),
            aliases: Vec::new(),
            source: Source {
                kind: SourceKind::AgentPrimary,
                reference: None,
                harness: source_harness.map(str::to_string),
                harness_version: None,
                session_id: source_session_id.map(str::to_string),
                subagent_id: None,
                device: None,
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
                max_scope: Scope::Agent,
                mask_personal_for_synthesis: false,
                index_body: true,
                index_embeddings: false,
            },
            write_policy: WritePolicy {
                human_review_required: false,
                policy_applied: "default-v1".to_string(),
                expected_base_hash: None,
            },
            merge_diagnostics: None,
            extras: Default::default(),
        },
        body: format!("body {id}"),
        path: Some(RepoPath::new(format!("agent/patterns/{id}.md"))),
    }
}
