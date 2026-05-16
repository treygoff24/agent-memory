use std::process::Command;

use memory_substrate::{
    Author, AuthorKind, ClassificationOutcome, EventContext, Frontmatter, InitOptions, Memory, MemoryId, MemoryStatus,
    MemoryType, RetrievalPolicy, Roots, Scope, Sensitivity, Source, SourceKind, Substrate, TrustLevel, WriteMode,
    WritePolicy, WriteRequest,
};
use memoryd::handlers::handle_request;
use memoryd::protocol::{RequestEnvelope, RequestPayload, ResponsePayload, ResponseResult, MAX_FRAME_BYTES};

#[tokio::test]
async fn review_queue_returns_quarantined_memories_from_substrate() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = init_substrate(roots).await;
    let quarantined = review_memory();

    substrate
        .write_memory(WriteRequest {
            operation_id: None,
            memory: quarantined.clone(),
            expected_base_hash: None,
            write_mode: WriteMode::CreateNew,
            index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Trusted,
        })
        .await
        .expect("write quarantined memory");

    let response = handle_request(
        &substrate,
        RequestEnvelope::new("req-review-queue", RequestPayload::ReviewQueue { limit: Some(10) }),
    )
    .await;

    let ResponseResult::Success(ResponsePayload::ReviewQueue(queue)) = response.result else {
        panic!("expected review queue success, got {:?}", response.result);
    };
    assert_eq!(queue.items.len(), 1);
    assert_eq!(queue.items[0].id, quarantined.frontmatter.id.as_str());
    assert_eq!(queue.items[0].summary, quarantined.frontmatter.summary);
    assert_eq!(queue.items[0].status, "quarantined");
    assert_eq!(queue.items[0].policy_applied, "governance-quarantine-v1");
    assert_eq!(queue.items[0].reason.as_deref(), Some("contradiction requires review"));
    assert_eq!(queue.items[0].next_actions, ["review_approve", "review_reject"]);
    assert_eq!(queue.items[0].body, quarantined.body);
    assert!(!queue.items[0].body_truncated);
}

#[tokio::test]
async fn review_decision_rejects_active_memory_without_mutating_it() {
    for (decision, id) in
        [("approve", "mem_20260429_a1b2c3d4e5f60718_800002"), ("reject", "mem_20260429_a1b2c3d4e5f60718_800003")]
    {
        let temp = tempfile::tempdir().expect("tempdir");
        let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
        let substrate = init_substrate(roots).await;
        let active = active_memory_with_id(id);
        write_test_memory(&substrate, active.clone()).await;

        let request = match decision {
            "approve" => RequestPayload::ReviewApprove { id: id.to_string() },
            "reject" => RequestPayload::ReviewReject { id: id.to_string(), reason: "not appropriate".to_string() },
            _ => unreachable!("known review decision"),
        };
        let response = handle_request(&substrate, RequestEnvelope::new("req-review-decision", request)).await;
        let ResponseResult::Error(error) = response.result else {
            panic!("expected invalid_request, got {:?}", response.result);
        };
        assert_eq!(error.code, "invalid_request");

        let saved = substrate.read_memory(&MemoryId::new(id)).await.expect("active memory remains readable");
        assert_eq!(saved, active, "{decision} must not mutate active memories");
    }
}

#[tokio::test]
async fn review_queue_response_is_frame_bounded_for_oversized_review_fields() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = init_substrate(roots).await;
    let mut oversized = review_memory();
    oversized.frontmatter.summary = "s".repeat(280);
    oversized.frontmatter.write_policy.policy_applied = "p".repeat(MAX_FRAME_BYTES);
    oversized
        .frontmatter
        .extras
        .insert("governance_reason".to_string(), serde_json::json!("r".repeat(MAX_FRAME_BYTES)));
    write_test_memory(&substrate, oversized).await;

    let response = handle_request(
        &substrate,
        RequestEnvelope::new("req-review-queue-oversized", RequestPayload::ReviewQueue { limit: Some(10) }),
    )
    .await;

    let line = response.to_json_line().expect("response serializes");
    assert!(
        line.len() <= MAX_FRAME_BYTES,
        "review queue response frame was {} bytes, expected <= {MAX_FRAME_BYTES}",
        line.len()
    );
    let ResponseResult::Success(ResponsePayload::ReviewQueue(queue)) = response.result else {
        panic!("expected review queue success");
    };
    assert_eq!(queue.items.len(), 1);
    assert_eq!(queue.items[0].summary.len(), 280);
    assert!(queue.items[0].reason.as_deref().expect("reason").len() < MAX_FRAME_BYTES / 2);
}

#[tokio::test]
async fn review_queue_response_drops_items_to_fit_aggregate_frame_budget() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = init_substrate(roots).await;
    let total_items = 100usize;

    for index in 0..total_items {
        let id = format!("mem_20260429_a1b2c3d4e5f60718_{:06}", 810_000 + index);
        let mut memory = review_memory_with_id(&id);
        memory.frontmatter.summary = format!("{} {index:03}", "s".repeat(508));
        memory.frontmatter.write_policy.policy_applied = "p".repeat(512);
        memory.frontmatter.extras.insert("governance_reason".to_string(), serde_json::json!("r".repeat(512)));
        memory.body = format!("{}tail-not-rendered-{index:03}", "b".repeat(1024));
        write_test_memory(&substrate, memory).await;
    }

    let expected_prefix = memory_substrate::tree::relative_memory_paths(substrate.roots().repo.as_path())
        .into_iter()
        .map(|path| path.file_stem().expect("memory filename").to_string_lossy().to_string())
        .collect::<Vec<_>>();
    assert_eq!(expected_prefix.len(), total_items);

    let response = handle_request(
        &substrate,
        RequestEnvelope::new("req-review-queue-aggregate-budget", RequestPayload::ReviewQueue { limit: Some(100) }),
    )
    .await;

    let line = response.to_json_line().expect("response serializes");
    assert!(
        line.len() <= MAX_FRAME_BYTES,
        "review queue response frame was {} bytes, expected <= {MAX_FRAME_BYTES}",
        line.len()
    );
    let ResponseResult::Success(ResponsePayload::ReviewQueue(queue)) = response.result else {
        panic!("expected review queue success");
    };
    assert!(!queue.items.is_empty());
    assert!(
        queue.items.len() < total_items,
        "aggregate frame budget should drop excess review items, returned {}",
        queue.items.len()
    );
    let returned_ids = queue.items.iter().map(|item| item.id.clone()).collect::<Vec<_>>();
    assert_eq!(returned_ids.as_slice(), &expected_prefix[..returned_ids.len()]);
}

#[tokio::test]
async fn review_queue_marks_truncated_bodies() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = init_substrate(roots).await;
    let mut oversized = review_memory();
    oversized.body = format!("{}{}", "x".repeat(1024), "tail-not-rendered");
    write_test_memory(&substrate, oversized).await;

    let response = handle_request(
        &substrate,
        RequestEnvelope::new("req-review-queue-body-truncated", RequestPayload::ReviewQueue { limit: Some(10) }),
    )
    .await;

    let ResponseResult::Success(ResponsePayload::ReviewQueue(queue)) = response.result else {
        panic!("expected review queue success");
    };
    assert_eq!(queue.items.len(), 1);
    assert_eq!(queue.items[0].body.len(), 1024);
    assert!(queue.items[0].body_truncated);
    assert!(!queue.items[0].body.contains("tail-not-rendered"));
}

#[test]
fn review_help_is_available_without_daemon() {
    let output = Command::new(env!("CARGO_BIN_EXE_memoryd"))
        .args(["review", "--help"])
        .output()
        .expect("run memoryd review --help");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("help is utf8");
    assert!(stdout.contains("queue"));
    assert!(stdout.contains("approve"));
    assert!(stdout.contains("reject"));
}

#[test]
fn mcp_manifest_still_excludes_admin_review_tools() {
    let manifest = memoryd::mcp::manifest();

    for admin_tool in ["memory_review_queue", "memory_review_approve", "memory_review_reject"] {
        assert!(
            manifest.tools.iter().all(|tool| tool.name != admin_tool),
            "admin review tool leaked into MCP manifest: {admin_tool}"
        );
    }
}

async fn init_substrate(roots: Roots) -> Substrate {
    Substrate::init(
        roots,
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_reviewqueue".to_string()) },
    )
    .await
    .expect("init substrate")
}

async fn write_test_memory(substrate: &Substrate, memory: Memory) {
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
        .expect("write test memory");
}

fn active_memory_with_id(id: &str) -> Memory {
    let mut memory = review_memory();
    memory.frontmatter.id = MemoryId::new(id);
    memory.frontmatter.status = MemoryStatus::Active;
    memory.frontmatter.trust_level = TrustLevel::Trusted;
    memory.frontmatter.requires_user_confirmation = false;
    memory.frontmatter.review_state = None;
    memory.frontmatter.write_policy.human_review_required = false;
    memory.frontmatter.summary = "active memory must not be review-mutated".to_string();
    memory.path =
        Some(memory_substrate::RepoPath::new(format!("agent/patterns/{}.md", memory.frontmatter.id.as_str())));
    memory
}

fn review_memory() -> Memory {
    review_memory_with_id("mem_20260429_a1b2c3d4e5f60718_800001")
}

fn review_memory_with_id(id: &str) -> Memory {
    let id = MemoryId::new(id);
    let now = chrono::DateTime::parse_from_rfc3339("2026-04-29T12:00:00Z")
        .expect("fixed test date")
        .with_timezone(&chrono::Utc);
    Memory {
        frontmatter: Frontmatter {
            schema_version: memory_substrate::SUBSTRATE_SCHEMA_VERSION,
            id: id.clone(),
            memory_type: MemoryType::Pattern,
            scope: Scope::Agent,
            summary: "quarantined review memory".to_string(),
            confidence: 0.2,
            original_confidence: None,
            trust_level: TrustLevel::Quarantined,
            sensitivity: Sensitivity::Internal,
            status: MemoryStatus::Quarantined,
            created_at: now,
            updated_at: now,
            observed_at: None,
            author: Author {
                kind: AuthorKind::System,
                user_handle: None,
                harness: None,
                harness_version: None,
                session_id: None,
                subagent_id: None,
                phase: None,
                component: Some("memoryd-review-test".to_string()),
            },
            namespace: None,
            canonical_namespace_id: None,
            tags: vec!["quarantine".to_string()],
            entities: Vec::new(),
            aliases: Vec::new(),
            source: Source {
                kind: SourceKind::Import,
                reference: Some("review-test".to_string()),
                harness: None,
                harness_version: None,
                session_id: None,
                subagent_id: None,
                device: None,
            },
            evidence: Vec::new(),
            requires_user_confirmation: false,
            review_state: Some("quarantined".to_string()),
            supersedes: Vec::new(),
            superseded_by: Vec::new(),
            related: Vec::new(),
            tombstone_events: Vec::new(),
            retrieval_policy: RetrievalPolicy {
                passive_recall: false,
                max_scope: Scope::Agent,
                mask_personal_for_synthesis: false,
                index_body: false,
                index_embeddings: false,
            },
            write_policy: WritePolicy {
                human_review_required: true,
                policy_applied: "governance-quarantine-v1".to_string(),
                expected_base_hash: None,
            },
            merge_diagnostics: Some(serde_json::json!({
                "human_reason": "contradiction requires review",
                "preserved_sources": [],
                "lifecycle_notes": [],
                "evidence_near_duplicates": []
            })),
            extras: [("governance_reason".to_string(), serde_json::json!("contradiction requires review"))]
                .into_iter()
                .collect(),
        },
        body: "This quarantined memory should appear in the admin review queue.".to_string(),
        path: Some(memory_substrate::RepoPath::new(format!("agent/patterns/{}.md", id.as_str()))),
    }
}
