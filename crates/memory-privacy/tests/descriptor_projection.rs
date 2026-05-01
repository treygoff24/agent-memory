use memory_privacy::{
    safe_descriptor_projection, safe_plaintext_fragment, DeterministicPrivacyClassifier, SafeFragmentDecision,
};

#[test]
fn descriptor_projection_removes_private_spans_but_keeps_safe_context() {
    let classifier = DeterministicPrivacyClassifier::new();
    let text = "Follow up with reviewer@example.com about auth flow integration.";

    let projection = safe_descriptor_projection(
        &classifier,
        text,
        "encrypted observation substrate fragment",
        &["observation".to_string()],
    );

    assert!(projection.summary_safe.contains("auth flow integration"), "{projection:?}");
    assert!(!projection.summary_safe.contains("reviewer@example.com"), "{projection:?}");
    assert_eq!(
        safe_plaintext_fragment(&classifier, &projection.summary_safe),
        SafeFragmentDecision::Allow,
        "{projection:?}"
    );
    assert!(projection.tag_safe.iter().any(|tag| tag == "auth"), "{projection:?}");
}

#[test]
fn descriptor_projection_falls_back_when_only_private_text_remains() {
    let classifier = DeterministicPrivacyClassifier::new();

    let projection = safe_descriptor_projection(
        &classifier,
        "reviewer@example.com",
        "encrypted observation substrate fragment",
        &["observation".to_string()],
    );

    assert_eq!(projection.summary_safe, "encrypted observation substrate fragment");
    assert_eq!(projection.tag_safe, vec!["observation".to_string()]);
}
