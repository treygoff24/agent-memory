use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use memory_substrate::{
    index::{open_index, Index},
    MemoryStatus, RecallIndexRow, Sensitivity, Substrate, SubstrateResult,
};
use rusqlite::params_from_iter;

use crate::protocol::ComponentScores;
use crate::reality_check::types::{ScoreFacts, ScoreWeights, ScoredMemory, ScoringConfig};

const SQL_PARAM_CHUNK_SIZE: usize = 500;

pub fn score_memories(
    pool: &[RecallIndexRow],
    substrate: &Substrate,
    config: &ScoringConfig,
) -> SubstrateResult<Vec<ScoredMemory>> {
    score_memories_at(pool, substrate, config, Utc::now())
}

pub fn score_memories_at(
    pool: &[RecallIndexRow],
    substrate: &Substrate,
    config: &ScoringConfig,
    now: DateTime<Utc>,
) -> SubstrateResult<Vec<ScoredMemory>> {
    let candidates = pool.iter().filter(|row| is_scoring_candidate(row)).collect::<Vec<_>>();
    if candidates.is_empty() {
        return Ok(Vec::new());
    }

    let index = Index::new(open_index(&substrate.roots().runtime.join("index.sqlite"))?);
    let candidate_ids = candidates.iter().map(|row| row.id.as_str()).collect::<Vec<_>>();
    let recall_counts = recall_counts_30d(&index, &candidate_ids, now)?;
    let static_fields_by_id = indexed_static_fields_by_id(&index, &candidates)?;
    let distinct_sources_by_id = distinct_sources_by_id(&index, &candidate_ids)?;
    let max_recall = candidates
        .iter()
        .map(|row| recall_counts.get(row.id.as_str()).map_or(0, |summary| summary.count))
        .max()
        .unwrap_or(0);

    let weights = config.weights.normalized_or_default();
    let mut scored = Vec::with_capacity(candidates.len());
    for row in candidates {
        let static_fields = match static_fields_by_id.get(row.id.as_str()) {
            Some(fields) => *fields,
            None => return Err(rusqlite::Error::QueryReturnedNoRows.into()),
        };
        let recall = recall_counts.get(row.id.as_str()).copied().unwrap_or_default();
        let facts = ScoreFacts {
            recall_count_30d: recall.count,
            last_recalled_at: recall.last_recalled_at,
            last_observed_at: static_fields.last_observed_at,
            original_confidence: static_fields.original_confidence,
            distinct_sources: distinct_sources_by_id.get(row.id.as_str()).copied().unwrap_or(0),
            max_recall_30d_active: max_recall,
            encrypted: static_fields.encrypted,
        };
        let component_scores = component_scores(row, facts, now);
        let score = bounded_score(component_scores.clone(), weights);
        scored.push(ScoredMemory::from_row(row, score, component_scores, facts));
    }

    scored.sort_by(compare_scored_memories);
    Ok(take_top_with_pins(scored, config.top_n))
}

pub fn days_since_observed_norm(last_observed_at: DateTime<Utc>, now: DateTime<Utc>) -> f64 {
    let elapsed_days = now.signed_duration_since(last_observed_at).num_days().max(0) as f64;
    (elapsed_days / 90.0).min(1.0)
}

pub fn recall_frequency_norm(recall_count_30d: u32, max_recall_30d_active: u32) -> f64 {
    f64::from(recall_count_30d) / f64::from(max_recall_30d_active.max(1))
}

pub fn cross_source_corroboration(distinct_sources: u32) -> f64 {
    if distinct_sources >= 2 {
        1.0
    } else {
        0.0
    }
}

pub fn confidence_decay(original_confidence: Option<f64>, current_confidence: f64) -> f64 {
    original_confidence.map_or(0.0, |baseline| (baseline - current_confidence).max(0.0))
}

pub fn sensitivity_weight(sensitivity: Sensitivity) -> f64 {
    match sensitivity {
        Sensitivity::Public => 0.0,
        Sensitivity::Internal => 0.3,
        Sensitivity::Confidential => 0.6,
        Sensitivity::Personal => 1.0,
    }
}

fn is_scoring_candidate(row: &RecallIndexRow) -> bool {
    matches!(row.status, MemoryStatus::Active | MemoryStatus::Pinned) && row.passive_recall
}

fn component_scores(row: &RecallIndexRow, facts: ScoreFacts, now: DateTime<Utc>) -> ComponentScores {
    ComponentScores {
        days_since_observed_norm: days_since_observed_norm(facts.last_observed_at, now),
        recall_frequency_norm: recall_frequency_norm(facts.recall_count_30d, facts.max_recall_30d_active),
        cross_source_corroboration: cross_source_corroboration(facts.distinct_sources),
        confidence_decay: confidence_decay(facts.original_confidence, row.confidence),
        sensitivity_weight: sensitivity_weight(row.sensitivity),
    }
}

fn bounded_score(component_scores: ComponentScores, weights: ScoreWeights) -> f64 {
    let raw = weights.staleness * component_scores.days_since_observed_norm
        + weights.recall_frequency * (1.0 - component_scores.recall_frequency_norm)
        + weights.cross_source_corroboration * (1.0 - component_scores.cross_source_corroboration)
        + weights.confidence_decay * component_scores.confidence_decay
        + weights.sensitivity * component_scores.sensitivity_weight;
    raw.clamp(0.0, 1.0)
}

#[derive(Debug, Clone, Copy, Default)]
struct RecallSummary {
    count: u32,
    last_recalled_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy)]
struct IndexedStaticFields {
    last_observed_at: DateTime<Utc>,
    original_confidence: Option<f64>,
    encrypted: bool,
}

fn recall_counts_30d(
    index: &Index,
    memory_ids: &[&str],
    now: DateTime<Utc>,
) -> SubstrateResult<HashMap<String, RecallSummary>> {
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
            placeholders(chunk.len())
        );
        let mut statement = index.connection().prepare_cached(&query)?;
        let params = std::iter::once(cutoff.as_str()).chain(chunk.iter().copied());
        let rows = statement.query_map(params_from_iter(params), |row| {
            let memory_id: String = row.get(0)?;
            let count = row.get::<_, i64>(1)? as u32;
            let last_recalled_at = parse_optional_time(row.get::<_, Option<String>>(2)?);
            Ok((memory_id, RecallSummary { count, last_recalled_at }))
        })?;

        for row in rows {
            let (memory_id, summary) = row?;
            summaries.insert(memory_id, summary);
        }
    }
    Ok(summaries)
}

fn indexed_static_fields_by_id(
    index: &Index,
    candidates: &[&RecallIndexRow],
) -> SubstrateResult<HashMap<String, IndexedStaticFields>> {
    let fallback_updated_at = candidates.iter().map(|row| (row.id.as_str(), row.updated_at)).collect::<HashMap<_, _>>();
    let memory_ids = candidates.iter().map(|row| row.id.as_str()).collect::<Vec<_>>();
    let mut fields = HashMap::with_capacity(candidates.len());

    for chunk in memory_ids.chunks(SQL_PARAM_CHUNK_SIZE) {
        let query = format!(
            "SELECT id, path, COALESCE(observed_at, created_at), original_confidence, metadata_only
             FROM memories
             WHERE id IN ({})",
            placeholders(chunk.len())
        );
        let mut statement = index.connection().prepare_cached(&query)?;
        let rows = statement.query_map(params_from_iter(chunk.iter().copied()), |db_row| {
            let id: String = db_row.get(0)?;
            let path: String = db_row.get(1)?;
            let observed_at: String = db_row.get(2)?;
            let original_confidence = db_row.get(3)?;
            let metadata_only = db_row.get::<_, i64>(4)? != 0;
            Ok((id, path, observed_at, original_confidence, metadata_only))
        })?;

        for row in rows {
            let (id, path, observed_at, original_confidence, metadata_only) = row?;
            let fallback = fallback_updated_at.get(id.as_str()).copied().ok_or(rusqlite::Error::QueryReturnedNoRows)?;
            fields.insert(
                id,
                IndexedStaticFields {
                    last_observed_at: parse_time(&observed_at).unwrap_or(fallback),
                    original_confidence,
                    encrypted: metadata_only || path.starts_with("encrypted/"),
                },
            );
        }
    }

    Ok(fields)
}

fn distinct_sources_by_id(index: &Index, memory_ids: &[&str]) -> SubstrateResult<HashMap<String, u32>> {
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
            placeholders(chunk.len())
        );
        let mut statement = index.connection().prepare_cached(&query)?;
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

fn compare_scored_memories(left: &ScoredMemory, right: &ScoredMemory) -> std::cmp::Ordering {
    right
        .status
        .eq(&MemoryStatus::Pinned)
        .cmp(&left.status.eq(&MemoryStatus::Pinned))
        .then_with(|| right.score.total_cmp(&left.score))
        .then_with(|| left.memory_id.cmp(&right.memory_id))
}

fn take_top_with_pins(scored: Vec<ScoredMemory>, top_n: usize) -> Vec<ScoredMemory> {
    if top_n == 0 {
        return Vec::new();
    }
    scored.into_iter().take(top_n).collect()
}

fn parse_optional_time(value: Option<String>) -> Option<DateTime<Utc>> {
    value.as_deref().and_then(parse_time)
}

fn parse_time(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value).ok().map(|time| time.with_timezone(&Utc))
}

fn placeholders(count: usize) -> String {
    debug_assert!(count > 0);
    std::iter::repeat_n("?", count).collect::<Vec<_>>().join(",")
}
