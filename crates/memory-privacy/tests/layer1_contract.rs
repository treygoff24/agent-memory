use memory_privacy::{
    CallerSensitivity, DeterministicPrivacyClassifier, PrivacyClassifier, PrivacyNamespace, PrivacyStorageAction,
    PrivacyTier,
};

#[test]
fn layer1_secret_token_refuses_storage() {
    let classifier = DeterministicPrivacyClassifier::new();

    let decision = classifier
        .classify(&format!("AWS key {} must never persist", fake_aws_key()), PrivacyNamespace::Project, None)
        .expect("classify");

    assert_eq!(decision.tier, PrivacyTier::Internal);
    assert_eq!(decision.storage_action, PrivacyStorageAction::Refuse);
}

fn fake_aws_key() -> String {
    let suffix = (0..16).map(|index| char::from(b'A' + (index % 10) as u8)).collect::<String>();
    ["AK", "IA", &suffix].concat()
}

#[test]
fn layer1_personal_identifier_requires_encryption() {
    let classifier = DeterministicPrivacyClassifier::new();

    let decision = classifier
        .classify("Email trey@example.com before launch.", PrivacyNamespace::Project, None)
        .expect("classify");

    assert_eq!(decision.tier, PrivacyTier::Internal);
    assert_eq!(decision.storage_action, PrivacyStorageAction::EncryptAtRest);
}

#[test]
fn layer1_phone_requires_encryption_without_personal_tier_elevation() {
    let classifier = DeterministicPrivacyClassifier::new();

    let decision = classifier
        .classify("Rep. Mills Chief of Staff cell is 202-555-0198.", PrivacyNamespace::Project, None)
        .expect("classify");

    assert_eq!(decision.tier, PrivacyTier::Internal);
    assert_eq!(decision.storage_action, PrivacyStorageAction::EncryptAtRest);
}

#[test]
fn layer1_url_and_iso_date_stay_plaintext_project_memory() {
    let classifier = DeterministicPrivacyClassifier::new();

    let decision = classifier
        .classify("See https://docs.example.com/foo for the 2026-04-28 release notes.", PrivacyNamespace::Project, None)
        .expect("classify");

    assert_eq!(decision.tier, PrivacyTier::Internal);
    assert_eq!(decision.storage_action, PrivacyStorageAction::Plaintext);
}

#[test]
fn layer1_high_risk_identity_numbers_are_refused() {
    let classifier = DeterministicPrivacyClassifier::new();

    let ssn = classifier
        .classify("SSN 123-45-6789 must not persist.", PrivacyNamespace::Project, None)
        .expect("classify ssn");
    let card = classifier
        .classify("Card 4111 1111 1111 1111 must not persist.", PrivacyNamespace::Project, None)
        .expect("classify card");

    assert_eq!(ssn.storage_action, PrivacyStorageAction::Refuse);
    assert_eq!(card.storage_action, PrivacyStorageAction::Refuse);
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

    let lowered = classifier
        .classify("Email trey@example.com before launch.", PrivacyNamespace::Project, Some(CallerSensitivity::Public))
        .expect("classify");

    assert_eq!(lowered.tier, PrivacyTier::Internal);
    assert_eq!(lowered.storage_action, PrivacyStorageAction::EncryptAtRest);

    let elevated = classifier
        .classify("The release branch is main.", PrivacyNamespace::Project, Some(CallerSensitivity::Personal))
        .expect("classify caller-elevated plain project text");

    assert_eq!(elevated.tier, PrivacyTier::Personal);
    assert_eq!(elevated.storage_action, PrivacyStorageAction::EncryptAtRest);
}
