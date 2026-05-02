use std::sync::{Arc, Barrier, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, TimeZone, Utc};
use memorum_coordination::claim_lock::{ClaimLockAcquireRequest, ClaimLockClock, ClaimLockRegistry};
use memorum_coordination::config::PresenceConfig;
use memorum_coordination::presence::{
    cleanup_stale_sessions, spawn_stale_session_cleanup_task, ActivePeerQuery, ClaimLockHeartbeatRenewal,
    StaleSessionClaimLockReleaser, PRESENCE_CLEANUP_INTERVAL,
};
use memorum_coordination::{
    handle_peer_heartbeat, ConcurrentSessionMode, PeerHeartbeat, PeerHeartbeatError, PeerHeartbeatOptions,
    PresenceRecord, PresenceRegistry, ProjectBinding,
};
use tokio::sync::watch;

#[test]
fn test_upsert_and_snapshot() {
    let registry = PresenceRegistry::new();
    let now = Instant::now();

    registry.upsert(record("sess_a", "project:alpha", now));
    registry.upsert(record("sess_b", "project:beta", now));

    let records = registry.snapshot_for_namespace("project:alpha");

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].session_id, "sess_a");
    assert_eq!(records[0].project_binding.as_ref().and_then(|binding| binding.cwd.as_deref()), Some("/repo/alpha"));
}

#[test]
fn test_stale_removal() {
    let registry = PresenceRegistry::new();
    let now = Instant::now();
    let stale_after = Duration::from_secs(300);

    registry.upsert(record("sess_stale", "project:alpha", now - stale_after - Duration::from_secs(1)));

    let removed = registry.cleanup_stale_at(now, stale_after);

    assert_eq!(removed, vec!["sess_stale".to_string()]);
    assert!(registry.snapshot_for_namespace("project:alpha").is_empty());
}

#[test]
fn test_fresh_not_removed() {
    let registry = PresenceRegistry::new();
    let now = Instant::now();
    let stale_after = Duration::from_secs(300);

    registry.upsert(record("sess_fresh", "project:alpha", now - stale_after + Duration::from_secs(1)));

    assert!(registry.cleanup_stale_at(now, stale_after).is_empty());
    assert_eq!(registry.snapshot_for_namespace("project:alpha").len(), 1);
}

#[test]
fn test_stale_cleanup_releases_claim_locks() {
    let registry = PresenceRegistry::new();
    let claim_locks = RecordingClaimLockReleaser::default();
    let now = Instant::now();
    let stale_after = Duration::from_secs(300);

    registry.upsert(record("sess_stale", "project:alpha", now - stale_after - Duration::from_secs(1)));
    registry.upsert(record("sess_fresh", "project:alpha", now));

    let removed = cleanup_stale_sessions(&registry, &claim_locks, now, stale_after);

    assert_eq!(removed, vec!["sess_stale".to_string()]);
    assert_eq!(claim_locks.released_session_ids(), vec!["sess_stale".to_string()]);
    let remaining = registry.snapshot_for_namespace("project:alpha");
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].session_id, "sess_fresh");
}

#[tokio::test(start_paused = true)]
async fn test_cleanup_task_uses_sixty_second_interval_and_default_threshold() {
    let registry = Arc::new(PresenceRegistry::new());
    let claim_locks = Arc::new(RecordingClaimLockReleaser::default());
    let now = Instant::now();
    registry.upsert(record(
        "sess_stale",
        "project:alpha",
        now - PresenceConfig::default().stale_after() - Duration::from_secs(1),
    ));

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let cleanup =
        spawn_stale_session_cleanup_task(registry.clone(), claim_locks.clone(), PresenceConfig::default(), shutdown_rx);

    tokio::task::yield_now().await;
    assert_eq!(registry.snapshot_for_namespace("project:alpha").len(), 1);

    tokio::time::advance(PRESENCE_CLEANUP_INTERVAL).await;
    tokio::task::yield_now().await;

    assert!(registry.snapshot_for_namespace("project:alpha").is_empty());
    assert_eq!(claim_locks.released_session_ids(), vec!["sess_stale".to_string()]);

    shutdown_tx.send(true).expect("shutdown signal should send");
    cleanup.await.expect("cleanup task should shut down cleanly");
}

#[tokio::test(start_paused = true)]
async fn test_cleanup_task_shuts_down_cleanly_on_shutdown_signal() {
    let registry = Arc::new(PresenceRegistry::new());
    let claim_locks = Arc::new(RecordingClaimLockReleaser::default());
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let cleanup = spawn_stale_session_cleanup_task(registry, claim_locks, PresenceConfig::default(), shutdown_rx);

    shutdown_tx.send(true).expect("shutdown signal should send");

    tokio::time::timeout(Duration::from_secs(1), cleanup)
        .await
        .expect("cleanup task should not hang")
        .expect("cleanup task should not panic");
}

#[tokio::test(start_paused = true)]
async fn test_cleanup_task_does_not_block_heartbeat_handler() {
    let registry = Arc::new(PresenceRegistry::new());
    let claim_locks = Arc::new(RecordingClaimLockReleaser::default());
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let cleanup =
        spawn_stale_session_cleanup_task(registry.clone(), claim_locks, PresenceConfig::default(), shutdown_rx);

    tokio::time::advance(PRESENCE_CLEANUP_INTERVAL).await;

    let heartbeat = tokio::time::timeout(Duration::from_secs(1), async {
        handle_peer_heartbeat(
            &registry,
            heartbeat("sess_a", Some(timestamp(14, 2))),
            PeerHeartbeatOptions {
                default_level: 3,
                now: Instant::now(),
                stale_threshold: PresenceConfig::default().stale_after(),
                claim_lock_renewal: None,
            },
        )
    })
    .await
    .expect("heartbeat handler should not block behind cleanup task")
    .expect("heartbeat should be valid");

    assert_eq!(heartbeat.session_id, "sess_a");

    shutdown_tx.send(true).expect("shutdown signal should send");
    cleanup.await.expect("cleanup task should shut down cleanly");
}

#[test]
fn test_concurrent_upsert() {
    let registry = Arc::new(PresenceRegistry::new());
    let barrier = Arc::new(Barrier::new(3));
    let now = Instant::now();

    let first = spawn_upsert(registry.clone(), barrier.clone(), record("sess_same", "project:alpha", now));
    let second = spawn_upsert(
        registry.clone(),
        barrier.clone(),
        record("sess_same", "project:alpha", now + Duration::from_secs(1)),
    );

    barrier.wait();
    first.join().expect("first upsert thread should not panic");
    second.join().expect("second upsert thread should not panic");

    let records = registry.snapshot_for_namespace("project:alpha");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].session_id, "sess_same");
}

#[test]
fn test_remove() {
    let registry = PresenceRegistry::new();
    let now = Instant::now();

    registry.upsert(record("sess_a", "project:alpha", now));
    registry.remove("sess_a");

    assert!(registry.snapshot_for_namespace("project:alpha").is_empty());
}

#[test]
fn test_all_records_snapshot() {
    let registry = PresenceRegistry::new();
    let now = Instant::now();

    registry.upsert(record("sess_a", "project:alpha", now));
    registry.upsert(record("sess_b", "project:beta", now));

    let mut session_ids = registry.all_records().into_iter().map(|record| record.session_id).collect::<Vec<_>>();
    session_ids.sort();

    assert_eq!(session_ids, vec!["sess_a".to_string(), "sess_b".to_string()]);
}

#[test]
fn test_upsert_retains_original_started_at() {
    let registry = PresenceRegistry::new();
    let now = Instant::now();
    let first_started_at = timestamp(14, 2);
    let later_started_at = timestamp(14, 30);

    let mut first = record("sess_a", "project:alpha", now);
    first.started_at = Some(first_started_at);
    registry.upsert(first);

    let mut update = record("sess_a", "project:alpha", now + Duration::from_secs(60));
    update.started_at = Some(later_started_at);
    update.salient_paths = vec!["project:alpha/updated.md".to_string()];
    registry.upsert(update);

    let records = registry.snapshot_for_namespace("project:alpha");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].started_at, Some(first_started_at));
    assert_eq!(records[0].salient_paths, vec!["project:alpha/updated.md".to_string()]);
}

#[test]
fn test_active_peers_exclude_self_and_stale_records() {
    let registry = PresenceRegistry::new();
    let now = Instant::now();
    let stale_after = Duration::from_secs(300);

    registry.upsert(record("self", "project:alpha", now));
    registry.upsert(record("peer", "project:alpha", now - Duration::from_secs(10)));
    registry.upsert(record("stale", "project:alpha", now - stale_after - Duration::from_secs(1)));
    registry.upsert(record("other_namespace", "project:beta", now));

    let peers = registry.active_peers(ActivePeerQuery {
        namespace: "project:alpha",
        own_session_id: Some("self"),
        now,
        stale_threshold: stale_after,
    });

    assert_eq!(peers.len(), 1);
    assert_eq!(peers[0].session_id, "peer");
}

#[test]
fn test_heartbeat_serde_roundtrip() {
    let with_started_at = heartbeat("sess_a", Some(timestamp(14, 2)));

    let json = serde_json::to_string(&with_started_at).expect("heartbeat should serialize");
    let decoded: PeerHeartbeat = serde_json::from_str(&json).expect("heartbeat should deserialize");

    assert_eq!(decoded, with_started_at);
    assert_eq!(decoded.started_at, Some(timestamp(14, 2)));

    let without_started_at = heartbeat("sess_b", None);
    let json = serde_json::to_string(&without_started_at).expect("heartbeat without started_at should serialize");
    let decoded: PeerHeartbeat = serde_json::from_str(&json).expect("heartbeat without started_at should deserialize");

    assert_eq!(decoded.started_at, None);
}

#[test]
fn test_handle_peer_heartbeat_updates_presence_and_returns_active_peers() {
    let registry = PresenceRegistry::new();
    let now = Instant::now();
    let stale_after = Duration::from_secs(300);

    registry.upsert(record("peer", "project:alpha", now - Duration::from_secs(10)));
    registry.upsert(record("stale", "project:alpha", now - stale_after - Duration::from_secs(1)));
    registry.upsert(record("other_namespace", "project:beta", now));

    let ack = handle_peer_heartbeat(
        &registry,
        heartbeat("self", Some(timestamp(14, 2))),
        PeerHeartbeatOptions { default_level: 3, now, stale_threshold: stale_after, claim_lock_renewal: None },
    )
    .expect("valid heartbeat should be accepted");

    assert_eq!(ack.session_id, "self");
    assert_eq!(ack.active_level, 3);
    assert_eq!(ack.peer_session_count, 1);
    assert_eq!(ack.active_peers.len(), 1);
    assert_eq!(ack.active_peers[0].session_id, "peer");
    assert_eq!(ack.active_peers[0].salient_entities, vec!["ent_shared".to_string()]);

    let peer_json = serde_json::to_value(&ack.active_peers[0]).expect("active peer should serialize");
    for hidden_field in ["device_id", "project_binding", "salient_paths", "capabilities", "claim_locks_held"] {
        assert!(peer_json.get(hidden_field).is_none(), "active peer ack should not expose {hidden_field}");
    }

    let self_record = registry
        .snapshot_for_namespace("project:alpha")
        .into_iter()
        .find(|record| record.session_id == "self")
        .expect("self heartbeat should update presence");
    assert_eq!(self_record.device_id.as_deref(), Some("device_a"));
    assert_eq!(self_record.harness, "codex");
    assert_eq!(self_record.project_binding.as_ref().and_then(|binding| binding.cwd.as_deref()), Some("/repo/alpha"));
    assert_eq!(self_record.salient_paths, vec!["project:alpha/decision.md".to_string()]);
    assert_eq!(self_record.capabilities, vec!["presence".to_string(), "claim-lock-renewal".to_string()]);
}

#[test]
fn test_active_peer_payload_caps_entities_to_presence_surface() {
    let registry = PresenceRegistry::new();
    let now = Instant::now();
    let mut peer = record("long_peer_session_id", "project:alpha", now);
    peer.salient_entities = (0..7).map(|index| format!("ent_{index}")).collect();
    registry.upsert(peer);

    let ack = handle_peer_heartbeat(
        &registry,
        heartbeat("self", Some(timestamp(14, 2))),
        PeerHeartbeatOptions {
            default_level: 3,
            now,
            stale_threshold: Duration::from_secs(300),
            claim_lock_renewal: None,
        },
    )
    .expect("valid heartbeat should be accepted");

    assert_eq!(ack.active_peers.len(), 1);
    assert_eq!(ack.active_peers[0].session_id, "long_p");
    assert_eq!(ack.active_peers[0].salient_entities, vec!["ent_0", "ent_1", "ent_2", "ent_3", "ent_4"]);
}

#[test]
fn test_level3_heartbeat_renews_recognized_claim_locks_from_heartbeat_time() {
    let presence = PresenceRegistry::new();
    let claim_locks = ClaimLockRegistry::new();
    let base = Instant::now();
    let memory_id = "mem_20260501_a1b2c3d4e5f60718_000001";
    claim_locks.acquire_at(
        ClaimLockAcquireRequest::new(memory_id, "sess_a", "codex", Duration::from_secs(30)),
        clock_at(base, timestamp(14, 2)),
    );

    let mut heartbeat = heartbeat("sess_a", Some(timestamp(14, 2)));
    heartbeat.claim_locks_held = vec![memory_id.to_string(), "mem_20260501_unrecognized".to_string()];

    handle_peer_heartbeat(
        &presence,
        heartbeat,
        PeerHeartbeatOptions {
            default_level: 3,
            now: base + Duration::from_secs(20),
            stale_threshold: Duration::from_secs(300),
            claim_lock_renewal: Some(ClaimLockHeartbeatRenewal {
                registry: &claim_locks,
                ttl: Duration::from_secs(120),
                clock: clock_at(base + Duration::from_secs(20), timestamp(14, 3)),
            }),
        },
    )
    .expect("valid heartbeat should be accepted");

    let renewed = claim_locks.get_at(memory_id, base + Duration::from_secs(139)).unwrap();
    assert_eq!(renewed.expires_at, timestamp(14, 5));
    assert!(claim_locks.get_at(memory_id, base + Duration::from_secs(141)).is_none());
    assert!(claim_locks.get("mem_20260501_unrecognized").is_none());
}

#[test]
fn test_heartbeat_started_at_retained() {
    let registry = PresenceRegistry::new();
    let now = Instant::now();
    let stale_after = Duration::from_secs(300);

    handle_peer_heartbeat(
        &registry,
        heartbeat("sess_a", Some(timestamp(14, 2))),
        PeerHeartbeatOptions { default_level: 3, now, stale_threshold: stale_after, claim_lock_renewal: None },
    )
    .expect("first heartbeat should be accepted");
    handle_peer_heartbeat(
        &registry,
        heartbeat("sess_a", Some(timestamp(14, 30))),
        PeerHeartbeatOptions {
            default_level: 3,
            now: now + Duration::from_secs(60),
            stale_threshold: stale_after,
            claim_lock_renewal: None,
        },
    )
    .expect("second heartbeat should be accepted");

    let records = registry.snapshot_for_namespace("project:alpha");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].started_at, Some(timestamp(14, 2)));
}

#[test]
fn test_heartbeat_started_at_none_first_then_some() {
    let registry = PresenceRegistry::new();
    let now = Instant::now();
    let stale_after = Duration::from_secs(300);

    handle_peer_heartbeat(
        &registry,
        heartbeat("sess_a", None),
        PeerHeartbeatOptions { default_level: 3, now, stale_threshold: stale_after, claim_lock_renewal: None },
    )
    .expect("heartbeat without started_at should be accepted");
    handle_peer_heartbeat(
        &registry,
        heartbeat("sess_a", Some(timestamp(14, 2))),
        PeerHeartbeatOptions {
            default_level: 3,
            now: now + Duration::from_secs(60),
            stale_threshold: stale_after,
            claim_lock_renewal: None,
        },
    )
    .expect("heartbeat with started_at should be accepted");

    let records = registry.snapshot_for_namespace("project:alpha");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].started_at, Some(timestamp(14, 2)));
}

#[test]
fn test_heartbeat_validation_empty_session_id() {
    let registry = PresenceRegistry::new();
    let mut invalid = heartbeat(" ", Some(timestamp(14, 2)));
    invalid.session_id = " ".to_string();

    let error =
        handle_peer_heartbeat(&registry, invalid, level3_options()).expect_err("blank session_id should be invalid");

    assert!(matches!(error, PeerHeartbeatError::InvalidRequest { .. }));
    assert!(registry.all_records().is_empty());
}

#[test]
fn test_heartbeat_validation_entity_overflow() {
    let registry = PresenceRegistry::new();
    let mut invalid = heartbeat("sess_a", Some(timestamp(14, 2)));
    invalid.salient_entities = (0..33).map(|index| format!("ent_{index}")).collect();

    let error =
        handle_peer_heartbeat(&registry, invalid, level3_options()).expect_err("too many entities should be invalid");

    assert!(matches!(error, PeerHeartbeatError::InvalidRequest { .. }));
    assert!(registry.all_records().is_empty());
}

#[test]
fn test_heartbeat_validation_capability_overflow() {
    let registry = PresenceRegistry::new();
    let mut invalid = heartbeat("sess_a", Some(timestamp(14, 2)));
    invalid.capabilities = (0..17).map(|index| format!("cap_{index}")).collect();

    let error = handle_peer_heartbeat(&registry, invalid, level3_options())
        .expect_err("too many capabilities should be invalid");

    assert!(matches!(error, PeerHeartbeatError::InvalidRequest { .. }));
    assert!(registry.all_records().is_empty());
}

#[test]
fn test_heartbeat_validation_claim_lock_id_bounds() {
    let registry = PresenceRegistry::new();
    let mut invalid = heartbeat("sess_a", Some(timestamp(14, 2)));
    invalid.claim_locks_held = vec!["mem/invalid".to_string()];

    let error =
        handle_peer_heartbeat(&registry, invalid, level3_options()).expect_err("invalid claim lock id should fail");

    assert!(matches!(error, PeerHeartbeatError::InvalidRequest { .. }));
    assert!(registry.all_records().is_empty());
}

#[test]
fn test_level2_heartbeat_ack_does_not_update_presence() {
    let registry = PresenceRegistry::new();
    let mut level2_heartbeat = heartbeat("sess_a", Some(timestamp(14, 2)));
    if let Some(binding) = level2_heartbeat.project_binding.as_mut() {
        binding.concurrent_session_mode = None;
    }

    let ack = handle_peer_heartbeat(
        &registry,
        level2_heartbeat,
        PeerHeartbeatOptions {
            default_level: 2,
            now: Instant::now(),
            stale_threshold: Duration::from_secs(300),
            claim_lock_renewal: None,
        },
    )
    .expect("level 2 heartbeat should be acknowledged");

    assert_eq!(ack.active_level, 2);
    assert!(registry.all_records().is_empty());
}

fn spawn_upsert(
    registry: Arc<PresenceRegistry>,
    barrier: Arc<Barrier>,
    record: PresenceRecord,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        barrier.wait();
        registry.upsert(record);
    })
}

fn record(session_id: &str, namespace: &str, last_heartbeat_at: Instant) -> PresenceRecord {
    let project = namespace.trim_start_matches("project:");
    PresenceRecord {
        session_id: session_id.to_string(),
        device_id: Some("device_a".to_string()),
        harness: "codex".to_string(),
        project_binding: Some(ProjectBinding {
            canonical_id: project.to_string(),
            alias: Some(project.to_string()),
            cwd: Some(format!("/repo/{project}")),
            concurrent_session_mode: Some(ConcurrentSessionMode::Collaborative),
        }),
        namespace: namespace.to_string(),
        salient_entities: vec!["ent_shared".to_string()],
        salient_paths: vec![format!("{namespace}/decision.md")],
        capabilities: vec!["presence".to_string(), "claim-lock-renewal".to_string()],
        started_at: Some(timestamp(14, 2)),
        last_heartbeat_at,
        claim_locks_held: vec!["mem_20260501_a1b2c3d4e5f60718_000001".to_string()],
    }
}

fn heartbeat(session_id: &str, started_at: Option<DateTime<Utc>>) -> PeerHeartbeat {
    PeerHeartbeat {
        session_id: session_id.to_string(),
        device_id: Some("device_a".to_string()),
        harness: "codex".to_string(),
        project_binding: Some(ProjectBinding {
            canonical_id: "alpha".to_string(),
            alias: Some("alpha".to_string()),
            cwd: Some("/repo/alpha".to_string()),
            concurrent_session_mode: Some(ConcurrentSessionMode::Collaborative),
        }),
        namespace: "project:alpha".to_string(),
        salient_entities: vec!["ent_shared".to_string()],
        salient_paths: vec!["project:alpha/decision.md".to_string()],
        capabilities: vec!["presence".to_string(), "claim-lock-renewal".to_string()],
        started_at,
        claim_locks_held: vec!["mem_20260501_a1b2c3d4e5f60718_000001".to_string()],
    }
}

fn level3_options() -> PeerHeartbeatOptions<'static> {
    PeerHeartbeatOptions {
        default_level: 3,
        now: Instant::now(),
        stale_threshold: Duration::from_secs(300),
        claim_lock_renewal: None,
    }
}

fn clock_at(instant: Instant, utc: DateTime<Utc>) -> ClaimLockClock {
    ClaimLockClock { instant, utc }
}

fn timestamp(hour: u32, minute: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 5, 1, hour, minute, 0).single().expect("test timestamp should be valid")
}

#[derive(Default)]
struct RecordingClaimLockReleaser {
    released_session_ids: Mutex<Vec<String>>,
}

impl RecordingClaimLockReleaser {
    fn released_session_ids(&self) -> Vec<String> {
        self.released_session_ids.lock().expect("test releaser mutex should not be poisoned").clone()
    }
}

impl StaleSessionClaimLockReleaser for RecordingClaimLockReleaser {
    fn release_all_held_by(&self, _harness: &str, session_id: &str) {
        self.released_session_ids
            .lock()
            .expect("test releaser mutex should not be poisoned")
            .push(session_id.to_string());
    }
}
