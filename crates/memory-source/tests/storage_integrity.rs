use chrono::Utc;
use memory_privacy::{FileKeyProvider, PrivacyEncryptor};
use memory_source::hash::sha256_prefixed;
use memory_source::storage::excerpts_jsonl;
use memory_source::{
    ArtifactStore, CaptureMethod, CaptureRequestSnapshot, CaptureResponseSnapshot, CaptureStatus, EncryptionEnvelope,
    ExcerptLocator, ExcerptMatchKind, ExcerptRecord, ExtractedTextStorage, RawStorage, SourceArtifactId,
    WebCaptureArtifact, WebCaptureManifest, WebCaptureSourceRef,
};

fn fixture(raw_storage: RawStorage) -> WebCaptureArtifact {
    let artifact_id = SourceArtifactId::try_new("src_01J0Z7Y8Q9R0ABCDE123456789").unwrap();
    let extracted_text = "The exact quote supports the saved claim.".to_string();
    let excerpt = ExcerptRecord {
        excerpt_id: "quote_0001".to_string(),
        artifact_id: artifact_id.clone(),
        quote: "exact quote".to_string(),
        quote_sha256: sha256_prefixed("exact quote".as_bytes()),
        locator: ExcerptLocator::ByteRange { start: 4, end: 15 },
        match_kind: ExcerptMatchKind::Exact,
        created_at: Utc::now(),
    };
    let excerpts = vec![excerpt];
    let raw = b"<html>The exact quote supports the saved claim.</html>".to_vec();
    let raw_zstd = zstd::encode_all(raw.as_slice(), 0).unwrap();
    let excerpts_jsonl = excerpts_jsonl(&excerpts).unwrap();
    let manifest = WebCaptureManifest {
        schema_version: 2,
        artifact_id,
        kind: "web_capture".to_string(),
        original_url: "https://example.com/report".to_string(),
        final_url: "https://example.com/report".to_string(),
        redirect_chain: Vec::new(),
        captured_at: Utc::now(),
        capture_method: CaptureMethod::HttpStaticV1,
        request: CaptureRequestSnapshot::default(),
        response: CaptureResponseSnapshot { http_status: 200, ..CaptureResponseSnapshot::default() },
        raw_sha256: Some(sha256_prefixed(&raw)),
        raw_zstd_sha256: matches!(raw_storage, RawStorage::Stored).then(|| sha256_prefixed(&raw_zstd)),
        raw_encrypted_sha256: None,
        raw_storage,
        raw_omitted_reason: (!matches!(raw_storage, RawStorage::Stored)).then(|| "privacy".to_string()),
        extracted_text_storage: ExtractedTextStorage::Plaintext,
        encryption_envelope: None,
        extracted_text_sha256: Some(sha256_prefixed(extracted_text.as_bytes())),

        extracted_text_encrypted_sha256: None,
        excerpts_sha256: sha256_prefixed(excerpts_jsonl.as_bytes()),
        raw_byte_len: raw.len(),
        extracted_text_byte_len: Some(extracted_text.len()),

        extracted_text_encrypted_byte_len: None,
        capture_status: CaptureStatus::Complete,
        warnings: Vec::new(),
        merge_conflict: None,
    };
    WebCaptureArtifact {
        manifest,
        extracted_text,
        excerpts,
        raw_bytes: matches!(raw_storage, RawStorage::Stored).then_some(raw),
        encrypted_extracted_bytes: None,
        encrypted_raw_bytes: None,
    }
}

fn encrypted_fixture() -> WebCaptureArtifact {
    let mut artifact = fixture(RawStorage::OmittedPrivacy);
    let key_dir = tempfile::tempdir().unwrap();
    let key_provider = FileKeyProvider::new(key_dir.path().join("age-key.json"));
    key_provider.onboard_local_file().unwrap();
    let encryptor = PrivacyEncryptor::new(key_provider);
    let encrypted_extracted = encryptor.encrypt("encrypted extracted text").unwrap().ciphertext;
    let encrypted_raw = encryptor.encrypt("encoded raw bytes").unwrap().ciphertext;
    artifact.manifest.raw_storage = RawStorage::Encrypted;
    artifact.manifest.raw_sha256 = None;
    artifact.manifest.raw_zstd_sha256 = None;
    artifact.manifest.raw_encrypted_sha256 = Some(sha256_prefixed(&encrypted_raw));
    artifact.manifest.raw_omitted_reason = None;
    artifact.manifest.extracted_text_storage = ExtractedTextStorage::Encrypted;
    artifact.manifest.extracted_text_sha256 = None;
    artifact.manifest.extracted_text_byte_len = None;
    artifact.manifest.extracted_text_encrypted_sha256 = Some(sha256_prefixed(&encrypted_extracted));
    artifact.manifest.extracted_text_encrypted_byte_len = Some(encrypted_extracted.len());
    artifact.manifest.encryption_envelope = Some(EncryptionEnvelope::age_x25519("age1test"));
    artifact.extracted_text.clear();
    artifact.raw_bytes = None;
    artifact.encrypted_extracted_bytes = Some(encrypted_extracted);
    artifact.encrypted_raw_bytes = Some(encrypted_raw);
    artifact
}

#[test]
fn write_and_verify_web_capture_artifact() {
    let temp = tempfile::tempdir().unwrap();
    let store = ArtifactStore::new(temp.path());
    let path = store.write_web_capture(&fixture(RawStorage::Stored)).unwrap();
    assert!(temp.path().join(path.relative()).join("manifest.json").exists());
    assert!(temp.path().join(path.relative()).join("extracted.txt").exists());
    assert!(temp.path().join(path.relative()).join("excerpts.jsonl").exists());
    assert!(temp.path().join(path.relative()).join("raw.bin.zst").exists());
    let verified = store.verify_web_capture(&path).unwrap();
    assert_eq!(verified.excerpts[0].quote, "exact quote");
    let source_ref = WebCaptureSourceRef::new(verified.manifest.artifact_id, "quote_0001").to_string();
    assert_eq!(store.resolve_excerpt_ref(&source_ref).unwrap().quote, "exact quote");
}

#[test]
fn raw_is_written_only_when_stored() {
    let temp = tempfile::tempdir().unwrap();
    let store = ArtifactStore::new(temp.path());
    let mut artifact = fixture(RawStorage::OmittedPrivacy);
    artifact.manifest.capture_status = CaptureStatus::CompleteTextOnly;
    let path = store.write_web_capture(&artifact).unwrap();
    assert!(!temp.path().join(path.relative()).join("raw.bin.zst").exists());
    store.verify_web_capture(&path).unwrap();
}

#[test]
fn encrypted_artifacts_verify_ciphertext_hashes() {
    let temp = tempfile::tempdir().unwrap();
    let store = ArtifactStore::new(temp.path());
    let path = store.write_web_capture(&encrypted_fixture()).unwrap();
    let dir = temp.path().join(path.relative());
    assert!(!dir.join("extracted.txt").exists());
    assert!(!dir.join("raw.bin.zst").exists());
    assert!(dir.join("extracted.enc.age").exists());
    assert!(dir.join("raw.enc.age").exists());
    let verified = store.verify_web_capture(&path).unwrap();
    assert_eq!(verified.raw_bytes, None);
    assert!(verified.encrypted_extracted_bytes.is_some());
    assert!(verified.encrypted_raw_bytes.is_some());

    std::fs::write(dir.join("raw.enc.age"), b"tampered").unwrap();
    assert!(store.verify_web_capture(&path).is_err());
}

#[test]
fn mutations_fail_integrity_verification() {
    let temp = tempfile::tempdir().unwrap();
    let store = ArtifactStore::new(temp.path());
    let path = store.write_web_capture(&fixture(RawStorage::Stored)).unwrap();
    std::fs::write(temp.path().join(path.relative()).join("extracted.txt"), "tampered").unwrap();
    assert!(store.verify_web_capture(&path).is_err());

    let temp = tempfile::tempdir().unwrap();
    let store = ArtifactStore::new(temp.path());
    let path = store.write_web_capture(&fixture(RawStorage::Stored)).unwrap();
    std::fs::write(temp.path().join(path.relative()).join("raw.bin.zst"), b"tampered").unwrap();
    assert!(store.verify_web_capture(&path).is_err());
}

#[test]
fn partial_or_failed_artifact_is_not_groundable() {
    for status in [CaptureStatus::Partial, CaptureStatus::Failed] {
        let temp = tempfile::tempdir().unwrap();
        let store = ArtifactStore::new(temp.path());
        let mut artifact = fixture(RawStorage::OmittedPrivacy);
        artifact.manifest.capture_status = status;
        let path = store.source_artifact_path(&artifact.manifest.artifact_id, artifact.manifest.captured_at);
        let dir = temp.path().join(path.relative());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("manifest.json"), serde_json::to_vec(&artifact.manifest).unwrap()).unwrap();
        std::fs::write(dir.join("extracted.txt"), artifact.extracted_text).unwrap();
        std::fs::write(dir.join("excerpts.jsonl"), excerpts_jsonl(&artifact.excerpts).unwrap()).unwrap();
        assert!(store.verify_web_capture(&path).is_err());
    }
}
