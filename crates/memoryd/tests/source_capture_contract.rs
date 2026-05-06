use memory_substrate::{InitOptions, Roots, Substrate};
use memoryd::handlers::handle_request;
use memoryd::protocol::{RequestEnvelope, RequestPayload, ResponseResult};

#[tokio::test]
async fn source_capture_rejects_empty_or_sensitive_operator_inputs_before_network() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = Substrate::init(
        Roots::new(temp.path().join("repo"), temp.path().join("runtime")),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_capturecontract".to_owned()) },
    )
    .await
    .expect("init substrate");

    let response = handle_request(
        &substrate,
        RequestEnvelope::new(
            "capture-empty",
            RequestPayload::CaptureSource { url: "https://example.com".to_string(), excerpts: Vec::new(), note: None },
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
            RequestPayload::CaptureSource {
                url: "https://example.com".to_string(),
                excerpts: vec!["quote".to_string()],
                note: Some("SSN 123-45-6789".to_string()),
            },
        ),
    )
    .await;
    match response.result {
        ResponseResult::Error(error) => assert_eq!(error.code, "invalid_request"),
        other => panic!("expected invalid request, got {other:?}"),
    }
}
