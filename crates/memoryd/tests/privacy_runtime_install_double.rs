use memory_privacy::{
    install_runtime_enforcement, DeterministicPrivacyClassifier, PrivacyClassifier, PrivacyEnforcement,
    PrivacyNamespace, PrivacyStorageAction,
};

#[test]
fn double_install_is_rejected_and_first_config_wins() {
    let first = PrivacyEnforcement { classifier: false, encryption: false, masking: false };
    let second = PrivacyEnforcement { classifier: true, encryption: true, masking: true };

    install_runtime_enforcement(first).expect("first install succeeds");
    assert!(install_runtime_enforcement(first).is_err());
    assert!(install_runtime_enforcement(second).is_err());

    let decision = DeterministicPrivacyClassifier::new()
        .classify("Email trey@example.com before launch.", PrivacyNamespace::Project, None)
        .expect("classify");

    assert_eq!(decision.storage_action, PrivacyStorageAction::Plaintext);
}
