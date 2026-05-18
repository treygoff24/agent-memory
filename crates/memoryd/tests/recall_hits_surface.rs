use chrono::{DateTime, Utc};
use memory_substrate::{
    events::{append_event, Event, EventKind, EVENT_SCHEMA_VERSION},
    Author, AuthorKind, ClassificationOutcome, DeviceId, EventContext, EventId, Frontmatter, InitOptions, Memory,
    MemoryId, MemoryStatus, MemoryType, OperationId, RepoPath, RetrievalPolicy, Roots, Scope, Sensitivity, Source,
    SourceKind, Substrate, TrustLevel, WriteMode, WritePolicy, WriteRequest,
};
use memoryd::handlers::handle_request;
use memoryd::protocol::{RequestEnvelope, RequestPayload, ResponsePayload, ResponseResult};

#[tokio::test]
async fn recall_hits_protocol_reads_recent_events_log_rows_with_memory_summary() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = Substrate::init(
        Roots::new(temp.path().join("repo"), temp.path().join("runtime")),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_recallhits01".to_owned()) },
    )
    .await
    .expect("substrate init");
    let memory_id = MemoryId::new("mem_20260502_aaaaaaaaaaaaaaaa_000001");
    write_memory(&substrate, memory_id.clone()).await;
    let event_path = substrate.roots().repo.join("events/dev_recallhits01.jsonl");
    append_event(
        &event_path,
        &recall_event("evt_old", 1, memory_id.clone(), "2026-05-01T00:00:00Z", "2026-05-01T00:05:00Z"),
    )
    .expect("old event");
    let recalled_at = instant("2026-05-02T00:05:00Z");
    append_event(
        &event_path,
        &recall_event("evt_new", 2, memory_id.clone(), "2026-05-02T00:00:00Z", "2026-05-02T00:05:00Z"),
    )
    .expect("new event");
    substrate.doctor_reindex_events_log().expect("reindex events_log mirror");

    let response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "req-recall-hits",
            RequestPayload::RecallHits { since: Some(instant("2026-05-01T12:00:00Z")), limit: Some(10) },
        ),
    )
    .await;

    let ResponseResult::Success(ResponsePayload::RecallHits(response)) = response.result else {
        panic!("expected recall hits success, got {:?}", response.result);
    };
    assert_eq!(response.hits.len(), 1);
    assert_eq!(response.hits[0].event_id, "evt_new");
    assert_eq!(response.hits[0].memory_id, memory_id);
    assert_eq!(response.hits[0].recalled_at, recalled_at);
    assert_eq!(response.hits[0].summary.as_deref(), Some("Recall-hit surface fixture"));
}

async fn write_memory(substrate: &Substrate, id: MemoryId) {
    substrate
        .write_memory(WriteRequest {
            operation_id: None,
            memory: Memory {
                frontmatter: Frontmatter {
                    schema_version: 1,
                    id: id.clone(),
                    memory_type: MemoryType::Project,
                    scope: Scope::User,
                    summary: "Recall-hit surface fixture".to_owned(),
                    confidence: 0.9,
                    original_confidence: None,
                    trust_level: TrustLevel::Trusted,
                    sensitivity: Sensitivity::Internal,
                    status: MemoryStatus::Active,
                    created_at: instant("2026-05-01T00:00:00Z"),
                    updated_at: instant("2026-05-01T00:00:00Z"),
                    observed_at: None,
                    author: Author {
                        kind: AuthorKind::Agent,
                        user_handle: None,
                        harness: Some("codex".to_owned()),
                        harness_version: None,
                        session_id: Some("sess_recall_hits_surface".to_owned()),
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
                        harness: Some("codex".to_owned()),
                        harness_version: None,
                        session_id: Some("sess_recall_hits_surface".to_owned()),
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
                        max_scope: Scope::User,
                        mask_personal_for_synthesis: false,
                        index_body: true,
                        index_embeddings: true,
                    },
                    write_policy: WritePolicy {
                        human_review_required: false,
                        policy_applied: "recall-hit-surface-test".to_owned(),
                        expected_base_hash: None,
                    },
                    merge_diagnostics: None,
                    extras: Default::default(),
                },
                body: "Recall-hit surface fixture body".to_owned(),
                path: Some(RepoPath::new(format!("me/{}.md", id.as_str()))),
            },
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

#[allow(clippy::too_many_arguments)]
fn recall_event(event_id: &str, seq: u64, id: MemoryId, event_at: &str, recalled_at: &str) -> Event {
    Event {
        schema: EVENT_SCHEMA_VERSION,
        id: EventId::new(event_id),
        at: instant(event_at),
        device: DeviceId::new("dev_recallhits01"),
        seq,
        operation_id: Some(OperationId::new(format!("op_{event_id}"))),
        kind: EventKind::RecallHit { id, recalled_at: instant(recalled_at) },
        crc32c: 0,
    }
}

fn instant(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value).expect("timestamp parses").with_timezone(&Utc)
}
