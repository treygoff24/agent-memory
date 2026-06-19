use chrono::{DateTime, Duration, TimeZone, Utc};
use memory_substrate::{
    Author, AuthorKind, ClassificationOutcome, Entity, EventContext, Frontmatter, InitOptions, Memory, MemoryId,
    MemoryStatus, MemoryType, RepoPath, RetrievalPolicy, Roots, Scope, Sensitivity, Source, SourceKind, Substrate,
    TrustLevel, WriteMode, WritePolicy, WriteRequest,
};
use memoryd::handlers::{handle_request_with_state, HandlerState};
use memoryd::protocol::{RequestEnvelope, RequestPayload, ResponsePayload, ResponseResult};
use memoryd::recall::{estimated_tokens, DeltaRequest, RecallSectionName, StartupRequest};
use rusqlite::Connection;

#[tokio::test]
async fn memory_startup_returns_recall_block_and_increments_success_counter() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let runtime = temp.path().join("runtime");
    let substrate = Substrate::init(
        Roots::new(&repo, &runtime),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_startup".to_owned()) },
    )
    .await
    .expect("substrate init");
    let state = HandlerState::new();

    let response = handle_request_with_state(
        &substrate,
        &state,
        RequestEnvelope::new("req-startup", RequestPayload::Startup(startup_request(repo.to_string_lossy().as_ref()))),
    )
    .await;

    match response.result {
        ResponseResult::Success(ResponsePayload::Startup(startup)) => {
            assert_eq!(startup.session_binding.session_id, "sess_startup");
            assert!(startup.recall_block.starts_with("<memory-recall version=\"stream-e-v0.6\""));
            assert!(startup.recall_block.contains("<identity>"));
            assert!(startup.recall_block.contains("<pending-attention>"));
            assert_eq!(startup.recall_explanation.policy, "stream-e-v0.6");
        }
        other => panic!("expected startup success, got {other:?}"),
    }

    let status =
        handle_request_with_state(&substrate, &state, RequestEnvelope::new("req-status", RequestPayload::Status)).await;
    match status.result {
        ResponseResult::Success(ResponsePayload::Status(status)) => {
            assert_eq!(status.recall.startup_invoked_total, 1);
            assert!(status.recall.startup_failed_total.is_empty());
        }
        other => panic!("expected status success, got {other:?}"),
    }
}

#[tokio::test]
async fn memory_startup_response_shape_sections_and_budget_match_contract() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let runtime = temp.path().join("runtime");
    let substrate = Substrate::init(
        Roots::new(&repo, &runtime),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_startup".to_owned()) },
    )
    .await
    .expect("substrate init");
    let state = HandlerState::new();

    let response = handle_request_with_state(
        &substrate,
        &state,
        RequestEnvelope::new(
            "req-startup-shape",
            RequestPayload::Startup(startup_request(repo.to_string_lossy().as_ref())),
        ),
    )
    .await;
    let line = response.to_json_line().expect("startup response serializes");
    assert!(line.contains("\"startup\""), "response payload is the startup variant");

    let ResponseResult::Success(ResponsePayload::Startup(startup)) = response.result else {
        panic!("expected startup response");
    };

    assert!(startup.recall_block.starts_with("<memory-recall version=\"stream-e-v0.6\" harness=\"codex\""));
    assert_ordered(
        &startup.recall_block,
        &[
            "<identity>",
            "<project-state>",
            "<entity-recall",
            "<recent-memory>",
            "<pending-attention>",
            "<recall-explanation",
        ],
    );
    assert_eq!(startup.budget_used_tokens, estimated_tokens(&startup.recall_block));
    assert_eq!(startup.recall_explanation.budget_used_tokens, startup.budget_used_tokens);
    assert_eq!(
        startup.recall_explanation.sections.iter().map(|section| section.name).collect::<Vec<_>>(),
        RecallSectionName::STARTUP_ORDER
    );
    assert!(startup.recall_explanation.sections.iter().all(|section| section.matched_entities.is_empty()));
}

#[tokio::test]
async fn test_cross_device_startup_peer_update() {
    let fixture = StartupCoordinationFixture::new("dev_localstartup").await;
    fixture.write_project_file(Some("default"));
    fixture
        .write_project_peer_memory(
            "mem_20260501_a1b2c3d4e5f60718_000501",
            "other device captured OAuthProvider rename",
            Some("dev_otherstartup"),
        )
        .await;

    let startup = fixture.startup().await;

    assert!(startup.recall_block.contains("<cross-device-updates from-sync=\""));
    assert!(startup.recall_block.contains("device=\"other\""));
    assert!(startup.recall_block.contains("<ref>mem_20260501_a1b2c3d4e5f60718_000501</ref>"));
    assert_ordered(&startup.recall_block, &["<entity-recall", "<cross-device-updates", "</entity-recall>"]);
}

#[tokio::test]
async fn test_startup_no_cross_device_outside_window() {
    let fixture = StartupCoordinationFixture::new("dev_localwindow").await;
    fixture.write_project_file(Some("default"));
    let stale_id = "mem_20260501_a1b2c3d4e5f60718_000502";
    fixture.write_project_peer_memory(stale_id, "stale synced peer update", Some("dev_otherwindow")).await;
    fixture.set_indexed_at(stale_id, Utc::now() - Duration::days(2));

    let startup = fixture.startup().await;

    assert!(!startup.recall_block.contains("<cross-device-updates"));
    assert!(!startup.recall_block.contains(stale_id));
}

#[tokio::test]
async fn test_startup_peer_update_requires_receiving_session_salience() {
    let fixture = StartupCoordinationFixture::new("dev_startupunrel").await;
    fixture.write_project_file(Some("default"));
    let unrelated_id = "mem_20260501_a1b2c3d4e5f60718_000508";
    fixture
        .write_peer_memory(unrelated_id, "unrelated peer write should not self-match", Some("dev_startupunrel"))
        .await;

    let startup = fixture.startup().await;

    assert!(
        !startup.recall_block.contains("<peer-update"),
        "unrelated peer row self-matched: {}",
        startup.recall_block
    );
    assert!(!startup.recall_block.contains(unrelated_id), "unrelated peer row surfaced: {}", startup.recall_block);
}

#[tokio::test]
async fn test_startup_same_device_peer_update_no_device_attr() {
    let fixture = StartupCoordinationFixture::new("dev_localsame").await;
    fixture.write_project_file(Some("default"));
    let same_id = "mem_20260501_a1b2c3d4e5f60718_000503";
    fixture.write_project_peer_memory(same_id, "same device peer update", Some("dev_localsame")).await;

    let startup = fixture.startup().await;
    let opening = peer_update_opening_for_ref(&startup.recall_block, same_id);

    assert!(!startup.recall_block.contains("<cross-device-updates"));
    assert!(!opening.contains("device="), "same-device startup peer-update should not carry device attr: {opening}");
}

#[tokio::test]
async fn test_startup_peer_update_uses_writer_attribution_not_source_device() {
    let fixture = StartupCoordinationFixture::new("dev_startup").await;
    fixture.write_project_file(Some("default"));
    let peer_id = "mem_20260501_a1b2c3d4e5f60718_000507";
    fixture
        .write_project_peer_memory_as(StartupPeerMemorySpec {
            id: peer_id,
            summary: "startup peer attribution for ent_stream_i_startup",
            source_device: Some("dev_startup"),
            harness: "claude-code",
            session_id: "sess_peer_writer",
        })
        .await;

    let startup = fixture.startup().await;
    let opening = peer_update_opening_for_ref(&startup.recall_block, peer_id);

    assert!(opening.contains("from=\"claude-code\""), "{opening}");
    assert!(opening.contains("session=\"sess_pee\""), "{opening}");
    assert!(!opening.contains("session=\"dev_sta\""), "{opening}");
    assert!(
        !startup.recall_block.contains("session=\"dev_startup\""),
        "source device must not render as a session: {}",
        startup.recall_block
    );
}

#[tokio::test]
async fn test_level1_no_peer_update_from_project_mode() {
    let fixture = StartupCoordinationFixture::new("dev_minimalmode").await;
    fixture.write_project_file(Some("minimal"));
    fixture
        .write_project_peer_memory(
            "mem_20260501_a1b2c3d4e5f60718_000504",
            "minimal mode peer update should be suppressed",
            Some("dev_minimalmode"),
        )
        .await;

    let startup = fixture.startup().await;

    assert!(!startup.recall_block.contains("coordination="));
    assert!(!startup.recall_block.contains("<peer-update"));
    assert!(!startup.recall_block.contains("<peer-presence"));
}

#[tokio::test]
async fn project_peer_update_requires_receiving_session_salience() {
    let fixture = StartupCoordinationFixture::new("dev_defaultmode").await;
    fixture.write_project_file(None);
    let peer_id = "mem_20260501_a1b2c3d4e5f60718_000505";
    fixture
        .write_project_peer_memory_with_entities(
            peer_id,
            "default mode unrelated project peer update",
            Some("dev_defaultmode"),
            &["ent_unrelated_project"],
        )
        .await;

    let startup = fixture.startup().await;
    assert!(!startup.recall_block.contains("<peer-update"));
    assert!(!startup.recall_block.contains(peer_id));
}

#[tokio::test]
async fn project_default_mode_overrides_level1_config_fallback() {
    let fixture = StartupCoordinationFixture::new("dev_projectdefault").await;
    fixture.write_project_file(Some("default"));
    let peer_id = "mem_20260501_a1b2c3d4e5f60718_000506";
    fixture
        .write_project_peer_memory_with_entities(
            peer_id,
            "project default unrelated peer update",
            Some("dev_projectdefault"),
            &["ent_unrelated_project"],
        )
        .await;
    let state = HandlerState::with_coordination_level(1);

    let startup = fixture.startup_with_state(&state).await;

    assert_eq!(
        startup.session_binding.project.as_ref().and_then(|project| project.concurrent_session_mode),
        Some(memoryd::recall::ConcurrentSessionMode::Default)
    );
    assert!(!startup.recall_block.contains("<peer-update"));
    assert!(!startup.recall_block.contains(peer_id));
}

#[tokio::test]
async fn unknown_project_mode_rejects_startup_end_to_end() {
    let fixture = StartupCoordinationFixture::new("dev_badmode").await;
    fixture.write_project_file(Some("gibberish"));

    let response = fixture.startup_response_with_state(&HandlerState::new()).await;

    let ResponseResult::Error(error) = response.result else {
        panic!("expected invalid_request for unknown project mode, got {:?}", response.result);
    };
    assert_eq!(error.code, "invalid_request");
    assert!(error.message.contains("concurrent_session_mode"));
}

#[tokio::test]
async fn memory_startup_validation_failure_increments_failure_counter_by_code() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = Substrate::init(
        Roots::new(temp.path().join("repo"), temp.path().join("runtime")),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_startup".to_owned()) },
    )
    .await
    .expect("substrate init");
    let state = HandlerState::new();
    let mut request = startup_request("relative/path");
    request.since_event_id = Some("evt_future".to_owned());

    let response = handle_request_with_state(
        &substrate,
        &state,
        RequestEnvelope::new("req-startup-invalid", RequestPayload::Startup(request)),
    )
    .await;

    match response.result {
        ResponseResult::Error(error) => {
            assert_eq!(error.code, "invalid_request", "cwd validation must run before since_event_id");
            assert!(!error.retryable);
        }
        other => panic!("expected invalid_request, got {other:?}"),
    }

    let status =
        handle_request_with_state(&substrate, &state, RequestEnvelope::new("req-status", RequestPayload::Status)).await;
    match status.result {
        ResponseResult::Success(ResponsePayload::Status(status)) => {
            assert_eq!(status.recall.startup_invoked_total, 0);
            assert_eq!(status.recall.startup_failed_total.get("invalid_request"), Some(&1));
        }
        other => panic!("expected status success, got {other:?}"),
    }
}

#[tokio::test]
async fn memory_startup_since_event_id_missing_event_falls_back_to_full_startup() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let substrate = Substrate::init(
        Roots::new(&repo, temp.path().join("runtime")),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_startup".to_owned()) },
    )
    .await
    .expect("substrate init");
    let state = HandlerState::new();
    let mut request = startup_request(repo.to_string_lossy().as_ref());
    request.since_event_id = Some("evt_future".to_owned());

    let response = handle_request_with_state(
        &substrate,
        &state,
        RequestEnvelope::new("req-startup-delta", RequestPayload::Startup(request)),
    )
    .await;

    match response.result {
        ResponseResult::Success(ResponsePayload::Startup(startup)) => {
            assert!(startup.recall_block.contains("<recall"));
        }
        other => panic!("expected startup fallback success, got {other:?}"),
    }
}

#[tokio::test]
async fn memory_delta_validation_failure_increments_failure_counter_by_code() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let substrate = Substrate::init(
        Roots::new(&repo, temp.path().join("runtime")),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_startup".to_owned()) },
    )
    .await
    .expect("substrate init");
    let state = HandlerState::new();

    let response = handle_request_with_state(
        &substrate,
        &state,
        RequestEnvelope::new(
            "req-delta-invalid",
            RequestPayload::Delta(DeltaRequest {
                cwd: repo.to_string_lossy().into_owned(),
                session_id: "sess_delta".to_owned(),
                harness: "codex".to_owned(),
                message: " ".to_owned(),
                budget_tokens: Some(512),
                passive: false,
            }),
        ),
    )
    .await;

    match response.result {
        ResponseResult::Error(error) => assert_eq!(error.code, "invalid_request"),
        other => panic!("expected invalid_request, got {other:?}"),
    }

    let status =
        handle_request_with_state(&substrate, &state, RequestEnvelope::new("req-status", RequestPayload::Status)).await;
    match status.result {
        ResponseResult::Success(ResponsePayload::Status(status)) => {
            assert_eq!(status.recall.delta_invoked_total, 0);
            assert_eq!(status.recall.delta_failed_total.get("invalid_request"), Some(&1));
        }
        other => panic!("expected status success, got {other:?}"),
    }
}

fn startup_request(cwd: &str) -> StartupRequest {
    StartupRequest {
        cwd: cwd.to_owned(),
        session_id: "sess_startup".to_owned(),
        harness: "codex".to_owned(),
        harness_version: Some("0.0.0".to_owned()),
        include_recent: true,
        since_event_id: None,
        budget_tokens: Some(512),
        passive: false,
    }
}

fn assert_ordered(haystack: &str, needles: &[&str]) {
    let mut previous = 0usize;
    for needle in needles {
        let index = haystack.find(needle).unwrap_or_else(|| panic!("missing section marker {needle}"));
        assert!(index >= previous, "{needle} appeared out of order");
        previous = index;
    }
}

struct StartupCoordinationFixture {
    _temp: tempfile::TempDir,
    repo: std::path::PathBuf,
    runtime: std::path::PathBuf,
    substrate: Substrate,
}

impl StartupCoordinationFixture {
    async fn new(device_id: &str) -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let repo = temp.path().join("repo");
        let runtime = temp.path().join("runtime");
        let substrate = Substrate::init(
            Roots::new(&repo, &runtime),
            InitOptions { force_unsafe_durability: true, device_id: Some(device_id.to_owned()) },
        )
        .await
        .expect("substrate init");
        let repo = repo.canonicalize().expect("repo canonicalizes");
        let runtime = runtime.canonicalize().expect("runtime canonicalizes");
        Self { _temp: temp, repo, runtime, substrate }
    }

    async fn write_peer_memory(&self, id: &str, summary: &str, source_device: Option<&str>) {
        self.write_peer_memory_as(StartupPeerMemorySpec {
            id,
            summary,
            source_device,
            harness: "codex",
            session_id: "sess_peer_writer",
        })
        .await;
    }

    async fn write_peer_memory_as(&self, spec: StartupPeerMemorySpec<'_>) {
        self.substrate
            .write_memory(WriteRequest {
                operation_id: None,
                memory: peer_memory(&spec),
                expected_base_hash: None,
                write_mode: WriteMode::CreateNew,
                index_projection: None,
                event_context: EventContext::default(),
                allow_best_effort_durability: true,
                classification: ClassificationOutcome::Trusted,
            })
            .await
            .expect("peer memory write");
    }

    async fn write_project_peer_memory(&self, id: &str, summary: &str, source_device: Option<&str>) {
        self.write_project_peer_memory_as(StartupPeerMemorySpec {
            id,
            summary,
            source_device,
            harness: "codex",
            session_id: "sess_peer_writer",
        })
        .await;
    }

    async fn write_project_peer_memory_as(&self, spec: StartupPeerMemorySpec<'_>) {
        self.write_project_peer_memory_as_with_entities(&spec, &["proj_stream_i", "stream-i"]).await;
    }

    #[allow(clippy::too_many_arguments)]
    async fn write_project_peer_memory_with_entities(
        &self,
        id: &str,
        summary: &str,
        source_device: Option<&str>,
        entities: &[&str],
    ) {
        let spec =
            StartupPeerMemorySpec { id, summary, source_device, harness: "codex", session_id: "sess_peer_writer" };
        self.write_project_peer_memory_as_with_entities(&spec, entities).await;
    }

    async fn write_project_peer_memory_as_with_entities(&self, spec: &StartupPeerMemorySpec<'_>, entities: &[&str]) {
        let mut memory = peer_memory(spec);
        memory.frontmatter.scope = Scope::Project;
        memory.frontmatter.namespace = Some("stream-i".to_string());
        memory.frontmatter.canonical_namespace_id = Some("proj_stream_i".to_string());
        memory.frontmatter.entities = entities
            .iter()
            .map(|entity| Entity {
                id: (*entity).to_string(),
                label: (*entity).to_string(),
                aliases: if *entity == "proj_stream_i" { vec!["stream-i".to_string()] } else { Vec::new() },
            })
            .collect();
        memory.frontmatter.retrieval_policy.max_scope = Scope::Project;
        memory.path = Some(RepoPath::new(format!("projects/proj_stream_i/patterns/{}.md", spec.id)));
        self.substrate
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
            .expect("project peer memory write");
        self.set_indexed_at(spec.id, Utc::now());
    }

    fn set_indexed_at(&self, id: &str, indexed_at: DateTime<Utc>) {
        let connection = Connection::open(self.runtime.join("index.sqlite")).expect("open index sqlite");
        let changed = connection
            .execute("UPDATE memories SET indexed_at = ?1 WHERE id = ?2", (indexed_at.to_rfc3339(), id))
            .expect("update indexed_at");
        assert_eq!(changed, 1, "fixture indexed_at update should affect one row");
    }

    fn write_project_file(&self, mode: Option<&str>) {
        let mode_line = mode.map(|mode| format!("concurrent_session_mode: {mode}\n")).unwrap_or_default();
        std::fs::write(
            self.repo.join(".memory-project.yaml"),
            format!("canonical_id: proj_stream_i\nalias: stream-i\n{mode_line}"),
        )
        .expect("write project binding file");
    }

    async fn startup(&self) -> Box<memoryd::recall::StartupResponse> {
        self.startup_with_state(&HandlerState::new()).await
    }

    async fn startup_with_state(&self, state: &HandlerState) -> Box<memoryd::recall::StartupResponse> {
        let response = self.startup_response_with_state(state).await;

        match response.result {
            ResponseResult::Success(ResponsePayload::Startup(startup)) => startup,
            other => panic!("expected startup success, got {other:?}"),
        }
    }

    async fn startup_response_with_state(&self, state: &HandlerState) -> memoryd::protocol::ResponseEnvelope {
        handle_request_with_state(
            &self.substrate,
            state,
            RequestEnvelope::new(
                "req-startup-coordination",
                RequestPayload::Startup(StartupRequest {
                    cwd: self.repo.to_string_lossy().into_owned(),
                    session_id: "sess_startup".to_owned(),
                    harness: "codex".to_owned(),
                    harness_version: Some("0.0.0".to_owned()),
                    include_recent: false,
                    since_event_id: None,
                    budget_tokens: Some(2048),
                    passive: false,
                }),
            ),
        )
        .await
    }
}

struct StartupPeerMemorySpec<'a> {
    id: &'a str,
    summary: &'a str,
    source_device: Option<&'a str>,
    harness: &'a str,
    session_id: &'a str,
}

fn peer_memory(spec: &StartupPeerMemorySpec<'_>) -> Memory {
    let authored_at = Utc.with_ymd_and_hms(2026, 5, 1, 9, 45, 0).single().expect("fixture timestamp");
    Memory {
        frontmatter: Frontmatter {
            schema_version: 1,
            id: MemoryId::new(spec.id),
            memory_type: MemoryType::Project,
            scope: Scope::Agent,
            summary: spec.summary.to_owned(),
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
                harness: Some(spec.harness.to_owned()),
                harness_version: None,
                session_id: Some(spec.session_id.to_owned()),
                subagent_id: None,
                phase: None,
                component: None,
            },
            namespace: None,
            canonical_namespace_id: None,
            tags: Vec::new(),
            entities: vec![Entity {
                id: "ent_stream_i_startup".to_owned(),
                label: "Stream I startup".to_owned(),
                aliases: Vec::new(),
            }],
            aliases: Vec::new(),
            source: Source {
                kind: SourceKind::AgentPrimary,
                reference: None,
                harness: Some(spec.harness.to_owned()),
                harness_version: None,
                session_id: Some(spec.session_id.to_owned()),
                subagent_id: None,
                device: spec.source_device.map(str::to_owned),
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
                policy_applied: "stream-i-startup-test".to_owned(),
                expected_base_hash: None,
            },
            merge_diagnostics: None,
            extras: Default::default(),
        },
        body: spec.summary.to_owned(),
        path: Some(RepoPath::new(format!("agent/patterns/{}.md", spec.id))),
    }
}

fn peer_update_opening_for_ref<'a>(recall_block: &'a str, reference: &str) -> &'a str {
    let ref_marker = format!("<ref>{reference}</ref>");
    let ref_position = recall_block.find(&ref_marker).expect("peer update ref exists");
    let before_ref = &recall_block[..ref_position];
    let opening_start = before_ref.rfind("<peer-update").expect("peer update opening exists before ref");
    let opening_end = before_ref[opening_start..].find('>').expect("peer update opening closes") + opening_start + 1;
    &before_ref[opening_start..opening_end]
}
