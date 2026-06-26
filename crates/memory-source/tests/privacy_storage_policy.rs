use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use memory_privacy::{FileKeyProvider, PrivacyEncryptor};
use memory_source::{
    capture_web_source_with_resolver, extract::DEFAULT_EXTRACTED_TEXT_CAP, storage::ArtifactStore, AddressPolicy,
    CaptureStatus, CaptureWebSourceRequest, RawStorage, StaticDnsResolver,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[tokio::test]
async fn extracted_sensitive_text_refuses_before_artifact_write() {
    let (url, resolver) = spawn_once("text/plain", "Safe quote plus SSN 123-45-6789").await;
    let temp = tempfile::tempdir().unwrap();
    let err = capture_web_source_with_resolver(
        temp.path(),
        CaptureWebSourceRequest {
            url,
            excerpts: vec!["Safe quote".to_string()],
            note: None,
            ..CaptureWebSourceRequest::default()
        },
        &resolver,
        AddressPolicy::AllowLoopbackForTests,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("privacy"));
    assert!(!temp.path().join("sources").exists());
}

#[tokio::test]
async fn extracted_text_requiring_encryption_refuses_v0_1() {
    let (url, resolver) = spawn_once("text/plain", "Email trey@example.com before launch. Safe quote.").await;
    let temp = tempfile::tempdir().unwrap();
    let err = capture_web_source_with_resolver(
        temp.path(),
        CaptureWebSourceRequest {
            url,
            excerpts: vec!["Safe quote".to_string()],
            note: None,
            ..CaptureWebSourceRequest::default()
        },
        &resolver,
        AddressPolicy::AllowLoopbackForTests,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("encrypted_source_artifacts_unsupported"));
}

#[tokio::test]
async fn extracted_text_requiring_encryption_writes_ciphertext_with_key() {
    let body = "Email trey@example.com before launch. Safe quote.";
    let (url, resolver) = spawn_once("text/plain", body).await;
    let temp = tempfile::tempdir().unwrap();
    let key_path = temp.path().join("runtime/privacy/age-key.json");
    let key_provider = FileKeyProvider::new(&key_path);
    key_provider.onboard_local_file().unwrap();
    let response = capture_web_source_with_resolver(
        temp.path(),
        CaptureWebSourceRequest {
            url,
            excerpts: vec!["Safe quote".to_string()],
            note: None,
            key_path: Some(key_path.clone()),
            ..CaptureWebSourceRequest::default()
        },
        &resolver,
        AddressPolicy::AllowLoopbackForTests,
    )
    .await
    .unwrap();

    assert!(response.warnings.contains(&"extracted_encrypted".to_string()));
    let artifact_id = memory_source::SourceArtifactId::try_new(response.artifact_id).unwrap();
    let artifact = ArtifactStore::new(temp.path()).verify_artifact_id(&artifact_id).unwrap();
    let artifact_dir = temp.path().join("sources/web").join(artifact.manifest.captured_at.format("%Y/%m").to_string());
    let artifact_dir = artifact_dir.join(artifact.manifest.artifact_id.as_str());
    assert!(!artifact_dir.join("extracted.txt").exists());
    assert!(artifact_dir.join("extracted.enc.age").exists());
    assert_eq!(artifact.extracted_text, "");
    assert!(artifact.encrypted_extracted_bytes.is_some());

    let payload = memory_privacy::EncryptedPayload {
        ciphertext: artifact.encrypted_extracted_bytes.unwrap(),
        envelope: serde_json::json!({
            "scheme": artifact.manifest.encryption_envelope.as_ref().unwrap().scheme,
            "recipient": artifact.manifest.encryption_envelope.as_ref().unwrap().recipient,
        }),
    };
    let plaintext = PrivacyEncryptor::new(key_provider).decrypt(&payload).unwrap();
    assert_eq!(plaintext, body);
}

#[tokio::test]
async fn sensitive_excerpt_refuses_even_when_artifact_encryption_key_exists() {
    let body = "Email trey@example.com before launch. Safe quote.";
    let (url, resolver) = spawn_once("text/plain", body).await;
    let temp = tempfile::tempdir().unwrap();
    let key_path = temp.path().join("runtime/privacy/age-key.json");
    FileKeyProvider::new(&key_path).onboard_local_file().unwrap();
    let err = capture_web_source_with_resolver(
        temp.path(),
        CaptureWebSourceRequest {
            url,
            excerpts: vec!["trey@example.com".to_string()],
            note: None,
            key_path: Some(key_path),
            ..CaptureWebSourceRequest::default()
        },
        &resolver,
        AddressPolicy::AllowLoopbackForTests,
    )
    .await
    .unwrap_err();

    assert!(err.to_string().contains("excerpts must be safe plaintext"), "{err}");
    assert!(!temp.path().join("sources").exists());
}

#[tokio::test]
async fn unsafe_raw_html_omits_raw_but_safe_extraction_remains_groundable() {
    let body = "<html><head><script>SSN 123-45-6789</script></head><body>Safe exact quote.</body></html>";
    let (url, resolver) = spawn_once("text/html; charset=utf-8", body).await;
    let temp = tempfile::tempdir().unwrap();
    let response = capture_web_source_with_resolver(
        temp.path(),
        CaptureWebSourceRequest {
            url,
            excerpts: vec!["Safe exact quote.".to_string()],
            note: None,
            ..CaptureWebSourceRequest::default()
        },
        &resolver,
        AddressPolicy::AllowLoopbackForTests,
    )
    .await
    .unwrap();
    assert_eq!(response.capture_status, CaptureStatus::CompleteTextOnly);
    let store = ArtifactStore::new(temp.path());
    let artifact_id = memory_source::SourceArtifactId::try_new(response.artifact_id).unwrap();
    let artifact = store.verify_artifact_id(&artifact_id).unwrap();
    assert!(artifact.raw_bytes.is_none());
    assert_eq!(store.resolve_excerpt_ref(&response.source_refs[0]).unwrap().quote, "Safe exact quote.");
}

#[tokio::test]
async fn raw_privacy_check_scans_beyond_extraction_projection_cap() {
    let body = format!(
        "<html><body>Safe exact quote.</body><script>{}trey@example.com</script></html>",
        "a".repeat(DEFAULT_EXTRACTED_TEXT_CAP * 4)
    );
    let (url, resolver) = spawn_once("text/html; charset=utf-8", &body).await;
    let temp = tempfile::tempdir().unwrap();
    let response = capture_web_source_with_resolver(
        temp.path(),
        CaptureWebSourceRequest {
            url,
            excerpts: vec!["Safe exact quote.".to_string()],
            note: None,
            ..CaptureWebSourceRequest::default()
        },
        &resolver,
        AddressPolicy::AllowLoopbackForTests,
    )
    .await
    .unwrap();

    assert_eq!(response.capture_status, CaptureStatus::CompleteTextOnly);
    let store = ArtifactStore::new(temp.path());
    let artifact_id = memory_source::SourceArtifactId::try_new(response.artifact_id).unwrap();
    let artifact = store.verify_artifact_id(&artifact_id).unwrap();
    assert!(artifact.raw_bytes.is_none());
}

#[tokio::test]
async fn unsafe_raw_html_encrypts_raw_with_key_instead_of_omitting() {
    let body = "<html><head><script>SSN 123-45-6789</script></head><body>Safe exact quote.</body></html>";
    let (url, resolver) = spawn_once("text/html; charset=utf-8", body).await;
    let temp = tempfile::tempdir().unwrap();
    let key_path = temp.path().join("runtime/privacy/age-key.json");
    let key_provider = FileKeyProvider::new(&key_path);
    key_provider.onboard_local_file().unwrap();
    let response = capture_web_source_with_resolver(
        temp.path(),
        CaptureWebSourceRequest {
            url,
            excerpts: vec!["Safe exact quote.".to_string()],
            note: None,
            key_path: Some(key_path.clone()),
            ..CaptureWebSourceRequest::default()
        },
        &resolver,
        AddressPolicy::AllowLoopbackForTests,
    )
    .await
    .unwrap();

    assert_eq!(response.capture_status, CaptureStatus::Complete);
    assert!(response.warnings.contains(&"raw_encrypted".to_string()));
    let artifact_id = memory_source::SourceArtifactId::try_new(response.artifact_id).unwrap();
    let artifact = ArtifactStore::new(temp.path()).verify_artifact_id(&artifact_id).unwrap();
    assert_eq!(artifact.manifest.raw_storage, RawStorage::Encrypted);
    assert!(artifact.raw_bytes.is_none());
    let encrypted_raw = artifact.encrypted_raw_bytes.unwrap();
    let payload = memory_privacy::EncryptedPayload {
        ciphertext: encrypted_raw,
        envelope: serde_json::json!({
            "scheme": artifact.manifest.encryption_envelope.as_ref().unwrap().scheme,
            "recipient": artifact.manifest.encryption_envelope.as_ref().unwrap().recipient,
        }),
    };
    let encoded_raw = PrivacyEncryptor::new(key_provider).decrypt(&payload).unwrap();
    let decoded_raw = BASE64_STANDARD.decode(encoded_raw).unwrap();
    assert_eq!(decoded_raw, body.as_bytes());
}

async fn spawn_once(content_type: &str, body: &str) -> (String, StaticDnsResolver) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let body = body.to_string();
    let content_type = content_type.to_string();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buf = [0_u8; 1024];
        let _ = stream.read(&mut buf).await.unwrap();
        let response =
            format!("HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\r\n{body}", body.len());
        stream.write_all(response.as_bytes()).await.unwrap();
        stream.shutdown().await.unwrap();
    });
    let resolver = StaticDnsResolver::new(vec![std::net::SocketAddr::new("127.0.0.1".parse().unwrap(), addr.port())]);
    (format!("http://example.test:{}", addr.port()), resolver)
}
