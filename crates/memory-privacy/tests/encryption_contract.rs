use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use memory_privacy::{
    EncryptedPayload, FileKeyProvider, KeyMaterial, KeyProvider, PrivacyEncryptor, PrivacyError, PrivacyResult,
};

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
    let load_calls = Arc::new(AtomicUsize::new(0));
    let encryptor = PrivacyEncryptor::new(LoadCountingKeyProvider { load_calls: Arc::clone(&load_calls) });
    let payload = EncryptedPayload {
        ciphertext: b"not reached".to_vec(),
        envelope: serde_json::json!({ "scheme": "unsupported" }),
    };

    let error = encryptor.decrypt(&payload).expect_err("unsupported envelope is rejected");

    assert!(error.to_string().contains("unsupported encryption envelope scheme"));
    assert_eq!(load_calls.load(Ordering::SeqCst), 0);
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

struct LoadCountingKeyProvider {
    load_calls: Arc<AtomicUsize>,
}

impl KeyProvider for LoadCountingKeyProvider {
    fn load_key(&self) -> PrivacyResult<KeyMaterial> {
        self.load_calls.fetch_add(1, Ordering::SeqCst);
        Err(PrivacyError::KeyUnavailable("load_key should not be called for unsupported envelopes".to_owned()))
    }
}
