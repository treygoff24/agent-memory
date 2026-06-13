use memory_privacy::{FileKeyProvider, KeyProvider, PrivacyEncryptor};

#[test]
fn encryption_round_trips_without_plaintext_ciphertext() {
    let temp = tempfile::tempdir().expect("tempdir");
    let provider = FileKeyProvider::runtime_default(temp.path());
    provider.onboard_local_file().expect("onboard key");
    let encryptor = PrivacyEncryptor::new(provider);

    let payload = encryptor.encrypt("private plaintext body").expect("encrypt");

    assert!(!String::from_utf8_lossy(&payload.ciphertext).contains("private plaintext body"));
    assert_eq!(encryptor.decrypt(&payload).expect("decrypt"), "private plaintext body");
}

#[test]
fn identical_plaintext_encrypts_to_different_ciphertext() {
    let temp = tempfile::tempdir().expect("tempdir");
    let provider = FileKeyProvider::runtime_default(temp.path());
    provider.onboard_local_file().expect("onboard key");
    let encryptor = PrivacyEncryptor::new(provider);

    let first = encryptor.encrypt("private plaintext body").expect("first");
    let second = encryptor.encrypt("private plaintext body").expect("second");

    assert_ne!(first.ciphertext, second.ciphertext);
}

#[test]
fn rotated_file_provider_keeps_old_ciphertext_revealable_and_uses_new_recipient() {
    let temp = tempfile::tempdir().expect("tempdir");
    let provider = FileKeyProvider::runtime_default(temp.path());
    provider.onboard_local_file().expect("onboard key");
    let encryptor = PrivacyEncryptor::new(provider.clone());
    let before = encryptor.encrypt("private body before rotation").expect("encrypt before");
    let before_recipient = before.envelope["recipient"].as_str().expect("recipient before").to_owned();

    let rotation = provider.rotate_local_file().expect("rotate key");
    let after = encryptor.encrypt("private body after rotation").expect("encrypt after");

    assert_ne!(after.envelope["recipient"].as_str(), Some(before_recipient.as_str()));
    assert_eq!(rotation.previous_recipient.as_deref(), Some(before_recipient.as_str()));
    assert!(rotation.archived_key_path.as_ref().is_some_and(|path| path.is_file()));
    assert!(provider.active_manifest_path().is_file());
    assert_eq!(encryptor.decrypt(&before).expect("decrypt before"), "private body before rotation");
    assert_eq!(encryptor.decrypt(&after).expect("decrypt after"), "private body after rotation");
}

#[test]
fn missing_key_fails_closed() {
    let temp = tempfile::tempdir().expect("tempdir");
    let provider = FileKeyProvider::runtime_default(temp.path());
    let error = provider.load_key().expect_err("missing key");

    // A genuinely-absent key file is surfaced as the typed `KeyMissing` variant
    // (sourced from io::ErrorKind::NotFound), not by substring-matching a
    // locale/platform-dependent OS error string.
    assert!(matches!(error, memory_privacy::PrivacyError::KeyMissing(_)), "expected KeyMissing, got {error:?}");
}

#[test]
fn rotate_from_absent_prior_key_starts_fresh() {
    // Rotation must treat a missing prior key as "no previous key" rather than a
    // hard error. This branch is gated on the typed KeyMissing classification;
    // the regression guards against locale-dependent message matching that could
    // misclassify the absent key and flip fail-open/fail-closed behavior.
    let temp = tempfile::tempdir().expect("tempdir");
    let provider = FileKeyProvider::runtime_default(temp.path());

    let rotation = provider.rotate_local_file().expect("rotate with no prior key");

    assert!(rotation.previous_recipient.is_none());
    assert!(rotation.archived_key_path.is_none());
    assert!(rotation.active_key_path.is_file());
    assert!(provider.load_key().is_ok());
}

#[test]
fn decrypt_rejects_unsupported_envelope_scheme_before_key_use() {
    let temp = tempfile::tempdir().expect("tempdir");
    let provider = FileKeyProvider::runtime_default(temp.path());
    provider.onboard_local_file().expect("onboard key");
    let encryptor = PrivacyEncryptor::new(provider);
    let mut payload = encryptor.encrypt("private plaintext body").expect("encrypt");
    payload.envelope["scheme"] = serde_json::json!("unsupported");

    let error = encryptor.decrypt(&payload).expect_err("unsupported envelope is rejected");

    assert!(error.to_string().contains("unsupported encryption envelope scheme"));
}

#[cfg(unix)]
#[test]
fn local_key_file_is_private() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().expect("tempdir");
    let provider = FileKeyProvider::runtime_default(temp.path());
    provider.onboard_local_file().expect("onboard key");

    let metadata = std::fs::metadata(provider.path()).expect("key metadata");
    assert_eq!(metadata.permissions().mode() & 0o077, 0);
    let parent = provider.path().parent().expect("key parent");
    let metadata = std::fs::metadata(parent).expect("parent metadata");
    assert_eq!(metadata.permissions().mode() & 0o077, 0);
}
