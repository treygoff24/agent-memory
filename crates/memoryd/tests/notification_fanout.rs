use std::path::Path;
use std::time::Duration as StdDuration;

use chrono::{Duration, TimeZone, Utc};
use memory_governance::review::{over_threshold, REVIEW_QUEUE_DOGFOOD_THRESHOLD};
use memory_governance::{ReviewQueue, ReviewQueueItem, ReviewStatus};
use memory_substrate::frontmatter;
use memory_substrate::{
    Author, AuthorKind, Frontmatter, InitOptions, Memory, MemoryId, MemoryStatus, MemoryType, RetrievalPolicy, Roots,
    Scope, Sensitivity, Source, SourceKind, Substrate, TrustLevel, WritePolicy,
};
use memoryd::client;
use memoryd::handlers::handle_request;
use memoryd::notifications::config::NotificationConfig;
use memoryd::notifications::dispatcher::NotificationDispatcher;
use memoryd::notifications::external::ExternalNotifier;
use memoryd::notifications::os::OsNotifier;
use memoryd::notifications::passive::PassiveQueue;
use memoryd::notifications::triggers::{notification_for, EventKind};
use memoryd::protocol::{
    NotificationEvent, QuarantineResolutionMode, RequestEnvelope, RequestPayload, ResponsePayload, ResponseResult,
    StatusResponse,
};
use memoryd::reality_check::RcScheduler;
use memoryd::state::RealityCheckState;
use tokio::time::sleep;

mod common;
use common::{shutdown, spawn_daemon, unique_socket_path, wait_for_socket};

#[tokio::test]
async fn dispatcher_passively_records_five_dogfood_notification_variants() {
    let passive = PassiveQueue::new();
    let dispatcher = NotificationDispatcher::new(
        passive.clone(),
        NotificationConfig::default(),
        OsNotifier::disabled(),
        ExternalNotifier::disabled(),
    );

    for event in dogfood_events() {
        dispatcher.dispatch_event(event).await;
    }

    let messages = passive.messages();
    assert_eq!(messages.len(), 5);
    assert!(messages.iter().any(|message| message.contains("merge conflict")), "{messages:#?}");
    assert!(messages.iter().any(|message| message.contains("Dream run completed")), "{messages:#?}");
    assert!(messages.iter().any(|message| message.contains("Daily synthesis")), "{messages:#?}");
    assert!(messages.iter().any(|message| message.contains("Review queue")), "{messages:#?}");
    assert!(messages.iter().any(|message| message.contains("Reality Check is overdue")), "{messages:#?}");
}

#[test]
fn review_queue_threshold_is_pure_and_dogfood_sized() {
    assert!(!over_threshold(&review_queue(REVIEW_QUEUE_DOGFOOD_THRESHOLD - 1)));
    assert!(over_threshold(&review_queue(REVIEW_QUEUE_DOGFOOD_THRESHOLD)));
}

#[tokio::test]
async fn reality_check_due_emits_overdue_before_due_when_window_is_exceeded() {
    let now = Utc.with_ymd_and_hms(2026, 5, 1, 12, 0, 0).unwrap();
    let state = RealityCheckState { last_completed_at: Some(now - Duration::days(30)), snooze_until: None };
    let (sender, mut receiver) = tokio::sync::broadcast::channel(4);

    assert!(RcScheduler::default().check_and_fire_if_due(&state, now, &sender));

    assert_eq!(
        receiver.recv().await.expect("overdue event"),
        NotificationEvent::RealityCheckOverdue { last_completed_at: state.last_completed_at, weeks_skipped: 4 }
    );
    assert_eq!(receiver.recv().await.expect("due event"), NotificationEvent::RealityCheckDue { due_at: now });
}

#[tokio::test]
async fn startup_reconcile_quarantine_report_fans_out_blocking_merge_notification() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let initialized = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_notify".to_string()) },
    )
    .await
    .expect("substrate init");
    drop(initialized);

    let conflict_path = write_quarantined_memory(&roots, "mem_20260508_a1b2c3d4e5f60718_000001");
    let substrate = Substrate::open(roots.clone()).await.expect("reopen with quarantined memory");

    assert_eq!(substrate.startup_reconcile_report().blocking_conflicts, vec![conflict_path.clone()]);

    let socket = unique_socket_path("notify", "blocking-conflict");
    let (shutdown_tx, server) = spawn_daemon(&socket, substrate);
    wait_for_socket(&socket).await;

    let status = wait_for_merge_conflict_passive_notification(&socket).await;
    assert!(
        status.passive_notifications.iter().any(|notification| notification.message.contains("merge conflict")),
        "{:#?}",
        status.passive_notifications
    );

    shutdown(shutdown_tx, server, &socket).await;
}

#[tokio::test]
async fn recovery_required_emits_operator_finding() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let initialized = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_recovery".to_string()) },
    )
    .await
    .expect("substrate init");
    drop(initialized);

    std::fs::write(roots.runtime.join("startup-reconcile.required"), "test recovery marker")
        .expect("write startup recovery marker");
    let substrate = Substrate::open(roots).await.expect("reopen with recovery marker");
    assert!(substrate.startup_reconcile_report().recovery_required);

    let socket = unique_socket_path("notify", "recovery-required");
    let (shutdown_tx, server) = spawn_daemon(&socket, substrate);
    wait_for_socket(&socket).await;

    let status = wait_for_passive_notification(&socket, "recovery is required").await;
    assert!(
        status.passive_notifications.iter().any(|notification| notification.message.contains("recovery is required")),
        "{:#?}",
        status.passive_notifications
    );

    shutdown(shutdown_tx, server, &socket).await;
}

#[tokio::test]
async fn quarantine_resolve_clears_sync_blocked_without_restart() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let initialized = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_quarantineresolve".to_string()) },
    )
    .await
    .expect("substrate init");
    drop(initialized);

    let memory_id = "mem_20260508_a1b2c3d4e5f60718_000002";
    let conflict_path = write_quarantined_memory(&roots, memory_id);
    let substrate = Substrate::open(roots).await.expect("reopen with quarantined memory");
    assert_eq!(substrate.startup_reconcile_report().blocking_conflicts, vec![conflict_path.clone()]);

    let socket = unique_socket_path("notify", "quarantine-resolve");
    let (shutdown_tx, server) = spawn_daemon(&socket, substrate);
    wait_for_socket(&socket).await;
    let blocked = wait_for_merge_conflict_passive_notification(&socket).await;
    assert!(
        blocked.passive_notifications.iter().any(|notification| notification.message.contains(&conflict_path)),
        "{:#?}",
        blocked.passive_notifications
    );

    let response = client::request(
        &socket,
        "quarantine-resolve",
        RequestPayload::QuarantineResolve { id: memory_id.to_owned(), mode: QuarantineResolutionMode::Edited },
    )
    .await
    .expect("quarantine resolve reaches daemon");
    match response.result {
        ResponseResult::Success(ResponsePayload::QuarantineResolve(resolve)) => {
            assert_eq!(resolve.id, memory_id);
            assert!(resolve.remaining_blocking_conflicts.is_empty(), "{resolve:#?}");
        }
        other => panic!("expected quarantine resolve response, got {other:?}"),
    }

    let clear = wait_for_no_passive_notification(&socket, "Sync is blocked").await;
    assert!(
        clear.passive_notifications.iter().all(|notification| !notification.message.contains("Sync is blocked")),
        "{:#?}",
        clear.passive_notifications
    );

    shutdown(shutdown_tx, server, &socket).await;
}

#[tokio::test]
async fn quarantine_resolve_rejects_governance_quarantine_without_mutating_it() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let initialized = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_governancequarantine".to_string()) },
    )
    .await
    .expect("substrate init");
    drop(initialized);

    let memory_id = "mem_20260508_a1b2c3d4e5f60718_000003";
    write_governance_quarantined_memory(&roots, memory_id);
    let substrate = Substrate::open(roots).await.expect("reopen with governance-quarantined memory");

    let response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "governance-quarantine-resolve",
            RequestPayload::QuarantineResolve { id: memory_id.to_owned(), mode: QuarantineResolutionMode::Edited },
        ),
    )
    .await;
    let ResponseResult::Error(error) = response.result else {
        panic!("expected invalid_request for governance quarantine, got {:?}", response.result);
    };
    assert_eq!(error.code, "invalid_request");
    assert!(
        error.message.contains("merge-conflict") && error.message.contains("review/governance"),
        "error should route operator to the right flow: {}",
        error.message
    );

    let saved = substrate.read_memory(&MemoryId::new(memory_id)).await.expect("memory remains readable");
    assert_eq!(saved.frontmatter.status, MemoryStatus::Quarantined);
    assert_eq!(saved.frontmatter.trust_level, TrustLevel::Quarantined);
    assert_eq!(saved.frontmatter.review_state.as_deref(), Some("quarantined"));
    assert_eq!(
        saved.frontmatter.extras.get("governance_reason").and_then(serde_json::Value::as_str),
        Some("governance quarantine")
    );
}

#[tokio::test]
async fn trust_level_only_quarantine_is_counted_and_survives_other_resolve() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let initialized = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_trustquarantine".to_string()) },
    )
    .await
    .expect("substrate init");
    drop(initialized);

    // Two quarantines created pre-open so the startup reconcile (OR predicate)
    // records both: one status-quarantined (the one we will resolve) and one
    // quarantined ONLY by `trust_level` with a non-quarantined `status` — the case
    // a status-only scan/count misses.
    let status_id = "mem_20260508_a1b2c3d4e5f60718_000010";
    let trust_id = "mem_20260508_a1b2c3d4e5f60718_000011";
    let status_path = write_quarantined_memory(&roots, status_id);
    let trust_path = write_trust_level_only_quarantined_memory(&roots, trust_id);

    let substrate = Substrate::open(roots).await.expect("reopen with quarantined memories");
    let mut expected = vec![status_path.clone(), trust_path.clone()];
    expected.sort();
    assert_eq!(substrate.startup_reconcile_report().blocking_conflicts, expected);

    let socket = unique_socket_path("notify", "trust-quarantine");
    let (shutdown_tx, server) = spawn_daemon(&socket, substrate);
    wait_for_socket(&socket).await;

    // Both blocking-conflict notices fan out, and the trust-level-only quarantine
    // is counted (FIX 2a): a status-only count would report 1, not 2.
    wait_for_passive_notification(&socket, status_path.as_str()).await;
    let before = wait_for_passive_notification(&socket, trust_path.as_str()).await;
    assert!(
        before.passive_notifications.iter().any(|notification| notification.message.contains(&trust_path)),
        "{:#?}",
        before.passive_notifications
    );
    assert_eq!(before.conflicts_count, Some(2), "{before:#?}");

    // Resolve the DIFFERENT (status-quarantined) memory.
    let response = client::request(
        &socket,
        "trust-quarantine-resolve",
        RequestPayload::QuarantineResolve { id: status_id.to_owned(), mode: QuarantineResolutionMode::Edited },
    )
    .await
    .expect("quarantine resolve reaches daemon");
    match response.result {
        ResponseResult::Success(ResponsePayload::QuarantineResolve(resolve)) => {
            // Only the trust-level-only quarantine remains blocking.
            assert_eq!(resolve.remaining_blocking_conflicts, vec![trust_path.clone()], "{resolve:#?}");
        }
        other => panic!("expected quarantine resolve response, got {other:?}"),
    }

    // FIX 2b: resolving the other quarantine must NOT false-clear the
    // trust-level-only "Sync is blocked" notice, and the count drops to exactly 1.
    let after = wait_for_conflicts_count(&socket, 1).await;
    assert_eq!(after.conflicts_count, Some(1), "{after:#?}");
    assert!(
        after.passive_notifications.iter().any(|notification| notification.message.contains(&trust_path)),
        "trust-level-only sync-blocked notice was false-cleared: {:#?}",
        after.passive_notifications
    );
    assert!(
        after.passive_notifications.iter().all(|notification| !notification.message.contains(&status_path)),
        "resolved quarantine's notice should be pruned: {:#?}",
        after.passive_notifications
    );

    shutdown(shutdown_tx, server, &socket).await;
}

#[test]
fn trigger_registry_maps_dogfood_kinds_to_protocol_events() {
    assert_eq!(
        notification_for(EventKind::MergeQuarantined { path: "agent/conflict.md".to_string() }),
        NotificationEvent::BlockingMergeConflict { path: "agent/conflict.md".to_string() }
    );
    assert_eq!(
        notification_for(EventKind::DreamCompleted { scope: "agent".to_string(), promoted: 0, queued: 2, dropped: 1 }),
        NotificationEvent::DreamRunCompleted { scope: "agent".to_string(), promoted: 0, queued: 2, dropped: 1 }
    );
    assert_eq!(
        notification_for(EventKind::DailySynthesis { scope: "agent".to_string() }),
        NotificationEvent::DailySynthesisSummaryReady { scope: "agent".to_string() }
    );
}

fn dogfood_events() -> Vec<NotificationEvent> {
    let last_completed_at = Utc.with_ymd_and_hms(2026, 4, 1, 9, 0, 0).unwrap();
    vec![
        NotificationEvent::BlockingMergeConflict { path: "agent/conflict.md".to_string() },
        NotificationEvent::DreamRunCompleted { scope: "agent".to_string(), promoted: 0, queued: 2, dropped: 1 },
        NotificationEvent::DailySynthesisSummaryReady { scope: "agent".to_string() },
        NotificationEvent::ReviewQueueOverThreshold {
            count: REVIEW_QUEUE_DOGFOOD_THRESHOLD,
            threshold: REVIEW_QUEUE_DOGFOOD_THRESHOLD,
        },
        NotificationEvent::RealityCheckOverdue { last_completed_at: Some(last_completed_at), weeks_skipped: 4 },
    ]
}

fn review_queue(count: usize) -> ReviewQueue {
    ReviewQueue { items: (0..count).map(review_item).collect() }
}

fn review_item(index: usize) -> ReviewQueueItem {
    ReviewQueueItem {
        id: format!("mem_{index:02}"),
        summary: "candidate".to_string(),
        status: ReviewStatus::Candidate,
        policy_applied: "dogfood".to_string(),
        reason: None,
        next_actions: Vec::new(),
    }
}

fn write_quarantined_memory(roots: &Roots, id: &str) -> String {
    let mut memory = sample_memory(id);
    memory.frontmatter.status = MemoryStatus::Quarantined;
    memory.frontmatter.trust_level = TrustLevel::Quarantined;
    memory.frontmatter.requires_user_confirmation = true;
    memory.frontmatter.review_state = Some("quarantined".to_string());
    memory.frontmatter.write_policy.human_review_required = true;
    memory.frontmatter.merge_diagnostics = Some(merge_quarantine_diagnostics("blocking merge conflict"));
    memory.body.push_str("\n\n<!-- merge quarantine; admin review required -->\n");
    let path = memory.path.clone().expect("memory has path");
    let text = frontmatter::serialize_document(&memory).expect("serialize quarantined memory");
    let disk_path = roots.repo.join(path.as_path());
    std::fs::create_dir_all(disk_path.parent().expect("memory path has parent")).expect("create memory dir");
    std::fs::write(&disk_path, text).expect("write quarantined memory");
    path.as_str().to_string()
}

/// Write a memory quarantined ONLY by `trust_level`, with a non-quarantined
/// `status`. Uses `(Candidate, Quarantined)` — a valid lifecycle pair (`frontmatter::
/// validate`: a `Candidate` status permits `Quarantined` trust; an `Active` one does
/// NOT, so do not "simplify" this to `Active`). The authoritative OR predicate
/// (`reconcile.rs`) treats it as a blocking conflict, but a status-only query/count
/// misses it (`status != Quarantined`).
fn write_trust_level_only_quarantined_memory(roots: &Roots, id: &str) -> String {
    let mut memory = sample_memory(id);
    memory.frontmatter.status = MemoryStatus::Candidate;
    memory.frontmatter.trust_level = TrustLevel::Quarantined;
    memory.frontmatter.merge_diagnostics = Some(merge_quarantine_diagnostics("blocking merge conflict"));
    let path = memory.path.clone().expect("memory has path");
    let text = frontmatter::serialize_document(&memory).expect("serialize trust-level quarantined memory");
    let disk_path = roots.repo.join(path.as_path());
    std::fs::create_dir_all(disk_path.parent().expect("memory path has parent")).expect("create memory dir");
    std::fs::write(&disk_path, text).expect("write trust-level quarantined memory");
    path.as_str().to_string()
}

fn write_governance_quarantined_memory(roots: &Roots, id: &str) -> String {
    let mut memory = sample_memory(id);
    memory.frontmatter.status = MemoryStatus::Quarantined;
    memory.frontmatter.trust_level = TrustLevel::Quarantined;
    memory.frontmatter.requires_user_confirmation = true;
    memory.frontmatter.review_state = Some("quarantined".to_string());
    memory.frontmatter.write_policy.human_review_required = true;
    memory.frontmatter.write_policy.policy_applied = "governance-quarantine-v1".to_string();
    memory.frontmatter.merge_diagnostics = Some(serde_json::json!({
        "human_reason": "governance quarantine",
        "preserved_sources": [],
        "lifecycle_notes": [],
        "evidence_near_duplicates": []
    }));
    memory.frontmatter.extras.insert("governance_reason".to_string(), serde_json::json!("governance quarantine"));
    memory.body = "governance-quarantined body".to_string();
    let path = memory.path.clone().expect("memory has path");
    let text = frontmatter::serialize_document(&memory).expect("serialize governance-quarantined memory");
    let disk_path = roots.repo.join(path.as_path());
    std::fs::create_dir_all(disk_path.parent().expect("memory path has parent")).expect("create memory dir");
    std::fs::write(&disk_path, text).expect("write governance-quarantined memory");
    path.as_str().to_string()
}

fn merge_quarantine_diagnostics(reason: &str) -> serde_json::Value {
    serde_json::json!([{
        "merge_id": "merge_test",
        "created_at": "2026-05-08T12:00:00Z",
        "status": "quarantined",
        "conflicting_fields": ["body"],
        "human_reason": reason
    }])
}

fn sample_memory(id: &str) -> Memory {
    let now = chrono::DateTime::parse_from_rfc3339("2026-05-08T12:00:00Z").expect("date").with_timezone(&Utc);
    Memory {
        frontmatter: Frontmatter {
            schema_version: 1,
            id: MemoryId::new(id),
            memory_type: MemoryType::Pattern,
            scope: Scope::Agent,
            summary: "quarantined merge conflict".to_string(),
            confidence: 1.0,
            original_confidence: None,
            trust_level: TrustLevel::Trusted,
            sensitivity: Sensitivity::Internal,
            status: MemoryStatus::Active,
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
                component: Some("test".to_string()),
            },
            namespace: None,
            canonical_namespace_id: None,
            tags: Vec::new(),
            entities: Vec::new(),
            aliases: Vec::new(),
            source: Source {
                kind: SourceKind::Import,
                reference: None,
                harness: None,
                harness_version: None,
                session_id: None,
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
                max_scope: Scope::Agent,
                mask_personal_for_synthesis: false,
                index_body: true,
                index_embeddings: true,
            },
            write_policy: WritePolicy {
                human_review_required: false,
                policy_applied: "default-v1".to_string(),
                expected_base_hash: None,
            },
            merge_diagnostics: None,
            abstraction: None,
            cues: Vec::new(),
            extras: std::collections::BTreeMap::new(),
        },
        body: "memory body quarantined by merge".to_string(),
        path: Some(memory_substrate::RepoPath::new(format!("agent/patterns/{id}.md"))),
    }
}

async fn wait_for_merge_conflict_passive_notification(socket: &Path) -> StatusResponse {
    wait_for_passive_notification(socket, "merge conflict").await
}

async fn wait_for_passive_notification(socket: &Path, needle: &str) -> StatusResponse {
    let deadline = tokio::time::Instant::now() + StdDuration::from_secs(2);
    loop {
        let status = status(socket).await;
        if status.passive_notifications.iter().any(|notification| notification.message.contains(needle)) {
            return status;
        }
        if tokio::time::Instant::now() >= deadline {
            return status;
        }
        sleep(StdDuration::from_millis(25)).await;
    }
}

async fn wait_for_no_passive_notification(socket: &Path, needle: &str) -> StatusResponse {
    let deadline = tokio::time::Instant::now() + StdDuration::from_secs(2);
    loop {
        let status = status(socket).await;
        if status.passive_notifications.iter().all(|notification| !notification.message.contains(needle)) {
            return status;
        }
        if tokio::time::Instant::now() >= deadline {
            return status;
        }
        sleep(StdDuration::from_millis(25)).await;
    }
}

async fn wait_for_conflicts_count(socket: &Path, target: u32) -> StatusResponse {
    let deadline = tokio::time::Instant::now() + StdDuration::from_secs(2);
    loop {
        let status = status(socket).await;
        if status.conflicts_count == Some(target) {
            return status;
        }
        if tokio::time::Instant::now() >= deadline {
            return status;
        }
        sleep(StdDuration::from_millis(25)).await;
    }
}

async fn status(socket: &Path) -> StatusResponse {
    let response = client::request(socket, "status-after-blocking-conflict", RequestPayload::Status)
        .await
        .expect("status request reaches daemon");
    match response.result {
        ResponseResult::Success(ResponsePayload::Status(status)) => status,
        other => panic!("expected status response, got {other:?}"),
    }
}
