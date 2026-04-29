use memory_governance::{GovernanceWriteDecision, NextWriteAction, SupersessionPlan, SupersessionStatusTransition};

#[test]
fn supersession_contract_builds_daemon_executable_plan_from_contradiction_decision() {
    let decision = GovernanceWriteDecision::Supersession {
        existing_id: "mem_20260429_a1b2c3d4e5f60718_000001".to_string(),
        replacement_id: "mem_20260429_a1b2c3d4e5f60718_000002".to_string(),
        policy_applied: "project-standard@v2".to_string(),
        next_action: NextWriteAction::SupersedeWithChain,
    };

    let plan = SupersessionPlan::from_contradiction_decision(&decision, "new source contradicts the active memory")
        .expect("contradiction decision should plan supersession");

    assert_eq!(plan.old_id(), "mem_20260429_a1b2c3d4e5f60718_000001");
    assert_eq!(plan.new_id(), "mem_20260429_a1b2c3d4e5f60718_000002");
    assert_eq!(plan.reason(), "new source contradicts the active memory");
    assert_eq!(plan.frontmatter_mutations().supersedes, vec!["mem_20260429_a1b2c3d4e5f60718_000001"]);
    assert_eq!(plan.frontmatter_mutations().superseded_by, Vec::<String>::new());
    assert_eq!(
        plan.expected_status_transitions(),
        &[SupersessionStatusTransition {
            memory_id: "mem_20260429_a1b2c3d4e5f60718_000001".to_string(),
            from: "active".to_string(),
            to: "superseded".to_string(),
        }]
    );
}

#[test]
fn supersession_contract_rejects_non_supersession_decisions() {
    let decision = GovernanceWriteDecision::Promoted {
        id: "mem_20260429_a1b2c3d4e5f60718_000002".to_string(),
        namespace: "project/agent-memory".to_string(),
        policy_applied: "project-standard@v2".to_string(),
        next_action: NextWriteAction::PromoteToSubstrate,
    };

    let error = SupersessionPlan::from_contradiction_decision(&decision, "not a contradiction")
        .expect_err("only supersession decisions should produce a plan");

    assert_eq!(error.to_string(), "decision is not executable as a supersession plan");
}
