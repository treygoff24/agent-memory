use memory_substrate::*;

#[tokio::test]
async fn deterministic_crash_matrix_converges_to_documented_write_states() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");

    let before_write = substrate
        .write_memory(write_request(
            sample_memory("mem_20260424_a1b2c3d4e5f60718_040001"),
            ClassificationOutcome::Secret,
        ))
        .await
        .expect_err("before-write refusal");
    assert!(!before_write.outcome.committed);

    let mut invalid_path = sample_memory("mem_20260424_a1b2c3d4e5f60718_040002");
    invalid_path.path = Some(RepoPath::from_unchecked("../escape.md"));
    let during_write = substrate
        .write_memory(write_request(invalid_path, ClassificationOutcome::Trusted))
        .await
        .expect_err("pre-rename path refusal");
    assert!(!during_write.outcome.committed);

    let committed = substrate
        .write_memory(write_request(
            sample_memory("mem_20260424_a1b2c3d4e5f60718_040003"),
            ClassificationOutcome::Trusted,
        ))
        .await
        .expect("fully committed write");
    assert!(committed.committed && committed.indexed && committed.event_recorded);

    let blocked_event_log = roots.repo.join("events/dev_test.jsonl");
    if blocked_event_log.exists() {
        std::fs::remove_file(&blocked_event_log).expect("remove event log");
    }
    std::fs::create_dir_all(&blocked_event_log).expect("block event log");
    std::fs::create_dir_all(roots.runtime.join("pending/events.jsonl")).expect("block pending event queue");
    let event_failed = substrate
        .write_memory(write_request(
            sample_memory("mem_20260424_a1b2c3d4e5f60718_040004"),
            ClassificationOutcome::Trusted,
        ))
        .await
        .expect_err("event failure after commit");
    assert!(event_failed.outcome.committed && event_failed.outcome.indexed && !event_failed.outcome.event_recorded);
    assert_eq!(event_failed.outcome.repair_required, Some(RepairRequired::FullStartupScan));
}

fn write_request(memory: Memory, classification: ClassificationOutcome) -> WriteRequest {
    WriteRequest {
        operation_id: None,
        memory,
        expected_base_hash: None,
        write_mode: WriteMode::CreateNew,
        index_projection: None,
        event_context: EventContext::default(),
        allow_best_effort_durability: true,
        classification,
    }
}

fn sample_memory(id: &str) -> Memory {
    let now = chrono::DateTime::parse_from_rfc3339("2026-04-24T12:00:00Z").expect("date").with_timezone(&chrono::Utc);
    Memory {
        frontmatter: Frontmatter {
            schema_version: 1,
            id: MemoryId::new(id),
            memory_type: MemoryType::Pattern,
            scope: Scope::Agent,
            summary: "crash".to_string(),
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
            extras: std::collections::BTreeMap::new(),
        },
        // Body includes the id so each fixture's chunk_id is distinct
        // (R-IX-4 cross-phase: index has UNIQUE constraint on content-addressed chunk_id).
        body: format!("body for {id}"),
        path: Some(RepoPath::new(format!("agent/patterns/{id}.md"))),
    }
}
