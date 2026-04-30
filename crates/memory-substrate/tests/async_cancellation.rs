use memory_substrate::*;

#[tokio::test]
async fn dropping_unpolled_write_future_has_no_repo_index_or_event_effects() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");
    let memory = sample_memory("mem_20260424_a1b2c3d4e5f60718_030001");
    let path = roots.repo.join(memory.path.clone().expect("path").as_path());
    let before_events = substrate.events().expect("events before").len();

    let future = substrate.write_memory(WriteRequest {
        operation_id: None,
        memory: memory.clone(),
        expected_base_hash: None,
        write_mode: WriteMode::CreateNew,
        index_projection: None,
        event_context: EventContext::default(),
        allow_best_effort_durability: true,
        classification: ClassificationOutcome::Trusted,
    });
    drop(future);

    assert!(!path.exists());
    assert!(substrate
        .query_memory(MemoryQuery {
            id: Some(memory.frontmatter.id),
            tag: None,
            include_metadata_only: false,
            ..MemoryQuery::default()
        })
        .await
        .expect("query")
        .is_empty());
    assert_eq!(substrate.events().expect("events after").len(), before_events);
}

fn sample_memory(id: &str) -> Memory {
    let now = chrono::DateTime::parse_from_rfc3339("2026-04-24T12:00:00Z").expect("date").with_timezone(&chrono::Utc);
    Memory {
        frontmatter: Frontmatter {
            schema_version: 1,
            id: MemoryId::new(id),
            memory_type: MemoryType::Pattern,
            scope: Scope::Agent,
            summary: "cancel".to_string(),
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
