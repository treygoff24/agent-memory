use std::process::Command;

use memory_substrate::{
    Author, AuthorKind, ClassificationOutcome, EventContext, Frontmatter, InitOptions, Memory, MemoryId, MemoryStatus,
    MemoryType, RetrievalPolicy, Roots, Scope, Sensitivity, Source, SourceKind, Substrate, TrustLevel, WriteMode,
    WritePolicy, WriteRequest,
};
use memoryd::handlers::handle_request;
use memoryd::protocol::{
    RequestEnvelope, RequestPayload, ResponsePayload, ResponseResult, ReviewStatus, MAX_FRAME_BYTES,
};

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
    assert_eq!(queue.items[0].status, ReviewStatus::Quarantined.as_str());
    assert_eq!(queue.items[0].policy_applied, "governance-quarantine-v1");
    assert_eq!(queue.items[0].reason.as_deref(), Some("contradiction requires review"));
    assert_eq!(queue.items[0].next_actions, ["review_approve", "review_reject"]);
}

#[tokio::test]
async fn review_queue_bounded_page_is_a_stable_id_prefix_not_a_newest_first_window() {
    // Regression guard for the index-backed `review_queue` ordering. The path it
    // replaced built the full queue from a filesystem walk (id/path order, not
    // `updated_at`) and truncated, so the bounded page is a stable prefix that
    // does not reshuffle as memories are touched. Seed more qualifying items than
    // `limit`, deliberately making the highest-id items the *most recently
    // updated*, so a `ORDER BY updated_at DESC` page would return a different
    // (newer) subset and starve the oldest-id pending items off the page.
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = init_substrate(roots).await;

    const SEEDED: usize = 6;
    const LIMIT: usize = 3;
    let base = chrono::DateTime::parse_from_rfc3339("2026-04-29T12:00:00Z")
        .expect("fixed test date")
        .with_timezone(&chrono::Utc);

    let mut ids: Vec<String> = Vec::new();
    for index in 0..SEEDED {
        // Distinct, lexicographically ascending ids. Higher-id memories get a
        // newer `updated_at`, so id-ascending and updated_at-descending orders
        // are maximally divergent on the bounded page.
        let id = format!("mem_20260429_a1b2c3d4e5f60718_80{:04}", 1000 + index);
        let updated_at = base + chrono::Duration::minutes(index as i64);
        let mut memory = review_memory();
        memory.frontmatter.id = MemoryId::new(&id);
        memory.frontmatter.updated_at = updated_at;
        memory.path =
            Some(memory_substrate::RepoPath::new(format!("agent/patterns/{}.md", memory.frontmatter.id.as_str())));
        write_test_memory(&substrate, memory).await;
        ids.push(id);
    }
    ids.sort();

    let response = handle_request(
        &substrate,
        RequestEnvelope::new("req-review-queue-prefix", RequestPayload::ReviewQueue { limit: Some(LIMIT) }),
    )
    .await;

    let ResponseResult::Success(ResponsePayload::ReviewQueue(queue)) = response.result else {
        panic!("expected review queue success, got {:?}", response.result);
    };

    let page_ids: Vec<String> = queue.items.iter().map(|item| item.id.clone()).collect();
    // The page must be the first `LIMIT` ids in stable ascending-id order — the
    // oldest-id pending items, not the newest-updated ones.
    assert_eq!(page_ids, ids[..LIMIT].to_vec(), "bounded page must be the stable ascending-id prefix");
}

#[tokio::test]
async fn review_decision_rejects_active_memory_without_mutating_it() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = init_substrate(roots).await;
    let active = active_memory();
    let id = active.frontmatter.id.as_str().to_string();
    write_test_memory(&substrate, active.clone()).await;

    for request in [
        RequestPayload::ReviewApprove { id: id.clone() },
        RequestPayload::ReviewReject { id: id.clone(), reason: "not appropriate".to_string() },
    ] {
        let response = handle_request(&substrate, RequestEnvelope::new("req-review-decision", request)).await;
        let ResponseResult::Error(error) = response.result else {
            panic!("expected invalid_request, got {:?}", response.result);
        };
        assert_eq!(error.code, "invalid_request");

        let saved = substrate.read_memory(&MemoryId::new(&id)).await.expect("active memory remains readable");
        assert_eq!(saved.frontmatter.status, MemoryStatus::Active);
        assert_eq!(saved.frontmatter.trust_level, TrustLevel::Trusted);
        assert_eq!(saved.frontmatter.requires_user_confirmation, active.frontmatter.requires_user_confirmation);
        assert_eq!(
            saved.frontmatter.write_policy.human_review_required,
            active.frontmatter.write_policy.human_review_required
        );
        assert_eq!(saved.frontmatter.review_state, active.frontmatter.review_state);
    }
}

#[tokio::test]
async fn review_approve_promotes_governance_quarantine_and_restores_retrieval() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = init_substrate(roots).await;
    let quarantined = review_memory();
    let id = quarantined.frontmatter.id.as_str().to_string();
    write_test_memory(&substrate, quarantined).await;

    let response = handle_request(
        &substrate,
        RequestEnvelope::new("req-review-approve-quarantine", RequestPayload::ReviewApprove { id: id.clone() }),
    )
    .await;
    let ResponseResult::Success(ResponsePayload::ReviewApprove(decision)) = response.result else {
        panic!("expected approve success for governance quarantine, got {:?}", response.result);
    };
    assert_eq!(decision.status, "approved");

    let saved = substrate.read_memory(&MemoryId::new(&id)).await.expect("promoted memory readable");
    assert_eq!(saved.frontmatter.status, MemoryStatus::Active);
    assert_eq!(saved.frontmatter.trust_level, TrustLevel::Trusted);
    assert_eq!(saved.frontmatter.review_state, None);
    assert!(!saved.frontmatter.requires_user_confirmation);
    assert!(!saved.frontmatter.write_policy.human_review_required);
    // The quarantine suppressed the retrieval surface at write time; promotion
    // must restore it or the memory stays invisible to recall forever.
    assert!(saved.frontmatter.retrieval_policy.passive_recall);
    assert!(saved.frontmatter.retrieval_policy.index_body);
    assert!(saved.frontmatter.retrieval_policy.index_embeddings);
    // Quarantine artifacts are dropped on promotion.
    assert_eq!(saved.frontmatter.merge_diagnostics, None);
    assert!(!saved.frontmatter.extras.contains_key("governance_reason"));
}

#[tokio::test]
async fn review_approve_still_refuses_merge_quarantine() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = init_substrate(roots).await;
    let mut merge_quarantined = review_memory();
    merge_quarantined.frontmatter.merge_diagnostics = Some(serde_json::json!([{
        "merge_id": "merge_test",
        "created_at": "2026-05-08T12:00:00Z",
        "status": "quarantined",
        "conflicting_fields": ["body"],
        "human_reason": "body diff3 conflict"
    }]));
    let id = merge_quarantined.frontmatter.id.as_str().to_string();
    write_test_memory(&substrate, merge_quarantined).await;

    let response = handle_request(
        &substrate,
        RequestEnvelope::new("req-review-approve-merge", RequestPayload::ReviewApprove { id: id.clone() }),
    )
    .await;
    let ResponseResult::Error(error) = response.result else {
        panic!("expected invalid_request for merge quarantine, got {:?}", response.result);
    };
    assert_eq!(error.code, "invalid_request");
    assert!(
        error.message.contains("quarantine resolve"),
        "error should route operator to quarantine resolve: {}",
        error.message
    );

    let saved = substrate.read_memory(&MemoryId::new(&id)).await.expect("merge quarantine remains readable");
    assert_eq!(saved.frontmatter.status, MemoryStatus::Quarantined);
    assert_eq!(saved.frontmatter.trust_level, TrustLevel::Quarantined);
}

#[tokio::test]
async fn review_reject_archives_quarantined_memory_as_untrusted() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = init_substrate(roots).await;
    let quarantined = review_memory();
    let id = quarantined.frontmatter.id.as_str().to_string();
    write_test_memory(&substrate, quarantined).await;

    let response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "req-review-reject-quarantine",
            RequestPayload::ReviewReject { id: id.clone(), reason: "not appropriate".to_string() },
        ),
    )
    .await;
    let ResponseResult::Success(ResponsePayload::ReviewReject(decision)) = response.result else {
        panic!("expected reject success for quarantined memory, got {:?}", response.result);
    };
    assert_eq!(decision.status, "rejected");

    let saved = substrate.read_memory(&MemoryId::new(&id)).await.expect("rejected memory readable");
    assert_eq!(saved.frontmatter.status, MemoryStatus::Archived);
    // (Archived, Quarantined) is an invalid lifecycle pair — reject normalizes to Untrusted.
    assert_eq!(saved.frontmatter.trust_level, TrustLevel::Untrusted);
    assert_eq!(saved.frontmatter.review_state.as_deref(), Some("rejected"));
}

#[tokio::test]
async fn review_reject_still_refuses_secret_bearing_reason() {
    // SEC-001 regression guard: the rejection reason passes the privacy scanner and a
    // secret span refuses the write. (The namespace floor must NOT do this job — `Me`
    // floors at EncryptAtRest and refused every reject unconditionally.)
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = init_substrate(roots).await;
    let quarantined = review_memory();
    let id = quarantined.frontmatter.id.as_str().to_string();
    write_test_memory(&substrate, quarantined).await;

    let response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "req-review-reject-secret",
            RequestPayload::ReviewReject { id: id.clone(), reason: "leaked key AKIA1234567890ABCDEF".to_string() },
        ),
    )
    .await;
    let ResponseResult::Error(error) = response.result else {
        panic!("expected refusal for secret-bearing rejection reason, got {:?}", response.result);
    };
    assert_eq!(error.code, "invalid_request");

    let saved = substrate.read_memory(&MemoryId::new(&id)).await.expect("memory unchanged");
    assert_eq!(saved.frontmatter.status, MemoryStatus::Quarantined);
    assert!(!saved.frontmatter.extras.contains_key("review_rejection_reason"));
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

fn active_memory() -> Memory {
    let mut memory = review_memory();
    memory.frontmatter.id = MemoryId::new("mem_20260429_a1b2c3d4e5f60718_800002");
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
    let id = MemoryId::new("mem_20260429_a1b2c3d4e5f60718_800001");
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
