//! Index upsert and query helpers.
//!
//! Layout (stepdown / newspaper): orchestrator-level methods first, SQL helpers
//! below.  Column lists, value bindings, and index names are kept in the same
//! vertical region as the statement that uses them so readers don't scroll.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use rusqlite::{named_params, params, params_from_iter, Connection, Transaction};

use crate::error::{SubstrateError, SubstrateResult, VectorError};
use crate::events::{Event, EventKind};
use crate::index::chunking::chunk_memory;
use crate::markdown::hash_bytes;
use crate::model::{
    ChunkResult, EmbeddingTriple, EmbeddingUpdate, Entity, EventsLogMirrorHealth, Memory, MemoryId, MemoryQuery,
    MemoryStatus, QueryResult, RecallIndexQuery, RecallIndexRow, RepoPath, Scope, Sensitivity, SourceKind,
};

/// Index handle.  Owns a single SQLite connection; all mutating methods take
/// `&mut self` so the borrow checker prevents concurrent transactions.
pub struct Index {
    connection: Connection,
    active_embedding: EmbeddingTriple,
}

impl Index {
    /// Construct an index handle with an explicit active embedding triple.
    ///
    /// Spec §10.2.2 #5: the triple is identity, not flavor.  No silent
    /// fallback — callers must supply the triple loaded from `config.yaml`.
    pub fn with_active_embedding(connection: Connection, active_embedding: EmbeddingTriple) -> Self {
        Self { connection, active_embedding }
    }

    /// Test/fixture constructor using the synthetic embedding triple.
    ///
    /// Production code uses [`Self::with_active_embedding`]; callers that need
    /// the configured triple must load it from `config::load_active_embedding`.
    /// The synthetic triple is inert (no real embedding worker targets it).
    ///
    /// Exposed without `#[cfg(test)]` so integration tests (which compile as
    /// separate crates) can construct an `Index` without a `config.yaml`.
    /// Do not use in production write paths.
    pub fn new(connection: Connection) -> Self {
        Self::with_active_embedding(
            connection,
            EmbeddingTriple {
                provider: "synthetic".to_string(),
                model_ref: "stream-a-test".to_string(),
                dimension: 32,
            },
        )
    }

    /// Borrow the underlying connection (read-only callers).
    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    /// Mirror one canonical JSONL event into the derived SQLite projection.
    pub fn mirror_event(&mut self, event: &Event) -> rusqlite::Result<()> {
        mirror_event_row(&self.connection, event)
    }

    /// Rebuild the derived SQLite events-log projection from canonical JSONL events.
    pub fn rebuild_events_log_mirror(&mut self, events: &[Event]) -> rusqlite::Result<()> {
        let txn = self.connection.transaction()?;
        txn.execute("DELETE FROM events_log", [])?;
        for event in events {
            mirror_event_row(&txn, event)?;
        }
        txn.commit()
    }

    /// Return mirror lag and row-identity drift against canonical JSONL events.
    pub fn events_log_mirror_health(&self, canonical_events: &[Event]) -> rusqlite::Result<EventsLogMirrorHealth> {
        query_events_log_mirror_health(&self.connection, canonical_events)
    }

    /// Upsert a memory, populating all `memories` table columns (spec §10.1).
    pub fn upsert_memory(&mut self, memory: &Memory, metadata_only: bool) -> rusqlite::Result<()> {
        upsert_memory_row_with_full_metadata(&mut self.connection, memory, metadata_only, &self.active_embedding)
    }

    /// Clear all derived rows before a full reindex.
    pub fn clear_memory_index(&mut self) -> rusqlite::Result<()> {
        let txn = self.connection.transaction()?;
        txn.execute("DELETE FROM memory_chunks", [])?;
        txn.execute("DELETE FROM memories", [])?;
        txn.execute("DELETE FROM chunk_vectors", [])?;
        txn.execute("DELETE FROM chunk_embedding_meta", [])?;
        txn.commit()
    }

    /// Clear plaintext-derived rows before reindexing Markdown files, preserving encrypted metadata rows.
    pub fn clear_plaintext_memory_index(&mut self) -> rusqlite::Result<()> {
        let txn = self.connection.transaction()?;
        txn.execute(
            "DELETE FROM memory_chunks
             WHERE memory_id IN (SELECT id FROM memories WHERE path NOT LIKE 'encrypted/%')",
            [],
        )?;
        txn.execute("DELETE FROM memories WHERE path NOT LIKE 'encrypted/%'", [])?;
        txn.execute("DELETE FROM chunk_vectors WHERE chunk_id NOT IN (SELECT chunk_id FROM memory_chunks)", [])?;
        txn.execute("DELETE FROM chunk_embedding_meta WHERE chunk_id NOT IN (SELECT chunk_id FROM memory_chunks)", [])?;
        txn.execute(
            "DELETE FROM pending_embedding_jobs WHERE chunk_id NOT IN (SELECT chunk_id FROM memory_chunks)",
            [],
        )?;
        txn.commit()
    }

    /// Update a chunk embedding.
    ///
    /// Spec §10.2.1 step 4 ordering: vector upsert FIRST (outside any txn),
    /// then a single SQLite transaction for `chunk_embedding_meta` +
    /// `pending_embedding_jobs`.  Taking `&mut self` prevents concurrent
    /// transactions on the same connection.
    pub fn update_embedding(&mut self, update: &EmbeddingUpdate) -> Result<(), VectorError> {
        // Validate before touching anything.
        validate_update_preconditions(&self.connection, update)?;
        let chunk_rowid = read_chunk_rowid(&self.connection, update.chunk_id.as_str())?;

        // Step 1: vector upsert — outside any SQLite transaction.
        ensure_vector_table(&self.connection, &update.triple)?;
        upsert_vector_payload(&self.connection, &update.triple, update.chunk_id.as_str(), chunk_rowid, &update.vector)?;

        // Step 2: one SQLite transaction for metadata + job resolution.
        let txn = self.connection.transaction()?;
        upsert_chunk_embedding_meta(&txn, update)?;
        resolve_pending_embedding_job(&txn, update)?;
        txn.commit()?;
        Ok(())
    }

    /// Drop an embedding triple's vector and metadata rows.
    pub fn drop_embedding_model(&mut self, triple: &EmbeddingTriple) -> Result<usize, VectorError> {
        Ok(self.drop_embedding_model_report(triple)?.vectors_removed as usize)
    }

    /// Drop an embedding triple and return the removal report.
    pub fn drop_embedding_model_report(
        &mut self,
        triple: &EmbeddingTriple,
    ) -> Result<crate::model::DropTripleReport, VectorError> {
        let vectors_removed = self.connection.execute(
            "DELETE FROM chunk_vectors WHERE provider=?1 AND model_ref=?2 AND dimension=?3",
            params![triple.provider, triple.model_ref, i64::from(triple.dimension)],
        )? as u64;
        let meta_rows_removed = self.connection.execute(
            "DELETE FROM chunk_embedding_meta WHERE provider=?1 AND model_ref=?2 AND dimension=?3",
            params![triple.provider, triple.model_ref, i64::from(triple.dimension)],
        )? as u64;
        let pending_jobs_dropped = self.connection.execute(
            "DELETE FROM pending_embedding_jobs WHERE provider=?1 AND model_ref=?2 AND dimension=?3",
            params![triple.provider, triple.model_ref, i64::from(triple.dimension)],
        )? as u64;
        let table = crate::index::sqlite_vec::vector_table_name(triple);
        let table_dropped = table_exists(&self.connection, &table)?;
        self.connection.execute(
            "INSERT OR IGNORE INTO dropped_embedding_triples(provider,model_ref,dimension) VALUES (?1,?2,?3)",
            params![triple.provider, triple.model_ref, i64::from(triple.dimension)],
        )?;
        self.connection.execute(&format!("DROP TABLE IF EXISTS {table}"), [])?;
        Ok(crate::model::DropTripleReport { vectors_removed, meta_rows_removed, pending_jobs_dropped, table_dropped })
    }

    /// Count vectors stored for a triple.
    pub fn vector_count(&self, triple: &EmbeddingTriple) -> Result<usize, VectorError> {
        self.connection
            .query_row(
                "SELECT COUNT(*) FROM chunk_vectors WHERE provider=?1 AND model_ref=?2 AND dimension=?3",
                params![triple.provider, triple.model_ref, i64::from(triple.dimension)],
                |row| row.get::<_, i64>(0),
            )
            .map(|count| count as usize)
            .map_err(Into::into)
    }

    /// Reconcile chunk/vector metadata and enqueue missing embeddings for the active triple.
    pub fn reconcile_active_embedding_jobs(&mut self) -> Result<usize, VectorError> {
        let triple = self.active_embedding.clone();
        reconcile_active_embedding_jobs_impl(&mut self.connection, &triple)
    }

    /// Query chunks through FTS.
    ///
    /// Free-form user text is sanitized into a sequence of FTS5 phrase tokens
    /// before reaching MATCH so that a query like `end-to-end` does not get
    /// reinterpreted as `end NOT to NOT end` and surface a SQLite error.
    /// See `sanitize_fts_query` (private) for the exact transformation.
    ///
    /// R-IX-1 defense-in-depth: the join against `memories` filters out
    /// encrypted-memory chunks (`metadata_only = 1`) and rows disabled for
    /// passive recall even if upstream forgot.
    pub fn query_chunks(&self, text: &str) -> rusqlite::Result<Vec<ChunkResult>> {
        let sanitized = sanitize_fts_query(text);
        if sanitized.is_empty() {
            return Ok(Vec::new());
        }
        let mut stmt = self.connection.prepare_cached(
            "SELECT memory_chunks.memory_id, memory_chunks.text, bm25(memory_chunks_fts) AS score
             FROM memory_chunks_fts
             JOIN memory_chunks ON memory_chunks_fts.rowid = memory_chunks.chunk_rowid
             JOIN memories      ON memories.id = memory_chunks.memory_id
             WHERE memory_chunks_fts MATCH ?1
               AND memories.metadata_only = 0
               AND memories.passive_recall = 1
             ORDER BY score
             LIMIT 20",
        )?;
        // Materialize before stmt drops (E0597 — stmt lifetime).
        let rows = stmt
            .query_map([sanitized.as_str()], |row| {
                Ok(ChunkResult {
                    memory_id: MemoryId::new(row.get::<_, String>(0)?),
                    text: row.get(1)?,
                    score: row.get(2)?,
                })
            })?
            .collect();
        rows
    }

    /// Query chunks through sqlite-vec nearest-neighbor search.
    ///
    /// R-IX-1 defense-in-depth: filters out encrypted-memory chunks.
    pub fn query_vector_chunks(
        &self,
        triple: &EmbeddingTriple,
        vector: &[f32],
        limit: usize,
    ) -> Result<Vec<ChunkResult>, VectorError> {
        crate::index::sqlite_vec::validate_dimension(triple, vector)?;
        let table = crate::index::sqlite_vec::vector_table_name(triple);
        if is_dropped_triple(&self.connection, triple)? || !table_exists(&self.connection, &table)? {
            return Err(VectorError::UnknownEmbeddingTriple(triple.clone()));
        }
        let sql = format!(
            "SELECT memory_chunks.memory_id, memory_chunks.text, {table}.distance
             FROM {table}
             JOIN memory_chunks ON memory_chunks.chunk_rowid = {table}.rowid
             JOIN memories      ON memories.id = memory_chunks.memory_id
             WHERE embedding MATCH ?1
               AND k = ?2
               AND memories.metadata_only = 0
             ORDER BY {table}.distance"
        );
        let blob = crate::index::sqlite_vec::serialize_f32(vector);
        let mut stmt = self.connection.prepare_cached(&sql)?;
        // Materialize before stmt drops (E0597 — stmt lifetime).
        let rows = stmt
            .query_map(params![blob, limit as i64], |row| {
                Ok(ChunkResult {
                    memory_id: MemoryId::new(row.get::<_, String>(0)?),
                    text: row.get(1)?,
                    score: row.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into);
        rows
    }

    /// Return the stored `file_hash` for a repo path, or `None` if not indexed.
    ///
    /// Used by phase 6 index-consistency check to avoid a full reindex on every
    /// startup. If the stored hash equals the on-disk hash, the memory is clean.
    pub fn file_hash_for(&self, path: &RepoPath) -> Option<crate::model::Sha256> {
        self.connection
            .query_row("SELECT file_hash FROM memories WHERE path = ?1", [path.as_str()], |row| row.get::<_, String>(0))
            .ok()
            .map(crate::model::Sha256::new)
    }

    /// Query memories by structured filter.
    ///
    /// SQL is built dynamically rather than using `(?N IS NULL OR ...)` patterns
    /// because the latter defeats SQLite's index seek planner and forces a table
    /// scan even when a selective filter (e.g. PK lookup by `id`) is bound.
    /// Each filter combination yields a distinct prepared statement; `prepare_cached`
    /// keeps the small set of variants warm.
    pub fn query_memory(&self, query: &MemoryQuery) -> SubstrateResult<Vec<QueryResult>> {
        let mut sql = String::from("SELECT memories.id,memories.path,memories.summary FROM memories");
        if query.tag.is_some() {
            sql.push_str(" JOIN memory_tags ON memory_tags.memory_id = memories.id");
        }

        let mut filters = Vec::new();
        let mut bindings = Vec::new();
        append_memory_query_filters(query, &mut filters, &mut bindings)?;
        if let Some(tag) = query.tag.as_ref() {
            filters.push("memory_tags.tag = ?".to_string());
            bindings.push(rusqlite::types::Value::Text(tag.clone()));
        }
        append_filters_and_order(&mut sql, filters, "memories.id");
        collect_query_results(&self.connection, &sql, bindings).map_err(Into::into)
    }

    /// Query recall-index rows without hydrating full memory envelopes.
    pub fn query_recall_index(&self, query: &RecallIndexQuery) -> SubstrateResult<Vec<RecallIndexRow>> {
        self.query_recall_index_inner(query, false)
    }

    /// Query recall-index rows including encrypted metadata-only projections.
    pub fn query_recall_index_including_metadata_only(
        &self,
        query: &RecallIndexQuery,
    ) -> SubstrateResult<Vec<RecallIndexRow>> {
        self.query_recall_index_inner(query, true)
    }

    fn query_recall_index_inner(
        &self,
        query: &RecallIndexQuery,
        include_metadata_only: bool,
    ) -> SubstrateResult<Vec<RecallIndexRow>> {
        let mut sql = String::from(
            "SELECT memories.id,memories.path,memories.summary,memories.status,memories.scope,
                    memories.canonical_namespace_id,memories.updated_at,memories.indexed_at,memories.confidence,
                    memories.source_kind,memories.source_device,memories.sensitivity,memories.passive_recall,memories.index_body,
                    memories.requires_user_confirmation,memories.review_state,
                    memories.human_review_required,memories.max_scope
             FROM memories",
        );
        let mut filters = Vec::new();
        let mut bindings = Vec::new();
        append_recall_index_filters(query, include_metadata_only, &mut filters, &mut bindings)?;
        append_match_term_filters(query, &mut filters, &mut bindings);
        append_filters_and_order(&mut sql, filters, "memories.id");

        let mut stmt = self.connection.prepare_cached(&sql)?;
        let mut rows = stmt.query(params_from_iter(bindings.iter()))?;
        let mut results = Vec::new();
        while let Some(row) = rows.next()? {
            results.push(row_to_recall_index_row(row)?);
        }
        hydrate_recall_index_auxiliary(&self.connection, &mut results)?;
        Ok(results)
    }
}

/// Health helper for the derived events-log mirror.
pub fn query_events_log_mirror_health(
    connection: &Connection,
    canonical_events: &[Event],
) -> rusqlite::Result<EventsLogMirrorHealth> {
    let jsonl_max_seq = canonical_events.iter().map(|event| event.seq).max().unwrap_or(0);
    let jsonl_count = canonical_events.len() as u64;
    let sqlite_max_seq =
        connection.query_row("SELECT COALESCE(MAX(seq), 0) FROM events_log", [], |row| row.get::<_, i64>(0))? as u64;
    let sqlite_count = connection.query_row("SELECT COUNT(*) FROM events_log", [], |row| row.get::<_, i64>(0))? as u64;
    let missing_count = count_missing_events_log_rows(connection, canonical_events)?;
    Ok(EventsLogMirrorHealth {
        jsonl_max_seq,
        sqlite_max_seq,
        lag: jsonl_max_seq.saturating_sub(sqlite_max_seq),
        jsonl_count,
        sqlite_count,
        missing_count,
    })
}

fn mirror_event_row(connection: &Connection, event: &Event) -> rusqlite::Result<()> {
    let payload_json =
        serde_json::to_string(&event.kind).map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
    connection.execute(
        "INSERT OR REPLACE INTO events_log(event_id, device, seq, kind, memory_id, ts, payload_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            event.id.as_str(),
            event.device.as_str(),
            event.seq as i64,
            event_kind_name(&event.kind),
            event_memory_id(&event.kind),
            event.at.to_rfc3339(),
            payload_json,
        ],
    )?;
    Ok(())
}

fn count_missing_events_log_rows(connection: &Connection, canonical_events: &[Event]) -> rusqlite::Result<u64> {
    let mut stmt = connection.prepare("SELECT EXISTS(SELECT 1 FROM events_log WHERE event_id = ?1)")?;
    let mut missing = 0_u64;
    for event in canonical_events {
        let exists: i64 = stmt.query_row([event.id.as_str()], |row| row.get(0))?;
        if exists == 0 {
            missing = missing.saturating_add(1);
        }
    }
    Ok(missing)
}

fn event_kind_name(kind: &EventKind) -> &'static str {
    match kind {
        EventKind::WriteCommitted { .. } => "write_committed",
        EventKind::EncryptedWriteCommitted { .. } => "encrypted_write_committed",
        EventKind::TombstoneCommitted { .. } => "tombstone_committed",
        EventKind::DuplicateIdRepaired { .. } => "duplicate_id_repaired",
        EventKind::EmbeddingModelChanged { .. } => "embedding_model_changed",
        EventKind::StartupReconciliationCompleted { .. } => "startup_reconciliation_completed",
        EventKind::OperatorRepairRequired { .. } => "operator_repair_required",
        EventKind::GitPushFailed { .. } => "git_push_failed",
        EventKind::WriteRefused { .. } => "write_refused",
        EventKind::EncryptedContentRevealed { .. } => "encrypted_content_revealed",
        EventKind::SubstrateFragmentWritten { .. } => "substrate_fragment_written",
        EventKind::RecallHit { .. } => "recall_hit",
        EventKind::RealityCheckConfirmed { .. } => "reality_check_confirmed",
        EventKind::RealityCheckForgotten { .. } => "reality_check_forgotten",
        EventKind::RealityCheckNotRelevant { .. } => "reality_check_not_relevant",
        EventKind::ClaimLockContention { .. } => "claim_lock_contention",
        EventKind::DeviceKeysRotated { .. } => "device_keys_rotated",
        EventKind::PolicyChanged { .. } => "policy_changed",
    }
}

fn event_memory_id(kind: &EventKind) -> Option<&str> {
    match kind {
        EventKind::WriteCommitted { id, .. }
        | EventKind::EncryptedWriteCommitted { id, .. }
        | EventKind::TombstoneCommitted { id }
        | EventKind::WriteRefused { id: Some(id), .. }
        | EventKind::EncryptedContentRevealed { id, .. }
        | EventKind::RecallHit { id, .. }
        | EventKind::RealityCheckConfirmed { id, .. }
        | EventKind::RealityCheckForgotten { id, .. }
        | EventKind::RealityCheckNotRelevant { id, .. } => Some(id.as_str()),
        EventKind::ClaimLockContention { memory_id, .. } => Some(memory_id.as_str()),
        EventKind::WriteRefused { id: None, .. }
        | EventKind::DuplicateIdRepaired { .. }
        | EventKind::EmbeddingModelChanged { .. }
        | EventKind::StartupReconciliationCompleted { .. }
        | EventKind::OperatorRepairRequired { .. }
        | EventKind::GitPushFailed { .. }
        | EventKind::SubstrateFragmentWritten { .. }
        | EventKind::DeviceKeysRotated { .. }
        | EventKind::PolicyChanged { .. } => None,
    }
}

/// Validate: dimension OK, triple not dropped, content hash matches stored hash.
fn validate_update_preconditions(conn: &Connection, update: &EmbeddingUpdate) -> Result<(), VectorError> {
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
fn read_chunk_rowid(conn: &Connection, chunk_id: &str) -> Result<i64, VectorError> {
    conn.query_row("SELECT chunk_rowid FROM memory_chunks WHERE chunk_id=?1", [chunk_id], |row| row.get::<_, i64>(0))
        .map_err(Into::into)
}

/// Upsert the vector payload: sqlite-vec virtual table + chunk_vectors shadow.
///
/// Called OUTSIDE any SQLite transaction (spec §10.2.1 step 4).  If the
/// subsequent metadata transaction rolls back, the orphan vector row is cleaned
/// by the startup reconciliation pass.
#[allow(clippy::too_many_arguments)]
fn upsert_vector_payload(
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
    let vector_json = serde_json::to_string(vector).map_err(|e| VectorError::Storage(e.to_string()))?;
    conn.execute(
        "INSERT INTO chunk_vectors(chunk_id,provider,model_ref,dimension,vector_json) VALUES (?1,?2,?3,?4,?5)
         ON CONFLICT(chunk_id,provider,model_ref,dimension) DO UPDATE SET vector_json=excluded.vector_json",
        params![chunk_id, triple.provider, triple.model_ref, i64::from(triple.dimension), vector_json],
    )?;
    Ok(())
}

/// Record that a chunk was embedded: upsert `chunk_embedding_meta`.
fn upsert_chunk_embedding_meta(txn: &Transaction<'_>, update: &EmbeddingUpdate) -> Result<(), VectorError> {
    let vector_table = crate::index::sqlite_vec::vector_table_name(&update.triple);
    let embedded_at = chrono::Utc::now().to_rfc3339();
    txn.execute(
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
            vector_table,
            embedded_at,
            update.expected_chunk_hash.as_str()
        ],
    )?;
    Ok(())
}

/// Delete the pending job that triggered this embedding update.
fn resolve_pending_embedding_job(txn: &Transaction<'_>, update: &EmbeddingUpdate) -> Result<(), VectorError> {
    txn.execute(
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

/// Upsert a memory into SQLite, populating all `memories` columns (spec §10.1).
///
/// `file_hash` mirrors `body_hash` and `file_mtime_ns` is 0 until the write
/// path plumbs the real on-disk values (deferred).
fn upsert_memory_row_with_full_metadata(
    connection: &mut Connection,
    memory: &Memory,
    metadata_only: bool,
    active_embedding: &EmbeddingTriple,
) -> rusqlite::Result<()> {
    let active_embedding_dropped = is_dropped_triple_rusqlite(connection, active_embedding)?;
    let txn = connection.transaction()?;

    let path = resolve_memory_path(memory);
    let sensitivity = sensitivity_str(memory.frontmatter.sensitivity);
    let memory_type = memory_type_str(&memory.frontmatter.memory_type);
    let scope = scope_str(memory.frontmatter.scope);
    let trust_level = trust_level_str(memory.frontmatter.trust_level);
    let status = status_str(memory.frontmatter.status);
    let author = author_kind_str(memory.frontmatter.author.kind);
    let source_kind = source_kind_str(memory.frontmatter.source.kind);
    let body_hash = hash_bytes(memory.body.as_bytes()).to_string();
    let frontmatter_json = serde_json::to_string(&memory.frontmatter).unwrap_or_else(|_| "{}".to_string());
    let file_hash = body_hash.clone(); // placeholder; deferred: plumb from fs::metadata
    let file_mtime_ns: i64 = 0; // placeholder; deferred: plumb from fs::metadata
    let indexed_at = chrono::Utc::now().to_rfc3339();
    let created_at = memory.frontmatter.created_at.to_rfc3339();
    let updated_at = memory.frontmatter.updated_at.to_rfc3339();
    let observed_at = observed_at_for_index(memory).unwrap_or_else(|| created_at.clone());

    let passive_recall = memory.frontmatter.retrieval_policy.passive_recall as i64;
    let index_body = memory.frontmatter.retrieval_policy.index_body as i64;
    let human_review_required = memory.frontmatter.write_policy.human_review_required as i64;
    let max_scope = scope_str(memory.frontmatter.retrieval_policy.max_scope);

    txn.execute(
        "INSERT INTO memories(
             id, path, schema_version, type, scope, namespace, canonical_namespace_id,
             summary, confidence, original_confidence, trust_level, sensitivity, status, review_state,
             requires_user_confirmation, created_at, updated_at,
             observed_at, valid_from, valid_until, ttl,
             author, source_kind, source_harness, source_device,
             body_hash, frontmatter_json, file_hash, file_mtime_ns, indexed_at, metadata_only,
             passive_recall, index_body, human_review_required, max_scope
         ) VALUES (
             :id, :path, :schema_version, :type, :scope, :namespace, :canonical_namespace_id,
             :summary, :confidence, :original_confidence, :trust_level, :sensitivity, :status, :review_state,
             :requires_user_confirmation, :created_at, :updated_at,
             :observed_at, :valid_from, :valid_until, :ttl,
             :author, :source_kind, :source_harness, :source_device,
             :body_hash, :frontmatter_json, :file_hash, :file_mtime_ns, :indexed_at, :metadata_only,
             :passive_recall, :index_body, :human_review_required, :max_scope
         )
         ON CONFLICT(id) DO UPDATE SET
             path=excluded.path, schema_version=excluded.schema_version,
             type=excluded.type, scope=excluded.scope,
             namespace=excluded.namespace, canonical_namespace_id=excluded.canonical_namespace_id,
             summary=excluded.summary, confidence=excluded.confidence,
             original_confidence=excluded.original_confidence,
             trust_level=excluded.trust_level, sensitivity=excluded.sensitivity,
             status=excluded.status, review_state=excluded.review_state,
             requires_user_confirmation=excluded.requires_user_confirmation,
             updated_at=excluded.updated_at, observed_at=excluded.observed_at,
             valid_from=excluded.valid_from, valid_until=excluded.valid_until,
             ttl=excluded.ttl, author=excluded.author,
             source_kind=excluded.source_kind, source_harness=excluded.source_harness,
             source_device=excluded.source_device, body_hash=excluded.body_hash,
             frontmatter_json=excluded.frontmatter_json,
             file_hash=excluded.file_hash, file_mtime_ns=excluded.file_mtime_ns,
             indexed_at=excluded.indexed_at, metadata_only=excluded.metadata_only,
             passive_recall=excluded.passive_recall, index_body=excluded.index_body,
             human_review_required=excluded.human_review_required,
             max_scope=excluded.max_scope",
        named_params! {
            ":id":                        memory.frontmatter.id.as_str(),
            ":path":                      &path,
            ":schema_version":            memory.frontmatter.schema_version as i64,
            ":type":                      memory_type,
            ":scope":                     scope,
            ":namespace":                 &memory.frontmatter.namespace,
            ":canonical_namespace_id":    &memory.frontmatter.canonical_namespace_id,
            ":summary":                   &memory.frontmatter.summary,
            ":confidence":                memory.frontmatter.confidence,
            ":original_confidence":       memory.frontmatter.original_confidence,
            ":trust_level":               trust_level,
            ":sensitivity":               sensitivity,
            ":status":                    status,
            ":review_state":              &memory.frontmatter.review_state,
            ":requires_user_confirmation": memory.frontmatter.requires_user_confirmation as i64,
            ":created_at":                &created_at,
            ":updated_at":                &updated_at,
            ":observed_at":               &observed_at,
            ":valid_from":                rusqlite::types::Null,
            ":valid_until":               rusqlite::types::Null,
            ":ttl":                       rusqlite::types::Null,
            ":author":                    author,
            ":source_kind":               source_kind,
            ":source_harness":            &memory.frontmatter.source.harness,
            ":source_device":             &memory.frontmatter.source.device,
            ":body_hash":                 &body_hash,
            ":frontmatter_json":          &frontmatter_json,
            ":file_hash":                 &file_hash,
            ":file_mtime_ns":             file_mtime_ns,
            ":indexed_at":                &indexed_at,
            ":metadata_only":             metadata_only as i64,
            ":passive_recall":            passive_recall,
            ":index_body":                index_body,
            ":human_review_required":     human_review_required,
            ":max_scope":                 max_scope,
        },
    )?;

    sync_auxiliary_tables(&txn, memory)?;

    // Rebuild chunks for this memory.
    txn.execute("DELETE FROM memory_chunks WHERE memory_id = ?1", [memory.frontmatter.id.as_str()])?;
    if !metadata_only && memory.frontmatter.retrieval_policy.index_body {
        for chunk in chunk_memory(memory) {
            txn.execute(
                "INSERT INTO memory_chunks(memory_id,chunk_id,body_hash,text,start_byte,end_byte)
                 VALUES (?1,?2,?3,?4,?5,?6)",
                params![
                    memory.frontmatter.id.as_str(),
                    chunk.chunk_id.as_str(),
                    chunk.body_hash.as_str(),
                    chunk.text,
                    chunk.start_byte as i64,
                    chunk.end_byte as i64
                ],
            )?;
            if memory.frontmatter.retrieval_policy.index_embeddings && !active_embedding_dropped {
                let enqueued_at = chrono::Utc::now().to_rfc3339();
                txn.execute(
                    "INSERT OR IGNORE INTO pending_embedding_jobs(
                         chunk_id, provider, model_ref, dimension, content_hash, enqueued_at
                     ) VALUES (?1,?2,?3,?4,?5,?6)",
                    params![
                        chunk.chunk_id.as_str(),
                        active_embedding.provider.as_str(),
                        active_embedding.model_ref.as_str(),
                        i64::from(active_embedding.dimension),
                        chunk.body_hash.as_str(),
                        enqueued_at
                    ],
                )?;
            }
        }
    }

    txn.commit()
}

/// Sync priority auxiliary tables: tags, aliases, entities, evidence, supersession.
///
/// Each table is replaced wholesale for this memory_id — safe because these
/// are derived projections; canonical data lives in the Markdown file.
///
/// Deferred: memory_related, memory_regressions tables.
fn sync_auxiliary_tables(txn: &Transaction<'_>, memory: &Memory) -> rusqlite::Result<()> {
    let id = memory.frontmatter.id.as_str();
    sync_tags(txn, id, &memory.frontmatter.tags)?;
    sync_aliases(txn, id, &memory.frontmatter.aliases)?;
    sync_entities(txn, id, &memory.frontmatter.entities)?;
    sync_evidence(txn, id, &memory.frontmatter.evidence)?;
    sync_supersession(txn, id, &memory.frontmatter.supersedes)?;
    Ok(())
}

fn sync_tags(txn: &Transaction<'_>, memory_id: &str, tags: &[String]) -> rusqlite::Result<()> {
    txn.execute("DELETE FROM memory_tags WHERE memory_id = ?1", [memory_id])?;
    for tag in tags {
        txn.execute("INSERT OR IGNORE INTO memory_tags(memory_id, tag) VALUES (?1, ?2)", params![memory_id, tag])?;
    }
    Ok(())
}

fn sync_aliases(txn: &Transaction<'_>, memory_id: &str, aliases: &[String]) -> rusqlite::Result<()> {
    txn.execute("DELETE FROM memory_aliases WHERE memory_id = ?1", [memory_id])?;
    for alias in aliases {
        txn.execute(
            "INSERT OR IGNORE INTO memory_aliases(memory_id, alias) VALUES (?1, ?2)",
            params![memory_id, alias],
        )?;
    }
    Ok(())
}

fn sync_entities(txn: &Transaction<'_>, memory_id: &str, entities: &[crate::model::Entity]) -> rusqlite::Result<()> {
    txn.execute("DELETE FROM memory_entity_aliases WHERE memory_id = ?1", [memory_id])?;
    txn.execute("DELETE FROM memory_entities WHERE memory_id = ?1", [memory_id])?;
    for entity in entities {
        txn.execute(
            "INSERT OR IGNORE INTO memory_entities(memory_id, entity_id, label) VALUES (?1, ?2, ?3)",
            params![memory_id, entity.id, entity.label],
        )?;
        for alias in &entity.aliases {
            txn.execute(
                "INSERT OR IGNORE INTO memory_entity_aliases(memory_id, entity_id, alias) VALUES (?1, ?2, ?3)",
                params![memory_id, entity.id, alias],
            )?;
        }
    }
    Ok(())
}

fn sync_evidence(txn: &Transaction<'_>, memory_id: &str, evidence: &[crate::model::Evidence]) -> rusqlite::Result<()> {
    txn.execute("DELETE FROM memory_evidence WHERE memory_id = ?1", [memory_id])?;
    for ev in evidence {
        let observed_at = ev.observed_at.as_ref().map(|t| t.to_rfc3339());
        txn.execute(
            "INSERT OR IGNORE INTO memory_evidence(
                 memory_id, evidence_id, quote, quote_norm_hash, ref_text, weight, observed_at
             ) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![memory_id, ev.id, ev.quote, ev.quote_norm_hash, ev.reference, ev.weight, observed_at],
        )?;
    }
    Ok(())
}

fn sync_supersession(txn: &Transaction<'_>, memory_id: &str, supersedes: &[MemoryId]) -> rusqlite::Result<()> {
    txn.execute("DELETE FROM memory_supersession WHERE memory_id = ?1", [memory_id])?;
    for supersedes_id in supersedes {
        txn.execute(
            "INSERT OR IGNORE INTO memory_supersession(memory_id, supersedes_id) VALUES (?1, ?2)",
            params![memory_id, supersedes_id.as_str()],
        )?;
    }
    Ok(())
}

/// Delete orphan vectors/meta rows and enqueue missing embeddings for the active triple.
///
/// Takes `&mut Connection` — enforces exclusive access, preventing
/// `unchecked_transaction` races.  Spec §10.2.1 step 5.
///
/// Content-hash check: drops pending jobs whose `content_hash` no longer
/// matches `memory_chunks.body_hash` (spec §10.2.1 #6 third bullet).
fn reconcile_active_embedding_jobs_impl(
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
fn is_dropped_triple(conn: &Connection, triple: &EmbeddingTriple) -> Result<bool, VectorError> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM dropped_embedding_triples
         WHERE provider=?1 AND model_ref=?2 AND dimension=?3)",
        params![triple.provider, triple.model_ref, i64::from(triple.dimension)],
        |row| row.get::<_, i64>(0),
    )
    .map(|v| v != 0)
    .map_err(Into::into)
}

/// Same check but returns `rusqlite::Result` for callers already in that error domain.
fn is_dropped_triple_rusqlite(conn: &Connection, triple: &EmbeddingTriple) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM dropped_embedding_triples
         WHERE provider=?1 AND model_ref=?2 AND dimension=?3)",
        params![triple.provider, triple.model_ref, i64::from(triple.dimension)],
        |row| row.get::<_, i64>(0),
    )
    .map(|v| v != 0)
}

fn table_exists(conn: &Connection, table: &str) -> Result<bool, VectorError> {
    conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)", [table], |row| {
        row.get::<_, i64>(0)
    })
    .map(|v| v != 0)
    .map_err(Into::into)
}

fn ensure_vector_table(conn: &Connection, triple: &EmbeddingTriple) -> Result<(), VectorError> {
    let table = crate::index::sqlite_vec::vector_table_name(triple);
    conn.execute(
        &format!("CREATE VIRTUAL TABLE IF NOT EXISTS {table} USING vec0(embedding float[{}])", triple.dimension),
        [],
    )
    .map(|_| ())
    .map_err(Into::into)
}

fn append_memory_query_filters(
    query: &MemoryQuery,
    filters: &mut Vec<String>,
    bindings: &mut Vec<rusqlite::types::Value>,
) -> SubstrateResult<()> {
    if let Some(id) = query.id.as_ref() {
        filters.push("memories.id = ?".to_string());
        bindings.push(rusqlite::types::Value::Text(id.as_str().to_string()));
    }
    if !query.include_metadata_only {
        filters.push("memories.metadata_only = 0".to_string());
    }
    if let Some(status) = query.status {
        filters.push("memories.status = ?".to_string());
        bindings.push(rusqlite::types::Value::Text(status_str(status).to_string()));
    }
    append_namespace_filter(query.namespace_prefix.as_deref(), filters, bindings)?;
    if query.passive_recall_only {
        filters.push("memories.passive_recall = 1".to_string());
    }
    if let Some(updated_since) = query.updated_since.as_ref() {
        filters.push("memories.updated_at >= ?".to_string());
        bindings.push(rusqlite::types::Value::Text(updated_since.to_rfc3339()));
    }
    Ok(())
}

fn append_recall_index_filters(
    query: &RecallIndexQuery,
    include_metadata_only: bool,
    filters: &mut Vec<String>,
    bindings: &mut Vec<rusqlite::types::Value>,
) -> SubstrateResult<()> {
    append_namespace_filter(query.namespace_prefix.as_deref(), filters, bindings)?;
    if !include_metadata_only {
        filters.push("memories.metadata_only = 0".to_string());
    }
    if !query.statuses.is_empty() {
        let placeholders = vec!["?"; query.statuses.len()].join(",");
        filters.push(format!("memories.status IN ({placeholders})"));
        for status in &query.statuses {
            bindings.push(rusqlite::types::Value::Text(status_str(*status).to_string()));
        }
    }
    if query.passive_recall_only {
        filters.push("memories.passive_recall = 1".to_string());
    }
    if let Some(updated_since) = query.updated_since.as_ref() {
        filters.push("memories.updated_at >= ?".to_string());
        bindings.push(rusqlite::types::Value::Text(updated_since.to_rfc3339()));
    }
    Ok(())
}

fn observed_at_for_index(memory: &Memory) -> Option<String> {
    memory.frontmatter.observed_at.as_ref().map(chrono::DateTime::to_rfc3339)
}

fn append_namespace_filter(
    namespace_prefix: Option<&str>,
    filters: &mut Vec<String>,
    bindings: &mut Vec<rusqlite::types::Value>,
) -> SubstrateResult<()> {
    match namespace_prefix.map(parse_namespace_prefix).transpose()? {
        Some(NamespaceFilter::Scope(scope)) => {
            filters.push("memories.scope = ?".to_string());
            bindings.push(rusqlite::types::Value::Text(scope.to_string()));
        }
        Some(NamespaceFilter::ScopeAndCanonicalId { scope, canonical_id }) => {
            filters.push("memories.scope = ?".to_string());
            bindings.push(rusqlite::types::Value::Text(scope.to_string()));
            filters.push("memories.canonical_namespace_id = ?".to_string());
            bindings.push(rusqlite::types::Value::Text(canonical_id));
        }
        None => {}
    }
    Ok(())
}

fn append_match_term_filters(
    query: &RecallIndexQuery,
    filters: &mut Vec<String>,
    bindings: &mut Vec<rusqlite::types::Value>,
) {
    // Recall match terms intentionally use union semantics. A passive recall request should surface
    // candidates matching any observed tag, alias, entity id, or entity alias from the current turn.
    let terms = query.match_terms.iter().filter(|term| !term.trim().is_empty()).collect::<Vec<_>>();
    if terms.is_empty() {
        return;
    }

    let mut clauses = Vec::new();
    for term in terms {
        clauses.push(
            "(EXISTS (SELECT 1 FROM memory_tags WHERE memory_tags.memory_id = memories.id AND memory_tags.tag = ? COLLATE NOCASE)
              OR EXISTS (SELECT 1 FROM memory_aliases WHERE memory_aliases.memory_id = memories.id AND memory_aliases.alias = ? COLLATE NOCASE)
              OR EXISTS (SELECT 1 FROM memory_entities WHERE memory_entities.memory_id = memories.id AND (memory_entities.entity_id = ? OR memory_entities.label = ? COLLATE NOCASE))
              OR EXISTS (SELECT 1 FROM memory_entity_aliases WHERE memory_entity_aliases.memory_id = memories.id AND memory_entity_aliases.alias = ? COLLATE NOCASE))"
                .to_string(),
        );
        for _ in 0..5 {
            bindings.push(rusqlite::types::Value::Text(term.to_string()));
        }
    }
    filters.push(format!("({})", clauses.join(" OR ")));
}

fn append_filters_and_order(sql: &mut String, filters: Vec<String>, order_by: &str) {
    if !filters.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&filters.join(" AND "));
    }
    sql.push_str(" ORDER BY ");
    sql.push_str(order_by);
}

enum NamespaceFilter {
    Scope(&'static str),
    ScopeAndCanonicalId { scope: &'static str, canonical_id: String },
}

fn parse_namespace_prefix(value: &str) -> SubstrateResult<NamespaceFilter> {
    match value {
        "me" => Ok(NamespaceFilter::Scope("user")),
        "agent" => Ok(NamespaceFilter::Scope("agent")),
        _ if value.starts_with("project:") => parse_scoped_namespace(value, "project:", "project"),
        _ if value.starts_with("org:") => parse_scoped_namespace(value, "org:", "org"),
        _ => Err(invalid_namespace_prefix(value)),
    }
}

fn parse_scoped_namespace(value: &str, prefix: &str, scope: &'static str) -> SubstrateResult<NamespaceFilter> {
    let canonical_id = value.strip_prefix(prefix).unwrap_or_default();
    if canonical_id.is_empty() || canonical_id.contains(':') {
        return Err(invalid_namespace_prefix(value));
    }
    Ok(NamespaceFilter::ScopeAndCanonicalId { scope, canonical_id: canonical_id.to_string() })
}

fn invalid_namespace_prefix(value: &str) -> SubstrateError {
    SubstrateError::InvalidQuery {
        field: "namespace_prefix".to_string(),
        value: value.to_string(),
        message: "invalid_query: expected one of me, agent, project:<canonical_id>, org:<canonical_id>".to_string(),
    }
}

fn collect_query_results(
    conn: &Connection,
    sql: &str,
    bindings: Vec<rusqlite::types::Value>,
) -> rusqlite::Result<Vec<QueryResult>> {
    let mut stmt = conn.prepare_cached(sql)?;
    let mut rows = stmt.query(params_from_iter(bindings.iter()))?;
    let mut results = Vec::new();
    while let Some(row) = rows.next()? {
        results.push(row_to_result(row)?);
    }
    Ok(results)
}

fn row_to_recall_index_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RecallIndexRow> {
    Ok(RecallIndexRow {
        id: MemoryId::new(row.get::<_, String>(0)?),
        // `from_unchecked`: path was validated at index-write time; hydrating from DB row.
        path: RepoPath::from_unchecked(row.get::<_, String>(1)?),
        summary: row.get(2)?,
        status: memory_status_from_str(row.get::<_, String>(3)?.as_str())?,
        scope: scope_from_str(row.get::<_, String>(4)?.as_str())?,
        canonical_namespace_id: row.get(5)?,
        updated_at: parse_index_time(row.get::<_, String>(6)?.as_str())?,
        indexed_at: parse_index_time(row.get::<_, String>(7)?.as_str())?,
        confidence: row.get(8)?,
        source_kind: source_kind_from_str(row.get::<_, String>(9)?.as_str())?,
        source_device: row.get(10)?,
        sensitivity: sensitivity_from_str(row.get::<_, String>(11)?.as_str())?,
        passive_recall: row.get::<_, i64>(12)? != 0,
        index_body: row.get::<_, i64>(13)? != 0,
        requires_user_confirmation: row.get::<_, i64>(14)? != 0,
        review_state: row.get(15)?,
        human_review_required: row.get::<_, i64>(16)? != 0,
        max_scope: scope_from_str(row.get::<_, String>(17)?.as_str())?,
        tags: Vec::new(),
        aliases: Vec::new(),
        entities: Vec::new(),
    })
}

fn hydrate_recall_index_auxiliary(conn: &Connection, rows: &mut [RecallIndexRow]) -> rusqlite::Result<()> {
    if rows.is_empty() {
        return Ok(());
    }

    let ids = rows.iter().map(|row| row.id.as_str().to_owned()).collect::<Vec<_>>();
    let mut tags_by_memory = read_strings_by_memory(
        conn,
        AuxiliaryStringTable {
            table: "memory_tags",
            column: "tag",
            order_by: "ORDER BY memory_id, tag COLLATE NOCASE, tag",
        },
        &ids,
    )?;
    let mut aliases_by_memory = read_strings_by_memory(
        conn,
        AuxiliaryStringTable {
            table: "memory_aliases",
            column: "alias",
            order_by: "ORDER BY memory_id, alias COLLATE NOCASE, alias",
        },
        &ids,
    )?;
    let mut entities_by_memory = read_entities_by_memory(conn, &ids)?;

    for row in rows {
        row.tags = tags_by_memory.remove(row.id.as_str()).unwrap_or_default();
        row.aliases = aliases_by_memory.remove(row.id.as_str()).unwrap_or_default();
        row.entities = entities_by_memory.remove(row.id.as_str()).unwrap_or_default();
    }
    Ok(())
}

struct AuxiliaryStringTable {
    table: &'static str,
    column: &'static str,
    order_by: &'static str,
}

fn read_strings_by_memory(
    conn: &Connection,
    table: AuxiliaryStringTable,
    ids: &[String],
) -> rusqlite::Result<BTreeMap<String, Vec<String>>> {
    let placeholders = vec!["?"; ids.len()].join(",");
    let sql = format!(
        "SELECT memory_id,{} FROM {} WHERE memory_id IN ({placeholders}) {}",
        table.column, table.table, table.order_by
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let mut rows = stmt.query(params_from_iter(ids.iter()))?;
    let mut values = BTreeMap::<String, Vec<String>>::new();
    while let Some(row) = rows.next()? {
        values.entry(row.get::<_, String>(0)?).or_default().push(row.get(1)?);
    }
    Ok(values)
}

fn read_entities_by_memory(conn: &Connection, ids: &[String]) -> rusqlite::Result<BTreeMap<String, Vec<Entity>>> {
    let placeholders = vec!["?"; ids.len()].join(",");
    let sql = format!(
        "SELECT memory_id,entity_id,label FROM memory_entities
         WHERE memory_id IN ({placeholders})
         ORDER BY memory_id, entity_id COLLATE NOCASE, entity_id"
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let mut rows = stmt.query(params_from_iter(ids.iter()))?;
    let aliases_by_entity = read_entity_aliases_by_memory(conn, ids)?;
    let mut entities = BTreeMap::<String, Vec<Entity>>::new();
    while let Some(row) = rows.next()? {
        let memory_id = row.get::<_, String>(0)?;
        let entity_id = row.get::<_, String>(1)?;
        let label = row.get::<_, String>(2)?;
        let aliases = aliases_by_entity.get(&(memory_id.clone(), entity_id.clone())).cloned().unwrap_or_default();
        entities.entry(memory_id).or_default().push(Entity { id: entity_id, label, aliases });
    }
    Ok(entities)
}

fn read_entity_aliases_by_memory(
    conn: &Connection,
    ids: &[String],
) -> rusqlite::Result<BTreeMap<(String, String), Vec<String>>> {
    let placeholders = vec!["?"; ids.len()].join(",");
    let sql = format!(
        "SELECT memory_id,entity_id,alias FROM memory_entity_aliases
         WHERE memory_id IN ({placeholders})
         ORDER BY memory_id, entity_id COLLATE NOCASE, entity_id, alias COLLATE NOCASE, alias"
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let mut rows = stmt.query(params_from_iter(ids.iter()))?;
    let mut aliases = BTreeMap::<(String, String), Vec<String>>::new();
    while let Some(row) = rows.next()? {
        aliases.entry((row.get(0)?, row.get(1)?)).or_default().push(row.get(2)?);
    }
    Ok(aliases)
}

fn row_to_result(row: &rusqlite::Row<'_>) -> rusqlite::Result<QueryResult> {
    Ok(QueryResult {
        id: MemoryId::new(row.get::<_, String>(0)?),
        // `from_unchecked`: path was validated at index-write time; hydrating from DB row.
        path: RepoPath::from_unchecked(row.get::<_, String>(1)?),
        summary: row.get(2)?,
    })
}

fn resolve_memory_path(memory: &Memory) -> String {
    memory
        .path
        .as_ref()
        .map_or_else(|| format!("agent/patterns/{}.md", memory.frontmatter.id.as_str()), |p| p.as_str().to_string())
}

fn sensitivity_str(s: Sensitivity) -> &'static str {
    match s {
        Sensitivity::Public => "public",
        Sensitivity::Internal => "internal",
        Sensitivity::Confidential => "confidential",
        Sensitivity::Personal => "personal",
    }
}

fn sensitivity_from_str(value: &str) -> rusqlite::Result<Sensitivity> {
    match value {
        "public" => Ok(Sensitivity::Public),
        "internal" => Ok(Sensitivity::Internal),
        "confidential" => Ok(Sensitivity::Confidential),
        "personal" => Ok(Sensitivity::Personal),
        _ => Err(invalid_column_value("sensitivity", value)),
    }
}

fn memory_type_str(t: &crate::model::MemoryType) -> &'static str {
    match t {
        crate::model::MemoryType::Project => "project",
        crate::model::MemoryType::Person => "person",
        crate::model::MemoryType::Procedure => "procedure",
        crate::model::MemoryType::Episode => "episode",
        crate::model::MemoryType::Claim => "claim",
        crate::model::MemoryType::Artifact => "artifact",
        crate::model::MemoryType::Prospective => "prospective",
        crate::model::MemoryType::Pattern => "pattern",
        crate::model::MemoryType::Playbook => "playbook",
        crate::model::MemoryType::Postmortem => "postmortem",
        crate::model::MemoryType::AntiPattern => "anti-pattern",
        crate::model::MemoryType::Heuristic => "heuristic",
        crate::model::MemoryType::Regression => "regression",
        crate::model::MemoryType::Correction => "correction",
        crate::model::MemoryType::Invariant => "invariant",
        crate::model::MemoryType::Decision => "decision",
        crate::model::MemoryType::OpenQuestion => "open-question",
    }
}

fn scope_str(s: crate::model::Scope) -> &'static str {
    match s {
        crate::model::Scope::User => "user",
        crate::model::Scope::Project => "project",
        crate::model::Scope::Org => "org",
        crate::model::Scope::Agent => "agent",
        crate::model::Scope::Subagent => "subagent",
    }
}

fn scope_from_str(value: &str) -> rusqlite::Result<Scope> {
    match value {
        "user" => Ok(Scope::User),
        "project" => Ok(Scope::Project),
        "org" => Ok(Scope::Org),
        "agent" => Ok(Scope::Agent),
        "subagent" => Ok(Scope::Subagent),
        _ => Err(invalid_column_value("scope", value)),
    }
}

fn trust_level_str(t: crate::model::TrustLevel) -> &'static str {
    match t {
        crate::model::TrustLevel::Trusted => "trusted",
        crate::model::TrustLevel::Untrusted => "untrusted",
        crate::model::TrustLevel::Candidate => "candidate",
        crate::model::TrustLevel::Quarantined => "quarantined",
        crate::model::TrustLevel::Pinned => "pinned",
    }
}

fn status_str(s: crate::model::MemoryStatus) -> &'static str {
    match s {
        crate::model::MemoryStatus::Candidate => "candidate",
        crate::model::MemoryStatus::Active => "active",
        crate::model::MemoryStatus::Pinned => "pinned",
        crate::model::MemoryStatus::Superseded => "superseded",
        crate::model::MemoryStatus::Archived => "archived",
        crate::model::MemoryStatus::Tombstoned => "tombstoned",
        crate::model::MemoryStatus::Quarantined => "quarantined",
    }
}

fn memory_status_from_str(value: &str) -> rusqlite::Result<MemoryStatus> {
    match value {
        "candidate" => Ok(MemoryStatus::Candidate),
        "active" => Ok(MemoryStatus::Active),
        "pinned" => Ok(MemoryStatus::Pinned),
        "superseded" => Ok(MemoryStatus::Superseded),
        "archived" => Ok(MemoryStatus::Archived),
        "tombstoned" => Ok(MemoryStatus::Tombstoned),
        "quarantined" => Ok(MemoryStatus::Quarantined),
        _ => Err(invalid_column_value("status", value)),
    }
}

fn author_kind_str(k: crate::model::AuthorKind) -> &'static str {
    match k {
        crate::model::AuthorKind::User => "user",
        crate::model::AuthorKind::Agent => "agent",
        crate::model::AuthorKind::Subagent => "subagent",
        crate::model::AuthorKind::Dreaming => "dreaming",
        crate::model::AuthorKind::System => "system",
    }
}

fn source_kind_str(k: SourceKind) -> &'static str {
    match k {
        SourceKind::User => "user",
        SourceKind::AgentPrimary => "agent-primary",
        SourceKind::AgentSubagent => "agent-subagent",
        SourceKind::Tool => "tool",
        SourceKind::Web => "web",
        SourceKind::Email => "email",
        SourceKind::File => "file",
        SourceKind::Synthesis => "synthesis",
        SourceKind::Import => "import",
        SourceKind::System => "system",
    }
}

fn source_kind_from_str(value: &str) -> rusqlite::Result<SourceKind> {
    match value {
        "user" => Ok(SourceKind::User),
        "agent-primary" => Ok(SourceKind::AgentPrimary),
        "agent-subagent" => Ok(SourceKind::AgentSubagent),
        "tool" => Ok(SourceKind::Tool),
        "web" => Ok(SourceKind::Web),
        "email" => Ok(SourceKind::Email),
        "file" => Ok(SourceKind::File),
        "synthesis" => Ok(SourceKind::Synthesis),
        "import" => Ok(SourceKind::Import),
        "system" => Ok(SourceKind::System),
        _ => Err(invalid_column_value("source_kind", value)),
    }
}

fn parse_index_time(value: &str) -> rusqlite::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|time| time.with_timezone(&Utc))
        .map_err(|err| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(err)))
}

fn invalid_column_value(field: &'static str, value: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Text,
        Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("invalid {field}: {value}"))),
    )
}

/// Sanitize a free-form user query for FTS5.
///
/// FTS5 has its own query syntax — `NOT`, `AND`, `OR`, `"phrase"`, column
/// qualifiers `col:term`, and the bare `-` prefix that means NOT. Forwarding
/// raw user text into MATCH means a query like `end-to-end` is parsed as
/// `end NOT to NOT end`, where `to` is then misread as a column qualifier and
/// the whole thing returns `sqlite error: no such column: to`.
///
/// The substrate's contract with callers is that `query.text` is a search
/// string, not an FTS5 expression. So at this boundary we transform the input
/// into a sequence of FTS5 phrase tokens — one quoted phrase per
/// whitespace-separated chunk, double-quotes escaped by doubling. Multiple
/// phrases are AND-ed by FTS5's default expression semantics.
///
/// Tokens with no alphanumeric content are dropped because FTS5's tokenizer
/// would reduce them to zero terms inside the phrase, which is a syntax error
/// in some FTS5 builds. An input that produces no usable tokens yields an
/// empty string; the caller short-circuits to an empty result set.
fn sanitize_fts_query(input: &str) -> String {
    input
        .split_whitespace()
        .filter(|token| token.chars().any(|character| character.is_alphanumeric()))
        .map(|token| format!("\"{}\"", token.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::sanitize_fts_query;

    #[test]
    fn sanitize_plain_word_wraps_as_single_phrase() {
        assert_eq!(sanitize_fts_query("needle"), "\"needle\"");
    }

    #[test]
    fn sanitize_multiple_words_ands_via_separate_phrases() {
        assert_eq!(sanitize_fts_query("daemon socket protocol"), "\"daemon\" \"socket\" \"protocol\"");
    }

    #[test]
    fn sanitize_hyphenated_word_stays_intact_inside_phrase() {
        // Inside FTS5 phrase quoting the tokenizer splits on `-`, so this
        // matches a body indexed as `end to end` — exactly what we want for
        // hyphenated agent queries. The key property is no MATCH error.
        assert_eq!(sanitize_fts_query("end-to-end"), "\"end-to-end\"");
    }

    #[test]
    fn sanitize_escapes_internal_double_quotes() {
        assert_eq!(sanitize_fts_query("say\"hi"), "\"say\"\"hi\"");
    }

    #[test]
    fn sanitize_drops_punctuation_only_tokens() {
        assert_eq!(sanitize_fts_query("hello -- world"), "\"hello\" \"world\"");
    }

    #[test]
    fn sanitize_empty_input_yields_empty_string() {
        assert_eq!(sanitize_fts_query(""), "");
        assert_eq!(sanitize_fts_query("   "), "");
        assert_eq!(sanitize_fts_query("--- !@#"), "");
    }

    #[test]
    fn sanitize_strips_fts5_operator_intent() {
        // `NOT to` is operator syntax in FTS5; after sanitization it becomes
        // two phrase matches, both required, neither one a NOT.
        assert_eq!(sanitize_fts_query("foo NOT bar"), "\"foo\" \"NOT\" \"bar\"");
    }
}
