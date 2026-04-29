use std::error::Error;

use memory_governance::{GovernanceDecision, GovernanceError};

#[test]
fn decision_contract_refusal_serializes_stable_reason_codes() {
    let decision = GovernanceDecision::refused("grounding", "missing live-source support")
        .expect("grounding is a stable refusal reason");

    let json = serde_json::to_value(&decision).expect("decision serializes");

    assert_eq!(json["status"], "refused");
    assert_eq!(json["reason"], "grounding");
    assert_eq!(json["message"], "missing live-source support");
    assert_eq!(json["next_action"], "no_write");
    assert!(json.get("policy_applied").is_none());
    assert!(json.get("supersedes").is_none());

    let round_trip: GovernanceDecision = serde_json::from_value(json).expect("decision deserializes");
    assert_eq!(round_trip, decision);
}

#[test]
fn decision_contract_promoted_includes_policy_and_optional_supersession() {
    let promoted = GovernanceDecision::promoted("mem_20260429_0123456789abcdef_000001", "project/agent-memory");

    let json = serde_json::to_value(&promoted).expect("decision serializes");

    assert_eq!(json["status"], "promoted");
    assert_eq!(json["id"], "mem_20260429_0123456789abcdef_000001");
    assert_eq!(json["namespace"], "project/agent-memory");
    assert_eq!(json["policy_applied"], "stream_c_governance_v0_1");
    assert_eq!(json["next_action"], "promote_to_substrate");
    assert!(json.get("supersedes").is_none());

    let superseding = promoted.with_supersedes("mem_20260428_0123456789abcdef_000001");
    let superseding_json = serde_json::to_value(&superseding).expect("decision serializes");

    assert_eq!(superseding_json["supersedes"], "mem_20260428_0123456789abcdef_000001");
}

#[test]
fn decision_contract_errors_are_typed_without_anyhow_public_api() {
    fn assert_error<T: Error>() {}
    assert_error::<GovernanceError>();

    let error = GovernanceDecision::refused("not_a_reason", "unsupported reason")
        .expect_err("unknown refusal reason is rejected");

    assert_eq!(error.to_string(), "unknown governance refusal reason: not_a_reason");
}
