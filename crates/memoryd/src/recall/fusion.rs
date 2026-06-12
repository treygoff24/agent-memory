use std::collections::HashMap;

use memory_substrate::{HybridMemoryCandidate, HybridScoreBreakdown, MemoryId};

use crate::recall::config::DEFAULT_VECTOR_RECALL_RECENCY_TIE_EPSILON;

/// A hybrid recall candidate after Reciprocal Rank Fusion.
#[derive(Debug, Clone, PartialEq)]
pub struct FusedHybridCandidate {
    pub memory_id: MemoryId,
    pub text: String,
    pub score_breakdown: HybridScoreBreakdown,
    pub rrf_score: f64,
}

/// Fuse BM25 and vector recall lanes with Reciprocal Rank Fusion.
///
/// Rank bases are 1-based throughout: substrate BM25 ranks are already 1-based,
/// and this helper derives 1-based vector ranks from descending cosine
/// similarity. A memory absent from a lane contributes nothing for that lane.
/// Near-equal fused scores may use the `mem_YYYYMMDD` id prefix as a
/// subordinate freshness signal; remaining ties resolve deterministically by
/// lexicographic memory id.
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
                text: candidate.text.clone(),
                score_breakdown: candidate.score_breakdown.clone(),
                rrf_score: bm25_score + vector_score,
            }
        })
        .collect::<Vec<_>>();

    sort_by_rrf_with_recency_ties(&mut fused, DEFAULT_VECTOR_RECALL_RECENCY_TIE_EPSILON);
    fused
}

fn reciprocal_rank_score(k: f64, rank: usize) -> f64 {
    1.0 / (k + rank as f64)
}

fn sort_by_rrf_with_recency_ties(candidates: &mut [FusedHybridCandidate], epsilon: f64) {
    candidates.sort_by(|left, right| {
        right.rrf_score.total_cmp(&left.rrf_score).then_with(|| left.memory_id.as_str().cmp(right.memory_id.as_str()))
    });
    if epsilon <= 0.0 {
        return;
    }

    let mut group_start = 0;
    while group_start < candidates.len() {
        let group_score = candidates[group_start].rrf_score;
        let mut group_end = group_start + 1;
        while group_end < candidates.len() && (group_score - candidates[group_end].rrf_score).abs() <= epsilon {
            group_end += 1;
        }

        candidates[group_start..group_end].sort_by(|left, right| {
            memory_id_date(right.memory_id.as_str())
                .cmp(&memory_id_date(left.memory_id.as_str()))
                .then_with(|| right.rrf_score.total_cmp(&left.rrf_score))
                .then_with(|| left.memory_id.as_str().cmp(right.memory_id.as_str()))
        });
        group_start = group_end;
    }
}

fn memory_id_date(memory_id: &str) -> Option<u32> {
    let date = memory_id.strip_prefix("mem_")?.get(..8)?;
    if date.bytes().all(|byte| byte.is_ascii_digit()) {
        date.parse().ok()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(id: &str, bm25_rank: Option<usize>, cosine_similarity: Option<f32>) -> HybridMemoryCandidate {
        HybridMemoryCandidate {
            memory_id: MemoryId::new(id),
            text: format!("text for {id}"),
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
        assert_eq!(fused[0].text, "text for mem_20260610_0000000000000002_000002");
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

    #[test]
    fn recency_breaks_only_near_ties() {
        let fused = fuse_rrf(
            &[
                candidate("mem_20250101_0000000000000001_000001", Some(4), None),
                candidate("mem_20260101_0000000000000002_000002", Some(5), None),
            ],
            60,
        );

        assert_eq!(fused[0].memory_id.as_str(), "mem_20260101_0000000000000002_000002");
        assert_eq!(fused[1].memory_id.as_str(), "mem_20250101_0000000000000001_000001");
    }

    #[test]
    fn recency_does_not_cross_meaningful_rrf_gap() {
        let fused = fuse_rrf(
            &[
                candidate("mem_20250101_0000000000000001_000001", Some(2), None),
                candidate("mem_20260101_0000000000000002_000002", Some(3), None),
            ],
            60,
        );

        assert_eq!(fused[0].memory_id.as_str(), "mem_20250101_0000000000000001_000001");
        assert_eq!(fused[1].memory_id.as_str(), "mem_20260101_0000000000000002_000002");
    }
}
