use std::cell::{Cell, RefCell};

use memory_governance::{
    CandidateMemory, ContradictionDecision, ContradictionDetector, ContradictionTiebreaker, ExistingMemorySummary,
    SimilaritySearch, Source, SourceKind, TiebreakOutcome,
};

#[test]
fn duplicate_claim_hash_returns_duplicate_without_tiebreaker() {
    let candidate = candidate("candidate-1", "Trey prefers concise implementation reports.");
    let existing = ExistingMemorySummary::new("mem_existing", candidate.namespace(), candidate.claim(), 1.0)
        .with_entity_ids(candidate.entity_ids().to_vec());
    let search = FakeSimilaritySearch::new().with_duplicate(existing);
    let tiebreaker = RecordingTiebreaker::new(TiebreakOutcome::Unclear);
    let detector = ContradictionDetector::new(search, tiebreaker).with_similarity_threshold(0.82);

    let decision = detector.detect(&candidate);

    assert_eq!(decision, ContradictionDecision::Duplicate { existing_id: "mem_existing".to_owned() });
    assert_eq!(detector.tiebreaker().call_count(), 0);
}

#[test]
fn above_threshold_candidate_invokes_tiebreaker_with_candidate_and_top_k_hits() {
    let candidate = candidate("candidate-2", "The governance crate owns deterministic write decisions.");
    let hit_a = ExistingMemorySummary::new(
        "mem_hit_a",
        candidate.namespace(),
        "The governance crate owns policy write decisions.",
        0.91,
    )
    .with_entity_ids(candidate.entity_ids().to_vec());
    let hit_b =
        ExistingMemorySummary::new("mem_hit_b", candidate.namespace(), "Governance decisions are deterministic.", 0.86)
            .with_entity_ids(candidate.entity_ids().to_vec());
    let search = FakeSimilaritySearch::new().with_hits(vec![hit_a, hit_b]);
    let tiebreaker = RecordingTiebreaker::new(TiebreakOutcome::Refinement { existing_id: "mem_hit_a".to_owned() });
    let detector = ContradictionDetector::new(search, tiebreaker).with_similarity_threshold(0.82).with_top_k_limit(2);

    let decision = detector.detect(&candidate);

    assert_eq!(decision, ContradictionDecision::Refinement { existing_id: "mem_hit_a".to_owned() });
    assert_eq!(detector.tiebreaker().call_count(), 1);
    assert_eq!(detector.tiebreaker().last_candidate_id(), Some("candidate-2".to_owned()));
    assert_eq!(detector.tiebreaker().last_hit_ids(), vec!["mem_hit_a".to_owned(), "mem_hit_b".to_owned()]);
}

fn candidate(id: &'static str, claim: &'static str) -> CandidateMemory {
    CandidateMemory::new(id, "project/agent-memory", claim, memory_governance::Scope::Project)
        .with_entity_ids(vec!["project:agent-memory".to_owned()])
        .with_confidence(0.95)
        .with_sources(vec![Source::new(SourceKind::User, None::<String>)])
        .with_explicit_user_context()
}

#[derive(Clone, Debug, Default)]
struct FakeSimilaritySearch {
    duplicate: Option<ExistingMemorySummary>,
    hits: Vec<ExistingMemorySummary>,
}

impl FakeSimilaritySearch {
    fn new() -> Self {
        Self::default()
    }

    fn with_duplicate(mut self, duplicate: ExistingMemorySummary) -> Self {
        self.duplicate = Some(duplicate);
        self
    }

    fn with_hits(mut self, hits: Vec<ExistingMemorySummary>) -> Self {
        self.hits = hits;
        self
    }
}

impl SimilaritySearch for FakeSimilaritySearch {
    fn find_active_by_claim_hash(&self, _candidate: &CandidateMemory) -> Option<ExistingMemorySummary> {
        self.duplicate.clone()
    }

    fn top_k(&self, _candidate: &CandidateMemory, limit: usize) -> Vec<ExistingMemorySummary> {
        self.hits.iter().take(limit).cloned().collect()
    }
}

#[derive(Debug)]
struct RecordingTiebreaker {
    outcome: TiebreakOutcome,
    call_count: Cell<usize>,
    last_candidate_id: RefCell<Option<String>>,
    last_hit_ids: RefCell<Vec<String>>,
}

impl RecordingTiebreaker {
    fn new(outcome: TiebreakOutcome) -> Self {
        Self {
            outcome,
            call_count: Cell::new(0),
            last_candidate_id: RefCell::new(None),
            last_hit_ids: RefCell::new(Vec::new()),
        }
    }

    fn call_count(&self) -> usize {
        self.call_count.get()
    }

    fn last_candidate_id(&self) -> Option<String> {
        self.last_candidate_id.borrow().clone()
    }

    fn last_hit_ids(&self) -> Vec<String> {
        self.last_hit_ids.borrow().clone()
    }
}

impl ContradictionTiebreaker for RecordingTiebreaker {
    fn tiebreak(&self, candidate: &CandidateMemory, hits: &[ExistingMemorySummary]) -> TiebreakOutcome {
        self.call_count.set(self.call_count.get() + 1);
        *self.last_candidate_id.borrow_mut() = Some(candidate.id().to_owned());
        *self.last_hit_ids.borrow_mut() = hits.iter().map(|hit| hit.id().to_owned()).collect();
        self.outcome.clone()
    }
}
