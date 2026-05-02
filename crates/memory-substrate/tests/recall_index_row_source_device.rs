use chrono::Utc;
use memory_substrate::index::{open_index, Index};
use memory_substrate::{
    Author, AuthorKind, Frontmatter, Memory, MemoryId, MemoryStatus, MemoryType, RecallIndexQuery, RepoPath,
    RetrievalPolicy, Scope, Sensitivity, Source, SourceKind, TrustLevel, WritePolicy,
};

#[test]
fn recall_index_row_hydrates_source_device_when_present_and_absent() {
    let temp = tempfile::tempdir().expect("tempdir");
    let mut index = Index::new(open_index(&temp.path().join("index.sqlite")).expect("open index"));

    index
        .upsert_memory(&sample_memory("mem_20260501_a1b2c3d4e5f60718_000401", Some("dev_a")), false)
        .expect("upsert device a");
    index.upsert_memory(&sample_memory("mem_20260501_a1b2c3d4e5f60718_000402", None), false).expect("upsert no device");

    let mut rows = index.query_recall_index(&RecallIndexQuery::default()).expect("query recall index");
    rows.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));

    assert_eq!(rows[0].source_device.as_deref(), Some("dev_a"));
    assert_eq!(rows[1].source_device, None);
}

fn sample_memory(id: &str, source_device: Option<&str>) -> Memory {
    let now = Utc::now();
    Memory {
        frontmatter: Frontmatter {
            schema_version: 1,
            id: MemoryId::new(id),
            memory_type: MemoryType::Pattern,
            scope: Scope::Agent,
            summary: "source device fixture".to_string(),
            confidence: 0.7,
            original_confidence: None,
            trust_level: TrustLevel::Trusted,
            sensitivity: Sensitivity::Internal,
            status: MemoryStatus::Active,
            created_at: now,
            updated_at: now,
            author: Author {
                kind: AuthorKind::System,
                user_handle: None,
                harness: None,
                harness_version: None,
                session_id: None,
                subagent_id: None,
                phase: None,
                component: Some("test".to_string()),
            },
            namespace: None,
            canonical_namespace_id: None,
            tags: Vec::new(),
            entities: Vec::new(),
            aliases: Vec::new(),
            source: Source {
                kind: SourceKind::Import,
                reference: None,
                harness: None,
                harness_version: None,
                session_id: None,
                subagent_id: None,
                device: source_device.map(str::to_string),
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
