//! Derived-index query surfaces: memory/recall queries, count/aggregation
//! scans, chunk + hybrid + KNN search, and the embedding-table family.

use super::*;

impl Substrate {
    /// Read-only size summary for API-lane cost estimates. Only plaintext-
    /// eligible (public/internal) chunks are counted — an upper bound on what a
    /// full re-embed would send, not a per-triple pending count.
    pub async fn api_lane_corpus_stats(&self) -> SubstrateResult<(u64, u64)> {
        let index = Arc::clone(&self.index);
        tokio::task::spawn_blocking(move || {
            let index = lock_index(&index);
            let eligible = Sensitivity::api_lane_eligible_db_strs();
            let sql = format!(
                "SELECT COUNT(*), COALESCE(SUM(length(mc.text)), 0) \
                 FROM memory_chunks mc JOIN memories m ON m.id = mc.memory_id \
                 WHERE m.sensitivity IN ({})",
                crate::index::sql_placeholders(eligible.len())
            );
            index
                .connection()
                .query_row(&sql, rusqlite::params_from_iter(eligible.iter()), |row| {
                    Ok((row.get::<_, i64>(0)? as u64, row.get::<_, i64>(1)? as u64))
                })
                .map_err(Into::into)
        })
        .await
        .map_err(|err| OpenError::InvalidRoots(format!("API-lane corpus stats task panicked: {err}")))?
    }
    /// Rebuild the derived index from files (backs the public `memoryd reindex`).
    ///
    /// Unconditional full reindex: clears all plaintext rows and rebuilds from
    /// every Markdown file. Startup uses the cheaper incremental sweep instead
    /// (see `incremental_reindex_at_open`); this is the explicit operator
    /// "rebuild everything" path.
    pub async fn reindex(&self) -> SubstrateResult<usize> {
        let mut index = lock_index(&self.index);
        full_reindex_from_repo(&self.roots.repo, &mut index)
            .map_err(|err| SubstrateError::from(OpenError::OperatorRepairRequired(err.to_string())))
    }

    /// Query memories.
    pub async fn query_memory(&self, query: MemoryQuery) -> SubstrateResult<Vec<QueryResult>> {
        lock_index(&self.index).query_memory(&query)
    }

    /// Query recall-index rows without hydrating memory envelopes.
    ///
    /// The SQLite read (a `memories` table scan that can materialize every
    /// active/pinned row in a namespace) runs on a blocking thread via
    /// `spawn_blocking`, matching the strength-hydration and disk-read paths, so
    /// the heaviest recall query does not occupy a tokio worker under load. The
    /// mutex guard is taken and released entirely inside the blocking closure;
    /// no lock is held across an `.await`.
    pub async fn query_recall_index(&self, query: RecallIndexQuery) -> SubstrateResult<Vec<RecallIndexRow>> {
        let index = Arc::clone(&self.index);
        tokio::task::spawn_blocking(move || lock_index(&index).query_recall_index(&query))
            .await
            .map_err(|err| OpenError::InvalidRoots(format!("recall index query task panicked: {err}")))?
    }

    /// Count recall-index rows matching `query` via an index-only `COUNT(*)`,
    /// without marshalling rows or hydrating auxiliary tables. The predicate is
    /// identical to [`Self::query_recall_index`], so this returns the same value
    /// as that call's `rows.len()` for the same query.
    ///
    /// Runs the index scan on a blocking thread (see [`Self::query_recall_index`]).
    pub async fn count_recall_index(&self, query: RecallIndexQuery) -> SubstrateResult<usize> {
        let index = Arc::clone(&self.index);
        tokio::task::spawn_blocking(move || lock_index(&index).count_recall_index(&query))
            .await
            .map_err(|err| OpenError::InvalidRoots(format!("recall index count task panicked: {err}")))?
    }

    /// Project the entities (with aliases) for a set of memory ids in one
    /// batched index query, without reading or parsing canonical files.
    ///
    /// Stream I uses this for claim-lock entity-intersection checks on the
    /// peer-heartbeat path. Ids absent from the index are omitted from the map.
    pub async fn entities_for_memories(
        &self,
        ids: &[String],
    ) -> SubstrateResult<BTreeMap<String, Vec<crate::model::Entity>>> {
        lock_index(&self.index).entities_for_memories(ids)
    }

    /// Query recall-index rows, including encrypted metadata-only rows.
    ///
    /// Runs the index scan on a blocking thread (see [`Self::query_recall_index`])
    /// because the predicate can materialize an entire namespace's rows.
    pub async fn query_recall_index_including_metadata_only(
        &self,
        query: RecallIndexQuery,
    ) -> SubstrateResult<Vec<RecallIndexRow>> {
        let index = Arc::clone(&self.index);
        tokio::task::spawn_blocking(move || {
            index
                .lock()
                .map_err(|err| OpenError::InvalidRoots(err.to_string()))?
                .query_recall_index_including_metadata_only(&query)
        })
        .await
        .map_err(|err| OpenError::InvalidRoots(format!("recall index query task panicked: {err}")))?
    }

    /// Count memories grouped by lifecycle status via a single index-only scan
    /// on the derived index, instead of materializing rows per status. Returns
    /// one `(status, count)` pair per distinct status present.
    ///
    /// Runs the scan on a blocking thread (see [`Self::query_recall_index`]).
    pub async fn count_memories_by_status(&self) -> SubstrateResult<Vec<(MemoryStatus, u64)>> {
        let index = Arc::clone(&self.index);
        tokio::task::spawn_blocking(move || lock_index(&index).count_by_status())
            .await
            .map_err(|err| OpenError::InvalidRoots(format!("count-by-status task panicked: {err}")))?
    }

    /// Serve the review queue from the derived index: the total count of
    /// review-queue members plus a bounded, newest-first slice carrying exactly
    /// the fields the response renders. Replaces the prior full repo walk +
    /// per-file frontmatter parse on this repeatedly-polled inbox surface.
    ///
    /// Runs the scan on a blocking thread (see [`Self::query_recall_index`]).
    pub async fn review_queue(&self, limit: usize) -> SubstrateResult<crate::model::ReviewQueuePage> {
        let index = Arc::clone(&self.index);
        tokio::task::spawn_blocking(move || lock_index(&index).review_queue(limit))
            .await
            .map_err(|err| OpenError::InvalidRoots(format!("review-queue task panicked: {err}")))?
    }

    /// Count memories grouped by `(scope, canonical_namespace_id)` for namespace
    /// aggregation, without hydrating per-row entities/tags/aliases.
    ///
    /// Runs the scan on a blocking thread (see [`Self::query_recall_index`]).
    pub async fn namespace_counts(&self) -> SubstrateResult<Vec<(Scope, Option<String>, u64)>> {
        let index = Arc::clone(&self.index);
        tokio::task::spawn_blocking(move || lock_index(&index).namespace_counts())
            .await
            .map_err(|err| OpenError::InvalidRoots(format!("namespace-counts task panicked: {err}")))?
    }

    /// Stream every indexed entity (with aliases) as `(memory_id, Entity)`
    /// pairs for entity-graph aggregation, reading only the entity tables.
    ///
    /// Runs the scan on a blocking thread (see [`Self::query_recall_index`]).
    pub async fn entity_index_rows(&self) -> SubstrateResult<Vec<(MemoryId, crate::model::Entity)>> {
        let index = Arc::clone(&self.index);
        tokio::task::spawn_blocking(move || lock_index(&index).entity_index_rows())
            .await
            .map_err(|err| OpenError::InvalidRoots(format!("entity-index task panicked: {err}")))?
    }

    /// Recent `recall_hit` events joined to memory summaries, newest-first,
    /// served from the existing long-lived index connection (no per-call
    /// `Connection::open`). Each tuple is
    /// `(event_id, device, seq, memory_id, recalled_at, summary)`.
    #[allow(clippy::type_complexity)]
    pub fn recent_recall_hits(
        &self,
        since: Option<DateTime<Utc>>,
        limit: usize,
    ) -> SubstrateResult<Vec<(String, String, i64, String, String, Option<String>)>> {
        lock_index(&self.index).recent_recall_hits(since, limit)
    }

    /// Query chunks.
    pub async fn query_chunks(&self, query: ChunkQuery) -> SubstrateResult<Vec<ChunkResult>> {
        if let (Some(triple), Some(vector)) = (query.triple.as_ref(), query.vector.as_ref()) {
            let index = lock_index(&self.index);
            return Ok(index.query_vector_chunks(triple, vector, 20)?);
        }
        let Some(text) = query.text else {
            return Ok(Vec::new());
        };
        let index = lock_index(&self.index);
        Ok(index.query_chunks(&text)?)
    }

    /// Query recall-eligible chunks through BM25 and, optionally, a structurally
    /// complete vector lane, collapsed to one candidate per memory.
    ///
    /// `limit` applies independently to each lane before the union is returned.
    /// This method does **not** fuse or RRF-rank candidates; memoryd owns fusion.
    /// A supplied vector lane always carries the exact embedding triple, and a
    /// missing/dropped vector table returns [`VectorError::UnknownEmbeddingTriple`]
    /// rather than silently falling back to BM25-only results.
    pub async fn query_hybrid_chunks(
        &self,
        text: &str,
        vector_query: Option<HybridVectorQuery<'_>>,
        limit: usize,
    ) -> Result<Vec<HybridMemoryCandidate>, VectorError> {
        // Own the borrowed query inputs so the FTS + KNN scan can run on a
        // blocking thread (the borrowed `text`/vector do not outlive this call).
        // Keeps the heaviest recall lane off the tokio worker pool.
        let index = Arc::clone(&self.index);
        let text = text.to_owned();
        let vector_query = vector_query.map(|q| (q.triple.clone(), q.vector.to_vec()));
        tokio::task::spawn_blocking(move || {
            let vector_query = vector_query.as_ref().map(|(triple, vector)| HybridVectorQuery { triple, vector });
            index
                .lock()
                .map_err(|err| VectorError::IndexUnavailable(format!("index mutex poisoned: {err}")))?
                .query_hybrid_chunks(&text, vector_query, limit)
        })
        .await
        .map_err(|err| VectorError::IndexUnavailable(format!("hybrid chunk query task panicked: {err}")))?
    }

    /// KNN over a triple's vector table, collapsed to one row per active,
    /// in-scope memory.
    ///
    /// The substrate seam for governance contradiction detection: given a
    /// candidate's query vector, return the nearest active memories whose scope
    /// is in `scopes` (the candidate's governance namespace). See
    /// [`crate::index::Index::knn_active_memories`] for the filtering and
    /// distance→similarity contract.
    ///
    /// Per invariant 3, a triple with no vector table is
    /// [`VectorError::UnknownEmbeddingTriple`], never a silent empty result, so
    /// the caller can distinguish "no neighbours" from "no embedding backend".
    #[allow(clippy::too_many_arguments)]
    pub async fn knn_active_memories(
        &self,
        triple: &EmbeddingTriple,
        vector: &[f32],
        scopes: &[Scope],
        limit: usize,
    ) -> Result<Vec<crate::model::SimilarMemory>, VectorError> {
        self.index
            .lock()
            .map_err(|err| VectorError::IndexUnavailable(format!("index mutex poisoned: {err}")))?
            .knn_active_memories(triple, vector, scopes, limit)
    }

    /// Update embedding for a chunk.
    pub async fn update_embedding(&self, update: EmbeddingUpdate) -> Result<(), VectorError> {
        let index = Arc::clone(&self.index);
        tokio::task::spawn_blocking(move || {
            index
                .lock()
                .map_err(|err| VectorError::IndexUnavailable(format!("index mutex poisoned: {err}")))?
                .update_embedding(&update)
        })
        .await
        .map_err(|err| VectorError::IndexUnavailable(format!("update-embedding task panicked: {err}")))?
    }

    /// Apply a batch of embedding updates, returning one result per input in
    /// positional order.
    ///
    /// Behaviorally identical to calling [`Self::update_embedding`] once per
    /// update — each chunk is validated and committed independently — but the
    /// per-chunk metadata/job-resolution writes share a single SQLite
    /// transaction, so a bulk reindex pays one WAL commit per batch instead of
    /// one per chunk. The drain worker uses this to amortize the embedding fill.
    pub async fn update_embeddings_batch(
        &self,
        updates: Vec<EmbeddingUpdate>,
    ) -> Result<Vec<Result<(), VectorError>>, VectorError> {
        let index = Arc::clone(&self.index);
        tokio::task::spawn_blocking(move || {
            Ok(index
                .lock()
                .map_err(|err| VectorError::IndexUnavailable(format!("index mutex poisoned: {err}")))?
                .update_embeddings_batch(&updates))
        })
        .await
        .map_err(|err| VectorError::IndexUnavailable(format!("update-embeddings-batch task panicked: {err}")))?
    }

    /// The active embedding triple this substrate was opened with.
    ///
    /// `(provider, model_ref, dimension)` is the unit of vector-table identity
    /// (spec §10.2.2 #9); the background embedding worker reads it to know which
    /// table to write vectors into and which jobs to drain.
    pub fn active_embedding_triple(&self) -> Result<EmbeddingTriple, VectorError> {
        Ok(self
            .index
            .lock()
            .map_err(|err| VectorError::IndexUnavailable(format!("index mutex poisoned: {err}")))?
            .active_embedding()
            .clone())
    }

    /// Drain up to `limit` pending embedding jobs for the active triple, each
    /// paired with the chunk text to embed. See
    /// [`crate::index::Index::pending_embedding_jobs`].
    pub async fn pending_embedding_jobs(
        &self,
        limit: usize,
        eligibility: crate::model::EmbeddingLaneEligibility,
    ) -> Result<Vec<crate::model::PendingEmbeddingJob>, VectorError> {
        self.index
            .lock()
            .map_err(|err| VectorError::IndexUnavailable(format!("index mutex poisoned: {err}")))?
            .pending_embedding_jobs(limit, eligibility)
    }

    /// Count pending embedding jobs for the active triple (doctor backlog).
    pub fn pending_embedding_job_count(
        &self,
        eligibility: crate::model::EmbeddingLaneEligibility,
    ) -> Result<usize, VectorError> {
        let index = lock_index(&self.index);
        let triple = index.active_embedding().clone();
        crate::index::reconcile_pending_jobs(index.connection(), &triple, eligibility).map_err(Into::into)
    }

    /// Count pending embedding jobs held local-only by lane eligibility.
    pub fn held_local_embedding_job_count(
        &self,
        eligibility: crate::model::EmbeddingLaneEligibility,
    ) -> Result<usize, VectorError> {
        let index = lock_index(&self.index);
        let triple = index.active_embedding().clone();
        crate::index::held_local_embedding_jobs(index.connection(), &triple, eligibility).map_err(Into::into)
    }

    /// Drop embedding model and return the structured report (spec §16.4, B-API-4).
    ///
    /// Phase 5 surface: returns counts for each derived table affected so callers
    /// can confirm the drop matched their expectation.
    ///
    /// Delegates to `Index::drop_embedding_model_report` (index/query.rs) which executes
    /// all three DELETEs and the `table_exists` check atomically on the same connection, avoiding
    /// the TOCTOU window that existed when pre-counts were fetched as separate SELECT queries
    /// before the DELETE. The `table_dropped` field now correctly reflects whether the
    /// per-triple vector table existed (not whether rows were deleted), matching the
    /// semantics callers should expect.
    pub async fn drop_embedding_model_report(&self, triple: EmbeddingTriple) -> Result<DropTripleReport, VectorError> {
        self.index
            .lock()
            .map_err(|err| VectorError::IndexUnavailable(format!("index mutex poisoned: {err}")))?
            .drop_embedding_model_report(&triple)
    }

    /// Count vectors for a triple.
    pub async fn vector_count(&self, triple: EmbeddingTriple) -> Result<usize, VectorError> {
        self.index
            .lock()
            .map_err(|err| VectorError::IndexUnavailable(format!("index mutex poisoned: {err}")))?
            .vector_count(&triple)
    }
}
