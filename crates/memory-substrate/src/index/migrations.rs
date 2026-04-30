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
pub const INDEX_SUPPORTED_SCHEMA_VERSION: u32 = 3;

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

fn add_column_if_missing(tx: &Transaction<'_>, column: &str, definition: &str) -> rusqlite::Result<()> {
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
