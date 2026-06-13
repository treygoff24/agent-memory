use chrono::{DateTime, Utc};
use memory_substrate::{MemoryStatus, Scope, SourceKind};

use crate::recall::budget::estimated_tokens;
use crate::recall::candidates::RecallCandidate;
use crate::recall::types::{OmissionReason, RecallOmission, RecallSectionName};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RankingContext {
    pub now: DateTime<Utc>,
    pub exact_project_namespace: Option<String>,
    /// Strength-term ceiling (memory-dynamics-v0.1 §3). `0` disables the term
    /// (dynamics off → structural-only ranking, byte-identical to pre-dynamics
    /// except the policy version string). At the default `12`, strength is capped
    /// at `11`, so it can flip near-ties (structural gap `< 12`) but can never
    /// tie or flip a structural gap `>= 12`.
    pub alpha_points: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RankedRecallCandidate {
    pub score: i64,
    pub candidate: RecallCandidate,
}

impl RankedRecallCandidate {
    /// Candidate id, borrowed from the embedded [`RecallCandidate`].
    ///
    /// `RankedRecallCandidate` previously carried its own `id: String` field that
    /// duplicated `candidate.id`; this accessor reads the single owned copy
    /// instead, removing one heap allocation per candidate on the ranking path.
    pub fn id(&self) -> &str {
        &self.candidate.id
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RankedSelection {
    pub selected: Vec<RankedRecallCandidate>,
    pub omitted: Vec<RecallOmission>,
}

pub fn rank_recall_candidates(candidates: Vec<RecallCandidate>, context: RankingContext) -> Vec<RankedRecallCandidate> {
    let mut ranked = candidates
        .into_iter()
        .map(|candidate| {
            let score = score_candidate(&candidate, &context);
            RankedRecallCandidate { score, candidate }
        })
        .collect::<Vec<_>>();

    ranked.sort_by(compare_ranked_candidates);
    ranked
}

pub fn select_ranked_candidates(
    section: RecallSectionName,
    candidates: Vec<RecallCandidate>,
    context: RankingContext,
    budget_tokens: usize,
) -> RankedSelection {
    let ranked = rank_recall_candidates(candidates, context);
    let mut selected = Vec::new();
    let mut omitted = Vec::new();
    let mut used_tokens = 0usize;

    for candidate in ranked {
        let candidate_tokens = estimated_tokens(&candidate.candidate.row.summary).max(1);
        if used_tokens + candidate_tokens <= budget_tokens {
            used_tokens += candidate_tokens;
            selected.push(candidate);
        } else {
            omitted.push(RecallOmission {
                id: Some(candidate.candidate.id),
                section,
                reason: OmissionReason::BudgetExhausted,
                alias: None,
                colliding_ids: Vec::new(),
            });
        }
    }

    RankedSelection { selected, omitted }
}

fn score_candidate(candidate: &RecallCandidate, context: &RankingContext) -> i64 {
    status_weight(candidate.candidate_status())
        + scope_weight(candidate, context)
        + candidate.entity_match.weight()
        + recency_weight(candidate, context)
        + confidence_weight(candidate)
        + source_weight(candidate.candidate_source())
        + strength_points_for(candidate, context)
}

/// Bounded additive strength term (memory-dynamics-v0.1 §3).
///
/// `min(floor(strength × alpha_points), alpha_points - 1)`, in
/// `[0, alpha_points - 1]`. When the candidate has no hydrated strength
/// (dynamics off, or usage query soft-failed) or `alpha_points == 0`, the term is
/// `0` — leaving the structural ranking exactly as it was before dynamics.
fn strength_points_for(candidate: &RecallCandidate, context: &RankingContext) -> i64 {
    match candidate.strength {
        Some(strength) if context.alpha_points > 0 => {
            crate::dynamics::strength::strength_points(strength, context.alpha_points)
        }
        _ => 0,
    }
}

fn compare_ranked_candidates(left: &RankedRecallCandidate, right: &RankedRecallCandidate) -> std::cmp::Ordering {
    right
        .score
        .cmp(&left.score)
        .then_with(|| {
            status_sort_key(right.candidate.candidate_status()).cmp(&status_sort_key(left.candidate.candidate_status()))
        })
        .then_with(|| right.candidate.row.updated_at.cmp(&left.candidate.row.updated_at))
        .then_with(|| left.candidate.id.cmp(&right.candidate.id))
}

fn status_weight(status: MemoryStatus) -> i64 {
    match status {
        MemoryStatus::Pinned => 100,
        MemoryStatus::Active => 50,
        _ => 0,
    }
}

fn status_sort_key(status: MemoryStatus) -> i64 {
    match status {
        MemoryStatus::Pinned => 1,
        _ => 0,
    }
}

fn scope_weight(candidate: &RecallCandidate, context: &RankingContext) -> i64 {
    match candidate.row.scope {
        Scope::Project
            if candidate.row.canonical_namespace_id.as_deref() == context.exact_project_namespace.as_deref() =>
        {
            30
        }
        Scope::User => 25,
        Scope::Agent => 15,
        _ => 0,
    }
}

fn recency_weight(candidate: &RecallCandidate, context: &RankingContext) -> i64 {
    let age = context.now.signed_duration_since(candidate.row.updated_at);
    if age < chrono::Duration::zero() {
        return 0;
    }
    if age <= chrono::Duration::days(7) {
        10
    } else if age <= chrono::Duration::days(30) {
        5
    } else {
        0
    }
}

fn confidence_weight(candidate: &RecallCandidate) -> i64 {
    (candidate.row.confidence * 10.0).floor() as i64
}

fn source_weight(source_kind: SourceKind) -> i64 {
    match source_kind {
        SourceKind::User => 10,
        SourceKind::AgentPrimary => 5,
        SourceKind::AgentSubagent | SourceKind::Tool | SourceKind::File => 3,
        _ => 0,
    }
}

trait CandidateRankFields {
    fn candidate_status(&self) -> MemoryStatus;
    fn candidate_source(&self) -> SourceKind;
}

impl CandidateRankFields for RecallCandidate {
    fn candidate_status(&self) -> MemoryStatus {
        self.row.status
    }

    fn candidate_source(&self) -> SourceKind {
        self.row.source_kind
    }
}
