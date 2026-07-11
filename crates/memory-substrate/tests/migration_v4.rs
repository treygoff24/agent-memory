use chrono::{TimeZone, Utc};
use memory_substrate::events::{append_event, Event, EventKind, EVENT_SCHEMA_VERSION};
use memory_substrate::index::{open_index, INDEX_SUPPORTED_SCHEMA_VERSION};
use memory_substrate::{DeviceId, EventId, InitOptions, MemoryId, OperationId, Roots, Substrate};
use rusqlite::Connection;

#[test]
fn index_supported_schema_version_is_6() {
    assert_eq!(INDEX_SUPPORTED_SCHEMA_VERSION, 6);
}

#[test]
fn fresh_schema_includes_v4_tables_and_original_confidence() {
    let temp = tempfile::tempdir().expect("tempdir");
    let conn = open_index(&temp.path().join("index.sqlite")).expect("open index");

    assert!(table_exists(&conn, "events_log"));
    assert!(table_exists(&conn, "memory_supersession"));
    assert_eq!(
        table_columns(&conn, "events_log"),
        vec!["event_id", "device", "seq", "kind", "memory_id", "ts", "payload_json"]
    );
    assert_eq!(column_type(&conn, "memories", "original_confidence").as_deref(), Some("REAL"));

    let max_version: u32 = conn
        .query_row("SELECT COALESCE(MAX(version), 0) FROM schema_migrations", [], |row| row.get(0))
        .expect("read schema migration version");
    assert_eq!(max_version, 6);

    let user_version: u32 = conn.pragma_query_value(None, "user_version", |row| row.get(0)).expect("user_version");
    assert_eq!(user_version, 0, "schema_migrations, not PRAGMA user_version, is canonical");
}

#[test]
fn opening_v3_database_migrates_to_v4_idempotently() {
    let temp = tempfile::tempdir().expect("tempdir");
    let db_path = temp.path().join("index.sqlite");
    {
        let conn = open_index(&db_path).expect("first open");
        // Simulate a pre-v4 (v3) database: drop every migration row at or above 4
        // so the reopen actually re-runs migrate_v4 (and migrate_v5) rather than
        // seeing MAX(version) already at the current head and skipping the DDL.
        conn.execute("DELETE FROM schema_migrations WHERE version >= 4", []).expect("simulate v3 schema");
        conn.execute("DROP TABLE events_log", []).expect("drop v4 table");
        conn.execute("DROP TABLE memory_supersession", []).expect("drop v4 table");
    }

    let conn = open_index(&db_path).expect("migrate v3 forward");
    assert!(table_exists(&conn, "events_log"));
    assert!(table_exists(&conn, "memory_supersession"));
    assert_eq!(column_type(&conn, "memories", "original_confidence").as_deref(), Some("REAL"));
    let max_version: u32 = conn
        .query_row("SELECT COALESCE(MAX(version), 0) FROM schema_migrations", [], |row| row.get(0))
        .expect("read schema migration version");
    assert_eq!(max_version, 6);
    drop(conn);

    let reopened = open_index(&db_path).expect("second open idempotent");
    let version_rows: u32 = reopened
        .query_row("SELECT COUNT(*) FROM schema_migrations WHERE version = 4", [], |row| row.get(0))
        .expect("count v4 rows");
    assert_eq!(version_rows, 1);
}

#[tokio::test]
async fn open_rebuilds_v4_events_log_from_existing_jsonl() {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    {
        let substrate = Substrate::init(
            roots.clone(),
            InitOptions { force_unsafe_durability: true, device_id: Some("dev_testdevice01".to_string()) },
        )
        .await
        .expect("init substrate");
        append_event(
            &roots.repo.join("events/dev_peerdevice01.jsonl"),
            &recall_hit_event("evt_backfill_peer", "dev_peerdevice01", 1),
        )
        .expect("append peer event");
        let conn = Connection::open(roots.runtime.join("index.sqlite")).expect("open sqlite");
        conn.execute("DELETE FROM events_log", []).expect("clear mirror");
        drop(conn);
        drop(substrate);
    }

    let reopened = Substrate::open(roots.clone()).await.expect("reopen substrate");
    let health = reopened.events_log_mirror_health().expect("mirror health after open backfill");
    assert_eq!(health.missing_count, 0);
    assert_eq!(health.jsonl_count, health.sqlite_count);

    let conn = Connection::open(roots.runtime.join("index.sqlite")).expect("open sqlite");
    let mirrored: String = conn
        .query_row("SELECT device FROM events_log WHERE event_id = 'evt_backfill_peer'", [], |row| row.get(0))
        .expect("backfilled event row");
    assert_eq!(mirrored, "dev_peerdevice01");
}

fn table_exists(conn: &Connection, table: &str) -> bool {
    conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)", [table], |row| {
        row.get::<_, i64>(0)
    })
    .map(|exists| exists != 0)
    .expect("table exists query")
}

fn table_columns(conn: &Connection, table: &str) -> Vec<String> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})")).expect("prepare table_info");
    stmt.query_map([], |row| row.get::<_, String>(1))
        .expect("query table_info")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect columns")
}

fn column_type(conn: &Connection, table: &str, column: &str) -> Option<String> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})")).expect("prepare table_info");
    let columns = stmt
        .query_map([], |row| Ok((row.get::<_, String>(1)?, row.get::<_, String>(2)?)))
        .expect("query table_info")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect columns");
    columns.into_iter().find_map(|(name, ty)| (name == column).then_some(ty))
}

fn recall_hit_event(event_id: &str, device: &str, seq: u64) -> Event {
    Event {
        schema: EVENT_SCHEMA_VERSION,
        id: EventId::new(event_id),
        at: Utc.with_ymd_and_hms(2026, 5, 1, 12, 30, 0).single().expect("fixture time"),
        device: DeviceId::new(device),
        seq,
        operation_id: Some(OperationId::new(format!("op_{event_id}"))),
        kind: EventKind::RecallHit {
            id: MemoryId::new("mem_20260501_a1b2c3d4e5f60718_000016"),
            recalled_at: Utc.with_ymd_and_hms(2026, 5, 1, 12, 30, 0).single().expect("fixture time"),
        },
        crc32c: 0,
    }
}
