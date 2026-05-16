#[path = "../src/policy.rs"]
mod policy;

use std::path::Path;

use policy::{
    CandidateContext, ContradictionPolicy, Policy, PolicyError, PolicySet, PolicySource, Scope,
    TombstoneEnforcementMode,
};

const FIXTURE_DIR: &str = "tests/fixtures/policies";

#[test]
fn policy_contract_all_fixture_policies_parse() {
    let policies = PolicySet::load_from_dir(Path::new(FIXTURE_DIR)).expect("fixtures should parse");

    assert_eq!(policies.policy_named("me-strict").expect("me-strict").source(), PolicySource::Disk);
    assert_eq!(policies.policy_named("project-standard").expect("project-standard").source(), PolicySource::Disk);
    assert_eq!(policies.policy_named("agent-strict").expect("agent-strict").source(), PolicySource::Disk);
    assert_eq!(policies.policy_named("dreaming-strict").expect("dreaming-strict").source(), PolicySource::Disk);
}

#[test]
fn policy_contract_repo_policies_parse_as_disk_policies() {
    let repo_policies = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../policies");
    let policies = PolicySet::load_from_dir(&repo_policies).expect("repo policies should parse");

    assert_eq!(policies.policy_for_scope(Scope::Me).expect("me policy").policy_applied(), "me-strict@v1");
    assert_eq!(
        policies.policy_for_scope(Scope::Project).expect("project policy").policy_applied(),
        "project-standard@v2"
    );
    assert_eq!(policies.policy_for_scope(Scope::Agent).expect("agent policy").policy_applied(), "agent-strict@v3");
    assert_eq!(
        policies.policy_for_scope(Scope::Dreaming).expect("dreaming policy").policy_applied(),
        "dreaming-strict@v1"
    );
}

#[test]
fn policy_contract_unknown_yaml_keys_are_rejected() {
    let yaml = r#"
name: agent-strict
version: 3
scope: agent
confidence_floor: 0.82
requires_grounding: true
tombstone_enforcement: refuse
contradiction_policy: quarantine
review_gates: []
unexpected: nope
"#;

    assert!(yaml_serde::from_str::<Policy>(yaml).is_err());
}

#[test]
fn policy_contract_confidence_floor_must_be_between_zero_and_one() {
    assert_policy_confidence_floor("0.0", true);
    assert_policy_confidence_floor("1.0", true);
    assert_policy_confidence_floor("-0.01", false);
    assert_policy_confidence_floor("1.01", false);
}

#[test]
fn policy_contract_policy_applied_includes_name_and_version() {
    let policies = PolicySet::load_from_dir(Path::new(FIXTURE_DIR)).expect("fixtures should parse");

    assert_eq!(policies.policy_named("agent-strict").expect("agent-strict").policy_applied(), "agent-strict@v3");
}

#[test]
fn policy_contract_policy_for_agent_scope_resolves_agent_strict() {
    let policies = PolicySet::builtin();
    let context = CandidateContext::new(Scope::Agent).with_confidence(0.95).with_grounding(true);

    let policy = policies.policy_for_scope(Scope::Agent).expect("agent policy");
    let selected = policies.policy_for_candidate(&context).expect("candidate policy");

    assert_eq!(policy.name(), "agent-strict");
    assert_eq!(selected.name(), "agent-strict");
}

#[test]
fn policy_contract_dry_run_reports_policy_gates_and_enforcement() {
    let policies = PolicySet::builtin();
    let context = CandidateContext::new(Scope::Agent).with_confidence(0.72).with_grounding(false);

    let policy = policies.policy_for_candidate(&context).expect("agent policy");
    let preview = policy.dry_run(&context);

    assert_eq!(preview.selected_policy, "agent-strict@v3");
    assert_eq!(preview.policy_source, PolicySource::BuiltInFallback);
    assert!(!preview.confidence_floor_passed);
    assert_eq!(preview.triggered_review_gates, vec!["low_confidence".to_owned(), "missing_grounding".to_owned()]);
    assert!(preview.requires_grounding);
    assert!(!preview.grounding_satisfied);
    assert_eq!(preview.tombstone_enforcement, TombstoneEnforcementMode::Refuse);
}

#[test]
fn policy_contract_requires_all_scopes() {
    let temp_dir = unique_temp_dir("missing-scope");
    write_policy(&temp_dir, "me-strict.yaml", PolicyFixture::me().to_yaml());
    write_policy(&temp_dir, "project-standard.yaml", PolicyFixture::project().to_yaml());
    write_policy(&temp_dir, "agent-strict.yaml", PolicyFixture::agent().to_yaml());

    let error = PolicySet::load_from_dir(&temp_dir).expect_err("missing dreaming scope should fail closed");

    assert!(matches!(error, PolicyError::MissingPolicyForScope { scope: Scope::Dreaming }));
    std::fs::remove_dir_all(temp_dir).expect("remove temp policy dir");
}

#[test]
fn policy_contract_rejects_duplicate_scopes() {
    let temp_dir = unique_temp_dir("duplicate-scope");
    write_required_policy_set(&temp_dir);
    write_policy(&temp_dir, "project-alternate.yaml", PolicyFixture::project_alternate().to_yaml());

    let error = PolicySet::load_from_dir(&temp_dir).expect_err("duplicate project scope should fail closed");

    assert!(matches!(error, PolicyError::DuplicatePolicyScope { scope: Scope::Project }));
    std::fs::remove_dir_all(temp_dir).expect("remove temp policy dir");
}

#[test]
fn policy_contract_rejects_duplicate_names() {
    let temp_dir = unique_temp_dir("duplicate-name");
    write_required_policy_set(&temp_dir);
    write_policy(&temp_dir, "duplicate-name.yaml", PolicyFixture::duplicate_agent_name().to_yaml());

    let error = PolicySet::load_from_dir(&temp_dir).expect_err("duplicate policy name should fail closed");

    assert!(matches!(error, PolicyError::DuplicatePolicyName { name } if name == "agent-strict"));
    std::fs::remove_dir_all(temp_dir).expect("remove temp policy dir");
}

#[test]
fn policy_contract_contradiction_policy_is_required_and_typed() {
    let missing = r#"
name: agent-strict
version: 3
scope: agent
confidence_floor: 0.82
requires_grounding: true
tombstone_enforcement: refuse
review_gates: []
"#;
    let unknown = r#"
name: agent-strict
version: 3
scope: agent
confidence_floor: 0.82
requires_grounding: true
tombstone_enforcement: refuse
contradiction_policy: ask_around
review_gates: []
"#;

    assert!(yaml_serde::from_str::<Policy>(missing).is_err());
    assert!(yaml_serde::from_str::<Policy>(unknown).is_err());
    assert_eq!(
        PolicySet::builtin().policy_for_scope(Scope::Project).expect("project policy").contradiction_policy(),
        ContradictionPolicy::Supersede
    );
    assert_eq!(
        PolicySet::builtin().policy_for_scope(Scope::Project).expect("project policy").tombstone_enforcement(),
        TombstoneEnforcementMode::Review
    );
}

fn assert_policy_confidence_floor(confidence_floor: &str, should_parse: bool) {
    let yaml = format!(
        r#"
name: agent-strict
version: 3
scope: agent
confidence_floor: {confidence_floor}
requires_grounding: true
tombstone_enforcement: refuse
contradiction_policy: quarantine
review_gates: []
"#
    );

    assert_eq!(yaml_serde::from_str::<Policy>(&yaml).is_ok(), should_parse);
}

fn write_required_policy_set(path: &Path) {
    write_policy(path, "me-strict.yaml", PolicyFixture::me().to_yaml());
    write_policy(path, "project-standard.yaml", PolicyFixture::project().to_yaml());
    write_policy(path, "agent-strict.yaml", PolicyFixture::agent().to_yaml());
    write_policy(path, "dreaming-strict.yaml", PolicyFixture::dreaming().to_yaml());
}

struct PolicyFixture {
    name: &'static str,
    version: u32,
    scope: &'static str,
    tombstone: &'static str,
    contradiction: &'static str,
}

impl PolicyFixture {
    fn me() -> Self {
        Self { name: "me-strict", version: 1, scope: "me", tombstone: "refuse", contradiction: "quarantine" }
    }

    fn project() -> Self {
        Self { name: "project-standard", version: 2, scope: "project", tombstone: "review", contradiction: "supersede" }
    }

    fn agent() -> Self {
        Self { name: "agent-strict", version: 3, scope: "agent", tombstone: "refuse", contradiction: "quarantine" }
    }

    fn dreaming() -> Self {
        Self {
            name: "dreaming-strict",
            version: 1,
            scope: "dreaming",
            tombstone: "refuse",
            contradiction: "quarantine",
        }
    }

    fn project_alternate() -> Self {
        Self {
            name: "project-alternate",
            version: 1,
            scope: "project",
            tombstone: "review",
            contradiction: "supersede",
        }
    }

    fn duplicate_agent_name() -> Self {
        Self { name: "agent-strict", version: 4, scope: "project", tombstone: "review", contradiction: "supersede" }
    }

    fn to_yaml(&self) -> String {
        let Self { name, version, scope, tombstone, contradiction } = self;
        format!(
            r#"
name: {name}
version: {version}
scope: {scope}
confidence_floor: 0.9
requires_grounding: true
tombstone_enforcement: {tombstone}
contradiction_policy: {contradiction}
review_gates: []
"#
        )
    }
}

fn write_policy(path: &Path, file_name: &str, yaml: String) {
    std::fs::create_dir_all(path).expect("create temp policy dir");
    std::fs::write(path.join(file_name), yaml).expect("write temp policy");
}

fn unique_temp_dir(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("memory-governance-policy-{name}-{}", std::process::id()))
}
