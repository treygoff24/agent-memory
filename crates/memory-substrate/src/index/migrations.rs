//! Migration runner.

use std::path::Path;

use rusqlite::{params, Connection, Transaction};

use crate::error::OpenError;
use crate::index::schema::SCHEMA_SQL;

/// Highest `schema_migrations.version` this build understands.
///
/// Spec §10.1 makes `schema_migrations` the canonical version row; opening a
/// database whose `MAX(version)` exceeds this constant returns
/// [`OpenError::IndexSchemaVersionUnsupported`] without applying any DDL.
pub const INDEX_SUPPORTED_SCHEMA_VERSION: u32 = 4;

/// Open and migrate an index database, applying spec §10.1 pragmas before any DDL.
pub fn open_index(path: &Path) -> Result<Connection, OpenError> {
    crate::index::sqlite_vec::register_extension();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut connection = Connection::open(path).map_err(sqlite_to_open)?;
    apply_pragmas(&connection).map_err(sqlite_to_open)?;
    connection.execute_batch(SCHEMA_SQL).map_err(sqlite_to_open)?;
    migrate_schema(&mut connection)?;
    ensure_events_log_identity_schema(&mut connection).map_err(sqlite_to_open)?;
    Ok(connection)
}

fn sqlite_to_open(err: rusqlite::Error) -> OpenError {
    OpenError::InvalidRoots(format!("sqlite open failed: {err}"))
}

/// Pragmas that must run **outside any transaction**.
///
/// `journal_mode = WAL` is a connection-scoped setting that the engine refuses
/// to change inside an active transaction, so we apply it on the bare
/// connection before any DDL/data work begins.
fn apply_pragmas(connection: &Connection) -> rusqlite::Result<()> {
    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.pragma_update(None, "synchronous", "NORMAL")?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    Ok(())
}

fn migrate_schema(connection: &mut Connection) -> Result<(), OpenError> {
    let found: u32 = connection
        .query_row("SELECT COALESCE(MAX(version), 0) FROM schema_migrations", [], |row| row.get(0))
        .map_err(sqlite_to_open)?;
    if found > INDEX_SUPPORTED_SCHEMA_VERSION {
        return Err(OpenError::IndexSchemaVersionUnsupported { found, supported: INDEX_SUPPORTED_SCHEMA_VERSION });
    }
    if found < 2 {
        migrate_v2(connection).map_err(sqlite_to_open)?;
    }
    if found < 3 {
        migrate_v3(connection).map_err(sqlite_to_open)?;
    }
    if found < 4 {
        migrate_v4(connection).map_err(sqlite_to_open)?;
    }
    Ok(())
}

fn migrate_v2(connection: &mut Connection) -> rusqlite::Result<()> {
    let tx = connection.transaction()?;
    add_column_if_missing(&tx, "passive_recall", "INTEGER NOT NULL DEFAULT 1")?;
    add_column_if_missing(&tx, "index_body", "INTEGER NOT NULL DEFAULT 1")?;
    tx.execute_batch(
        r#"
UPDATE memories
SET passive_recall =
  CASE
    WHEN json_extract(frontmatter_json, '$.retrieval_policy.passive_recall') = 0 THEN 0
    ELSE 1
  END;

UPDATE memories
SET index_body =
  CASE
    WHEN json_extract(frontmatter_json, '$.retrieval_policy.index_body') = 0 THEN 0
    ELSE 1
  END;

CREATE INDEX IF NOT EXISTS idx_memories_status_passive_updated
  ON memories(status, passive_recall, updated_at);
CREATE INDEX IF NOT EXISTS idx_memories_scope_canon_status_passive_updated
  ON memories(scope, canonical_namespace_id, status, passive_recall, updated_at DESC);
"#,
    )?;
    tx.execute("INSERT OR IGNORE INTO schema_migrations(version) VALUES (?1)", params![2_i64])?;
    tx.commit()
}

fn migrate_v3(connection: &mut Connection) -> rusqlite::Result<()> {
    let tx = connection.transaction()?;
    add_column_if_missing(&tx, "human_review_required", "INTEGER NOT NULL DEFAULT 0")?;
    add_column_if_missing(&tx, "max_scope", "TEXT NOT NULL DEFAULT 'agent'")?;
    tx.execute_batch(
        r#"
UPDATE memories
SET human_review_required =
  CASE
    WHEN json_extract(frontmatter_json, '$.write_policy.human_review_required') = 1 THEN 1
    ELSE 0
  END;

UPDATE memories
SET max_scope =
  CASE json_extract(frontmatter_json, '$.retrieval_policy.max_scope')
    WHEN 'user' THEN 'user'
    WHEN 'project' THEN 'project'
    WHEN 'org' THEN 'org'
    WHEN 'agent' THEN 'agent'
    WHEN 'subagent' THEN 'subagent'
    ELSE 'agent'
  END;
"#,
    )?;
    tx.execute("INSERT OR IGNORE INTO schema_migrations(version) VALUES (?1)", params![3_i64])?;
    tx.commit()
}

fn migrate_v4(connection: &mut Connection) -> rusqlite::Result<()> {
    let tx = connection.transaction()?;
    add_column_if_missing(&tx, "original_confidence", "REAL")?;
    tx.execute_batch(
        r#"
CREATE TABLE IF NOT EXISTS events_log(
  event_id      TEXT PRIMARY KEY,
  device        TEXT NOT NULL,
  seq           INTEGER NOT NULL,
  kind          TEXT NOT NULL,
  memory_id     TEXT,
  ts            TEXT NOT NULL,
  payload_json  TEXT NOT NULL CHECK (json_valid(payload_json))
);
CREATE INDEX IF NOT EXISTS idx_events_log_kind_memory_ts
  ON events_log(kind, memory_id, ts);

CREATE TABLE IF NOT EXISTS memory_supersession(
  memory_id     TEXT NOT NULL,
  supersedes_id TEXT NOT NULL,
  PRIMARY KEY(memory_id, supersedes_id),
  FOREIGN KEY(memory_id) REFERENCES memories(id) ON DELETE CASCADE,
  FOREIGN KEY(supersedes_id) REFERENCES memories(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_memory_supersession_supersedes_id
  ON memory_supersession(supersedes_id);

UPDATE memories
SET original_confidence = json_extract(frontmatter_json, '$.original_confidence')
WHERE json_type(frontmatter_json, '$.original_confidence') IN ('integer', 'real');

INSERT OR IGNORE INTO memory_supersession(memory_id, supersedes_id)
SELECT memories.id, superseded.value
FROM memories, json_each(memories.frontmatter_json, '$.supersedes') AS superseded
WHERE superseded.value IS NOT NULL
  AND EXISTS (SELECT 1 FROM memories AS target WHERE target.id = superseded.value);
"#,
    )?;
    tx.execute("INSERT OR IGNORE INTO schema_migrations(version) VALUES (?1)", params![4_i64])?;
    tx.commit()
}

fn ensure_events_log_identity_schema(connection: &mut Connection) -> rusqlite::Result<()> {
    if !events_log_needs_identity_migration(connection)? {
        return Ok(());
    }

    tracing::warn!(
        table = "events_log",
        old_primary_key = "seq",
        new_primary_key = "event_id",
        source_of_truth = "JSONL events log",
        "recreating legacy events_log mirror with event_id primary key; SQLite mirror rows are derived and will be replayed from JSONL"
    );

    let tx = connection.transaction()?;
    tx.execute_batch(
        r#"
ALTER TABLE events_log RENAME TO events_log_legacy_seq_key;

CREATE TABLE events_log(
  event_id      TEXT PRIMARY KEY,
  device        TEXT NOT NULL,
  seq           INTEGER NOT NULL,
  kind          TEXT NOT NULL,
  memory_id     TEXT,
  ts            TEXT NOT NULL,
  payload_json  TEXT NOT NULL CHECK (json_valid(payload_json))
);

DROP TABLE events_log_legacy_seq_key;

CREATE INDEX IF NOT EXISTS idx_events_log_kind_memory_ts
  ON events_log(kind, memory_id, ts);
"#,
    )?;
    tx.commit()
}

fn events_log_needs_identity_migration(connection: &Connection) -> rusqlite::Result<bool> {
    let mut stmt = connection.prepare("PRAGMA table_info(events_log)")?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?;
        if name == "event_id" {
            return Ok(false);
        }
    }
    Ok(true)
}

fn add_column_if_missing(tx: &Transaction<'_>, column: &'static str, definition: &'static str) -> rusqlite::Result<()> {
    if memory_column_exists(tx, column)? {
        return Ok(());
    }
    tx.execute(&format!("ALTER TABLE memories ADD COLUMN {column} {definition}"), [])?;
    Ok(())
}

fn memory_column_exists(tx: &Transaction<'_>, column: &str) -> rusqlite::Result<bool> {
    let mut stmt = tx.prepare("PRAGMA table_info(memories)")?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?;
        if name == column {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use std::fmt;
    use std::sync::{Arc, Mutex};

    use rusqlite::Connection;
    use tracing::field::{Field, Visit};
    use tracing::span::{Attributes, Record};
    use tracing::{Event, Id, Metadata, Subscriber};

    use super::ensure_events_log_identity_schema;

    #[test]
    fn events_log_identity_migration_warns_before_recreating_legacy_mirror() -> Result<(), Box<dyn std::error::Error>> {
        let warnings = Arc::new(Mutex::new(Vec::new()));
        let subscriber = RecordingSubscriber { warnings: Arc::clone(&warnings) };
        let mut connection = Connection::open_in_memory()?;
        connection.execute_batch(
            r#"
CREATE TABLE events_log(
  seq           INTEGER PRIMARY KEY,
  kind          TEXT NOT NULL,
  memory_id     TEXT,
  ts            TEXT NOT NULL,
  payload_json  TEXT NOT NULL
);
"#,
        )?;

        tracing::subscriber::with_default(subscriber, || ensure_events_log_identity_schema(&mut connection))?;

        let warnings = warnings.lock().map_err(|_| "warnings lock poisoned")?;
        assert!(
            warnings.iter().any(|warning| {
                warning.contains("recreating legacy events_log mirror")
                    && warning.contains("old_primary_key=\"seq\"")
                    && warning.contains("new_primary_key=\"event_id\"")
            }),
            "expected structured warning, got {warnings:?}"
        );
        let event_id_pk: i64 = connection.query_row(
            "SELECT pk FROM pragma_table_info('events_log') WHERE name = 'event_id'",
            [],
            |row| row.get(0),
        )?;
        let seq_pk: i64 =
            connection
                .query_row("SELECT pk FROM pragma_table_info('events_log') WHERE name = 'seq'", [], |row| row.get(0))?;
        assert_eq!(event_id_pk, 1);
        assert_eq!(seq_pk, 0);
        Ok(())
    }

    struct RecordingSubscriber {
        warnings: Arc<Mutex<Vec<String>>>,
    }

    impl Subscriber for RecordingSubscriber {
        fn enabled(&self, metadata: &Metadata<'_>) -> bool {
            *metadata.level() == tracing::Level::WARN
        }

        fn new_span(&self, _span: &Attributes<'_>) -> Id {
            Id::from_u64(1)
        }

        fn record(&self, _span: &Id, _values: &Record<'_>) {}

        fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

        fn event(&self, event: &Event<'_>) {
            let mut visitor = EventVisitor::default();
            event.record(&mut visitor);
            if let Ok(mut warnings) = self.warnings.lock() {
                warnings.push(visitor.fields.join(" "));
            }
        }

        fn enter(&self, _span: &Id) {}

        fn exit(&self, _span: &Id) {}
    }

    #[derive(Default)]
    struct EventVisitor {
        fields: Vec<String>,
    }

    impl Visit for EventVisitor {
        fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
            self.fields.push(format!("{}={value:?}", field.name()));
        }
    }
}
