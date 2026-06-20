//! Hybrid-retrieval scoring rows and the chunk → memory collapse helpers.
//!
//! Holds the lane-local BM25/vector row shapes, their `FromRow` decoders, the
//! recency/precedence comparators, the chunk → memory collapse helper shared by
//! the BM25 and vector lanes, and the L2-distance → cosine conversion.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use rusqlite::{params_from_iter, Connection};

use crate::model::HybridMemoryCandidate;

use super::util::parse_index_time;
use super::{bucketed_in_clause_width, sql_placeholders};

pub(super) struct Bm25ChunkHit {
    pub(super) memory_id: String,
    pub(super) text: String,
    pub(super) chunk_rowid: i64,
    pub(super) score: f64,
    pub(super) recency_at: Option<DateTime<Utc>>,
}

pub(super) struct Bm25MemoryRank {
    pub(super) memory_id: String,
    pub(super) text: String,
    pub(super) rank: usize,
    pub(super) recency_at: Option<DateTime<Utc>>,
}

pub(super) struct VectorMemoryScore {
    pub(super) memory_id: String,
    pub(super) text: String,
    pub(super) cosine_similarity: f32,
    pub(super) recency_at: Option<DateTime<Utc>>,
}

/// Text-free nearest-chunk reference: the over-fetch projection used by the
/// vector KNN lane before it knows which chunks survive collapse/truncation.
/// Carries only the columns needed to pick one nearest chunk per memory; text is
/// fetched separately for survivors via [`chunk_texts_by_rowid`].
pub(super) struct VectorChunkRef {
    pub(super) memory_id: String,
    pub(super) chunk_rowid: i64,
    pub(super) distance: f64,
    pub(super) recency_at: Option<DateTime<Utc>>,
}

pub(super) fn bm25_chunk_hit_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Bm25ChunkHit> {
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

pub(super) fn vector_chunk_ref_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<VectorChunkRef> {
    let updated_at: String = row.get(3)?;
    let observed_at: Option<String> = row.get(4)?;
    Ok(VectorChunkRef {
        memory_id: row.get(0)?,
        chunk_rowid: row.get(1)?,
        distance: row.get(2)?,
        recency_at: memory_recency_at(&updated_at, observed_at.as_deref()),
    })
}

/// Fetch chunk text for the given `chunk_rowid`s, keyed by rowid. Used to hydrate
/// the surviving nearest chunks after the vector KNN lane collapses and truncates
/// its text-free over-fetch. Reuses the bucketed `IN (...)` plan cache so each
/// call maps to one of a handful of cached statements.
pub(super) fn chunk_texts_by_rowid(connection: &Connection, rowids: &[i64]) -> rusqlite::Result<BTreeMap<i64, String>> {
    /// Rowid `IN (...)` batch size for survivor chunk-text fetch after KNN.
    const CHUNK_TEXT_FETCH_BATCH: usize = 256;

    let mut texts = BTreeMap::new();
    if rowids.is_empty() {
        return Ok(texts);
    }
    for chunk in rowids.chunks(CHUNK_TEXT_FETCH_BATCH) {
        let width = bucketed_in_clause_width(chunk.len());
        let sql =
            format!("SELECT chunk_rowid, text FROM memory_chunks WHERE chunk_rowid IN ({})", sql_placeholders(width));
        let mut stmt = connection.prepare_cached(&sql)?;
        // Pad to the bucketed width by repeating the first rowid — `IN` is set
        // membership, so the duplicate matches the same row and adds nothing.
        let first = chunk[0];
        let padded = chunk.iter().copied().chain(std::iter::repeat_n(first, width - chunk.len()));
        let mut rows = stmt.query(params_from_iter(padded))?;
        while let Some(row) = rows.next()? {
            texts.insert(row.get::<_, i64>(0)?, row.get::<_, String>(1)?);
        }
    }
    Ok(texts)
}

pub(super) fn memory_recency_at(updated_at: &str, observed_at: Option<&str>) -> Option<DateTime<Utc>> {
    let updated = parse_index_time(updated_at).ok()?;
    let observed = observed_at.and_then(|value| parse_index_time(value).ok());
    Some(match observed {
        Some(observed) if observed > updated => observed,
        _ => updated,
    })
}

pub(super) fn later_recency_at(left: Option<DateTime<Utc>>, right: Option<DateTime<Utc>>) -> Option<DateTime<Utc>> {
    match (left, right) {
        (Some(left), Some(right)) => Some(if left > right { left } else { right }),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

/// Collapse per-chunk hits to one nearest chunk per memory, then sort.
///
/// Shared by the BM25 and vector lanes (which differ only in their row type,
/// precedence predicate, and final sort key): each memory keeps the hit for
/// which `precedes(candidate, current_best)` holds, and the survivors are
/// returned sorted by `order`. Callers truncate to their lane limit afterward.
pub(super) fn collapse_nearest_chunk_per_memory<H>(
    rows: Vec<H>,
    memory_id: impl Fn(&H) -> &str,
    precedes: impl Fn(&H, &H) -> bool,
    order: impl Fn(&H, &H) -> std::cmp::Ordering,
) -> Vec<H> {
    let mut best_by_memory = BTreeMap::new();
    for hit in rows {
        let key = memory_id(&hit).to_string();
        match best_by_memory.get(&key) {
            Some(best) if !precedes(&hit, best) => {}
            _ => {
                best_by_memory.insert(key, hit);
            }
        }
    }

    let mut collapsed: Vec<_> = best_by_memory.into_values().collect();
    collapsed.sort_by(order);
    collapsed
}

pub(super) fn collapse_bm25_memory_hits(rows: Vec<Bm25ChunkHit>) -> Vec<Bm25ChunkHit> {
    collapse_nearest_chunk_per_memory(
        rows,
        |hit| hit.memory_id.as_str(),
        bm25_chunk_precedes,
        |left, right| left.score.total_cmp(&right.score).then_with(|| left.memory_id.cmp(&right.memory_id)),
    )
}

/// Collapse the vector KNN over-fetch to one nearest chunk per memory, ordered by
/// ascending distance then memory id. Callers truncate to the lane limit after.
pub(super) fn collapse_vector_chunk_refs(rows: Vec<VectorChunkRef>) -> Vec<VectorChunkRef> {
    collapse_nearest_chunk_per_memory(
        rows,
        |hit| hit.memory_id.as_str(),
        vector_chunk_ref_precedes,
        |left, right| left.distance.total_cmp(&right.distance).then_with(|| left.memory_id.cmp(&right.memory_id)),
    )
}

fn bm25_chunk_precedes(left: &Bm25ChunkHit, right: &Bm25ChunkHit) -> bool {
    left.score.total_cmp(&right.score).then_with(|| left.chunk_rowid.cmp(&right.chunk_rowid)).is_lt()
}

fn vector_chunk_ref_precedes(left: &VectorChunkRef, right: &VectorChunkRef) -> bool {
    left.distance.total_cmp(&right.distance).then_with(|| left.chunk_rowid.cmp(&right.chunk_rowid)).is_lt()
}

pub(super) fn compare_hybrid_candidates(
    left: &HybridMemoryCandidate,
    right: &HybridMemoryCandidate,
) -> std::cmp::Ordering {
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
pub(super) fn cosine_from_l2_distance(distance: f64) -> f32 {
    (1.0 - (distance * distance) / 2.0).clamp(-1.0, 1.0) as f32
}
