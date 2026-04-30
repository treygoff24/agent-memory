use memory_privacy::{
    DeterministicPrivacyClassifier, DisabledPrivacyFilter, FixturePrivacyFilter, PrivacyClassifier, PrivacyLabel,
    PrivacyNamespace, PrivacySpan, PrivacyTier,
};

#[test]
fn disabled_privacy_filter_returns_explicit_unavailable_error() {
    let provider = DisabledPrivacyFilter;
    let error = memory_privacy::PrivacyFilterProvider::detect(&provider, "hello").expect_err("disabled");

    assert!(error.to_string().contains("privacy filter unavailable"));
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

    assert_eq!(decision.tier, PrivacyTier::Personal);
    assert!(decision.spans.iter().any(|span| span.label == PrivacyLabel::PrivatePerson));
    assert!(decision.spans.iter().any(|span| span.label == PrivacyLabel::PrivateEmail));
}
