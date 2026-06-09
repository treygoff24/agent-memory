use memory_privacy::{
    DeterministicPrivacyClassifier, PrivacyClassifier, PrivacyEnforcement, PrivacyNamespace, PrivacyStorageAction,
    PrivacyTier,
};

fn enforcement(classifier: bool, encryption: bool, masking: bool) -> PrivacyEnforcement {
    PrivacyEnforcement { classifier, encryption, masking }
}

#[test]
fn classifier_off_lets_email_through_plaintext() {
    let classifier = DeterministicPrivacyClassifier::with_enforcement(enforcement(false, false, false));

    let decision = classifier
        .classify("Email trey@example.com before launch.", PrivacyNamespace::Project, None)
        .expect("classify");

    assert_eq!(decision.tier, PrivacyTier::Internal);
    assert_eq!(decision.storage_action, PrivacyStorageAction::Plaintext);
    assert!(decision.spans.is_empty());
}

#[test]
fn classifier_off_lets_non_secret_digest_like_numbers_through_plaintext() {
    let classifier = DeterministicPrivacyClassifier::with_enforcement(enforcement(false, false, false));

    let decision = classifier
        .classify("Docker layer sha256:0123456789abcdef0123456789abcdef is fine.", PrivacyNamespace::Project, None)
        .expect("classify");

    assert_eq!(decision.storage_action, PrivacyStorageAction::Plaintext);
    assert!(decision.spans.is_empty());
}

#[test]
fn classifier_off_still_refuses_ssn() {
    let classifier = DeterministicPrivacyClassifier::with_enforcement(enforcement(false, false, false));

    let decision =
        classifier.classify("SSN 123-45-6789 must not persist.", PrivacyNamespace::Project, None).expect("classify");

    assert_eq!(decision.storage_action, PrivacyStorageAction::Refuse);
}

#[test]
fn classifier_off_still_refuses_luhn_card_number() {
    let classifier = DeterministicPrivacyClassifier::with_enforcement(enforcement(false, false, false));

    let decision = classifier
        .classify("Card 4111 1111 1111 1111 must not persist.", PrivacyNamespace::Project, None)
        .expect("classify");

    assert_eq!(decision.storage_action, PrivacyStorageAction::Refuse);
}

#[test]
fn classifier_on_still_refuses_ssn() {
    let classifier = DeterministicPrivacyClassifier::with_enforcement(enforcement(true, true, true));

    let decision =
        classifier.classify("SSN 123-45-6789 must not persist.", PrivacyNamespace::Project, None).expect("classify");

    assert_eq!(decision.storage_action, PrivacyStorageAction::Refuse);
}

#[test]
fn classifier_on_email_gets_encrypt_at_rest() {
    let classifier = DeterministicPrivacyClassifier::with_enforcement(enforcement(true, true, true));

    let decision = classifier
        .classify("Email trey@example.com before launch.", PrivacyNamespace::Project, None)
        .expect("classify");

    assert_eq!(decision.storage_action, PrivacyStorageAction::EncryptAtRest);
}

#[test]
fn encryption_off_routes_encrypt_at_rest_labels_to_plaintext() {
    let classifier = DeterministicPrivacyClassifier::with_enforcement(enforcement(true, false, true));

    let decision = classifier
        .classify("Email trey@example.com before launch.", PrivacyNamespace::Project, None)
        .expect("classify");

    assert!(decision.spans.iter().any(|span| span.label.storage_action().requires_encryption()));
    assert_eq!(decision.storage_action, PrivacyStorageAction::Plaintext);
}

#[test]
fn masking_off_is_preserved_in_runtime_enforcement_for_callers() {
    let classifier = DeterministicPrivacyClassifier::with_enforcement(enforcement(true, true, false));

    let decision = classifier
        .classify("Email trey@example.com before launch.", PrivacyNamespace::Project, None)
        .expect("classify");

    assert_eq!(decision.storage_action, PrivacyStorageAction::EncryptAtRest);
    assert!(!enforcement(true, true, false).masking);
}

#[test]
fn encryption_off_downgrade_returns_plaintext_and_sets_flag() {
    // When encryption enforcement is disabled, spans that would require
    // EncryptAtRest must be routed to Plaintext (behavior preserved) and
    // the `downgraded_by_enforcement` audit flag must be set so callers
    // can surface the misconfiguration without re-classifying.
    let classifier = DeterministicPrivacyClassifier::with_enforcement(enforcement(true, false, true));

    let decision = classifier
        .classify("Email trey@example.com before launch.", PrivacyNamespace::Project, None)
        .expect("classify");

    assert_eq!(decision.storage_action, PrivacyStorageAction::Plaintext, "downgraded action must be Plaintext");
    assert!(
        decision.downgraded_by_enforcement,
        "downgraded_by_enforcement flag must be set when encryption is suppressed"
    );
}

#[test]
fn encryption_on_does_not_set_downgrade_flag() {
    // When encryption enforcement is active the flag must stay false even
    // for content that requires encryption.
    let classifier = DeterministicPrivacyClassifier::with_enforcement(enforcement(true, true, true));

    let decision = classifier
        .classify("Email trey@example.com before launch.", PrivacyNamespace::Project, None)
        .expect("classify");

    assert_eq!(decision.storage_action, PrivacyStorageAction::EncryptAtRest);
    assert!(!decision.downgraded_by_enforcement, "downgraded_by_enforcement must be false when encryption is enforced");
}
