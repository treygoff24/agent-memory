use memory_privacy::{
    DeterministicPrivacyClassifier, DisabledPrivacyFilter, FixturePrivacyFilter, PrivacyClassifier, PrivacyError,
    PrivacyLabel, PrivacyNamespace, PrivacySpan, PrivacyStorageAction, PrivacyTier,
};

#[test]
fn disabled_privacy_filter_returns_explicit_unavailable_error() {
    let provider = DisabledPrivacyFilter;
    let error = memory_privacy::PrivacyFilterProvider::detect(&provider, "hello").expect_err("disabled");

    let PrivacyError::PrivacyFilterUnavailable(message) = error else {
        panic!("disabled provider must return PrivacyFilterUnavailable, got {error:?}");
    };
    assert_eq!(message, "privacy filter is disabled");
}

#[test]
fn fixture_provider_merges_with_layer1_spans() {
    let classifier =
        DeterministicPrivacyClassifier::with_provider(Box::new(FixturePrivacyFilter::new(vec![PrivacySpan::new(
            PrivacyLabel::PrivatePerson,
            0,
            4,
            0.90,
        )])));

    let decision =
        classifier.classify("Trey wrote trey@example.com", PrivacyNamespace::Project, None).expect("classify");

    assert_eq!(decision.tier, PrivacyTier::Internal);
    assert_eq!(decision.storage_action, PrivacyStorageAction::EncryptAtRest);
    assert!(decision.spans.contains(&PrivacySpan::new(PrivacyLabel::PrivatePerson, 0, 4, 0.90)));
    assert!(decision.spans.iter().any(|span| span.label == PrivacyLabel::PrivateEmail));
    assert_eq!(decision.scan.model, "memory-privacy/layer1@v0.1+openai/privacy-filter@fixture");
}
