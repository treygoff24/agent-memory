use chrono::{Duration, Utc};
use serde_json::json;
use tempfile::TempDir;

#[path = "../src/state.rs"]
mod state;

use state::{DaemonState, RcPendingCache, RcSessionState, RcSessionStore, StateLoadFailure};

#[test]
fn test_state_json_loads_cleanly() {
    let temp = TempDir::new().expect("tempdir");
    let state_dir = temp.path().join("state");
    std::fs::create_dir_all(&state_dir).expect("state dir");
    let last_completed_at = Utc::now() - Duration::days(2);
    let snooze_until = Utc::now() + Duration::days(1);
    std::fs::write(
        state_dir.join("state.json"),
        serde_json::to_vec_pretty(&json!({
            "version": 1,
            "reality_check": {
                "last_completed_at": last_completed_at,
                "snooze_until": snooze_until
            }
        }))
        .expect("state serializes"),
    )
    .expect("state writes");

    let state = DaemonState::load(temp.path());

    assert_eq!(state.reality_check.last_completed_at, Some(last_completed_at));
    assert_eq!(state.reality_check.snooze_until, Some(snooze_until));
}

#[test]
fn test_state_json_missing_treated_as_defaults() {
    let temp = TempDir::new().expect("tempdir");

    let state = DaemonState::load(temp.path());

    assert_eq!(state.reality_check.last_completed_at, None);
    assert_eq!(state.reality_check.snooze_until, None);
}

#[test]
fn test_state_json_corrupt_treated_as_defaults() {
    let temp = TempDir::new().expect("tempdir");
    let state_dir = temp.path().join("state");
    std::fs::create_dir_all(&state_dir).expect("state dir");
    std::fs::write(state_dir.join("state.json"), b"not json").expect("state writes");

    let state = DaemonState::load(temp.path());

    assert_eq!(state, DaemonState::default());
}

#[test]
fn test_state_json_corrupt_reports_fallback_reason() {
    let temp = TempDir::new().expect("tempdir");
    let state_dir = temp.path().join("state");
    std::fs::create_dir_all(&state_dir).expect("state dir");
    std::fs::write(state_dir.join("state.json"), b"not json").expect("state writes");

    let report = DaemonState::load_with_report(temp.path());

    assert_eq!(report.state, DaemonState::default());
    assert!(matches!(report.failure, Some(StateLoadFailure::Parse { .. })));
}

#[test]
fn test_state_json_version_mismatch_treated_as_defaults() {
    let temp = TempDir::new().expect("tempdir");
    let state_dir = temp.path().join("state");
    std::fs::create_dir_all(&state_dir).expect("state dir");
    std::fs::write(
        state_dir.join("state.json"),
        serde_json::to_vec_pretty(&json!({
            "version": 99,
            "reality_check": {
                "last_completed_at": Utc::now(),
                "snooze_until": Utc::now()
            }
        }))
        .expect("state serializes"),
    )
    .expect("state writes");

    let state = DaemonState::load(temp.path());

    assert_eq!(state, DaemonState::default());
}

#[test]
fn test_state_json_version_mismatch_reports_fallback_reason() {
    let temp = TempDir::new().expect("tempdir");
    let state_dir = temp.path().join("state");
    std::fs::create_dir_all(&state_dir).expect("state dir");
    std::fs::write(state_dir.join("state.json"), b"{\"version\":99,\"reality_check\":{}}").expect("state writes");

    let report = DaemonState::load_with_report(temp.path());

    assert_eq!(report.state, DaemonState::default());
    assert!(matches!(report.failure, Some(StateLoadFailure::VersionMismatch { expected: 1, actual: 99 })));
}

#[test]
fn test_state_json_write_atomic() {
    let temp = TempDir::new().expect("tempdir");
    let state = DaemonState {
        reality_check: state::RealityCheckState { last_completed_at: Some(Utc::now()), snooze_until: None },
        ..DaemonState::default()
    };

    state.save(temp.path()).expect("state saves");

    let state_file = temp.path().join("state").join("state.json");
    assert!(state_file.exists(), "state file should be written under runtime state root");
    assert!(!temp.path().join("state").join("state.json.tmp").exists(), "tmp file should not remain");
    let reloaded = DaemonState::load(temp.path());
    assert_eq!(reloaded.reality_check.last_completed_at, state.reality_check.last_completed_at);
}

#[test]
fn test_pending_json_stale_triggers_recompute() {
    let cache = RcPendingCache {
        computed_at: Utc::now() - Duration::minutes(31),
        items: vec![json!({"id": "mem_20260501_0000000000000001_000001"})],
        ..RcPendingCache::default()
    };

    assert!(!cache.is_fresh(Utc::now()));
}

#[test]
fn test_pending_json_fresh_returns_cached() {
    let cache = RcPendingCache { computed_at: Utc::now() - Duration::minutes(5), ..RcPendingCache::default() };

    assert!(cache.is_fresh(Utc::now()));
}

#[test]
fn test_pending_json_save_and_load_round_trips_under_runtime_state_root() {
    let temp = TempDir::new().expect("tempdir");
    let cache = RcPendingCache {
        computed_at: Utc::now() - Duration::minutes(5),
        items: vec![json!({"id": "mem_20260501_0000000000000001_000001"})],
        ..RcPendingCache::default()
    };

    cache.save(temp.path()).expect("pending cache saves");
    let loaded = RcPendingCache::load(temp.path()).expect("pending cache loads");

    assert_eq!(loaded, cache);
    assert!(temp.path().join("state").join("reality-check-pending.json").exists());
    assert!(!temp.path().join("reality-check-pending.json").exists());
}

#[test]
fn test_pending_json_delete_removes_runtime_state_file() {
    let temp = TempDir::new().expect("tempdir");
    let cache = RcPendingCache { computed_at: Utc::now() - Duration::minutes(5), ..RcPendingCache::default() };
    cache.save(temp.path()).expect("pending cache saves");

    RcPendingCache::delete(temp.path()).expect("pending cache deletes");

    assert!(!temp.path().join("state").join("reality-check-pending.json").exists());
}

#[test]
fn test_session_json_old_auto_discarded() {
    let temp = TempDir::new().expect("tempdir");
    let store = RcSessionStore::new(temp.path());
    let session = RcSessionState {
        session_id: "rcs_old".to_string(),
        started_at: Utc::now() - Duration::days(8),
        items_total: 12,
        current_index: 5,
        ..RcSessionState::default()
    };
    store.save(&session).expect("session saves");

    let loaded = store.load_if_recent(Utc::now()).expect("session load recovers");

    assert_eq!(loaded, None);
    assert!(!temp.path().join("state").join("reality-check-session.json").exists());
}

#[test]
fn test_session_json_corrupt_renamed() {
    let temp = TempDir::new().expect("tempdir");
    let state_dir = temp.path().join("state");
    std::fs::create_dir_all(&state_dir).expect("state dir");
    std::fs::write(state_dir.join("reality-check-session.json"), b"not json").expect("session writes");
    let store = RcSessionStore::new(temp.path());

    let loaded = store.load_if_recent(Utc::now()).expect("session load recovers");

    assert_eq!(loaded, None);
    assert!(!state_dir.join("reality-check-session.json").exists());
    let corrupt_files = std::fs::read_dir(&state_dir)
        .expect("state dir reads")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("reality-check-session.json.corrupt-"))
        .count();
    assert_eq!(corrupt_files, 1);
}

#[test]
fn test_session_json_valid_loaded() {
    let temp = TempDir::new().expect("tempdir");
    let store = RcSessionStore::new(temp.path());
    let session = RcSessionState {
        session_id: "rcs_valid".to_string(),
        started_at: Utc::now() - Duration::days(1),
        items_total: 12,
        items_reviewed: vec!["mem_20260501_0000000000000001_000001".to_string()],
        items_deferred: vec![],
        items_remaining: vec!["mem_20260501_0000000000000001_000002".to_string()],
        current_index: 1,
        ..RcSessionState::default()
    };
    store.save(&session).expect("session saves");

    let loaded = store.load_if_recent(Utc::now()).expect("session loads");

    assert_eq!(loaded, Some(session));
}

#[test]
fn test_session_json_delete_removes_session_file() {
    let temp = TempDir::new().expect("tempdir");
    let store = RcSessionStore::new(temp.path());
    store.save(&RcSessionState::default()).expect("session saves");

    store.delete().expect("session deletes");

    assert!(!temp.path().join("state").join("reality-check-session.json").exists());
}
