use memory_governance::{CandidateContext, PolicySet, Scope};

#[test]
fn me_strict_builtin_accepts_dogfood_confidence_floor() {
    let policies = PolicySet::builtin();
    let policy = policies.policy_for_scope(Scope::Me).expect("me policy");

    let preview = policy.dry_run(&CandidateContext::new(Scope::Me).with_confidence(0.85).with_grounding(true));

    assert_eq!(preview.selected_policy, "me-strict@v1");
    assert_eq!(preview.confidence_floor, 0.85);
    assert!(preview.confidence_floor_passed);
    assert!(preview.triggered_review_gates.is_empty());
}

#[test]
fn me_strict_builtin_still_reviews_below_dogfood_floor() {
    let policies = PolicySet::builtin();
    let policy = policies.policy_for_scope(Scope::Me).expect("me policy");

    let preview = policy.dry_run(&CandidateContext::new(Scope::Me).with_confidence(0.80).with_grounding(true));

    assert!(!preview.confidence_floor_passed);
    assert_eq!(preview.triggered_review_gates, vec!["low_confidence".to_string()]);
}

#[test]
fn non_me_policy_floors_remain_unchanged() {
    let policies = PolicySet::builtin();
    let agent = policies.policy_for_scope(Scope::Agent).expect("agent policy");
    let project = policies.policy_for_scope(Scope::Project).expect("project policy");
    let dreaming = policies.policy_for_scope(Scope::Dreaming).expect("dreaming policy");

    assert_eq!(agent.dry_run(&CandidateContext::new(Scope::Agent)).confidence_floor, 0.82);
    assert_eq!(project.dry_run(&CandidateContext::new(Scope::Project)).confidence_floor, 0.70);
    assert_eq!(dreaming.dry_run(&CandidateContext::new(Scope::Dreaming)).confidence_floor, 0.95);
}
