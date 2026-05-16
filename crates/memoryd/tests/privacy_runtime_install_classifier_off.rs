use memory_privacy::{DeterministicPrivacyClassifier, PrivacyClassifier, PrivacyNamespace, PrivacyStorageAction};
use memory_substrate::config::PrivacyEnforcement;
use memoryd::runtime_privacy::{install_privacy_runtime_from_roots, RuntimePrivacyInstallStatus};

#[test]
fn installed_classifier_off_config_controls_fresh_classifier() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let runtime = temp.path().join("runtime");
    std::fs::create_dir_all(&repo).expect("repo dir");
    std::fs::create_dir_all(&runtime).expect("runtime dir");
    std::fs::write(
        runtime.join("local-device.yaml"),
        r#"schema_version: 1
device:
  id: dev_privacy_runtime
privacy:
  classifier: false
  encryption: false
  masking: false
"#,
    )
    .expect("local-device config");

    let install = install_privacy_runtime_from_roots(&repo, &runtime).expect("install runtime enforcement from config");
    assert_eq!(
        install,
        RuntimePrivacyInstallStatus::Installed(PrivacyEnforcement {
            classifier: false,
            encryption: false,
            masking: false
        })
    );

    let decision = DeterministicPrivacyClassifier::new()
        .classify("Email trey@example.com before launch.", PrivacyNamespace::Project, None)
        .expect("classify");

    assert_eq!(decision.storage_action, PrivacyStorageAction::Plaintext);
    assert!(decision.spans.is_empty());
}
