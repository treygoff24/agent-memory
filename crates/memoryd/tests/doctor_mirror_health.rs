use chrono::Utc;
use memory_substrate::events::{append_event, Event, EventKind, EVENT_SCHEMA_VERSION};
use memory_substrate::{DeviceId, EventId, InitOptions, MemoryId, OperationId, Roots, Substrate};
use memoryd::handlers::handle_request;
use memoryd::protocol::{RequestEnvelope, RequestPayload, ResponsePayload, ResponseResult};
use rusqlite::Connection;
use tempfile::TempDir;

#[tokio::test]
async fn test_doctor_emits_no_mirror_finding_when_mirror_in_sync() {
    let fixture = Fixture::new().await;
    append_mirrored_recall_hits(&fixture.substrate, 3);

    let doctor = request_doctor(&fixture.substrate).await;

    assert!(
        !doctor.findings.iter().any(|finding| finding.code == "events_log_mirror_lag"),
        "doctor should not report mirror lag when JSONL and SQLite mirror agree: {doctor:?}"
    );
}

#[tokio::test]
async fn test_doctor_emits_finding_when_mirror_lag_positive() {
    let fixture = Fixture::new().await;
    append_mirrored_recall_hits(&fixture.substrate, 3);
    append_jsonl_only_recall_hit(&fixture, 4);

    let doctor = request_doctor(&fixture.substrate).await;

    let finding = doctor
        .findings
        .iter()
        .find(|finding| finding.code == "events_log_mirror_lag")
        .expect("mirror lag finding should be present");
    assert_eq!(finding.repair.as_deref(), Some("memoryd doctor --reindex"));
    assert!(finding.message.contains("1 event"), "message should include lag count: {}", finding.message);
}

#[tokio::test]
async fn test_doctor_emits_finding_when_mirror_missing_middle_row_with_equal_max_seq() {
    let fixture = Fixture::new().await;
    append_mirrored_recall_hits(&fixture.substrate, 3);
    delete_mirrored_event_seq(&fixture, 2);

    let health = fixture.substrate.events_log_mirror_health().expect("mirror health");
    assert_eq!(health.jsonl_max_seq, health.sqlite_max_seq, "fixture should isolate equal-max mirror drift");
    assert_eq!(health.lag, 0, "max-seq lag alone cannot reveal a missing middle row");
    assert_eq!(health.missing_count, 1, "fixture should leave exactly one mirrored row missing");

    let doctor = request_doctor(&fixture.substrate).await;

    let finding = doctor
        .findings
        .iter()
        .find(|finding| finding.code == "events_log_mirror_lag")
        .expect("mirror missing-row finding should be present");
    assert_eq!(finding.repair.as_deref(), Some("memoryd doctor --reindex"));
    assert!(finding.message.contains("1 event"), "message should include missing count: {}", finding.message);
}

#[tokio::test]
async fn test_doctor_finding_lag_message_includes_lag_count() {
    let one_lag = mirror_lag_message_after_jsonl_only_events(1).await;
    assert!(one_lag.contains("1 event"), "singular lag message should include count: {one_lag}");

    let three_lag = mirror_lag_message_after_jsonl_only_events(3).await;
    assert!(three_lag.contains("3 events"), "plural lag message should include count: {three_lag}");
}

async fn mirror_lag_message_after_jsonl_only_events(count: u64) -> String {
    let fixture = Fixture::new().await;
    append_mirrored_recall_hits(&fixture.substrate, 1);
    for seq in 2..(2 + count) {
        append_jsonl_only_recall_hit(&fixture, seq);
    }
    let doctor = request_doctor(&fixture.substrate).await;
    doctor
        .findings
        .into_iter()
        .find(|finding| finding.code == "events_log_mirror_lag")
        .expect("mirror lag finding should be present")
        .message
}

async fn request_doctor(substrate: &Substrate) -> memoryd::protocol::DoctorResponse {
    let response = handle_request(substrate, RequestEnvelope::new("doctor", RequestPayload::Doctor)).await;
    match response.result {
        ResponseResult::Success(ResponsePayload::Doctor(doctor)) => doctor,
        other => panic!("expected doctor success, got {other:?}"),
    }
}

fn append_mirrored_recall_hits(substrate: &Substrate, count: u64) {
    for index in 1..=count {
        substrate
            .record_event_best_effort(EventKind::RecallHit { id: memory_id(index), recalled_at: Utc::now() })
            .expect("recall event records");
    }
}

fn append_jsonl_only_recall_hit(fixture: &Fixture, seq: u64) {
    let event = Event {
        schema: EVENT_SCHEMA_VERSION,
        id: EventId::new(format!("evt_jsonl_only_{seq}")),
        at: Utc::now(),
        device: DeviceId::new("dev_doctormirror"),
        seq,
        operation_id: Some(OperationId::new(format!("op_jsonl_only_{seq}"))),
        kind: EventKind::RecallHit { id: memory_id(seq), recalled_at: Utc::now() },
        crc32c: 0,
    };
    append_event(&fixture.event_log, &event).expect("jsonl-only event appends");
}

fn delete_mirrored_event_seq(fixture: &Fixture, seq: u64) {
    let conn = Connection::open(&fixture.index).expect("open sqlite mirror");
    let deleted = conn
        .execute("DELETE FROM events_log WHERE device = 'dev_doctormirror' AND seq = ?1", [seq as i64])
        .expect("delete mirrored event row");
    assert_eq!(deleted, 1, "fixture should delete exactly one mirrored event row");
}

fn memory_id(index: u64) -> MemoryId {
    MemoryId::new(format!("mem_20260501_{index:016x}_000001"))
}

struct Fixture {
    _temp: TempDir,
    substrate: Substrate,
    event_log: std::path::PathBuf,
    index: std::path::PathBuf,
}

impl Fixture {
    async fn new() -> Self {
        let temp = TempDir::new().expect("tempdir");
        let repo = temp.path().join("repo");
        let runtime = temp.path().join("runtime");
        let index = runtime.join("index.sqlite");
        let substrate = Substrate::init(
            Roots::new(repo.clone(), runtime),
            InitOptions { force_unsafe_durability: true, device_id: Some("dev_doctormirror".to_string()) },
        )
        .await
        .expect("substrate init");
        let event_log = repo.join("events").join("dev_doctormirror.jsonl");
        Self { _temp: temp, substrate, event_log, index }
    }
}
