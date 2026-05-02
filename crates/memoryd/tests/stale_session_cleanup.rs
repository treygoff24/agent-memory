use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use memorum_coordination::claim_lock::ClaimLockAcquireRequest;
use memorum_coordination::PresenceRecord;
use memoryd::handlers::HandlerState;
use tokio::sync::watch;

#[tokio::test(start_paused = true)]
async fn server_cleanup_task_removes_stale_presence_and_releases_claim_locks() {
    let state = Arc::new(HandlerState::with_coordination_level(3));
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let cleanup = memoryd::server::spawn_coordination_cleanup_for_state(state.clone(), shutdown_rx);
    tokio::task::yield_now().await;

    state.presence().upsert(stale_record("codex", "sess_stale"));
    state.claim_locks().acquire(ClaimLockAcquireRequest::new(
        "mem_20260501_a1b2c3d4e5f60718_000001",
        "sess_stale",
        "codex",
        Duration::from_secs(300),
    ));

    tokio::time::advance(Duration::from_secs(61)).await;
    tokio::task::yield_now().await;

    assert!(state.presence().all_records().is_empty());
    assert!(state.claim_locks().get("mem_20260501_a1b2c3d4e5f60718_000001").is_none());

    shutdown_tx.send(true).expect("send shutdown");
    cleanup.await.expect("cleanup task exits cleanly");
}

#[tokio::test(start_paused = true)]
async fn server_cleanup_task_leaves_fresh_presence_and_handlers_unblocked() {
    let state = Arc::new(HandlerState::with_coordination_level(3));
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let cleanup = memoryd::server::spawn_coordination_cleanup_for_state(state.clone(), shutdown_rx);
    tokio::task::yield_now().await;

    state.presence().upsert(fresh_record("codex", "sess_fresh"));

    tokio::time::advance(Duration::from_secs(61)).await;
    tokio::task::yield_now().await;

    let records = tokio::time::timeout(Duration::from_secs(1), async { state.presence().all_records() })
        .await
        .expect("presence read should not block");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].session_id, "sess_fresh");

    shutdown_tx.send(true).expect("send shutdown");
    cleanup.await.expect("cleanup task exits cleanly");
}

#[tokio::test(start_paused = true)]
async fn server_cleanup_task_sweeps_expired_claim_locks_without_stale_presence() {
    let state = Arc::new(HandlerState::with_coordination_level(3));
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let cleanup = memoryd::server::spawn_coordination_cleanup_for_state(state.clone(), shutdown_rx);
    tokio::task::yield_now().await;

    state.presence().upsert(fresh_record("codex", "sess_fresh"));
    state.claim_locks().acquire(ClaimLockAcquireRequest::new(
        "mem_20260501_a1b2c3d4e5f60718_000002",
        "sess_other",
        "claude-code",
        Duration::ZERO,
    ));

    tokio::time::advance(Duration::from_secs(61)).await;
    tokio::task::yield_now().await;

    assert_eq!(state.presence().all_records().len(), 1);
    assert!(state.claim_locks().get("mem_20260501_a1b2c3d4e5f60718_000002").is_none());

    shutdown_tx.send(true).expect("send shutdown");
    cleanup.await.expect("cleanup task exits cleanly");
}

fn stale_record(harness: &str, session_id: &str) -> PresenceRecord {
    let mut record = fresh_record(harness, session_id);
    record.last_heartbeat_at = Instant::now() - Duration::from_secs(301);
    record
}

fn fresh_record(harness: &str, session_id: &str) -> PresenceRecord {
    PresenceRecord {
        session_id: session_id.to_string(),
        device_id: Some("dev_stale01".to_string()),
        harness: harness.to_string(),
        project_binding: None,
        namespace: "project:agent-memory".to_string(),
        salient_entities: vec!["ent_stream_i".to_string()],
        salient_paths: vec!["docs/specs/stream-i-cross-session-v0.1.md".to_string()],
        capabilities: Vec::new(),
        started_at: Some(Utc::now()),
        last_heartbeat_at: Instant::now(),
        claim_locks_held: vec!["mem_20260501_a1b2c3d4e5f60718_000001".to_string()],
    }
}
