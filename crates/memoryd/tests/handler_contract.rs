use memory_substrate::{
    Author, AuthorKind, ClassificationOutcome, EventContext, Frontmatter, InitOptions, Memory, MemoryId, MemoryStatus,
    MemoryType, RetrievalPolicy, Roots, Scope, Sensitivity, Source, SourceKind, Substrate, TrustLevel, WriteMode,
    WritePolicy, WriteRequest,
};
use memoryd::handlers::handle_request;
use memoryd::protocol::{RequestEnvelope, RequestPayload, ResponsePayload, ResponseResult};

#[tokio::test]
async fn search_and_get_return_bounded_protocol_responses_from_substrate() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = init_substrate(roots).await;
    let memory = sample_memory(
        "mem_20260428_a1b2c3d4e5f60718_300001",
        "Handler contracts search Stream A chunks and return bounded protocol snippets. \
         This extra sentence should not force unbounded response bodies into search results.",
    );

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
        .expect("write memory through Stream A");

    let search = handle_request(
        &substrate,
        RequestEnvelope::new(
            "req-search",
            RequestPayload::Search {
                query: "bounded protocol snippets".to_string(),
                limit: Some(1),
                include_body: false,
            },
        ),
    )
    .await;

    let ResponseResult::Success(ResponsePayload::Search(search)) = search.result else {
        panic!("expected search success, got {:?}", search.result);
    };
    assert_eq!(search.hits.len(), 1);
    assert_eq!(search.total, 1);
    assert_eq!(search.hits[0].id, memory.frontmatter.id.as_str());
    assert!(search.hits[0].snippet.len() <= 240, "search snippets stay bounded");
    assert!(search.guidance.contains("memory_get"));

    let get = handle_request(
        &substrate,
        RequestEnvelope::new(
            "req-get",
            RequestPayload::Get { id: memory.frontmatter.id.as_str().to_string(), include_provenance: false },
        ),
    )
    .await;

    let ResponseResult::Success(ResponsePayload::Get(get)) = get.result else {
        panic!("expected get success, got {:?}", get.result);
    };
    assert_eq!(get.id, memory.frontmatter.id.as_str());
    assert_eq!(get.summary, memory.frontmatter.summary);
    assert!(get.body.len() <= 4_096, "get bodies are bounded protocol previews");
    assert!(get.guidance.contains("bounded"));
}

#[tokio::test]
async fn write_note_creates_candidate_safe_record_through_substrate() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = init_substrate(roots).await;

    let response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "req-write",
            RequestPayload::WriteNote { text: "Candidate note from handler".to_string() },
        ),
    )
    .await;

    let ResponseResult::Success(ResponsePayload::WriteNote(write)) = response.result else {
        panic!("expected write-note success, got {:?}", response.result);
    };
    let saved = substrate.read_memory(&MemoryId::new(&write.id)).await.expect("candidate note is readable");

    assert_eq!(saved.frontmatter.status, MemoryStatus::Candidate);
    assert_eq!(saved.frontmatter.sensitivity, Sensitivity::Internal);
    assert!(saved.frontmatter.tags.iter().any(|tag| tag == "candidate"));
    assert!(saved.frontmatter.requires_user_confirmation);
    assert_eq!(saved.body, "Candidate note from handler");
}

async fn init_substrate(roots: Roots) -> Substrate {
    Substrate::init(
        roots,
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_handlercontract".to_string()) },
    )
    .await
    .expect("init substrate")
}

fn sample_memory(id: &str, body: &str) -> Memory {
    let now = chrono::DateTime::parse_from_rfc3339("2026-04-28T12:00:00Z")
        .expect("fixed test date")
        .with_timezone(&chrono::Utc);
    Memory {
        frontmatter: Frontmatter {
            schema_version: 1,
            id: MemoryId::new(id),
            memory_type: MemoryType::Pattern,
            scope: Scope::Agent,
            summary: "handler contract memory".to_string(),
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
                component: Some("memoryd-handler-test".to_string()),
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
        body: body.to_string(),
        path: Some(memory_substrate::RepoPath::new(format!("agent/patterns/{id}.md"))),
    }
}
