//! Index upsert and query helpers.
//!
//! Layout (stepdown / newspaper): this file is the public `Index` facade —
//! the orchestrator-level methods.  The SQL helper bodies live in focused
//! sibling submodules (`upsert`, `embedding`, `search`, `read`, `fts`, `util`)
//! and are imported below under their original names so the method bodies read
//! the same as before the split.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use rusqlite::{params, params_from_iter, types::Value, Connection};

use crate::error::{SubstrateResult, VectorError};
use crate::events::{Event, EventKind};
use crate::model::{
    AbstractionVectorHit, AbstractionVectorRow, AuxEmbeddingUpdate, AuxPendingEmbeddingJob, AuxRowKind, ChunkResult,
    CueVectorHit, EmbeddingLaneEligibility, EmbeddingTriple, EmbeddingUpdate, Entity, EventsLogMirrorHealth,
    HybridMemoryCandidate, HybridScoreBreakdown, HybridVectorQuery, Memory, MemoryId, MemoryQuery, MemoryStatus,
    QueryResult, RecallIndexQuery, RecallIndexRow, RepoPath, ReviewQueuePage, ReviewQueueRow, Scope, Sha256,
};

use super::embedding::{
    ensure_vector_table, is_dropped_triple, is_dropped_triple_rusqlite, read_chunk_rowid,
    reconcile_active_embedding_jobs_impl, resolve_pending_embedding_job, table_exists, upsert_chunk_embedding_meta,
    upsert_vector_payload, validate_update_preconditions,
};
use super::fts::{sanitize_fts_query, sanitize_relaxed_fts_query, RELAXED_RANK_OFFSET};

/// The hash-scoped pending-aux-job delete used by `update_aux_embedding`.
/// `pub` solely so the vector-lifecycle predicate pin executes the SAME SQL as
/// production — the `AND content_hash=?6` scoping is the W2-F2 fix; a drifted
/// copy in the test would let a regression ship behind a green pin (round-3
/// review finding).
pub const AUX_PENDING_JOB_HASH_SCOPED_DELETE_SQL: &str = "DELETE FROM aux_pending_embedding_jobs
             WHERE row_kind=?1 AND target_id=?2 AND provider=?3 AND model_ref=?4 AND dimension=?5 AND content_hash=?6";

/// W3 single-source non-servability predicate. Excludes `superseded` rows and
/// merge-staged candidate replacements (`status = 'candidate'` with
/// `write_policy.policy_applied = 'merge-staged-v1'`). Shared by every gated
/// read lane so the SQL and Rust predicate stay one definition.
pub const MERGE_NON_SERVABLE_SQL: &str = "NOT (memories.status = 'superseded' OR (memories.status = 'candidate' AND json_extract(memories.frontmatter_json, '$.write_policy.policy_applied') = 'merge-staged-v1'))";
use super::read::{
    append_filters_and_order, append_match_term_filters, append_memory_query_filters, append_recall_index_filters,
    collect_query_results, hydrate_recall_index_auxiliary, read_all_entity_rows, read_entities_by_memory,
    row_to_recall_index_row,
};
use super::search::{
    bm25_chunk_hit_from_row, chunk_texts_by_rowid, collapse_bm25_memory_hits, compare_hybrid_candidates,
    cosine_from_l2_distance, later_recency_at, vector_chunk_ref_from_row, Bm25ChunkHit, Bm25MemoryRank,
    VectorMemoryScore,
};
use super::upsert::{
    file_consistency_state_in_connection, resync_supersession_edges_sql, upsert_memory_row_in_txn,
    upsert_memory_row_with_full_metadata, MemoryUpsertOptions,
};
use super::util::{invalid_column_value, EMBEDDING_TRIPLE_PREDICATE};
use super::{bucketed_in_clause_width, pad_in_clause_bindings, sql_placeholders};

/// Index handle.  Owns a single SQLite connection; all mutating methods take
/// `&mut self` so the borrow checker prevents concurrent transactions.
pub struct Index {
    connection: Connection,
    active_embedding: EmbeddingTriple,
}

impl Index {
    /// Active/pinned memories missing an abstraction or stale against the body hash.
    pub fn abstraction_compile_candidates(&self, limit: usize) -> rusqlite::Result<Vec<MemoryId>> {
        // "Already compiled" is a frontmatter fact, not a servable-vector fact:
        // encrypted rows carry their amended abstraction in frontmatter but are
        // never servable, so `memory_abstractions` stays intentionally empty for
        // them — keying candidacy on that table alone re-selects (and re-amends)
        // every encrypted row forever. Body-drift recompiles still key on the
        // servable row's source hash, which only plaintext rows have.
        let mut stmt = self.connection.prepare_cached(
            "SELECT memories.id FROM memories
             LEFT JOIN memory_abstractions ON memory_abstractions.memory_id=memories.id
             WHERE memories.status IN ('active','pinned')
               AND (json_extract(memories.frontmatter_json,'$.abstraction') IS NULL
                    OR (memory_abstractions.memory_id IS NOT NULL
                        AND memory_abstractions.source_body_hash<>memories.body_hash))
             ORDER BY memories.updated_at LIMIT ?1",
        )?;
        let ids = stmt.query_map([limit as i64], |row| Ok(MemoryId::new(row.get::<_, String>(0)?)))?.collect();
        ids
    }
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

    /// Upsert many memories under a single transaction, computing the
    /// loop-invariant "active embedding triple dropped?" flag once.
    ///
    /// Equivalent to calling [`Self::upsert_memory_with_file_hash`] for each
    /// `(memory, metadata_only, file_hash)` triple, but the whole batch shares
    /// one transaction (one WAL commit cycle instead of N) and one
    /// `dropped_embedding_triples` probe (the active triple is fixed for the
    /// batch). Each row's FK-guarded supersession behavior is unchanged, so
    /// callers that relied on the per-row `resync_supersession_edges` follow-up
    /// must still run it after the batch. On any row error the transaction is
    /// rolled back as a unit.
    ///
    /// Intended for SMALL drift sets (the startup phase-6 and encrypted-tier
    /// incremental sweeps, which batch only the handful of files that actually
    /// changed). A full-corpus rebuild deliberately does NOT use this: a single
    /// transaction over thousands of rows leaves a large un-checkpointed WAL that
    /// measurably slows the immediately-following point reads (a `query_by_id`
    /// perf-gate regression, unfixed by a post-batch checkpoint), so
    /// `full_reindex_from_repo` stays per-row. Keep batches here bounded.
    pub fn batch_upsert_memories_with_file_hash<'a, I>(&mut self, memories: I) -> rusqlite::Result<()>
    where
        I: IntoIterator<Item = (&'a Memory, bool, Option<&'a Sha256>)>,
    {
        let active_embedding_dropped = is_dropped_triple_rusqlite(&self.connection, &self.active_embedding)?;
        let active_embedding = self.active_embedding.clone();
        let txn = self.connection.transaction()?;
        for (memory, metadata_only, file_hash) in memories {
            upsert_memory_row_in_txn(
                &txn,
                memory,
                MemoryUpsertOptions { metadata_only, file_hash, active_embedding: &active_embedding },
                active_embedding_dropped,
            )?;
        }
        txn.commit()
    }

    /// Clear plaintext-derived rows before reindexing Markdown files.
    ///
    /// Encrypted-tier rows (`encrypted/%`) are intentionally preserved here:
    /// their safe projections are handled by the encrypted incremental/full
    /// reindex paths, and out-of-band encrypted deletions are not pruned by this
    /// plaintext clear.
    pub fn clear_plaintext_memory_index(&mut self) -> rusqlite::Result<()> {
        let aux_vector_tables = {
            let mut stmt = self.connection.prepare(
                "SELECT name FROM sqlite_master WHERE type='table'
                   AND (name LIKE 'vec_abstractions_%' OR name LIKE 'vec_cues_%')",
            )?;
            let tables = stmt.query_map([], |row| row.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
            tables
        };
        for table in aux_vector_tables {
            self.connection.execute(&format!("DROP TABLE IF EXISTS {table}"), [])?;
        }
        let txn = self.connection.transaction()?;
        txn.execute(
            "DELETE FROM memory_chunks
             WHERE memory_id IN (SELECT id FROM memories WHERE path NOT LIKE 'encrypted/%')",
            [],
        )?;
        txn.execute("DELETE FROM memories WHERE path NOT LIKE 'encrypted/%'", [])?;
        txn.execute("DELETE FROM aux_embedding_meta", [])?;
        txn.execute("DELETE FROM aux_pending_embedding_jobs", [])?;
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

    /// Batched sibling of [`Self::update_embedding`] for the drain worker.
    ///
    /// Applies a slice of embedding updates, returning one `Result` per input in
    /// positional order. Each chunk's metadata + job resolution runs in its own
    /// savepoint inside one shared transaction, so a per-chunk failure rolls back
    /// only that chunk — including the benign `StaleChunk` skip, the per-chunk
    /// outcome matches calling [`Self::update_embedding`] once per update. The win
    /// is amortization: one WAL commit for the whole metadata phase instead of one
    /// transaction per chunk.
    ///
    /// The one divergence from the per-call path is the outer commit boundary: if
    /// the final shared `COMMIT` fails (e.g. `SQLITE_FULL`), every savepoint-applied
    /// chunk rolls back together and is reported `IndexUnavailable`, whereas N
    /// separate calls would have durably committed the earlier chunks. The drain
    /// worker treats that as a retryable failure and re-enqueues, so the only cost
    /// is repeated work — never a lost or phantom embedding.
    ///
    /// Spec §10.2.1 step 4 ordering is preserved: every vector payload is upserted
    /// OUTSIDE the transaction first, then the `chunk_embedding_meta` +
    /// `pending_embedding_jobs` rows for the chunks that upserted cleanly are
    /// written in the shared transaction. A chunk whose validation or vector upsert
    /// fails contributes its error to the result vector and is excluded, exactly as
    /// the per-chunk path would leave it.
    pub fn update_embeddings_batch(&mut self, updates: &[EmbeddingUpdate]) -> Vec<Result<(), VectorError>> {
        let mut results: Vec<Result<(), VectorError>> = Vec::with_capacity(updates.len());
        // Indices of updates that validated and upserted their vector cleanly and
        // therefore still need their metadata/job rows written in the shared txn.
        let mut committed_indices: Vec<usize> = Vec::with_capacity(updates.len());

        // Step 1: per-chunk validation + vector upsert, OUTSIDE any transaction
        // (spec §10.2.1 step 4). Mirrors the head of `update_embedding`.
        for update in updates {
            let outcome = (|| {
                validate_update_preconditions(&self.connection, update)?;
                let chunk_rowid = read_chunk_rowid(&self.connection, update.chunk_id.as_str())?;
                ensure_vector_table(&self.connection, &update.triple)?;
                upsert_vector_payload(
                    &self.connection,
                    &update.triple,
                    update.chunk_id.as_str(),
                    chunk_rowid,
                    &update.vector,
                )
            })();
            results.push(outcome);
        }
        for (idx, result) in results.iter().enumerate() {
            if result.is_ok() {
                committed_indices.push(idx);
            }
        }
        if committed_indices.is_empty() {
            return results;
        }

        // Step 2: one SQLite transaction for the metadata + job resolution of
        // every chunk that upserted cleanly. If opening or committing the txn
        // fails, downgrade the affected entries to that error so callers do not
        // observe a vector without its resolved job.
        let mut txn = match self.connection.transaction() {
            Ok(txn) => txn,
            Err(err) => {
                // `VectorError` is not `Clone` (its rusqlite/serde sources are
                // not), so fan the single failure out as a message-preserving
                // `IndexUnavailable` per affected chunk.
                let message = err.to_string();
                for idx in &committed_indices {
                    results[*idx] = Err(VectorError::IndexUnavailable(message.clone()));
                }
                return results;
            }
        };
        for idx in &committed_indices {
            let update = &updates[*idx];
            // Isolate each chunk in its own savepoint so a failed metadata/job write
            // rolls back only that chunk — a chunk reported as failed never leaves a
            // committed `chunk_embedding_meta` row behind, matching `update_embedding`.
            let outcome = (|| {
                let savepoint = txn.savepoint()?;
                upsert_chunk_embedding_meta(&savepoint, update)?;
                resolve_pending_embedding_job(&savepoint, update)?;
                savepoint.commit()?;
                Ok(())
            })();
            if let Err(err) = outcome {
                results[*idx] = Err(err);
            }
        }
        if let Err(err) = txn.commit() {
            let message = err.to_string();
            for idx in &committed_indices {
                if results[*idx].is_ok() {
                    results[*idx] = Err(VectorError::IndexUnavailable(message.clone()));
                }
            }
        }
        results
    }

    /// Drop an embedding triple and return the removal report.
    pub fn drop_embedding_model_report(
        &mut self,
        triple: &EmbeddingTriple,
    ) -> Result<crate::model::DropTripleReport, VectorError> {
        let mut vectors_removed = self.connection.execute(
            &format!("DELETE FROM chunk_vectors WHERE {EMBEDDING_TRIPLE_PREDICATE}"),
            params![triple.provider, triple.model_ref, i64::from(triple.dimension)],
        )? as u64;
        let mut meta_rows_removed = self.connection.execute(
            &format!("DELETE FROM chunk_embedding_meta WHERE {EMBEDDING_TRIPLE_PREDICATE}"),
            params![triple.provider, triple.model_ref, i64::from(triple.dimension)],
        )? as u64;
        let mut pending_jobs_dropped = self.connection.execute(
            &format!("DELETE FROM pending_embedding_jobs WHERE {EMBEDDING_TRIPLE_PREDICATE}"),
            params![triple.provider, triple.model_ref, i64::from(triple.dimension)],
        )? as u64;
        meta_rows_removed += self.connection.execute(
            &format!("DELETE FROM aux_embedding_meta WHERE {EMBEDDING_TRIPLE_PREDICATE}"),
            params![triple.provider, triple.model_ref, i64::from(triple.dimension)],
        )? as u64;
        pending_jobs_dropped += self.connection.execute(
            &format!("DELETE FROM aux_pending_embedding_jobs WHERE {EMBEDDING_TRIPLE_PREDICATE}"),
            params![triple.provider, triple.model_ref, i64::from(triple.dimension)],
        )? as u64;
        let table = crate::index::sqlite_vec::vector_table_name(triple);
        let table_dropped = table_exists(&self.connection, &table)?;
        self.connection.execute(
            "INSERT OR IGNORE INTO dropped_embedding_triples(provider,model_ref,dimension) VALUES (?1,?2,?3)",
            params![triple.provider, triple.model_ref, i64::from(triple.dimension)],
        )?;
        self.connection.execute(&format!("DROP TABLE IF EXISTS {table}"), [])?;
        for kind in [AuxRowKind::Abstraction, AuxRowKind::Cue] {
            let aux_table = crate::index::sqlite_vec::aux_vector_table_name(kind, triple);
            if table_exists(&self.connection, &aux_table)? {
                vectors_removed +=
                    self.connection
                        .query_row(&format!("SELECT COUNT(*) FROM {aux_table}"), [], |row| row.get::<_, i64>(0))?
                        as u64;
            }
            self.connection.execute(&format!("DROP TABLE IF EXISTS {aux_table}"), [])?;
        }
        Ok(crate::model::DropTripleReport { vectors_removed, meta_rows_removed, pending_jobs_dropped, table_dropped })
    }

    /// Count vectors stored for a triple.
    pub fn vector_count(&self, triple: &EmbeddingTriple) -> Result<usize, VectorError> {
        self.connection
            .query_row(
                &format!("SELECT COUNT(*) FROM chunk_vectors WHERE {EMBEDDING_TRIPLE_PREDICATE}"),
                params![triple.provider, triple.model_ref, i64::from(triple.dimension)],
                |row| row.get::<_, i64>(0),
            )
            .map(|count| count as usize)
            .map_err(Into::into)
    }

    /// Reconcile chunk/vector metadata and enqueue missing embeddings for the active triple.
    pub fn reconcile_active_embedding_jobs(
        &mut self,
        eligibility: EmbeddingLaneEligibility,
    ) -> Result<usize, VectorError> {
        let triple = self.active_embedding.clone();
        reconcile_active_embedding_jobs_impl(&mut self.connection, &triple, eligibility)
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
        self.connection.execute(resync_supersession_edges_sql(), [])
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
    pub fn pending_embedding_jobs(
        &self,
        limit: usize,
        eligibility: EmbeddingLaneEligibility,
    ) -> Result<Vec<crate::model::PendingEmbeddingJob>, VectorError> {
        let triple = &self.active_embedding;
        let allowed_sensitivities = eligibility.allowed_sensitivity_db_strs();
        let mut sql = String::from(
            "SELECT mc.chunk_id, mc.text, mc.body_hash
             FROM pending_embedding_jobs pj
             JOIN memory_chunks mc ON mc.chunk_id = pj.chunk_id
             JOIN memories m ON m.id = mc.memory_id
             WHERE pj.provider = ? AND pj.model_ref = ? AND pj.dimension = ?
               AND pj.content_hash = mc.body_hash",
        );
        if eligibility.requires_plaintext_filter() {
            sql.push_str(" AND m.sensitivity IN (");
            sql.push_str(&sql_placeholders(allowed_sensitivities.len()));
            sql.push(')');
        }
        sql.push_str(
            "
             ORDER BY pj.enqueued_at
             LIMIT ?",
        );
        let mut bindings = vec![
            Value::from(triple.provider.clone()),
            Value::from(triple.model_ref.clone()),
            Value::from(i64::from(triple.dimension)),
        ];
        bindings.extend(allowed_sensitivities.into_iter().map(|sensitivity| Value::from(sensitivity.to_string())));
        bindings.push(Value::from(limit as i64));
        let mut stmt = self.connection.prepare_cached(&sql)?;
        let rows = stmt
            .query_map(params_from_iter(bindings), |row| {
                Ok(crate::model::PendingEmbeddingJob {
                    chunk_id: row.get::<_, String>(0)?,
                    text: row.get::<_, String>(1)?,
                    content_hash: crate::model::Sha256::new(row.get::<_, String>(2)?),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Drainable abstraction/cue jobs for the active triple.
    pub fn pending_aux_embedding_jobs(
        &self,
        limit: usize,
        eligibility: EmbeddingLaneEligibility,
    ) -> Result<Vec<AuxPendingEmbeddingJob>, VectorError> {
        let triple = &self.active_embedding;
        let allowed = eligibility.allowed_sensitivity_db_strs();
        let mut sql = String::from(
            "SELECT jobs.row_kind, jobs.target_id,
                    CASE jobs.row_kind WHEN 'abstraction' THEN abstractions.abstraction ELSE cues.cue_text END,
                    jobs.content_hash
             FROM aux_pending_embedding_jobs jobs
             LEFT JOIN memory_abstractions abstractions
               ON jobs.row_kind='abstraction' AND abstractions.memory_id=jobs.target_id
             LEFT JOIN memory_cues cues
               ON jobs.row_kind='cue'
              AND cues.memory_id=substr(jobs.target_id,1,instr(jobs.target_id,':')-1)
              AND cues.ordinal=CAST(substr(jobs.target_id,instr(jobs.target_id,':')+1) AS INTEGER)
             JOIN memories ON memories.id=COALESCE(abstractions.memory_id,cues.memory_id)
             WHERE jobs.provider=? AND jobs.model_ref=? AND jobs.dimension=?
               AND jobs.content_hash=CASE jobs.row_kind WHEN 'abstraction' THEN abstractions.abstraction_hash ELSE cues.cue_hash END",
        );
        if eligibility.requires_plaintext_filter() {
            sql.push_str(" AND memories.sensitivity IN (");
            sql.push_str(&sql_placeholders(allowed.len()));
            sql.push(')');
        }
        sql.push_str(" ORDER BY jobs.enqueued_at LIMIT ?");
        let mut bindings = vec![
            Value::from(triple.provider.clone()),
            Value::from(triple.model_ref.clone()),
            Value::from(i64::from(triple.dimension)),
        ];
        bindings.extend(allowed.into_iter().map(|value| Value::from(value.to_string())));
        bindings.push(Value::from(limit as i64));
        let mut stmt = self.connection.prepare(&sql)?;
        let rows = stmt
            .query_map(params_from_iter(bindings), |row| {
                let kind: String = row.get(0)?;
                Ok(AuxPendingEmbeddingJob {
                    row_kind: if kind == "abstraction" { AuxRowKind::Abstraction } else { AuxRowKind::Cue },
                    target_id: row.get(1)?,
                    text: row.get(2)?,
                    content_hash: Sha256::new(row.get::<_, String>(3)?),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Commit one stale-fenced abstraction/cue embedding.
    pub fn update_aux_embedding(&mut self, update: &AuxEmbeddingUpdate) -> Result<(), VectorError> {
        crate::index::sqlite_vec::validate_dimension(&update.triple, &update.vector)?;
        if is_dropped_triple(&self.connection, &update.triple)? {
            return Err(VectorError::UnknownEmbeddingTriple(update.triple.clone()));
        }
        let table = crate::index::sqlite_vec::aux_vector_table_name(update.row_kind, &update.triple);
        let txn = self.connection.transaction()?;
        let (rowid, actual_hash) = Self::aux_target_row_conn(&txn, update.row_kind, &update.target_id)?;
        if actual_hash != update.expected_content_hash.as_str() {
            return Err(VectorError::StaleAux {
                row_kind: update.row_kind.as_db_str().to_string(),
                target_id: update.target_id.clone(),
                expected: update.expected_content_hash.clone(),
                found: Sha256::new(actual_hash),
            });
        }
        // Vector table creation/insert, meta upsert, and job delete all happen in
        // one transaction so the hash fence cannot be invalidated between the
        // read and the delete.
        txn.execute(
            &format!(
                "CREATE VIRTUAL TABLE IF NOT EXISTS {table} USING vec0(embedding float[{}])",
                update.triple.dimension
            ),
            [],
        )?;
        txn.execute(
            &format!("INSERT OR REPLACE INTO {table}(rowid,embedding) VALUES (?1,?2)"),
            params![rowid, crate::index::sqlite_vec::serialize_f32(&update.vector)],
        )?;
        txn.execute(
            "INSERT INTO aux_embedding_meta(row_kind,target_id,content_hash,provider,model_ref,dimension,embedded_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7)
             ON CONFLICT(row_kind,target_id,provider,model_ref,dimension) DO UPDATE SET
               content_hash=excluded.content_hash,embedded_at=excluded.embedded_at",
            params![
                update.row_kind.as_db_str(),
                update.target_id,
                update.expected_content_hash.as_str(),
                update.triple.provider,
                update.triple.model_ref,
                i64::from(update.triple.dimension),
                Utc::now().to_rfc3339()
            ],
        )?;
        txn.execute(
            AUX_PENDING_JOB_HASH_SCOPED_DELETE_SQL,
            params![
                update.row_kind.as_db_str(),
                update.target_id,
                update.triple.provider,
                update.triple.model_ref,
                i64::from(update.triple.dimension),
                update.expected_content_hash.as_str()
            ],
        )?;
        txn.commit()?;
        Ok(())
    }

    fn aux_target_row_conn(conn: &Connection, kind: AuxRowKind, target_id: &str) -> Result<(i64, String), VectorError> {
        let result = match kind {
            AuxRowKind::Abstraction => conn.query_row(
                "SELECT rowid,abstraction_hash FROM memory_abstractions WHERE memory_id=?1",
                [target_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            ),
            AuxRowKind::Cue => {
                let (memory_id, ordinal) = target_id.rsplit_once(':').ok_or_else(|| VectorError::StaleAux {
                    row_kind: "cue".to_string(),
                    target_id: target_id.to_string(),
                    expected: Sha256::new("missing"),
                    found: Sha256::new("missing"),
                })?;
                conn.query_row(
                    "SELECT rowid,cue_hash FROM memory_cues WHERE memory_id=?1 AND ordinal=?2",
                    params![memory_id, ordinal.parse::<i64>().unwrap_or(-1)],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
            }
        };
        result.map_err(|_| VectorError::StaleAux {
            row_kind: kind.as_db_str().to_string(),
            target_id: target_id.to_string(),
            expected: Sha256::new("missing"),
            found: Sha256::new("missing"),
        })
    }

    /// KNN query over abstraction vectors. W2 deliberately does not wire this into recall.
    pub fn query_abstraction_vectors(
        &self,
        triple: &EmbeddingTriple,
        vector: &[f32],
        limit: usize,
    ) -> Result<Vec<AbstractionVectorHit>, VectorError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        crate::index::sqlite_vec::validate_dimension(triple, vector)?;
        let table = crate::index::sqlite_vec::aux_vector_table_name(AuxRowKind::Abstraction, triple);
        if is_dropped_triple(&self.connection, triple)? || !table_exists(&self.connection, &table)? {
            return Err(VectorError::UnknownEmbeddingTriple(triple.clone()));
        }
        let sql = format!(
            "SELECT abstractions.memory_id,{table}.distance FROM {table}
             JOIN memory_abstractions abstractions ON abstractions.rowid={table}.rowid
             JOIN aux_embedding_meta meta ON meta.row_kind='abstraction' AND meta.target_id=abstractions.memory_id
               AND meta.provider=?3 AND meta.model_ref=?4 AND meta.dimension=?5
               AND meta.content_hash=abstractions.abstraction_hash
             JOIN memories ON memories.id=abstractions.memory_id
               AND memories.status IN ('active','pinned')
               AND memories.metadata_only=0 AND memories.passive_recall=1 AND memories.index_body=1
             WHERE embedding MATCH ?1 AND k=?2
             ORDER BY {table}.distance, abstractions.memory_id"
        );
        let blob = crate::index::sqlite_vec::serialize_f32(vector);
        let mut stmt = self.connection.prepare(&sql)?;
        let hits = stmt
            .query_map(
                params![blob, limit as i64, triple.provider, triple.model_ref, i64::from(triple.dimension)],
                |row| {
                    Ok(AbstractionVectorHit {
                        memory_id: MemoryId::new(row.get::<_, String>(0)?),
                        distance: row.get::<_, f64>(1)? as f32,
                    })
                },
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(hits)
    }

    pub fn all_abstraction_vectors(&self, triple: &EmbeddingTriple) -> Result<Vec<AbstractionVectorRow>, VectorError> {
        let table = crate::index::sqlite_vec::aux_vector_table_name(AuxRowKind::Abstraction, triple);
        if is_dropped_triple(&self.connection, triple)? || !table_exists(&self.connection, &table)? {
            return Err(VectorError::UnknownEmbeddingTriple(triple.clone()));
        }
        let sql = format!(
            "SELECT abstractions.memory_id,{table}.embedding FROM {table}
             JOIN memory_abstractions abstractions ON abstractions.rowid={table}.rowid
             JOIN aux_embedding_meta meta ON meta.row_kind='abstraction' AND meta.target_id=abstractions.memory_id
               AND meta.provider=?1 AND meta.model_ref=?2 AND meta.dimension=?3
               AND meta.content_hash=abstractions.abstraction_hash
             JOIN memories ON memories.id=abstractions.memory_id AND memories.status IN ('active','pinned')
             ORDER BY abstractions.memory_id"
        );
        let mut stmt = self.connection.prepare(&sql)?;
        let rows = stmt
            .query_map(params![triple.provider, triple.model_ref, i64::from(triple.dimension)], |row| {
                let bytes = row.get::<_, Vec<u8>>(1)?;
                let vector = bytes
                    .chunks_exact(4)
                    .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                    .collect();
                Ok(AbstractionVectorRow { memory_id: MemoryId::new(row.get::<_, String>(0)?), vector })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// KNN query over cue vectors. W2 deliberately does not wire this into recall.
    pub fn query_cue_vectors(
        &self,
        triple: &EmbeddingTriple,
        vector: &[f32],
        limit: usize,
    ) -> Result<Vec<CueVectorHit>, VectorError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        crate::index::sqlite_vec::validate_dimension(triple, vector)?;
        let table = crate::index::sqlite_vec::aux_vector_table_name(AuxRowKind::Cue, triple);
        if is_dropped_triple(&self.connection, triple)? || !table_exists(&self.connection, &table)? {
            return Err(VectorError::UnknownEmbeddingTriple(triple.clone()));
        }
        let sql = format!(
            "SELECT cues.memory_id,cues.ordinal,{table}.distance FROM {table}
             JOIN memory_cues cues ON cues.rowid={table}.rowid
             JOIN aux_embedding_meta meta ON meta.row_kind='cue' AND meta.target_id=cues.memory_id||':'||cues.ordinal
               AND meta.provider=?3 AND meta.model_ref=?4 AND meta.dimension=?5 AND meta.content_hash=cues.cue_hash
             JOIN memories ON memories.id=cues.memory_id
               AND memories.status IN ('active','pinned')
               AND memories.metadata_only=0 AND memories.passive_recall=1 AND memories.index_body=1
             WHERE embedding MATCH ?1 AND k=?2
             ORDER BY {table}.distance, cues.memory_id, cues.ordinal"
        );
        let blob = crate::index::sqlite_vec::serialize_f32(vector);
        let mut stmt = self.connection.prepare(&sql)?;
        let hits = stmt
            .query_map(
                params![blob, limit as i64, triple.provider, triple.model_ref, i64::from(triple.dimension)],
                |row| {
                    Ok(CueVectorHit {
                        memory_id: MemoryId::new(row.get::<_, String>(0)?),
                        ordinal: row.get::<_, i64>(1)? as u8,
                        distance: row.get::<_, f64>(2)? as f32,
                    })
                },
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(hits)
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
               AND memories.status IN ('active','pinned')
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
               AND memories.status IN ('active','pinned')
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
        const SQL_BASE: &str =
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

        if let Some(row_limit) = row_limit {
            let sql_limited = format!("{SQL_BASE}\n             LIMIT ?2");
            let mut stmt = self.connection.prepare_cached(&sql_limited)?;
            let rows = stmt
                .query_map(params![fts_query, row_limit as i64], bm25_chunk_hit_from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(Into::into);
            return rows;
        }

        let mut stmt = self.connection.prepare_cached(SQL_BASE)?;
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
        // Over-fetch text-free rows only: the KNN lane retrieves up to `knn_k`
        // chunks (limit*8) but collapses to one chunk per memory and truncates to
        // `limit`, discarding ~90% of rows. Selecting `memory_chunks.text` here
        // would materialize a heap String per over-fetched chunk (up to
        // MAX_CHUNK_BYTES each) only to drop most of them. Instead we project the
        // tiny `(memory_id, chunk_rowid, distance, recency)` tuple, collapse and
        // truncate, then fetch text only for the surviving nearest chunks below.
        let sql = format!(
            "SELECT memory_chunks.memory_id, memory_chunks.chunk_rowid, {table}.distance,
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
            .query_map(params![blob, knn_k as i64], vector_chunk_ref_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let mut collapsed = super::search::collapse_vector_chunk_refs(rows);
        collapsed.truncate(limit);

        // Fetch text only for the surviving nearest chunks, one row per memory.
        let survivor_rowids: Vec<i64> = collapsed.iter().map(|hit| hit.chunk_rowid).collect();
        let texts = chunk_texts_by_rowid(&self.connection, &survivor_rowids)?;

        Ok(collapsed
            .into_iter()
            .map(|hit| VectorMemoryScore {
                memory_id: hit.memory_id,
                text: texts.get(&hit.chunk_rowid).cloned().unwrap_or_default(),
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
            bindings.push(rusqlite::types::Value::Text(scope.as_db_str().to_string()));
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
            let scope = Scope::from_db_str(&scope_text).ok_or_else(|| invalid_column_value("scope", &scope_text))?;
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

    /// Return the indexed `(file_hash, is_quarantined)` for a repo path, or
    /// `None` if not indexed.
    ///
    /// Lets phase-6 index consistency gate the expensive Markdown parse behind a
    /// cheap raw-bytes hash comparison: when the on-disk hash matches the stored
    /// `file_hash`, the file is clean and its frontmatter is already faithfully
    /// reflected by the indexed `status`/`trust_level`, so the blocking-conflict
    /// check can read the quarantine flag from here instead of re-parsing.
    /// `is_quarantined` mirrors the prior `scan_blocking_conflicts` predicate:
    /// either `status` or `trust_level` equal to `quarantined`.
    pub fn file_consistency_state(&self, path: &RepoPath) -> Option<(crate::model::Sha256, bool)> {
        file_consistency_state_in_connection(&self.connection, path)
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
            let status =
                MemoryStatus::from_db_str(&status_text).ok_or_else(|| invalid_column_value("status", &status_text))?;
            counts.push((status, count.max(0) as u64));
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
            let scope = Scope::from_db_str(&scope_text).ok_or_else(|| invalid_column_value("scope", &scope_text))?;
            counts.push((scope, canonical_namespace_id, count.max(0) as u64));
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
        // Base scalar projection (18 columns). `source_harness` is read from its
        // materialized column rather than re-parsed out of `frontmatter_json` —
        // the column is written from the identical `source.harness` frontmatter
        // field at upsert time, so the values are byte-equal and the column read
        // saves a JSON parse per row.
        //
        // The remaining identity/merge-diagnostics fields require a per-row
        // `json_extract` parse of `frontmatter_json`. Only the peer-write
        // attribution and conflict-list readers consume them, so the projection
        // is gated behind `query.source_identity`: the hot ranking/omission path
        // omits the four extra `json_extract` calls entirely and leaves those
        // fields `None`. Both SQL variants stay warm in `prepare_cached` under
        // distinct keys.
        let mut sql = String::from(
            "SELECT memories.id,memories.path,memories.summary,memories.status,memories.scope,
                    memories.canonical_namespace_id,memories.updated_at,memories.indexed_at,memories.confidence,
                    memories.source_kind,memories.source_device,memories.sensitivity,memories.passive_recall,memories.index_body,
                    memories.requires_user_confirmation,memories.review_state,
                    memories.human_review_required,memories.max_scope,
                    memories.source_harness",
        );
        if query.source_identity {
            sql.push_str(
                ",
                    json_extract(memories.frontmatter_json, '$.source.session_id'),
                    json_extract(memories.frontmatter_json, '$.author.harness'),
                    json_extract(memories.frontmatter_json, '$.author.session_id'),
                    json_extract(memories.frontmatter_json, '$._merge_diagnostics')",
            );
        }
        sql.push_str(" FROM memories");
        let mut filters = Vec::new();
        let mut bindings = Vec::new();
        append_recall_index_filters(query, include_metadata_only, &mut filters, &mut bindings)?;
        append_match_term_filters(query, &mut filters, &mut bindings);
        append_filters_and_order(&mut sql, filters, "memories.id");

        let mut stmt = self.connection.prepare_cached(&sql)?;
        let mut rows = stmt.query(params_from_iter(bindings.iter()))?;
        let mut results = Vec::new();
        while let Some(row) = rows.next()? {
            results.push(row_to_recall_index_row(row, query.source_identity)?);
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
        let mut sql = String::from("SELECT COUNT(*) FROM memories");
        let mut filters = Vec::new();
        let mut bindings = Vec::new();
        append_recall_index_filters(query, false, &mut filters, &mut bindings)?;
        append_match_term_filters(query, &mut filters, &mut bindings);
        if !filters.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&filters.join(" AND "));
        }
        let mut stmt = self.connection.prepare_cached(&sql)?;
        let count: i64 = stmt.query_row(params_from_iter(bindings.iter()), |row| row.get(0))?;
        Ok(count.max(0) as usize)
    }

    /// Count recall-index rows with the W3 non-servability predicate applied.
    /// These are the same rows that gated read lanes surface, so counts such as
    /// the pending-attention total never include merge-staged replacements or
    /// superseded rows.
    pub fn count_recall_index_excluding_merge_staged(&self, query: &RecallIndexQuery) -> SubstrateResult<usize> {
        let mut query = query.clone();
        query.exclude_merge_non_servable = true;
        self.count_recall_index(&query)
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
        EventKind::MetadataAmended { .. } => "metadata_amended",
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
        EventKind::MergeApplied { .. } => "merge_applied",
        EventKind::MergeRolledBack { .. } => "merge_rolled_back",
    }
}

fn event_memory_id(kind: &EventKind) -> Option<&str> {
    match kind {
        EventKind::WriteCommitted { id, .. }
        | EventKind::EncryptedWriteCommitted { id, .. }
        | EventKind::MetadataAmended { id, .. }
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
        EventKind::MergeApplied { replacement_id, .. } | EventKind::MergeRolledBack { replacement_id, .. } => {
            Some(replacement_id.as_str())
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::Index;
    use crate::index::chunk_memory;
    use crate::index::fts::RELAXED_RANK_OFFSET;
    use crate::index::open_index;
    use crate::model::{
        Author, AuthorKind, Frontmatter, Memory, MemoryId, MemoryStatus, MemoryType, RepoPath, RetrievalPolicy, Scope,
        Sensitivity, Source, SourceKind, TrustLevel, WritePolicy,
    };

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
            assert_eq!(hit.rank, strict_len + idx + 1 + RELAXED_RANK_OFFSET);
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
                abstraction: None,
                cues: Vec::new(),
                extras: std::collections::BTreeMap::new(),
            },
            body: "bm25 relaxed fallback body".to_string(),
            path: Some(RepoPath::new(format!("agent/patterns/{id}.md"))),
        }
    }
}
