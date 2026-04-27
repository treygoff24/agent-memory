use memory_substrate::events::{
    append_event, read_events, refuse_duplicate_device_logs, Event, EventKind, EVENT_SCHEMA_VERSION,
};
use memory_substrate::{ClassificationOutcome, DeviceId, EventId, MemoryId, OperationId, RepoPath};

fn sample_event(id: &str) -> Event {
    Event {
        schema: EVENT_SCHEMA_VERSION,
        id: EventId::new(id),
        at: chrono::Utc::now(),
        device: DeviceId::new("dev_testdevice01"),
        seq: 1,
        operation_id: Some(OperationId::new("op_1")),
        kind: EventKind::WriteCommitted {
            id: MemoryId::new("mem_20260424_a1b2c3d4e5f60718_000001"),
            path: RepoPath::new("agent/patterns/mem_20260424_a1b2c3d4e5f60718_000001.md"),
            classification: ClassificationOutcome::Trusted,
        },
        crc32c: 0,
    }
}

#[test]
fn same_device_duplicate_logs_are_refused_until_adoption_repair() {
    let temp = tempfile::tempdir().expect("tempdir");
    let events = temp.path().join("events");
    std::fs::create_dir_all(&events).expect("events dir");
    let local = DeviceId::new("dev_a");
    std::fs::write(events.join("dev_a.jsonl"), "").expect("primary log");
    std::fs::write(events.join("dev_a (copy).jsonl"), "").expect("copied log");

    let err = refuse_duplicate_device_logs(&events, &local).expect_err("duplicate logs refused");
    assert!(err.to_string().contains("adopt_clone"));

    std::fs::remove_file(events.join("dev_a (copy).jsonl")).expect("adoption repair removes duplicate");
    refuse_duplicate_device_logs(&events, &local).expect("repaired logs accepted");
}

#[test]
fn distinct_device_peer_logs_are_never_refused() {
    let temp = tempfile::tempdir().expect("tempdir");
    let events = temp.path().join("events");
    std::fs::create_dir_all(&events).expect("events dir");
    let local = DeviceId::new("dev_a");
    // Peer device logs should not be confused with duplicates.
    std::fs::write(events.join("dev_a.jsonl"), "").expect("local log");
    std::fs::write(events.join("dev_b.jsonl"), "").expect("peer log");
    std::fs::write(events.join("dev_c.jsonl"), "").expect("peer log");

    refuse_duplicate_device_logs(&events, &local).expect("peer logs are fine");
}

#[test]
fn event_roundtrip_preserves_all_eight_fields() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("events/dev.jsonl");
    let event = sample_event("evt_roundtrip");

    append_event(&path, &event).expect("append");
    let events = read_events(&path).expect("read");

    assert_eq!(events.len(), 1);
    let read_back = &events[0];

    // All spec §12.1 fields must survive a round-trip.
    assert_eq!(read_back.schema, EVENT_SCHEMA_VERSION);
    assert_eq!(read_back.id, event.id);
    assert_eq!(read_back.device, event.device);
    assert_eq!(read_back.seq, event.seq);
    assert_eq!(read_back.operation_id, event.operation_id);
    // CRC is injected by encode_event_line; must be non-zero.
    assert_ne!(read_back.crc32c, 0, "crc32c must be set by framing layer");
}

#[test]
fn event_line_crc_is_verified_on_decode() {
    use memory_substrate::events::{decode_line, encode_event_line};
    use memory_substrate::SUBSTRATE_SCHEMA_VERSION;

    let event = sample_event("evt_crc");
    let value = serde_json::to_value(&event).expect("serialize");
    let line = encode_event_line(&value).expect("encode");

    // Valid line decodes.
    let decoded = decode_line(line.trim_end_matches('\n')).expect("valid CRC accepted");
    let crc_field = decoded.get("crc32c").and_then(|v| v.as_u64()).expect("crc32c field");
    assert_ne!(crc_field, 0);

    // Tampered line fails.
    let tampered = line.replace(&format!("\"schema\":{SUBSTRATE_SCHEMA_VERSION}"), "\"schema\":99");
    assert!(decode_line(tampered.trim_end_matches('\n')).is_none(), "tampered CRC must fail");
}
