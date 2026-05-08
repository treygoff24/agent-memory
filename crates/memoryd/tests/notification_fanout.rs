use std::path::{Path, PathBuf};
use std::time::{Duration as StdDuration, SystemTime, UNIX_EPOCH};

use chrono::{Duration, TimeZone, Utc};
use memory_governance::review::{over_threshold, REVIEW_QUEUE_DOGFOOD_THRESHOLD};
use memory_governance::{ReviewQueue, ReviewQueueItem, ReviewStatus};
use memory_substrate::frontmatter;
use memory_substrate::{
    Author, AuthorKind, Frontmatter, InitOptions, Memory, MemoryId, MemoryStatus, MemoryType, RetrievalPolicy, Roots,
    Scope, Sensitivity, Source, SourceKind, Substrate, TrustLevel, WritePolicy,
};
use memoryd::client;
use memoryd::notifications::config::NotificationConfig;
use memoryd::notifications::dispatcher::NotificationDispatcher;
use memoryd::notifications::external::ExternalNotifier;
use memoryd::notifications::os::OsNotifier;
use memoryd::notifications::passive::PassiveQueue;
use memoryd::notifications::triggers::{notification_for, EventKind};
use memoryd::protocol::{NotificationEvent, RequestPayload, ResponsePayload, ResponseResult, StatusResponse};
use memoryd::reality_check::RcScheduler;
use memoryd::server::{serve_substrate_with, ServerOptions};
use memoryd::state::RealityCheckState;
use tokio::net::UnixStream;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout};

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

    let socket = unique_socket_path("blocking-conflict");
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
    memory.frontmatter.merge_diagnostics = Some(serde_json::json!({"reason": "blocking merge conflict"}));
    memory.body.push_str("\n\n<!-- merge quarantine; admin review required -->\n");
    let path = memory.path.clone().expect("memory has path");
    let text = frontmatter::serialize_document(&memory).expect("serialize quarantined memory");
    let disk_path = roots.repo.join(path.as_path());
    std::fs::create_dir_all(disk_path.parent().expect("memory path has parent")).expect("create memory dir");
    std::fs::write(&disk_path, text).expect("write quarantined memory");
    path.as_str().to_string()
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
            extras: std::collections::BTreeMap::new(),
        },
        body: "memory body quarantined by merge".to_string(),
        path: Some(memory_substrate::RepoPath::new(format!("agent/patterns/{id}.md"))),
    }
}

fn spawn_daemon(socket: &Path, substrate: Substrate) -> (watch::Sender<bool>, JoinHandle<anyhow::Result<()>>) {
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let socket = socket.to_path_buf();
    let options = ServerOptions { idle_frame_timeout: StdDuration::from_secs(5) };
    let task = tokio::spawn(serve_substrate_with(socket, substrate, options, shutdown_rx));
    (shutdown_tx, task)
}

async fn wait_for_merge_conflict_passive_notification(socket: &Path) -> StatusResponse {
    let deadline = tokio::time::Instant::now() + StdDuration::from_secs(2);
    loop {
        let status = status(socket).await;
        if status.passive_notifications.iter().any(|notification| notification.message.contains("merge conflict")) {
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

async fn wait_for_socket(socket: &Path) {
    for _ in 0..200 {
        if UnixStream::connect(socket).await.is_ok() {
            return;
        }
        sleep(StdDuration::from_millis(10)).await;
    }
    panic!("daemon did not bind socket at {}", socket.display());
}

async fn shutdown(shutdown_tx: watch::Sender<bool>, server: JoinHandle<anyhow::Result<()>>, socket: &Path) {
    shutdown_tx.send(true).expect("shutdown signal lands");
    timeout(StdDuration::from_secs(2), server)
        .await
        .expect("server stops before timeout")
        .expect("server task joins")
        .expect("server returns Ok");
    let _ = std::fs::remove_file(socket);
}

fn unique_socket_path(test_name: &str) -> PathBuf {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).expect("system clock is after epoch").as_nanos();
    let dir = PathBuf::from(format!("/tmp/memd-notify-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create short socket directory");
    dir.join(format!("{test_name}-{nonce}.sock"))
}
