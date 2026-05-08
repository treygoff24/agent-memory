use chrono::TimeZone;
use memorum_coordination::claim_lock::{ClaimLockAcquireRequest, ClaimLockRegistry};
use memory_substrate::{
    Author, AuthorKind, ClassificationOutcome, EventContext, Frontmatter, InitOptions, Memory, MemoryId, MemoryStatus,
    MemoryType, RepoPath, RetrievalPolicy, Roots, Scope, Sensitivity, Source, SourceKind, Substrate, TrustLevel,
    WriteMode, WritePolicy, WriteRequest,
};
use memoryd::trust_artifact::TrustArtifactBuilder;
use serde_json::json;
use std::time::Duration;

#[tokio::test]
async fn trust_artifact_does_not_emit_stream_i_placeholder_claim_lock_status() {
    let fixture = Fixture::new().await;
    let artifact = TrustArtifactBuilder::new(&fixture.substrate).build(&fixture.memory_id).await.expect("artifact");

    assert_eq!(artifact.sync_state.claim_lock_status, None);
}

#[tokio::test]
async fn trust_artifact_reports_active_claim_lock_holder_and_expiry() {
    let fixture = Fixture::new().await;
    let registry = ClaimLockRegistry::new();
    registry.acquire(ClaimLockAcquireRequest::new(
        fixture.memory_id.as_str(),
        "sess_active_lock",
        "codex",
        Duration::from_secs(300),
    ));

    let artifact = TrustArtifactBuilder::new(&fixture.substrate)
        .with_claim_locks(&registry)
        .build(&fixture.memory_id)
        .await
        .expect("artifact");
    let status = artifact.sync_state.claim_lock_status.expect("active claim lock status");

    assert!(status.contains("codex:sess_active_lock"));
    assert!(status.contains("until"));
    assert!(!status.contains("Stream I not active"));
}

struct Fixture {
    _temp: tempfile::TempDir,
    substrate: Substrate,
    memory_id: MemoryId,
}

impl Fixture {
    async fn new() -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
        let substrate = Substrate::init(
            roots,
            InitOptions { force_unsafe_durability: true, device_id: Some("dev_trustclaimlock".to_string()) },
        )
        .await
        .expect("init");
        let memory_id = MemoryId::new("mem_20260507_a1b2c3d4e5f60718_000001");
        let memory = sample_memory(&memory_id);
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
        Self { _temp: temp, substrate, memory_id }
    }
}

fn sample_memory(id: &MemoryId) -> Memory {
    let now = chrono::Utc.with_ymd_and_hms(2026, 5, 7, 12, 0, 0).single().expect("fixture time");
    Memory {
        frontmatter: Frontmatter {
            schema_version: 1,
            id: id.clone(),
            memory_type: MemoryType::Pattern,
            scope: Scope::Agent,
            summary: "claim-lock placeholder regression".to_string(),
            confidence: 1.0,
            original_confidence: Some(1.0),
            trust_level: TrustLevel::Trusted,
            sensitivity: Sensitivity::Internal,
            status: MemoryStatus::Active,
            created_at: now,
            updated_at: now,
            observed_at: None,
            author: Author {
                kind: AuthorKind::Agent,
                user_handle: None,
                harness: Some("codex".to_string()),
                harness_version: None,
                session_id: Some("sess_claimlocktest".to_string()),
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
                reference: Some("codex".to_string()),
                harness: Some("codex".to_string()),
                harness_version: None,
                session_id: Some("sess_claimlocktest".to_string()),
                subagent_id: None,
                device: Some("dev_trustclaimlock".to_string()),
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
                policy_applied: "agent-strict@v3".to_string(),
                expected_base_hash: None,
            },
            merge_diagnostics: None,
            extras: [("privacy_scan".to_string(), json!({"labels_detected": ["none"], "storage_action": "plaintext"}))]
                .into_iter()
                .collect(),
        },
        body: "No placeholder claim-lock status should be emitted.".to_string(),
        path: Some(RepoPath::new(format!("agent/patterns/{}.md", id.as_str()))),
    }
}
