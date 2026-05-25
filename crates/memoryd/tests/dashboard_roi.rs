use memory_substrate::{
    events::EventKind, Author, AuthorKind, ClassificationOutcome, EventContext, Frontmatter, InitOptions, Memory,
    MemoryId, MemoryStatus, MemoryType, RepoPath, RetrievalPolicy, Roots, Scope, Sensitivity, Source, SourceKind,
    Substrate, TrustLevel, WriteMode, WritePolicy, WriteRequest,
};
use memoryd::handlers::handle_request;
use memoryd::protocol::{RequestEnvelope, RequestPayload, ResponsePayload, ResponseResult};

#[tokio::test]
async fn dashboard_roi_empty_repo_returns_zero_metrics_not_fixtures() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(temp.path()).await;

    let response = handle_request(
        &substrate,
        RequestEnvelope::new("req-dashboard-roi-empty", RequestPayload::DashboardRoi { window_days: 90 }),
    )
    .await;

    let ResponseResult::Success(ResponsePayload::DashboardRoi(roi)) = response.result else {
        panic!("expected dashboard ROI success, got {:?}", response.result);
    };
    assert_eq!(roi.window_days, 90);
    assert_eq!(roi.promotion_rate, 0.0);
    assert_eq!(roi.promotion_precision, 0.0);
    assert!(roi.refusal_breakdown.is_empty());
    assert_eq!(roi.dreaming.candidates_generated, 0);
    assert_eq!(roi.dreaming.promoted_silent, 0);
    assert_eq!(roi.dreaming.entered_review_queue, 0);
    assert_eq!(roi.dreaming.dropped, 0);
    assert_eq!(roi.dreaming.review_queue_approval_rate, 0.0);
    assert_eq!(roi.reality_check_adherence.weeks_completed, 0);
    assert_eq!(roi.reality_check_adherence.weeks_skipped, 0);
}

#[tokio::test]
async fn dashboard_roi_derives_metrics_from_live_substrate_and_events() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(temp.path()).await;

    write_memory(&substrate, memory("mem_20260525_a1b2c3d4e5f60718_000001", MemoryStatus::Active, false)).await;
    write_memory(&substrate, dreaming_memory("mem_20260525_a1b2c3d4e5f60718_000002", MemoryStatus::Active, false))
        .await;
    write_memory(&substrate, dreaming_memory("mem_20260525_a1b2c3d4e5f60718_000003", MemoryStatus::Candidate, true))
        .await;
    write_memory(&substrate, memory("mem_20260525_a1b2c3d4e5f60718_000004", MemoryStatus::Candidate, true)).await;
    substrate
        .record_event_best_effort(EventKind::WriteRefused {
            id: None,
            path: None,
            classification: ClassificationOutcome::Secret,
            reason: "grounding".to_owned(),
        })
        .expect("refusal event recorded");
    substrate
        .record_event_best_effort(EventKind::RealityCheckConfirmed {
            id: MemoryId::new("mem_20260525_a1b2c3d4e5f60718_000001"),
            session_id: "rc_week_1".to_owned(),
        })
        .expect("reality check event recorded");
    substrate
        .record_event_best_effort(EventKind::RealityCheckNotRelevant {
            id: MemoryId::new("mem_20260525_a1b2c3d4e5f60718_000002"),
            session_id: "rc_week_2".to_owned(),
        })
        .expect("reality check event recorded");

    let response = handle_request(
        &substrate,
        RequestEnvelope::new("req-dashboard-roi-live", RequestPayload::DashboardRoi { window_days: 90 }),
    )
    .await;

    let ResponseResult::Success(ResponsePayload::DashboardRoi(roi)) = response.result else {
        panic!("expected dashboard ROI success, got {:?}", response.result);
    };
    assert_eq!(roi.window_days, 90);
    assert_eq!(roi.promotion_rate, 0.5);
    assert!((roi.promotion_precision - (2.0 / 3.0)).abs() < f64::EPSILON);
    assert_eq!(roi.refusal_breakdown.get("grounding"), Some(&1));
    assert_eq!(roi.dreaming.candidates_generated, 2);
    assert_eq!(roi.dreaming.promoted_silent, 1);
    assert_eq!(roi.dreaming.entered_review_queue, 1);
    assert_eq!(roi.dreaming.dropped, 0);
    assert_eq!(roi.dreaming.review_queue_approval_rate, 0.5);
    assert_eq!(roi.reality_check_adherence.weeks_completed, 2);
    assert_eq!(roi.reality_check_adherence.weeks_skipped, 0);
}

async fn init_substrate(root: &std::path::Path) -> Substrate {
    Substrate::init(
        Roots::new(root.join("repo"), root.join("runtime")),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_dashboardroi".to_owned()) },
    )
    .await
    .expect("init substrate")
}

async fn write_memory(substrate: &Substrate, memory: Memory) {
    let id = memory.frontmatter.id.as_str().to_owned();
    let status = memory.frontmatter.status;
    let result = substrate
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
        .await;
    if let Err(error) = result {
        let reindex = substrate.reindex().await;
        panic!("write memory {id} ({status:?}): {error:?}; reindex={reindex:?}");
    }
}

fn dreaming_memory(id: &str, status: MemoryStatus, human_review_required: bool) -> Memory {
    let mut memory = memory(id, status, human_review_required);
    memory.frontmatter.tags.push("dreaming".to_owned());
    memory
}

fn memory(id: &str, status: MemoryStatus, human_review_required: bool) -> Memory {
    let now = chrono::Utc::now();
    let trust_level = match status {
        MemoryStatus::Candidate => TrustLevel::Candidate,
        MemoryStatus::Quarantined => TrustLevel::Quarantined,
        MemoryStatus::Pinned => TrustLevel::Pinned,
        _ => TrustLevel::Trusted,
    };
    Memory {
        frontmatter: Frontmatter {
            schema_version: memory_substrate::SUBSTRATE_SCHEMA_VERSION,
            id: MemoryId::new(id),
            memory_type: MemoryType::Pattern,
            scope: Scope::Agent,
            summary: format!("dashboard ROI fixture {id}"),
            confidence: 0.9,
            original_confidence: None,
            trust_level,
            sensitivity: Sensitivity::Internal,
            status,
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
                component: Some("dashboard-roi-test".to_owned()),
            },
            namespace: None,
            canonical_namespace_id: None,
            tags: Vec::new(),
            entities: Vec::new(),
            aliases: Vec::new(),
            source: Source {
                kind: SourceKind::Import,
                reference: Some("dashboard-roi-test".to_owned()),
                harness: None,
                harness_version: None,
                session_id: None,
                subagent_id: None,
                device: None,
            },
            evidence: Vec::new(),
            requires_user_confirmation: human_review_required,
            review_state: human_review_required.then(|| "candidate".to_owned()),
            supersedes: Vec::new(),
            superseded_by: Vec::new(),
            related: Vec::new(),
            tombstone_events: Vec::new(),
            retrieval_policy: RetrievalPolicy {
                passive_recall: true,
                max_scope: Scope::Agent,
                mask_personal_for_synthesis: false,
                index_body: true,
                index_embeddings: true,
            },
            write_policy: WritePolicy {
                human_review_required,
                policy_applied: "dashboard-roi-test@v1".to_owned(),
                expected_base_hash: None,
            },
            merge_diagnostics: None,
            extras: Default::default(),
        },
        body: format!("dashboard ROI test memory {id}"),
        path: Some(RepoPath::new(format!("agent/patterns/{id}.md"))),
    }
}
