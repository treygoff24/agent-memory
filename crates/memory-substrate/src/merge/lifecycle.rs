//! Spec §14.5 lifecycle/status pair table.

use crate::model::{Frontmatter, MemoryStatus, TrustLevel};

use super::field_rules::QuarantineReason;

/// Result of merging the lifecycle pair `(ours_status, theirs_status)`.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum LifecycleOutcome {
    /// Take the named side's lifecycle wholesale.
    Take {
        /// Side to copy from.
        side: LifecycleSide,
        /// Whether `superseded_by` must be cleared (spec §14.5 #1).
        clear_superseded_by: bool,
        /// Operator-facing note for `lifecycle_notes[]`, when non-empty.
        note: Option<String>,
    },
    /// Status pair quarantines the document (e.g. archived vs superseded).
    Quarantine(QuarantineReason),
    /// Both sides agree; nothing to do.
    Continue,
}

/// Which side wins a lifecycle take.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum LifecycleSide {
    /// Local side.
    Ours,
    /// Incoming side.
    Theirs,
}

/// Apply the spec §14.5 lifecycle pair table to `(ours, theirs)` statuses.
///
/// The `base_status` is consulted only to decide which side actually changed
/// (so a no-op pair returns `Continue` rather than re-emitting diagnostics).
#[allow(clippy::too_many_arguments)]
pub(super) fn merge_lifecycle(
    base_status: MemoryStatus,
    ours_status: MemoryStatus,
    theirs_status: MemoryStatus,
    ours_superseded_by_valid: bool,
    theirs_superseded_by_valid: bool,
) -> LifecycleOutcome {
    use LifecycleSide::*;
    use MemoryStatus::*;
    let ours_changed = ours_status != base_status;
    let theirs_changed = theirs_status != base_status;
    if !ours_changed && !theirs_changed {
        return LifecycleOutcome::Continue;
    }
    if ours_status == theirs_status {
        return LifecycleOutcome::Take { side: Ours, clear_superseded_by: ours_status == Tombstoned, note: None };
    }
    // Asymmetric "one side unchanged" cases come first. When the changed side
    // moved to `superseded`, the spec §14.5 #4 validation still applies.
    if !theirs_changed {
        if ours_status == Superseded && !ours_superseded_by_valid {
            return LifecycleOutcome::Quarantine(QuarantineReason {
                conflicting_fields: vec!["status".to_string(), "superseded_by".to_string()],
                human_reason: "superseded but superseded_by failed validation - 14.5 #4".to_string(),
            });
        }
        return LifecycleOutcome::Take { side: Ours, clear_superseded_by: ours_status == Tombstoned, note: None };
    }
    if !ours_changed {
        if theirs_status == Superseded && !theirs_superseded_by_valid {
            return LifecycleOutcome::Quarantine(QuarantineReason {
                conflicting_fields: vec!["status".to_string(), "superseded_by".to_string()],
                human_reason: "superseded but superseded_by failed validation - 14.5 #4".to_string(),
            });
        }
        return LifecycleOutcome::Take { side: Theirs, clear_superseded_by: theirs_status == Tombstoned, note: None };
    }
    // Both sides changed; consult the pair table.
    match (ours_status, theirs_status) {
        // §14.5 #1: tombstone wins everywhere; clears `superseded_by`.
        (Tombstoned, _) => LifecycleOutcome::Take {
            side: Ours,
            clear_superseded_by: true,
            note: Some("tombstone clears superseded_by per 14.5 #1".to_string()),
        },
        (_, Tombstoned) => LifecycleOutcome::Take {
            side: Theirs,
            clear_superseded_by: true,
            note: Some("tombstone clears superseded_by per 14.5 #1".to_string()),
        },
        // §14.5 #2: quarantined wins over anything except tombstone (handled above).
        (Quarantined, _) => LifecycleOutcome::Take {
            side: Ours,
            clear_superseded_by: false,
            note: Some("quarantined wins per 14.5 #2".to_string()),
        },
        (_, Quarantined) => LifecycleOutcome::Take {
            side: Theirs,
            clear_superseded_by: false,
            note: Some("quarantined wins per 14.5 #2".to_string()),
        },
        // §14.5 #5: archived vs superseded → quarantine unless both sides have
        // compatible lifecycle diagnostics (Phase 4 conservatively quarantines;
        // the spec carve-out for "both have compatible diagnostics" is left as
        // a future relaxation when we have a concrete diagnostic schema for it).
        (Archived, Superseded) | (Superseded, Archived) => LifecycleOutcome::Quarantine(QuarantineReason {
            conflicting_fields: vec!["status".to_string()],
            human_reason: "archived vs superseded requires admin review - 14.5 #5".to_string(),
        }),
        // §14.5 #4: superseded beats active/candidate only if `superseded_by`
        // survives validation.
        (Superseded, Active) | (Superseded, Candidate) => {
            if ours_superseded_by_valid {
                LifecycleOutcome::Take {
                    side: Ours,
                    clear_superseded_by: false,
                    note: Some("superseded beats active per 14.5 #4".to_string()),
                }
            } else {
                LifecycleOutcome::Quarantine(QuarantineReason {
                    conflicting_fields: vec!["status".to_string(), "superseded_by".to_string()],
                    human_reason: "superseded but superseded_by failed validation - 14.5 #4".to_string(),
                })
            }
        }
        (Active, Superseded) | (Candidate, Superseded) => {
            if theirs_superseded_by_valid {
                LifecycleOutcome::Take {
                    side: Theirs,
                    clear_superseded_by: false,
                    note: Some("superseded beats active per 14.5 #4".to_string()),
                }
            } else {
                LifecycleOutcome::Quarantine(QuarantineReason {
                    conflicting_fields: vec!["status".to_string(), "superseded_by".to_string()],
                    human_reason: "superseded but superseded_by failed validation - 14.5 #4".to_string(),
                })
            }
        }
        // §14.5 #3: pinned beats active/candidate.
        (Pinned, Active) | (Pinned, Candidate) => LifecycleOutcome::Take {
            side: Ours,
            clear_superseded_by: false,
            note: Some("pinned beats active/candidate per 14.5 #3".to_string()),
        },
        (Active, Pinned) | (Candidate, Pinned) => LifecycleOutcome::Take {
            side: Theirs,
            clear_superseded_by: false,
            note: Some("pinned beats active/candidate per 14.5 #3".to_string()),
        },
        // §14.5 #5: archived beats active/candidate.
        (Archived, Active) | (Archived, Candidate) => LifecycleOutcome::Take {
            side: Ours,
            clear_superseded_by: false,
            note: Some("archived beats active/candidate per 14.5 #5".to_string()),
        },
        (Active, Archived) | (Candidate, Archived) => LifecycleOutcome::Take {
            side: Theirs,
            clear_superseded_by: false,
            note: Some("archived beats active/candidate per 14.5 #5".to_string()),
        },
        // §14.5 #6: active beats candidate.
        (Active, Candidate) => LifecycleOutcome::Take {
            side: Ours,
            clear_superseded_by: false,
            note: Some("active beats candidate per 14.5 #6".to_string()),
        },
        (Candidate, Active) => LifecycleOutcome::Take {
            side: Theirs,
            clear_superseded_by: false,
            note: Some("active beats candidate per 14.5 #6".to_string()),
        },
        // Pinned vs archived, pinned vs superseded — spec §14.5 doesn't enumerate
        // these directly; quarantine for admin review keeps us safe.
        (Pinned, Archived) | (Archived, Pinned) | (Pinned, Superseded) | (Superseded, Pinned) => {
            LifecycleOutcome::Quarantine(QuarantineReason {
                conflicting_fields: vec!["status".to_string()],
                human_reason: format!("lifecycle pair ({ours_status:?}, {theirs_status:?}) requires admin review"),
            })
        }
        // Same-status (handled above) and any pair we haven't enumerated.
        _ => LifecycleOutcome::Take { side: LifecycleSide::Ours, clear_superseded_by: false, note: None },
    }
}

/// Apply a [`LifecycleOutcome::Take`] to the merged frontmatter, copying
/// status / trust_level / review_state / tombstone_events / superseded_by
/// from the chosen side. Spec §14.5 #1: when `clear_superseded_by` is true,
/// the field is wiped regardless of source side.
#[allow(clippy::too_many_arguments)]
pub(super) fn apply_lifecycle_take(
    merged: &mut Frontmatter,
    side: LifecycleSide,
    ours: &Frontmatter,
    theirs: &Frontmatter,
    clear_superseded_by: bool,
) {
    let source = match side {
        LifecycleSide::Ours => ours,
        LifecycleSide::Theirs => theirs,
    };
    merged.status = source.status;
    merged.trust_level = source.trust_level;
    merged.review_state = source.review_state.clone();
    merged.tombstone_events = source.tombstone_events.clone();
    if clear_superseded_by {
        merged.superseded_by.clear();
    } else {
        merged.superseded_by = source.superseded_by.clone();
    }
    if matches!(merged.status, MemoryStatus::Quarantined) {
        merged.trust_level = TrustLevel::Quarantined;
        merged.review_state = Some("pending".to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pair_table_tombstone_clears_superseded() {
        let outcome =
            merge_lifecycle(MemoryStatus::Active, MemoryStatus::Tombstoned, MemoryStatus::Superseded, true, true);
        assert!(matches!(outcome, LifecycleOutcome::Take { side: LifecycleSide::Ours, clear_superseded_by: true, .. }));
    }

    #[test]
    fn pair_table_archived_vs_superseded_quarantines() {
        let outcome =
            merge_lifecycle(MemoryStatus::Active, MemoryStatus::Archived, MemoryStatus::Superseded, true, true);
        assert!(matches!(outcome, LifecycleOutcome::Quarantine(_)));
    }

    #[test]
    fn pair_table_superseded_invalid_quarantines() {
        let outcome =
            merge_lifecycle(MemoryStatus::Active, MemoryStatus::Superseded, MemoryStatus::Active, false, true);
        assert!(matches!(outcome, LifecycleOutcome::Quarantine(_)));
    }

    #[test]
    fn pair_table_active_vs_candidate_active_wins() {
        let outcome =
            merge_lifecycle(MemoryStatus::Candidate, MemoryStatus::Active, MemoryStatus::Candidate, true, true);
        assert!(matches!(outcome, LifecycleOutcome::Take { side: LifecycleSide::Ours, .. }));
    }

    #[test]
    fn pair_table_no_change_continues() {
        let outcome = merge_lifecycle(MemoryStatus::Active, MemoryStatus::Active, MemoryStatus::Active, true, true);
        assert!(matches!(outcome, LifecycleOutcome::Continue));
    }
}
