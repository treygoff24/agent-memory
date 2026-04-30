use memory_privacy::{CallerSensitivity, PrivacyLabel, PrivacyNamespace, PrivacyPolicy, PrivacySpan, PrivacyTier};

#[test]
fn namespace_defaults_are_stable() {
    assert_eq!(PrivacyPolicy::default_tier(PrivacyNamespace::Me), PrivacyTier::Personal);
    assert_eq!(PrivacyPolicy::default_tier(PrivacyNamespace::Project), PrivacyTier::Internal);
    assert_eq!(PrivacyPolicy::default_tier(PrivacyNamespace::Agent), PrivacyTier::Internal);
}

#[test]
fn secret_span_dominates_every_other_signal() {
    let tier = PrivacyPolicy::resolve_tier(
        PrivacyNamespace::Project,
        Some(CallerSensitivity::Public),
        &[PrivacySpan::new(PrivacyLabel::Secret, 0, 10, 0.99)],
    )
    .expect("resolve");

    assert_eq!(tier, PrivacyTier::Secret);
}
