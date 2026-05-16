use chrono::{TimeZone, Utc};
use memory_substrate::events::{append_event, Event, EventKind, EVENT_SCHEMA_VERSION};
use memory_substrate::index::{open_index, INDEX_SUPPORTED_SCHEMA_VERSION};
use memory_substrate::{DeviceId, EventId, InitOptions, MemoryId, OperationId, Roots, Substrate};
use rusqlite::{params, Connection};

#[test]
fn index_supported_schema_version_is_4() {
    assert_eq!(INDEX_SUPPORTED_SCHEMA_VERSION, 4);
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
    assert_eq!(max_version, 4);

    let user_version: u32 = conn.pragma_query_value(None, "user_version", |row| row.get(0)).expect("user_version");
    assert_eq!(user_version, 0, "schema_migrations, not PRAGMA user_version, is canonical");
}

#[test]
fn opening_v3_database_migrates_to_v4_idempotently() {
    let temp = tempfile::tempdir().expect("tempdir");
    let db_path = temp.path().join("index.sqlite");
    create_v3_index_fixture(&db_path);
    {
        let v3 = Connection::open(&db_path).expect("inspect v3 fixture");
        assert!(!table_exists(&v3, "events_log"));
        assert!(!table_exists(&v3, "memory_supersession"));
        assert_eq!(column_type(&v3, "memories", "original_confidence"), None);
        let max_version: u32 = v3
            .query_row("SELECT COALESCE(MAX(version), 0) FROM schema_migrations", [], |row| row.get(0))
            .expect("read v3 migration version");
        assert_eq!(max_version, 3);
    }

    let conn = open_index(&db_path).expect("migrate v3 to v4");
    assert!(table_exists(&conn, "events_log"));
    assert!(table_exists(&conn, "memory_supersession"));
    assert_eq!(column_type(&conn, "memories", "original_confidence").as_deref(), Some("REAL"));
    let original_confidence: f64 = conn
        .query_row(
            "SELECT original_confidence FROM memories WHERE id = ?1",
            ["mem_20260429_a1b2c3d4e5f60718_800001"],
            |row| row.get(0),
        )
        .expect("original_confidence backfilled from v3 frontmatter");
    assert_eq!(original_confidence, 0.73);
    let max_version: u32 = conn
        .query_row("SELECT COALESCE(MAX(version), 0) FROM schema_migrations", [], |row| row.get(0))
        .expect("read schema migration version");
    assert_eq!(max_version, 4);
    drop(conn);

    let reopened = open_index(&db_path).expect("second open idempotent");
    let version_rows: u32 = reopened
        .query_row("SELECT COUNT(*) FROM schema_migrations WHERE version = 4", [], |row| row.get(0))
        .expect("count v4 rows");
    assert_eq!(version_rows, 1);
    let original_confidence_after_reopen: f64 = reopened
        .query_row(
            "SELECT original_confidence FROM memories WHERE id = ?1",
            ["mem_20260429_a1b2c3d4e5f60718_800001"],
            |row| row.get(0),
        )
        .expect("original_confidence remains backfilled after idempotent reopen");
    assert_eq!(original_confidence_after_reopen, 0.73);
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

fn create_v3_index_fixture(path: &std::path::Path) {
    let conn = Connection::open(path).expect("open v3 fixture db");
    conn.execute_batch(
        r#"
CREATE TABLE schema_migrations(
  version INTEGER PRIMARY KEY,
  applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE memories(
  id                          TEXT PRIMARY KEY,
  path                        TEXT NOT NULL UNIQUE,
  schema_version              INTEGER NOT NULL,
  type                        TEXT NOT NULL,
  scope                       TEXT NOT NULL,
  namespace                   TEXT,
  canonical_namespace_id      TEXT,
  summary                     TEXT NOT NULL,
  confidence                  REAL NOT NULL,
  trust_level                 TEXT NOT NULL,
  sensitivity                 TEXT NOT NULL,
  status                      TEXT NOT NULL,
  review_state                TEXT,
  requires_user_confirmation  INTEGER NOT NULL,
  created_at                  TEXT NOT NULL,
  updated_at                  TEXT NOT NULL,
  observed_at                 TEXT,
  valid_from                  TEXT,
  valid_until                 TEXT,
  ttl                         TEXT,
  author                      TEXT NOT NULL,
  source_kind                 TEXT NOT NULL,
  source_harness              TEXT,
  source_device               TEXT,
  body_hash                   TEXT NOT NULL,
  frontmatter_json            TEXT NOT NULL CHECK (json_valid(frontmatter_json)),
  file_hash                   TEXT NOT NULL,
  file_mtime_ns               INTEGER NOT NULL,
  indexed_at                  TEXT NOT NULL,
  metadata_only               INTEGER NOT NULL DEFAULT 0,
  passive_recall              INTEGER NOT NULL DEFAULT 1,
  index_body                  INTEGER NOT NULL DEFAULT 1,
  human_review_required       INTEGER NOT NULL DEFAULT 0,
  max_scope                   TEXT NOT NULL DEFAULT 'agent'
);
"#,
    )
    .expect("create v3 schema");
    conn.execute("INSERT INTO schema_migrations(version) VALUES (1), (2), (3)", []).expect("seed v3 migrations");
    let frontmatter = serde_json::json!({
        "id": "mem_20260429_a1b2c3d4e5f60718_800001",
        "summary": "v3 memory with original confidence in frontmatter",
        "confidence": 0.91,
        "original_confidence": 0.73,
        "supersedes": []
    });
    conn.execute(
        r#"
INSERT INTO memories(
  id, path, schema_version, type, scope, namespace, canonical_namespace_id, summary, confidence,
  trust_level, sensitivity, status, review_state, requires_user_confirmation,
  created_at, updated_at, observed_at, valid_from, valid_until, ttl, author, source_kind,
  source_harness, source_device, body_hash, frontmatter_json, file_hash, file_mtime_ns,
  indexed_at, metadata_only, passive_recall, index_body, human_review_required, max_scope
) VALUES (
  ?1, ?2, 1, 'pattern', 'agent', NULL, NULL, ?3, 0.91,
  'trusted', 'internal', 'active', NULL, 0,
  '2026-04-29T12:00:00Z', '2026-04-29T12:00:00Z', NULL, NULL, NULL, NULL,
  '{"kind":"system"}', 'import', NULL, NULL, 'body-hash', ?4, 'file-hash', 1,
  '2026-04-29T12:00:00Z', 0, 1, 1, 0, 'agent'
)
"#,
        params![
            "mem_20260429_a1b2c3d4e5f60718_800001",
            "agent/patterns/mem_20260429_a1b2c3d4e5f60718_800001.md",
            "v3 memory with original confidence in frontmatter",
            frontmatter.to_string(),
        ],
    )
    .expect("seed v3 memory");
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
