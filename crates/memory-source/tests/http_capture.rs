use std::net::SocketAddr;

use memory_source::{
    capture_web_source_with_resolver, AddressPolicy, ArtifactStore, CaptureMethod, CaptureMode,
    CaptureWebSourceRequest, SourceArtifactId, StaticDnsResolver,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[tokio::test]
async fn capture_records_final_url_redirect_and_source_ref() {
    let ok_body = "<html><body>The exact relevant quote is present.</body></html>";
    let (base_url, resolver) = spawn_server(vec![
        "HTTP/1.1 302 Found\r\nLocation: /final\r\nContent-Length: 0\r\n\r\n".to_string(),
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nETag: \"abc\"\r\nLast-Modified: Tue, 05 May 2026 17:00:00 GMT\r\nContent-Length: {}\r\n\r\n{}",
            ok_body.len(),
            ok_body
        ),
    ])
    .await;
    let temp = tempfile::tempdir().unwrap();
    let response = capture_web_source_with_resolver(
        temp.path(),
        CaptureWebSourceRequest {
            url: format!("{base_url}/start"),
            excerpts: vec!["exact relevant quote".to_string()],
            note: None,
            ..CaptureWebSourceRequest::default()
        },
        &resolver,
        AddressPolicy::AllowLoopbackForTests,
    )
    .await
    .unwrap();
    assert_eq!(response.capture_status, "complete");
    assert_eq!(response.final_url, format!("{base_url}/final"));
    assert_eq!(response.source_refs.len(), 1);
    assert!(response.source_refs[0].starts_with("webcap:src_"));
    let artifact_id = SourceArtifactId::try_new(response.artifact_id).unwrap();
    let artifact = ArtifactStore::new(temp.path()).verify_artifact_id(&artifact_id).unwrap();
    assert_eq!(artifact.manifest.redirect_chain.len(), 1);
    assert_eq!(artifact.manifest.response.etag.as_deref(), Some("\"abc\""));
    assert_eq!(artifact.manifest.response.last_modified.as_deref(), Some("Tue, 05 May 2026 17:00:00 GMT"));
}

#[tokio::test]
async fn http_errors_and_oversized_bodies_fail_before_groundable_artifact() {
    let (base_url, resolver) =
        spawn_server(vec!["HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n".to_string()]).await;
    let temp = tempfile::tempdir().unwrap();
    let err = capture_web_source_with_resolver(
        temp.path(),
        CaptureWebSourceRequest {
            url: base_url,
            excerpts: vec!["quote".to_string()],
            note: None,
            ..CaptureWebSourceRequest::default()
        },
        &resolver,
        AddressPolicy::AllowLoopbackForTests,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("HTTP status 500"));
    assert!(!temp.path().join("sources").exists());

    let body = "a".repeat(2 * 1024 * 1024 + 1);
    let response =
        format!("HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{}", body.len(), body);
    let (base_url, resolver) = spawn_server(vec![response]).await;
    let temp = tempfile::tempdir().unwrap();
    let err = capture_web_source_with_resolver(
        temp.path(),
        CaptureWebSourceRequest {
            url: base_url,
            excerpts: vec!["a".to_string()],
            note: None,
            ..CaptureWebSourceRequest::default()
        },
        &resolver,
        AddressPolicy::AllowLoopbackForTests,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("exceeded"));
}

#[tokio::test]
async fn oversized_content_length_fails_before_reading_body() {
    let response =
        format!("HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n", 2 * 1024 * 1024 + 1);
    let (base_url, resolver) = spawn_server(vec![response]).await;
    let temp = tempfile::tempdir().unwrap();

    let err = capture_web_source_with_resolver(
        temp.path(),
        CaptureWebSourceRequest {
            url: base_url,
            excerpts: vec!["quote".to_string()],
            note: None,
            ..CaptureWebSourceRequest::default()
        },
        &resolver,
        AddressPolicy::AllowLoopbackForTests,
    )
    .await
    .unwrap_err();

    assert!(err.to_string().contains("exceeded"), "{err}");
    assert!(!temp.path().join("sources").exists());
}

#[tokio::test]
async fn mixed_public_private_resolution_is_rejected() {
    let resolver = StaticDnsResolver::new(vec!["93.184.216.34:80".parse().unwrap(), "127.0.0.1:80".parse().unwrap()]);
    let temp = tempfile::tempdir().unwrap();
    assert!(capture_web_source_with_resolver(
        temp.path(),
        CaptureWebSourceRequest {
            url: "http://example.test/".to_string(),
            excerpts: vec!["quote".to_string()],
            note: None,
            ..CaptureWebSourceRequest::default()
        },
        &resolver,
        AddressPolicy::PublicOnly,
    )
    .await
    .is_err());
}

#[tokio::test]
async fn local_artifact_capture_records_local_semantics_without_http_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let local_path = temp.path().join("evidence.txt");
    std::fs::write(&local_path, "The local exact quote is present.").unwrap();
    let resolver = StaticDnsResolver::new(Vec::new());

    let response = capture_web_source_with_resolver(
        temp.path(),
        CaptureWebSourceRequest {
            url: "local:evidence.txt".to_string(),
            excerpts: vec!["local exact quote".to_string()],
            note: None,
            mode: CaptureMode::LocalArtifact,
            local_path: Some(local_path),
            ..CaptureWebSourceRequest::default()
        },
        &resolver,
        AddressPolicy::AllowLoopbackForTests,
    )
    .await
    .unwrap();

    assert_eq!(response.final_url, "local:artifact");
    let artifact_id = SourceArtifactId::try_new(response.artifact_id).unwrap();
    let artifact = ArtifactStore::new(temp.path()).verify_artifact_id(&artifact_id).unwrap();
    assert_eq!(artifact.manifest.capture_method, CaptureMethod::LocalArtifactV1);
    assert_eq!(artifact.manifest.redirect_chain, Vec::new());
    assert_eq!(artifact.manifest.response.http_status, 0);
    assert!(artifact.manifest.response.remote_addr.is_none());
}

#[tokio::test]
async fn oversized_local_artifact_fails_before_groundable_artifact() {
    let temp = tempfile::tempdir().unwrap();
    let local_path = temp.path().join("oversized.txt");
    std::fs::write(&local_path, vec![b'a'; 2 * 1024 * 1024 + 1]).unwrap();
    let resolver = StaticDnsResolver::new(Vec::new());

    let err = capture_web_source_with_resolver(
        temp.path(),
        CaptureWebSourceRequest {
            url: "local:oversized".to_string(),
            excerpts: vec!["a".to_string()],
            note: None,
            mode: CaptureMode::LocalArtifact,
            local_path: Some(local_path),
            ..CaptureWebSourceRequest::default()
        },
        &resolver,
        AddressPolicy::AllowLoopbackForTests,
    )
    .await
    .unwrap_err();

    assert!(err.to_string().contains("exceeded"), "{err}");
    assert!(!temp.path().join("sources").exists());
}

async fn spawn_server(responses: Vec<String>) -> (String, StaticDnsResolver) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        for response in responses {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0_u8; 1024];
            let _ = stream.read(&mut buf).await.unwrap();
            stream.write_all(response.as_bytes()).await.unwrap();
            stream.shutdown().await.unwrap();
        }
    });
    let resolver = StaticDnsResolver::new(vec![SocketAddr::new("127.0.0.1".parse().unwrap(), addr.port())]);
    (format!("http://example.test:{}", addr.port()), resolver)
}
