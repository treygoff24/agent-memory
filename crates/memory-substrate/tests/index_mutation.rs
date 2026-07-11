use chrono::Utc;
use memory_substrate::index::{open_index, Index};
use memory_substrate::{
    Author, AuthorKind, Frontmatter, Memory, MemoryId, MemoryStatus, MemoryType, RepoPath, RetrievalPolicy, Scope,
    Sensitivity, Sha256, Source, SourceKind, TrustLevel, WritePolicy,
};

#[test]
fn fts_update_and_delete_remove_old_terms() {
    let temp = tempfile::tempdir().expect("tempdir");
    let mut index = Index::new(open_index(&temp.path().join("index.sqlite")).expect("open index"));
    let mut memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_010001", "oldneedle body");
    index.upsert_memory(&memory, false).expect("initial upsert");
    assert_eq!(index.query_chunks("oldneedle").expect("old query").len(), 1);

    memory.body = "newneedle body".to_string();
    index.upsert_memory(&memory, false).expect("update upsert");
    assert!(index.query_chunks("oldneedle").expect("old query after update").is_empty());
    assert_eq!(index.query_chunks("newneedle").expect("new query").len(), 1);

    index
        .connection()
        .execute("DELETE FROM memory_chunks WHERE memory_id=?1", [memory.frontmatter.id.as_str()])
        .expect("delete chunks");
    assert!(index.query_chunks("newneedle").expect("new query after delete").is_empty());
}

#[test]
fn upsert_can_store_actual_disk_file_hash_separately_from_body_hash() {
    let temp = tempfile::tempdir().expect("tempdir");
    let mut index = Index::new(open_index(&temp.path().join("index.sqlite")).expect("open index"));
    let memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_010101", "body hash differs from file hash");
    let path = memory.path.clone().expect("fixture has path");
    let file_hash = Sha256::new("sha256:file-bytes");

    index.upsert_memory_with_file_hash(&memory, false, Some(&file_hash)).expect("upsert with file hash");

    assert_eq!(index.file_hash_for(&path), Some(file_hash));
}

#[test]
fn vacuum_preserves_chunk_fts_matches() {
    let temp = tempfile::tempdir().expect("tempdir");
    let mut index = Index::new(open_index(&temp.path().join("index.sqlite")).expect("open index"));
    for seq in 0..1_000 {
        // Include seq in the body so every memory has distinct chunk text.
        // Chunk IDs are content-addressable (chk_<sha256(text)>) — identical
        // body text across different memories would collide on the UNIQUE(chunk_id)
        // constraint, which is correct per-spec but wrong for this fixture.
        let token = if seq == 777 { "vacuumneedle777".to_string() } else { format!("filler unique {seq}") };
        let id = format!("mem_20260424_a1b2c3d4e5f60718_{seq:06}");
        index.upsert_memory(&sample_memory(&id, &token), false).expect("upsert fixture");
    }
    let before = index.query_chunks("vacuumneedle777").expect("before vacuum");

    index.connection().execute_batch("VACUUM").expect("vacuum");
    let after = index.query_chunks("vacuumneedle777").expect("after vacuum");

    assert_eq!(before.len(), 1);
    assert_eq!(after.len(), 1);
    assert_eq!(before[0].memory_id, after[0].memory_id);
}

fn sample_memory(id: &str, body: &str) -> Memory {
    let now = Utc::now();
    Memory {
        frontmatter: Frontmatter {
            schema_version: 1,
            id: MemoryId::new(id),
            memory_type: MemoryType::Pattern,
            scope: Scope::Agent,
            summary: "index".to_string(),
            confidence: 1.0,
            original_confidence: None,
            trust_level: TrustLevel::Trusted,
            sensitivity: Sensitivity::Internal,
            status: MemoryStatus::Active,
            created_at: now,
            updated_at: now,
            observed_at: None,
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
                index_embeddings: true,
            },
            write_policy: WritePolicy {
                human_review_required: false,
                policy_applied: "default-v1".to_string(),
                expected_base_hash: None,
            },
            merge_diagnostics: None,
            abstraction: None,
            cues: Vec::new(),
            extras: std::collections::BTreeMap::new(),
        },
        body: body.to_string(),
        path: Some(RepoPath::new(format!("agent/patterns/{id}.md"))),
    }
}
