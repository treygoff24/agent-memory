use chrono::{TimeZone, Utc};
use memory_substrate::events::EventKind;
use memory_substrate::MemoryId;

#[test]
fn recall_hit_round_trips_serde() {
    let recalled_at = Utc.with_ymd_and_hms(2026, 5, 1, 12, 0, 0).single().expect("fixture time");
    let kind = EventKind::RecallHit { id: MemoryId::new("mem_20260501_a1b2c3d4e5f60718_000001"), recalled_at };

    let encoded = serde_json::to_value(&kind).expect("serialize recall hit");
    assert_eq!(encoded["kind"], "recall_hit");
    assert_eq!(encoded["data"]["id"], "mem_20260501_a1b2c3d4e5f60718_000001");
    assert_eq!(encoded["data"]["recalled_at"], "2026-05-01T12:00:00Z");

    let decoded: EventKind = serde_json::from_value(encoded).expect("deserialize recall hit");
    assert_eq!(decoded, kind);
}

#[test]
fn reality_check_variants_round_trip_serde() {
    let memory_id = MemoryId::new("mem_20260501_a1b2c3d4e5f60718_000002");
    let variants = vec![
        EventKind::RealityCheckConfirmed { id: memory_id.clone(), session_id: "rc_sess_1".to_string() },
        EventKind::RealityCheckForgotten {
            id: memory_id.clone(),
            session_id: "rc_sess_1".to_string(),
            reason: "stale source".to_string(),
        },
        EventKind::RealityCheckNotRelevant { id: memory_id, session_id: "rc_sess_1".to_string() },
    ];

    for kind in variants {
        let encoded = serde_json::to_value(&kind).expect("serialize reality check variant");
        let decoded: EventKind = serde_json::from_value(encoded).expect("deserialize reality check variant");
        assert_eq!(decoded, kind);
    }
}

#[test]
fn claim_lock_contention_round_trips_serde() {
    let kind = EventKind::ClaimLockContention {
        memory_id: MemoryId::new("mem_20260501_a1b2c3d4e5f60718_000003"),
        holder: "claude-code:sess_a".to_string(),
        contender: "codex:sess_b".to_string(),
    };

    let encoded = serde_json::to_value(&kind).expect("serialize contention");
    assert_eq!(encoded["kind"], "claim_lock_contention");
    assert_eq!(encoded["data"]["memory_id"], "mem_20260501_a1b2c3d4e5f60718_000003");

    let decoded: EventKind = serde_json::from_value(encoded).expect("deserialize contention");
    assert_eq!(decoded, kind);
}
