//! Vector store reconciliation helpers.
//!
//! Also owns adapter-agnostic vector utilities (e.g. `validate_dimension`) that
//! do not depend on the sqlite-vec adapter.  Spec §10.2.2 #4: dimension
//! validation is a contract of the `EmbeddingTriple`, not the adapter.

use std::collections::HashSet;

use rusqlite::OptionalExtension as _;
use rusqlite::{params, params_from_iter, types::Value};

use crate::error::VectorError;
use crate::model::{EmbeddingLaneEligibility, EmbeddingTriple};

use super::sql_placeholders;

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
/// This low-level helper is intentionally lane-unaware and is not used by the
/// production active-triple reconciliation path. API-lane callers must use
/// `Index::reconcile_active_embedding_jobs` /
/// `reconcile_active_embedding_jobs_impl`, which take
/// [`EmbeddingLaneEligibility`] and apply the sensitivity fence before enqueue.
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

/// Count pending jobs for a triple that are eligible to drain.
///
/// `AllTiers` preserves the historical unfiltered count for the local lane.
/// `PlaintextOnly` joins through `memories` and counts only jobs whose current
/// chunk text may transit an API embedding provider.
pub fn reconcile_pending_jobs(
    connection: &rusqlite::Connection,
    triple: &EmbeddingTriple,
    eligibility: EmbeddingLaneEligibility,
) -> rusqlite::Result<usize> {
    if matches!(eligibility, EmbeddingLaneEligibility::AllTiers) {
        return connection
            .query_row(
                "SELECT COUNT(*) FROM pending_embedding_jobs WHERE provider=?1 AND model_ref=?2 AND dimension=?3",
                params![triple.provider, triple.model_ref, i64::from(triple.dimension)],
                |row| row.get::<_, i64>(0),
            )
            .map(|count| count as usize);
    }
    count_plaintext_filtered_pending_jobs(connection, triple, eligibility, true)
}

/// Count pending jobs held local-only by the lane eligibility fence.
pub fn held_local_embedding_jobs(
    connection: &rusqlite::Connection,
    triple: &EmbeddingTriple,
    eligibility: EmbeddingLaneEligibility,
) -> rusqlite::Result<usize> {
    if matches!(eligibility, EmbeddingLaneEligibility::AllTiers) {
        return Ok(0);
    }
    count_plaintext_filtered_pending_jobs(connection, triple, eligibility, false)
}

fn count_plaintext_filtered_pending_jobs(
    connection: &rusqlite::Connection,
    triple: &EmbeddingTriple,
    eligibility: EmbeddingLaneEligibility,
    eligible: bool,
) -> rusqlite::Result<usize> {
    let allowed_sensitivities = eligibility.allowed_sensitivity_db_strs();
    let predicate = if eligible { "IN" } else { "NOT IN" };
    let sql = format!(
        "SELECT COUNT(*)
         FROM pending_embedding_jobs pj
         JOIN memory_chunks mc ON mc.chunk_id = pj.chunk_id
         JOIN memories m ON m.id = mc.memory_id
         WHERE pj.provider = ? AND pj.model_ref = ? AND pj.dimension = ?
           AND pj.content_hash = mc.body_hash
           AND m.sensitivity {predicate} ({})",
        sql_placeholders(allowed_sensitivities.len())
    );
    let mut bindings = vec![
        Value::from(triple.provider.clone()),
        Value::from(triple.model_ref.clone()),
        Value::from(i64::from(triple.dimension)),
    ];
    bindings.extend(allowed_sensitivities.into_iter().map(|sensitivity| Value::from(sensitivity.to_string())));
    connection.query_row(&sql, params_from_iter(bindings), |row| row.get::<_, i64>(0)).map(|count| count as usize)
}
