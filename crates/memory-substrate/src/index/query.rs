//! Index upsert and query helpers.
//!
//! Layout (stepdown / newspaper): orchestrator-level methods first, SQL helpers
//! below.  Column lists, value bindings, and index names are kept in the same
//! vertical region as the statement that uses them so readers don't scroll.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use rusqlite::{named_params, params, params_from_iter, Connection, Transaction};

use crate::error::{SubstrateError, SubstrateResult, VectorError};
use crate::events::{Event, EventKind};
use crate::index::chunking::chunk_memory;
use crate::markdown::hash_bytes;
use crate::model::{
    AuxScope, ChunkResult, EmbeddingTriple, EmbeddingUpdate, Entity, EventsLogMirrorHealth, HybridMemoryCandidate,
    HybridScoreBreakdown, HybridVectorQuery, Memory, MemoryId, MemoryQuery, MemoryStatus, QueryResult,
    RecallIndexQuery, RecallIndexRow, RepoPath, ReviewQueuePage, ReviewQueueRow, Scope, Sensitivity, Sha256,
    SourceKind,
};

use super::{bucketed_in_clause_width, pad_in_clause_bindings, sql_placeholders};

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
        self.upsert_memory_with_file_hash(memory, metadata_only, None)
    }

    /// Upsert a memory with the actual on-disk file hash when the caller has it.
    ///
    /// Startup reconciliation uses this to compare future disk reads against the
    /// exact bytes that were indexed, not merely the body hash.
    pub fn upsert_memory_with_file_hash(
        &mut self,
        memory: &Memory,
        metadata_only: bool,
        file_hash: Option<&Sha256>,
    ) -> rusqlite::Result<()> {
        upsert_memory_row_with_full_metadata(
            &mut self.connection,
            memory,
            MemoryUpsertOptions { metadata_only, file_hash, active_embedding: &self.active_embedding },
        )
    }

    /// Clear plaintext-derived rows before reindexing Markdown files.
    ///
    /// Encrypted-tier rows (`encrypted/%`) are intentionally preserved here:
    /// their safe projections are handled by the encrypted incremental/full
    /// reindex paths, and out-of-band encrypted deletions are not pruned by this
    /// plaintext clear.
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

    /// Drop plaintext index rows whose canonical file no longer exists on disk.
    ///
    /// Startup orphan sweep (spec §13.5.1 phase-6 companion): the index can
    /// retain rows for memories that were deleted or moved out of band (e.g. a
    /// tombstone landed via merge, or an operator removed a file). Incremental
    /// reindex only visits files that *exist*, so without this sweep those rows
    /// would linger.
    ///
    /// Stats each indexed plaintext `repo_path` against `repo`; rows whose path
    /// no longer stats are removed. Encrypted-tier rows (`encrypted/%`) are left
    /// untouched, which means out-of-band encrypted deletions are a known gap in
    /// this sweep rather than "fully handled" orphan cleanup. This deletes
    /// **derived index rows only** — never canonical files; tombstones remain the
    /// sole delete path for memories. Returns the number of orphaned memory rows
    /// removed.
    ///
    /// Cost is O(n_index_rows) stat calls, not O(n) file reads.
    pub fn prune_orphaned_plaintext_rows(&mut self, repo: &std::path::Path) -> rusqlite::Result<usize> {
        let orphan_paths: Vec<String> = {
            let mut stmt = self.connection.prepare("SELECT path FROM memories WHERE path NOT LIKE 'encrypted/%'")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            let mut orphans = Vec::new();
            for path in rows {
                let path = path?;
                if !repo.join(&path).exists() {
                    orphans.push(path);
                }
            }
            orphans
        };
        if orphan_paths.is_empty() {
            return Ok(0);
        }

        let txn = self.connection.transaction()?;
        let mut removed = 0usize;
        for path in &orphan_paths {
            // FK cascade (memory_chunks, memory_tags/aliases/entities/evidence/
            // supersession, chunk_embedding_meta) fires off this delete; the two
            // FK-less projection tables are swept by chunk-id below.
            removed += txn.execute("DELETE FROM memories WHERE path = ?1", [path])?;
        }
        txn.execute("DELETE FROM chunk_vectors WHERE chunk_id NOT IN (SELECT chunk_id FROM memory_chunks)", [])?;
        txn.execute(
            "DELETE FROM pending_embedding_jobs WHERE chunk_id NOT IN (SELECT chunk_id FROM memory_chunks)",
            [],
        )?;
        txn.commit()?;
        Ok(removed)
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

    /// Deferred second pass for supersession edges after a bulk reindex.
    ///
    /// The per-memory `sync_supersession` insert is FK-guarded (it skips any
    /// edge whose `supersedes` target's `memories` row does not yet exist), so a
    /// bulk reindex that visits a supersessor before its target silently drops
    /// that edge rather than aborting. This pass re-derives every supersession
    /// edge from the indexed `frontmatter_json`, by which point all `memories`
    /// rows of the bulk pass are present, so every edge whose target is indexed
    /// is (re-)added. It is the exact set-based form the v4 migration's
    /// supersession bootstrap uses (`migrations.rs`), kept identical so the two
    /// can never drift, and is idempotent: `INSERT OR IGNORE` over a table whose
    /// per-memory rows the upsert path already replaced, plus the same
    /// `EXISTS (target)` guard, leaves an already-consistent table unchanged.
    ///
    /// Returns the number of edge rows inserted by this pass (edges already
    /// present, or whose target is still unindexed, contribute zero).
    pub fn resync_supersession_edges(&mut self) -> rusqlite::Result<usize> {
        let inserted = self.connection.execute(
            "INSERT OR IGNORE INTO memory_supersession(memory_id, supersedes_id)
             SELECT memories.id, superseded.value
             FROM memories, json_each(memories.frontmatter_json, '$.supersedes') AS superseded
             WHERE superseded.value IS NOT NULL
               AND EXISTS (SELECT 1 FROM memories AS target WHERE target.id = superseded.value)",
            [],
        )?;
        Ok(inserted)
    }

    /// Supersession targets absent from the current `memories` table.
    ///
    /// Bulk reindex callers intentionally ignore this and rely on
    /// [`Self::resync_supersession_edges`] after all rows are present. Runtime
    /// write callers use it to detect when the FK guard skipped a declared edge
    /// and must leave a durable repair signal instead of silently dropping the
    /// relation until the next open.
    pub fn missing_supersession_targets(&self, supersedes: &[MemoryId]) -> rusqlite::Result<Vec<MemoryId>> {
        let mut missing = Vec::new();
        let mut stmt = self.connection.prepare_cached("SELECT EXISTS(SELECT 1 FROM memories WHERE id = ?1)")?;
        for supersedes_id in supersedes {
            let exists: i64 = stmt.query_row([supersedes_id.as_str()], |row| row.get(0))?;
            if exists == 0 {
                missing.push(supersedes_id.clone());
            }
        }
        Ok(missing)
    }

    /// The active embedding triple this index was opened with.
    pub fn active_embedding(&self) -> &EmbeddingTriple {
        &self.active_embedding
    }

    /// Drain up to `limit` pending embedding jobs for the active triple, joined
    /// to the chunk text the worker must embed.
    ///
    /// Only jobs whose chunk still exists *and* whose `content_hash` still
    /// matches the live `memory_chunks.body_hash` are returned — a stale job
    /// (chunk edited since enqueue) is skipped here and swept by
    /// [`Self::reconcile_active_embedding_jobs`]. Returning the matching
    /// `content_hash` lets the worker pass it back as `expected_chunk_hash` so
    /// the vector-store write is gated a second time at commit (spec §10.2.1).
    ///
    /// Ordered by `enqueued_at` so the oldest backlog drains first.
    pub fn pending_embedding_jobs(&self, limit: usize) -> Result<Vec<crate::model::PendingEmbeddingJob>, VectorError> {
        let triple = &self.active_embedding;
        let mut stmt = self.connection.prepare_cached(
            "SELECT mc.chunk_id, mc.text, mc.body_hash
             FROM pending_embedding_jobs pj
             JOIN memory_chunks mc ON mc.chunk_id = pj.chunk_id
             WHERE pj.provider = ?1 AND pj.model_ref = ?2 AND pj.dimension = ?3
               AND pj.content_hash = mc.body_hash
             ORDER BY pj.enqueued_at
             LIMIT ?4",
        )?;
        let rows = stmt
            .query_map(params![triple.provider, triple.model_ref, i64::from(triple.dimension), limit as i64], |row| {
                Ok(crate::model::PendingEmbeddingJob {
                    chunk_id: row.get::<_, String>(0)?,
                    text: row.get::<_, String>(1)?,
                    content_hash: crate::model::Sha256::new(row.get::<_, String>(2)?),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
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
    /// R-IX-1 defense-in-depth: the join against `memories` filters out
    /// encrypted-memory chunks (`metadata_only = 1`) and rows disabled for
    /// passive recall (`passive_recall = 0`), matching [`Self::query_chunks`]
    /// so both retrieval paths apply the identical row-exclusion contract.
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
               AND memories.passive_recall = 1
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

    /// Query recall-eligible chunks through BM25 and, optionally, a vector KNN
    /// lane, collapsed to one candidate per memory.
    ///
    /// This surface deliberately stops before fusion: it returns lane-local
    /// BM25 rank and vector cosine evidence so memoryd can perform RRF later.
    /// `limit` is lane-local because applying one final limit here would itself
    /// require a fusion policy.
    pub fn query_hybrid_chunks(
        &self,
        text: &str,
        vector_query: Option<HybridVectorQuery<'_>>,
        limit: usize,
    ) -> Result<Vec<HybridMemoryCandidate>, VectorError> {
        let bm25_hits = self.query_hybrid_bm25_memories(text, limit)?;
        let vector_hits = if let Some(query) = vector_query {
            self.query_hybrid_vector_memories(query.triple, query.vector, limit)?
        } else {
            Vec::new()
        };

        let mut candidates: BTreeMap<String, HybridMemoryCandidate> = BTreeMap::new();
        for hit in bm25_hits {
            candidates.insert(
                hit.memory_id.clone(),
                HybridMemoryCandidate {
                    memory_id: MemoryId::new(hit.memory_id),
                    text: hit.text,
                    score_breakdown: HybridScoreBreakdown { bm25_rank: Some(hit.rank), cosine_similarity: None },
                    recency_at: hit.recency_at,
                },
            );
        }

        for hit in vector_hits {
            candidates
                .entry(hit.memory_id.clone())
                .and_modify(|candidate| {
                    candidate.score_breakdown.cosine_similarity = Some(hit.cosine_similarity);
                    candidate.recency_at = later_recency_at(candidate.recency_at, hit.recency_at);
                })
                .or_insert_with(|| HybridMemoryCandidate {
                    memory_id: MemoryId::new(hit.memory_id),
                    text: hit.text,
                    score_breakdown: HybridScoreBreakdown {
                        bm25_rank: None,
                        cosine_similarity: Some(hit.cosine_similarity),
                    },
                    recency_at: hit.recency_at,
                });
        }

        let mut out: Vec<_> = candidates.into_values().collect();
        out.sort_by(compare_hybrid_candidates);
        Ok(out)
    }

    fn query_hybrid_bm25_memories(&self, text: &str, limit: usize) -> Result<Vec<Bm25MemoryRank>, VectorError> {
        let sanitized = sanitize_fts_query(text);
        if sanitized.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }

        let mut collapsed = collapse_bm25_memory_hits(self.query_hybrid_bm25_chunks(&sanitized, None)?);
        collapsed.truncate(limit);

        let strict_len = collapsed.len();
        let relaxed = sanitize_relaxed_fts_query(text);
        if collapsed.len() < limit && !relaxed.is_empty() && relaxed != sanitized {
            let relaxed_hits = self.query_hybrid_bm25_chunks_memory_collapsed(&relaxed, limit)?;
            let mut seen = collapsed.iter().map(|hit| hit.memory_id.clone()).collect::<BTreeSet<_>>();
            for hit in relaxed_hits {
                if collapsed.len() >= limit {
                    break;
                }
                if seen.insert(hit.memory_id.clone()) {
                    collapsed.push(hit);
                }
            }
        }

        Ok(collapsed
            .into_iter()
            .enumerate()
            .map(|(idx, hit)| {
                let rank = if idx < strict_len {
                    idx + 1
                } else {
                    let relaxed_position = idx - strict_len + 1;
                    strict_len + relaxed_position + RELAXED_RANK_OFFSET
                };
                Bm25MemoryRank { memory_id: hit.memory_id, text: hit.text, rank, recency_at: hit.recency_at }
            })
            .collect())
    }

    fn query_hybrid_bm25_chunks(
        &self,
        fts_query: &str,
        row_limit: Option<usize>,
    ) -> Result<Vec<Bm25ChunkHit>, VectorError> {
        const SQL: &str =
            "SELECT memory_chunks.memory_id, memory_chunks.text, memory_chunks.chunk_rowid, bm25(memory_chunks_fts) AS score,
                    memories.updated_at, memories.observed_at
             FROM memory_chunks_fts
             JOIN memory_chunks ON memory_chunks_fts.rowid = memory_chunks.chunk_rowid
             JOIN memories      ON memories.id = memory_chunks.memory_id
             WHERE memory_chunks_fts MATCH ?1
               AND memories.metadata_only = 0
               AND memories.passive_recall = 1
               AND memories.status IN ('active', 'pinned')
             ORDER BY score, memory_chunks.memory_id, memory_chunks.chunk_rowid";
        const SQL_LIMITED: &str =
            "SELECT memory_chunks.memory_id, memory_chunks.text, memory_chunks.chunk_rowid, bm25(memory_chunks_fts) AS score,
                    memories.updated_at, memories.observed_at
             FROM memory_chunks_fts
             JOIN memory_chunks ON memory_chunks_fts.rowid = memory_chunks.chunk_rowid
             JOIN memories      ON memories.id = memory_chunks.memory_id
             WHERE memory_chunks_fts MATCH ?1
               AND memories.metadata_only = 0
               AND memories.passive_recall = 1
               AND memories.status IN ('active', 'pinned')
             ORDER BY score, memory_chunks.memory_id, memory_chunks.chunk_rowid
             LIMIT ?2";

        if let Some(row_limit) = row_limit {
            let mut stmt = self.connection.prepare_cached(SQL_LIMITED)?;
            let rows = stmt
                .query_map(params![fts_query, row_limit as i64], bm25_chunk_hit_from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(Into::into);
            return rows;
        }

        let mut stmt = self.connection.prepare_cached(SQL)?;
        let rows = stmt
            .query_map([fts_query], bm25_chunk_hit_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into);
        rows
    }

    fn query_hybrid_bm25_chunks_memory_collapsed(
        &self,
        fts_query: &str,
        memory_limit: usize,
    ) -> Result<Vec<Bm25ChunkHit>, VectorError> {
        const SQL: &str = "SELECT memory_id, text, chunk_rowid, score, updated_at, observed_at FROM (
               SELECT memory_id, text, chunk_rowid, score, updated_at, observed_at,
                      ROW_NUMBER() OVER (PARTITION BY memory_id ORDER BY score, chunk_rowid) AS rn
               FROM (
                 SELECT memory_chunks.memory_id, memory_chunks.text, memory_chunks.chunk_rowid,
                        bm25(memory_chunks_fts) AS score, memories.updated_at, memories.observed_at
                 FROM memory_chunks_fts
                 JOIN memory_chunks ON memory_chunks_fts.rowid = memory_chunks.chunk_rowid
                 JOIN memories      ON memories.id = memory_chunks.memory_id
                 WHERE memory_chunks_fts MATCH ?1
                   AND memories.metadata_only = 0
                   AND memories.passive_recall = 1
                   AND memories.status IN ('active', 'pinned')
               )
             )
             WHERE rn = 1
             ORDER BY score, memory_id
             LIMIT ?2";

        let mut stmt = self.connection.prepare_cached(SQL)?;
        let rows = stmt
            .query_map(params![fts_query, memory_limit as i64], bm25_chunk_hit_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into);
        rows
    }

    fn query_hybrid_vector_memories(
        &self,
        triple: &EmbeddingTriple,
        vector: &[f32],
        limit: usize,
    ) -> Result<Vec<VectorMemoryScore>, VectorError> {
        crate::index::sqlite_vec::validate_dimension(triple, vector)?;
        let table = crate::index::sqlite_vec::vector_table_name(triple);
        if is_dropped_triple(&self.connection, triple)? || !table_exists(&self.connection, &table)? {
            return Err(VectorError::UnknownEmbeddingTriple(triple.clone()));
        }
        if limit == 0 {
            return Ok(Vec::new());
        }

        const CHUNK_FANOUT: usize = 8;
        let knn_k = limit.saturating_mul(CHUNK_FANOUT).clamp(limit, 512);
        let sql = format!(
            "SELECT memory_chunks.memory_id, memory_chunks.text, memory_chunks.chunk_rowid, {table}.distance,
                    memories.updated_at, memories.observed_at
             FROM {table}
             JOIN memory_chunks ON memory_chunks.chunk_rowid = {table}.rowid
             JOIN memories      ON memories.id = memory_chunks.memory_id
             WHERE embedding MATCH ?1
               AND k = ?2
               AND memories.metadata_only = 0
               AND memories.passive_recall = 1
               AND memories.status IN ('active', 'pinned')
             ORDER BY {table}.distance, memory_chunks.memory_id, memory_chunks.chunk_rowid"
        );
        let blob = crate::index::sqlite_vec::serialize_f32(vector);
        let mut stmt = self.connection.prepare_cached(&sql)?;
        let rows = stmt
            .query_map(params![blob, knn_k as i64], vector_chunk_hit_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let mut best_by_memory = BTreeMap::new();
        for hit in rows {
            let memory_id = hit.memory_id.clone();
            match best_by_memory.get(&memory_id) {
                Some(best) if !vector_chunk_precedes(&hit, best) => {}
                _ => {
                    best_by_memory.insert(memory_id, hit);
                }
            }
        }

        let mut collapsed: Vec<_> = best_by_memory.into_values().collect();
        collapsed.sort_by(|left, right| {
            left.distance.total_cmp(&right.distance).then_with(|| left.memory_id.cmp(&right.memory_id))
        });
        collapsed.truncate(limit);

        Ok(collapsed
            .into_iter()
            .map(|hit| VectorMemoryScore {
                memory_id: hit.memory_id,
                text: hit.text,
                cosine_similarity: cosine_from_l2_distance(hit.distance),
                recency_at: hit.recency_at,
            })
            .collect())
    }

    /// KNN over the active embedding triple's vector table, collapsed to one row
    /// per *memory* and restricted to active, non-encrypted, in-scope rows.
    ///
    /// This is the substrate seam for governance contradiction detection
    /// (Stream C `SimilaritySearch::top_k`): given a candidate's query vector,
    /// return the nearest active memories within the candidate's namespace scope
    /// so the engine can decide same/refine/contradict.
    ///
    /// ## Scope filtering
    ///
    /// `scopes` is the set of `memories.scope` values that share the candidate's
    /// governance namespace (`me` → `user`; `project` → `project`/`org`;
    /// `agent` → `agent`/`subagent`). An empty `scopes` slice returns no rows
    /// rather than every memory — a candidate with no resolvable scope has no
    /// in-scope neighbours by definition.
    ///
    /// ## Status / encryption filtering
    ///
    /// Only `status = 'active'` rows participate, and `metadata_only = 0`
    /// excludes encrypted-body memories (their bodies are never embedded). This
    /// intentionally differs from recall membership: rows with
    /// `passive_recall = 0` still participate because contradiction detection is
    /// write governance, not passive retrieval. Superseded, tombstoned,
    /// quarantined, and candidate rows must not trigger a contradiction against a
    /// write.
    ///
    /// ## Distance → similarity
    ///
    /// The `vec0` table stores L2 (euclidean) distance. For L2-normalized
    /// (unit) vectors — which both the production Qwen3 lane and the test
    /// fixture provider emit — cosine similarity is `1 - d²/2`. Governance
    /// thresholds are expressed as cosine similarity, so the conversion happens
    /// here. The unit-vector assumption is the contract: a provider that emits
    /// un-normalized vectors would skew the similarity, which is a provider bug,
    /// not silently absorbed here.
    ///
    /// ## Chunk → memory collapse
    ///
    /// A memory can produce several chunks, so the raw KNN can return multiple
    /// rows per memory. We over-fetch (`k` is widened past `limit`) and keep each
    /// memory's nearest chunk (`MIN(distance)`), then truncate to `limit`
    /// memories ordered by ascending distance. Over-fetching guards against the
    /// post-`MATCH` WHERE filters (scope/status) shrinking a `k`-row neighbour
    /// set below `limit` distinct memories.
    ///
    /// Per invariant 3 (spec §10.2.2): a triple whose vector table does not exist
    /// (or was dropped) is [`VectorError::UnknownEmbeddingTriple`], never a silent
    /// empty result — the caller distinguishes "no neighbours" from "no backend".
    #[allow(clippy::too_many_arguments)]
    pub fn knn_active_memories(
        &self,
        triple: &EmbeddingTriple,
        vector: &[f32],
        scopes: &[Scope],
        limit: usize,
    ) -> Result<Vec<crate::model::SimilarMemory>, VectorError> {
        crate::index::sqlite_vec::validate_dimension(triple, vector)?;
        if scopes.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let table = crate::index::sqlite_vec::vector_table_name(triple);
        if is_dropped_triple(&self.connection, triple)? || !table_exists(&self.connection, &table)? {
            return Err(VectorError::UnknownEmbeddingTriple(triple.clone()));
        }

        // Over-fetch chunk neighbours so the post-MATCH scope/status filters and
        // the chunk→memory collapse still leave at least `limit` distinct
        // memories in the common case. Capped so a huge `limit` cannot ask
        // sqlite-vec for an unbounded scan.
        const CHUNK_FANOUT: usize = 8;
        let knn_k = limit.saturating_mul(CHUNK_FANOUT).clamp(limit, 512);

        // sqlite-vec's KNN form is restrictive: the `embedding MATCH ? AND k = ?`
        // shape is kept simple (mirroring `query_vector_chunks` — plain WHERE
        // filters + ORDER BY, no GROUP BY/aggregate over the virtual table). The
        // chunk→memory collapse (nearest chunk per memory) happens in Rust below.
        let scope_placeholders = sql_placeholders(scopes.len());
        let sql = format!(
            "SELECT memory_chunks.memory_id, memories.scope, {table}.distance
             FROM {table}
             JOIN memory_chunks ON memory_chunks.chunk_rowid = {table}.rowid
             JOIN memories      ON memories.id = memory_chunks.memory_id
             WHERE embedding MATCH ?1
               AND k = ?2
               AND memories.status = 'active'
               AND memories.metadata_only = 0
               AND memories.scope IN ({scope_placeholders})
             ORDER BY {table}.distance"
        );

        let blob = crate::index::sqlite_vec::serialize_f32(vector);
        let mut bindings: Vec<rusqlite::types::Value> = Vec::with_capacity(scopes.len() + 2);
        bindings.push(rusqlite::types::Value::Blob(blob));
        bindings.push(rusqlite::types::Value::Integer(knn_k as i64));
        for scope in scopes {
            bindings.push(rusqlite::types::Value::Text(scope_str(*scope).to_string()));
        }

        let mut stmt = self.connection.prepare_cached(&sql)?;
        let rows = stmt
            .query_map(params_from_iter(bindings.iter()), |row| {
                let memory_id: String = row.get(0)?;
                let scope_text: String = row.get(1)?;
                let distance: f64 = row.get(2)?;
                Ok((memory_id, scope_text, distance))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        // Collapse chunk hits to one row per memory (rows are ascending by
        // distance, so the first hit for each memory is its nearest chunk), then
        // truncate to `limit` memories preserving distance order.
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut out = Vec::with_capacity(limit.min(rows.len()));
        for (memory_id, scope_text, distance) in rows {
            if out.len() >= limit {
                break;
            }
            if !seen.insert(memory_id.clone()) {
                continue;
            }
            let scope = scope_from_str(&scope_text)?;
            out.push(crate::model::SimilarMemory {
                memory_id: MemoryId::new(memory_id),
                scope,
                similarity: cosine_from_l2_distance(distance),
            });
        }
        Ok(out)
    }

    /// Return the stored `file_hash` for a repo path, or `None` if not indexed.
    ///
    /// Used by phase 6 index-consistency check to avoid a full reindex on every
    /// startup. If the stored hash equals the on-disk hash, the memory is clean.
    pub fn file_hash_for(&self, path: &RepoPath) -> Option<crate::model::Sha256> {
        match self
            .connection
            .query_row("SELECT file_hash FROM memories WHERE path = ?1", [path.as_str()], |row| row.get::<_, String>(0))
        {
            Ok(hash) => Some(crate::model::Sha256::new(hash)),
            Err(rusqlite::Error::QueryReturnedNoRows) => None,
            Err(error) => {
                tracing::warn!(path = path.as_str(), %error, "index file-hash lookup failed; forcing safe reindex");
                None
            }
        }
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

    /// Project the entities (with aliases) attached to a set of memory ids in a
    /// single batched query against `memory_entities`/`memory_entity_aliases`.
    ///
    /// Stream I uses this for claim-lock entity-intersection checks without
    /// re-reading canonical files. Returns a map keyed by memory id; ids absent
    /// from the index are simply omitted.
    pub fn entities_for_memories(&self, ids: &[String]) -> SubstrateResult<BTreeMap<String, Vec<Entity>>> {
        if ids.is_empty() {
            return Ok(BTreeMap::new());
        }
        read_entities_by_memory(&self.connection, ids).map_err(Into::into)
    }

    /// Count memories grouped by lifecycle status in a single index-only scan.
    ///
    /// Replaces N separate `query_memory(status=…)` calls that each materialized
    /// every matching row only to discard all but `rows.len()`. Counts every
    /// memory regardless of `metadata_only` (matching the prior callers, which
    /// passed `include_metadata_only: true`). One pair per distinct status.
    pub fn count_by_status(&self) -> SubstrateResult<Vec<(MemoryStatus, u64)>> {
        let mut stmt = self.connection.prepare_cached("SELECT status, COUNT(*) FROM memories GROUP BY status")?;
        let mut rows = stmt.query([])?;
        let mut counts = Vec::new();
        while let Some(row) = rows.next()? {
            let status_text: String = row.get(0)?;
            let count: i64 = row.get(1)?;
            counts.push((memory_status_from_str(&status_text)?, count.max(0) as u64));
        }
        Ok(counts)
    }

    /// Recent `recall_hit` events joined to their memory summaries, newest-first.
    ///
    /// `since` is applied with a dynamically-appended `AND e.ts > ?` (not the
    /// index-defeating `(? IS NULL OR …)` form) so the `kind = ? AND ts > ?
    /// ORDER BY ts DESC` shape rides `idx_events_log_kind_ts` as an ordered range
    /// seek. Each tuple is `(event_id, device, seq, memory_id, recalled_at, summary)`.
    #[allow(clippy::type_complexity)]
    pub fn recent_recall_hits(
        &self,
        since: Option<DateTime<Utc>>,
        limit: usize,
    ) -> SubstrateResult<Vec<(String, String, i64, String, String, Option<String>)>> {
        let mut sql = String::from(
            "SELECT e.event_id, e.device, e.seq, e.memory_id, e.ts, m.summary
             FROM events_log e
             LEFT JOIN memories m ON m.id = e.memory_id
             WHERE e.kind = 'recall_hit'",
        );
        let mut bindings: Vec<rusqlite::types::Value> = Vec::new();
        if let Some(since) = since {
            sql.push_str(" AND e.ts > ?");
            bindings.push(rusqlite::types::Value::Text(since.to_rfc3339()));
        }
        sql.push_str(" ORDER BY e.ts DESC, e.event_id DESC LIMIT ?");
        bindings.push(rusqlite::types::Value::Integer(limit as i64));

        let mut stmt = self.connection.prepare_cached(&sql)?;
        let mut rows = stmt.query(params_from_iter(bindings.iter()))?;
        let mut hits = Vec::new();
        while let Some(row) = rows.next()? {
            hits.push((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
            ));
        }
        Ok(hits)
    }

    /// Stream every indexed entity (with its aliases) as `(memory_id, Entity)`
    /// pairs, ordered by `memory_id` then `entity_id`.
    ///
    /// Reads only `memory_entities`/`memory_entity_aliases` — no `memories`
    /// scan, no `json_extract`, no recall-index hydration — for entity-graph
    /// aggregation that needs nothing from the memory envelope. The ordering
    /// matches the per-memory hydration the recall index used, so callers that
    /// aggregate in id order are unaffected.
    pub fn entity_index_rows(&self) -> SubstrateResult<Vec<(MemoryId, Entity)>> {
        read_all_entity_rows(&self.connection).map_err(Into::into)
    }

    /// Count memories grouped by `(scope, canonical_namespace_id)` in a single
    /// index-only scan, for namespace-tree aggregation.
    ///
    /// Counts every memory regardless of `metadata_only` (matching the prior
    /// `query_recall_index_including_metadata_only(default)` caller) but skips
    /// all entity/tag/alias hydration, which the namespace tree never reads.
    pub fn namespace_counts(&self) -> SubstrateResult<Vec<(Scope, Option<String>, u64)>> {
        let mut stmt = self.connection.prepare_cached(
            "SELECT scope, canonical_namespace_id, COUNT(*)
             FROM memories
             GROUP BY scope, canonical_namespace_id",
        )?;
        let mut rows = stmt.query([])?;
        let mut counts = Vec::new();
        while let Some(row) = rows.next()? {
            let scope_text: String = row.get(0)?;
            let canonical_namespace_id: Option<String> = row.get(1)?;
            let count: i64 = row.get(2)?;
            counts.push((scope_from_str(&scope_text)?, canonical_namespace_id, count.max(0) as u64));
        }
        Ok(counts)
    }

    /// Read a bounded, kind-filtered page of mirror events (newest-first); see
    /// [`crate::index::EventsLogPage`] for the device/cursor/limit parameters.
    pub fn events_log_page(
        &self,
        page: &crate::index::EventsLogPage,
    ) -> SubstrateResult<Vec<crate::index::MirrorEvent>> {
        crate::index::events_read::query_events_log_page(&self.connection, page).map_err(Into::into)
    }

    /// Read mirror events within a time window, optionally kind-restricted and/or
    /// scoped to one authoring `device`.
    pub fn events_log_window(
        &self,
        kind_labels: Option<&[&str]>,
        device: Option<&str>,
        since: DateTime<Utc>,
    ) -> SubstrateResult<Vec<crate::index::MirrorEvent>> {
        crate::index::events_read::query_events_log_window(&self.connection, kind_labels, device, since)
            .map_err(Into::into)
    }

    /// Most recent mirror-event timestamp for a given kind label.
    pub fn latest_event_ts_for_kind(&self, kind_label: &str) -> SubstrateResult<Option<DateTime<Utc>>> {
        crate::index::events_read::latest_ts_for_kind(&self.connection, kind_label).map_err(Into::into)
    }

    /// Timestamp of a single mirror event looked up by canonical event id.
    pub fn event_ts_by_id(&self, event_id: &str) -> SubstrateResult<Option<DateTime<Utc>>> {
        crate::index::events_read::ts_for_event_id(&self.connection, event_id).map_err(Into::into)
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
                    memories.human_review_required,memories.max_scope,
                    json_extract(memories.frontmatter_json, '$.source.harness'),
                    json_extract(memories.frontmatter_json, '$.source.session_id'),
                    json_extract(memories.frontmatter_json, '$.author.harness'),
                    json_extract(memories.frontmatter_json, '$.author.session_id'),
                    json_extract(memories.frontmatter_json, '$._merge_diagnostics')
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
        hydrate_recall_index_auxiliary(&self.connection, &mut results, query.hydrate)?;
        Ok(results)
    }

    /// Count recall-index rows matching `query` via an index-only `COUNT(*)`,
    /// without marshalling rows or hydrating auxiliary tables.
    ///
    /// Shares `append_recall_index_filters`/`append_match_term_filters` with
    /// [`Self::query_recall_index`] so the predicate (and therefore which rows
    /// are counted) is identical to fetching and calling `rows.len()` on the
    /// result. `query.hydrate` is irrelevant to a count and is ignored.
    pub fn count_recall_index(&self, query: &RecallIndexQuery) -> SubstrateResult<usize> {
        self.count_recall_index_inner(query, false)
    }

    fn count_recall_index_inner(
        &self,
        query: &RecallIndexQuery,
        include_metadata_only: bool,
    ) -> SubstrateResult<usize> {
        let mut sql = String::from("SELECT COUNT(*) FROM memories");
        let mut filters = Vec::new();
        let mut bindings = Vec::new();
        append_recall_index_filters(query, include_metadata_only, &mut filters, &mut bindings)?;
        append_match_term_filters(query, &mut filters, &mut bindings);
        if !filters.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&filters.join(" AND "));
        }
        let mut stmt = self.connection.prepare_cached(&sql)?;
        let count: i64 = stmt.query_row(params_from_iter(bindings.iter()), |row| row.get(0))?;
        Ok(count.max(0) as usize)
    }

    /// Serve the review queue from the derived index instead of walking and
    /// re-parsing every canonical memory file (the prior O(total) disk+parse
    /// hot path on a repeatedly-polled inbox surface).
    ///
    /// The membership predicate rides `idx_memories_review(review_state,
    /// requires_user_confirmation)` and is byte-for-byte the SQL equivalent of
    /// `ReviewStatus::from_review_metadata`: a row qualifies when its status is
    /// `quarantined`, when it is a `candidate` requiring user confirmation, or
    /// when its `review_state` is one of the pending spellings. `total` counts
    /// every qualifying row (for the over-threshold notification) while `rows`
    /// is bounded by `limit` and ordered by the stable canonical `memories.id`
    /// key so callers hydrate only what the response renders. Ordering by id
    /// (rather than `updated_at`) keeps the bounded page a deterministic prefix
    /// that does not reshuffle as memories are touched: the prior full-walk path
    /// this replaces collected qualifying rows in filesystem-walk order and
    /// truncated, so a fixed oldest-first-ish prefix — not a newest-first window
    /// that can starve persistently-pending items off the page — is the
    /// behavior-preserving choice. `policy_applied` and
    /// `governance_reason` are projected from `frontmatter_json` via
    /// `json_extract`, matching `RecallIndexRow::merge_diagnostics_json`.
    pub fn review_queue(&self, limit: usize) -> SubstrateResult<ReviewQueuePage> {
        let total: i64 = self
            .connection
            .prepare_cached(&format!("SELECT COUNT(*) FROM memories WHERE {REVIEW_QUEUE_PREDICATE}"))?
            .query_row([], |row| row.get(0))?;

        let mut stmt = self.connection.prepare_cached(&format!(
            "SELECT memories.id, memories.summary, memories.status,
                    memories.requires_user_confirmation, memories.review_state,
                    json_extract(memories.frontmatter_json, '$.write_policy.policy_applied'),
                    json_extract(memories.frontmatter_json, '$.governance_reason')
             FROM memories
             WHERE {REVIEW_QUEUE_PREDICATE}
             ORDER BY memories.id
             LIMIT ?1"
        ))?;
        let mut query_rows = stmt.query(params![limit as i64])?;
        let mut rows = Vec::new();
        while let Some(row) = query_rows.next()? {
            rows.push(ReviewQueueRow {
                id: row.get(0)?,
                summary: row.get(1)?,
                status: row.get(2)?,
                requires_user_confirmation: row.get::<_, i64>(3)? != 0,
                review_state: row.get(4)?,
                policy_applied: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
                governance_reason: row.get(6)?,
            });
        }
        Ok(ReviewQueuePage { total: total.max(0) as usize, rows })
    }
}

/// SQL membership predicate mirroring `ReviewStatus::from_review_metadata`.
///
/// Kept as a single shared constant so the `COUNT(*)` total and the bounded row
/// fetch can never drift apart on which memories qualify for the queue.
const REVIEW_QUEUE_PREDICATE: &str = "memories.status = 'quarantined' \
     OR (memories.status = 'candidate' AND memories.requires_user_confirmation = 1) \
     OR memories.review_state IN ('pending', 'pending_review', 'pending-review')";

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

/// Largest chunk size for the `event_id IN (...)` presence scan below.
///
/// Bounds memory and scan cost to the input (canonical-event) size rather than
/// the whole `events_log`. Equal to the largest [`super::IN_CLAUSE_BUCKETS`]
/// width so a full chunk maps to the widest cached `IN (...)` plan; partial tail
/// chunks pad up to a bucket via [`pad_in_clause_bindings`] so they reuse a
/// cached plan too instead of minting one per distinct tail size.
const MIRROR_HEALTH_PRESENCE_CHUNK: usize = 256;

fn count_missing_events_log_rows(connection: &Connection, canonical_events: &[Event]) -> rusqlite::Result<u64> {
    if canonical_events.is_empty() {
        return Ok(0);
    }
    // Probe which canonical ids exist in `events_log` via chunked
    // `event_id IN (...)` scans riding the PK index, then apply the same
    // per-event "absent from the mirror" membership test the prior full-column
    // scan used. Memory and scan cost track the canonical-event count, not the
    // entire (unbounded, lifetime-growing) `events_log`. Each chunk's placeholder
    // width is bucketed (and its bindings padded) via the shared helpers, so even
    // the final partial chunk reuses one of a handful of cached `IN (...)` plans.
    // Using a presence set (rather than summing match counts) keeps the result
    // identical even if the canonical list ever repeated an id.
    let mut present = std::collections::HashSet::with_capacity(canonical_events.len());
    for chunk in canonical_events.chunks(MIRROR_HEALTH_PRESENCE_CHUNK) {
        let ids: Vec<String> = chunk.iter().map(|event| event.id.as_str().to_owned()).collect();
        let width = bucketed_in_clause_width(ids.len());
        let sql = format!("SELECT event_id FROM events_log WHERE event_id IN ({})", sql_placeholders(width));
        let mut stmt = connection.prepare_cached(&sql)?;
        let mut rows = stmt.query(params_from_iter(pad_in_clause_bindings(&ids, width)))?;
        while let Some(row) = rows.next()? {
            present.insert(row.get::<_, String>(0)?);
        }
    }
    let missing = canonical_events.iter().filter(|event| !present.contains(event.id.as_str())).count();
    Ok(missing as u64)
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
    let vector_json = serde_json::to_string(vector)?;
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
            vector_table.as_ref(),
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
/// `file_hash` is the exact on-disk hash when the caller already has it; the
/// body hash fallback preserves fixture call sites that do not touch disk.
/// `file_mtime_ns` is still 0 until the write path plumbs real metadata.
struct MemoryUpsertOptions<'a> {
    metadata_only: bool,
    file_hash: Option<&'a Sha256>,
    active_embedding: &'a EmbeddingTriple,
}

fn upsert_memory_row_with_full_metadata(
    connection: &mut Connection,
    memory: &Memory,
    options: MemoryUpsertOptions<'_>,
) -> rusqlite::Result<()> {
    let active_embedding_dropped = is_dropped_triple_rusqlite(connection, options.active_embedding)?;
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
    let file_hash = options.file_hash.map_or_else(|| body_hash.clone(), ToString::to_string);
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
            ":metadata_only":             options.metadata_only as i64,
            ":passive_recall":            passive_recall,
            ":index_body":                index_body,
            ":human_review_required":     human_review_required,
            ":max_scope":                 max_scope,
        },
    )?;

    sync_auxiliary_tables(&txn, memory)?;

    // Rebuild chunks for this memory.
    txn.execute("DELETE FROM memory_chunks WHERE memory_id = ?1", [memory.frontmatter.id.as_str()])?;
    if !options.metadata_only && memory.frontmatter.retrieval_policy.index_body {
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
                        options.active_embedding.provider.as_str(),
                        options.active_embedding.model_ref.as_str(),
                        i64::from(options.active_embedding.dimension),
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
        // FK guard, parity with the v4 migration's supersession bootstrap
        // (`migrations.rs`): the edge is inserted only when its target's
        // `memories` row already exists. `memory_supersession.supersedes_id`
        // is a `REFERENCES memories(id)` FK with `PRAGMA foreign_keys = ON`,
        // so an unguarded insert against a not-yet-indexed target trips the
        // constraint and — during a *bulk* reindex that walks files in
        // unsorted order — aborts the whole reconcile. Bulk callers keep this
        // skip-then-resync behavior. Runtime write callers audit the skipped
        // target set after upsert and enqueue durable §8.3 repair state instead
        // of silently waiting for a restart.
        txn.execute(
            "INSERT OR IGNORE INTO memory_supersession(memory_id, supersedes_id)
             SELECT ?1, ?2 WHERE EXISTS (SELECT 1 FROM memories WHERE id = ?2)",
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
    is_dropped_triple_rusqlite(conn, triple).map_err(Into::into)
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
        let placeholders = sql_placeholders(query.statuses.len());
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
        source_harness: row.get(18)?,
        source_session_id: row.get(19)?,
        author_harness: row.get(20)?,
        author_session_id: row.get(21)?,
        merge_diagnostics_json: row.get(22)?,
        tags: Vec::new(),
        aliases: Vec::new(),
        entities: Vec::new(),
    })
}

fn hydrate_recall_index_auxiliary(
    conn: &Connection,
    rows: &mut [RecallIndexRow],
    scope: AuxScope,
) -> rusqlite::Result<()> {
    if rows.is_empty() || scope == AuxScope::None {
        return Ok(());
    }

    let ids = rows.iter().map(|row| row.id.as_str().to_owned()).collect::<Vec<_>>();

    // Tags are needed by `All` and `Tags`; aliases/entities only by `All`/`Entities`.
    let want_tags = matches!(scope, AuxScope::All | AuxScope::Tags);
    let want_aliases = scope == AuxScope::All;
    let want_entities = matches!(scope, AuxScope::All | AuxScope::Entities);

    let mut tags_by_memory = if want_tags {
        read_strings_by_memory(
            conn,
            AuxiliaryStringTable {
                table: "memory_tags",
                column: "tag",
                order_by: "ORDER BY memory_id, tag COLLATE NOCASE, tag",
            },
            &ids,
        )?
    } else {
        BTreeMap::new()
    };
    let mut aliases_by_memory = if want_aliases {
        read_strings_by_memory(
            conn,
            AuxiliaryStringTable {
                table: "memory_aliases",
                column: "alias",
                order_by: "ORDER BY memory_id, alias COLLATE NOCASE, alias",
            },
            &ids,
        )?
    } else {
        BTreeMap::new()
    };
    let mut entities_by_memory = if want_entities { read_entities_by_memory(conn, &ids)? } else { BTreeMap::new() };

    for row in rows {
        if want_tags {
            row.tags = tags_by_memory.remove(row.id.as_str()).unwrap_or_default();
        }
        if want_aliases {
            row.aliases = aliases_by_memory.remove(row.id.as_str()).unwrap_or_default();
        }
        if want_entities {
            row.entities = entities_by_memory.remove(row.id.as_str()).unwrap_or_default();
        }
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
    let width = bucketed_in_clause_width(ids.len());
    let placeholders = sql_placeholders(width);
    let sql = format!(
        "SELECT memory_id,{} FROM {} WHERE memory_id IN ({placeholders}) {}",
        table.column, table.table, table.order_by
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let mut rows = stmt.query(params_from_iter(pad_in_clause_bindings(ids, width)))?;
    let mut values = BTreeMap::<String, Vec<String>>::new();
    while let Some(row) = rows.next()? {
        values.entry(row.get::<_, String>(0)?).or_default().push(row.get(1)?);
    }
    Ok(values)
}

fn read_entities_by_memory(conn: &Connection, ids: &[String]) -> rusqlite::Result<BTreeMap<String, Vec<Entity>>> {
    let width = bucketed_in_clause_width(ids.len());
    let placeholders = sql_placeholders(width);
    let sql = format!(
        "SELECT memory_id,entity_id,label FROM memory_entities
         WHERE memory_id IN ({placeholders})
         ORDER BY memory_id, entity_id COLLATE NOCASE, entity_id"
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let mut rows = stmt.query(params_from_iter(pad_in_clause_bindings(ids, width)))?;
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

/// Read every indexed entity (with aliases) as ordered `(memory_id, Entity)`
/// pairs. Unfiltered sibling of [`read_entities_by_memory`]; reads only the two
/// entity tables.
fn read_all_entity_rows(conn: &Connection) -> rusqlite::Result<Vec<(MemoryId, Entity)>> {
    let aliases_by_entity = read_all_entity_aliases(conn)?;
    let mut stmt = conn.prepare_cached(
        "SELECT memory_id,entity_id,label FROM memory_entities
         ORDER BY memory_id, entity_id COLLATE NOCASE, entity_id",
    )?;
    let mut rows = stmt.query([])?;
    let mut entities = Vec::new();
    while let Some(row) = rows.next()? {
        let memory_id = row.get::<_, String>(0)?;
        let entity_id = row.get::<_, String>(1)?;
        let label = row.get::<_, String>(2)?;
        let aliases = aliases_by_entity.get(&(memory_id.clone(), entity_id.clone())).cloned().unwrap_or_default();
        entities.push((MemoryId::new(memory_id), Entity { id: entity_id, label, aliases }));
    }
    Ok(entities)
}

fn read_all_entity_aliases(conn: &Connection) -> rusqlite::Result<BTreeMap<(String, String), Vec<String>>> {
    let mut stmt = conn.prepare_cached(
        "SELECT memory_id,entity_id,alias FROM memory_entity_aliases
         ORDER BY memory_id, entity_id COLLATE NOCASE, entity_id, alias COLLATE NOCASE, alias",
    )?;
    let mut rows = stmt.query([])?;
    let mut aliases = BTreeMap::<(String, String), Vec<String>>::new();
    while let Some(row) = rows.next()? {
        aliases.entry((row.get(0)?, row.get(1)?)).or_default().push(row.get(2)?);
    }
    Ok(aliases)
}

fn read_entity_aliases_by_memory(
    conn: &Connection,
    ids: &[String],
) -> rusqlite::Result<BTreeMap<(String, String), Vec<String>>> {
    let width = bucketed_in_clause_width(ids.len());
    let placeholders = sql_placeholders(width);
    let sql = format!(
        "SELECT memory_id,entity_id,alias FROM memory_entity_aliases
         WHERE memory_id IN ({placeholders})
         ORDER BY memory_id, entity_id COLLATE NOCASE, entity_id, alias COLLATE NOCASE, alias"
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let mut rows = stmt.query(params_from_iter(pad_in_clause_bindings(ids, width)))?;
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

struct Bm25ChunkHit {
    memory_id: String,
    text: String,
    chunk_rowid: i64,
    score: f64,
    recency_at: Option<DateTime<Utc>>,
}

struct Bm25MemoryRank {
    memory_id: String,
    text: String,
    rank: usize,
    recency_at: Option<DateTime<Utc>>,
}

struct VectorChunkHit {
    memory_id: String,
    text: String,
    chunk_rowid: i64,
    distance: f64,
    recency_at: Option<DateTime<Utc>>,
}

struct VectorMemoryScore {
    memory_id: String,
    text: String,
    cosine_similarity: f32,
    recency_at: Option<DateTime<Utc>>,
}

fn bm25_chunk_hit_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Bm25ChunkHit> {
    let updated_at: String = row.get(4)?;
    let observed_at: Option<String> = row.get(5)?;
    Ok(Bm25ChunkHit {
        memory_id: row.get(0)?,
        text: row.get(1)?,
        chunk_rowid: row.get(2)?,
        score: row.get(3)?,
        recency_at: memory_recency_at(&updated_at, observed_at.as_deref()),
    })
}

fn vector_chunk_hit_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<VectorChunkHit> {
    let updated_at: String = row.get(4)?;
    let observed_at: Option<String> = row.get(5)?;
    Ok(VectorChunkHit {
        memory_id: row.get(0)?,
        text: row.get(1)?,
        chunk_rowid: row.get(2)?,
        distance: row.get(3)?,
        recency_at: memory_recency_at(&updated_at, observed_at.as_deref()),
    })
}

fn memory_recency_at(updated_at: &str, observed_at: Option<&str>) -> Option<DateTime<Utc>> {
    let updated = parse_index_time(updated_at).ok()?;
    let observed = observed_at.and_then(|value| parse_index_time(value).ok());
    Some(match observed {
        Some(observed) if observed > updated => observed,
        _ => updated,
    })
}

fn later_recency_at(left: Option<DateTime<Utc>>, right: Option<DateTime<Utc>>) -> Option<DateTime<Utc>> {
    match (left, right) {
        (Some(left), Some(right)) => Some(if left > right { left } else { right }),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

fn collapse_bm25_memory_hits(rows: Vec<Bm25ChunkHit>) -> Vec<Bm25ChunkHit> {
    let mut best_by_memory = BTreeMap::new();
    for hit in rows {
        let memory_id = hit.memory_id.clone();
        match best_by_memory.get(&memory_id) {
            Some(best) if !bm25_chunk_precedes(&hit, best) => {}
            _ => {
                best_by_memory.insert(memory_id, hit);
            }
        }
    }

    let mut collapsed: Vec<_> = best_by_memory.into_values().collect();
    collapsed
        .sort_by(|left, right| left.score.total_cmp(&right.score).then_with(|| left.memory_id.cmp(&right.memory_id)));
    collapsed
}

fn bm25_chunk_precedes(left: &Bm25ChunkHit, right: &Bm25ChunkHit) -> bool {
    left.score.total_cmp(&right.score).then_with(|| left.chunk_rowid.cmp(&right.chunk_rowid)).is_lt()
}

fn vector_chunk_precedes(left: &VectorChunkHit, right: &VectorChunkHit) -> bool {
    left.distance.total_cmp(&right.distance).then_with(|| left.chunk_rowid.cmp(&right.chunk_rowid)).is_lt()
}

fn compare_hybrid_candidates(left: &HybridMemoryCandidate, right: &HybridMemoryCandidate) -> std::cmp::Ordering {
    compare_optional_rank(left.score_breakdown.bm25_rank, right.score_breakdown.bm25_rank)
        .then_with(|| {
            compare_optional_similarity(left.score_breakdown.cosine_similarity, right.score_breakdown.cosine_similarity)
        })
        .then_with(|| left.memory_id.as_str().cmp(right.memory_id.as_str()))
}

fn compare_optional_rank(left: Option<usize>, right: Option<usize>) -> std::cmp::Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left.cmp(&right),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

fn compare_optional_similarity(left: Option<f32>, right: Option<f32>) -> std::cmp::Ordering {
    match (left, right) {
        (Some(left), Some(right)) => right.total_cmp(&left),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

/// Convert a `vec0` L2 (euclidean) distance into cosine similarity, assuming the
/// stored and query vectors are L2-normalized (unit length).
///
/// For unit vectors `a`, `b`: `‖a − b‖² = 2 − 2·(a·b)`, so the cosine
/// similarity `a·b = 1 − d²/2`. Both the production Qwen3 lane and the test
/// fixture provider emit normalized vectors, so this is exact in practice; for
/// any residual numeric drift the result is clamped to the valid cosine range
/// `[-1, 1]`. A provider that emits un-normalized vectors would skew this — that
/// is a provider bug surfaced as off-distribution similarities, never silently
/// corrected here.
fn cosine_from_l2_distance(distance: f64) -> f32 {
    (1.0 - (distance * distance) / 2.0).clamp(-1.0, 1.0) as f32
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

const RELAXED_FTS_MAX_TERMS: usize = 8;

/// Rank penalty applied to relaxed OR-fallback hits before RRF fusion.
///
/// Strict AND hits keep contiguous ranks `1..=S`. Relaxed-only hits receive
/// `S + i + RELAXED_RANK_OFFSET` (i = 1-based position among appended relaxed
/// hits), demoting OR-matches to tie-breakers of last resort because their BM25
/// scores come from a different query expression and are not rank-comparable
/// with strict AND hits.
///
/// 15 was chosen by a deterministic sweep on the recall-quality corpus
/// (2026-06-12, offsets 0/15/30/60): 15 was the only value that beat the
/// undiscounted behavior on nDCG@5 (0.7776 vs 0.7754) while recall@5 gave back
/// only 0.003 — heavier discounts (30, 60) lost real answers the OR fallback
/// was legitimately surfacing, not just noise (trap rate was flat across the
/// whole sweep).
const RELAXED_RANK_OFFSET: usize = 15;

/// Build a bounded OR query for the hybrid BM25 lane's fallback pass.
///
/// The primary BM25 pass remains strict (`term term term`, implicit AND). This
/// relaxed expression only fills unused lane slots, so exact all-term matches
/// keep better BM25 ranks while memories sharing distinctive query anchors can
/// still corroborate the vector lane.
///
/// Short tokens (1–3 alphanumeric characters) are kept only when they look
/// like identifiers (digits, all-caps acronyms, or mixed alnum); lone letters and
/// short lowercase filler are dropped. Longer tokens still pass through the
/// low-signal stopword filter.
fn sanitize_relaxed_fts_query(input: &str) -> String {
    input
        .split_whitespace()
        .filter_map(relaxed_fts_token)
        .take(RELAXED_FTS_MAX_TERMS)
        .map(|token| format!("\"{}\"", token.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" OR ")
}

/// Retain a whitespace token for the relaxed OR fallback when it survives
/// identifier-aware short-token filtering and the low-signal stopword list.
///
/// Tokens with fewer than four alphanumeric characters are kept only when they
/// look like recall anchors: they contain a digit, are an all-uppercase acronym
/// (two or more letters), or mix letters and digits. Lone letters and short
/// lowercase pure-alpha filler are dropped.
fn relaxed_fts_token(token: &str) -> Option<&str> {
    let trimmed = token.trim_matches(|character: char| !character.is_alphanumeric());
    if trimmed.is_empty() {
        return None;
    }

    let alnum_count = trimmed.chars().filter(|character| character.is_alphanumeric()).count();
    if alnum_count < 4 && !should_keep_short_identifier(trimmed) {
        return None;
    }

    let lower = trimmed.to_ascii_lowercase();
    if is_low_signal_query_term(&lower) {
        return None;
    }

    Some(trimmed)
}

fn should_keep_short_identifier(trimmed: &str) -> bool {
    let alnum = trimmed.chars().filter(|character| character.is_alphanumeric()).collect::<Vec<_>>();
    let count = alnum.len();
    if count == 0 {
        return false;
    }

    // Lone letters (any case) are noise; lone digits are anchors.
    if count == 1 {
        return alnum[0].is_ascii_digit();
    }

    let has_digit = alnum.iter().any(|character| character.is_ascii_digit());
    if has_digit {
        return true;
    }

    if count >= 2 && alnum.iter().all(|character| character.is_ascii_uppercase()) {
        return true;
    }

    false
}

fn is_low_signal_query_term(term: &str) -> bool {
    matches!(
        term,
        "about"
            | "after"
            | "again"
            | "also"
            | "before"
            | "being"
            | "could"
            | "does"
            | "doing"
            | "from"
            | "have"
            | "into"
            | "memory"
            | "memories"
            | "should"
            | "that"
            | "their"
            | "there"
            | "these"
            | "this"
            | "those"
            | "user"
            | "what"
            | "when"
            | "where"
            | "which"
            | "with"
            | "would"
            | "your"
    )
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::{relaxed_fts_token, sanitize_fts_query, sanitize_relaxed_fts_query, Index};
    use crate::index::{chunk_memory, open_index};
    use crate::model::{
        Author, AuthorKind, Frontmatter, Memory, MemoryId, MemoryStatus, MemoryType, RepoPath, RetrievalPolicy, Scope,
        Sensitivity, Source, SourceKind, TrustLevel, WritePolicy,
    };

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

    #[test]
    fn relaxed_sanitize_ors_distinctive_terms_for_fallback() {
        assert_eq!(
            sanitize_relaxed_fts_query("what language preference should the user use"),
            "\"language\" OR \"preference\""
        );
    }

    #[test]
    fn relaxed_sanitize_bounds_terms_and_keeps_fts_escaping() {
        assert_eq!(
            sanitize_relaxed_fts_query("alpha beta gamma delta epsilon zeta eta theta iota kappa say\"hi"),
            "\"alpha\" OR \"beta\" OR \"gamma\" OR \"delta\" OR \"epsilon\" OR \"zeta\" OR \"theta\" OR \"iota\""
        );
    }

    #[test]
    fn relaxed_token_keeps_short_identifier_anchors() {
        assert_eq!(relaxed_fts_token("v2"), Some("v2"));
        assert_eq!(relaxed_fts_token("PR"), Some("PR"));
        // `trim_matches` strips only leading/trailing non-alnum; interior hyphens stay.
        assert_eq!(relaxed_fts_token("B-7"), Some("B-7"));
        assert_eq!(relaxed_fts_token("7"), Some("7"));
        assert_eq!(relaxed_fts_token("Rust"), Some("Rust"));
    }

    #[test]
    fn relaxed_token_drops_short_low_signal_filler() {
        assert_eq!(relaxed_fts_token("at"), None);
        assert_eq!(relaxed_fts_token("a"), None);
        assert_eq!(relaxed_fts_token("I"), None);
    }

    #[test]
    fn relaxed_sanitize_keeps_identifier_tokens_in_or_fallback() {
        assert_eq!(sanitize_relaxed_fts_query("what is the PR for v2 B-7"), "\"PR\" OR \"v2\" OR \"B-7\"");
    }

    #[test]
    fn relaxed_bm25_fallback_limits_distinct_memories_not_chunks() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let mut index = Index::new(open_index(&temp.path().join("index.sqlite"))?);

        let mut multi_chunk = sample_memory("mem_20260612_a1b2c3d4e5f60718_010100");
        multi_chunk.body = (0..15_000).map(|index| format!("relaxedanchor token{index}")).collect::<Vec<_>>().join(" ");
        let multi_chunks = chunk_memory(&multi_chunk);
        assert!(
            multi_chunks.len() > 32,
            "fixture must exceed the old chunk-row cap for limit=4, got {}",
            multi_chunks.len()
        );

        let satellite_terms = ["bravoextra", "charlieextra", "deltaextra", "echoextra", "foxtrotextra"];
        let satellite_ids = [
            "mem_20260612_a1b2c3d4e5f60718_010101",
            "mem_20260612_a1b2c3d4e5f60718_010102",
            "mem_20260612_a1b2c3d4e5f60718_010103",
            "mem_20260612_a1b2c3d4e5f60718_010104",
            "mem_20260612_a1b2c3d4e5f60718_010105",
        ];
        let mut satellites = Vec::new();
        for (id, term) in satellite_ids.into_iter().zip(satellite_terms) {
            let mut memory = sample_memory(id);
            memory.body = format!("relaxedanchor {term} satellite body");
            satellites.push(memory);
        }

        index.upsert_memory(&multi_chunk, false)?;
        for memory in &satellites {
            index.upsert_memory(memory, false)?;
        }

        let limit = 4;
        let hits = index.query_hybrid_bm25_memories(
            "relaxedanchor bravoextra charlieextra deltaextra echoextra foxtrotextra golfextra",
            limit,
        )?;

        assert_eq!(hits.len(), limit, "relaxed fallback should fill the lane with distinct memories");
        let memory_ids: Vec<_> = hits.iter().map(|hit| hit.memory_id.as_str()).collect();
        assert_eq!(
            memory_ids.iter().collect::<std::collections::BTreeSet<_>>().len(),
            limit,
            "each hit must be a distinct memory, not duplicate chunks from one memory"
        );
        Ok(())
    }

    #[test]
    fn relaxed_bm25_fallback_applies_rank_offset_to_or_hits() -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let mut index = Index::new(open_index(&temp.path().join("index.sqlite"))?);

        let strict_id = "mem_20260612_a1b2c3d4e5f60718_020100";
        let mut strict_match = sample_memory(strict_id);
        strict_match.body =
            "rankanchor bravoextra charlieextra deltaextra echoextra foxtrotextra golfextra strict body".to_string();

        let satellite_terms = ["bravoextra", "charlieextra", "deltaextra", "echoextra", "foxtrotextra"];
        let satellite_ids = [
            "mem_20260612_a1b2c3d4e5f60718_020101",
            "mem_20260612_a1b2c3d4e5f60718_020102",
            "mem_20260612_a1b2c3d4e5f60718_020103",
            "mem_20260612_a1b2c3d4e5f60718_020104",
            "mem_20260612_a1b2c3d4e5f60718_020105",
        ];
        let mut satellites = Vec::new();
        for (id, term) in satellite_ids.into_iter().zip(satellite_terms) {
            let mut memory = sample_memory(id);
            memory.body = format!("rankanchor {term} relaxed-only satellite body");
            satellites.push(memory);
        }

        index.upsert_memory(&strict_match, false)?;
        for memory in &satellites {
            index.upsert_memory(memory, false)?;
        }

        let limit = 4;
        let hits = index.query_hybrid_bm25_memories(
            "rankanchor bravoextra charlieextra deltaextra echoextra foxtrotextra golfextra",
            limit,
        )?;

        assert_eq!(hits.len(), limit);

        let strict_hits: Vec<_> = hits.iter().filter(|hit| hit.memory_id == strict_id).collect();
        assert_eq!(strict_hits.len(), 1, "exactly one strict AND match expected");
        assert_eq!(strict_hits[0].rank, 1);

        let relaxed_hits: Vec<_> = hits.iter().filter(|hit| hit.memory_id != strict_id).collect();
        assert_eq!(relaxed_hits.len(), limit - 1);
        let strict_len = strict_hits.len();
        for (idx, hit) in relaxed_hits.iter().enumerate() {
            assert_eq!(hit.rank, strict_len + idx + 1 + super::RELAXED_RANK_OFFSET);
        }

        Ok(())
    }

    fn sample_memory(id: &str) -> Memory {
        let now = Utc::now();
        Memory {
            frontmatter: Frontmatter {
                schema_version: 1,
                id: MemoryId::new(id),
                memory_type: MemoryType::Pattern,
                scope: Scope::Agent,
                summary: "bm25 relaxed fallback".to_string(),
                confidence: 1.0,
                original_confidence: None,
                trust_level: TrustLevel::Trusted,
                sensitivity: Sensitivity::Internal,
                status: MemoryStatus::Active,
                created_at: now,
                updated_at: now,
                observed_at: None,
                author: Author {
                    kind: AuthorKind::System,
                    user_handle: None,
                    harness: None,
                    harness_version: None,
                    session_id: None,
                    subagent_id: None,
                    phase: None,
                    component: Some("test".to_string()),
                },
                namespace: None,
                canonical_namespace_id: None,
                tags: Vec::new(),
                entities: Vec::new(),
                aliases: Vec::new(),
                source: Source {
                    kind: SourceKind::Import,
                    reference: None,
                    harness: None,
                    harness_version: None,
                    session_id: None,
                    subagent_id: None,
                    device: None,
                },
                evidence: Vec::new(),
                requires_user_confirmation: false,
                review_state: None,
                supersedes: Vec::new(),
                superseded_by: Vec::new(),
                related: Vec::new(),
                tombstone_events: Vec::new(),
                retrieval_policy: RetrievalPolicy {
                    passive_recall: true,
                    max_scope: Scope::Agent,
                    mask_personal_for_synthesis: false,
                    index_body: true,
                    index_embeddings: true,
                },
                write_policy: WritePolicy {
                    human_review_required: false,
                    policy_applied: "default-v1".to_string(),
                    expected_base_hash: None,
                },
                merge_diagnostics: None,
                extras: std::collections::BTreeMap::new(),
            },
            body: "bm25 relaxed fallback body".to_string(),
            path: Some(RepoPath::new(format!("agent/patterns/{id}.md"))),
        }
    }
}
