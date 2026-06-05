//! Derived SQLite index, chunk, and vector helpers.

mod chunking;
mod events_read;
mod migrations;
mod query;
mod schema;
pub mod sqlite_vec;
mod vector;

pub use chunking::{chunk_memory, Chunk};
pub use events_read::{EventsLogPage, MirrorEvent};
pub use migrations::{open_index, INDEX_SUPPORTED_SCHEMA_VERSION};
pub use query::Index;
pub use vector::{reconcile_missing, reconcile_orphans, reconcile_pending_jobs, VectorStore};

/// Render `count` comma-separated `?` SQL bind placeholders (e.g. `?,?,?`).
///
/// Shared by the index read modules that build `IN (...)` clauses. Callers are
/// responsible for never passing `count == 0`: an empty `IN ()` is invalid SQL,
/// and an empty filter set means "match nothing", which each caller handles by
/// short-circuiting before reaching here.
fn sql_placeholders(count: usize) -> String {
    std::iter::repeat_n("?", count).collect::<Vec<_>>().join(",")
}
