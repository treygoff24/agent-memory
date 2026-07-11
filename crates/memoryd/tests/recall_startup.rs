use chrono::{Duration, Utc};
use memory_substrate::events::EventKind;
use memory_substrate::{
    Author, AuthorKind, ClassificationOutcome, EventContext, Frontmatter, InitOptions, Memory, MemoryId, MemoryStatus,
    MemoryType, RepoPath, RetrievalPolicy, Roots, Scope, Sensitivity, Source, SourceKind, Substrate, TrustLevel,
    WriteMode, WritePolicy, WriteRequest,
};
use memoryd::handlers::{handle_request_with_state, HandlerState};
use memoryd::protocol::{RequestEnvelope, RequestPayload, ResponsePayload, ResponseResult};
use memoryd::recall::StartupRequest;

#[tokio::test]
async fn startup_since_event_id_missing_event_falls_back_to_full_startup() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let substrate = init_substrate(&repo, temp.path().join("runtime")).await;
    write_memory(
        &substrate,
        sample_memory(
            "mem_20260507_a1b2c3d4e5f60001_000001",
            "Full startup fallback memory remains visible.",
            Utc::now(),
        ),
    )
    .await;

    let state = HandlerState::new();
    let mut request = startup_request(repo.to_string_lossy().as_ref());
    request.since_event_id = Some("evt_missing".to_owned());

    let response = handle_request_with_state(
        &substrate,
        &state,
        RequestEnvelope::new("req-startup-missing-event", RequestPayload::Startup(request)),
    )
    .await;

    let ResponseResult::Success(ResponsePayload::Startup(startup)) = response.result else {
        panic!("expected startup fallback success, got {:?}", response.result);
    };
    assert!(startup.recall_block.contains("Full startup fallback memory remains visible"), "{}", startup.recall_block);
}

#[tokio::test]
async fn startup_since_event_id_uses_event_timestamp_as_delta_floor() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let substrate = init_substrate(&repo, temp.path().join("runtime")).await;
    let old_id = MemoryId::new("mem_20260507_a1b2c3d4e5f60002_000002");
    write_memory(
        &substrate,
        sample_memory(
            old_id.as_str(),
            "Old startup memory should be below the delta floor.",
            Utc::now() - Duration::days(1),
        ),
    )
    .await;
    substrate
        .record_event_best_effort(EventKind::EncryptedContentRevealed { id: old_id, reason: "delta marker".to_owned() })
        .expect("record marker event");
    let marker_event_id = substrate.events().expect("events").last().expect("marker event").id.clone();
    write_memory(
        &substrate,
        sample_memory(
            "mem_20260507_a1b2c3d4e5f60003_000003",
            "Fresh startup memory should appear after the delta floor.",
            Utc::now() + Duration::seconds(1),
        ),
    )
    .await;

    let state = HandlerState::new();
    let mut request = startup_request(repo.to_string_lossy().as_ref());
    request.since_event_id = Some(marker_event_id.to_string());

    let response = handle_request_with_state(
        &substrate,
        &state,
        RequestEnvelope::new("req-startup-valid-event", RequestPayload::Startup(request)),
    )
    .await;

    let ResponseResult::Success(ResponsePayload::Startup(startup)) = response.result else {
        panic!("expected startup delta success, got {:?}", response.result);
    };
    assert!(startup.recall_block.contains("Fresh startup memory should appear"), "{}", startup.recall_block);
    assert!(!startup.recall_block.contains("Old startup memory should be below"), "{}", startup.recall_block);
}

async fn init_substrate(repo: &std::path::Path, runtime: impl AsRef<std::path::Path>) -> Substrate {
    Substrate::init(
        Roots::new(repo, runtime.as_ref()),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_startup".to_owned()) },
    )
    .await
    .expect("substrate init")
}

async fn write_memory(substrate: &Substrate, memory: Memory) {
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
        .expect("write memory");
}

fn startup_request(cwd: &str) -> StartupRequest {
    StartupRequest {
        cwd: cwd.to_owned(),
        session_id: "sess_startup".to_owned(),
        harness: "codex".to_owned(),
        harness_version: Some("0.0.0".to_owned()),
        include_recent: true,
        since_event_id: None,
        budget_tokens: Some(1024),
        passive: false,
    }
}

fn sample_memory(id: &str, body: &str, updated_at: chrono::DateTime<Utc>) -> Memory {
    Memory {
        frontmatter: Frontmatter {
            schema_version: 1,
            id: MemoryId::new(id),
            memory_type: MemoryType::Pattern,
            scope: Scope::Agent,
            summary: body.to_owned(),
            confidence: 0.95,
            original_confidence: None,
            trust_level: TrustLevel::Trusted,
            sensitivity: Sensitivity::Internal,
            status: MemoryStatus::Active,
            created_at: updated_at,
            updated_at,
            observed_at: None,
            author: Author {
                kind: AuthorKind::System,
                user_handle: None,
                harness: Some("codex".to_owned()),
                harness_version: None,
                session_id: Some("sess_startup".to_owned()),
                subagent_id: None,
                phase: None,
                component: Some("recall-startup-test".to_owned()),
            },
            namespace: None,
            canonical_namespace_id: None,
            tags: Vec::new(),
            entities: Vec::new(),
            aliases: Vec::new(),
            source: Source {
                kind: SourceKind::Import,
                reference: None,
                harness: Some("codex".to_owned()),
                harness_version: None,
                session_id: Some("sess_startup".to_owned()),
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
                policy_applied: "default-v1".to_owned(),
                expected_base_hash: None,
            },
            merge_diagnostics: None,
            abstraction: None,
            cues: Vec::new(),
            extras: std::collections::BTreeMap::new(),
        },
        body: body.to_owned(),
        path: Some(RepoPath::new(format!("agent/patterns/{id}.md"))),
    }
}
