use std::time::{Duration, Instant};

use chrono::{TimeZone, Utc};
use memorum_coordination::claim_lock::ClaimLockAcquireRequest;
use memorum_coordination::{CoordinationConfig, PresenceRecord};
use memory_substrate::{
    Author, AuthorKind, ClassificationOutcome, Entity, EventContext, Frontmatter, InitOptions, Memory, MemoryId,
    MemoryStatus, MemoryType, RepoPath, RetrievalPolicy, Roots, Scope, Sensitivity, Source, SourceKind, Substrate,
    TrustLevel, WriteMode, WritePolicy, WriteRequest,
};
use memoryd::handlers::{handle_request_with_state, HandlerState};
use memoryd::protocol::{
    PeerActivityFormat, PeerActivityResponse, RequestEnvelope, RequestPayload, ResponsePayload, ResponseResult,
};
use memoryd::recall::DeltaRequest;

#[tokio::test]
async fn level1_daemon_delta_has_no_coordination_insertion() {
    let fixture = CoordinationFixture::new("dev_deltalevel1", 1).await;
    let peer_id = "mem_20260501_a1b2c3d4e5f60718_000800";
    fixture
        .write_peer_memory(
            peer_id,
            "Level one should suppress ent_stream_i_level1 coordination.",
            &["ent_stream_i_level1"],
        )
        .await;

    let delta = fixture.delta(&message_for(peer_id, "ent_stream_i_level1")).await;

    assert!(!delta.contains("coordination="), "{delta}");
    assert!(!delta.contains("<peer-update"), "{delta}");
    assert!(!delta.contains("<peer-presence"), "{delta}");
}

#[tokio::test]
async fn level2_daemon_delta_includes_relevant_peer_update_from_index() {
    let fixture = CoordinationFixture::new("dev_deltalevel2", 2).await;
    let peer_id = "mem_20260501_a1b2c3d4e5f60718_000801";
    fixture
        .write_peer_memory(peer_id, "Coordination delta peer summary for ent_stream_i_delta.", &["ent_stream_i_delta"])
        .await;
    fixture.acquire_lock(peer_id, "claude-code", "sess_claim_holder");

    let delta = fixture.delta(&message_for(peer_id, "ent_stream_i_delta")).await;

    assert!(delta.contains("coordination=\"stream-i-v0.1\""), "{delta}");
    assert!(delta.contains("<peer-update"), "{delta}");
    assert!(delta.contains(&format!("<ref>{peer_id}</ref>")), "{delta}");
    assert!(delta.contains("claim_locked=\"claude-code:sess_claim_holder\""), "{delta}");
    assert!(!delta.contains("raw body secret"), "{delta}");
}

#[tokio::test]
async fn level2_daemon_delta_omits_below_threshold_peer_update() {
    let fixture = CoordinationFixture::new("dev_deltabelow", 2).await;
    let peer_id = "mem_20260501_a1b2c3d4e5f60718_000806";
    fixture.write_peer_memory(peer_id, "Unrelated peer summary for ent_peer_only.", &["ent_peer_only"]).await;

    let delta = fixture.delta(&message_for(peer_id, "ent_receiver_only")).await;

    assert!(!delta.contains("<peer-update"), "{delta}");
}

#[tokio::test]
async fn daemon_delta_uses_non_default_coordination_threshold() {
    let mut config = CoordinationConfig::default();
    config.relevance_gate.threshold = 0.9;
    let fixture = CoordinationFixture::new_with_config("dev_deltathreshold", config).await;
    let peer_id = "mem_20260501_a1b2c3d4e5f60718_000811";
    fixture
        .write_peer_memory(peer_id, "Threshold fixture for ent_stream_i_threshold.", &["ent_stream_i_threshold"])
        .await;

    let delta = fixture.delta(&message_for(peer_id, "ent_stream_i_threshold")).await;

    assert!(!delta.contains("<peer-update"), "0.8 score should not clear configured 0.9 threshold: {delta}");
}

#[tokio::test]
async fn level2_daemon_delta_caps_peer_updates_and_counts_pending_attention() {
    let fixture = CoordinationFixture::new("dev_deltalevel2", 2).await;
    for peer_id in [
        "mem_20260501_a1b2c3d4e5f60718_000811",
        "mem_20260501_a1b2c3d4e5f60718_000812",
        "mem_20260501_a1b2c3d4e5f60718_000813",
        "mem_20260501_a1b2c3d4e5f60718_000814",
    ] {
        fixture
            .write_peer_memory(
                peer_id,
                "Coordination delta peer summary for ent_stream_i_delta.",
                &["ent_stream_i_delta"],
            )
            .await;
    }

    let delta = fixture.delta("ent_stream_i_delta agent/patterns/mem_20260501_a1b2c3d4e5f60718_000811.md agent/patterns/mem_20260501_a1b2c3d4e5f60718_000812.md agent/patterns/mem_20260501_a1b2c3d4e5f60718_000813.md agent/patterns/mem_20260501_a1b2c3d4e5f60718_000814.md").await;

    assert_eq!(count_occurrences(&delta, "<peer-update"), 2, "{delta}");
    assert!(delta.contains("kind=\"coordination_overflow\" count=\"2\""), "{delta}");
}

#[tokio::test]
async fn level3_daemon_delta_includes_peer_presence_before_peer_update() {
    let fixture = CoordinationFixture::new("dev_deltalevel3", 3).await;
    let peer_id = "mem_20260501_a1b2c3d4e5f60718_000802";
    fixture
        .write_peer_memory(peer_id, "Level three peer update for ent_stream_i_presence.", &["ent_stream_i_presence"])
        .await;
    fixture.state.presence().upsert(presence_record("codex", "sess_current", fixture.device_id.as_str()));
    fixture.state.presence().upsert(presence_record("claude-code", "sess_peer_presence", fixture.device_id.as_str()));

    let delta = fixture.delta(&message_for(peer_id, "ent_stream_i_presence")).await;

    assert_in_order(
        &delta,
        &["<peer-presence>", "<session harness=\"claude-code\"", "</peer-presence>", "<peer-update"],
    );
    assert!(!delta.contains("id=\"sess_c\""), "current session must be excluded when possible: {delta}");
}

#[tokio::test]
async fn level3_presence_requires_salient_entity_or_path_overlap() {
    let fixture = CoordinationFixture::new("dev_deltapresencefilter", 3).await;
    fixture.state.presence().upsert(presence_record("codex", "sess_current", fixture.device_id.as_str()));
    fixture.state.presence().upsert(presence_record_with_entities(
        "claude-code",
        "overlap123",
        fixture.device_id.as_str(),
        ["ent_stream_i_presence"],
    ));
    fixture.state.presence().upsert(presence_record_with_entities(
        "cursor",
        "unrel999",
        fixture.device_id.as_str(),
        ["ent_unrelated_work"],
    ));

    let delta = fixture.delta("ent_stream_i_presence").await;

    assert!(delta.contains("<peer-presence>"), "{delta}");
    assert!(delta.contains("harness=\"claude-code\""), "{delta}");
    assert!(delta.contains("id=\"overla\""), "{delta}");
    assert!(!delta.contains("harness=\"cursor\""), "{delta}");
    assert!(!delta.contains("id=\"unrel9\""), "{delta}");
}

#[tokio::test]
async fn level3_presence_renders_for_salient_path_overlap_without_entity_overlap() {
    let fixture = CoordinationFixture::new("dev_deltapathpresence", 3).await;
    fixture.state.presence().upsert(presence_record("codex", "sess_current", fixture.device_id.as_str()));
    fixture.state.presence().upsert(presence_record_with_entities_and_paths(
        ("claude-code", "pathov123"),
        fixture.device_id.as_str(),
        ["ent_peer_only"],
        ["docs/specs/stream-i-cross-session-v0.1.md"],
    ));
    fixture.state.presence().upsert(presence_record_with_entities_and_paths(
        ("cursor", "nopth999"),
        fixture.device_id.as_str(),
        ["ent_other_peer"],
        ["docs/specs/unrelated.md"],
    ));

    let delta = fixture.delta("docs/specs/stream-i-cross-session-v0.1.md").await;

    assert!(delta.contains("<peer-presence>"), "{delta}");
    assert!(delta.contains("harness=\"claude-code\""), "{delta}");
    assert!(delta.contains("id=\"pathov\""), "{delta}");
    assert!(!delta.contains("harness=\"cursor\""), "{delta}");
    assert!(!delta.contains("id=\"nopth9\""), "{delta}");
}

#[tokio::test]
async fn level3_presence_cap_prefers_highest_entity_overlap_before_tie_breakers() {
    let fixture = CoordinationFixture::new("dev_presencecap", 3).await;
    fixture.state.presence().upsert(presence_record("codex", "sess_current", fixture.device_id.as_str()));
    fixture.state.presence().upsert(presence_record_with_entities(
        "aaa-low",
        "low111",
        fixture.device_id.as_str(),
        ["ent_stream_i_presence"],
    ));
    for (harness, session, extra) in [
        ("zzz-high1", "high111", "ent_extra_1"),
        ("zzz-high2", "high222", "ent_extra_2"),
        ("zzz-high3", "high333", "ent_extra_3"),
        ("zzz-high4", "high444", "ent_extra_4"),
    ] {
        fixture.state.presence().upsert(presence_record_with_entities(
            harness,
            session,
            fixture.device_id.as_str(),
            ["ent_stream_i_presence", extra],
        ));
    }

    let delta = fixture.delta("ent_stream_i_presence ent_extra_1 ent_extra_2 ent_extra_3 ent_extra_4").await;

    assert_eq!(count_occurrences(&delta, "<session harness="), 4, "{delta}");
    assert!(!delta.contains("harness=\"aaa-low\""), "{delta}");
    assert!(delta.contains("harness=\"zzz-high1\""), "{delta}");
    assert!(delta.contains("harness=\"zzz-high4\""), "{delta}");
    assert!(delta.contains("coordination_overflow\" count=\"1\""), "{delta}");
}

#[tokio::test]
async fn production_delta_delivery_populates_peer_activity_audit() {
    let fixture = CoordinationFixture::new("dev_deltaaudit", 2).await;
    let peer_id = "mem_20260501_a1b2c3d4e5f60718_000803";
    fixture
        .write_peer_memory(peer_id, "Audit records rendered ent_stream_i_audit peer summary.", &["ent_stream_i_audit"])
        .await;

    let delta = fixture.delta(&message_for(peer_id, "ent_stream_i_audit")).await;
    assert!(delta.contains("<peer-update"), "{delta}");

    let activity = fixture.peer_activity().await;
    assert_eq!(activity.total_recorded, 1);
    assert_eq!(activity.entries[0].memory_id, peer_id);
    assert_eq!(activity.entries[0].to_session_id, "sess_current");
    assert!(activity.entries[0].summary.contains("Audit records rendered"));
}

#[tokio::test]
async fn peer_update_uses_actual_writer_attribution_in_delta_and_audit() {
    let fixture = CoordinationFixture::new("dev_deltaattribution", 2).await;
    let peer_id = "mem_20260501_a1b2c3d4e5f60718_000804";
    fixture
        .write_peer_memory_as(PeerMemorySpec {
            id: peer_id,
            summary: "Attribution carries ent_stream_i_attribution source identity.",
            entities: &["ent_stream_i_attribution"],
            harness: "claude-code",
            session_id: "sess_peer_writer",
        })
        .await;

    let delta = fixture.delta(&message_for(peer_id, "ent_stream_i_attribution")).await;

    assert!(
        delta.contains("<peer-update from=\"claude-code\" session=\"sess_pee\""),
        "actual writer harness/session should render, got: {delta}"
    );
    assert!(!delta.contains("from=\"codex\""), "{delta}");
    assert!(!delta.contains("session=\"dev_delt\""), "{delta}");

    let activity = fixture.peer_activity().await;
    assert_eq!(activity.total_recorded, 1);
    assert_eq!(activity.entries[0].from_harness, "claude-code");
    assert_eq!(activity.entries[0].from_session_id, "sess_peer_writer");
}

#[tokio::test]
async fn peer_update_cooldown_is_per_receiving_session_in_daemon_ram() {
    let fixture = CoordinationFixture::new("dev_deltacooldown", 2).await;
    let peer_id = "mem_20260501_a1b2c3d4e5f60718_000805";
    fixture
        .write_peer_memory(
            peer_id,
            "Cooldown suppresses repeated ent_stream_i_cooldown peer updates.",
            &["ent_stream_i_cooldown"],
        )
        .await;

    let first = fixture.delta_as("sess_current", "codex", &message_for(peer_id, "ent_stream_i_cooldown")).await;
    let second = fixture.delta_as("sess_current", "codex", &message_for(peer_id, "ent_stream_i_cooldown")).await;
    let other_receiver =
        fixture.delta_as("sess_other_receiver", "codex", &message_for(peer_id, "ent_stream_i_cooldown")).await;

    assert!(first.contains("<peer-update"), "{first}");
    assert!(!second.contains("<peer-update"), "{second}");
    assert!(other_receiver.contains("<peer-update"), "{other_receiver}");
}

struct CoordinationFixture {
    _temp: tempfile::TempDir,
    device_id: String,
    repo: std::path::PathBuf,
    substrate: Substrate,
    state: HandlerState,
}

impl CoordinationFixture {
    async fn new(device_id: &str, coordination_level: u8) -> Self {
        let config = CoordinationConfig { level: coordination_level, ..CoordinationConfig::default() };
        Self::new_with_config(device_id, config).await
    }

    async fn new_with_config(device_id: &str, coordination_config: CoordinationConfig) -> Self {
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
        Self {
            _temp: temp,
            device_id: device_id.to_owned(),
            repo,
            substrate,
            state: HandlerState::with_coordination_config(coordination_config),
        }
    }

    async fn delta(&self, message: &str) -> String {
        self.delta_as("sess_current", "codex", message).await
    }

    async fn delta_as(&self, session_id: &str, harness: &str, message: &str) -> String {
        let response = handle_request_with_state(
            &self.substrate,
            &self.state,
            RequestEnvelope::new(
                "req-delta-coordination",
                RequestPayload::Delta(DeltaRequest {
                    cwd: self.repo.to_string_lossy().into_owned(),
                    session_id: session_id.to_owned(),
                    harness: harness.to_owned(),
                    message: message.to_owned(),
                    budget_tokens: Some(2_048),
                    passive: false,
                }),
            ),
        )
        .await;

        match response.result {
            ResponseResult::Success(ResponsePayload::Delta(delta)) => delta.delta_block,
            other => panic!("expected delta success, got {other:?}"),
        }
    }

    async fn write_peer_memory(&self, id: &str, summary: &str, entities: &[&str]) {
        self.write_peer_memory_as(PeerMemorySpec {
            id,
            summary,
            entities,
            harness: "codex",
            session_id: "sess_peer_writer",
        })
        .await;
    }

    async fn write_peer_memory_as(&self, spec: PeerMemorySpec<'_>) {
        self.substrate
            .write_memory(WriteRequest {
                operation_id: None,
                memory: peer_memory(&spec, &self.device_id),
                expected_base_hash: None,
                write_mode: WriteMode::CreateNew,
                index_projection: None,
                event_context: EventContext::default(),
                allow_best_effort_durability: true,
                classification: ClassificationOutcome::Trusted,
            })
            .await
            .unwrap_or_else(|error| panic!("peer memory write {}: {error:?}", spec.id));
    }

    async fn peer_activity(&self) -> PeerActivityResponse {
        let response = handle_request_with_state(
            &self.substrate,
            &self.state,
            RequestEnvelope::new(
                "req-peer-activity",
                RequestPayload::PeerActivity {
                    session: None,
                    since: None,
                    limit: Some(10),
                    format: PeerActivityFormat::Human,
                },
            ),
        )
        .await;

        match response.result {
            ResponseResult::Success(ResponsePayload::PeerActivity(activity)) => activity,
            other => panic!("expected peer activity success, got {other:?}"),
        }
    }

    fn acquire_lock(&self, memory_id: &str, harness: &str, session_id: &str) {
        self.state.claim_locks().acquire(ClaimLockAcquireRequest::new(
            memory_id,
            session_id,
            harness,
            Duration::from_secs(300),
        ));
    }
}

struct PeerMemorySpec<'a> {
    id: &'a str,
    summary: &'a str,
    entities: &'a [&'a str],
    harness: &'a str,
    session_id: &'a str,
}

fn peer_memory(spec: &PeerMemorySpec<'_>, device_id: &str) -> Memory {
    let authored_at = Utc.with_ymd_and_hms(2026, 5, 1, 15, 23, 0).single().expect("fixture timestamp");
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
            entities: spec
                .entities
                .iter()
                .map(|entity| Entity { id: (*entity).to_owned(), label: (*entity).to_owned(), aliases: Vec::new() })
                .collect(),
            aliases: Vec::new(),
            source: Source {
                kind: SourceKind::AgentPrimary,
                reference: None,
                harness: Some(spec.harness.to_owned()),
                harness_version: None,
                session_id: Some(spec.session_id.to_owned()),
                subagent_id: None,
                device: Some(device_id.to_owned()),
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
                policy_applied: "stream-i-coordination-test".to_owned(),
                expected_base_hash: None,
            },
            merge_diagnostics: None,
            abstraction: None,
            cues: Vec::new(),
            extras: Default::default(),
        },
        body: format!("raw body secret for {} that must stay out of peer XML", spec.id),
        path: Some(RepoPath::new(format!("agent/patterns/{}.md", spec.id))),
    }
}

fn presence_record(harness: &str, session_id: &str, device_id: &str) -> PresenceRecord {
    presence_record_with_entities(
        harness,
        session_id,
        device_id,
        &["ent_stream_i_presence".to_owned(), "ent_coordination".to_owned()],
    )
}

fn presence_record_with_entities<I, S>(harness: &str, session_id: &str, device_id: &str, entities: I) -> PresenceRecord
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    presence_record_with_entities_and_paths((harness, session_id), device_id, entities, std::iter::empty::<&str>())
}

fn presence_record_with_entities_and_paths<I, S, P, Q>(
    peer: (&str, &str),
    device_id: &str,
    entities: I,
    paths: P,
) -> PresenceRecord
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
    P: IntoIterator<Item = Q>,
    Q: AsRef<str>,
{
    let (harness, session_id) = peer;
    PresenceRecord {
        session_id: session_id.to_owned(),
        device_id: Some(device_id.to_owned()),
        harness: harness.to_owned(),
        project_binding: None,
        namespace: "agent".to_owned(),
        salient_entities: entities.into_iter().map(|entity| entity.as_ref().to_owned()).collect(),
        salient_paths: paths.into_iter().map(|path| path.as_ref().to_owned()).collect(),
        capabilities: Vec::new(),
        started_at: Some(Utc.with_ymd_and_hms(2026, 5, 1, 14, 2, 0).unwrap()),
        last_heartbeat_at: Instant::now(),
        claim_locks_held: Vec::new(),
    }
}

fn message_for(memory_id: &str, entity_id: &str) -> String {
    format!("{entity_id} agent/patterns/{memory_id}.md")
}

fn assert_in_order(value: &str, needles: &[&str]) {
    let mut cursor = 0;
    for needle in needles {
        let position = value[cursor..]
            .find(needle)
            .unwrap_or_else(|| panic!("missing expected fragment {needle:?} after byte {cursor} in {value}"));
        cursor += position + needle.len();
    }
}

fn count_occurrences(value: &str, needle: &str) -> usize {
    value.match_indices(needle).count()
}
