use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

use chrono::{DateTime, TimeZone, Utc};
use memorum_coordination::claim_lock::{
    ClaimLockAcquireRequest, ClaimLockAcquireResult, ClaimLockClock, ClaimLockRegistry, ClaimLockRenewRequest,
    ClaimLockRenewResult, CLAIM_LOCK_CONTENTION_CODE,
};

macro_rules! acquire_at {
    ($registry:expr, $memory_id:expr, $session_id:expr, $harness:expr, $ttl:expr, $clock:expr $(,)?) => {
        $registry.acquire_at(ClaimLockAcquireRequest::new($memory_id, $session_id, $harness, $ttl), $clock)
    };
}

macro_rules! renew_at {
    ($registry:expr, $memory_id:expr, $session_id:expr, $harness:expr, $ttl:expr, $clock:expr $(,)?) => {
        $registry.renew_at(ClaimLockRenewRequest::new($memory_id, $session_id, $harness, $ttl), $clock)
    };
}

#[test]
fn test_acquire_success() {
    let registry = ClaimLockRegistry::new();
    let now = clock_at(Instant::now(), timestamp(15, 23, 0));

    let result = acquire_at!(&registry, "mem_x", "sess_a", "codex", Duration::from_secs(30), now);

    let lock = assert_acquired(result);
    assert_eq!(lock.memory_id, "mem_x");
    assert_eq!(lock.holder_session_id, "sess_a");
    assert_eq!(lock.holder_harness, "codex");
    assert_eq!(lock.expires_at, timestamp(15, 23, 30));
}

#[test]
fn test_acquire_contention_returns_holder_and_contender_info() {
    let registry = ClaimLockRegistry::new();
    let base = Instant::now();
    acquire_at!(
        &registry,
        "mem_x",
        "sess_a",
        "claude-code",
        Duration::from_secs(60),
        clock_at(base, timestamp(15, 23, 0)),
    );

    let result = acquire_at!(
        &registry,
        "mem_x",
        "sess_b",
        "codex",
        Duration::from_secs(60),
        clock_at(base + Duration::from_secs(5), timestamp(15, 23, 5)),
    );

    let contention = match result {
        ClaimLockAcquireResult::Contended(contention) => contention,
        other => panic!("expected contention, got {other:?}"),
    };
    assert_eq!(contention.warning_code, CLAIM_LOCK_CONTENTION_CODE);
    assert_eq!(contention.memory_id, "mem_x");
    assert_eq!(contention.holder.holder_harness, "claude-code");
    assert_eq!(contention.holder.holder_session_id, "sess_a");
    assert_eq!(contention.contender_harness, "codex");
    assert_eq!(contention.contender_session_id, "sess_b");
    let active_lock = registry.get_at("mem_x", base + Duration::from_secs(5)).unwrap();
    assert_eq!(active_lock.holder_session_id, "sess_b");
    assert_eq!(active_lock.holder_harness, "codex");
}

#[test]
fn test_concurrent_first_acquire_reports_exactly_one_contention() {
    let registry = Arc::new(ClaimLockRegistry::new());
    let barrier = Arc::new(Barrier::new(3));
    let base = Instant::now();
    let clock = clock_at(base, timestamp(15, 23, 0));

    let first = spawn_acquire(registry.clone(), barrier.clone(), ("codex", "sess_a"), clock);
    let second = spawn_acquire(registry.clone(), barrier.clone(), ("claude-code", "sess_b"), clock);

    barrier.wait();

    let results = vec![
        first.join().expect("first acquire thread should not panic"),
        second.join().expect("second acquire thread should not panic"),
    ];

    let acquired_count = results.iter().filter(|result| matches!(result, ClaimLockAcquireResult::Acquired(_))).count();
    let contended_count =
        results.iter().filter(|result| matches!(result, ClaimLockAcquireResult::Contended(_))).count();
    assert_eq!(acquired_count, 1);
    assert_eq!(contended_count, 1);
}

#[test]
fn test_renew_extends_ttl_from_renew_time() {
    let registry = ClaimLockRegistry::new();
    let base = Instant::now();
    acquire_at!(&registry, "mem_x", "sess_a", "codex", Duration::from_secs(5), clock_at(base, timestamp(15, 23, 0)));

    let result = renew_at!(
        &registry,
        "mem_x",
        "sess_a",
        "codex",
        Duration::from_secs(10),
        clock_at(base + Duration::from_secs(2), timestamp(15, 23, 2)),
    );

    let renewed = match result {
        ClaimLockRenewResult::Renewed(lock) => lock,
        other => panic!("expected renewed, got {other:?}"),
    };
    assert_eq!(renewed.expires_at, timestamp(15, 23, 12));
    assert!(registry.get_at("mem_x", base + Duration::from_secs(11)).is_some());
    assert!(registry.get_at("mem_x", base + Duration::from_secs(13)).is_none());
}

#[test]
fn test_renew_only_by_current_holder_before_expiry() {
    let registry = ClaimLockRegistry::new();
    let base = Instant::now();
    acquire_at!(&registry, "mem_x", "sess_a", "codex", Duration::from_secs(5), clock_at(base, timestamp(15, 23, 0)));

    let wrong_holder = renew_at!(
        &registry,
        "mem_x",
        "sess_b",
        "codex",
        Duration::from_secs(10),
        clock_at(base + Duration::from_secs(1), timestamp(15, 23, 1)),
    );
    let expired_holder = renew_at!(
        &registry,
        "mem_x",
        "sess_a",
        "codex",
        Duration::from_secs(10),
        clock_at(base + Duration::from_secs(6), timestamp(15, 23, 6)),
    );

    assert_eq!(wrong_holder, ClaimLockRenewResult::NotHeld);
    assert_eq!(expired_holder, ClaimLockRenewResult::NotHeld);
}

#[test]
fn test_release_clears_lock() {
    let registry = ClaimLockRegistry::new();
    let base = Instant::now();
    acquire_at!(&registry, "mem_x", "sess_a", "codex", Duration::from_secs(30), clock_at(base, timestamp(15, 23, 0)));

    assert!(registry.release("mem_x", "codex", "sess_a").is_some());

    let result = acquire_at!(
        &registry,
        "mem_x",
        "sess_b",
        "claude-code",
        Duration::from_secs(30),
        clock_at(base + Duration::from_secs(1), timestamp(15, 23, 1)),
    );
    assert_eq!(assert_acquired(result).holder_session_id, "sess_b");
}

#[test]
fn test_release_only_by_current_holder() {
    let registry = ClaimLockRegistry::new();
    let base = Instant::now();
    acquire_at!(&registry, "mem_x", "sess_a", "codex", Duration::from_secs(30), clock_at(base, timestamp(15, 23, 0)));

    assert!(registry.release("mem_x", "codex", "sess_b").is_none());
    assert_eq!(registry.get_at("mem_x", base).unwrap().holder_session_id, "sess_a");
}

#[test]
fn test_same_session_id_different_harness_is_not_holder() {
    let registry = ClaimLockRegistry::new();
    let base = Instant::now();
    acquire_at!(&registry, "mem_x", "sess_a", "codex", Duration::from_secs(30), clock_at(base, timestamp(15, 23, 0)));

    let wrong_harness_renew = renew_at!(
        &registry,
        "mem_x",
        "sess_a",
        "claude-code",
        Duration::from_secs(30),
        clock_at(base + Duration::from_secs(1), timestamp(15, 23, 1)),
    );

    assert_eq!(wrong_harness_renew, ClaimLockRenewResult::NotHeld);
    assert!(registry.release("mem_x", "claude-code", "sess_a").is_none());
    assert!(registry.release_all_held_by("claude-code", "sess_a").is_empty());
    assert_eq!(registry.get_at("mem_x", base + Duration::from_secs(2)).unwrap().holder_harness, "codex");
}

#[test]
fn test_release_does_not_remove_new_contender_lock() {
    let registry = ClaimLockRegistry::new();
    let base = Instant::now();
    acquire_at!(&registry, "mem_x", "sess_a", "codex", Duration::from_secs(30), clock_at(base, timestamp(15, 23, 0)));
    acquire_at!(
        &registry,
        "mem_x",
        "sess_b",
        "claude-code",
        Duration::from_secs(30),
        clock_at(base + Duration::from_secs(1), timestamp(15, 23, 1)),
    );

    assert!(registry.release("mem_x", "codex", "sess_a").is_none());
    let active_lock = registry.get_at("mem_x", base + Duration::from_secs(2)).unwrap();
    assert_eq!(active_lock.holder_harness, "claude-code");
    assert_eq!(active_lock.holder_session_id, "sess_b");
}

#[test]
fn test_restore_previous_holder_after_failed_contention() {
    let registry = ClaimLockRegistry::new();
    let base = Instant::now();
    let utc = Utc::now();
    acquire_at!(&registry, "mem_x", "sess_a", "codex", Duration::from_secs(60), clock_at(base, utc));
    let previous_holder = match acquire_at!(
        &registry,
        "mem_x",
        "sess_b",
        "claude-code",
        Duration::from_secs(60),
        clock_at(base + Duration::from_secs(1), utc + chrono::Duration::seconds(1)),
    ) {
        ClaimLockAcquireResult::Contended(contention) => contention.holder,
        other => panic!("expected contention, got {other:?}"),
    };

    assert!(registry.release("mem_x", "claude-code", "sess_b").is_some());
    let restored = registry.restore(previous_holder).expect("restore previous holder");

    assert_eq!(restored.holder_harness, "codex");
    assert_eq!(restored.holder_session_id, "sess_a");
    let active_lock = registry.get("mem_x").expect("restored lock should be active");
    assert_eq!(active_lock.holder_harness, "codex");
    assert_eq!(active_lock.holder_session_id, "sess_a");
}

#[test]
fn test_restore_does_not_replace_unrelated_live_holder() {
    let registry = ClaimLockRegistry::new();
    let base = Instant::now();
    let utc = Utc::now();
    acquire_at!(&registry, "mem_x", "sess_a", "codex", Duration::from_secs(60), clock_at(base, utc));
    let previous_holder = match acquire_at!(
        &registry,
        "mem_x",
        "sess_b",
        "claude-code",
        Duration::from_secs(60),
        clock_at(base + Duration::from_secs(1), utc + chrono::Duration::seconds(1)),
    ) {
        ClaimLockAcquireResult::Contended(contention) => contention.holder,
        other => panic!("expected contention, got {other:?}"),
    };
    acquire_at!(
        &registry,
        "mem_x",
        "sess_c",
        "cursor",
        Duration::from_secs(60),
        clock_at(base + Duration::from_secs(2), utc + chrono::Duration::seconds(2)),
    );

    assert!(registry.restore(previous_holder).is_none());

    let active_lock = registry.get_at("mem_x", base + Duration::from_secs(3)).expect("current holder remains");
    assert_eq!(active_lock.holder_harness, "cursor");
    assert_eq!(active_lock.holder_session_id, "sess_c");
}

#[test]
fn test_expired_sweep_does_not_remove_reacquired_live_lock() {
    let registry = ClaimLockRegistry::new();
    let base = Instant::now();
    acquire_at!(&registry, "mem_x", "sess_a", "codex", Duration::from_secs(1), clock_at(base, timestamp(15, 23, 0)));
    acquire_at!(
        &registry,
        "mem_x",
        "sess_b",
        "claude-code",
        Duration::from_secs(30),
        clock_at(base + Duration::from_secs(2), timestamp(15, 23, 2)),
    );

    assert!(registry.sweep_expired_at(base + Duration::from_secs(2)).is_empty());
    let active_lock = registry.get_at("mem_x", base + Duration::from_secs(3)).unwrap();
    assert_eq!(active_lock.holder_harness, "claude-code");
    assert_eq!(active_lock.holder_session_id, "sess_b");
}

#[test]
fn test_ttl_expiry() {
    let registry = ClaimLockRegistry::new();
    let base = Instant::now();
    acquire_at!(&registry, "mem_x", "sess_a", "codex", Duration::from_secs(1), clock_at(base, timestamp(15, 23, 0)));

    let released = registry.sweep_expired_at(base + Duration::from_secs(2));

    assert_eq!(released.len(), 1);
    assert_eq!(released[0].memory_id, "mem_x");
    assert!(registry.get_at("mem_x", base + Duration::from_secs(2)).is_none());
}

#[test]
fn test_expired_lock_can_be_reacquired_by_another_session() {
    let registry = ClaimLockRegistry::new();
    let base = Instant::now();
    acquire_at!(&registry, "mem_x", "sess_a", "codex", Duration::from_secs(1), clock_at(base, timestamp(15, 23, 0)));

    let result = acquire_at!(
        &registry,
        "mem_x",
        "sess_b",
        "claude-code",
        Duration::from_secs(10),
        clock_at(base + Duration::from_secs(2), timestamp(15, 23, 2)),
    );

    let lock = assert_acquired(result);
    assert_eq!(lock.holder_session_id, "sess_b");
    assert_eq!(lock.expires_at, timestamp(15, 23, 12));
}

#[test]
fn test_contention_warn_not_refuse() {
    let registry = ClaimLockRegistry::new();
    let base = Instant::now();
    acquire_at!(
        &registry,
        "mem_x",
        "sess_a",
        "claude-code",
        Duration::from_secs(60),
        clock_at(base, timestamp(15, 23, 0)),
    );

    let result = acquire_at!(
        &registry,
        "mem_x",
        "sess_b",
        "codex",
        Duration::from_secs(60),
        clock_at(base + Duration::from_secs(1), timestamp(15, 23, 1)),
    );

    let contention = match result {
        ClaimLockAcquireResult::Contended(contention) => contention,
        other => panic!("expected contention warning, got {other:?}"),
    };
    assert_eq!(contention.warning_code, "claim_lock_contention");
    assert!(contention.message.contains("mem_x"));
    assert!(contention.message.contains("claude-code:sess_a"));
    assert_eq!(contention.holder_label(), "claude-code:sess_a");
    assert_eq!(contention.contender_label(), "codex:sess_b");
}

#[test]
fn test_stale_session_releases_lock() {
    let registry = ClaimLockRegistry::new();
    let base = Instant::now();
    acquire_at!(&registry, "mem_x", "sess_a", "codex", Duration::from_secs(60), clock_at(base, timestamp(15, 23, 0)));

    let released = registry.release_all_held_by("codex", "sess_a");

    assert_eq!(released.len(), 1);
    assert_eq!(released[0].memory_id, "mem_x");
    assert!(registry.get_at("mem_x", base).is_none());
}

#[test]
fn test_release_all_held_by_multiple() {
    let registry = ClaimLockRegistry::new();
    let base = Instant::now();
    for memory_id in ["mem_a", "mem_b", "mem_c"] {
        acquire_at!(
            &registry,
            memory_id,
            "sess_a",
            "codex",
            Duration::from_secs(60),
            clock_at(base, timestamp(15, 23, 0)),
        );
    }
    acquire_at!(
        &registry,
        "mem_other",
        "sess_b",
        "codex",
        Duration::from_secs(60),
        clock_at(base, timestamp(15, 23, 0)),
    );

    let mut released_ids =
        registry.release_all_held_by("codex", "sess_a").into_iter().map(|lock| lock.memory_id).collect::<Vec<_>>();
    released_ids.sort();

    assert_eq!(released_ids, vec!["mem_a".to_string(), "mem_b".to_string(), "mem_c".to_string()]);
    assert!(registry.get_at("mem_a", base).is_none());
    assert!(registry.get_at("mem_b", base).is_none());
    assert!(registry.get_at("mem_c", base).is_none());
    assert_eq!(registry.get_at("mem_other", base).unwrap().holder_session_id, "sess_b");
}

fn assert_acquired(result: ClaimLockAcquireResult) -> memorum_coordination::ClaimLockInfo {
    match result {
        ClaimLockAcquireResult::Acquired(lock) => lock,
        other => panic!("expected acquired lock, got {other:?}"),
    }
}

fn spawn_acquire(
    registry: Arc<ClaimLockRegistry>,
    barrier: Arc<Barrier>,
    owner: (&'static str, &'static str),
    clock: ClaimLockClock,
) -> std::thread::JoinHandle<ClaimLockAcquireResult> {
    std::thread::spawn(move || {
        barrier.wait();
        let (harness, session_id) = owner;
        acquire_at!(&registry, "mem_concurrent", session_id, harness, Duration::from_secs(30), clock)
    })
}

fn clock_at(instant: Instant, utc: DateTime<Utc>) -> ClaimLockClock {
    ClaimLockClock { instant, utc }
}

fn timestamp(hour: u32, minute: u32, second: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 5, 1, hour, minute, second).single().expect("timestamp should be valid")
}
