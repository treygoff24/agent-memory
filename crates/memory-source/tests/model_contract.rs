use chrono::Utc;
use memory_source::{
    CaptureMethod, CaptureRequestSnapshot, CaptureResponseSnapshot, CaptureStatus, ExcerptLocator, ExcerptMatchKind,
    ExcerptRecord, RawStorage, RedirectHop, SourceArtifactId, WebCaptureManifest, WebCaptureSourceRef,
};

#[test]
fn artifact_id_accepts_only_prefixed_crockford_ulid() {
    assert!(SourceArtifactId::try_new("src_01J0Z7Y8Q9R0ABCDE123456789").is_ok());
    for invalid in [
        "01J0Z7Y8Q9R0ABCDE123456789",
        "src_01J0Z7Y8Q9R0ABCDE12345678",
        "src_01J0Z7Y8Q9R0ABCDE1234567890",
        "src_01J0Z7Y8Q9R0ABCDE12345678I",
        "src_01j0z7y8q9r0abcde123456789",
    ] {
        assert!(SourceArtifactId::try_new(invalid).is_err(), "{invalid} must be rejected");
    }
}

#[test]
fn manifest_serializes_contract_enums_as_snake_case() {
    let artifact_id = SourceArtifactId::try_new("src_01J0Z7Y8Q9R0ABCDE123456789").unwrap();
    let manifest = WebCaptureManifest {
        schema_version: 1,
        artifact_id,
        kind: "web_capture".to_string(),
        original_url: "https://example.com/report".to_string(),
        final_url: "https://example.com/report".to_string(),
        redirect_chain: vec![RedirectHop {
            url: "https://example.com".into(),
            status: 301,
            location: "https://example.com/report".into(),
        }],
        captured_at: Utc::now(),
        capture_method: CaptureMethod::HttpStaticV1,
        request: CaptureRequestSnapshot::default(),
        response: CaptureResponseSnapshot::default(),
        raw_sha256: None,
        raw_zstd_sha256: None,
        raw_storage: RawStorage::OmittedPrivacy,
        raw_omitted_reason: Some("raw privacy policy".to_string()),
        extracted_text_sha256: "sha256:extract".to_string(),
        excerpts_sha256: "sha256:excerpt".to_string(),
        raw_byte_len: 0,
        extracted_text_byte_len: 0,
        capture_status: CaptureStatus::CompleteTextOnly,
        warnings: Vec::new(),
        merge_conflict: None,
    };

    let json = serde_json::to_string(&manifest).unwrap();
    assert!(json.contains("\"capture_status\":\"complete_text_only\""));
    assert!(json.contains("\"capture_method\":\"http_static_v1\""));
    assert!(json.contains("\"raw_storage\":\"omitted_privacy\""));
}

#[test]
fn source_ref_requires_webcap_artifact_and_excerpt() {
    let source_ref = WebCaptureSourceRef::parse("webcap:src_01J0Z7Y8Q9R0ABCDE123456789#quote_0001").unwrap();
    assert_eq!(source_ref.artifact_id().as_str(), "src_01J0Z7Y8Q9R0ABCDE123456789");
    assert_eq!(source_ref.excerpt_id(), "quote_0001");
    assert_eq!(source_ref.to_string(), "webcap:src_01J0Z7Y8Q9R0ABCDE123456789#quote_0001");
    assert!(WebCaptureSourceRef::parse("https://example.com/report").is_err());
    assert!(WebCaptureSourceRef::parse("webcap:src_01J0Z7Y8Q9R0ABCDE123456789").is_err());
}

#[test]
fn excerpt_record_shape_is_stable() {
    let artifact_id = SourceArtifactId::try_new("src_01J0Z7Y8Q9R0ABCDE123456789").unwrap();
    let record = ExcerptRecord {
        excerpt_id: "quote_0001".to_string(),
        artifact_id,
        quote: "quoted text".to_string(),
        quote_sha256: "sha256:quote".to_string(),
        locator: ExcerptLocator::ByteRange { start: 0, end: 11 },
        match_kind: ExcerptMatchKind::Exact,
        created_at: Utc::now(),
    };
    let value = serde_json::to_value(record).unwrap();
    assert_eq!(value["locator"]["kind"], "byte_range");
    assert_eq!(value["match_kind"], "exact");
}
