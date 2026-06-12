//! Shared usage computation (memory-dynamics-v0.1 §5).
//!
//! One usage *query*, two consumers (spec §1.1). Reality Check drift scoring
//! reads these raw inputs through an *inverse*-frequency normalization
//! ("validate what you haven't been using"); the strength term reads the same
//! inputs through a *direct*, log-scaled normalization ("trust what keeps proving
//! useful"). Both call the functions here — they share the query, never the curve.
//!
//! These functions, and the index-acquisition path they depend on, were moved out
//! of `reality_check/scoring.rs` so both consumers read through one connection
//! path. The behavior of each query is unchanged from the original; only the home
//! and the public names differ.

use std::collections::HashMap;
use std::path::Path;

use chrono::{DateTime, Duration, Utc};
use memory_substrate::index::{open_index, Index};
use memory_substrate::{Substrate, SubstrateResult};
use rusqlite::{params_from_iter, Connection};

/// SQLite has a default bound-parameter limit (`SQLITE_MAX_VARIABLE_NUMBER`);
/// chunk `IN (...)` lists well under it so large candidate pools never trip it.
const SQL_PARAM_CHUNK_SIZE: usize = 500;

/// Per-memory recall usage over the trailing 30 days.
///
/// `count` is the number of `recall_hit` events; `last_recalled_at` is the most
/// recent one. A memory with no recall hits in the window is absent from the map
/// (callers treat absence as `count = 0`, `last_recalled_at = None`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UsageSummary {
    pub count: u32,
    pub last_recalled_at: Option<DateTime<Utc>>,
}

/// Open the derived-index connection for a substrate, wrapped in [`Index`].
///
/// Both usage consumers (RC scoring and strength hydration) acquire the index
/// through this one path rather than constructing it inline, so the connection
/// semantics stay identical.
pub fn open_runtime_index(substrate: &Substrate) -> SubstrateResult<Index> {
    open_runtime_index_at(&substrate.roots().runtime)
}

/// Open the derived-index connection from a runtime root directly.
pub fn open_runtime_index_at(runtime_root: &Path) -> SubstrateResult<Index> {
    Ok(Index::new(open_index(&runtime_root.join("index.sqlite"))?))
}

/// Recall usage for a set of memory ids over the trailing 30 days from `now`.
///
/// This is the exact `events_log WHERE kind='recall_hit'` query that
/// `reality_check/scoring.rs::recall_counts_30d` shipped (chunked `IN`, covering
/// index `idx_events_log_kind_memory_ts`), moved here verbatim so both consumers
/// share it.
pub fn recall_usage_for(
    index: &Index,
    memory_ids: &[&str],
    now: DateTime<Utc>,
) -> SubstrateResult<HashMap<String, UsageSummary>> {
    recall_usage_for_conn(index.connection(), memory_ids, now)
}

/// As [`recall_usage_for`], reading through a raw [`Connection`] (callers that
/// already hold a connection rather than an [`Index`] handle).
pub fn recall_usage_for_conn(
    connection: &Connection,
    memory_ids: &[&str],
    now: DateTime<Utc>,
) -> SubstrateResult<HashMap<String, UsageSummary>> {
    if memory_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let cutoff = (now - Duration::days(30)).to_rfc3339();
    let mut summaries = HashMap::with_capacity(memory_ids.len());
    for chunk in memory_ids.chunks(SQL_PARAM_CHUNK_SIZE) {
        let query = format!(
            "SELECT memory_id, COUNT(*), MAX(ts)
             FROM events_log
             WHERE kind = 'recall_hit'
               AND memory_id IS NOT NULL
               AND ts > ?
               AND memory_id IN ({})
             GROUP BY memory_id",
            crate::util::sql_placeholders(chunk.len())
        );
        let mut statement = connection.prepare_cached(&query)?;
        let params = std::iter::once(cutoff.as_str()).chain(chunk.iter().copied());
        let rows = statement.query_map(params_from_iter(params), |row| {
            let memory_id: String = row.get(0)?;
            let count = row.get::<_, i64>(1)? as u32;
            let last_recalled_at = parse_optional_time(row.get::<_, Option<String>>(2)?);
            Ok((memory_id, UsageSummary { count, last_recalled_at }))
        })?;

        for row in rows {
            let (memory_id, summary) = row?;
            summaries.insert(memory_id, summary);
        }
    }
    Ok(summaries)
}

/// Distinct `source_harness` count across each memory's supersession chain.
///
/// The depth-bounded recursive CTE shipped in
/// `reality_check/scoring.rs::distinct_sources_by_id`, moved here verbatim. A
/// count `>= 2` is the binary cross-source corroboration signal (spec §2).
pub fn distinct_sources_for(index: &Index, memory_ids: &[&str]) -> SubstrateResult<HashMap<String, u32>> {
    distinct_sources_for_conn(index.connection(), memory_ids)
}

/// As [`distinct_sources_for`], reading through a raw [`Connection`].
pub fn distinct_sources_for_conn(
    connection: &Connection,
    memory_ids: &[&str],
) -> SubstrateResult<HashMap<String, u32>> {
    if memory_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let mut counts = HashMap::with_capacity(memory_ids.len());
    for chunk in memory_ids.chunks(SQL_PARAM_CHUNK_SIZE) {
        let query = format!(
            "WITH RECURSIVE chain(root_id, memory_id, depth) AS (
               SELECT id, id, 0 FROM memories WHERE id IN ({})
               UNION ALL
               SELECT c.root_id, ms.supersedes_id, c.depth + 1
                 FROM chain c
                 JOIN memory_supersession ms ON ms.memory_id = c.memory_id
                WHERE c.depth < 8
             )
             SELECT chain.root_id, COUNT(DISTINCT mem.source_harness)
               FROM chain
               JOIN memories mem ON chain.memory_id = mem.id
              GROUP BY chain.root_id",
            crate::util::sql_placeholders(chunk.len())
        );
        let mut statement = connection.prepare_cached(&query)?;
        let rows = statement.query_map(params_from_iter(chunk.iter().copied()), |db_row| {
            let id: String = db_row.get(0)?;
            let distinct_sources = db_row.get::<_, i64>(1)? as u32;
            Ok((id, distinct_sources))
        })?;

        for row in rows {
            let (id, count) = row?;
            counts.insert(id, count);
        }
    }

    Ok(counts)
}

fn parse_optional_time(value: Option<String>) -> Option<DateTime<Utc>> {
    value.as_deref().and_then(crate::util::parse_rfc3339_utc)
}
