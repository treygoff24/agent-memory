use std::io::Write;

use chrono::Utc;
use memory_substrate::events::{
    append_event, read_events, read_events_strict, recover_event_log, Event, EventKind, EVENT_SCHEMA_VERSION,
};
use memory_substrate::{ClassificationOutcome, DeviceId, EventId, MemoryId, OperationId, RepoPath};

fn sample_event(id: &str, op: &str) -> Event {
    Event {
        schema: EVENT_SCHEMA_VERSION,
        id: EventId::new(id),
        at: Utc::now(),
        device: DeviceId::new("dev_testdevice01"),
        seq: 1,
        operation_id: Some(OperationId::new(op)),
        kind: EventKind::WriteCommitted {
            id: MemoryId::new("mem_20260424_a1b2c3d4e5f60718_000001"),
            path: RepoPath::new("agent/patterns/mem_20260424_a1b2c3d4e5f60718_000001.md"),
            classification: ClassificationOutcome::Trusted,
        },
        crc32c: 0,
    }
}

#[test]
fn event_log_recovery_truncates_one_malformed_trailing_line() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("events/dev.jsonl");
    let event = sample_event("evt_1", "op_1");
    append_event(&path, &event).expect("append event");
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .expect("open log")
        .write_all(b"{malformed trailing line")
        .expect("write malformed line");

    let malformed = recover_event_log(&path).expect("recover log");
    let events = read_events(&path).expect("read events");

    assert_eq!(malformed, 1);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].id, event.id);
}

#[test]
fn event_log_recovery_refuses_nonfinal_malformed_line() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("events/dev.jsonl");
    let first = sample_event("evt_1", "op_1");
    let second = sample_event("evt_2", "op_2");
    append_event(&path, &first).expect("append first");
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .expect("open log")
        .write_all(b"{malformed nonfinal line\n")
        .expect("write malformed line");
    append_event(&path, &second).expect("append second");

    let err = recover_event_log(&path).expect_err("nonfinal malformed line requires repair");

    assert!(err.to_string().contains("non-final malformed event log line"));
}

#[test]
fn read_events_refuses_malformed_line_after_recovery_boundary() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("events/dev.jsonl");
    std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
    std::fs::write(&path, "not crc framed\n").expect("write malformed");

    let err = read_events_strict(&path).expect_err("malformed event log line rejected");

    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
}

#[test]
fn read_events_falls_back_to_recovery_on_single_trailing_malformed_line() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("events/dev.jsonl");
    let event = sample_event("evt_1", "op_1");
    append_event(&path, &event).expect("append");
    std::fs::OpenOptions::new().append(true).open(&path).expect("open").write_all(b"{garbage}").expect("write garbage");

    // read_events should recover and return the valid event.
    let events = read_events(&path).expect("forgiving read");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].id, event.id);
}

/// Property test: random byte garbage appended to a valid log is recovered
/// and the valid prefix round-trips byte-stably.
#[test]
fn event_log_recovery_byte_stable_after_garbage_suffix() {
    use std::io::Write;
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("events/prop.jsonl");

    // Write several valid events.
    let events: Vec<Event> = (0..5).map(|i| sample_event(&format!("evt_{i}"), &format!("op_{i}"))).collect();
    for ev in &events {
        append_event(&path, ev).expect("append");
    }
    let valid_bytes = std::fs::read(&path).expect("read valid bytes");

    // Append garbage without a newline (partial write simulation).
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .expect("open")
        .write_all(b"\xff\xfe{broken")
        .expect("write garbage");

    recover_event_log(&path).expect("recover");

    let recovered = std::fs::read(&path).expect("read after recovery");
    assert_eq!(recovered, valid_bytes, "recovery must restore byte-identical valid prefix");
}
