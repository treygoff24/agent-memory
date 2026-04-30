use memory_privacy::{
    CallerSensitivity, DeterministicPrivacyClassifier, PrivacyClassifier, PrivacyNamespace, PrivacyTier,
};

#[test]
fn layer1_secret_token_refuses_storage() {
    let classifier = DeterministicPrivacyClassifier::new();

    let decision = classifier
        .classify("AWS key AKIA1234567890ABCDEF must never persist", PrivacyNamespace::Project, None)
        .expect("classify");

    assert_eq!(decision.tier, PrivacyTier::Secret);
}

#[test]
fn layer1_personal_identifier_requires_encryption() {
    let classifier = DeterministicPrivacyClassifier::new();

    let decision = classifier
        .classify("Email trey@example.com before launch.", PrivacyNamespace::Project, None)
        .expect("classify");

    assert_eq!(decision.tier, PrivacyTier::Personal);
    assert!(decision.tier.requires_encryption());
}

#[test]
fn project_text_without_hits_defaults_internal() {
    let classifier = DeterministicPrivacyClassifier::new();

    let decision =
        classifier.classify("The release branch is main.", PrivacyNamespace::Project, None).expect("classify");

    assert_eq!(decision.tier, PrivacyTier::Internal);
}

#[test]
fn caller_metadata_can_raise_but_not_lower() {
    let classifier = DeterministicPrivacyClassifier::new();

    let decision = classifier
        .classify("Email trey@example.com before launch.", PrivacyNamespace::Project, Some(CallerSensitivity::Public))
        .expect("classify");

    assert_eq!(decision.tier, PrivacyTier::Personal);
}
