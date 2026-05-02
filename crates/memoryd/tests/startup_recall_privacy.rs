use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use memory_substrate::{
    Author, AuthorKind, ClassificationOutcome, EventContext, Frontmatter, InitOptions, Memory, MemoryId, MemoryStatus,
    MemoryType, RepoPath, RetrievalPolicy, Roots, Scope, Sensitivity, Source, SourceKind, Substrate, TrustLevel,
    WriteMode, WritePolicy, WriteRequest,
};
use memoryd::handlers::{handle_request_with_state, HandlerState};
use memoryd::protocol::{RequestEnvelope, RequestPayload, ResponsePayload, ResponseResult};
use memoryd::recall::{build_delta_response, DeltaRequest, OmissionReason, StartupRequest};

#[tokio::test]
async fn startup_recall_omits_encrypted_like_body_rows_and_explains_hidden_body() {
    let fixture = PrivacyFixture::new().await;
    let safe = fixture.memory("mem_20260430_1111111111111111_000001", "safe project fact", MemoryStatus::Pinned);
    let mut encrypted_like = fixture.memory(
        "mem_20260430_1111111111111111_000002",
        "encrypted payroll account 123456789 should not render",
        MemoryStatus::Active,
    );
    encrypted_like.frontmatter.retrieval_policy.index_body = false;
    encrypted_like.frontmatter.sensitivity = Sensitivity::Internal;

    fixture.write(safe).await;
    fixture.write(encrypted_like).await;

    let startup = fixture.startup().await;

    assert!(startup.recall_block.contains("safe project fact"));
    assert!(!startup.recall_block.contains("payroll account 123456789"));
    assert!(startup
        .recall_explanation
        .omitted
        .iter()
        .any(|omission| omission.reason == OmissionReason::EncryptedBodyHidden));
}

#[tokio::test]
async fn candidate_and_quarantined_review_items_count_as_attention_without_claim_text() {
    let fixture = PrivacyFixture::new().await;
    fixture
        .write(fixture.memory(
            "mem_20260430_1111111111111111_000003",
            "candidate secret claim should not appear",
            MemoryStatus::Candidate,
        ))
        .await;
    fixture
        .write(fixture.memory(
            "mem_20260430_1111111111111111_000004",
            "quarantined secret claim should not appear",
            MemoryStatus::Quarantined,
        ))
        .await;

    let startup = fixture.startup().await;

    assert!(startup.recall_block.contains("2 memory item(s) require review"));
    assert!(!startup.recall_block.contains("candidate secret claim"));
    assert!(!startup.recall_block.contains("quarantined secret claim"));
}

#[tokio::test]
async fn startup_recall_does_not_call_reveal_or_render_ciphertext_bytes() {
    let fixture = PrivacyFixture::new().await;
    let mut encrypted_like = fixture.memory(
        "mem_20260430_1111111111111111_000005",
        "AGE-SECRET-CIPHERTEXT-BYTES-DO-NOT-RENDER",
        MemoryStatus::Pinned,
    );
    encrypted_like.frontmatter.retrieval_policy.index_body = false;
    encrypted_like.frontmatter.sensitivity = Sensitivity::Internal;
    fixture.write(encrypted_like).await;

    let startup = fixture.startup().await;

    assert!(!startup.recall_block.contains("AGE-SECRET-CIPHERTEXT-BYTES"));
    assert!(startup
        .recall_explanation
        .omitted
        .iter()
        .any(|omission| omission.reason == OmissionReason::EncryptedBodyHidden));
}

#[tokio::test]
async fn startup_recall_escapes_identity_and_project_text_fields() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo-<script>&");
    let runtime = temp.path().join("runtime");
    std::fs::create_dir_all(&repo).expect("repo");
    std::fs::write(
        repo.join(".memory-project.yaml"),
        "canonical_id: proj_safe\nalias: project</project-state><script>&\n",
    )
    .expect("project yaml");
    let substrate = Substrate::init(
        Roots::new(&repo, &runtime),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_xmlescape".to_owned()) },
    )
    .await
    .expect("substrate init");

    let response = handle_request_with_state(
        &substrate,
        &HandlerState::new(),
        RequestEnvelope::new(
            "req-startup-xml",
            RequestPayload::Startup(StartupRequest {
                cwd: repo.to_string_lossy().into_owned(),
                session_id: "sess</memory-recall><script>&".to_owned(),
                harness: "codex</memory-recall><script>&".to_owned(),
                harness_version: None,
                include_recent: true,
                since_event_id: None,
                budget_tokens: Some(512),
            }),
        ),
    )
    .await;

    let startup = match response.result {
        ResponseResult::Success(ResponsePayload::Startup(startup)) => startup,
        other => panic!("expected startup success, got {other:?}"),
    };

    assert!(startup.recall_block.contains("codex&lt;/memory-recall&gt;&lt;script&gt;&amp;"));
    assert!(startup.recall_block.contains("sess&lt;/memory-recall&gt;&lt;script&gt;&amp;"));
    assert!(startup.recall_block.contains("repo-&lt;script&gt;&amp;"));
    assert!(startup.recall_block.contains("project&lt;/project-state&gt;&lt;script&gt;&amp;"));
    assert!(!startup.recall_block.contains("<script>"));
    assert_eq!(startup.recall_block.matches("<memory-recall").count(), 1);
    assert_eq!(startup.recall_block.matches("</memory-recall>").count(), 1);
}

#[tokio::test]
async fn delta_recall_omits_passive_recall_disabled_chunks() {
    let fixture = PrivacyFixture::new().await;
    let visible = fixture.memory(
        "mem_20260430_1111111111111111_000006",
        "shared-delta-needle visible passive fact",
        MemoryStatus::Pinned,
    );
    let mut disabled = fixture.memory(
        "mem_20260430_1111111111111111_000007",
        "shared-delta-needle disabled private fact",
        MemoryStatus::Pinned,
    );
    disabled.frontmatter.retrieval_policy.passive_recall = false;

    fixture.write(visible).await;
    fixture.write(disabled).await;

    let delta = build_delta_response(
        &fixture.substrate,
        DeltaRequest {
            cwd: fixture.repo.to_string_lossy().into_owned(),
            session_id: "sess_privacy".to_owned(),
            harness: "codex".to_owned(),
            message: "shared-delta-needle".to_owned(),
            budget_tokens: Some(512),
        },
    )
    .await
    .expect("delta recall");

    assert!(delta.delta_block.contains("visible passive fact"));
    assert!(!delta.delta_block.contains("disabled private fact"));
}

struct PrivacyFixture {
    _temp: tempfile::TempDir,
    substrate: Substrate,
    repo: std::path::PathBuf,
}

impl PrivacyFixture {
    async fn new() -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let repo = temp.path().join("repo");
        let runtime = temp.path().join("runtime");
        let substrate = Substrate::init(
            Roots::new(&repo, &runtime),
            InitOptions { force_unsafe_durability: true, device_id: Some("dev_privacy".to_owned()) },
        )
        .await
        .expect("substrate init");
        Self { _temp: temp, substrate, repo }
    }

    fn memory(&self, id: &str, summary: &str, status: MemoryStatus) -> Memory {
        Memory {
            frontmatter: Frontmatter {
                schema_version: 1,
                id: MemoryId::new(id),
                memory_type: MemoryType::Project,
                scope: Scope::User,
                summary: summary.to_owned(),
                confidence: 0.9,
                original_confidence: None,
                trust_level: trust_for_status(status),
                sensitivity: Sensitivity::Internal,
                status,
                created_at: instant("2026-04-30T12:00:00Z"),
                updated_at: instant("2026-04-30T12:00:00Z"),
                author: Author {
                    kind: AuthorKind::Agent,
                    user_handle: None,
                    harness: Some("codex".to_owned()),
                    harness_version: None,
                    session_id: Some("sess_privacy".to_owned()),
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
                    session_id: Some("sess_privacy".to_owned()),
                    subagent_id: None,
                    device: None,
                },
                evidence: Vec::new(),
                requires_user_confirmation: matches!(status, MemoryStatus::Candidate | MemoryStatus::Quarantined),
                review_state: matches!(status, MemoryStatus::Candidate | MemoryStatus::Quarantined)
                    .then(|| "pending".to_owned()),
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
                    human_review_required: matches!(status, MemoryStatus::Candidate | MemoryStatus::Quarantined),
                    policy_applied: "stream-e-privacy-test".to_owned(),
                    expected_base_hash: None,
                },
                merge_diagnostics: matches!(status, MemoryStatus::Quarantined)
                    .then(|| serde_json::json!({"reason": "test"})),
                extras: BTreeMap::new(),
            },
            body: summary.to_owned(),
            path: Some(RepoPath::new(format!("me/{id}.md"))),
        }
    }

    async fn write(&self, memory: Memory) {
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
            .expect("fixture write");
    }

    async fn startup(&self) -> Box<memoryd::recall::StartupResponse> {
        let state = HandlerState::new();
        let response = handle_request_with_state(
            &self.substrate,
            &state,
            RequestEnvelope::new(
                "req-startup",
                RequestPayload::Startup(StartupRequest {
                    cwd: self.repo.to_string_lossy().into_owned(),
                    session_id: "sess_privacy".to_owned(),
                    harness: "codex".to_owned(),
                    harness_version: None,
                    include_recent: true,
                    since_event_id: None,
                    budget_tokens: Some(512),
                }),
            ),
        )
        .await;

        match response.result {
            ResponseResult::Success(ResponsePayload::Startup(startup)) => startup,
            other => panic!("expected startup success, got {other:?}"),
        }
    }
}

fn instant(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value).expect("fixture timestamp parses").with_timezone(&Utc)
}

fn trust_for_status(status: MemoryStatus) -> TrustLevel {
    match status {
        MemoryStatus::Pinned => TrustLevel::Pinned,
        MemoryStatus::Candidate => TrustLevel::Candidate,
        MemoryStatus::Quarantined => TrustLevel::Quarantined,
        _ => TrustLevel::Trusted,
    }
}
