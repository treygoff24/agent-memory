use chrono::{DateTime, Utc};
use memory_substrate::events::{Event, EventKind};
use memory_substrate::*;
use serde_json::{json, Value};

#[test]
fn every_current_event_kind_has_canonical_json_fixture() {
    let fixtures = event_kind_fixtures();
    assert_eq!(fixtures.len(), 16, "update schema fixtures when EventKind changes");

    for (index, fixture) in fixtures.into_iter().enumerate() {
        assert_eq!(event_kind_tag(&fixture.kind), fixture.tag);
        assert_eq!(serde_json::to_value(&fixture.kind).expect("serialize event kind"), fixture.canonical);

        let decoded_kind: EventKind =
            serde_json::from_value(fixture.canonical.clone()).expect("decode event kind fixture");
        assert_eq!(decoded_kind, fixture.kind);

        let event_json = canonical_event_json(index, fixture.canonical);
        let decoded_event: Event = serde_json::from_value(event_json.clone()).expect("decode event fixture");
        assert_eq!(decoded_event.operation_id.as_ref().expect("operation_id").as_str(), format!("op_{index}"));
        assert_eq!(event_kind_tag(&decoded_event.kind), fixture.tag);
        assert_eq!(serde_json::to_value(&decoded_event).expect("serialize event fixture"), event_json);
    }
}

struct EventKindFixture {
    tag: &'static str,
    kind: EventKind,
    canonical: Value,
}

fn event_kind_fixtures() -> Vec<EventKindFixture> {
    let first_memory_id = memory_id("000001");
    let path = RepoPath::new("agent/patterns/mem_20260424_a1b2c3d4e5f60718_000001.md");
    vec![
        fixture(
            "write_committed",
            EventKind::WriteCommitted {
                id: first_memory_id.clone(),
                path: path.clone(),
                classification: ClassificationOutcome::Trusted,
            },
            json!({"kind":"write_committed","data":{"id":first_memory_id.as_str(),"path":path.as_path(),"classification":"trusted"}}),
        ),
        fixture(
            "encrypted_write_committed",
            EventKind::EncryptedWriteCommitted {
                id: memory_id("000002"),
                path: RepoPath::new("encrypted/agent/patterns/mem_20260424_a1b2c3d4e5f60718_000002.md"),
                classification: ClassificationOutcome::RequiresEncryption,
            },
            json!({
                "kind":"encrypted_write_committed",
                "data":{
                    "id":"mem_20260424_a1b2c3d4e5f60718_000002",
                    "path":"encrypted/agent/patterns/mem_20260424_a1b2c3d4e5f60718_000002.md",
                    "classification":"requires_encryption"
                }
            }),
        ),
        fixture(
            "tombstone_committed",
            EventKind::TombstoneCommitted { id: memory_id("000003") },
            json!({"kind":"tombstone_committed","data":{"id":"mem_20260424_a1b2c3d4e5f60718_000003"}}),
        ),
        fixture(
            "duplicate_id_repaired",
            EventKind::DuplicateIdRepaired { old_id: memory_id("000004"), new_id: memory_id("000005") },
            json!({
                "kind":"duplicate_id_repaired",
                "data":{
                    "old_id":"mem_20260424_a1b2c3d4e5f60718_000004",
                    "new_id":"mem_20260424_a1b2c3d4e5f60718_000005"
                }
            }),
        ),
        fixture(
            "embedding_model_changed",
            EventKind::EmbeddingModelChanged { chunks_requeued: 7 },
            json!({"kind":"embedding_model_changed","data":{"chunks_requeued":7}}),
        ),
        fixture(
            "startup_reconciliation_completed",
            EventKind::StartupReconciliationCompleted { reindexed: 3, repaired_events: 2 },
            json!({"kind":"startup_reconciliation_completed","data":{"reindexed":3,"repaired_events":2}}),
        ),
        fixture(
            "operator_repair_required",
            EventKind::OperatorRepairRequired { reason: "fixture".to_string() },
            json!({"kind":"operator_repair_required","data":{"reason":"fixture"}}),
        ),
        fixture(
            "git_push_failed",
            EventKind::GitPushFailed { reason: "network unavailable".to_string() },
            json!({"kind":"git_push_failed","data":{"reason":"network unavailable"}}),
        ),
        fixture(
            "write_refused",
            EventKind::WriteRefused {
                id: Some(memory_id("000006")),
                path: Some(RepoPath::new("agent/patterns/mem_20260424_a1b2c3d4e5f60718_000006.md")),
                classification: ClassificationOutcome::Secret,
                reason: "secret content refused".to_string(),
            },
            json!({
                "kind":"write_refused",
                "data":{
                    "id":"mem_20260424_a1b2c3d4e5f60718_000006",
                    "path":"agent/patterns/mem_20260424_a1b2c3d4e5f60718_000006.md",
                    "classification":"secret",
                    "reason":"secret content refused"
                }
            }),
        ),
        fixture(
            "encrypted_content_revealed",
            EventKind::EncryptedContentRevealed {
                id: memory_id("000007"),
                reason: "user requested reveal".to_string(),
            },
            json!({"kind":"encrypted_content_revealed","data":{"id":"mem_20260424_a1b2c3d4e5f60718_000007","reason":"user requested reveal"}}),
        ),
        fixture(
            "substrate_fragment_written",
            EventKind::SubstrateFragmentWritten {
                id: "sub_01HZXJK7J7W0X4Q4KJ7A2R8V1A".to_string(),
                path: RepoPath::new("substrate/dev_test/2026-04-30.jsonl"),
                classification: ClassificationOutcome::Trusted,
            },
            json!({
                "kind":"substrate_fragment_written",
                "data":{
                    "id":"sub_01HZXJK7J7W0X4Q4KJ7A2R8V1A",
                    "path":"substrate/dev_test/2026-04-30.jsonl",
                    "classification":"trusted"
                }
            }),
        ),
        fixture(
            "recall_hit",
            EventKind::RecallHit { id: memory_id("000008"), recalled_at: fixed_time() },
            json!({"kind":"recall_hit","data":{"id":"mem_20260424_a1b2c3d4e5f60718_000008","recalled_at":"2026-01-02T03:04:05Z"}}),
        ),
        fixture(
            "reality_check_confirmed",
            EventKind::RealityCheckConfirmed { id: memory_id("000009"), session_id: "rc_session_1".to_string() },
            json!({"kind":"reality_check_confirmed","data":{"id":"mem_20260424_a1b2c3d4e5f60718_000009","session_id":"rc_session_1"}}),
        ),
        fixture(
            "reality_check_forgotten",
            EventKind::RealityCheckForgotten {
                id: memory_id("000010"),
                session_id: "rc_session_1".to_string(),
                reason: "stale".to_string(),
            },
            json!({"kind":"reality_check_forgotten","data":{"id":"mem_20260424_a1b2c3d4e5f60718_000010","session_id":"rc_session_1","reason":"stale"}}),
        ),
        fixture(
            "reality_check_not_relevant",
            EventKind::RealityCheckNotRelevant { id: memory_id("000011"), session_id: "rc_session_1".to_string() },
            json!({"kind":"reality_check_not_relevant","data":{"id":"mem_20260424_a1b2c3d4e5f60718_000011","session_id":"rc_session_1"}}),
        ),
        fixture(
            "claim_lock_contention",
            EventKind::ClaimLockContention {
                memory_id: memory_id("000012"),
                holder: "session_holder".to_string(),
                contender: "session_contender".to_string(),
            },
            json!({"kind":"claim_lock_contention","data":{"memory_id":"mem_20260424_a1b2c3d4e5f60718_000012","holder":"session_holder","contender":"session_contender"}}),
        ),
    ]
}

fn fixture(tag: &'static str, kind: EventKind, canonical: Value) -> EventKindFixture {
    EventKindFixture { tag, kind, canonical }
}

fn canonical_event_json(index: usize, kind: Value) -> Value {
    json!({
        "schema": memory_substrate::SUBSTRATE_SCHEMA_VERSION,
        "id": format!("evt_{index}"),
        "ts": "2026-01-02T03:04:05Z",
        "device": "dev_test",
        "seq": index as u64,
        "operation_id": format!("op_{index}"),
        "kind": kind["kind"],
        "data": kind["data"],
        "crc32c": 0
    })
}

fn fixed_time() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-01-02T03:04:05Z").expect("fixture timestamp").with_timezone(&Utc)
}

fn memory_id(suffix: &str) -> MemoryId {
    MemoryId::new(format!("mem_20260424_a1b2c3d4e5f60718_{suffix}"))
}

fn event_kind_tag(kind: &EventKind) -> &'static str {
    match kind {
        EventKind::WriteCommitted { .. } => "write_committed",
        EventKind::EncryptedWriteCommitted { .. } => "encrypted_write_committed",
        EventKind::TombstoneCommitted { .. } => "tombstone_committed",
        EventKind::DuplicateIdRepaired { .. } => "duplicate_id_repaired",
        EventKind::EmbeddingModelChanged { .. } => "embedding_model_changed",
        EventKind::StartupReconciliationCompleted { .. } => "startup_reconciliation_completed",
        EventKind::OperatorRepairRequired { .. } => "operator_repair_required",
        EventKind::GitPushFailed { .. } => "git_push_failed",
        EventKind::WriteRefused { .. } => "write_refused",
        EventKind::EncryptedContentRevealed { .. } => "encrypted_content_revealed",
        EventKind::SubstrateFragmentWritten { .. } => "substrate_fragment_written",
        EventKind::RecallHit { .. } => "recall_hit",
        EventKind::RealityCheckConfirmed { .. } => "reality_check_confirmed",
        EventKind::RealityCheckForgotten { .. } => "reality_check_forgotten",
        EventKind::RealityCheckNotRelevant { .. } => "reality_check_not_relevant",
        EventKind::ClaimLockContention { .. } => "claim_lock_contention",
    }
}
