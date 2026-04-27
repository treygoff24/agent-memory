//! Migration runner.

use std::path::Path;

use rusqlite::Connection;

use crate::error::OpenError;
use crate::index::schema::SCHEMA_SQL;

/// Highest `schema_migrations.version` this build understands.
///
/// Spec §10.1 makes `schema_migrations` the canonical version row; opening a
/// database whose `MAX(version)` exceeds this constant returns
/// [`OpenError::IndexSchemaVersionUnsupported`] without applying any DDL.
pub const INDEX_SUPPORTED_SCHEMA_VERSION: u32 = 1;

/// Open and migrate an index database, applying spec §10.1 pragmas before any DDL.
pub fn open_index(path: &Path) -> Result<Connection, OpenError> {
    crate::index::sqlite_vec::register_extension();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let connection = Connection::open(path).map_err(sqlite_to_open)?;
    apply_pragmas(&connection).map_err(sqlite_to_open)?;
    connection.execute_batch(SCHEMA_SQL).map_err(sqlite_to_open)?;
    enforce_schema_version(&connection)?;
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

fn enforce_schema_version(connection: &Connection) -> Result<(), OpenError> {
    let found: u32 = connection
        .query_row("SELECT COALESCE(MAX(version), 0) FROM schema_migrations", [], |row| row.get(0))
        .map_err(sqlite_to_open)?;
    if found > INDEX_SUPPORTED_SCHEMA_VERSION {
        return Err(OpenError::IndexSchemaVersionUnsupported { found, supported: INDEX_SUPPORTED_SCHEMA_VERSION });
    }
    Ok(())
}
