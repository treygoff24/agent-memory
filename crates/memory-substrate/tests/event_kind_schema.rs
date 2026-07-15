use memory_substrate::events::{Event, EventKind};
use memory_substrate::*;

#[test]
fn every_current_event_kind_has_typed_payload_fixture() {
    let events = vec![
        EventKind::WriteCommitted {
            id: MemoryId::new("mem_20260424_a1b2c3d4e5f60718_000001"),
            path: RepoPath::new("agent/patterns/mem_20260424_a1b2c3d4e5f60718_000001.md"),
            classification: ClassificationOutcome::Trusted,
        },
        EventKind::EncryptedWriteCommitted {
            id: MemoryId::new("mem_20260424_a1b2c3d4e5f60718_000002"),
            path: RepoPath::new("encrypted/agent/patterns/mem_20260424_a1b2c3d4e5f60718_000002.md"),
            classification: ClassificationOutcome::RequiresEncryption,
        },
        EventKind::MetadataAmended {
            id: MemoryId::new("mem_20260424_a1b2c3d4e5f60718_000006"),
            path: RepoPath::new("agent/patterns/mem_20260424_a1b2c3d4e5f60718_000006.md"),
            actor: "memoryd-abstraction-compile".to_string(),
            changed_fields: vec!["abstraction".to_string(), "cues".to_string()],
        },
        EventKind::TombstoneCommitted { id: MemoryId::new("mem_20260424_a1b2c3d4e5f60718_000003") },
        EventKind::DuplicateIdRepaired {
            old_id: MemoryId::new("mem_20260424_a1b2c3d4e5f60718_000004"),
            new_id: MemoryId::new("mem_20260424_a1b2c3d4e5f60718_000005"),
        },
        EventKind::EmbeddingModelChanged { chunks_requeued: 1 },
        EventKind::StartupReconciliationCompleted { reindexed: 1, repaired_events: 1 },
        EventKind::OperatorRepairRequired { reason: "fixture".to_string() },
        EventKind::GitPushFailed { reason: "fixture".to_string() },
        EventKind::SubstrateFragmentWritten {
            id: "sub_01HZXJK7J7W0X4Q4KJ7A2R8V1A".to_string(),
            path: RepoPath::new("substrate/dev_test/2026-04-30.jsonl"),
            classification: ClassificationOutcome::Trusted,
        },
    ];
    assert_eq!(events.len(), 10);
    for (index, kind) in events.into_iter().enumerate() {
        let event = Event {
            schema: memory_substrate::SUBSTRATE_SCHEMA_VERSION,
            id: EventId::new(format!("evt_{index}")),
            at: chrono::Utc::now(),
            device: DeviceId::new("dev_test"),
            seq: 0,
            operation_id: Some(OperationId::new(format!("op_{index}"))),
            kind,
            crc32c: 0,
        };
        let encoded = serde_json::to_value(&event).expect("serialize event");
        let decoded: Event = serde_json::from_value(encoded).expect("typed event round trip");
        assert_eq!(decoded.operation_id.as_ref().expect("operation_id").as_str(), format!("op_{index}"));
    }
}
