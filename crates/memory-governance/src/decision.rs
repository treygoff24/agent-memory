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

impl GovernanceRefusalReason {
    /// Stable snake_case code for this reason — the canonical runtime accessor.
    /// `FromStr` is its exact inverse and the `#[serde(rename_all = "snake_case")]`
    /// derive its serialization; the `as_str_matches_serde_and_roundtrips` test
    /// locks all three together so a new variant cannot silently diverge.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Grounding => "grounding",
            Self::Policy => "policy",
            Self::Tombstone => "tombstone",
            Self::Contradiction => "contradiction",
            Self::Privacy => "privacy",
            Self::Superseded => "superseded",
            Self::ReviewRequired => "review_required",
        }
    }
}

impl FromStr for GovernanceRefusalReason {
    type Err = GovernanceError;

    /// Parse a stable snake_case reason code. This is the inverse of the
    /// `#[serde(rename_all = "snake_case")]` derive on the enum, kept aligned by
    /// hand; adding a variant means extending this match too.
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

#[cfg(test)]
mod refusal_reason_contract {
    use super::*;

    /// Lock the three hand-aligned representations of GovernanceRefusalReason
    /// together so a new variant cannot silently diverge: `as_str` (runtime
    /// accessor), the `#[serde(rename_all = "snake_case")]` serialization, and
    /// `FromStr` (the inverse). Every variant must round-trip through all three.
    #[test]
    fn as_str_matches_serde_and_roundtrips() {
        let variants = [
            GovernanceRefusalReason::Grounding,
            GovernanceRefusalReason::Policy,
            GovernanceRefusalReason::Tombstone,
            GovernanceRefusalReason::Contradiction,
            GovernanceRefusalReason::Privacy,
            GovernanceRefusalReason::Superseded,
            GovernanceRefusalReason::ReviewRequired,
        ];
        for variant in variants {
            // expect-justified: contract test asserts the canonical reason code.
            let serde_code = serde_json::to_value(variant).expect("serialize").as_str().expect("string").to_owned();
            assert_eq!(variant.as_str(), serde_code, "as_str must equal the serde rename");
            // expect-justified: contract test asserts FromStr is the inverse.
            assert_eq!(GovernanceRefusalReason::from_str(variant.as_str()).expect("parse"), variant);
        }
    }
}
