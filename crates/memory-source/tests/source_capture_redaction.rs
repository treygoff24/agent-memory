use memory_source::storage::ArtifactStore;
use memory_source::{
    capture_web_source_with_resolver, url_safety, AddressPolicy, CaptureWebSourceRequest, SourceArtifactId,
    StaticDnsResolver,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[test]
fn redacts_sensitive_query_params_and_fragments() {
    let url = url::Url::parse("https://example.com/path?token=secret&keep=yes#access_token=secret").unwrap();
    let redacted = url_safety::redact_sensitive_url(&url);

    assert_eq!(redacted.as_str(), "https://example.com/path?keep=yes");
}

#[test]
fn redacts_relative_location_headers_against_base_url() {
    let base = url::Url::parse("https://example.com/start").unwrap();

    assert_eq!(url_safety::redact_sensitive_location_header("/reset?token=secret&keep=yes", &base), "/reset?keep=yes");
}

#[tokio::test]
async fn capture_persists_only_redacted_urls() {
    let ok_body = "The exact relevant quote is present.";
    let (base_url, resolver) = spawn_server(vec![
        "HTTP/1.1 302 Found\r\nLocation: /final?token=redirect-secret&keep=yes\r\nContent-Length: 0\r\n\r\n"
            .to_string(),
        format!("HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{}", ok_body.len(), ok_body),
    ])
    .await;
    let temp = tempfile::tempdir().unwrap();

    let response = capture_web_source_with_resolver(
        temp.path(),
        CaptureWebSourceRequest {
            url: format!("{base_url}/start?api_key=initial-secret&keep=1"),
            excerpts: vec!["exact relevant quote".to_string()],
            note: None,
        },
        &resolver,
        AddressPolicy::AllowLoopbackForTests,
    )
    .await
    .unwrap();

    assert!(!response.final_url.contains("redirect-secret"));
    let artifact_id = SourceArtifactId::try_new(response.artifact_id).unwrap();
    let artifact = ArtifactStore::new(temp.path()).verify_artifact_id(&artifact_id).unwrap();
    let manifest_json = serde_json::to_string(&artifact.manifest).unwrap();
    assert!(!manifest_json.contains("initial-secret"));
    assert!(!manifest_json.contains("redirect-secret"));
    assert!(manifest_json.contains("keep=1"));
    assert!(manifest_json.contains("keep=yes"));
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
    let resolver = StaticDnsResolver::new(vec![std::net::SocketAddr::new("127.0.0.1".parse().unwrap(), addr.port())]);
    (format!("http://example.test:{}", addr.port()), resolver)
}
