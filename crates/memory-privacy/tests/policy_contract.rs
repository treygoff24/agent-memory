use memory_privacy::{
    CallerSensitivity, PrivacyLabel, PrivacyNamespace, PrivacyPolicy, PrivacySpan, PrivacyStorageAction, PrivacyTier,
};

#[test]
fn namespace_defaults_are_stable() {
    assert_eq!(PrivacyPolicy::default_tier(PrivacyNamespace::Me), PrivacyTier::Personal);
    assert_eq!(PrivacyPolicy::default_tier(PrivacyNamespace::Project), PrivacyTier::Internal);
    assert_eq!(PrivacyPolicy::default_tier(PrivacyNamespace::Agent), PrivacyTier::Internal);
}

#[test]
fn secret_span_dominates_every_other_signal() {
    let decision = PrivacyPolicy::resolve(
        PrivacyNamespace::Project,
        Some(CallerSensitivity::Public),
        &[PrivacySpan::new(PrivacyLabel::Secret, 0, 10, 0.99)],
    )
    .expect("resolve");

    assert_eq!(decision.tier, PrivacyTier::Internal);
    assert_eq!(decision.storage_action, PrivacyStorageAction::Refuse);
}

#[test]
fn pii_spans_encrypt_without_tier_elevation() {
    let decision = PrivacyPolicy::resolve(
        PrivacyNamespace::Project,
        None,
        &[PrivacySpan::new(PrivacyLabel::PrivatePhone, 0, 12, 0.85)],
    )
    .expect("resolve");

    assert_eq!(decision.tier, PrivacyTier::Internal);
    assert_eq!(decision.storage_action, PrivacyStorageAction::EncryptAtRest);
}

#[test]
fn caller_personal_still_requires_encryption_without_spans() {
    let decision =
        PrivacyPolicy::resolve(PrivacyNamespace::Project, Some(CallerSensitivity::Personal), &[]).expect("resolve");

    assert_eq!(decision.tier, PrivacyTier::Personal);
    assert_eq!(decision.storage_action, PrivacyStorageAction::EncryptAtRest);
}
