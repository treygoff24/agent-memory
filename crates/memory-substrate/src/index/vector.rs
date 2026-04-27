//! Vector store reconciliation helpers.
//!
//! Also owns adapter-agnostic vector utilities (e.g. `validate_dimension`) that
//! do not depend on the sqlite-vec adapter.  Spec §10.2.2 #4: dimension
//! validation is a contract of the `EmbeddingTriple`, not the adapter.

use std::collections::HashSet;

use rusqlite::params;
use rusqlite::OptionalExtension as _;

use crate::error::VectorError;
use crate::model::EmbeddingTriple;

/// Validate that `vector.len()` matches `triple.dimension`.
///
/// Adapter-agnostic: the rule applies regardless of whether the backend is
/// sqlite-vec, an external store, or any future adapter.
pub fn validate_dimension(triple: &EmbeddingTriple, vector: &[f32]) -> Result<(), VectorError> {
    if vector.len() == triple.dimension as usize {
        Ok(())
    } else {
        Err(VectorError::DimensionMismatch { expected: triple.dimension, found: vector.len() as u32 })
    }
}

/// Minimal vector store contract.
pub trait VectorStore {
    /// List chunk ids present in the vector table for a triple.
    fn list_chunk_ids(&self, triple: &EmbeddingTriple) -> Result<HashSet<String>, VectorError>;
    /// Delete one vector.
    fn delete_vector(&mut self, triple: &EmbeddingTriple, chunk_id: &str) -> Result<(), VectorError>;
}

/// Delete vectors whose chunks no longer exist.
pub fn reconcile_orphans(
    store: &mut dyn VectorStore,
    triple: &EmbeddingTriple,
    valid_chunk_ids: &HashSet<String>,
) -> Result<usize, VectorError> {
    let existing = store.list_chunk_ids(triple)?;
    let mut deleted = 0usize;
    for chunk_id in existing.difference(valid_chunk_ids) {
        store.delete_vector(triple, chunk_id)?;
        deleted += 1;
    }
    Ok(deleted)
}

/// Insert pending embedding jobs for missing vectors.
///
/// Looks up `body_hash` from `memory_chunks` for each missing chunk to
/// populate the `content_hash` column (spec §10.2.1 #6 stale-job gate).
/// When the chunk row is not found in `memory_chunks` (e.g. a test stub or a
/// race with a concurrent delete), a sentinel empty string is used so the
/// job is still enqueued — the embedding worker will discard it if the chunk
/// is no longer present when it runs.
///
/// Propagates `VectorError::Sqlite` on SQL failure — not the misleading
/// `UnknownEmbeddingTriple` the old code emitted on any SQL error.
pub fn reconcile_missing(
    connection: &rusqlite::Connection,
    store: &dyn VectorStore,
    triple: &EmbeddingTriple,
    valid_chunk_ids: &HashSet<String>,
) -> Result<usize, VectorError> {
    let existing = store.list_chunk_ids(triple)?;
    let missing: Vec<_> = valid_chunk_ids.difference(&existing).cloned().collect();
    let enqueued_at = chrono::Utc::now().to_rfc3339();
    for chunk_id in &missing {
        // Fetch the content hash from the chunks table.  Use an empty sentinel
        // when the row is absent so we still enqueue the job — the worker
        // will drop it if the chunk is gone by the time it runs.
        let content_hash: String = connection
            .query_row("SELECT body_hash FROM memory_chunks WHERE chunk_id=?1", [chunk_id.as_str()], |row| row.get(0))
            .optional()?
            .unwrap_or_default();
        connection.execute(
            "INSERT OR IGNORE INTO pending_embedding_jobs(
                     chunk_id, provider, model_ref, dimension, content_hash, enqueued_at
                 ) VALUES (?1,?2,?3,?4,?5,?6)",
            params![chunk_id, triple.provider, triple.model_ref, triple.dimension, content_hash, enqueued_at],
        )?;
    }
    Ok(missing.len())
}

/// Count pending jobs for a triple.
pub fn reconcile_pending_jobs(connection: &rusqlite::Connection, triple: &EmbeddingTriple) -> rusqlite::Result<usize> {
    connection
        .query_row(
            "SELECT COUNT(*) FROM pending_embedding_jobs WHERE provider=?1 AND model_ref=?2 AND dimension=?3",
            params![triple.provider, triple.model_ref, triple.dimension],
            |row| row.get::<_, i64>(0),
        )
        .map(|count| count as usize)
}
