use std::collections::HashMap;

use chrono::{DateTime, Utc};
use memory_substrate::{HybridMemoryCandidate, HybridScoreBreakdown, MemoryId};

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

fn reciprocal_rank_score(k: f64, rank: usize) -> f64 {
    1.0 / (k + rank as f64)
}

fn apply_recency_prior_and_sort(
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
}
