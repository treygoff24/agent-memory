use std::path::PathBuf;

use memory_source::{ArtifactStore, CaptureMethod, SourceArtifactId};
use memory_substrate::{InitOptions, Roots, Substrate};
use memoryd::handlers::handle_request;
use memoryd::protocol::{
    CaptureSourceMode, RequestEnvelope, RequestPayload, ResponsePayload, ResponseResult, SourceCapturePayload,
};

#[tokio::test]
async fn source_capture_rejects_empty_or_sensitive_operator_inputs_before_network() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(temp.path()).await;

    let response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "capture-empty",
            RequestPayload::CaptureSource(SourceCapturePayload {
                source: "https://example.com".to_string(),
                excerpts: Vec::new(),
                ..SourceCapturePayload::default()
            }),
        ),
    )
    .await;
    match response.result {
        ResponseResult::Error(error) => assert_eq!(error.code, "invalid_request"),
        other => panic!("expected invalid request, got {other:?}"),
    }

    let response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "capture-sensitive-note",
            RequestPayload::CaptureSource(SourceCapturePayload {
                source: "https://example.com".to_string(),
                excerpts: vec!["quote".to_string()],
                note: Some("SSN 123-45-6789".to_string()),
                ..SourceCapturePayload::default()
            }),
        ),
    )
    .await;
    match response.result {
        ResponseResult::Error(error) => assert_eq!(error.code, "invalid_request"),
        other => panic!("expected invalid request, got {other:?}"),
    }
}

#[tokio::test]
async fn source_capture_local_artifact_writes_local_capture_manifest() {
    let temp = tempfile::tempdir().expect("tempdir");
    let local_path = temp.path().join("evidence.md");
    std::fs::write(&local_path, "The daemon local quote is present.").expect("write local artifact");
    let substrate = init_substrate(temp.path()).await;

    let response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "capture-local",
            RequestPayload::CaptureSource(SourceCapturePayload {
                source: "local:evidence.md".to_string(),
                mode: CaptureSourceMode::LocalArtifact,
                excerpts: vec!["daemon local quote".to_string()],
                note: Some("safe local note".to_string()),
                local_path: Some(local_path),
            }),
        ),
    )
    .await;

    let ResponseResult::Success(ResponsePayload::CaptureSource(capture)) = response.result else {
        panic!("expected successful local capture, got {:?}", response.result);
    };
    assert_eq!(capture.final_url, "local:artifact");
    assert_eq!(capture.mode, CaptureSourceMode::LocalArtifact);
    assert_eq!(capture.source_refs.len(), 1);
    let artifact_id = SourceArtifactId::try_new(capture.artifact_id).expect("artifact id");
    let artifact = ArtifactStore::new(substrate.roots().repo.clone())
        .verify_artifact_id(&artifact_id)
        .expect("written local capture verifies");
    assert_eq!(artifact.manifest.capture_method, CaptureMethod::LocalArtifactV1);
    assert_eq!(artifact.manifest.response.http_status, 0);
    assert!(artifact.manifest.response.remote_addr.is_none());
}

#[tokio::test]
async fn source_capture_rejects_local_artifact_without_path_or_with_traversal() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(temp.path()).await;

    for (id, local_path, expected) in [
        ("missing-path", None, "requires local_path"),
        ("path-traversal", Some(PathBuf::from("../evidence.md")), "path traversal"),
    ] {
        let response = handle_request(
            &substrate,
            RequestEnvelope::new(
                id,
                RequestPayload::CaptureSource(SourceCapturePayload {
                    source: "local:evidence.md".to_string(),
                    mode: CaptureSourceMode::LocalArtifact,
                    excerpts: vec!["quote".to_string()],
                    note: None,
                    local_path,
                }),
            ),
        )
        .await;
        match response.result {
            ResponseResult::Error(error) => {
                assert_eq!(error.code, "invalid_request");
                assert!(error.message.contains(expected), "unexpected error: {error:?}");
            }
            other => panic!("expected invalid request, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn source_capture_rich_modes_return_typed_unsupported() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(temp.path()).await;

    for mode in [
        CaptureSourceMode::PdfText,
        CaptureSourceMode::BrowserRendered,
        CaptureSourceMode::Screenshot,
        CaptureSourceMode::Authenticated,
    ] {
        let response = handle_request(
            &substrate,
            RequestEnvelope::new(
                format!("unsupported-{mode:?}"),
                RequestPayload::CaptureSource(SourceCapturePayload {
                    source: "local:rich-artifact".to_string(),
                    mode,
                    excerpts: vec!["quote".to_string()],
                    note: None,
                    local_path: None,
                }),
            ),
        )
        .await;
        match response.result {
            ResponseResult::Error(error) => {
                assert_eq!(error.code, "unsupported");
                assert!(
                    error.message.contains("save/export a text/html"),
                    "unsupported guidance should be actionable: {error:?}"
                );
            }
            other => panic!("expected unsupported error, got {other:?}"),
        }
    }
}

#[test]
fn source_capture_protocol_rejects_external_key_material_and_bypass_flags() {
    for field in ["key_path", "raw_key", "key_material", "allow_private_network", "privacy_bypass"] {
        let json = format!(
            r#"{{
                "id": "unsafe-capture",
                "request": {{
                    "capture_source": {{
                        "source": "https://example.com",
                        "excerpts": ["quote"],
                        "{field}": true
                    }}
                }}
            }}"#
        );
        let error = RequestEnvelope::from_json_line(&json).expect_err("unsafe field must be rejected");
        assert!(error.to_string().contains(field), "unexpected error for {field}: {error}");
    }
}

async fn init_substrate(root: &std::path::Path) -> Substrate {
    Substrate::init(
        Roots::new(root.join("repo"), root.join("runtime")),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_capturecontract".to_owned()) },
    )
    .await
    .expect("init substrate")
}
