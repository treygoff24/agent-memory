use chrono::{TimeZone, Utc};
use memory_substrate::events::{append_event, read_events, Event, EventKind, EVENT_SCHEMA_VERSION};
use memory_substrate::{
    Author, AuthorKind, ClassificationOutcome, DeviceId, EventContext, EventId, Frontmatter, InitOptions, Memory,
    MemoryId, MemoryStatus, MemoryType, OperationId, RepoPath, RetrievalPolicy, Roots, Scope, Sensitivity, Source,
    SourceKind, Substrate, TrustLevel, WriteMode, WritePolicy, WriteRequest,
};
use rusqlite::Connection;

#[tokio::test]
async fn write_memory_dual_writes_jsonl_and_sqlite_events_log() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_testdevice01".to_string()) },
    )
    .await
    .expect("init substrate");

    let memory = sample_memory("mem_20260501_a1b2c3d4e5f60718_000011");
    substrate
        .write_memory(WriteRequest {
            operation_id: None,
            memory: memory.clone(),
            expected_base_hash: None,
            write_mode: WriteMode::CreateNew,
            index_projection: None,
            event_context: EventContext::default(),
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Trusted,
        })
        .await
        .expect("write memory");

    let events = read_events(&roots.repo.join("events/dev_testdevice01.jsonl")).expect("read jsonl events");
    let write_event = events
        .iter()
        .find(|event| matches!(event.kind, EventKind::WriteCommitted { .. }))
        .expect("write committed event");

    let conn = Connection::open(roots.runtime.join("index.sqlite")).expect("open sqlite mirror");
    let (kind, memory_id, payload_json): (String, String, String) = conn
        .query_row(
            "SELECT kind, memory_id, payload_json FROM events_log WHERE event_id = ?1",
            [write_event.id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("mirrored event row");

    assert_eq!(kind, "write_committed");
    assert_eq!(memory_id, memory.frontmatter.id.as_str());
    assert!(payload_json.contains("write_committed"));
}

#[tokio::test]
async fn doctor_reindex_rebuilds_events_log_from_jsonl_and_health_reports_sync() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_testdevice01".to_string()) },
    )
    .await
    .expect("init substrate");

    let event = Event {
        schema: EVENT_SCHEMA_VERSION,
        id: EventId::new("evt_recall_hit"),
        at: Utc.with_ymd_and_hms(2026, 5, 1, 12, 30, 0).single().expect("fixture time"),
        device: DeviceId::new("dev_peerdevice01"),
        seq: 41,
        operation_id: Some(OperationId::new("op_recall_hit")),
        kind: EventKind::RecallHit {
            id: MemoryId::new("mem_20260501_a1b2c3d4e5f60718_000012"),
            recalled_at: Utc.with_ymd_and_hms(2026, 5, 1, 12, 30, 0).single().expect("fixture time"),
        },
        crc32c: 0,
    };
    append_event(&roots.repo.join("events/dev_peerdevice01.jsonl"), &event).expect("append peer event");

    let conn = Connection::open(roots.runtime.join("index.sqlite")).expect("open sqlite mirror");
    conn.execute("DELETE FROM events_log", []).expect("clear mirror");
    drop(conn);

    let stale = substrate.events_log_mirror_health().expect("mirror health before reindex");
    assert_eq!(stale.lag, 41);
    assert_eq!(stale.missing_count, stale.jsonl_count);

    substrate.doctor_reindex_events_log().expect("reindex events log mirror");
    let healthy = substrate.events_log_mirror_health().expect("mirror health after reindex");
    assert_eq!(healthy.jsonl_max_seq, 41);
    assert_eq!(healthy.sqlite_max_seq, 41);
    assert_eq!(healthy.lag, 0);
    assert_eq!(healthy.missing_count, 0);
    assert_eq!(healthy.jsonl_count, healthy.sqlite_count);
}

#[test]
fn fresh_schema_has_events_log_table_and_covering_index() {
    let temp = tempfile::tempdir().expect("tempdir");
    let conn = memory_substrate::index::open_index(&temp.path().join("index.sqlite")).expect("open index");

    let columns = table_columns(&conn, "events_log");
    assert_eq!(columns, vec!["event_id", "device", "seq", "kind", "memory_id", "ts", "payload_json"]);

    let indexed_columns = index_columns(&conn, "idx_events_log_kind_memory_ts");
    assert_eq!(indexed_columns, vec!["kind", "memory_id", "ts"]);
}

#[tokio::test]
async fn doctor_reindex_preserves_multi_device_events_with_same_sequence() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_testdevice01".to_string()) },
    )
    .await
    .expect("init substrate");

    let first = recall_hit_event("evt_same_seq_a", "dev_peerdevice01", 1, "mem_20260501_a1b2c3d4e5f60718_000012");
    let second = recall_hit_event("evt_same_seq_b", "dev_peerdevice02", 1, "mem_20260501_a1b2c3d4e5f60718_000013");
    append_event(&roots.repo.join("events/dev_peerdevice01.jsonl"), &first).expect("append first peer event");
    append_event(&roots.repo.join("events/dev_peerdevice02.jsonl"), &second).expect("append second peer event");

    substrate.doctor_reindex_events_log().expect("reindex events log mirror");

    let conn = Connection::open(roots.runtime.join("index.sqlite")).expect("open sqlite mirror");
    let rows = event_log_rows(&conn);
    assert!(rows.contains(&("evt_same_seq_a".to_string(), "dev_peerdevice01".to_string(), 1)));
    assert!(rows.contains(&("evt_same_seq_b".to_string(), "dev_peerdevice02".to_string(), 1)));
    assert_eq!(recall_hit_count(&conn), 2, "same per-device seq values must not replace each other in the mirror");

    let health = substrate.events_log_mirror_health().expect("mirror health after multi-device rebuild");
    assert_eq!(health.missing_count, 0);
    assert_eq!(health.jsonl_count, health.sqlite_count);
}

#[tokio::test]
async fn events_log_mirror_health_detects_missing_middle_row() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_testdevice01".to_string()) },
    )
    .await
    .expect("init substrate");

    for (seq, event_id) in [(1, "evt_seq_1"), (2, "evt_seq_2"), (3, "evt_seq_3")] {
        append_event(
            &roots.repo.join("events/dev_peerdevice01.jsonl"),
            &recall_hit_event(event_id, "dev_peerdevice01", seq, "mem_20260501_a1b2c3d4e5f60718_000014"),
        )
        .expect("append peer event");
    }
    substrate.doctor_reindex_events_log().expect("reindex events log mirror");

    let conn = Connection::open(roots.runtime.join("index.sqlite")).expect("open sqlite mirror");
    conn.execute("DELETE FROM events_log WHERE event_id = 'evt_seq_2'", []).expect("delete middle row");
    drop(conn);

    let health = substrate.events_log_mirror_health().expect("mirror health");
    assert_eq!(health.jsonl_max_seq, 3);
    assert_eq!(health.sqlite_max_seq, 3);
    assert_eq!(health.lag, 0, "max-seq lag alone cannot reveal this drift");
    assert_eq!(health.missing_count, 1, "health must detect holes even when max seq matches");
    assert_eq!(health.sqlite_count + 1, health.jsonl_count);
}

#[tokio::test]
async fn recall_hit_drift_query_uses_kind_memory_ts_index() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate = Substrate::init(
        roots.clone(),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_testdevice01".to_string()) },
    )
    .await
    .expect("init substrate");
    let memory_id = "mem_20260501_a1b2c3d4e5f60718_000015";
    append_event(
        &roots.repo.join("events/dev_peerdevice01.jsonl"),
        &recall_hit_event("evt_plan", "dev_peerdevice01", 1, memory_id),
    )
    .expect("append peer event");
    substrate.doctor_reindex_events_log().expect("reindex events log mirror");

    let conn = Connection::open(roots.runtime.join("index.sqlite")).expect("open sqlite mirror");
    let plan = explain_query_plan(
        &conn,
        "EXPLAIN QUERY PLAN
         SELECT COUNT(*) FROM events_log
         WHERE kind = 'recall_hit' AND memory_id = ?1 AND ts > ?2",
        [memory_id, "2026-05-01T00:00:00Z"],
    );
    assert!(
        plan.contains("idx_events_log_kind_memory_ts"),
        "drift-score query must use idx_events_log_kind_memory_ts; plan was {plan}"
    );
}

fn recall_hit_event(event_id: &str, device: &str, seq: u64, memory_id: &str) -> Event {
    Event {
        schema: EVENT_SCHEMA_VERSION,
        id: EventId::new(event_id),
        at: Utc.with_ymd_and_hms(2026, 5, 1, 12, 30, seq as u32).single().expect("fixture time"),
        device: DeviceId::new(device),
        seq,
        operation_id: Some(OperationId::new(format!("op_{event_id}"))),
        kind: EventKind::RecallHit {
            id: MemoryId::new(memory_id),
            recalled_at: Utc.with_ymd_and_hms(2026, 5, 1, 12, 30, seq as u32).single().expect("fixture time"),
        },
        crc32c: 0,
    }
}

fn event_log_rows(conn: &Connection) -> Vec<(String, String, i64)> {
    let mut stmt =
        conn.prepare("SELECT event_id, device, seq FROM events_log ORDER BY event_id").expect("prepare rows");
    stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .expect("query rows")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect rows")
}

fn recall_hit_count(conn: &Connection) -> i64 {
    conn.query_row("SELECT COUNT(*) FROM events_log WHERE kind = 'recall_hit'", [], |row| row.get(0))
        .expect("count recall hits")
}

fn explain_query_plan(conn: &Connection, sql: &str, params: [&str; 2]) -> String {
    let mut stmt = conn.prepare(sql).expect("prepare explain");
    stmt.query_map(params, |row| row.get::<_, String>(3))
        .expect("query explain")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect explain")
        .join("\n")
}

fn table_columns(conn: &Connection, table: &str) -> Vec<String> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})")).expect("prepare table_info");
    stmt.query_map([], |row| row.get::<_, String>(1))
        .expect("query table_info")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect columns")
}

fn index_columns(conn: &Connection, index: &str) -> Vec<String> {
    let mut stmt = conn.prepare(&format!("PRAGMA index_info({index})")).expect("prepare index_info");
    stmt.query_map([], |row| row.get::<_, String>(2))
        .expect("query index_info")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect columns")
}

fn sample_memory(id: &str) -> Memory {
    let now = Utc::now();
    Memory {
        frontmatter: Frontmatter {
            schema_version: 1,
            id: MemoryId::new(id),
            memory_type: MemoryType::Pattern,
            scope: Scope::Agent,
            summary: "mirror fixture".to_string(),
            confidence: 0.9,
            original_confidence: None,
            trust_level: TrustLevel::Trusted,
            sensitivity: Sensitivity::Internal,
            status: MemoryStatus::Active,
            created_at: now,
            updated_at: now,
            author: Author {
                kind: AuthorKind::System,
                user_handle: None,
                harness: None,
                harness_version: None,
                session_id: None,
                subagent_id: None,
                phase: None,
                component: Some("test".to_string()),
            },
            namespace: None,
            canonical_namespace_id: None,
            tags: Vec::new(),
            entities: Vec::new(),
            aliases: Vec::new(),
            source: Source {
                kind: SourceKind::Import,
                reference: None,
                harness: None,
                harness_version: None,
                session_id: None,
                subagent_id: None,
                device: None,
            },
            evidence: Vec::new(),
            requires_user_confirmation: false,
            review_state: None,
            supersedes: Vec::new(),
            superseded_by: Vec::new(),
            related: Vec::new(),
            tombstone_events: Vec::new(),
            retrieval_policy: RetrievalPolicy {
                passive_recall: true,
                max_scope: Scope::Agent,
                mask_personal_for_synthesis: false,
                index_body: true,
                index_embeddings: false,
            },
            write_policy: WritePolicy {
                human_review_required: false,
                policy_applied: "default-v1".to_string(),
                expected_base_hash: None,
            },
            merge_diagnostics: None,
            extras: Default::default(),
        },
        body: "body".to_string(),
        path: Some(RepoPath::new(format!("agent/patterns/{id}.md"))),
    }
}
