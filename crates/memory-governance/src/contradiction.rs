//! Trait-backed contradiction detection for candidate memory writes.

use serde::{Deserialize, Serialize};

use crate::hash::{canonical_claim_hash, canonical_entity_hash};
use crate::policy::{DEFAULT_CONTRADICTION_SIMILARITY_THRESHOLD, DEFAULT_CONTRADICTION_TOP_K};
use crate::{CandidateTombstoneKey, Scope, Source};

/// Candidate memory facts needed by deterministic governance checks.
#[derive(Clone, Debug, PartialEq)]
pub struct CandidateMemory {
    id: String,
    namespace: String,
    claim: String,
    canonical_claim_hash: String,
    entity_ids: Vec<String>,
    entity_hash: String,
    scope: Scope,
    confidence: f32,
    sources: Vec<Source>,
    has_explicit_user_context: bool,
}

/// Active-memory summary returned by a similarity provider.
#[derive(Clone, Debug, PartialEq)]
pub struct ExistingMemorySummary {
    id: String,
    namespace: String,
    canonical_claim_hash: String,
    entity_ids: Vec<String>,
    entity_hash: String,
    similarity: f32,
}

/// Search boundary for exact duplicate lookup and top-K similarity retrieval.
pub trait SimilaritySearch {
    /// Return an active memory with the same canonical claim hash, if one exists.
    fn find_active_by_claim_hash(&self, candidate: &CandidateMemory) -> Option<ExistingMemorySummary>;

    /// Return the top-K active memories relevant to this candidate.
    fn top_k(&self, candidate: &CandidateMemory, limit: usize) -> Vec<ExistingMemorySummary>;
}

impl<S> SimilaritySearch for &S
where
    S: SimilaritySearch + ?Sized,
{
    fn find_active_by_claim_hash(&self, candidate: &CandidateMemory) -> Option<ExistingMemorySummary> {
        (**self).find_active_by_claim_hash(candidate)
    }

    fn top_k(&self, candidate: &CandidateMemory, limit: usize) -> Vec<ExistingMemorySummary> {
        (**self).top_k(candidate, limit)
    }
}

/// Provider boundary for non-deterministic contradiction adjudication.
pub trait ContradictionTiebreaker {
    /// Decide whether a candidate is the same, a refinement, a contradiction, or unclear.
    fn tiebreak(&self, candidate: &CandidateMemory, hits: &[ExistingMemorySummary]) -> TiebreakOutcome;
}

impl<T> ContradictionTiebreaker for &T
where
    T: ContradictionTiebreaker + ?Sized,
{
    fn tiebreak(&self, candidate: &CandidateMemory, hits: &[ExistingMemorySummary]) -> TiebreakOutcome {
        (**self).tiebreak(candidate, hits)
    }
}

/// Tiebreak provider outcome.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TiebreakOutcome {
    /// Candidate is materially the same claim as an existing memory.
    Same {
        /// Existing memory id.
        existing_id: String,
    },
    /// Candidate should merge evidence into an existing memory.
    Refinement {
        /// Existing memory id to refine.
        existing_id: String,
    },
    /// Candidate contradicts an existing memory.
    Contradiction {
        /// Existing memory id being contradicted.
        existing_id: String,
    },
    /// Provider could not adjudicate safely.
    Unclear,
}

/// Deterministic contradiction pipeline decision.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ContradictionDecision {
    /// Candidate duplicates an existing active memory.
    Duplicate {
        /// Existing memory id.
        existing_id: String,
    },
    /// Candidate should merge evidence instead of creating another active memory.
    Refinement {
        /// Existing memory id.
        existing_id: String,
    },
    /// Candidate contradicts an existing active memory.
    Contradiction {
        /// Existing memory id.
        existing_id: String,
    },
    /// Candidate is similar enough to require review, but not clear enough to classify.
    Unclear,
    /// No duplicate or above-threshold contradiction candidate was found.
    NoConflict,
}

/// Runs duplicate, top-K similarity, and provider tiebreak checks.
#[derive(Clone, Debug)]
pub struct ContradictionDetector<S, T> {
    search: S,
    tiebreaker: T,
    top_k_limit: usize,
    similarity_threshold: f32,
}

impl CandidateMemory {
    /// Build a candidate memory with normalized claim and entity hashes.
    pub fn new(id: impl Into<String>, namespace: impl Into<String>, claim: impl Into<String>, scope: Scope) -> Self {
        let claim = claim.into();

        Self {
            id: id.into(),
            namespace: namespace.into(),
            canonical_claim_hash: canonical_claim_hash(&claim),
            claim,
            entity_ids: Vec::new(),
            entity_hash: canonical_entity_hash(&[]),
            scope,
            confidence: 1.0,
            sources: Vec::new(),
            has_explicit_user_context: false,
        }
    }

    /// Attach canonical entity identifiers.
    #[must_use]
    pub fn with_entity_ids(mut self, entity_ids: Vec<String>) -> Self {
        self.entity_hash = canonical_entity_hash(&entity_ids);
        self.entity_ids = entity_ids;
        self
    }

    /// Attach candidate confidence.
    #[must_use]
    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = confidence;
        self
    }

    /// Attach source refs for grounding verification.
    #[must_use]
    pub fn with_sources(mut self, sources: Vec<Source>) -> Self {
        self.sources = sources;
        self
    }

    /// Mark the candidate as backed by explicit user context in this turn.
    #[must_use]
    pub fn with_explicit_user_context(mut self) -> Self {
        self.has_explicit_user_context = true;
        self
    }

    /// Candidate id.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Candidate namespace.
    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    /// Candidate claim text.
    pub fn claim(&self) -> &str {
        &self.claim
    }

    /// Canonical claim hash.
    pub fn canonical_claim_hash(&self) -> &str {
        &self.canonical_claim_hash
    }

    /// Canonical entity ids.
    pub fn entity_ids(&self) -> &[String] {
        &self.entity_ids
    }

    /// Canonical entity hash.
    pub fn entity_hash(&self) -> &str {
        &self.entity_hash
    }

    /// Policy scope.
    pub fn scope(&self) -> Scope {
        self.scope
    }

    /// Candidate confidence.
    pub fn confidence(&self) -> f32 {
        self.confidence
    }

    /// Source refs used for grounding verification.
    pub fn sources(&self) -> &[Source] {
        &self.sources
    }

    /// Whether explicit user context is present.
    pub fn has_explicit_user_context(&self) -> bool {
        self.has_explicit_user_context
    }

    /// Whether the policy preview can treat grounding as present.
    pub fn has_grounding(&self) -> bool {
        self.has_explicit_user_context || !self.sources.is_empty()
    }

    /// Tombstone matching key for this candidate.
    pub fn tombstone_key(&self) -> CandidateTombstoneKey {
        CandidateTombstoneKey {
            target_memory_id: Some(self.id.clone()),
            content_hash: self.canonical_claim_hash.clone(),
            entity_hash: self.entity_hash.clone(),
        }
    }
}

impl ExistingMemorySummary {
    /// Build an active-memory summary with canonical hashes.
    pub fn new(id: impl Into<String>, namespace: impl Into<String>, claim: impl Into<String>, similarity: f32) -> Self {
        let claim = claim.into();

        Self {
            id: id.into(),
            namespace: namespace.into(),
            canonical_claim_hash: canonical_claim_hash(&claim),
            entity_ids: Vec::new(),
            entity_hash: canonical_entity_hash(&[]),
            similarity,
        }
    }

    /// Attach canonical entity identifiers.
    #[must_use]
    pub fn with_entity_ids(mut self, entity_ids: Vec<String>) -> Self {
        self.entity_hash = canonical_entity_hash(&entity_ids);
        self.entity_ids = entity_ids;
        self
    }

    /// Override the similarity score.
    ///
    /// Active-memory summaries are built with `similarity = 1.0` (they exist, so
    /// they are trivially "similar to themselves"); a top-K similarity provider
    /// re-stamps each surfaced summary with its measured similarity to the
    /// candidate so the detector's threshold gate operates on real scores.
    #[must_use]
    pub fn with_similarity(mut self, similarity: f32) -> Self {
        self.similarity = similarity;
        self
    }

    /// Existing memory id.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Existing memory namespace.
    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    /// Similarity score returned by the provider.
    pub fn similarity(&self) -> f32 {
        self.similarity
    }

    /// Canonical claim hash.
    pub fn canonical_claim_hash(&self) -> &str {
        &self.canonical_claim_hash
    }

    /// Canonical entity hash.
    pub fn entity_hash(&self) -> &str {
        &self.entity_hash
    }

    fn is_duplicate_of(&self, candidate: &CandidateMemory) -> bool {
        self.namespace == candidate.namespace
            && self.canonical_claim_hash == candidate.canonical_claim_hash
            && self.entity_hash == candidate.entity_hash
    }
}

impl<S, T> ContradictionDetector<S, T>
where
    S: SimilaritySearch,
    T: ContradictionTiebreaker,
{
    /// Create a detector using deterministic provider boundaries.
    pub fn new(search: S, tiebreaker: T) -> Self {
        Self {
            search,
            tiebreaker,
            top_k_limit: DEFAULT_CONTRADICTION_TOP_K,
            similarity_threshold: DEFAULT_CONTRADICTION_SIMILARITY_THRESHOLD,
        }
    }

    /// Override the minimum similarity score that triggers provider tiebreaking.
    #[must_use]
    pub fn with_similarity_threshold(mut self, similarity_threshold: f32) -> Self {
        self.similarity_threshold = similarity_threshold;
        self
    }

    /// Override top-K retrieval width.
    #[must_use]
    pub fn with_top_k_limit(mut self, top_k_limit: usize) -> Self {
        self.top_k_limit = top_k_limit;
        self
    }

    /// Return a reference to the tiebreaker, primarily for deterministic test doubles.
    pub fn tiebreaker(&self) -> &T {
        &self.tiebreaker
    }

    /// Run duplicate, similarity, and tiebreak checks in contract order.
    pub fn detect(&self, candidate: &CandidateMemory) -> ContradictionDecision {
        if let Some(duplicate) = self.find_exact_duplicate(candidate) {
            return ContradictionDecision::Duplicate { existing_id: duplicate.id };
        }

        let hits = self.search.top_k(candidate, self.top_k_limit);
        if !self.has_above_threshold_hit(&hits) {
            return ContradictionDecision::NoConflict;
        }

        match self.tiebreaker.tiebreak(candidate, &hits) {
            TiebreakOutcome::Same { existing_id } => ContradictionDecision::Duplicate { existing_id },
            TiebreakOutcome::Refinement { existing_id } => ContradictionDecision::Refinement { existing_id },
            TiebreakOutcome::Contradiction { existing_id } => ContradictionDecision::Contradiction { existing_id },
            TiebreakOutcome::Unclear => ContradictionDecision::Unclear,
        }
    }

    fn find_exact_duplicate(&self, candidate: &CandidateMemory) -> Option<ExistingMemorySummary> {
        self.search.find_active_by_claim_hash(candidate).filter(|existing| existing.is_duplicate_of(candidate))
    }

    fn has_above_threshold_hit(&self, hits: &[ExistingMemorySummary]) -> bool {
        hits.iter().any(|hit| hit.similarity >= self.similarity_threshold)
    }
}
