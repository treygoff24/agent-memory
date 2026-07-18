//! Embedding-vector write helpers: per-chunk validation, vector/metadata upsert,
//! pending-job resolution, the active-triple reconcile sweep, and the
//! dropped-triple / table-existence probes shared with the query facade.

use rusqlite::{params, params_from_iter, types::Value, Connection, OptionalExtension};

use crate::error::VectorError;
use crate::model::{AuxRowKind, EmbeddingLaneEligibility, EmbeddingTriple, EmbeddingUpdate};

use super::sql_placeholders;
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
    let actual_hash = match actual_hash {
        Ok(actual_hash) => actual_hash,
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            return Err(VectorError::StaleChunk {
                expected: update.expected_chunk_hash.clone(),
                found: crate::model::Sha256::new("missing"),
            });
        }
        Err(error) => return Err(error.into()),
    };
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
    eligibility: EmbeddingLaneEligibility,
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
    let allowed_sensitivities = eligibility.allowed_sensitivity_db_strs();
    let mut sql = String::from(
        "INSERT OR IGNORE INTO pending_embedding_jobs(
             chunk_id, provider, model_ref, dimension, content_hash, enqueued_at
         )
         SELECT mc.chunk_id, ?, ?, ?, mc.body_hash, ?
         FROM memory_chunks mc
         JOIN memories m ON m.id = mc.memory_id
         LEFT JOIN chunk_vectors cv
           ON cv.chunk_id  = mc.chunk_id
          AND cv.provider  = ?
          AND cv.model_ref = ?
          AND cv.dimension = ?
         WHERE cv.chunk_id IS NULL",
    );
    if eligibility.requires_plaintext_filter() {
        sql.push_str(" AND m.sensitivity IN (");
        sql.push_str(&sql_placeholders(allowed_sensitivities.len()));
        sql.push(')');
    }
    let mut bindings = vec![
        Value::from(triple.provider.clone()),
        Value::from(triple.model_ref.clone()),
        Value::from(i64::from(triple.dimension)),
        Value::from(enqueued_at),
        Value::from(triple.provider.clone()),
        Value::from(triple.model_ref.clone()),
        Value::from(i64::from(triple.dimension)),
    ];
    bindings.extend(allowed_sensitivities.iter().map(|sensitivity| Value::from((*sensitivity).to_string())));
    let queued = txn.execute(&sql, params_from_iter(bindings))?;

    txn.execute(
        "DELETE FROM aux_pending_embedding_jobs
         WHERE (row_kind='abstraction' AND NOT EXISTS (
                  SELECT 1 FROM memory_abstractions a WHERE a.memory_id=target_id AND a.abstraction_hash=content_hash))
            OR (row_kind='cue' AND NOT EXISTS (
                  SELECT 1 FROM memory_cues c WHERE target_id=c.memory_id||':'||c.ordinal AND c.cue_hash=content_hash))",
        [],
    )?;
    txn.execute(
        "DELETE FROM aux_embedding_meta
         WHERE target_id NOT IN (SELECT memory_id FROM memory_abstractions)
           AND target_id NOT IN (SELECT memory_id||':'||ordinal FROM memory_cues)",
        [],
    )?;
    reconcile_aux_vector_state(&txn)?;
    let aux_enqueued_at = chrono::Utc::now().to_rfc3339();
    let mut aux_sql = String::from(
        "INSERT OR IGNORE INTO aux_pending_embedding_jobs(
           row_kind,target_id,content_hash,provider,model_ref,dimension,enqueued_at)
         SELECT rows.row_kind,rows.target_id,rows.content_hash,?,?,?,?
         FROM (
           SELECT 'abstraction' row_kind,a.memory_id target_id,a.abstraction_hash content_hash,m.sensitivity
             FROM memory_abstractions a JOIN memories m ON m.id=a.memory_id
           UNION ALL
           SELECT 'cue',c.memory_id||':'||c.ordinal,c.cue_hash,m.sensitivity
             FROM memory_cues c JOIN memories m ON m.id=c.memory_id
         ) rows
         LEFT JOIN aux_embedding_meta meta ON meta.row_kind=rows.row_kind AND meta.target_id=rows.target_id
           AND meta.content_hash=rows.content_hash AND meta.provider=? AND meta.model_ref=? AND meta.dimension=?
         WHERE meta.target_id IS NULL",
    );
    if eligibility.requires_plaintext_filter() {
        aux_sql.push_str(" AND rows.sensitivity IN (");
        aux_sql.push_str(&sql_placeholders(allowed_sensitivities.len()));
        aux_sql.push(')');
    }
    let mut aux_bindings = vec![
        Value::from(triple.provider.clone()),
        Value::from(triple.model_ref.clone()),
        Value::from(i64::from(triple.dimension)),
        Value::from(aux_enqueued_at),
        Value::from(triple.provider.clone()),
        Value::from(triple.model_ref.clone()),
        Value::from(i64::from(triple.dimension)),
    ];
    aux_bindings.extend(allowed_sensitivities.into_iter().map(|value| Value::from(value.to_string())));
    let aux_queued = txn.execute(&aux_sql, params_from_iter(aux_bindings))?;

    txn.commit()?;
    Ok(queued + aux_queued)
}

fn reconcile_aux_vector_state(txn: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
    let meta = {
        let mut stmt = txn.prepare("SELECT row_kind,target_id,provider,model_ref,dimension FROM aux_embedding_meta")?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    EmbeddingTriple { provider: row.get(2)?, model_ref: row.get(3)?, dimension: row.get(4)? },
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };
    for (row_kind, target_id, triple) in meta {
        let kind = if row_kind == "abstraction" { AuxRowKind::Abstraction } else { AuxRowKind::Cue };
        let rowid = match kind {
            AuxRowKind::Abstraction => txn
                .query_row("SELECT rowid FROM memory_abstractions WHERE memory_id=?1", [&target_id], |row| {
                    row.get::<_, i64>(0)
                })
                .optional()?,
            AuxRowKind::Cue => match target_id
                .rsplit_once(':')
                .and_then(|(memory_id, ordinal)| ordinal.parse::<i64>().ok().map(|ordinal| (memory_id, ordinal)))
            {
                Some((memory_id, ordinal)) => txn
                    .query_row(
                        "SELECT rowid FROM memory_cues WHERE memory_id=?1 AND ordinal=?2",
                        params![memory_id, ordinal],
                        |row| row.get::<_, i64>(0),
                    )
                    .optional()?,
                None => None,
            },
        };
        let table = crate::index::sqlite_vec::aux_vector_table_name(kind, &triple);
        let exists: i64 = txn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
            [&table],
            |row| row.get(0),
        )?;
        let vector_exists = match (exists != 0, rowid) {
            (true, Some(rowid)) => {
                txn.query_row(&format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE rowid=?1)"), [rowid], |row| {
                    row.get(0)
                })?
            }
            _ => 0_i64,
        };
        if vector_exists == 0 {
            txn.execute(
                "DELETE FROM aux_embedding_meta WHERE row_kind=?1 AND target_id=?2
                   AND provider=?3 AND model_ref=?4 AND dimension=?5",
                params![row_kind, target_id, triple.provider, triple.model_ref, i64::from(triple.dimension)],
            )?;
        }
    }

    let tables = {
        let mut stmt = txn.prepare(
            "SELECT name FROM sqlite_master WHERE type='table'
               AND (name LIKE 'vec_abstractions_%' OR name LIKE 'vec_cues_%')",
        )?;
        let tables = stmt.query_map([], |row| row.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        tables
    };
    for table in tables {
        let source = if table.starts_with("vec_abstractions_") { "memory_abstractions" } else { "memory_cues" };
        txn.execute(&format!("DELETE FROM {table} WHERE rowid NOT IN (SELECT rowid FROM {source})"), [])?;
    }
    Ok(())
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
