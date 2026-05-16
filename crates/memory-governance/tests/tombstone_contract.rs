#[path = "../src/tombstone.rs"]
mod tombstone;

use std::fs;
use std::path::PathBuf;

pub use memory_governance::{GovernanceDecision, GovernanceRefusalReason, NextAction};
use tombstone::{CandidateTombstoneKey, CanonicalEntities, TombstoneIndex, TombstoneLoadError};

#[test]
fn canonical_claim_hash_ignores_case_and_whitespace() {
    let compact = CandidateTombstoneKey::from_claim("Claim:   Keep   The   Red   Door", ["Home"]);
    let noisy = CandidateTombstoneKey::from_claim("  claim:\nkeep\tTHE red    door  ", ["Home"]);

    assert_eq!(compact.content_hash, noisy.content_hash);
}

#[test]
fn entity_set_order_does_not_change_tombstone_hash() {
    let first = CanonicalEntities::from(["Project:Atlas", "User:Trey", "Memory:Stream-C"]);
    let second = CanonicalEntities::from(["memory:stream-c", "user:trey", "project:atlas"]);

    assert_eq!(first.entity_hash(), second.entity_hash());
}

#[test]
fn matching_tombstone_refuses_with_tombstone_ref_details() {
    let index = TombstoneIndex::load_jsonl_dir(fixture_dir()).expect("fixture tombstones load");
    let candidate = CandidateTombstoneKey::from_claim("Claim: Keep The Red Door", ["Home"]);

    assert!(index.match_candidate(&candidate.clone().with_target_memory_id("mem_unrelated")).is_some());
    let matched = index.match_candidate(&candidate).expect("active tombstone matches");

    assert_eq!(matched.tombstone_ref.id, "tomb_20260429_0001");
    assert_eq!(matched.tombstone_ref.reason_text.as_deref(), Some("user asked to forget this claim"));
    assert_eq!(
        matched.decision,
        GovernanceDecision::Refused {
            reason: GovernanceRefusalReason::Tombstone,
            message: "candidate matches tombstone tomb_20260429_0001".to_owned(),
            next_action: NextAction::NoWrite,
        }
    );
}

#[test]
fn malformed_tombstone_jsonl_returns_typed_load_error_and_fails_closed() {
    let temp_dir = tempfile::tempdir().expect("create temp tombstone dir");
    fs::write(temp_dir.path().join("bad.jsonl"), "{not-json}\n").expect("write malformed tombstone");

    let load_error = TombstoneIndex::load_jsonl_dir(temp_dir.path()).expect_err("malformed JSONL is rejected");
    assert!(matches!(load_error, TombstoneLoadError::MalformedJsonl { line: 1, .. }));

    let decision = TombstoneIndex::fail_closed_decision(&load_error);
    assert_eq!(
        decision,
        GovernanceDecision::Refused {
            reason: GovernanceRefusalReason::Tombstone,
            message: "tombstone index failed to load; refusing candidate".to_owned(),
            next_action: NextAction::NoWrite,
        }
    );

    drop(temp_dir);
}

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tombstones")
}
