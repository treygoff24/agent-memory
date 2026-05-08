use memory_privacy::{FileKeyProvider, PrivacyEncryptor};

#[test]
fn envelope_metadata_round_trips_for_reveal_decryption() {
    let temp = tempfile::tempdir().expect("tempdir");
    let provider = FileKeyProvider::runtime_default(temp.path());
    provider.onboard_local_file().expect("onboard key");
    let encryptor = PrivacyEncryptor::new(provider);

    let payload = encryptor.encrypt("private plaintext body").expect("encrypt");
    assert_eq!(payload.envelope["scheme"].as_str(), Some("age-x25519"));
    assert!(payload.envelope["recipient"].as_str().is_some_and(|recipient| !recipient.is_empty()));

    let decrypted = encryptor.decrypt(&payload).expect("decrypt with envelope metadata");
    assert_eq!(decrypted, "private plaintext body");
}
