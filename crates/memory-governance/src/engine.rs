//! Governance write engine that composes policy, grounding, tombstone, and contradiction checks.

use serde::{Deserialize, Serialize};

use crate::{
    CandidateContext, CandidateMemory, ContradictionDecision, ContradictionDetector, ContradictionTiebreaker,
    GovernanceDecision, GovernanceRefusalReason, GroundingContext, GroundingVerifier, NeverResolveWebCapture,
    PolicySet, SessionSpawnResolver, SimilaritySearch, SourceKind, TombstoneEnforcementMode, TombstoneIndex,
    WebCaptureResolver,
};

/// Caller action implied by a governance write decision.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NextWriteAction {
    /// Promote the candidate as an active substrate memory.
    PromoteToSubstrate,
    /// Write the candidate with candidate status for later review.
    WriteCandidate,
    /// Write the candidate with quarantined status for later review.
    WriteQuarantined,
    /// Merge candidate evidence into an existing memory instead of creating a peer active memory.
    MergeEvidence,
    /// Execute a bidirectional supersession chain.
    SupersedeWithChain,
    /// Do not write anything.
    NoWrite,
}

/// Public engine decision for Stream C memory writes.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum GovernanceWriteDecision {
    /// Candidate passed governance and may be promoted.
    Promoted {
        /// Candidate memory id.
        id: String,
        /// Governance namespace.
        namespace: String,
        /// Policy marker used for the decision.
        policy_applied: String,
        /// Caller action.
        next_action: NextWriteAction,
    },
    /// Candidate should be written as candidate/pending-review, not active.
    Candidate {
        /// Candidate memory id.
        id: String,
        /// Review reason.
        reason: String,
        /// Policy marker used for the decision.
        policy_applied: String,
        /// Caller action.
        next_action: NextWriteAction,
    },
    /// Candidate should be quarantined.
    Quarantined {
        /// Candidate memory id.
        id: String,
        /// Quarantine reason.
        reason: String,
        /// Policy marker used for the decision.
        policy_applied: String,
        /// Caller action.
        next_action: NextWriteAction,
    },
    /// Candidate duplicates an existing active memory.
    Duplicate {
        /// Existing memory id.
        existing_id: String,
        /// Caller action.
        next_action: NextWriteAction,
    },
    /// Candidate should merge evidence into an existing memory.
    Refinement {
        /// Existing memory id.
        existing_id: String,
        /// Candidate memory id.
        candidate_id: String,
        /// Caller action.
        next_action: NextWriteAction,
    },
    /// Candidate should replace an existing memory through a supersession chain.
    Supersession {
        /// Existing memory id.
        existing_id: String,
        /// Replacement candidate id.
        replacement_id: String,
        /// Policy marker used for the decision.
        policy_applied: String,
        /// Caller action.
        next_action: NextWriteAction,
    },
    /// Candidate must not create or mutate a memory.
    Refused {
        /// Stable refusal reason.
        reason: GovernanceRefusalReason,
        /// Operator-facing message.
        message: String,
        /// Caller action.
        next_action: NextWriteAction,
    },
}

/// Provider bundle for contradiction detection.
#[derive(Clone, Debug)]
pub struct GovernanceProviders<S, T> {
    search: S,
    tiebreaker: T,
}

/// Engine configuration and providers.
#[derive(Clone, Debug)]
pub struct GovernanceEngine<S, T, R, W = NeverResolveWebCapture> {
    policies: PolicySet,
    grounding_verifier: GroundingVerifier<R, W>,
    tombstones: TombstoneIndex,
    search: S,
    tiebreaker: T,
    similarity_threshold: f32,
    top_k_limit: usize,
}

impl<S, T> GovernanceProviders<S, T> {
    /// Create a provider bundle.
    pub fn new(search: S, tiebreaker: T) -> Self {
        Self { search, tiebreaker }
    }
}

impl<S, T, R, W> GovernanceEngine<S, T, R, W>
where
    S: SimilaritySearch,
    T: ContradictionTiebreaker,
    R: SessionSpawnResolver,
    W: WebCaptureResolver,
{
    /// Build an engine from deterministic providers.
    pub fn new(
        policies: PolicySet,
        grounding_verifier: GroundingVerifier<R, W>,
        tombstones: TombstoneIndex,
        providers: GovernanceProviders<S, T>,
    ) -> Self {
        Self {
            policies,
            grounding_verifier,
            tombstones,
            search: providers.search,
            tiebreaker: providers.tiebreaker,
            similarity_threshold: 0.82,
            top_k_limit: 5,
        }
    }

    /// Override contradiction similarity threshold.
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

    /// Evaluate a candidate in Stream C contract order.
    pub fn evaluate_write(&self, candidate: &CandidateMemory) -> GovernanceWriteDecision {
        let policy_context = CandidateContext::new(candidate.scope())
            .with_confidence(candidate.confidence())
            .with_grounding(candidate.has_grounding());
        let policy = match self.policies.policy_for_candidate(&policy_context) {
            Ok(policy) => policy,
            Err(error) => return refused(GovernanceRefusalReason::Policy, error.to_string()),
        };
        let preview = policy.dry_run(&policy_context);
        let policy_applied = preview.selected_policy.clone();

        if let Some(refusal) = self.verify_grounding(candidate) {
            return refusal;
        }

        if let Some(tombstone_match) = self.tombstones.match_candidate(&candidate.tombstone_key()) {
            if policy.tombstone_enforcement() == TombstoneEnforcementMode::Review {
                return GovernanceWriteDecision::Candidate {
                    id: candidate.id().to_owned(),
                    reason: "tombstone".to_owned(),
                    policy_applied,
                    next_action: NextWriteAction::WriteCandidate,
                };
            }

            return match tombstone_match.decision {
                GovernanceDecision::Refused { reason, message, .. } => refused(reason, message),
                GovernanceDecision::Promoted { .. } => {
                    refused(GovernanceRefusalReason::Tombstone, "candidate matches tombstone")
                }
            };
        }

        let contradiction_decision = self.detect_contradiction(candidate);
        if !matches!(contradiction_decision, ContradictionDecision::NoConflict) {
            return self.map_contradiction(candidate, policy, contradiction_decision);
        }

        if candidate.sources().iter().any(|source| source.kind() == SourceKind::Subagent) {
            return GovernanceWriteDecision::Candidate {
                id: candidate.id().to_owned(),
                reason: "subagent_review".to_owned(),
                policy_applied,
                next_action: NextWriteAction::WriteCandidate,
            };
        }

        if let Some(reason) = preview.triggered_review_gates.first() {
            return GovernanceWriteDecision::Candidate {
                id: candidate.id().to_owned(),
                reason: reason.clone(),
                policy_applied,
                next_action: NextWriteAction::WriteCandidate,
            };
        }

        GovernanceWriteDecision::Promoted {
            id: candidate.id().to_owned(),
            namespace: candidate.namespace().to_owned(),
            policy_applied,
            next_action: NextWriteAction::PromoteToSubstrate,
        }
    }

    fn verify_grounding(&self, candidate: &CandidateMemory) -> Option<GovernanceWriteDecision> {
        let mut context = GroundingContext::new(candidate.id(), candidate.namespace(), candidate.sources().to_vec());
        if candidate.has_explicit_user_context() {
            context = context.with_explicit_user_context();
        }

        match self.grounding_verifier.verify(&context) {
            GovernanceDecision::Promoted { .. } => None,
            GovernanceDecision::Refused { reason, message, .. } => Some(refused(reason, message)),
        }
    }

    fn detect_contradiction(&self, candidate: &CandidateMemory) -> ContradictionDecision {
        ContradictionDetector::new(&self.search, &self.tiebreaker)
            .with_similarity_threshold(self.similarity_threshold)
            .with_top_k_limit(self.top_k_limit)
            .detect(candidate)
    }

    fn map_contradiction(
        &self,
        candidate: &CandidateMemory,
        policy: &crate::Policy,
        contradiction_decision: ContradictionDecision,
    ) -> GovernanceWriteDecision {
        let policy_applied = policy.policy_applied();

        match contradiction_decision {
            ContradictionDecision::Duplicate { existing_id } => {
                GovernanceWriteDecision::Duplicate { existing_id, next_action: NextWriteAction::NoWrite }
            }
            ContradictionDecision::Refinement { existing_id } => GovernanceWriteDecision::Refinement {
                existing_id,
                candidate_id: candidate.id().to_owned(),
                next_action: NextWriteAction::MergeEvidence,
            },
            ContradictionDecision::Contradiction { existing_id }
                if policy.contradiction_policy() == crate::ContradictionPolicy::Supersede =>
            {
                GovernanceWriteDecision::Supersession {
                    existing_id,
                    replacement_id: candidate.id().to_owned(),
                    policy_applied,
                    next_action: NextWriteAction::SupersedeWithChain,
                }
            }
            ContradictionDecision::Contradiction { .. } => GovernanceWriteDecision::Quarantined {
                id: candidate.id().to_owned(),
                reason: "contradiction".to_owned(),
                policy_applied,
                next_action: NextWriteAction::WriteQuarantined,
            },
            ContradictionDecision::Unclear => GovernanceWriteDecision::Quarantined {
                id: candidate.id().to_owned(),
                reason: "contradiction_unclear".to_owned(),
                policy_applied,
                next_action: NextWriteAction::WriteQuarantined,
            },
            ContradictionDecision::NoConflict => GovernanceWriteDecision::Promoted {
                id: candidate.id().to_owned(),
                namespace: candidate.namespace().to_owned(),
                policy_applied,
                next_action: NextWriteAction::PromoteToSubstrate,
            },
        }
    }
}

fn refused(reason: GovernanceRefusalReason, message: impl Into<String>) -> GovernanceWriteDecision {
    GovernanceWriteDecision::Refused { reason, message: message.into(), next_action: NextWriteAction::NoWrite }
}
