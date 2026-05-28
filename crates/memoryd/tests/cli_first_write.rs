//! Test fixture for the T12 first-write success signal. The detection logic is
//! pure CLI-side — given a `StatusResponse`, decide whether to emit the banner —
//! so these tests exercise the public helper in `memoryd::first_write` without
//! spinning up a daemon. The end-to-end "daemon plus CLI under DaemonScaffold"
//! exercise belongs in T08's import_end_to_end harness because it cohabits with
//! the importer's daemon-state assertions.

use memoryd::first_write::{emit_first_write_banner, should_emit_first_write_banner};
use memoryd::protocol::{IndexStats, StatusResponse};

fn status_with_active_count(count: u64) -> StatusResponse {
    StatusResponse {
        index_stats: Some(IndexStats { active_memories: count, last_reindex: None }),
        ..StatusResponse::default()
    }
}

#[test]
fn first_write_fires_banner_on_active_count_one() {
    assert!(should_emit_first_write_banner(&status_with_active_count(1)));
}

#[test]
fn second_write_does_not_fire_banner() {
    // Active count is 2 after the second write; banner must not re-fire.
    assert!(!should_emit_first_write_banner(&status_with_active_count(2)));
}

#[test]
fn banner_skipped_when_index_stats_missing() {
    // A status response without index stats means the SQLite index isn't
    // available yet (very early startup). Don't emit a banner with no evidence.
    let status = StatusResponse { index_stats: None, ..StatusResponse::default() };
    assert!(!should_emit_first_write_banner(&status));
}

#[test]
fn banner_format_matches_locked_shape() {
    let mut buf = Vec::new();
    emit_first_write_banner(&mut buf, "mem_20260527_a1b2c3d4e5f60718_000001").expect("write to in-memory buffer");
    let banner = String::from_utf8(buf).expect("banner is utf-8");

    // Locked banner shape from the plan T12 deliverable. The exact format keeps
    // docs and screenshots stable across releases; a downstream test catching a
    // format regression here is the point.
    assert!(banner.contains("✓ First memory saved: mem_20260527_a1b2c3d4e5f60718_000001"));
    assert!(banner.contains("memoryd get --id mem_20260527_a1b2c3d4e5f60718_000001"));
    assert!(banner.contains("memoryd search \"\""));
    assert!(banner.contains("docs/getting-started.md"));
}

#[test]
fn daemon_restart_does_not_re_emit_banner() {
    // A daemon restart preserves the SQLite-backed active_memories counter. The
    // banner skips on count > 1 regardless of whether the process holding the
    // counter is freshly started — this test locks that invariant by checking
    // count=5 (post-restart) does not re-fire.
    assert!(!should_emit_first_write_banner(&status_with_active_count(5)));
    assert!(!should_emit_first_write_banner(&status_with_active_count(0)));
}
