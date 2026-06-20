//! Embedding-vector write helpers: per-chunk validation, vector/metadata upsert,
//! pending-job resolution, the active-triple reconcile sweep, and the
//! dropped-triple / table-existence probes shared with the query facade.

use rusqlite::{params, Connection};

use crate::error::VectorError;
use crate::model::{EmbeddingTriple, EmbeddingUpdate};

use super::util::EMBEDDING_TRIPLE_PREDICATE;

/// Validate: dimension OK, triple not dropped, content hash matches stored hash.
pub(super) fn validate_update_preconditions(conn: &Connection, update: &EmbeddingUpdate) -> Result<(), VectorError> {
    crate::index::sqlite_vec::validate_dimension(&update.triple, &update.vector)?;
    if is_dropped_triple(conn, &update.triple)? {
        return Err(VectorError::UnknownEmbeddingTriple(update.triple.clone()));
    }
    let actual_hash: rusqlite::Result<String> =
        conn.query_row("SELECT body_hash FROM memory_chunks WHERE chunk_id=?1", [update.chunk_id.as_str()], |row| {
            row.get(0)
        });
    let actual_hash = actual_hash.map_err(|_| VectorError::StaleChunk {
        expected: update.expected_chunk_hash.clone(),
        found: crate::model::Sha256::new("missing"),
    })?;
    if actual_hash != update.expected_chunk_hash.as_str() {
        return Err(VectorError::StaleChunk {
            expected: update.expected_chunk_hash.clone(),
            found: crate::model::Sha256::new(actual_hash),
        });
    }
    Ok(())
}

/// Read the integer rowid for a chunk (needed to address the sqlite-vec table).
pub(super) fn read_chunk_rowid(conn: &Connection, chunk_id: &str) -> Result<i64, VectorError> {
    conn.query_row("SELECT chunk_rowid FROM memory_chunks WHERE chunk_id=?1", [chunk_id], |row| row.get::<_, i64>(0))
        .map_err(Into::into)
}

/// Upsert the vector payload: sqlite-vec virtual table + chunk_vectors shadow.
///
/// Called OUTSIDE any SQLite transaction (spec §10.2.1 step 4).  If the
/// subsequent metadata transaction rolls back, the orphan vector row is cleaned
/// by the startup reconciliation pass.
#[allow(clippy::too_many_arguments)]
pub(super) fn upsert_vector_payload(
    conn: &Connection,
    triple: &EmbeddingTriple,
    chunk_id: &str,
    chunk_rowid: i64,
    vector: &[f32],
) -> Result<(), VectorError> {
    let table = crate::index::sqlite_vec::vector_table_name(triple);
    let blob = crate::index::sqlite_vec::serialize_f32(vector);
    conn.execute(
        &format!("INSERT OR REPLACE INTO {table}(rowid, embedding) VALUES (?1, ?2)"),
        params![chunk_rowid, blob],
    )?;
    // The `chunk_vectors` shadow row exists only so `vector_count` has a table to
    // COUNT; the `embedding` blob written to the vec0 table above is the sole copy
    // KNN ever reads. `vector_json` is never SELECTed anywhere in the workspace,
    // so serializing the vector to a JSON float array (a per-element float→text
    // pass plus a large text payload on every embedding upsert) is dead weight.
    // The column is `NOT NULL`, so write an empty string to keep the row valid.
    conn.execute(
        "INSERT INTO chunk_vectors(chunk_id,provider,model_ref,dimension,vector_json) VALUES (?1,?2,?3,?4,'')
         ON CONFLICT(chunk_id,provider,model_ref,dimension) DO UPDATE SET vector_json=excluded.vector_json",
        params![chunk_id, triple.provider, triple.model_ref, i64::from(triple.dimension)],
    )?;
    Ok(())
}

/// Record that a chunk was embedded: upsert `chunk_embedding_meta`.
pub(super) fn upsert_chunk_embedding_meta(conn: &Connection, update: &EmbeddingUpdate) -> Result<(), VectorError> {
    let vector_table = crate::index::sqlite_vec::vector_table_name(&update.triple);
    let embedded_at = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO chunk_embedding_meta(
             chunk_id, provider, model_ref, dimension, vector_table, embedded_at, content_hash
         ) VALUES (?1,?2,?3,?4,?5,?6,?7)
         ON CONFLICT(chunk_id,provider,model_ref,dimension) DO UPDATE SET
           vector_table  = excluded.vector_table,
           embedded_at   = excluded.embedded_at,
           content_hash  = excluded.content_hash",
        params![
            update.chunk_id.as_str(),
            update.triple.provider,
            update.triple.model_ref,
            i64::from(update.triple.dimension),
            vector_table.as_ref(),
            embedded_at,
            update.expected_chunk_hash.as_str()
        ],
    )?;
    Ok(())
}

/// Delete the pending job that triggered this embedding update.
pub(super) fn resolve_pending_embedding_job(conn: &Connection, update: &EmbeddingUpdate) -> Result<(), VectorError> {
    conn.execute(
        "DELETE FROM pending_embedding_jobs
         WHERE chunk_id=?1 AND provider=?2 AND model_ref=?3 AND dimension=?4",
        params![
            update.chunk_id.as_str(),
            update.triple.provider,
            update.triple.model_ref,
            i64::from(update.triple.dimension)
        ],
    )?;
    Ok(())
}

/// Delete orphan vectors/meta rows and enqueue missing embeddings for the active triple.
///
/// Takes `&mut Connection` — enforces exclusive access, preventing
/// `unchecked_transaction` races.  Spec §10.2.1 step 5.
///
/// Content-hash check: drops pending jobs whose `content_hash` no longer
/// matches `memory_chunks.body_hash` (spec §10.2.1 #6 third bullet).
pub(super) fn reconcile_active_embedding_jobs_impl(
    connection: &mut Connection,
    triple: &EmbeddingTriple,
) -> Result<usize, VectorError> {
    if is_dropped_triple_rusqlite(connection, triple).map_err(VectorError::Sqlite)? {
        return Ok(0);
    }
    let txn = connection.transaction()?;

    // Remove orphan rows whose chunk no longer exists.
    txn.execute("DELETE FROM chunk_vectors WHERE chunk_id NOT IN (SELECT chunk_id FROM memory_chunks)", [])?;
    txn.execute("DELETE FROM chunk_embedding_meta WHERE chunk_id NOT IN (SELECT chunk_id FROM memory_chunks)", [])?;
    // Drop stale pending jobs: chunk gone OR content_hash drifted from current body.
    txn.execute(
        "DELETE FROM pending_embedding_jobs
         WHERE chunk_id NOT IN (SELECT chunk_id FROM memory_chunks)
            OR content_hash != (
                SELECT mc.body_hash FROM memory_chunks mc
                WHERE mc.chunk_id = pending_embedding_jobs.chunk_id
            )",
        [],
    )?;

    // Enqueue jobs for chunks missing a vector for this triple.
    let enqueued_at = chrono::Utc::now().to_rfc3339();
    let queued = txn.execute(
        "INSERT OR IGNORE INTO pending_embedding_jobs(
             chunk_id, provider, model_ref, dimension, content_hash, enqueued_at
         )
         SELECT mc.chunk_id, ?1, ?2, ?3, mc.body_hash, ?4
         FROM memory_chunks mc
         LEFT JOIN chunk_vectors cv
           ON cv.chunk_id  = mc.chunk_id
          AND cv.provider  = ?1
          AND cv.model_ref = ?2
          AND cv.dimension = ?3
         WHERE cv.chunk_id IS NULL",
        params![triple.provider, triple.model_ref, i64::from(triple.dimension), enqueued_at],
    )?;

    txn.commit()?;
    Ok(queued)
}

/// Check if a triple is in the dropped set.  Returns `VectorError`.
pub(super) fn is_dropped_triple(conn: &Connection, triple: &EmbeddingTriple) -> Result<bool, VectorError> {
    is_dropped_triple_rusqlite(conn, triple).map_err(Into::into)
}

/// Same check but returns `rusqlite::Result` for callers already in that error domain.
pub(super) fn is_dropped_triple_rusqlite(conn: &Connection, triple: &EmbeddingTriple) -> rusqlite::Result<bool> {
    conn.query_row(
        &format!(
            "SELECT EXISTS(SELECT 1 FROM dropped_embedding_triples
         WHERE {EMBEDDING_TRIPLE_PREDICATE})"
        ),
        params![triple.provider, triple.model_ref, i64::from(triple.dimension)],
        |row| row.get::<_, i64>(0),
    )
    .map(|v| v != 0)
}

pub(super) fn table_exists(conn: &Connection, table: &str) -> Result<bool, VectorError> {
    conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)", [table], |row| {
        row.get::<_, i64>(0)
    })
    .map(|v| v != 0)
    .map_err(Into::into)
}

pub(super) fn ensure_vector_table(conn: &Connection, triple: &EmbeddingTriple) -> Result<(), VectorError> {
    let table = crate::index::sqlite_vec::vector_table_name(triple);
    conn.execute(
        &format!("CREATE VIRTUAL TABLE IF NOT EXISTS {table} USING vec0(embedding float[{}])", triple.dimension),
        [],
    )
    .map(|_| ())
    .map_err(Into::into)
}
