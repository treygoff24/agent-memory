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
fn missing_key_fails_closed() {
    let temp = tempfile::tempdir().expect("tempdir");
    let provider = FileKeyProvider::runtime_default(temp.path());
    let error = provider.load_key().expect_err("missing key");

    assert!(error.to_string().contains("privacy key unavailable"));
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
