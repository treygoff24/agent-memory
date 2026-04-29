#[path = "../../memory-test-support/src/governance.rs"]
mod governance_fixtures;

use std::path::{Path, PathBuf};

use governance_fixtures::{
    GovernanceActor, GovernanceRelation, GovernanceScope, ACTOR_FIXTURES, RELATION_FIXTURES, SCOPE_POLICY_FIXTURES,
    SPAWNED_SUBAGENT_ID,
};
use memory_governance::{
    CandidateMemory, ContradictionTiebreaker, ExistingMemorySummary, FileSourceResolver, GovernanceEngine,
    GovernanceProviders, GovernanceRefusalReason, GovernanceWriteDecision, GroundingVerifier, NextWriteAction,
    PolicySet, Scope, SessionSpawnResolver, SimilaritySearch, Source, SourceKind, TiebreakOutcome, TombstoneIndex,
};

#[test]
fn governance_matrix_covers_actor_grounding_paths() {
    let grounding_file = write_grounding_file();

    for fixture in ACTOR_FIXTURES {
        let candidate = actor_candidate(fixture.actor, fixture.scope, fixture.claim, &grounding_file);
        let decision = engine(FakeSimilaritySearch::default(), FakeTiebreaker::new(TiebreakOutcome::Unclear))
            .evaluate_write(&candidate);

        match fixture.actor {
            GovernanceActor::User | GovernanceActor::GroundedAgent => assert_promoted(fixture.name, &decision),
            GovernanceActor::UngroundedAgent => {
                assert_refused_for(fixture.name, &decision, GovernanceRefusalReason::Grounding);
            }
            GovernanceActor::Subagent => assert!(
                matches!(
                    decision,
                    GovernanceWriteDecision::Candidate { .. } | GovernanceWriteDecision::Quarantined { .. }
                ),
                "{}: agent-strict subagent writes should be held for review, got {decision:?}",
                fixture.name
            ),
        }
    }

    std::fs::remove_file(grounding_file).expect("remove grounding fixture");
}

#[test]
fn governance_matrix_covers_duplicate_refinement_contradiction_and_tombstone_hits() {
    for fixture in RELATION_FIXTURES {
        let candidate = relation_candidate(fixture);
        let search = relation_search(fixture, &candidate);
        let tiebreaker = relation_tiebreaker(fixture);
        let tombstones = relation_tombstones(fixture);
        let decision = engine_with_tombstones(search, tiebreaker, tombstones).evaluate_write(&candidate);

        match fixture.relation {
            GovernanceRelation::Fresh => assert_promoted(fixture.name, &decision),
            GovernanceRelation::Duplicate => assert_eq!(
                decision,
                GovernanceWriteDecision::Duplicate {
                    existing_id: existing_id(fixture).to_owned(),
                    next_action: NextWriteAction::NoWrite,
                },
                "{}",
                fixture.name
            ),
            GovernanceRelation::Refinement => assert_eq!(
                decision,
                GovernanceWriteDecision::Refinement {
                    existing_id: existing_id(fixture).to_owned(),
                    candidate_id: candidate.id().to_owned(),
                    next_action: NextWriteAction::MergeEvidence,
                },
                "{}",
                fixture.name
            ),
            GovernanceRelation::Contradiction => assert_eq!(
                decision,
                GovernanceWriteDecision::Supersession {
                    existing_id: existing_id(fixture).to_owned(),
                    replacement_id: candidate.id().to_owned(),
                    policy_applied: "project-standard@v2".to_owned(),
                    next_action: NextWriteAction::SupersedeWithChain,
                },
                "{}",
                fixture.name
            ),
            GovernanceRelation::TombstoneHit => {
                assert_eq!(
                    decision,
                    GovernanceWriteDecision::Candidate {
                        id: candidate.id().to_owned(),
                        reason: "tombstone".to_owned(),
                        policy_applied: "project-standard@v2".to_owned(),
                        next_action: NextWriteAction::WriteCandidate,
                    },
                    "{}",
                    fixture.name
                );
            }
        }
    }
}

#[test]
fn governance_matrix_covers_scope_policy_selection() {
    for fixture in SCOPE_POLICY_FIXTURES {
        let candidate = CandidateMemory::new(
            format!("scope-{}", fixture.name),
            namespace_for_scope(fixture.scope),
            format!("{} scoped governance policy is deterministic.", fixture.name),
            scope(fixture.scope),
        )
        .with_confidence(fixture.confidence)
        .with_entity_ids(vec![format!("scope:{}", fixture.name)])
        .with_sources(vec![Source::new(SourceKind::User, None::<String>)])
        .with_explicit_user_context();

        let decision = engine(FakeSimilaritySearch::default(), FakeTiebreaker::new(TiebreakOutcome::Unclear))
            .evaluate_write(&candidate);

        match fixture.scope {
            GovernanceScope::Dreaming => assert_eq!(
                decision,
                GovernanceWriteDecision::Candidate {
                    id: candidate.id().to_owned(),
                    reason: "dream_source".to_owned(),
                    policy_applied: fixture.policy_applied.to_owned(),
                    next_action: NextWriteAction::WriteCandidate,
                },
                "{}",
                fixture.name
            ),
            _ => assert_eq!(
                decision,
                GovernanceWriteDecision::Promoted {
                    id: candidate.id().to_owned(),
                    namespace: candidate.namespace().to_owned(),
                    policy_applied: fixture.policy_applied.to_owned(),
                    next_action: NextWriteAction::PromoteToSubstrate,
                },
                "{}",
                fixture.name
            ),
        }
    }
}

fn actor_candidate(
    actor: GovernanceActor,
    governance_scope: GovernanceScope,
    claim: &str,
    grounding_file: &Path,
) -> CandidateMemory {
    let mut candidate = CandidateMemory::new(
        format!("actor-{actor:?}"),
        namespace_for_scope(governance_scope),
        claim,
        scope(governance_scope),
    )
    .with_confidence(0.96)
    .with_entity_ids(vec![format!("actor:{actor:?}")]);

    match actor {
        GovernanceActor::User => {
            candidate = candidate
                .with_sources(vec![Source::new(SourceKind::User, None::<String>)])
                .with_explicit_user_context();
        }
        GovernanceActor::GroundedAgent => {
            candidate = candidate.with_sources(vec![Source::new(
                SourceKind::AgentPrimary,
                Some(format!("file:{}", grounding_file.display())),
            )]);
        }
        GovernanceActor::UngroundedAgent => {
            candidate = candidate.with_sources(vec![Source::new(SourceKind::AgentPrimary, None::<String>)]);
        }
        GovernanceActor::Subagent => {
            candidate = candidate.with_sources(vec![Source::new(
                SourceKind::Subagent,
                Some(format!("session-spawn:{SPAWNED_SUBAGENT_ID}")),
            )]);
        }
    }

    candidate
}

fn relation_candidate(fixture: &governance_fixtures::RelationFixture) -> CandidateMemory {
    CandidateMemory::new(format!("relation-{}", fixture.name), "project", fixture.claim, Scope::Project)
        .with_confidence(0.96)
        .with_entity_ids(vec![fixture.entity.to_owned()])
        .with_sources(vec![Source::new(SourceKind::User, None::<String>)])
        .with_explicit_user_context()
}

fn relation_search(
    fixture: &governance_fixtures::RelationFixture,
    candidate: &CandidateMemory,
) -> FakeSimilaritySearch {
    match fixture.relation {
        GovernanceRelation::Fresh | GovernanceRelation::TombstoneHit => FakeSimilaritySearch::default(),
        GovernanceRelation::Duplicate => FakeSimilaritySearch {
            duplicate: Some(
                ExistingMemorySummary::new(existing_id(fixture), candidate.namespace(), candidate.claim(), 1.0)
                    .with_entity_ids(candidate.entity_ids().to_vec()),
            ),
            hits: Vec::new(),
        },
        GovernanceRelation::Refinement | GovernanceRelation::Contradiction => FakeSimilaritySearch {
            duplicate: None,
            hits: vec![ExistingMemorySummary::new(
                existing_id(fixture),
                candidate.namespace(),
                format!("Existing {}", candidate.claim()),
                0.93,
            )
            .with_entity_ids(candidate.entity_ids().to_vec())],
        },
    }
}

fn relation_tiebreaker(fixture: &governance_fixtures::RelationFixture) -> FakeTiebreaker {
    match fixture.relation {
        GovernanceRelation::Refinement => {
            FakeTiebreaker::new(TiebreakOutcome::Refinement { existing_id: existing_id(fixture).to_owned() })
        }
        GovernanceRelation::Contradiction => {
            FakeTiebreaker::new(TiebreakOutcome::Contradiction { existing_id: existing_id(fixture).to_owned() })
        }
        _ => FakeTiebreaker::new(TiebreakOutcome::Unclear),
    }
}

fn relation_tombstones(fixture: &governance_fixtures::RelationFixture) -> TombstoneIndex {
    if fixture.relation == GovernanceRelation::TombstoneHit {
        TombstoneIndex::load_jsonl_dir(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tombstones"))
            .expect("tombstone fixture loads")
    } else {
        TombstoneIndex::default()
    }
}

fn engine(
    search: FakeSimilaritySearch,
    tiebreaker: FakeTiebreaker,
) -> GovernanceEngine<FakeSimilaritySearch, FakeTiebreaker, FakeSessionSpawnResolver> {
    engine_with_tombstones(search, tiebreaker, TombstoneIndex::default())
}

fn engine_with_tombstones(
    search: FakeSimilaritySearch,
    tiebreaker: FakeTiebreaker,
    tombstones: TombstoneIndex,
) -> GovernanceEngine<FakeSimilaritySearch, FakeTiebreaker, FakeSessionSpawnResolver> {
    GovernanceEngine::new(
        PolicySet::builtin(),
        GroundingVerifier::new(FileSourceResolver, FakeSessionSpawnResolver),
        tombstones,
        GovernanceProviders::new(search, tiebreaker),
    )
}

fn scope(governance_scope: GovernanceScope) -> Scope {
    match governance_scope {
        GovernanceScope::Me => Scope::Me,
        GovernanceScope::Project => Scope::Project,
        GovernanceScope::Agent => Scope::Agent,
        GovernanceScope::Dreaming => Scope::Dreaming,
    }
}

fn namespace_for_scope(governance_scope: GovernanceScope) -> &'static str {
    match governance_scope {
        GovernanceScope::Me => "me",
        GovernanceScope::Project => "project",
        GovernanceScope::Agent => "agent",
        GovernanceScope::Dreaming => "dreaming",
    }
}

fn assert_promoted(case_name: &str, decision: &GovernanceWriteDecision) {
    assert!(
        matches!(decision, GovernanceWriteDecision::Promoted { .. }),
        "{case_name}: expected promoted decision, got {decision:?}"
    );
}

fn assert_refused_for(case_name: &str, decision: &GovernanceWriteDecision, expected: GovernanceRefusalReason) {
    assert!(
        matches!(decision, GovernanceWriteDecision::Refused { reason, .. } if *reason == expected),
        "{case_name}: expected refusal {expected:?}, got {decision:?}"
    );
}

fn existing_id(fixture: &governance_fixtures::RelationFixture) -> String {
    format!("existing-{}", fixture.name)
}

fn write_grounding_file() -> PathBuf {
    let path = std::env::temp_dir().join(format!("agent-memory-governance-grounding-{}.md", std::process::id()));
    std::fs::write(&path, "local evidence for a grounded agent write\n").expect("write grounding fixture");
    path
}

#[derive(Clone, Debug, Default)]
struct FakeSimilaritySearch {
    duplicate: Option<ExistingMemorySummary>,
    hits: Vec<ExistingMemorySummary>,
}

impl SimilaritySearch for FakeSimilaritySearch {
    fn find_active_by_claim_hash(&self, _candidate: &CandidateMemory) -> Option<ExistingMemorySummary> {
        self.duplicate.clone()
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

impl SessionSpawnResolver for FakeSessionSpawnResolver {
    fn spawned_in_session(&self, spawn_id: &str) -> bool {
        spawn_id == SPAWNED_SUBAGENT_ID
    }
}
