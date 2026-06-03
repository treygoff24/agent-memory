//! Derived SQLite index, chunk, and vector helpers.

mod chunking;
mod events_read;
mod migrations;
mod query;
mod schema;
pub mod sqlite_vec;
mod vector;

pub use chunking::{chunk_memory, Chunk};
pub use events_read::MirrorEvent;
pub use migrations::{open_index, INDEX_SUPPORTED_SCHEMA_VERSION};
pub use query::Index;
pub use vector::{reconcile_missing, reconcile_orphans, reconcile_pending_jobs, VectorStore};
