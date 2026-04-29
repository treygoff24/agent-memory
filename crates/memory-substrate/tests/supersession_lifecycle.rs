use memory_substrate::events::EventKind;
use memory_substrate::tree::{validate_tree, TreeValidationMode};
use memory_substrate::{
    Author, AuthorKind, ClassificationOutcome, EventContext, Frontmatter, InitOptions, Memory, MemoryId, MemoryStatus,
    MemoryType, RepoPath, RetrievalPolicy, Roots, Scope, Sensitivity, Source, SourceKind, Substrate, SupersedeRequest,
    TrustLevel, WriteMode, WritePolicy, WriteRequest,
};

#[tokio::test]
async fn supersession_lifecycle_updates_bidirectional_chain_and_records_event() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_test".to_string()) },
    )
    .await
    .expect("init");
    let old_id = MemoryId::new("mem_20260429_a1b2c3d4e5f60718_000001");
    let new_id = MemoryId::new("mem_20260429_a1b2c3d4e5f60718_000002");

    substrate
        .write_memory(WriteRequest {
            operation_id: None,
            memory: sample_memory(old_id.clone(), "Old claim", "The old claim is active."),
            expected_base_hash: None,
            write_mode: WriteMode::CreateNew,
            index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Trusted,
        })
        .await
        .expect("write old memory");
    let event_count_before_supersession = substrate.events().expect("events before").len();

    let outcome = substrate
        .supersede_memory(SupersedeRequest {
            old_id: old_id.clone(),
            replacement: sample_memory(new_id.clone(), "New claim", "The replacement claim is active."),
            reason: "contradiction resolved by newer source".to_string(),
            classification: ClassificationOutcome::Trusted,
            allow_best_effort_durability: true,
        })
        .await
        .expect("supersede memory");

    assert_eq!(outcome.old_id, old_id);
    assert_eq!(outcome.new_id, new_id);
    assert!(outcome.new_outcome.committed);
    assert!(outcome.old_outcome.committed);

    let old_memory = substrate.read_memory(&outcome.old_id).await.expect("read old");
    let new_memory = substrate.read_memory(&outcome.new_id).await.expect("read new");
    assert_eq!(old_memory.frontmatter.status, MemoryStatus::Superseded);
    assert_eq!(old_memory.frontmatter.superseded_by, vec![outcome.new_id.clone()]);
    assert_eq!(new_memory.frontmatter.supersedes, vec![outcome.old_id.clone()]);

    validate_tree(&roots.repo, TreeValidationMode::FullySynced).expect("valid supersession graph");

    let events = substrate.events().expect("events after");
    assert!(events.len() > event_count_before_supersession);
    assert!(events
        .iter()
        .skip(event_count_before_supersession)
        .any(|event| matches!(&event.kind, EventKind::WriteCommitted { id, .. } if id == &outcome.old_id)));
}

fn sample_memory(id: MemoryId, summary: &str, body: &str) -> Memory {
    let now = chrono::DateTime::parse_from_rfc3339("2026-04-29T12:00:00Z").expect("date").with_timezone(&chrono::Utc);
    Memory {
        frontmatter: Frontmatter {
            schema_version: 1,
            id: id.clone(),
            memory_type: MemoryType::Pattern,
            scope: Scope::Agent,
            summary: summary.to_string(),
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
                policy_applied: "project-standard@v2".to_string(),
                expected_base_hash: None,
            },
            merge_diagnostics: None,
            extras: std::collections::BTreeMap::new(),
        },
        body: body.to_string(),
        path: Some(RepoPath::new(format!("agent/patterns/{id}.md"))),
    }
}
