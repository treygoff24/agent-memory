use std::collections::HashMap;

use memory_substrate::{HybridMemoryCandidate, HybridScoreBreakdown, MemoryId};

/// A hybrid recall candidate after Reciprocal Rank Fusion.
#[derive(Debug, Clone, PartialEq)]
pub struct FusedHybridCandidate {
    pub memory_id: MemoryId,
    pub score_breakdown: HybridScoreBreakdown,
    pub rrf_score: f64,
}

/// Fuse BM25 and vector recall lanes with Reciprocal Rank Fusion.
///
/// Rank bases are 1-based throughout: substrate BM25 ranks are already 1-based,
/// and this helper derives 1-based vector ranks from descending cosine
/// similarity. A memory absent from a lane contributes nothing for that lane.
/// Equal fused scores resolve deterministically by lexicographic memory id.
pub fn fuse_rrf(candidates: &[HybridMemoryCandidate], rrf_k: u32) -> Vec<FusedHybridCandidate> {
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
        .iter()
        .map(|candidate| {
            let bm25_score =
                candidate.score_breakdown.bm25_rank.map(|rank| reciprocal_rank_score(k, rank)).unwrap_or_default();
            let vector_score = vector_rank_by_id
                .get(candidate.memory_id.as_str())
                .copied()
                .map(|rank| reciprocal_rank_score(k, rank))
                .unwrap_or_default();
            FusedHybridCandidate {
                memory_id: candidate.memory_id.clone(),
                score_breakdown: candidate.score_breakdown.clone(),
                rrf_score: bm25_score + vector_score,
            }
        })
        .collect::<Vec<_>>();

    fused.sort_by(|left, right| {
        right.rrf_score.total_cmp(&left.rrf_score).then_with(|| left.memory_id.as_str().cmp(right.memory_id.as_str()))
    });
    fused
}

fn reciprocal_rank_score(k: f64, rank: usize) -> f64 {
    1.0 / (k + rank as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(id: &str, bm25_rank: Option<usize>, cosine_similarity: Option<f32>) -> HybridMemoryCandidate {
        HybridMemoryCandidate {
            memory_id: MemoryId::new(id),
            score_breakdown: HybridScoreBreakdown { bm25_rank, cosine_similarity },
        }
    }

    #[test]
    fn fuses_two_one_based_rank_lanes() {
        let fused = fuse_rrf(
            &[
                candidate("mem_20260610_0000000000000001_000001", Some(1), None),
                candidate("mem_20260610_0000000000000002_000002", Some(2), Some(0.9)),
                candidate("mem_20260610_0000000000000003_000003", None, Some(0.95)),
            ],
            60,
        );

        assert_eq!(fused[0].memory_id.as_str(), "mem_20260610_0000000000000002_000002");
        assert_eq!(fused[1].memory_id.as_str(), "mem_20260610_0000000000000001_000001");
        assert_eq!(fused[2].memory_id.as_str(), "mem_20260610_0000000000000003_000003");
    }

    #[test]
    fn equal_scores_tie_break_by_memory_id() {
        let fused = fuse_rrf(
            &[
                candidate("mem_20260610_0000000000000002_000002", Some(1), None),
                candidate("mem_20260610_0000000000000001_000001", None, Some(0.8)),
            ],
            60,
        );

        assert_eq!(fused[0].memory_id.as_str(), "mem_20260610_0000000000000001_000001");
        assert_eq!(fused[1].memory_id.as_str(), "mem_20260610_0000000000000002_000002");
    }
}
