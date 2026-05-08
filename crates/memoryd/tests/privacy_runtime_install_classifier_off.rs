use memory_privacy::{
    install_runtime_enforcement, DeterministicPrivacyClassifier, PrivacyClassifier, PrivacyEnforcement,
    PrivacyNamespace, PrivacyStorageAction,
};

#[test]
fn installed_classifier_off_config_controls_fresh_classifier() {
    install_runtime_enforcement(PrivacyEnforcement { classifier: false, encryption: false, masking: false })
        .expect("install runtime enforcement");

    let decision = DeterministicPrivacyClassifier::new()
        .classify("Email trey@example.com before launch.", PrivacyNamespace::Project, None)
        .expect("classify");

    assert_eq!(decision.storage_action, PrivacyStorageAction::Plaintext);
    assert!(decision.spans.is_empty());
}
