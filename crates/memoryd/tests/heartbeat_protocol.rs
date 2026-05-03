use std::time::Duration;

use chrono::{TimeZone, Utc};
use memorum_coordination::claim_lock::ClaimLockAcquireRequest;
use memorum_coordination::{ConcurrentSessionMode, PeerHeartbeat, ProjectBinding};
use memory_substrate::{
    Author, AuthorKind, ClassificationOutcome, Entity, EventContext, Frontmatter, InitOptions, Memory, MemoryId,
    MemoryStatus, MemoryType, RepoPath, RetrievalPolicy, Roots, Scope, Sensitivity, Source, SourceKind, Substrate,
    TrustLevel, WriteMode, WritePolicy, WriteRequest,
};
use memoryd::handlers::{handle_request_with_state, HandlerState};
use memoryd::protocol::{RequestEnvelope, RequestPayload, ResponsePayload, ResponseResult};
use tempfile::TempDir;
use tokio::time::sleep;

#[test]
fn heartbeat_serde_roundtrip_preserves_optional_started_at() {
    let with_started_at =
        RequestPayload::PeerHeartbeat(heartbeat("sess_a", Some(Utc.with_ymd_and_hms(2026, 5, 1, 12, 0, 0).unwrap())));
    let json = serde_json::to_string(&with_started_at).expect("serialize heartbeat");
    let decoded: RequestPayload = serde_json::from_str(&json).expect("deserialize heartbeat");
    assert_eq!(decoded, with_started_at);

    let without_started_at = RequestPayload::PeerHeartbeat(heartbeat("sess_a", None));
    let json = serde_json::to_string(&without_started_at).expect("serialize heartbeat without started_at");
    let decoded: RequestPayload = serde_json::from_str(&json).expect("deserialize heartbeat without started_at");
    assert_eq!(decoded, without_started_at);
}

#[tokio::test]
async fn level3_heartbeat_retains_first_non_none_started_at() {
    let fixture = Fixture::new(3).await;
    let first = Utc.with_ymd_and_hms(2026, 5, 1, 12, 0, 0).unwrap();
    let second = Utc.with_ymd_and_hms(2026, 5, 1, 13, 0, 0).unwrap();

    fixture.heartbeat("first", heartbeat("sess_a", Some(first))).await;
    fixture.heartbeat("second", heartbeat("sess_a", Some(second))).await;

    let records = fixture.state.presence().all_records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].started_at, Some(first));
}

#[tokio::test]
async fn level3_heartbeat_records_started_at_after_initial_none() {
    let fixture = Fixture::new(3).await;
    let started_at = Utc.with_ymd_and_hms(2026, 5, 1, 12, 0, 0).unwrap();

    fixture.heartbeat("first", heartbeat("sess_a", None)).await;
    fixture.heartbeat("second", heartbeat("sess_a", Some(started_at))).await;

    let records = fixture.state.presence().all_records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].started_at, Some(started_at));
}

#[tokio::test]
async fn level3_heartbeat_ack_reports_active_peers() {
    let fixture = Fixture::new(3).await;
    fixture.heartbeat("peer-a", heartbeat("session_alpha", None)).await;

    let ack = fixture.heartbeat("peer-b", heartbeat("session_beta", None)).await;

    assert_eq!(ack.session_id, "session_beta");
    assert_eq!(ack.active_level, 3);
    assert_eq!(ack.peer_session_count, 1);
    assert_eq!(ack.active_peers.len(), 1);
    assert_eq!(ack.active_peers[0].harness, "codex");
}

#[tokio::test]
async fn level1_and_level2_heartbeat_ack_without_presence_update() {
    for level in [1, 2] {
        let fixture = Fixture::new(level).await;
        let ack = fixture.heartbeat("noop", heartbeat("sess_noop", None)).await;

        assert_eq!(ack.active_level, level);
        assert_eq!(ack.peer_session_count, 0);
        assert!(fixture.state.presence().all_records().is_empty());
    }
}

#[tokio::test]
async fn collaborative_project_mode_heartbeat_records_presence_and_renews_claim_locks() {
    let fixture = Fixture::new(1).await;
    let memory_id = "mem_20260501_a1b2c3d4e5f60718_000701";
    fixture.state.claim_locks().acquire(ClaimLockAcquireRequest::new(
        memory_id,
        "sess_a",
        "codex",
        Duration::from_secs(1),
    ));
    let before = fixture.state.claim_locks().get(memory_id).expect("initial lock exists");
    sleep(Duration::from_millis(5)).await;

    let mut request = heartbeat("sess_a", None);
    request.project_binding = Some(project_binding(ConcurrentSessionMode::Collaborative));
    request.claim_locks_held = vec![memory_id.to_string()];
    let ack = fixture.heartbeat("collaborative", request).await;

    assert_eq!(ack.active_level, 3);
    assert_eq!(fixture.state.presence().all_records().len(), 1);
    let after = fixture.state.claim_locks().get(memory_id).expect("renewed lock exists");
    assert!(after.expires_at > before.expires_at);
}

#[tokio::test]
async fn level3_heartbeat_ack_reports_conflicting_claim_locks_for_salient_entities() {
    let fixture = Fixture::new(3).await;
    let memory_id = "mem_20260501_a1b2c3d4e5f60718_000702";
    fixture.write_memory(memory_id, "ent_stream_i").await;
    fixture.state.claim_locks().acquire(ClaimLockAcquireRequest::new(
        memory_id,
        "sess_holder",
        "claude-code",
        Duration::from_secs(300),
    ));

    let ack = fixture.heartbeat("conflict", heartbeat("sess_a", None)).await;

    assert_eq!(ack.conflicting_claim_locks.len(), 1);
    assert_eq!(ack.conflicting_claim_locks[0].memory_id, memory_id);
    assert_eq!(ack.conflicting_claim_locks[0].holder_session_id, "sess_holder");
}

#[tokio::test]
async fn minimal_project_mode_heartbeat_overrides_level3_config() {
    let fixture = Fixture::new(3).await;
    let mut request = heartbeat("sess_minimal", None);
    request.project_binding = Some(project_binding(ConcurrentSessionMode::Minimal));

    let ack = fixture.heartbeat("minimal", request).await;

    assert_eq!(ack.active_level, 1);
    assert!(fixture.state.presence().all_records().is_empty());
}

#[tokio::test]
async fn heartbeat_validation_rejects_empty_session_id() {
    let fixture = Fixture::new(3).await;
    let response = handle_request_with_state(
        &fixture.substrate,
        &fixture.state,
        RequestEnvelope::new("bad-heartbeat", RequestPayload::PeerHeartbeat(heartbeat("   ", None))),
    )
    .await;

    let ResponseResult::Error(error) = response.result else {
        panic!("expected validation error, got {:?}", response.result);
    };
    assert_eq!(error.code, "invalid_request");
    assert!(error.message.contains("session_id"));
}

#[tokio::test]
async fn heartbeat_validation_rejects_entity_overflow() {
    let fixture = Fixture::new(3).await;
    let mut request = heartbeat("sess_a", None);
    request.salient_entities = (0..33).map(|index| format!("ent_{index}")).collect();
    let response = handle_request_with_state(
        &fixture.substrate,
        &fixture.state,
        RequestEnvelope::new("bad-heartbeat", RequestPayload::PeerHeartbeat(request)),
    )
    .await;

    let ResponseResult::Error(error) = response.result else {
        panic!("expected validation error, got {:?}", response.result);
    };
    assert_eq!(error.code, "invalid_request");
    assert!(error.message.contains("salient_entities"));
}

struct Fixture {
    _temp: TempDir,
    substrate: Substrate,
    state: HandlerState,
}

impl Fixture {
    async fn new(level: u8) -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
        let substrate = Substrate::init(
            roots,
            InitOptions { force_unsafe_durability: true, device_id: Some("dev_heartbeat01".to_string()) },
        )
        .await
        .expect("init substrate");
        Self { _temp: temp, substrate, state: HandlerState::with_coordination_level(level) }
    }

    async fn heartbeat(&self, request_id: &str, heartbeat: PeerHeartbeat) -> memorum_coordination::PeerHeartbeatAck {
        let response = handle_request_with_state(
            &self.substrate,
            &self.state,
            RequestEnvelope::new(request_id, RequestPayload::PeerHeartbeat(heartbeat)),
        )
        .await;
        let ResponseResult::Success(ResponsePayload::PeerHeartbeat(ack)) = response.result else {
            panic!("expected heartbeat ack, got {:?}", response.result);
        };
        ack
    }

    async fn write_memory(&self, id: &str, entity: &str) {
        let authored_at = Utc.with_ymd_and_hms(2026, 5, 1, 12, 0, 0).unwrap();
        self.substrate
            .write_memory(WriteRequest {
                operation_id: None,
                memory: Memory {
                    frontmatter: Frontmatter {
                        schema_version: 1,
                        id: MemoryId::new(id),
                        memory_type: MemoryType::Project,
                        scope: Scope::Agent,
                        summary: "claim lock conflict fixture".to_string(),
                        confidence: 0.9,
                        original_confidence: None,
                        trust_level: TrustLevel::Trusted,
                        sensitivity: Sensitivity::Internal,
                        status: MemoryStatus::Active,
                        created_at: authored_at,
                        updated_at: authored_at,
                        observed_at: None,
                        author: Author {
                            kind: AuthorKind::Agent,
                            user_handle: None,
                            harness: Some("claude-code".to_string()),
                            harness_version: None,
                            session_id: Some("sess_holder".to_string()),
                            subagent_id: None,
                            phase: None,
                            component: None,
                        },
                        namespace: None,
                        canonical_namespace_id: None,
                        tags: Vec::new(),
                        entities: vec![Entity {
                            id: entity.to_string(),
                            label: entity.to_string(),
                            aliases: Vec::new(),
                        }],
                        aliases: Vec::new(),
                        source: Source {
                            kind: SourceKind::AgentPrimary,
                            reference: None,
                            harness: Some("claude-code".to_string()),
                            harness_version: None,
                            session_id: Some("sess_holder".to_string()),
                            subagent_id: None,
                            device: Some("dev_heartbeat01".to_string()),
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
                            policy_applied: "stream-i-heartbeat-test".to_string(),
                            expected_base_hash: None,
                        },
                        merge_diagnostics: None,
                        extras: Default::default(),
                    },
                    body: "claim lock conflict fixture".to_string(),
                    path: Some(RepoPath::new(format!("agent/patterns/{id}.md"))),
                },
                expected_base_hash: None,
                write_mode: WriteMode::CreateNew,
                index_projection: None,
                event_context: EventContext::default(),
                allow_best_effort_durability: true,
                classification: ClassificationOutcome::Trusted,
            })
            .await
            .expect("write conflict memory");
    }
}

fn heartbeat(session_id: &str, started_at: Option<chrono::DateTime<Utc>>) -> PeerHeartbeat {
    PeerHeartbeat {
        session_id: session_id.to_string(),
        device_id: Some("dev_heartbeat01".to_string()),
        harness: "codex".to_string(),
        project_binding: None,
        namespace: "project:agent-memory".to_string(),
        salient_entities: vec!["ent_stream_i".to_string()],
        salient_paths: vec!["docs/specs/stream-i-cross-session-v0.1.md".to_string()],
        capabilities: vec!["memory".to_string()],
        started_at,
        claim_locks_held: Vec::new(),
    }
}

fn project_binding(mode: ConcurrentSessionMode) -> ProjectBinding {
    ProjectBinding {
        canonical_id: "proj_stream_i".to_string(),
        alias: Some("stream-i".to_string()),
        cwd: None,
        concurrent_session_mode: Some(mode),
    }
}
