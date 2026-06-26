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
    /// Candidate should be written as candidate/pending_review, not active.
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
    /// Engine-level override for the contradiction similarity threshold. When
    /// `None` (the production default), the threshold comes from the *selected*
    /// policy's `contradiction` block (defaulting to the crate default when the
    /// policy omits it). When `Some`, it overrides every policy — used by the
    /// builder for tests and bespoke wiring.
    similarity_threshold_override: Option<f32>,
    /// Engine-level override for the contradiction top-K width; same semantics as
    /// [`similarity_threshold_override`](Self::similarity_threshold_override).
    top_k_limit_override: Option<usize>,
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
            similarity_threshold_override: None,
            top_k_limit_override: None,
        }
    }

    /// Override the contradiction similarity threshold for every policy.
    ///
    /// Without this, the threshold is read from the selected policy's
    /// `contradiction` block (see [`crate::ContradictionThresholds`]); this
    /// builder forces a single value regardless of policy.
    #[must_use]
    pub fn with_similarity_threshold(mut self, similarity_threshold: f32) -> Self {
        self.similarity_threshold_override = Some(similarity_threshold);
        self
    }

    /// Override the contradiction top-K retrieval width for every policy.
    ///
    /// Without this, the width is read from the selected policy's `contradiction`
    /// block; this builder forces a single value regardless of policy.
    #[must_use]
    pub fn with_top_k_limit(mut self, top_k_limit: usize) -> Self {
        self.top_k_limit_override = Some(top_k_limit);
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

        let contradiction_decision = self.detect_contradiction(candidate, policy);
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

    fn detect_contradiction(&self, candidate: &CandidateMemory, policy: &crate::Policy) -> ContradictionDecision {
        // Engine-level overrides win; otherwise the thresholds come from the
        // selected policy's `contradiction` block (crate defaults when the policy
        // omits it). Threading the selected policy through here is what makes the
        // YAML-tunable thresholds per-scope.
        let thresholds = policy.contradiction_thresholds();
        let similarity_threshold = self.similarity_threshold_override.unwrap_or(thresholds.similarity_threshold);
        let top_k_limit = self.top_k_limit_override.unwrap_or(thresholds.top_k);
        ContradictionDetector::new(&self.search, &self.tiebreaker)
            .with_similarity_threshold(similarity_threshold)
            .with_top_k_limit(top_k_limit)
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
