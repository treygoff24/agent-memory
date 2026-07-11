use std::collections::HashMap;

use chrono::{DateTime, Utc};
use memory_substrate::{AbstractionVectorHit, CueVectorHit, HybridMemoryCandidate, HybridScoreBreakdown, MemoryId};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FourLaneWeights {
    pub chunk_vector: f64,
    pub bm25: f64,
    pub abstraction_vector: f64,
    pub cue_vector: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FourLaneFusionConfig {
    pub rrf_k: u32,
    pub weights: FourLaneWeights,
    pub recency_lambda: f64,
    pub recency_half_life_days: f64,
}

/// A hybrid recall candidate after Reciprocal Rank Fusion.
#[derive(Debug, Clone, PartialEq)]
pub struct FusedHybridCandidate {
    pub memory_id: MemoryId,
    pub text: String,
    pub score_breakdown: HybridScoreBreakdown,
    /// Pure RRF sum before the recency prior; retained for score-breakdown consumers.
    pub rrf_score: f64,
    /// `max(observed_at, updated_at)` carried from the substrate candidate.
    pub recency_at: Option<DateTime<Utc>>,
    /// Sort/display score: `rrf_score + recency_lambda * recency_norm`.
    pub final_score: f64,
}

/// Fuse BM25 and vector recall lanes with Reciprocal Rank Fusion.
///
/// Rank bases are 1-based throughout: substrate BM25 ranks are already 1-based,
/// and this helper derives 1-based vector ranks from descending cosine
/// similarity. A memory absent from a lane contributes nothing for that lane.
/// A continuous recency prior (`recency_lambda`, `recency_half_life_days`) may
/// nudge near-ties; remaining ties resolve deterministically by lexicographic
/// memory id.
pub fn fuse_rrf(
    candidates: Vec<HybridMemoryCandidate>,
    rrf_k: u32,
    recency_lambda: f64,
    recency_half_life_days: f64,
) -> Vec<FusedHybridCandidate> {
    // Borrow pre-pass: derive 1-based vector ranks before the consuming pass so
    // the owned candidates (each carrying a heap String `text` up to a full chunk)
    // can be moved — not cloned — into the fused output below.
    let mut vector_rank_by_id = HashMap::new();
    let mut vector_lane =
        candidates.iter().filter(|candidate| candidate.score_breakdown.cosine_similarity.is_some()).collect::<Vec<_>>();
    vector_lane.sort_by(|left, right| {
        let left_similarity = left.score_breakdown.cosine_similarity.unwrap_or(f32::NEG_INFINITY);
        let right_similarity = right.score_breakdown.cosine_similarity.unwrap_or(f32::NEG_INFINITY);
        right_similarity.total_cmp(&left_similarity).then_with(|| left.memory_id.as_str().cmp(right.memory_id.as_str()))
    });
    for (index, candidate) in vector_lane.into_iter().enumerate() {
        vector_rank_by_id.insert(candidate.memory_id.as_str().to_owned(), index + 1);
    }

    let k = f64::from(rrf_k);
    let mut fused = candidates
        .into_iter()
        .map(|candidate| {
            let bm25_score =
                candidate.score_breakdown.bm25_rank.map(|rank| reciprocal_rank_score(k, rank)).unwrap_or_default();
            let vector_score = vector_rank_by_id
                .get(candidate.memory_id.as_str())
                .copied()
                .map(|rank| reciprocal_rank_score(k, rank))
                .unwrap_or_default();
            FusedHybridCandidate {
                memory_id: candidate.memory_id,
                text: candidate.text,
                score_breakdown: candidate.score_breakdown,
                rrf_score: bm25_score + vector_score,
                recency_at: candidate.recency_at,
                final_score: 0.0,
            }
        })
        .collect::<Vec<_>>();

    apply_recency_prior_and_sort(&mut fused, recency_lambda, recency_half_life_days);
    fused
}

/// Fuse the four primitive retrieval lanes with weighted RRF.
///
/// Missing lanes contribute zero and never suppress healthy lanes. Cue rows are
/// collapsed to the owning memory's best raw rank before scoring, preventing
/// multiple cues for one memory from multiplying its contribution. Recency is
/// added only after the weighted RRF sum, so it cannot manufacture lane evidence.
/// Exact final-score ties resolve by lexicographic memory id.
pub fn fuse_four_lane_rrf(
    candidates: Vec<HybridMemoryCandidate>,
    abstractions: Vec<AbstractionVectorHit>,
    cues: Vec<CueVectorHit>,
    config: FourLaneFusionConfig,
) -> Vec<FusedHybridCandidate> {
    let mut by_id = candidates
        .into_iter()
        .map(|candidate| (candidate.memory_id.as_str().to_owned(), candidate))
        .collect::<HashMap<_, _>>();
    let chunk_ranks = ranked_chunk_ids(by_id.values());
    let abstraction_ranks = best_rank_by_memory(abstractions.iter().map(|hit| &hit.memory_id));
    let cue_ranks = best_rank_by_memory(cues.iter().map(|hit| &hit.memory_id));

    for hit in abstractions.iter().map(|hit| &hit.memory_id).chain(cues.iter().map(|hit| &hit.memory_id)) {
        by_id.entry(hit.as_str().to_owned()).or_insert_with(|| HybridMemoryCandidate {
            memory_id: hit.clone(),
            text: String::new(),
            score_breakdown: HybridScoreBreakdown::default(),
            recency_at: None,
        });
    }

    let k = f64::from(config.rrf_k);
    let mut fused = by_id
        .into_values()
        .map(|candidate| {
            let id = candidate.memory_id.as_str();
            let weighted = |rank: Option<usize>, weight: f64| {
                rank.map(|rank| weight * reciprocal_rank_score(k, rank)).unwrap_or_default()
            };
            let rrf_score = weighted(candidate.score_breakdown.bm25_rank, config.weights.bm25)
                + weighted(chunk_ranks.get(id).copied(), config.weights.chunk_vector)
                + weighted(abstraction_ranks.get(id).copied(), config.weights.abstraction_vector)
                + weighted(cue_ranks.get(id).copied(), config.weights.cue_vector);
            FusedHybridCandidate {
                memory_id: candidate.memory_id,
                text: candidate.text,
                score_breakdown: candidate.score_breakdown,
                rrf_score,
                recency_at: candidate.recency_at,
                final_score: 0.0,
            }
        })
        .collect::<Vec<_>>();
    apply_recency_prior_and_sort(&mut fused, config.recency_lambda, config.recency_half_life_days);
    fused
}

fn ranked_chunk_ids<'a>(candidates: impl Iterator<Item = &'a HybridMemoryCandidate>) -> HashMap<String, usize> {
    let mut ranked = candidates
        .filter_map(|candidate| candidate.score_breakdown.cosine_similarity.map(|score| (candidate, score)))
        .collect::<Vec<_>>();
    ranked.sort_by(|(left, left_score), (right, right_score)| {
        right_score.total_cmp(left_score).then_with(|| left.memory_id.as_str().cmp(right.memory_id.as_str()))
    });
    ranked
        .into_iter()
        .enumerate()
        .map(|(index, (candidate, _))| (candidate.memory_id.as_str().to_owned(), index + 1))
        .collect()
}

fn best_rank_by_memory<'a>(ids: impl Iterator<Item = &'a MemoryId>) -> HashMap<String, usize> {
    let mut ranks = HashMap::new();
    for (index, id) in ids.enumerate() {
        ranks.entry(id.as_str().to_owned()).or_insert(index + 1);
    }
    ranks
}

pub(crate) fn reciprocal_rank_score(k: f64, rank: usize) -> f64 {
    1.0 / (k + rank as f64)
}

pub(crate) fn apply_recency_prior_and_sort(
    candidates: &mut [FusedHybridCandidate],
    recency_lambda: f64,
    recency_half_life_days: f64,
) {
    let newest = candidates.iter().filter_map(|candidate| candidate.recency_at).max();
    for candidate in candidates.iter_mut() {
        let recency_norm = match (candidate.recency_at, newest) {
            (Some(recency_at), Some(newest)) => recency_norm(recency_at, newest, recency_half_life_days),
            _ => 0.0,
        };
        candidate.final_score = candidate.rrf_score + recency_lambda * recency_norm;
    }

    candidates.sort_by(|left, right| {
        right
            .final_score
            .total_cmp(&left.final_score)
            .then_with(|| left.memory_id.as_str().cmp(right.memory_id.as_str()))
    });
}

fn recency_norm(recency_at: DateTime<Utc>, newest: DateTime<Utc>, half_life_days: f64) -> f64 {
    if half_life_days <= 0.0 {
        return 0.0;
    }
    let age_secs = (newest - recency_at).num_seconds();
    if age_secs <= 0 {
        return 1.0;
    }
    let age_days = age_secs as f64 / 86_400.0;
    (-std::f64::consts::LN_2 * age_days / half_life_days).exp()
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn candidate(
        id: &str,
        bm25_rank: Option<usize>,
        cosine_similarity: Option<f32>,
        recency_at: Option<DateTime<Utc>>,
    ) -> HybridMemoryCandidate {
        HybridMemoryCandidate {
            memory_id: MemoryId::new(id),
            text: format!("text for {id}"),
            score_breakdown: HybridScoreBreakdown { bm25_rank, cosine_similarity },
            recency_at,
        }
    }

    fn utc(y: i32, m: u32, d: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, 0, 0, 0).unwrap()
    }

    fn four_lane_config() -> FourLaneFusionConfig {
        FourLaneFusionConfig {
            rrf_k: 60,
            weights: FourLaneWeights { chunk_vector: 1.0, bm25: 1.0, abstraction_vector: 2.0, cue_vector: 1.0 },
            recency_lambda: 0.0,
            recency_half_life_days: 90.0,
        }
    }

    #[test]
    fn four_lane_weights_abstraction_and_collapses_duplicate_cues() {
        let first = MemoryId::new("mem_20260610_0000000000000001_000001");
        let second = MemoryId::new("mem_20260610_0000000000000002_000002");
        let fused = fuse_four_lane_rrf(
            vec![candidate(first.as_str(), Some(1), None, None), candidate(second.as_str(), Some(2), None, None)],
            vec![AbstractionVectorHit { memory_id: second.clone(), distance: 0.1 }],
            vec![
                CueVectorHit { memory_id: first.clone(), ordinal: 0, distance: 0.1 },
                CueVectorHit { memory_id: first, ordinal: 1, distance: 0.2 },
                CueVectorHit { memory_id: second.clone(), ordinal: 0, distance: 0.3 },
            ],
            four_lane_config(),
        );

        assert_eq!(fused[0].memory_id, second);
        let first_score = fused[1].rrf_score;
        assert_eq!(first_score, reciprocal_rank_score(60.0, 1) * 2.0);
    }

    #[test]
    fn four_lane_missing_lanes_and_ties_are_deterministic() {
        let fused = fuse_four_lane_rrf(
            vec![
                candidate("mem_20260610_0000000000000002_000002", Some(1), None, None),
                candidate("mem_20260610_0000000000000001_000001", Some(1), None, None),
            ],
            Vec::new(),
            Vec::new(),
            four_lane_config(),
        );
        assert_eq!(fused[0].memory_id.as_str(), "mem_20260610_0000000000000001_000001");
    }

    #[test]
    fn fuses_two_one_based_rank_lanes() {
        let fused = fuse_rrf(
            vec![
                candidate("mem_20260610_0000000000000001_000001", Some(1), None, None),
                candidate("mem_20260610_0000000000000002_000002", Some(2), Some(0.9), None),
                candidate("mem_20260610_0000000000000003_000003", None, Some(0.95), None),
            ],
            60,
            0.0005,
            90.0,
        );

        assert_eq!(fused[0].memory_id.as_str(), "mem_20260610_0000000000000002_000002");
        assert_eq!(fused[0].text, "text for mem_20260610_0000000000000002_000002");
        assert_eq!(fused[1].memory_id.as_str(), "mem_20260610_0000000000000001_000001");
        assert_eq!(fused[2].memory_id.as_str(), "mem_20260610_0000000000000003_000003");
    }

    #[test]
    fn equal_scores_tie_break_by_memory_id() {
        let fused = fuse_rrf(
            vec![
                candidate("mem_20260610_0000000000000002_000002", Some(1), None, None),
                candidate("mem_20260610_0000000000000001_000001", None, Some(0.8), None),
            ],
            60,
            0.0005,
            90.0,
        );

        assert_eq!(fused[0].memory_id.as_str(), "mem_20260610_0000000000000001_000001");
        assert_eq!(fused[1].memory_id.as_str(), "mem_20260610_0000000000000002_000002");
    }

    #[test]
    fn zero_lambda_preserves_pure_rrf_order() {
        let fresh = utc(2026, 6, 1);
        let stale = utc(2026, 1, 1);
        let fused = fuse_rrf(
            vec![
                candidate("mem_20260610_0000000000000001_000001", Some(4), None, Some(stale)),
                candidate("mem_20260610_0000000000000002_000002", Some(5), None, Some(fresh)),
            ],
            60,
            0.0,
            90.0,
        );

        assert_eq!(fused[0].memory_id.as_str(), "mem_20260610_0000000000000001_000001");
        assert_eq!(fused[1].memory_id.as_str(), "mem_20260610_0000000000000002_000002");
        assert_eq!(fused[0].final_score, fused[0].rrf_score);
    }

    #[test]
    fn recency_prior_flips_adjacent_near_tie() {
        let fresh = utc(2026, 6, 1);
        let stale = utc(2026, 3, 3);
        let fused = fuse_rrf(
            vec![
                candidate("mem_20260610_0000000000000001_000001", Some(4), None, Some(stale)),
                candidate("mem_20260610_0000000000000002_000002", Some(5), None, Some(fresh)),
            ],
            60,
            0.0005,
            90.0,
        );

        assert_eq!(fused[0].memory_id.as_str(), "mem_20260610_0000000000000002_000002");
        assert_eq!(fused[1].memory_id.as_str(), "mem_20260610_0000000000000001_000001");
        assert!(fused[0].rrf_score < fused[1].rrf_score);
        assert!(fused[0].final_score > fused[1].final_score);
    }

    #[test]
    fn recency_prior_does_not_cross_meaningful_rrf_gap() {
        let fresh = utc(2026, 6, 1);
        let stale = utc(2026, 5, 1);
        let fused = fuse_rrf(
            vec![
                candidate("mem_20260610_0000000000000001_000001", Some(2), None, Some(stale)),
                candidate("mem_20260610_0000000000000002_000002", Some(3), None, Some(fresh)),
            ],
            60,
            0.0005,
            90.0,
        );

        assert_eq!(fused[0].memory_id.as_str(), "mem_20260610_0000000000000001_000001");
        assert_eq!(fused[1].memory_id.as_str(), "mem_20260610_0000000000000002_000002");
    }

    #[test]
    fn default_k_rrf_score_matches_fts_score_shape() {
        use crate::recall::config::DEFAULT_VECTOR_RECALL_RRF_K;

        let k = f64::from(DEFAULT_VECTOR_RECALL_RRF_K);
        assert!((reciprocal_rank_score(k, 1) - 1.0 / 61.0).abs() < f64::EPSILON);
        assert!((reciprocal_rank_score(k, 2) - 1.0 / 62.0).abs() < f64::EPSILON);
    }

    #[test]
    fn missing_recency_at_sorts_behind_dated_candidate_at_equal_rrf() {
        let dated = utc(2026, 6, 1);
        let fused = fuse_rrf(
            vec![
                candidate("mem_20260610_0000000000000002_000002", Some(1), None, None),
                candidate("mem_20260610_0000000000000001_000001", None, Some(0.8), Some(dated)),
            ],
            60,
            0.0005,
            90.0,
        );

        assert_eq!(fused[0].memory_id.as_str(), "mem_20260610_0000000000000001_000001");
        assert_eq!(fused[1].memory_id.as_str(), "mem_20260610_0000000000000002_000002");
    }

    #[test]
    fn four_lane_rrf_score_matches_hand_computed_golden_value() {
        let first = MemoryId::new("mem_20260610_0000000000000001_000001");
        let second = MemoryId::new("mem_20260610_0000000000000002_000002");
        let fused = fuse_four_lane_rrf(
            vec![
                candidate(first.as_str(), Some(1), Some(0.8), None),
                candidate(second.as_str(), Some(2), Some(0.9), None),
            ],
            vec![
                AbstractionVectorHit { memory_id: first.clone(), distance: 0.1 },
                AbstractionVectorHit { memory_id: second.clone(), distance: 0.2 },
            ],
            vec![
                CueVectorHit { memory_id: first.clone(), ordinal: 0, distance: 0.1 },
                CueVectorHit { memory_id: second.clone(), ordinal: 0, distance: 0.3 },
            ],
            four_lane_config(),
        );

        let first = fused.iter().find(|c| c.memory_id == first).expect("first candidate");
        let second = fused.iter().find(|c| c.memory_id == second).expect("second candidate");

        // first: bm25=1, chunk=2 (cosine 0.8 < 0.9), abstraction=2, cue=1 => 4/61 + 1/62
        let expected_first = 4.0 / 61.0 + 1.0 / 62.0;
        assert!((first.rrf_score - expected_first).abs() < 1e-12, "first RRF score: got {}", first.rrf_score);

        // second: bm25=2 (1/62), chunk=1 (1/61), abstraction=2 (2/62), cue=2 (1/62) => 1/61 + 4/62
        let expected_second = 1.0 / 61.0 + 4.0 / 62.0;
        assert!((second.rrf_score - expected_second).abs() < 1e-12, "second RRF score: got {}", second.rrf_score);
    }

    #[test]
    fn cue_rank_collapse_preserves_best_raw_rank() {
        let first = MemoryId::new("mem_20260610_0000000000000001_000001");
        let second = MemoryId::new("mem_20260610_0000000000000002_000002");
        let cues = vec![
            CueVectorHit { memory_id: first.clone(), ordinal: 0, distance: 0.1 },
            CueVectorHit { memory_id: first.clone(), ordinal: 1, distance: 0.2 },
            CueVectorHit { memory_id: second.clone(), ordinal: 0, distance: 0.3 },
        ];

        let ranks = best_rank_by_memory(cues.iter().map(|c| &c.memory_id));
        assert_eq!(ranks.get(first.as_str()), Some(&1));
        assert_eq!(ranks.get(second.as_str()), Some(&3));

        let fused = fuse_four_lane_rrf(
            vec![candidate(first.as_str(), None, None, None), candidate(second.as_str(), None, None, None)],
            Vec::new(),
            cues,
            four_lane_config(),
        );
        let first_candidate = fused.iter().find(|c| c.memory_id == first).expect("first candidate");
        let expected = reciprocal_rank_score(60.0, 1);
        assert!((first_candidate.rrf_score - expected).abs() < 1e-12, "cue rank 1 should survive collapse");
    }
}
