use std::path::Path;

use memory_source::{
    capture_web_source_with_resolver, extract::DEFAULT_EXTRACTED_TEXT_CAP, storage::ArtifactStore, AddressPolicy,
    CaptureWebSourceRequest, RawStorage, StaticDnsResolver, WebCaptureManifest,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[tokio::test]
async fn extracted_sensitive_text_refuses_before_artifact_write() {
    let (url, resolver) = spawn_once("text/plain", "Safe quote plus SSN 123-45-6789").await;
    let temp = tempfile::tempdir().unwrap();
    let err = capture_web_source_with_resolver(
        temp.path(),
        CaptureWebSourceRequest { url, excerpts: vec!["Safe quote".to_string()], note: None },
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
        CaptureWebSourceRequest { url, excerpts: vec!["Safe quote".to_string()], note: None },
        &resolver,
        AddressPolicy::AllowLoopbackForTests,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("encrypted_source_artifacts_unsupported"));
    assert!(!temp.path().join("sources").exists());
}

#[tokio::test]
async fn unsafe_raw_html_omits_raw_but_safe_extraction_remains_groundable() {
    let body = "<html><head><script>SSN 123-45-6789</script></head><body>Safe exact quote.</body></html>";
    let (url, resolver) = spawn_once("text/html; charset=utf-8", body).await;
    let temp = tempfile::tempdir().unwrap();
    let response = capture_web_source_with_resolver(
        temp.path(),
        CaptureWebSourceRequest { url, excerpts: vec!["Safe exact quote.".to_string()], note: None },
        &resolver,
        AddressPolicy::AllowLoopbackForTests,
    )
    .await
    .unwrap();
    assert_eq!(response.capture_status, "complete_text_only");
    let store = ArtifactStore::new(temp.path());
    let artifact_id = memory_source::SourceArtifactId::try_new(response.artifact_id).unwrap();
    let artifact = store.verify_artifact_id(&artifact_id).unwrap();
    assert!(artifact.raw_bytes.is_none());
    assert_privacy_omitted_raw(temp.path(), &artifact.manifest, "SSN 123-45-6789");
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
        CaptureWebSourceRequest { url, excerpts: vec!["Safe exact quote.".to_string()], note: None },
        &resolver,
        AddressPolicy::AllowLoopbackForTests,
    )
    .await
    .unwrap();

    assert_eq!(response.capture_status, "complete_text_only");
    let store = ArtifactStore::new(temp.path());
    let artifact_id = memory_source::SourceArtifactId::try_new(response.artifact_id).unwrap();
    let artifact = store.verify_artifact_id(&artifact_id).unwrap();
    assert!(artifact.raw_bytes.is_none());
    assert_privacy_omitted_raw(temp.path(), &artifact.manifest, "trey@example.com");
}

fn assert_privacy_omitted_raw(repo_root: &Path, manifest: &WebCaptureManifest, forbidden_raw_text: &str) {
    assert_eq!(manifest.raw_storage, RawStorage::OmittedPrivacy);
    assert_raw_omitted_reason(manifest.raw_omitted_reason.as_deref());
    let store = ArtifactStore::new(repo_root);
    let artifact_path = store.find_artifact_path(&manifest.artifact_id).expect("artifact path");
    let artifact_dir = repo_root.join(artifact_path.relative());
    assert!(!artifact_dir.join("raw.bin.zst").exists(), "privacy-omitted raw capture must not write raw.bin.zst");

    let manifest_from_disk: WebCaptureManifest =
        serde_json::from_slice(&std::fs::read(artifact_dir.join("manifest.json")).expect("manifest file"))
            .expect("manifest json");
    assert_eq!(manifest_from_disk.raw_storage, RawStorage::OmittedPrivacy);
    assert_raw_omitted_reason(manifest_from_disk.raw_omitted_reason.as_deref());
    assert_persisted_files_do_not_contain(&artifact_dir, forbidden_raw_text);
}

fn assert_raw_omitted_reason(reason: Option<&str>) {
    let reason = reason.expect("privacy-omitted raw capture records omission reason");
    assert!(
        reason.contains("privacy") || reason.contains("safe plaintext"),
        "raw omission reason should explain the privacy/safe-plaintext policy, got {reason:?}"
    );
}

fn assert_persisted_files_do_not_contain(path: &Path, forbidden: &str) {
    for entry in std::fs::read_dir(path).expect("artifact dir") {
        let entry = entry.expect("artifact entry");
        let path = entry.path();
        if path.is_dir() {
            assert_persisted_files_do_not_contain(&path, forbidden);
            continue;
        }
        let bytes = std::fs::read(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
        let text = String::from_utf8_lossy(&bytes);
        assert!(!text.contains(forbidden), "{} should not persist raw text {forbidden:?}", path.display());
    }
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
