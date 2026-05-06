use memory_source::{
    capture_web_source_with_resolver, extract::DEFAULT_EXTRACTED_TEXT_CAP, storage::ArtifactStore, AddressPolicy,
    CaptureWebSourceRequest, StaticDnsResolver,
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
