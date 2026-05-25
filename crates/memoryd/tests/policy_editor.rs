use memory_substrate::{events::EventKind, InitOptions, Roots, Substrate};
use memoryd::handlers::handle_request;
use memoryd::protocol::{RequestEnvelope, RequestPayload, ResponsePayload, ResponseResult};

const PROJECT_POLICY: &str = r#"name: project-standard
version: 2
scope: project
confidence_floor: 0.7
requires_grounding: true
tombstone_enforcement: review
contradiction_policy: supersede
review_gates:
  - low_confidence
"#;

#[tokio::test]
async fn policy_validate_accepts_valid_yaml_without_writing() {
    let (_temp, substrate) = seeded_substrate().await;
    let original = read_project_policy(&substrate);
    let updated = PROJECT_POLICY.replace("confidence_floor: 0.7", "confidence_floor: 0.72");

    let response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "policy-validate",
            RequestPayload::PolicyValidate { raw_yaml: updated, file_name: Some("project-standard.yaml".to_owned()) },
        ),
    )
    .await;

    let ResponseResult::Success(ResponsePayload::PolicyValidate(result)) = response.result else {
        panic!("expected policy validate success, got {:?}", response.result);
    };
    assert!(result.accepted);
    assert_eq!(result.file_name, "project-standard.yaml");
    assert!(result.policies.iter().any(|policy| policy.scope == "project"));
    assert_eq!(read_project_policy(&substrate), original);
}

#[tokio::test]
async fn policy_dump_materializes_writable_builtin_templates_for_fresh_repo() {
    let (_temp, substrate) = fresh_substrate("dev_policyfresh").await;

    let response =
        handle_request(&substrate, RequestEnvelope::new("policy-dump-fresh", RequestPayload::GovernancePolicyDump))
            .await;

    let ResponseResult::Success(ResponsePayload::GovernancePolicyDump(snapshot)) = response.result else {
        panic!("expected policy dump success, got {:?}", response.result);
    };
    assert!(snapshot.writable);
    assert_eq!(snapshot.source, "disk");
    assert!(snapshot.raw_yaml.as_deref().is_some_and(|yaml| yaml.contains("name:")));
    assert!(snapshot.files.iter().any(|file| file == "project-standard.yaml"));
    assert!(substrate.roots().repo.join("policies/project-standard.yaml").is_file());
}

#[tokio::test]
async fn policy_write_on_fresh_repo_bootstraps_complete_policy_set() {
    let (_temp, substrate) = fresh_substrate("dev_policywrite").await;

    let response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "policy-write-fresh",
            RequestPayload::PolicyWrite {
                raw_yaml: PROJECT_POLICY.replace("confidence_floor: 0.7", "confidence_floor: 0.73"),
                file_name: Some("project-standard.yaml".to_owned()),
            },
        ),
    )
    .await;

    let ResponseResult::Success(ResponsePayload::PolicyWrite(result)) = response.result else {
        panic!("expected policy write success, got {:?}", response.result);
    };
    assert!(result.accepted);
    assert!(read_project_policy(&substrate).contains("confidence_floor: 0.73"));
    assert!(substrate.roots().repo.join("policies/me-strict.yaml").is_file());
    assert!(substrate.roots().repo.join("policies/agent-strict.yaml").is_file());
    assert!(substrate.roots().repo.join("policies/dreaming-strict.yaml").is_file());
}

#[tokio::test]
async fn policy_write_persists_atomically_and_audits_event() {
    let (_temp, substrate) = seeded_substrate().await;
    let updated = PROJECT_POLICY.replace("confidence_floor: 0.7", "confidence_floor: 0.72");

    let response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "policy-write",
            RequestPayload::PolicyWrite { raw_yaml: updated, file_name: Some("project-standard.yaml".to_owned()) },
        ),
    )
    .await;

    let ResponseResult::Success(ResponsePayload::PolicyWrite(result)) = response.result else {
        panic!("expected policy write success, got {:?}", response.result);
    };
    assert!(result.accepted);
    assert_eq!(result.file_name, "project-standard.yaml");
    assert!(read_project_policy(&substrate).contains("confidence_floor: 0.72"));
    assert!(!substrate.roots().repo.join("policies/project-standard.yaml.tmp").exists());

    let events = substrate.events().expect("events readable");
    assert!(events.iter().any(|event| {
        matches!(&event.kind, EventKind::PolicyChanged { file_name } if file_name == "project-standard.yaml")
    }));
}

#[tokio::test]
async fn policy_write_rejects_invalid_yaml_without_mutating_existing_file() {
    let (_temp, substrate) = seeded_substrate().await;
    let original = read_project_policy(&substrate);

    let response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "policy-write-invalid",
            RequestPayload::PolicyWrite {
                raw_yaml: "name: project-standard\nscope: project\nunexpected: nope\n".to_owned(),
                file_name: Some("project-standard.yaml".to_owned()),
            },
        ),
    )
    .await;

    let ResponseResult::Error(error) = response.result else {
        panic!("expected invalid policy error, got {:?}", response.result);
    };
    assert_eq!(error.code, "invalid_request");
    assert_eq!(read_project_policy(&substrate), original);
}

async fn seeded_substrate() -> (tempfile::TempDir, Substrate) {
    let (temp, substrate) = fresh_substrate("dev_policyeditor").await;
    seed_policy_dir(&substrate.roots().repo.join("policies"));
    (temp, substrate)
}

async fn fresh_substrate(device_id: &str) -> (tempfile::TempDir, Substrate) {
    let temp = tempfile::tempdir().expect("tempdir");
    let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
    let substrate =
        Substrate::init(roots, InitOptions { force_unsafe_durability: true, device_id: Some(device_id.to_owned()) })
            .await
            .expect("substrate init");
    (temp, substrate)
}

fn read_project_policy(substrate: &Substrate) -> String {
    std::fs::read_to_string(substrate.roots().repo.join("policies/project-standard.yaml"))
        .expect("project policy readable")
}

fn seed_policy_dir(path: &std::path::Path) {
    std::fs::create_dir_all(path).expect("policy dir");
    std::fs::write(path.join("me-strict.yaml"), PolicyFixture::me().to_yaml()).expect("me policy");
    std::fs::write(path.join("project-standard.yaml"), PROJECT_POLICY).expect("project policy");
    std::fs::write(path.join("agent-strict.yaml"), PolicyFixture::agent().to_yaml()).expect("agent policy");
    std::fs::write(path.join("dreaming-strict.yaml"), PolicyFixture::dreaming().to_yaml()).expect("dreaming policy");
}

struct PolicyFixture<'a> {
    name: &'a str,
    version: u32,
    scope: &'a str,
    confidence_floor: &'a str,
    tombstone: &'a str,
    contradiction: &'a str,
    gates: &'a [&'a str],
}

impl<'a> PolicyFixture<'a> {
    fn me() -> Self {
        Self {
            name: "me-strict",
            version: 1,
            scope: "me",
            confidence_floor: "0.85",
            tombstone: "refuse",
            contradiction: "quarantine",
            gates: &["low_confidence", "missing_grounding"],
        }
    }

    fn agent() -> Self {
        Self {
            name: "agent-strict",
            version: 3,
            scope: "agent",
            confidence_floor: "0.82",
            tombstone: "refuse",
            contradiction: "quarantine",
            gates: &["low_confidence", "missing_grounding"],
        }
    }

    fn dreaming() -> Self {
        Self {
            name: "dreaming-strict",
            version: 1,
            scope: "dreaming",
            confidence_floor: "0.95",
            tombstone: "refuse",
            contradiction: "quarantine",
            gates: &["low_confidence", "missing_grounding", "dream_source"],
        }
    }

    fn to_yaml(&self) -> String {
        let review_gates = self.gates.iter().map(|gate| format!("  - {gate}\n")).collect::<String>();
        format!(
            "name: {}\nversion: {}\nscope: {}\nconfidence_floor: {}\nrequires_grounding: true\ntombstone_enforcement: {}\ncontradiction_policy: {}\nreview_gates:\n{}",
            self.name,
            self.version,
            self.scope,
            self.confidence_floor,
            self.tombstone,
            self.contradiction,
            review_gates
        )
    }
}
