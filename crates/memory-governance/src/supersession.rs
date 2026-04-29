//! Typed supersession plans for daemon-executed lifecycle writes.

use serde::{Deserialize, Serialize};

use crate::{GovernanceWriteDecision, NextWriteAction};

const ACTIVE_STATUS: &str = "active";
const SUPERSEDED_STATUS: &str = "superseded";

/// Error returned when a governance decision cannot produce a supersession plan.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SupersessionPlanError {
    /// Only `GovernanceWriteDecision::Supersession` is executable here.
    #[error("decision is not executable as a supersession plan")]
    NotSupersession,
    /// A supersession decision must direct the daemon to execute the lifecycle API.
    #[error("supersession decision has non-supersession next action")]
    NonExecutableAction,
}

/// Frontmatter changes the daemon applies to the replacement before handing it to Stream A.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SupersessionFrontmatterMutations {
    /// Old memory ids that the replacement supersedes.
    pub supersedes: Vec<String>,
    /// Newer memories superseding the replacement; empty for a fresh replacement.
    pub superseded_by: Vec<String>,
}

/// Expected old-memory lifecycle transition.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SupersessionStatusTransition {
    /// Memory id whose status should transition.
    pub memory_id: String,
    /// Expected prior status.
    pub from: String,
    /// Expected next status.
    pub to: String,
}

/// Governance-produced plan. This crate does not mutate files.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SupersessionPlan {
    old_id: String,
    new_id: String,
    frontmatter_mutations: SupersessionFrontmatterMutations,
    reason: String,
    expected_status_transitions: Vec<SupersessionStatusTransition>,
}

impl SupersessionPlan {
    /// Build a daemon-executable plan from a contradiction supersession decision.
    pub fn from_contradiction_decision(
        decision: &GovernanceWriteDecision,
        reason: impl Into<String>,
    ) -> Result<Self, SupersessionPlanError> {
        match decision {
            GovernanceWriteDecision::Supersession { existing_id, replacement_id, next_action, .. } => {
                if *next_action != NextWriteAction::SupersedeWithChain {
                    return Err(SupersessionPlanError::NonExecutableAction);
                }

                Ok(Self::new(existing_id.clone(), replacement_id.clone(), reason))
            }
            _ => Err(SupersessionPlanError::NotSupersession),
        }
    }

    /// Construct a supersession plan directly.
    pub fn new(old_id: String, new_id: String, reason: impl Into<String>) -> Self {
        Self {
            frontmatter_mutations: SupersessionFrontmatterMutations {
                supersedes: vec![old_id.clone()],
                superseded_by: Vec::new(),
            },
            expected_status_transitions: vec![SupersessionStatusTransition {
                memory_id: old_id.clone(),
                from: ACTIVE_STATUS.to_string(),
                to: SUPERSEDED_STATUS.to_string(),
            }],
            old_id,
            new_id,
            reason: reason.into(),
        }
    }

    /// Existing memory id.
    pub fn old_id(&self) -> &str {
        &self.old_id
    }

    /// Replacement memory id.
    pub fn new_id(&self) -> &str {
        &self.new_id
    }

    /// Operator/governance reason.
    pub fn reason(&self) -> &str {
        &self.reason
    }

    /// New-memory frontmatter mutations.
    pub fn frontmatter_mutations(&self) -> &SupersessionFrontmatterMutations {
        &self.frontmatter_mutations
    }

    /// Expected status transitions.
    pub fn expected_status_transitions(&self) -> &[SupersessionStatusTransition] {
        &self.expected_status_transitions
    }
}
