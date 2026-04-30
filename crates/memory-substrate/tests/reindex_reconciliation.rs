use memory_substrate::*;

#[tokio::test]
async fn external_edit_to_same_path_is_indexed_by_reindex() {
    let (temp, substrate, roots, mut memory) = seeded("mem_20260424_a1b2c3d4e5f60718_020001").await;
    let path = roots.repo.join(memory.path.clone().expect("path").as_path());
    memory.body = "externalneedle edited by human".to_string();
    let markdown = memory_substrate::frontmatter::serialize_document(&memory).expect("serialize edit");
    std::fs::write(&path, markdown).expect("external edit");

    let count = substrate.reindex().await.expect("reindex");
    let hits = substrate
        .query_chunks(ChunkQuery { text: Some("externalneedle".to_string()), triple: None, vector: None })
        .await
        .expect("query external edit");

    assert_eq!(count, 1);
    assert_eq!(hits.len(), 1);
    drop(temp);
}

#[tokio::test]
async fn reindex_refuses_plaintext_markdown_under_encrypted_namespace() {
    let (_temp, substrate, roots, memory) = seeded("mem_20260424_a1b2c3d4e5f60718_020005").await;
    let markdown = memory_substrate::frontmatter::serialize_document(&memory).expect("serialize leak fixture");
    std::fs::create_dir_all(roots.repo.join("encrypted/agent/patterns")).expect("encrypted dirs");
    std::fs::write(roots.repo.join("encrypted/agent/patterns/leak.md"), markdown).expect("plaintext leak");
    let before = substrate
        .query_memory(MemoryQuery {
            id: Some(memory.frontmatter.id.clone()),
            tag: None,
            include_metadata_only: false,
            ..MemoryQuery::default()
        })
        .await
        .expect("query before failed reindex");

    let err = substrate.reindex().await.expect_err("encrypted namespace plaintext refused");
    let after = substrate
        .query_memory(MemoryQuery {
            id: Some(memory.frontmatter.id),
            tag: None,
            include_metadata_only: false,
            ..MemoryQuery::default()
        })
        .await
        .expect("query after failed reindex");

    assert!(
        matches!(err, memory_substrate::SubstrateError::Open(memory_substrate::OpenError::OperatorRepairRequired(message)) if message.contains("encrypted namespace"))
    );
    assert_eq!(before, after);
}

#[tokio::test]
async fn reindex_refuses_malformed_candidate_without_clearing_existing_index() {
    let (_temp, substrate, roots, memory) = seeded("mem_20260424_a1b2c3d4e5f60718_020006").await;
    std::fs::write(roots.repo.join("agent/patterns/malformed.md"), "---\nschema_version: 99\n---\nbad")
        .expect("malformed candidate");
    let before = substrate
        .query_memory(MemoryQuery {
            id: Some(memory.frontmatter.id.clone()),
            tag: None,
            include_metadata_only: false,
            ..MemoryQuery::default()
        })
        .await
        .expect("query before malformed reindex");

    let err = substrate.reindex().await.expect_err("malformed candidate refused");
    let after = substrate
        .query_memory(MemoryQuery {
            id: Some(memory.frontmatter.id),
            tag: None,
            include_metadata_only: false,
            ..MemoryQuery::default()
        })
        .await
        .expect("query after malformed reindex");

    assert!(matches!(
        err,
        memory_substrate::SubstrateError::Open(memory_substrate::OpenError::OperatorRepairRequired(_))
    ));
    assert_eq!(before, after);
}

#[tokio::test]
async fn mass_changes_converge_to_fresh_reindex_state() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");
    for seq in 0..25 {
        let mut memory = sample_memory(&format!("mem_20260424_a1b2c3d4e5f60718_{seq:06}"));
        memory.body = format!("massneedle {seq}");
        substrate
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
            .expect("write mass fixture");
    }

    let reindexed = substrate.reindex().await.expect("reindex");
    let hits = substrate
        .query_chunks(ChunkQuery { text: Some("massneedle".to_string()), triple: None, vector: None })
        .await
        .expect("query mass fixtures");

    assert_eq!(reindexed, 25);
    assert_eq!(hits.len(), 20);
}

#[tokio::test]
async fn path_only_rename_reindexes_existing_memory_to_new_path() {
    let (_temp, substrate, roots, mut memory) = seeded("mem_20260424_a1b2c3d4e5f60718_020002").await;
    let old_path = roots.repo.join(memory.path.clone().expect("old path").as_path());
    let new_repo_path = RepoPath::new("agent/playbooks/renamed-memory.md");
    let new_path = roots.repo.join(new_repo_path.as_path());
    std::fs::create_dir_all(new_path.parent().expect("new parent")).expect("new parent");
    memory.path = Some(new_repo_path.clone());
    let markdown = memory_substrate::frontmatter::serialize_document(&memory).expect("serialize rename");
    std::fs::write(&new_path, markdown).expect("write renamed");
    std::fs::remove_file(old_path).expect("remove old");

    substrate.reindex().await.expect("reindex");
    let hits = substrate
        .query_memory(MemoryQuery {
            id: Some(memory.frontmatter.id),
            tag: None,
            include_metadata_only: false,
            ..MemoryQuery::default()
        })
        .await
        .expect("query renamed");

    assert_eq!(hits[0].path, new_repo_path);
}

#[tokio::test]
async fn rename_plus_id_change_removes_old_id_and_indexes_new_id() {
    let (_temp, substrate, roots, mut memory) = seeded("mem_20260424_a1b2c3d4e5f60718_020003").await;
    let old_id = memory.frontmatter.id.clone();
    let old_path = roots.repo.join(memory.path.clone().expect("old path").as_path());
    let new_id = MemoryId::new("mem_20260424_a1b2c3d4e5f60718_020004");
    let new_repo_path = RepoPath::new("agent/playbooks/renamed-with-new-id.md");
    let new_path = roots.repo.join(new_repo_path.as_path());
    std::fs::create_dir_all(new_path.parent().expect("new parent")).expect("new parent");
    memory.frontmatter.id = new_id.clone();
    memory.path = Some(new_repo_path.clone());
    let markdown = memory_substrate::frontmatter::serialize_document(&memory).expect("serialize rename");
    std::fs::write(&new_path, markdown).expect("write renamed");
    std::fs::remove_file(old_path).expect("remove old");

    substrate.reindex().await.expect("reindex");

    assert!(substrate
        .query_memory(MemoryQuery {
            id: Some(old_id),
            tag: None,
            include_metadata_only: false,
            ..MemoryQuery::default()
        })
        .await
        .expect("query old id")
        .is_empty());
    let hits = substrate
        .query_memory(MemoryQuery {
            id: Some(new_id),
            tag: None,
            include_metadata_only: false,
            ..MemoryQuery::default()
        })
        .await
        .expect("query new id");
    assert_eq!(hits[0].path, new_repo_path);
}

async fn seeded(id: &str) -> (tempfile::TempDir, Substrate, Roots, Memory) {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");
    let memory = sample_memory(id);
    substrate
        .write_memory(WriteRequest {
            operation_id: None,
            memory: memory.clone(),
            expected_base_hash: None,
            write_mode: WriteMode::CreateNew,
            index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Trusted,
        })
        .await
        .expect("write");
    (temp, substrate, roots, memory)
}

fn sample_memory(id: &str) -> Memory {
    let now = chrono::DateTime::parse_from_rfc3339("2026-04-24T12:00:00Z").expect("date").with_timezone(&chrono::Utc);
    Memory {
        frontmatter: Frontmatter {
            schema_version: 1,
            id: MemoryId::new(id),
            memory_type: MemoryType::Pattern,
            scope: Scope::Agent,
            summary: "reindex".to_string(),
            confidence: 1.0,
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
            extras: std::collections::BTreeMap::new(),
        },
        body: "body".to_string(),
        path: Some(RepoPath::new(format!("agent/patterns/{id}.md"))),
    }
}
