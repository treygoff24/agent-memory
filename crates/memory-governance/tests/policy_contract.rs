#[path = "../src/policy.rs"]
mod policy;

use std::path::Path;

use policy::{
    CandidateContext, ContradictionPolicy, ContradictionThresholds, Policy, PolicyError, PolicySet, PolicySource,
    Scope, TombstoneEnforcementMode, DEFAULT_CONTRADICTION_SIMILARITY_THRESHOLD, DEFAULT_CONTRADICTION_TOP_K,
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

    assert!(serde_yaml::from_str::<Policy>(yaml).is_err());
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

    assert!(serde_yaml::from_str::<Policy>(missing).is_err());
    assert!(serde_yaml::from_str::<Policy>(unknown).is_err());
    assert_eq!(
        PolicySet::builtin().policy_for_scope(Scope::Project).expect("project policy").contradiction_policy(),
        ContradictionPolicy::Supersede
    );
    assert_eq!(
        PolicySet::builtin().policy_for_scope(Scope::Project).expect("project policy").tombstone_enforcement(),
        TombstoneEnforcementMode::Review
    );
}

#[test]
fn policy_contract_omitting_contradiction_block_defaults_to_hardcoded_values() {
    // A policy with no `contradiction` block must behave exactly as before the
    // field existed: 0.82 / 5. This is the behavior-preservation guarantee.
    let yaml = r#"
name: agent-strict
version: 3
scope: agent
confidence_floor: 0.82
requires_grounding: true
tombstone_enforcement: refuse
contradiction_policy: quarantine
review_gates: []
"#;
    let policy = serde_yaml::from_str::<Policy>(yaml).expect("policy without contradiction block parses");
    let thresholds = policy.contradiction_thresholds();
    assert_eq!(thresholds.similarity_threshold, DEFAULT_CONTRADICTION_SIMILARITY_THRESHOLD);
    assert_eq!(thresholds.top_k, DEFAULT_CONTRADICTION_TOP_K);
    assert_eq!(ContradictionThresholds::default(), thresholds);

    // Built-ins carry the same defaults.
    let builtin = PolicySet::builtin();
    assert_eq!(builtin.policy_for_scope(Scope::Agent).expect("agent").contradiction_thresholds(), thresholds);
}

#[test]
fn policy_contract_contradiction_block_round_trips_set_values() {
    let yaml = r#"
name: agent-strict
version: 3
scope: agent
confidence_floor: 0.82
requires_grounding: true
tombstone_enforcement: refuse
contradiction_policy: quarantine
review_gates: []
contradiction:
  similarity_threshold: 0.7
  top_k: 12
"#;
    let policy = serde_yaml::from_str::<Policy>(yaml).expect("policy with contradiction block parses");
    let thresholds = policy.contradiction_thresholds();
    assert_eq!(thresholds.similarity_threshold, 0.7);
    assert_eq!(thresholds.top_k, 12);
}

#[test]
fn policy_contract_contradiction_block_partial_fields_fall_back_per_field() {
    // Only one of the two fields set; the other must default.
    let only_threshold = r#"
name: agent-strict
version: 3
scope: agent
confidence_floor: 0.82
requires_grounding: true
tombstone_enforcement: refuse
contradiction_policy: quarantine
review_gates: []
contradiction:
  similarity_threshold: 0.9
"#;
    let policy = serde_yaml::from_str::<Policy>(only_threshold).expect("partial block parses");
    assert_eq!(policy.contradiction_thresholds().similarity_threshold, 0.9);
    assert_eq!(policy.contradiction_thresholds().top_k, DEFAULT_CONTRADICTION_TOP_K);
}

#[test]
fn policy_contract_contradiction_block_rejects_unknown_keys() {
    let yaml = r#"
name: agent-strict
version: 3
scope: agent
confidence_floor: 0.82
requires_grounding: true
tombstone_enforcement: refuse
contradiction_policy: quarantine
review_gates: []
contradiction:
  similarity_threshold: 0.82
  bogus: 3
"#;
    assert!(serde_yaml::from_str::<Policy>(yaml).is_err(), "unknown contradiction key must be rejected");
}

#[test]
fn policy_contract_rejects_out_of_range_similarity_threshold() {
    for bad in ["1.5", "-0.1"] {
        let temp_dir = unique_temp_dir(&format!("bad-threshold-{}", bad.replace(['.', '-'], "_")));
        write_required_policy_set_without(&temp_dir, "agent-strict.yaml");
        write_policy(&temp_dir, "agent-strict.yaml", contradiction_threshold_yaml(bad, "5"));

        let error =
            PolicySet::load_from_dir(&temp_dir).expect_err("out-of-range similarity threshold must fail closed");

        assert!(
            matches!(error, PolicyError::InvalidContradictionThresholds { ref name, .. } if name == "agent-strict"),
            "got {error:?}"
        );
        std::fs::remove_dir_all(temp_dir).expect("remove temp policy dir");
    }
}

#[test]
fn policy_contract_rejects_zero_top_k() {
    let temp_dir = unique_temp_dir("zero-top-k");
    write_required_policy_set_without(&temp_dir, "agent-strict.yaml");
    write_policy(&temp_dir, "agent-strict.yaml", contradiction_threshold_yaml("0.82", "0"));

    let error = PolicySet::load_from_dir(&temp_dir).expect_err("zero top_k must fail closed");

    assert!(
        matches!(error, PolicyError::InvalidContradictionThresholds { ref name, .. } if name == "agent-strict"),
        "got {error:?}"
    );
    std::fs::remove_dir_all(temp_dir).expect("remove temp policy dir");
}

fn contradiction_threshold_yaml(similarity_threshold: &str, top_k: &str) -> String {
    format!(
        r#"
name: agent-strict
version: 3
scope: agent
confidence_floor: 0.82
requires_grounding: true
tombstone_enforcement: refuse
contradiction_policy: quarantine
review_gates: []
contradiction:
  similarity_threshold: {similarity_threshold}
  top_k: {top_k}
"#
    )
}

fn write_required_policy_set_without(path: &Path, omit: &str) {
    for (file, yaml) in [
        ("me-strict.yaml", PolicyFixture::me().to_yaml()),
        ("project-standard.yaml", PolicyFixture::project().to_yaml()),
        ("agent-strict.yaml", PolicyFixture::agent().to_yaml()),
        ("dreaming-strict.yaml", PolicyFixture::dreaming().to_yaml()),
    ] {
        if file != omit {
            write_policy(path, file, yaml);
        }
    }
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

    assert_eq!(serde_yaml::from_str::<Policy>(&yaml).is_ok(), should_parse);
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
