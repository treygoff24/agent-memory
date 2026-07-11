use chrono::{DateTime, Utc};
use memory_substrate::events::{append_event, Event, EventKind, EVENT_SCHEMA_VERSION};
use memory_substrate::{
    Author, AuthorKind, ClassificationOutcome, DeviceId, EventContext, EventId, Frontmatter, InitOptions, Memory,
    MemoryId, MemoryStatus, MemoryType, OperationId, RepoPath, RetrievalPolicy, Roots, Scope, Sensitivity, Source,
    SourceKind, Substrate, TrustLevel, WriteMode, WritePolicy, WriteRequest,
};
use memoryd::recall::{build_startup_response, RecallSectionName, StartupRequest};

#[tokio::test]
async fn startup_ranking_hydrates_strength_from_events_log_and_shifts_order() {
    let off = HydrationFixture::seeded(false).await;
    let off_response = off.startup().await;
    assert_eq!(recent_ids(&off_response), vec![LEADER_ID, FOLLOWER_ID]);
    assert!(off_response.recall_explanation.strengths.is_empty(), "dynamics off must not hydrate strengths");

    let on = HydrationFixture::seeded(true).await;
    let on_response = on.startup().await;
    assert_eq!(recent_ids(&on_response), vec![FOLLOWER_ID, LEADER_ID]);
    let follower_strength = on_response
        .recall_explanation
        .strengths
        .iter()
        .find(|entry| entry.id == FOLLOWER_ID)
        .map(|entry| entry.strength)
        .expect("follower strength surfaced");
    assert_eq!(follower_strength, 1.0, "seeded recall event should hydrate full frequency strength");
}

const LEADER_ID: &str = "mem_20260501_bbbbbbbbbbbbbbbb_000002";
const FOLLOWER_ID: &str = "mem_20260501_aaaaaaaaaaaaaaaa_000001";

struct HydrationFixture {
    _temp: tempfile::TempDir,
    roots: Roots,
    substrate: Substrate,
    device_id: String,
}

impl HydrationFixture {
    async fn seeded(dynamics_enabled: bool) -> Self {
        let fixture = Self::new(if dynamics_enabled { "dev_hydrationon" } else { "dev_hydrationoff" }).await;
        fixture.write_dynamics_config(dynamics_enabled);
        fixture
            .write_memory(fixture.memory(HydrationMemorySpec {
                id: LEADER_ID,
                summary: "Structurally stronger user memory without recall usage.",
                scope: Scope::User,
                path: "me/mem_20260501_bbbbbbbbbbbbbbbb_000002.md",
                confidence: 0.6,
                updated_at: "2026-04-20T12:00:00Z",
            }))
            .await;
        fixture
            .write_memory(fixture.memory(HydrationMemorySpec {
                id: FOLLOWER_ID,
                summary: "Frequently used agent memory should win only with dynamics on.",
                scope: Scope::Agent,
                path: "agent/patterns/mem_20260501_aaaaaaaaaaaaaaaa_000001.md",
                confidence: 0.0,
                updated_at: "2026-04-30T11:00:00Z",
            }))
            .await;
        fixture.append_recall_hit(FOLLOWER_ID, "2026-04-30T11:00:00Z");
        fixture.reindex_events();
        fixture
    }

    async fn new(device_id: &str) -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
        let substrate = Substrate::init(
            roots.clone(),
            InitOptions { force_unsafe_durability: true, device_id: Some(device_id.to_owned()) },
        )
        .await
        .expect("substrate init");
        Self { _temp: temp, roots, substrate, device_id: device_id.to_owned() }
    }

    fn write_dynamics_config(&self, enabled: bool) {
        let body = format!(
            "schema_version: 1\ndynamics:\n  enabled: {enabled}\n  alpha_points: 12\n  weights:\n    frequency: 1.0\n    recency: 0.0\n    corroboration: 0.0\n"
        );
        std::fs::write(self.roots.repo.join("config.yaml"), body).expect("write dynamics config");
    }

    async fn startup(&self) -> memoryd::recall::StartupResponse {
        build_startup_response(
            &self.substrate,
            StartupRequest {
                cwd: self.roots.repo.to_string_lossy().into_owned(),
                session_id: "sess_hydration".to_owned(),
                harness: "codex".to_owned(),
                harness_version: None,
                include_recent: true,
                since_event_id: None,
                budget_tokens: Some(1024),
                passive: false,
            },
        )
        .await
        .expect("startup recall")
    }

    async fn write_memory(&self, memory: Memory) {
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
            .expect("write memory");
    }

    fn append_recall_hit(&self, memory_id: &str, timestamp: &str) {
        let ts = instant(timestamp);
        append_event(
            &self.roots.repo.join(format!("events/{}.jsonl", self.device_id)),
            &Event {
                schema: EVENT_SCHEMA_VERSION,
                id: EventId::new(format!("evt_recall_{memory_id}")),
                at: ts,
                device: DeviceId::new(&self.device_id),
                seq: 1,
                operation_id: Some(OperationId::new(format!("op_recall_{memory_id}"))),
                kind: EventKind::RecallHit { id: MemoryId::new(memory_id), recalled_at: ts },
                crc32c: 0,
            },
        )
        .expect("append recall hit");
    }

    fn reindex_events(&self) {
        self.substrate.doctor_reindex_events_log().expect("reindex events");
    }

    fn memory(&self, spec: HydrationMemorySpec<'_>) -> Memory {
        let updated_at = instant(spec.updated_at);
        Memory {
            frontmatter: Frontmatter {
                schema_version: 1,
                id: MemoryId::new(spec.id),
                memory_type: MemoryType::Pattern,
                scope: spec.scope,
                summary: spec.summary.to_owned(),
                confidence: spec.confidence,
                original_confidence: None,
                trust_level: TrustLevel::Trusted,
                sensitivity: Sensitivity::Internal,
                status: MemoryStatus::Active,
                created_at: updated_at,
                updated_at,
                observed_at: None,
                author: Author {
                    kind: AuthorKind::Agent,
                    user_handle: None,
                    harness: Some("codex".to_owned()),
                    harness_version: None,
                    session_id: Some("sess_hydration".to_owned()),
                    subagent_id: None,
                    phase: None,
                    component: Some("startup-hydration-test".to_owned()),
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
                    session_id: Some("sess_hydration".to_owned()),
                    subagent_id: None,
                    device: Some(self.device_id.clone()),
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
                    max_scope: spec.scope,
                    mask_personal_for_synthesis: false,
                    index_body: true,
                    index_embeddings: false,
                },
                write_policy: WritePolicy {
                    human_review_required: false,
                    policy_applied: "startup-hydration-test".to_owned(),
                    expected_base_hash: None,
                },
                merge_diagnostics: None,
                abstraction: None,
                cues: Vec::new(),
                extras: Default::default(),
            },
            body: spec.summary.to_owned(),
            path: Some(RepoPath::new(spec.path)),
        }
    }
}

struct HydrationMemorySpec<'a> {
    id: &'a str,
    summary: &'a str,
    scope: Scope,
    path: &'a str,
    confidence: f64,
    updated_at: &'a str,
}

fn recent_ids(response: &memoryd::recall::StartupResponse) -> Vec<&str> {
    response
        .recall_explanation
        .sections
        .iter()
        .find(|section| section.name == RecallSectionName::RecentMemory)
        .expect("recent-memory section")
        .selected_ids
        .iter()
        .map(String::as_str)
        .collect()
}

fn instant(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value).expect("fixture timestamp parses").with_timezone(&Utc)
}
