use memory_privacy::{PrivacyDecision, PrivacyLabel, PrivacySpan, PrivacyTier};
use memory_substrate::{ClassificationOutcome, Sensitivity};

#[test]
fn privacy_tier_maps_to_stream_a_classification() {
    assert_eq!(PrivacyTier::Public.classification(), ClassificationOutcome::Trusted);
    assert_eq!(PrivacyTier::Internal.classification(), ClassificationOutcome::Trusted);
    assert_eq!(PrivacyTier::Confidential.classification(), ClassificationOutcome::RequiresEncryption);
    assert_eq!(PrivacyTier::Personal.classification(), ClassificationOutcome::RequiresEncryption);
    assert_eq!(PrivacyTier::Secret.classification(), ClassificationOutcome::Secret);
}

#[test]
fn secret_is_not_a_persisted_frontmatter_sensitivity() {
    assert_eq!(PrivacyTier::Public.persisted_sensitivity(), Some(Sensitivity::Public));
    assert_eq!(PrivacyTier::Internal.persisted_sensitivity(), Some(Sensitivity::Internal));
    assert_eq!(PrivacyTier::Confidential.persisted_sensitivity(), Some(Sensitivity::Confidential));
    assert_eq!(PrivacyTier::Personal.persisted_sensitivity(), Some(Sensitivity::Personal));
    assert_eq!(PrivacyTier::Secret.persisted_sensitivity(), None);
}

#[test]
fn privacy_decision_serializes_stable_snake_case_labels() {
    let decision = PrivacyDecision::new(
        PrivacyTier::Personal,
        vec![PrivacySpan::new(PrivacyLabel::PrivateEmail, 4, 20, 0.95)],
        "fixture",
    );

    let json = serde_json::to_value(decision).expect("serialize decision");

    assert_eq!(json["tier"], "personal");
    assert_eq!(json["spans"][0]["label"], "private_email");
    assert_eq!(json["scan"]["labels"][0], "private_email");
}
