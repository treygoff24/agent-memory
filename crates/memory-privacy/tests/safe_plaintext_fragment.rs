use std::sync::Mutex;

use memory_privacy::{
    safe_plaintext_fragment, DeterministicPrivacyClassifier, PrivacyClassifier, PrivacyDecision, PrivacyLabel,
    PrivacyNamespace, PrivacySpan, PrivacyStorageAction, PrivacyTier, SafeFragmentDecision,
};
use memory_privacy::{CallerSensitivity, PrivacyResult};

#[test]
fn allows_benign_url_and_date_only_fragments() {
    let classifier = DeterministicPrivacyClassifier::new();

    assert_eq!(safe_plaintext_fragment(&classifier, "Release branch main is ready."), SafeFragmentDecision::Allow);
    assert_eq!(
        safe_plaintext_fragment(&classifier, "See https://docs.example.com/runbook on 2026-04-30."),
        SafeFragmentDecision::Allow
    );
}

#[test]
fn omits_secret_fragments_as_encrypted_body_hidden() {
    let classifier = DeterministicPrivacyClassifier::new();

    assert_eq!(
        safe_plaintext_fragment(&classifier, &format!("AWS key {} must never appear.", fake_aws_key())),
        SafeFragmentDecision::OmitEncryptedBodyHidden
    );
    assert_eq!(
        safe_plaintext_fragment(
            &classifier,
            "JWT eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.sflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c"
        ),
        SafeFragmentDecision::OmitEncryptedBodyHidden
    );
    assert_eq!(
        safe_plaintext_fragment(&classifier, "-----BEGIN OPENSSH PRIVATE KEY-----"),
        SafeFragmentDecision::OmitEncryptedBodyHidden
    );
}

#[test]
fn omits_private_fragments_pending_review() {
    let classifier = DeterministicPrivacyClassifier::new();

    for fragment in [
        "Email trey@example.com before launch.",
        "Call 202-555-0198 before launch.",
        "Meet at 123 Main St before launch.",
    ] {
        assert_eq!(
            safe_plaintext_fragment(&classifier, fragment),
            SafeFragmentDecision::OmitReviewPending,
            "fragment should be review-pending: {fragment}"
        );
    }
}

#[test]
fn strictest_result_wins_across_mixed_labels() {
    let classifier = DeterministicPrivacyClassifier::new();

    assert_eq!(
        safe_plaintext_fragment(
            &classifier,
            &format!("Email trey@example.com and AWS key {} must not appear.", fake_aws_key())
        ),
        SafeFragmentDecision::OmitEncryptedBodyHidden
    );
}

fn fake_aws_key() -> String {
    let suffix = (0..16).map(|index| char::from(b'A' + (index % 10) as u8)).collect::<String>();
    ["AK", "IA", &suffix].concat()
}

#[test]
fn classifies_under_me_namespace_and_is_deterministic() {
    let classifier = RecordingClassifier::new(PrivacyDecision::new(
        PrivacyTier::Internal,
        PrivacyStorageAction::Plaintext,
        Vec::new(),
        "recording",
    ));

    assert_eq!(safe_plaintext_fragment(&classifier, "stable fragment"), SafeFragmentDecision::Allow);
    assert_eq!(safe_plaintext_fragment(&classifier, "stable fragment"), SafeFragmentDecision::Allow);
    assert_eq!(classifier.namespaces(), vec![PrivacyNamespace::Me, PrivacyNamespace::Me]);
}

struct RecordingClassifier {
    decision: PrivacyDecision,
    namespaces: Mutex<Vec<PrivacyNamespace>>,
}

impl RecordingClassifier {
    fn new(decision: PrivacyDecision) -> Self {
        Self { decision, namespaces: Mutex::new(Vec::new()) }
    }

    fn namespaces(&self) -> Vec<PrivacyNamespace> {
        self.namespaces.lock().expect("namespaces lock").clone()
    }
}

impl PrivacyClassifier for RecordingClassifier {
    fn classify(
        &self,
        _text: &str,
        namespace: PrivacyNamespace,
        caller: Option<CallerSensitivity>,
    ) -> PrivacyResult<PrivacyDecision> {
        assert_eq!(caller, None);
        self.namespaces.lock().expect("namespaces lock").push(namespace);
        Ok(self.decision.clone())
    }
}

#[test]
fn explicit_encrypt_at_rest_decision_with_private_span_omits_pending_review() {
    let classifier = RecordingClassifier::new(PrivacyDecision::new(
        PrivacyTier::Personal,
        PrivacyStorageAction::EncryptAtRest,
        vec![PrivacySpan::new(PrivacyLabel::PrivateEmail, 0, 6, 0.95)],
        "recording",
    ));

    assert_eq!(
        safe_plaintext_fragment(&classifier, "storage action requires encryption"),
        SafeFragmentDecision::OmitReviewPending
    );
}

#[test]
fn account_and_person_labels_omit_pending_review() {
    for label in [PrivacyLabel::AccountNumber, PrivacyLabel::PrivatePerson] {
        let classifier = RecordingClassifier::new(PrivacyDecision::new(
            PrivacyTier::Internal,
            PrivacyStorageAction::Plaintext,
            vec![PrivacySpan::new(label, 0, 6, 0.90)],
            "recording",
        ));

        assert_eq!(safe_plaintext_fragment(&classifier, "private label"), SafeFragmentDecision::OmitReviewPending);
    }
}

#[test]
fn explicit_secret_label_omits_as_hidden_even_without_refuse_action() {
    let classifier = RecordingClassifier::new(PrivacyDecision::new(
        PrivacyTier::Internal,
        PrivacyStorageAction::Plaintext,
        vec![PrivacySpan::new(PrivacyLabel::Secret, 0, 6, 0.99)],
        "recording",
    ));

    assert_eq!(safe_plaintext_fragment(&classifier, "secret"), SafeFragmentDecision::OmitEncryptedBodyHidden);
}
