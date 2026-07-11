use chrono::{DateTime, TimeZone, Utc};
use memory_substrate::events::{append_event, Event, EventKind, EVENT_SCHEMA_VERSION};
use memory_substrate::{
    Author, AuthorKind, ClassificationOutcome, DeviceId, EventContext, EventId, Frontmatter, InitOptions, Memory,
    MemoryId, MemoryStatus, MemoryType, OperationId, RepoPath, RetrievalPolicy, Roots, Scope, Sensitivity, Source,
    SourceKind, Substrate, TrustLevel, WriteMode, WritePolicy, WriteRequest,
};
use memoryd::trust_artifact::TrustArtifactBuilder;

const TARGET_ID: &str = "mem_20260501_d1d2d3d4d5d6d7d8_000101";

#[tokio::test]
async fn trust_artifact_strength_is_gated_and_uses_configured_weights() {
    let disabled = Fixture::seeded(false).await;
    let disabled_artifact = disabled.artifact().await;
    assert_eq!(disabled_artifact.recall.strength, "");
    let disabled_json = serde_json::to_string(&disabled_artifact).expect("serialize artifact");
    assert!(!disabled_json.contains("\"strength\""), "disabled dynamics must not render strength: {disabled_json}");

    let enabled = Fixture::seeded(true).await;
    let enabled_artifact = enabled.artifact().await;
    assert_eq!(enabled_artifact.recall.strength, "1.00 (approximate; computed at render time over this memory alone)");
}

struct Fixture {
    _temp: tempfile::TempDir,
    roots: Roots,
    substrate: Substrate,
    device_id: String,
}

impl Fixture {
    async fn seeded(dynamics_enabled: bool) -> Self {
        let fixture = Self::new(if dynamics_enabled { "dev_trustdynon" } else { "dev_trustdynoff" }).await;
        fixture.write_dynamics_config(dynamics_enabled);
        fixture.write_memory(sample_memory()).await;
        fixture.append_recall_hit();
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
            "schema_version: 1\ndynamics:\n  enabled: {enabled}\n  tau_days: 7\n  weights:\n    frequency: 1.0\n    recency: 0.0\n    corroboration: 0.0\n"
        );
        std::fs::write(self.roots.repo.join("config.yaml"), body).expect("write dynamics config");
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

    fn append_recall_hit(&self) {
        let ts = Utc.with_ymd_and_hms(2026, 5, 1, 12, 0, 0).single().expect("fixture time");
        append_event(
            &self.roots.repo.join(format!("events/{}.jsonl", self.device_id)),
            &Event {
                schema: EVENT_SCHEMA_VERSION,
                id: EventId::new(format!("evt_recall_{TARGET_ID}")),
                at: ts,
                device: DeviceId::new(&self.device_id),
                seq: 1,
                operation_id: Some(OperationId::new(format!("op_recall_{TARGET_ID}"))),
                kind: EventKind::RecallHit { id: MemoryId::new(TARGET_ID), recalled_at: ts },
                crc32c: 0,
            },
        )
        .expect("append recall hit");
    }

    fn reindex_events(&self) {
        self.substrate.doctor_reindex_events_log().expect("reindex events");
    }

    async fn artifact(&self) -> memoryd::trust_artifact::TrustArtifact {
        TrustArtifactBuilder::new(&self.substrate)
            .with_now(Utc.with_ymd_and_hms(2026, 5, 2, 0, 0, 0).single().expect("fixture time"))
            .build(&MemoryId::new(TARGET_ID))
            .await
            .expect("build artifact")
    }
}

fn sample_memory() -> Memory {
    let now = instant("2026-05-01T10:00:00Z");
    Memory {
        frontmatter: Frontmatter {
            schema_version: 1,
            id: MemoryId::new(TARGET_ID),
            memory_type: MemoryType::Pattern,
            scope: Scope::Project,
            summary: "Trust artifact dynamics config fixture".to_owned(),
            confidence: 0.9,
            original_confidence: None,
            trust_level: TrustLevel::Trusted,
            sensitivity: Sensitivity::Internal,
            status: MemoryStatus::Active,
            created_at: now,
            updated_at: now,
            observed_at: None,
            author: Author {
                kind: AuthorKind::Agent,
                user_handle: None,
                harness: Some("codex".to_owned()),
                harness_version: None,
                session_id: Some("sess_trustdyn".to_owned()),
                subagent_id: None,
                phase: None,
                component: Some("trust-artifact-dynamics-test".to_owned()),
            },
            namespace: Some("project:agent-memory".to_owned()),
            canonical_namespace_id: Some("agent-memory".to_owned()),
            tags: Vec::new(),
            entities: Vec::new(),
            aliases: Vec::new(),
            source: Source {
                kind: SourceKind::AgentPrimary,
                reference: None,
                harness: Some("codex".to_owned()),
                harness_version: None,
                session_id: Some("sess_trustdyn".to_owned()),
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
                max_scope: Scope::Project,
                mask_personal_for_synthesis: false,
                index_body: true,
                index_embeddings: false,
            },
            write_policy: WritePolicy {
                human_review_required: false,
                policy_applied: "test".to_owned(),
                expected_base_hash: None,
            },
            merge_diagnostics: None,
            abstraction: None,
            cues: Vec::new(),
            extras: Default::default(),
        },
        body: "Trust artifact dynamics config fixture".to_owned(),
        path: Some(RepoPath::new(format!("projects/agent-memory/{TARGET_ID}.md"))),
    }
}

fn instant(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value).expect("fixture timestamp parses").with_timezone(&Utc)
}
