use memory_privacy::{
    install_runtime_enforcement, DeterministicPrivacyClassifier, PrivacyClassifier, PrivacyEnforcement,
    PrivacyNamespace, PrivacyStorageAction,
};

#[test]
fn installed_full_config_controls_fresh_classifier() {
    install_runtime_enforcement(PrivacyEnforcement { classifier: true, encryption: true, masking: true })
        .expect("install runtime enforcement");

    let email = DeterministicPrivacyClassifier::new()
        .classify("Email trey@example.com before launch.", PrivacyNamespace::Project, None)
        .expect("classify email");
    let ssn = DeterministicPrivacyClassifier::new()
        .classify("SSN 123-45-6789 must not persist.", PrivacyNamespace::Project, None)
        .expect("classify ssn");

    assert_eq!(email.storage_action, PrivacyStorageAction::EncryptAtRest);
    assert_eq!(ssn.storage_action, PrivacyStorageAction::Refuse);
}
