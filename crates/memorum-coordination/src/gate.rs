use chrono::{DateTime, Utc};
use memory_substrate::{EmbeddingTriple, MemoryId, RecallIndexRow};
use std::collections::HashSet;

use crate::config::CoordinationConfig;
use crate::protocol::{CoordinationInsertion, PeerUpdateEntry};
use crate::session::{QueryEmbedding, SessionContext};

const ENTITY_WEIGHT: f64 = 0.5;
const PATH_WEIGHT: f64 = 0.3;
const TOPIC_WEIGHT: f64 = 0.2;

/// Relevance gate for selecting peer writes to surface in recall blocks.
#[derive(Clone, Debug)]
pub struct RelevanceGate {
    config: CoordinationConfig,
}

impl RelevanceGate {
    pub fn new(config: CoordinationConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &CoordinationConfig {
        &self.config
    }

    pub fn evaluate(
        &self,
        session: &mut SessionContext,
        candidates: &[PeerWriteCandidate],
        now: DateTime<Utc>,
    ) -> CoordinationInsertion {
        self.evaluate_with_scorer(session, candidates, now, &mut score_with_embedding)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn evaluate_with_scorer<S>(
        &self,
        session: &mut SessionContext,
        candidates: &[PeerWriteCandidate],
        now: DateTime<Utc>,
        scorer: &mut S,
    ) -> CoordinationInsertion
    where
        S: FnMut(&PeerWriteCandidate, &SessionContext, Option<&QueryEmbedding>) -> f64,
    {
        if session.is_observe_only_harness() {
            return CoordinationInsertion::empty();
        }

        let recency_cutoff = now
            - chrono::Duration::try_seconds(self.config.relevance_gate.recency_window_seconds as i64)
                .unwrap_or(chrono::Duration::MAX);

        let session_embedding = session.scoring_query_embedding();
        let mut scored_candidates = candidates
            .iter()
            .filter(|candidate| candidate.row.indexed_at >= recency_cutoff)
            .filter(|candidate| !session.has_surfaced_peer_write(candidate.memory_id.as_str()))
            .filter_map(|candidate| {
                let relevance = scorer(candidate, session, session_embedding.as_ref());
                (relevance >= self.config.relevance_gate.threshold).then_some((candidate, relevance))
            })
            .collect::<Vec<_>>();

        scored_candidates.sort_by(|(left_candidate, left_score), (right_candidate, right_score)| {
            // Sort highest relevance first; break ties by most recent, then by
            // memory id (lexicographic) for deterministic ordering. Compare
            // memory ids via `as_str()` to avoid a String allocation per
            // tiebreaker comparison (called O(N log N) times during sort).
            right_score
                .total_cmp(left_score)
                .then_with(|| right_candidate.row.updated_at.cmp(&left_candidate.row.updated_at))
                .then_with(|| left_candidate.memory_id.as_str().cmp(right_candidate.memory_id.as_str()))
        });

        let selected_count = scored_candidates.len().min(self.config.relevance_gate.per_turn_cap);
        let capped_peer_updates = scored_candidates.len().saturating_sub(selected_count).try_into().unwrap_or(u32::MAX);
        let selected_candidates = scored_candidates.into_iter().take(selected_count).collect::<Vec<_>>();
        for (candidate, _) in &selected_candidates {
            session.record_surfaced_peer_write(candidate.memory_id.as_str());
        }

        let peer_updates = selected_candidates
            .into_iter()
            .map(|(candidate, relevance)| peer_update_entry(candidate, relevance))
            .collect();

        CoordinationInsertion { peer_updates, peer_presence: Vec::new(), capped_peer_updates, capped_peer_presence: 0 }
    }
}

/// Weighted Stream I relevance score for one candidate/session pair.
pub fn score(candidate: &PeerWriteCandidate, session: &SessionContext) -> f64 {
    let session_embedding = session.scoring_query_embedding();
    score_with_embedding(candidate, session, session_embedding.as_ref())
}

fn score_with_embedding(
    candidate: &PeerWriteCandidate,
    session: &SessionContext,
    session_embedding: Option<&QueryEmbedding>,
) -> f64 {
    ENTITY_WEIGHT * entity_jaccard(&candidate_entity_ids(candidate), &session.salient_entities)
        + PATH_WEIGHT * path_fraction(&candidate.paths, &session.salient_paths)
        + TOPIC_WEIGHT * topic_similarity(candidate.embedding.as_ref(), session_embedding)
}

/// Jaccard similarity over normalized entity ids.
pub fn entity_jaccard(candidate_entities: &HashSet<String>, session_entities: &HashSet<String>) -> f64 {
    let candidate_entities = normalized_set(candidate_entities);
    let session_entities = normalized_set(session_entities);

    if candidate_entities.is_empty() && session_entities.is_empty() {
        return 0.0;
    }

    let intersection_count = candidate_entities.intersection(&session_entities).count() as f64;
    let union_count = candidate_entities.union(&session_entities).count() as f64;
    intersection_count / union_count
}

/// Fraction of candidate paths covered by the session's salient paths.
///
/// A candidate path is "covered" if any session path intersects it via
/// path-component-boundary prefix semantics:
/// - exact match (`/src/auth` == `/src/auth`),
/// - session path is a prefix of the candidate path (`/src/auth` covers
///   `/src/auth/service.rs`), or
/// - candidate path is a prefix of the session path (`/src/auth/service.rs`
///   covers `/src/auth` — e.g. session is focused on a specific file but the
///   peer write touched its parent directory).
///
/// The check uses [`std::path::Path::starts_with`], which operates on whole
/// path components, so `/src/auth` does NOT cover `/src/authentication.rs`.
pub fn path_fraction(candidate_paths: &[String], session_paths: &HashSet<String>) -> f64 {
    if candidate_paths.is_empty() {
        return 0.0;
    }

    let covered_count = candidate_paths
        .iter()
        .filter(|candidate| session_paths.iter().any(|session| paths_intersect(candidate.as_str(), session.as_str())))
        .count() as f64;
    covered_count / candidate_paths.len() as f64
}

/// Returns true if `a` and `b` intersect by path-component-boundary prefix
/// semantics: exact match, `a` is a prefix of `b`, or `b` is a prefix of `a`.
fn paths_intersect(a: &str, b: &str) -> bool {
    use std::path::Path;
    let a = Path::new(a);
    let b = Path::new(b);
    a == b || b.starts_with(a) || a.starts_with(b)
}

/// Cosine topic similarity. Missing or mismatched embedding triples score as no topic match.
pub fn topic_similarity(
    candidate_embedding: Option<&CandidateEmbedding>,
    session_embedding: Option<&crate::session::QueryEmbedding>,
) -> f64 {
    let Some(candidate_embedding) = candidate_embedding else {
        return 0.0;
    };
    let Some(session_embedding) = session_embedding else {
        return 0.0;
    };
    if candidate_embedding.triple != session_embedding.triple {
        return 0.0;
    }

    cosine_similarity(&candidate_embedding.vector, &session_embedding.vector)
}

fn candidate_entity_ids(candidate: &PeerWriteCandidate) -> HashSet<String> {
    candidate.row.entities.iter().map(|entity| entity.id.clone()).collect()
}

fn normalized_set(values: &HashSet<String>) -> HashSet<String> {
    values.iter().map(|value| value.trim().to_ascii_lowercase()).filter(|value| !value.is_empty()).collect()
}

fn cosine_similarity(candidate_vector: &[f32], session_vector: &[f32]) -> f64 {
    if candidate_vector.is_empty() || candidate_vector.len() != session_vector.len() {
        return 0.0;
    }

    let (dot_product, candidate_norm_squared, session_norm_squared) = candidate_vector.iter().zip(session_vector).fold(
        (0.0, 0.0, 0.0),
        |(dot, candidate_norm, session_norm), (candidate_value, session_value)| {
            let candidate_value = f64::from(*candidate_value);
            let session_value = f64::from(*session_value);
            (
                dot + candidate_value * session_value,
                candidate_norm + candidate_value * candidate_value,
                session_norm + session_value * session_value,
            )
        },
    );

    if candidate_norm_squared == 0.0 || session_norm_squared == 0.0 {
        return 0.0;
    }

    (dot_product / (candidate_norm_squared.sqrt() * session_norm_squared.sqrt())).clamp(0.0, 1.0)
}

fn peer_update_entry(candidate: &PeerWriteCandidate, relevance: f64) -> PeerUpdateEntry {
    PeerUpdateEntry {
        harness: candidate.harness.clone(),
        session_id: candidate.session_id.clone(),
        timestamp: candidate.row.updated_at,
        relevance,
        summary: candidate.row.summary.clone(),
        reference: candidate.memory_id.as_str().to_owned(),
        namespace: candidate.namespace.clone(),
        claim_locked: None,
        device: None,
    }
}

/// Candidate peer write considered by the relevance gate.
#[derive(Clone, Debug, PartialEq)]
pub struct PeerWriteCandidate {
    pub memory_id: MemoryId,
    pub row: RecallIndexRow,
    pub paths: Vec<String>,
    pub harness: String,
    pub session_id: String,
    pub namespace: String,
    pub embedding: Option<CandidateEmbedding>,
}

/// Embedding vector plus model identity for a candidate peer write.
#[derive(Clone, Debug, PartialEq)]
pub struct CandidateEmbedding {
    pub triple: EmbeddingTriple,
    pub vector: Vec<f32>,
}
