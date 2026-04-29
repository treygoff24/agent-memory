//! Public decision DTOs returned by the governance engine.

use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::{GovernanceError, GovernanceResult};

/// Baseline policy marker used until Task 3 adds policy loading.
pub const BASELINE_POLICY_APPLIED: &str = "stream_c_governance_v0_1";

/// Governance outcome category.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GovernanceStatus {
    /// The candidate can be promoted by the caller.
    Promoted,
    /// The candidate must not be written.
    Refused,
}

/// Next caller action implied by a governance decision.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NextAction {
    /// Caller may promote the candidate through Stream A substrate APIs.
    PromoteToSubstrate,
    /// Caller must not perform a write.
    NoWrite,
}

/// Stable refusal reason codes for fail-closed governance.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GovernanceRefusalReason {
    /// Grounding evidence is absent or insufficient.
    Grounding,
    /// Policy disallows the promotion.
    Policy,
    /// A tombstone blocks the promotion.
    Tombstone,
    /// Contradiction handling requires a non-promotion path.
    Contradiction,
    /// Privacy classification is unavailable or disallows the write.
    Privacy,
    /// The candidate has already been superseded.
    Superseded,
    /// Human review is required before any write.
    ReviewRequired,
}

impl FromStr for GovernanceRefusalReason {
    type Err = GovernanceError;

    fn from_str(reason_code: &str) -> Result<Self, Self::Err> {
        match reason_code {
            "grounding" => Ok(Self::Grounding),
            "policy" => Ok(Self::Policy),
            "tombstone" => Ok(Self::Tombstone),
            "contradiction" => Ok(Self::Contradiction),
            "privacy" => Ok(Self::Privacy),
            "superseded" => Ok(Self::Superseded),
            "review_required" => Ok(Self::ReviewRequired),
            unknown => Err(GovernanceError::UnknownRefusalReason { reason_code: unknown.to_owned() }),
        }
    }
}

/// Typed governance decision. This crate returns decisions only; callers own all writes.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum GovernanceDecision {
    /// Candidate passed governance and may be promoted by the caller.
    Promoted {
        /// Candidate memory id.
        id: String,
        /// Governance namespace used for policy selection.
        namespace: String,
        /// Stable policy marker that produced this decision.
        policy_applied: String,
        /// Existing memory id superseded by this promotion, when applicable.
        #[serde(skip_serializing_if = "Option::is_none")]
        supersedes: Option<String>,
        /// Caller action for the decision.
        next_action: NextAction,
    },
    /// Candidate failed governance and must not be written.
    Refused {
        /// Stable refusal reason code.
        reason: GovernanceRefusalReason,
        /// Operator-facing refusal explanation.
        message: String,
        /// Caller action for the decision.
        next_action: NextAction,
    },
}

impl GovernanceDecision {
    /// Build a promotion decision without mutating the substrate.
    pub fn promoted(id: impl Into<String>, namespace: impl Into<String>) -> Self {
        Self::Promoted {
            id: id.into(),
            namespace: namespace.into(),
            policy_applied: BASELINE_POLICY_APPLIED.to_owned(),
            supersedes: None,
            next_action: NextAction::PromoteToSubstrate,
        }
    }

    /// Build a refusal decision from a stable reason code.
    pub fn refused(reason_code: &str, message: impl Into<String>) -> GovernanceResult<Self> {
        Ok(Self::Refused {
            reason: GovernanceRefusalReason::from_str(reason_code)?,
            message: message.into(),
            next_action: NextAction::NoWrite,
        })
    }

    /// Attach supersession metadata to a promotion decision.
    #[must_use]
    pub fn with_supersedes(mut self, supersedes: impl Into<String>) -> Self {
        if let Self::Promoted { supersedes: promoted_supersedes, .. } = &mut self {
            *promoted_supersedes = Some(supersedes.into());
        }

        self
    }
}
