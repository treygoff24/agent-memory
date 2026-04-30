use memory_privacy::{MaskingSession, MaskingSessionId, PrivacyLabel, PrivacySpan};

#[test]
fn masking_tokens_are_stable_within_session() {
    let mut session = MaskingSession::new(MaskingSessionId::new("sess-a"));
    let text = "Trey emailed Trey.";
    let masked = session
        .mask(
            text,
            &[
                PrivacySpan::new(PrivacyLabel::PrivatePerson, 0, 4, 0.9),
                PrivacySpan::new(PrivacyLabel::PrivatePerson, 13, 17, 0.9),
            ],
        )
        .expect("mask");

    assert_eq!(masked, "Person_A emailed Person_A.");
    assert_eq!(session.restore(&MaskingSessionId::new("sess-a"), &masked).expect("restore"), text);
}

#[test]
fn restore_with_wrong_session_fails() {
    let mut session = MaskingSession::new(MaskingSessionId::new("sess-a"));
    let masked =
        session.mask("trey@example.com", &[PrivacySpan::new(PrivacyLabel::PrivateEmail, 0, 16, 0.9)]).expect("mask");

    assert!(session.restore(&MaskingSessionId::new("sess-b"), &masked).is_err());
}

#[test]
fn restore_does_not_rescan_replacement_text_for_later_tokens() {
    let mut session = MaskingSession::new(MaskingSessionId::new("sess-a"));
    let text = "Person_B emailed Trey.";
    let masked = session
        .mask(
            text,
            &[
                PrivacySpan::new(PrivacyLabel::PrivatePerson, 0, 8, 0.9),
                PrivacySpan::new(PrivacyLabel::PrivatePerson, 17, 21, 0.9),
            ],
        )
        .expect("mask");

    assert_eq!(masked, "Person_A emailed Person_B.");
    assert_eq!(session.restore(&MaskingSessionId::new("sess-a"), &masked).expect("restore"), text);
}
