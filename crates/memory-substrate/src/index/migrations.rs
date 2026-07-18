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
pub const INDEX_SUPPORTED_SCHEMA_VERSION: u32 = 6;

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
    // Under WAL, a writer that races the startup reconciler, merge driver, or a
    // second connection would otherwise hit SQLITE_BUSY and fail immediately.
    // Wait up to 5s for the lock to clear before erroring.
    connection.pragma_update(None, "busy_timeout", 5000)?;
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
    if found < 5 {
        migrate_v5(connection).map_err(sqlite_to_open)?;
    }
    if found < 6 {
        migrate_v6(connection).map_err(sqlite_to_open)?;
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

/// v5: `chunk_id` derivation changed to the spec §10.3 form
/// `chk_<sha256(memory_id || chunker_version || ordinal || chunk_hash)>`.
///
/// The shipped chunker had derived `chunk_id` from the chunk text alone, so two
/// memories sharing an identical chunk (or one memory repeating a chunk)
/// collided on the `memory_chunks.chunk_id` UNIQUE constraint and crashed startup
/// reconciliation with `OperatorRepairRequired`. Folding the memory id and
/// ordinal into the digest makes `chunk_id` globally unique by construction, but
/// every existing `chunk_id` must be recomputed.
///
/// Reconciliation rechunks a memory only when its stored `file_hash` no longer
/// matches the on-disk file (`runtime::reconcile` file-consistency check), so a
/// schema-only change would never rebuild the chunks. Invalidate every memory's
/// `file_hash` (plaintext and encrypted tiers alike) so the next `Substrate::open`
/// re-walks and rechunks every memory. The per-memory reindex deletes the old
/// chunk rows (cascading their FTS shadow and `chunk_embedding_meta`) and inserts
/// rows with the new ids; the chunk-id orphan sweeps reclaim stale
/// `chunk_vectors`/`pending_embedding_jobs`. This migration only triggers that
/// rebuild — it writes no chunk rows itself.
fn migrate_v5(connection: &mut Connection) -> rusqlite::Result<()> {
    let tx = connection.transaction()?;
    // Sentinel is not a `sha256:`-prefixed hash, so it can never match a real
    // file hash — every memory is treated as drifted and rechunked on open.
    tx.execute("UPDATE memories SET file_hash = 'force-reindex-v5'", [])?;
    tx.execute("INSERT OR IGNORE INTO schema_migrations(version) VALUES (?1)", params![5_i64])?;
    tx.commit()
}

fn migrate_v6(connection: &mut Connection) -> rusqlite::Result<()> {
    let tx = connection.transaction()?;
    tx.execute_batch(
        r#"
CREATE TABLE IF NOT EXISTS memory_abstractions (
  memory_id TEXT PRIMARY KEY REFERENCES memories(id) ON DELETE CASCADE,
  abstraction TEXT NOT NULL,
  abstraction_hash TEXT NOT NULL,
  source_body_hash TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS memory_cues (
  memory_id TEXT NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
  ordinal INTEGER NOT NULL,
  cue_text TEXT NOT NULL,
  cue_hash TEXT NOT NULL,
  PRIMARY KEY (memory_id, ordinal)
);
CREATE TABLE IF NOT EXISTS aux_embedding_meta (
  row_kind TEXT NOT NULL CHECK (row_kind IN ('abstraction','cue')),
  target_id TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  provider TEXT NOT NULL,
  model_ref TEXT NOT NULL,
  dimension INTEGER NOT NULL,
  embedded_at TEXT NOT NULL,
  PRIMARY KEY (row_kind, target_id, provider, model_ref, dimension)
);
CREATE TABLE IF NOT EXISTS aux_pending_embedding_jobs (
  row_kind TEXT NOT NULL CHECK (row_kind IN ('abstraction','cue')),
  target_id TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  provider TEXT NOT NULL,
  model_ref TEXT NOT NULL,
  dimension INTEGER NOT NULL,
  enqueued_at TEXT NOT NULL,
  attempts INTEGER NOT NULL DEFAULT 0,
  last_error TEXT,
  PRIMARY KEY (row_kind, target_id, provider, model_ref, dimension)
);
CREATE INDEX IF NOT EXISTS idx_aux_pending_jobs_enqueued ON aux_pending_embedding_jobs(enqueued_at);
"#,
    )?;
    tx.execute("INSERT OR IGNORE INTO schema_migrations(version) VALUES (?1)", params![6_i64])?;
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

#[cfg(test)]
mod migrate_v6_tests {
    use super::*;
    use crate::index::{chunk_memory, Index};
    use crate::{EmbeddingTriple, RepoPath};

    fn fixture_memory() -> Result<crate::Memory, Box<dyn std::error::Error>> {
        let markdown = r#"---
schema_version: 1
id: mem_20260424_a1b2c3d4e5f60718_000100
type: pattern
scope: agent
summary: representative migration memory
confidence: 1.0
trust_level: trusted
sensitivity: internal
status: active
created_at: 2026-04-24T12:00:00Z
updated_at: 2026-04-24T12:00:00Z
author:
  kind: system
  component: test
tags:
  - migration
aliases:
  - migration-alias
---
body text used for the migration fixture
"#;
        crate::frontmatter::parse_document(
            markdown,
            Some(RepoPath::new("agent/patterns/mem_20260424_a1b2c3d4e5f60718_000100.md")),
        )
        .map(|parsed| parsed.memory)
        .map_err(Into::into)
    }

    /// Exercise `migrate_v6` directly so the migration needs no public export,
    /// using a schema-5 database with representative data. The migration must be
    /// idempotent, the data must survive, and the rollback file copy must stay
    /// readable as raw v5 before a normal open re-migrates it.
    #[test]
    fn migrate_v6_is_idempotent_and_preserves_representative_data_and_rollback_is_readable(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let live = temp.path().join("index.sqlite");
        let backup = temp.path().join("index-v5.backup.sqlite");

        let triple = EmbeddingTriple { provider: "synthetic".into(), model_ref: "aux".into(), dimension: 3 };
        let memory = fixture_memory()?;
        let expected_body_hash = crate::markdown::hash_bytes(memory.body.as_bytes()).to_string();
        let expected_chunk_count = chunk_memory(&memory).len();
        let expected_id = memory.frontmatter.id.as_str().to_string();
        let expected_summary = memory.frontmatter.summary.clone();

        let mut index = Index::with_active_embedding(open_index(&live)?, triple);
        index.upsert_memory(&memory, false)?;
        drop(index);

        // Downgrade to a genuine schema-5 shape: drop the v6 tables and the v6
        // version row on a raw connection (no open_index — nothing re-runs
        // SCHEMA_SQL between here and the migrate_v6 call under test).
        let mut conn = rusqlite::Connection::open(&live)?;
        conn.execute_batch(
            "DROP TABLE IF EXISTS memory_abstractions;
             DROP TABLE IF EXISTS memory_cues;
             DROP TABLE IF EXISTS aux_embedding_meta;
             DROP TABLE IF EXISTS aux_pending_embedding_jobs;
             DELETE FROM schema_migrations WHERE version = 6;",
        )?;
        let v5_version: i64 =
            conn.query_row("SELECT COALESCE(MAX(version), 0) FROM schema_migrations", [], |row| row.get(0))?;
        assert_eq!(v5_version, 5);

        // WAL checkpoint before the file copy: without it the main DB file may
        // predate the downgrade (the writes sit in -wal) and the "backup" is a
        // lie. The SAME hazard applies to the operator runbook's pre-migration
        // copy on the live corpus — checkpoint (or stop the daemon) first.
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        std::fs::copy(&live, &backup)?;

        // migrate_v6 is the only creator of the v6 tables in this path — twice,
        // to pin idempotency.
        migrate_v6(&mut conn)?;
        migrate_v6(&mut conn)?;

        let version: i64 =
            conn.query_row("SELECT COALESCE(MAX(version), 0) FROM schema_migrations", [], |row| row.get(0))?;
        assert_eq!(version, 6);
        assert_eq!(INDEX_SUPPORTED_SCHEMA_VERSION, 6);
        for table in ["memory_abstractions", "memory_cues", "aux_embedding_meta", "aux_pending_embedding_jobs"] {
            let exists: i64 = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
                [table],
                |row| row.get(0),
            )?;
            assert_eq!(exists, 1, "{table}");
        }

        // Data integrity through the migration.
        let summary: String =
            conn.query_row("SELECT summary FROM memories WHERE id=?1", [expected_id.as_str()], |row| row.get(0))?;
        assert_eq!(summary, expected_summary);
        let body_hash: String =
            conn.query_row("SELECT body_hash FROM memories WHERE id=?1", [expected_id.as_str()], |row| row.get(0))?;
        assert_eq!(body_hash, expected_body_hash);
        let chunk_count: i64 =
            conn.query_row("SELECT COUNT(*) FROM memory_chunks WHERE memory_id=?1", [expected_id.as_str()], |row| {
                row.get(0)
            })?;
        assert_eq!(chunk_count, expected_chunk_count as i64);
        let pending_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM pending_embedding_jobs WHERE chunk_id IN (SELECT chunk_id FROM memory_chunks WHERE memory_id=?1)",
            [expected_id.as_str()],
            |row| row.get(0),
        )?;
        assert_eq!(pending_count, expected_chunk_count as i64);
        let tag_count: i64 =
            conn.query_row("SELECT COUNT(*) FROM memory_tags WHERE memory_id=?1", [expected_id.as_str()], |row| {
                row.get(0)
            })?;
        assert_eq!(tag_count, 1);
        let alias_count: i64 =
            conn.query_row("SELECT COUNT(*) FROM memory_aliases WHERE memory_id=?1", [expected_id.as_str()], |row| {
                row.get(0)
            })?;
        assert_eq!(alias_count, 1);
        drop(conn);

        // Rollback: the pre-migration copy must be readable as RAW v5 first
        // (version 5, no v6 tables, data present) — that is what "rollback =
        // restore the copy" means for an operator on an old binary.
        std::fs::copy(&backup, &live)?;
        let raw = rusqlite::Connection::open(&live)?;
        let raw_version: i64 =
            raw.query_row("SELECT COALESCE(MAX(version), 0) FROM schema_migrations", [], |row| row.get(0))?;
        assert_eq!(raw_version, 5, "restored backup is schema 5");
        let v6_present: i64 = raw.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='memory_abstractions')",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(v6_present, 0, "restored backup has no v6 tables");
        let raw_summary: String =
            raw.query_row("SELECT summary FROM memories WHERE id=?1", [expected_id.as_str()], |row| row.get(0))?;
        assert_eq!(raw_summary, expected_summary);
        drop(raw);

        // A normal open then migrates the restored copy back to v6.
        let reopened = open_index(&live)?;
        let reopened_version: i64 =
            reopened.query_row("SELECT COALESCE(MAX(version), 0) FROM schema_migrations", [], |row| row.get(0))?;
        assert_eq!(reopened_version, 6);
        let reopened_summary: String =
            reopened.query_row("SELECT summary FROM memories WHERE id=?1", [expected_id.as_str()], |row| row.get(0))?;
        assert_eq!(reopened_summary, expected_summary);
        Ok(())
    }
}
