//! Deterministic policy for merge-on-dream proposals.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// Initial cosine floor. Live tuning is deliberately external to this crate.
pub const DEFAULT_MERGE_SIMILARITY_THRESHOLD: f32 = 0.8;

/// Device-local proposal lifecycle.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeProposalStatus {
    Proposed,
    Approved,
    Rejected,
    Invalidated,
    Applying,
    Applied,
    RolledBack,
    Quarantined,
}

impl MergeProposalStatus {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Rejected | Self::Invalidated | Self::Applied | Self::RolledBack | Self::Quarantined)
    }
}

/// Policy-relevant projection of one abstraction-vector candidate.
#[derive(Clone, Debug, PartialEq)]
pub struct MergeCandidate {
    pub id: String,
    pub status: String,
    pub trust_level: String,
    pub review_state: Option<String>,
    pub requires_user_confirmation: bool,
    pub encrypted: bool,
    pub passive_recall: bool,
    pub scope: String,
    pub canonical_namespace: Option<String>,
    pub memory_type: String,
    pub sensitivity: String,
    pub claim_locked: bool,
}

/// Runtime exclusion sets supplied by other waves and proposal storage.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MergeCandidateExclusions {
    pub nonterminal_proposal_sources: BTreeSet<String>,
    pub import_repair_lineage: BTreeSet<String>,
    pub backfill_manifest: BTreeSet<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum MergeCandidateError {
    #[error("a merge proposal requires at least one source")]
    Empty,
    #[error("duplicate source id: {0}")]
    Duplicate(String),
    #[error("source is not merge eligible: {id} ({reason})")]
    Ineligible { id: String, reason: &'static str },
    #[error("source {id} does not share the proposal's {field}")]
    Incompatible { id: String, field: &'static str },
}

/// Apply every W3 candidate fence that is independent of substrate mutation.
pub fn validate_merge_candidates(
    candidates: &[MergeCandidate],
    exclusions: &MergeCandidateExclusions,
) -> Result<(), MergeCandidateError> {
    let Some(first) = candidates.first() else {
        return Err(MergeCandidateError::Empty);
    };
    // W3 sensitivity fence: compute the set's min/max once, then apply the
    // no-downgrade rule relative to the whole set (not the first element). The
    // replacement sensitivity floors at the strictest source (set max).
    let mut set_min_rank = i32::MAX;
    let mut set_max_rank = i32::MIN;
    for candidate in candidates {
        let rank = sensitivity_rank(&candidate.sensitivity);
        if rank < set_min_rank {
            set_min_rank = rank;
        }
        if rank > set_max_rank {
            set_max_rank = rank;
        }
    }
    assert!(set_max_rank >= set_min_rank, "set max rank must be at least min rank");
    let mut ids = BTreeSet::new();
    for candidate in candidates {
        if !ids.insert(candidate.id.as_str()) {
            return Err(MergeCandidateError::Duplicate(candidate.id.clone()));
        }
        validate_candidate(candidate, exclusions)?;
        require_same(candidate, first, set_max_rank)?;
    }
    Ok(())
}

fn validate_candidate(
    candidate: &MergeCandidate,
    exclusions: &MergeCandidateExclusions,
) -> Result<(), MergeCandidateError> {
    let reject = |reason| MergeCandidateError::Ineligible { id: candidate.id.clone(), reason };
    if !matches!(candidate.status.as_str(), "active" | "pinned") {
        return Err(reject("status"));
    }
    if candidate.status == "pinned" && candidate.trust_level != "pinned" {
        return Err(reject("pinned lifecycle"));
    }
    if candidate.requires_user_confirmation
        || matches!(candidate.review_state.as_deref(), Some("pending" | "pending_review" | "pending-review"))
    {
        return Err(reject("pending review"));
    }
    if candidate.encrypted {
        return Err(reject("encrypted tier"));
    }
    if matches!(candidate.sensitivity.as_str(), "confidential" | "personal") {
        return Err(reject("encrypted tier"));
    }
    if !candidate.passive_recall {
        return Err(reject("passive recall disabled"));
    }
    if candidate.claim_locked {
        return Err(reject("claim locked"));
    }
    if exclusions.nonterminal_proposal_sources.contains(&candidate.id) {
        return Err(reject("source already proposed"));
    }
    if exclusions.import_repair_lineage.contains(&candidate.id) {
        return Err(reject("import repair lineage"));
    }
    if exclusions.backfill_manifest.contains(&candidate.id) {
        return Err(reject("backfill manifest"));
    }
    Ok(())
}

fn require_same(
    candidate: &MergeCandidate,
    first: &MergeCandidate,
    set_sensitivity_floor: i32,
) -> Result<(), MergeCandidateError> {
    let incompatible = |field| MergeCandidateError::Incompatible { id: candidate.id.clone(), field };
    if candidate.scope != first.scope {
        return Err(incompatible("scope"));
    }
    if candidate.canonical_namespace != first.canonical_namespace {
        return Err(incompatible("canonical namespace"));
    }
    if candidate.memory_type != first.memory_type {
        return Err(incompatible("memory type"));
    }
    // W3 no-downgrade sensitivity compatibility: the replacement sensitivity
    // floors at the strictest source (set max). A candidate whose sensitivity is
    // lower than the set's max would be below the replacement's floor, so it is
    // incompatible.
    if sensitivity_rank(&candidate.sensitivity) < set_sensitivity_floor {
        return Err(incompatible("sensitivity"));
    }
    Ok(())
}

fn sensitivity_rank(sensitivity: &str) -> i32 {
    match sensitivity {
        "public" => 0,
        "internal" => 1,
        "confidential" => 2,
        "personal" => 3,
        _ => -1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type CandidateMutation = fn(&mut MergeCandidate);

    fn candidate(id: &str) -> MergeCandidate {
        MergeCandidate {
            id: id.to_string(),
            status: "active".to_string(),
            trust_level: "trusted".to_string(),
            review_state: None,
            requires_user_confirmation: false,
            encrypted: false,
            passive_recall: true,
            scope: "project".to_string(),
            canonical_namespace: Some("project:one".to_string()),
            memory_type: "procedure".to_string(),
            sensitivity: "internal".to_string(),
            claim_locked: false,
        }
    }

    #[test]
    fn accepts_eligible_same_surface_sources() {
        assert_eq!(
            validate_merge_candidates(&[candidate("one"), candidate("two")], &MergeCandidateExclusions::default()),
            Ok(())
        );
    }

    #[test]
    fn rejects_each_single_source_fence() {
        let cases: [(&str, CandidateMutation); 7] = [
            ("status", |c| c.status = "candidate".into()),
            ("pending review", |c| c.review_state = Some("pending-review".into())),
            ("pending review", |c| c.requires_user_confirmation = true),
            ("encrypted tier", |c| c.encrypted = true),
            ("encrypted tier", |c| c.sensitivity = "confidential".into()),
            ("passive recall disabled", |c| c.passive_recall = false),
            ("claim locked", |c| c.claim_locked = true),
        ];
        for (reason, mutate) in cases {
            let mut value = candidate("one");
            mutate(&mut value);
            assert_eq!(
                validate_merge_candidates(&[value], &MergeCandidateExclusions::default()),
                Err(MergeCandidateError::Ineligible { id: "one".into(), reason })
            );
        }
    }

    #[test]
    fn rejects_external_exclusion_sets() {
        for field in ["proposal", "lineage", "manifest"] {
            let mut exclusions = MergeCandidateExclusions::default();
            match field {
                "proposal" => exclusions.nonterminal_proposal_sources.insert("one".into()),
                "lineage" => exclusions.import_repair_lineage.insert("one".into()),
                _ => exclusions.backfill_manifest.insert("one".into()),
            };
            assert!(validate_merge_candidates(&[candidate("one")], &exclusions).is_err());
        }
    }

    #[test]
    fn rejects_cross_surface_groups() {
        for mutate in [
            |c: &mut MergeCandidate| c.scope = "user".into(),
            |c: &mut MergeCandidate| c.canonical_namespace = Some("project:two".into()),
            |c: &mut MergeCandidate| c.memory_type = "person".into(),
            |c: &mut MergeCandidate| c.sensitivity = "public".into(),
        ] {
            let mut other = candidate("two");
            mutate(&mut other);
            assert!(matches!(
                validate_merge_candidates(&[candidate("one"), other], &MergeCandidateExclusions::default()),
                Err(MergeCandidateError::Incompatible { .. })
            ));
        }
    }

    #[test]
    fn sensitivity_mismatch_is_order_independent() {
        for (first_id, second_id) in [("one", "two"), ("two", "one")] {
            let mut first = candidate(first_id);
            first.sensitivity = "internal".into();
            let mut second = candidate(second_id);
            second.sensitivity = "public".into();
            let result = validate_merge_candidates(&[first, second], &MergeCandidateExclusions::default());
            let err = result.expect_err("mixed public/internal sensitivity must be rejected regardless of order");
            assert!(
                matches!(err, MergeCandidateError::Incompatible { field, .. } if field == "sensitivity"),
                "expected sensitivity mismatch, got {err:?}"
            );
        }
    }
}
