use memory_governance::{
    CandidateMemory, ContradictionTiebreaker, ExistingMemorySummary, FileSourceResolver, GovernanceEngine,
    GovernanceProviders, GovernanceWriteDecision, GroundingVerifier, NextWriteAction, PolicySet, SimilaritySearch,
    Source, SourceKind, TiebreakOutcome, TombstoneIndex,
};

#[test]
fn refinement_tiebreaker_asks_caller_to_merge_evidence_not_create_second_active_memory() {
    let candidate = candidate(memory_governance::Scope::Project, 0.95);
    let existing = similar_existing(&candidate, "mem_refinement", 0.9);
    let engine = engine(
        FakeSimilaritySearch::new().with_hits(vec![existing]),
        FakeTiebreaker::new(TiebreakOutcome::Refinement { existing_id: "mem_refinement".to_owned() }),
    );

    let decision = engine.evaluate_write(&candidate);

    assert_eq!(
        decision,
        GovernanceWriteDecision::Refinement {
            existing_id: "mem_refinement".to_owned(),
            candidate_id: candidate.id().to_owned(),
            next_action: NextWriteAction::MergeEvidence,
        }
    );
}

#[test]
fn contradiction_tiebreaker_maps_to_supersession_or_quarantine_by_policy() {
    let project_candidate = candidate(memory_governance::Scope::Project, 0.95);
    let project_engine = engine(
        FakeSimilaritySearch::new().with_hits(vec![similar_existing(&project_candidate, "mem_old_project", 0.93)]),
        FakeTiebreaker::new(TiebreakOutcome::Contradiction { existing_id: "mem_old_project".to_owned() }),
    );
    let agent_candidate = candidate(memory_governance::Scope::Agent, 0.95);
    let agent_engine = engine(
        FakeSimilaritySearch::new().with_hits(vec![similar_existing(&agent_candidate, "mem_old_agent", 0.93)]),
        FakeTiebreaker::new(TiebreakOutcome::Contradiction { existing_id: "mem_old_agent".to_owned() }),
    );

    assert_eq!(
        project_engine.evaluate_write(&project_candidate),
        GovernanceWriteDecision::Supersession {
            existing_id: "mem_old_project".to_owned(),
            replacement_id: project_candidate.id().to_owned(),
            policy_applied: "project-standard@v2".to_owned(),
            next_action: NextWriteAction::SupersedeWithChain,
        }
    );
    assert_eq!(
        agent_engine.evaluate_write(&agent_candidate),
        GovernanceWriteDecision::Quarantined {
            id: agent_candidate.id().to_owned(),
            reason: "contradiction".to_owned(),
            policy_applied: "agent-strict@v3".to_owned(),
            next_action: NextWriteAction::WriteQuarantined,
        }
    );
}

#[test]
fn below_threshold_candidates_proceed_to_policy_promotion_or_candidate_decision() {
    let promoted_candidate = candidate(memory_governance::Scope::Project, 0.95);
    let candidate_for_review = candidate(memory_governance::Scope::Project, 0.5);
    let promoted_engine = engine(
        FakeSimilaritySearch::new().with_hits(vec![similar_existing(&promoted_candidate, "mem_low_similarity", 0.2)]),
        FakeTiebreaker::new(TiebreakOutcome::Unclear),
    );
    let review_engine = engine(
        FakeSimilaritySearch::new().with_hits(vec![similar_existing(&candidate_for_review, "mem_low_confidence", 0.2)]),
        FakeTiebreaker::new(TiebreakOutcome::Unclear),
    );

    assert_eq!(
        promoted_engine.evaluate_write(&promoted_candidate),
        GovernanceWriteDecision::Promoted {
            id: promoted_candidate.id().to_owned(),
            namespace: promoted_candidate.namespace().to_owned(),
            policy_applied: "project-standard@v2".to_owned(),
            next_action: NextWriteAction::PromoteToSubstrate,
        }
    );
    assert_eq!(
        review_engine.evaluate_write(&candidate_for_review),
        GovernanceWriteDecision::Candidate {
            id: candidate_for_review.id().to_owned(),
            reason: "low_confidence".to_owned(),
            policy_applied: "project-standard@v2".to_owned(),
            next_action: NextWriteAction::WriteCandidate,
        }
    );
}

fn engine(
    search: FakeSimilaritySearch,
    tiebreaker: FakeTiebreaker,
) -> GovernanceEngine<FakeSimilaritySearch, FakeTiebreaker, FakeSessionSpawnResolver> {
    GovernanceEngine::new(
        PolicySet::builtin(),
        GroundingVerifier::new(FileSourceResolver, FakeSessionSpawnResolver),
        TombstoneIndex::default(),
        GovernanceProviders::new(search, tiebreaker),
    )
    .with_similarity_threshold(0.82)
}

/// An engine wired to a caller-supplied `PolicySet` with *no* threshold
/// override, so the contradiction threshold is read straight from the selected
/// policy's `contradiction` block.
fn engine_from_policies(
    policies: PolicySet,
    search: FakeSimilaritySearch,
    tiebreaker: FakeTiebreaker,
) -> GovernanceEngine<FakeSimilaritySearch, FakeTiebreaker, FakeSessionSpawnResolver> {
    GovernanceEngine::new(
        policies,
        GroundingVerifier::new(FileSourceResolver, FakeSessionSpawnResolver),
        TombstoneIndex::default(),
        GovernanceProviders::new(search, tiebreaker),
    )
}

/// Load a four-scope `PolicySet` from a temp dir where the `project` policy
/// carries the given `contradiction.similarity_threshold`.
fn policies_with_project_similarity_threshold(threshold: f32) -> PolicySet {
    let dir = std::env::temp_dir().join(format!(
        "memory-governance-engine-threshold-{}-{}",
        (threshold * 1000.0) as i64,
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create temp policy dir");
    let files = [
        ("me-strict.yaml", "me-strict", 1, "me", "refuse", "quarantine"),
        ("project-standard.yaml", "project-standard", 2, "project", "review", "supersede"),
        ("agent-strict.yaml", "agent-strict", 3, "agent", "refuse", "quarantine"),
        ("dreaming-strict.yaml", "dreaming-strict", 1, "dreaming", "refuse", "quarantine"),
    ];
    for (file, name, version, scope, tombstone, contradiction) in files {
        let block = if scope == "project" {
            format!("contradiction:\n  similarity_threshold: {threshold}\n")
        } else {
            String::new()
        };
        let yaml = format!(
            "name: {name}\nversion: {version}\nscope: {scope}\nconfidence_floor: 0.7\nrequires_grounding: true\ntombstone_enforcement: {tombstone}\ncontradiction_policy: {contradiction}\nreview_gates: []\n{block}"
        );
        std::fs::write(dir.join(file), yaml).expect("write temp policy");
    }
    let policies = PolicySet::load_from_dir(&dir).expect("tuned policies parse");
    std::fs::remove_dir_all(&dir).expect("remove temp policy dir");
    policies
}

#[test]
fn policy_similarity_threshold_changes_the_contradiction_decision() {
    // One borderline hit at similarity 0.5: below the default 0.82 it is no
    // conflict (promote); a policy that lowers the threshold to 0.4 turns the
    // same hit into a tiebroken contradiction (supersession, for project scope).
    let candidate = candidate(memory_governance::Scope::Project, 0.95);
    let hit_id = "mem_borderline";

    let default_engine = engine_from_policies(
        policies_with_project_similarity_threshold(0.82),
        FakeSimilaritySearch::new().with_hits(vec![similar_existing(&candidate, hit_id, 0.5)]),
        FakeTiebreaker::new(TiebreakOutcome::Contradiction { existing_id: hit_id.to_owned() }),
    );
    let lowered_engine = engine_from_policies(
        policies_with_project_similarity_threshold(0.4),
        FakeSimilaritySearch::new().with_hits(vec![similar_existing(&candidate, hit_id, 0.5)]),
        FakeTiebreaker::new(TiebreakOutcome::Contradiction { existing_id: hit_id.to_owned() }),
    );

    assert_eq!(
        default_engine.evaluate_write(&candidate),
        GovernanceWriteDecision::Promoted {
            id: candidate.id().to_owned(),
            namespace: candidate.namespace().to_owned(),
            policy_applied: "project-standard@v2".to_owned(),
            next_action: NextWriteAction::PromoteToSubstrate,
        },
        "0.5 hit is below the 0.82 policy threshold, so no contradiction",
    );
    assert_eq!(
        lowered_engine.evaluate_write(&candidate),
        GovernanceWriteDecision::Supersession {
            existing_id: hit_id.to_owned(),
            replacement_id: candidate.id().to_owned(),
            policy_applied: "project-standard@v2".to_owned(),
            next_action: NextWriteAction::SupersedeWithChain,
        },
        "lowering the policy threshold to 0.4 makes the same 0.5 hit a contradiction",
    );
}

fn candidate(scope: memory_governance::Scope, confidence: f32) -> CandidateMemory {
    CandidateMemory::new(
        format!("candidate-{scope:?}-{confidence}"),
        "project/agent-memory",
        "Stream C governance should keep contradiction writes deterministic.",
        scope,
    )
    .with_entity_ids(vec!["project:agent-memory".to_owned()])
    .with_confidence(confidence)
    .with_sources(vec![Source::new(SourceKind::User, None::<String>)])
    .with_explicit_user_context()
}

fn similar_existing(candidate: &CandidateMemory, id: &'static str, similarity: f32) -> ExistingMemorySummary {
    ExistingMemorySummary::new(
        id,
        candidate.namespace(),
        "Stream C governance should keep similar contradiction writes deterministic.",
        similarity,
    )
    .with_entity_ids(candidate.entity_ids().to_vec())
}

#[derive(Clone, Debug, Default)]
struct FakeSimilaritySearch {
    hits: Vec<ExistingMemorySummary>,
}

impl FakeSimilaritySearch {
    fn new() -> Self {
        Self::default()
    }

    fn with_hits(mut self, hits: Vec<ExistingMemorySummary>) -> Self {
        self.hits = hits;
        self
    }
}

impl SimilaritySearch for FakeSimilaritySearch {
    fn find_active_by_claim_hash(&self, _candidate: &CandidateMemory) -> Option<ExistingMemorySummary> {
        None
    }

    fn top_k(&self, _candidate: &CandidateMemory, limit: usize) -> Vec<ExistingMemorySummary> {
        self.hits.iter().take(limit).cloned().collect()
    }
}

#[derive(Clone, Debug)]
struct FakeTiebreaker {
    outcome: TiebreakOutcome,
}

impl FakeTiebreaker {
    fn new(outcome: TiebreakOutcome) -> Self {
        Self { outcome }
    }
}

impl ContradictionTiebreaker for FakeTiebreaker {
    fn tiebreak(&self, _candidate: &CandidateMemory, _hits: &[ExistingMemorySummary]) -> TiebreakOutcome {
        self.outcome.clone()
    }
}

#[derive(Clone, Copy, Debug)]
struct FakeSessionSpawnResolver;

impl memory_governance::SessionSpawnResolver for FakeSessionSpawnResolver {
    fn spawned_in_session(&self, _spawn_id: &str) -> bool {
        false
    }
}
