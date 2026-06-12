use chrono::Utc;
use memory_source::hash::sha256_prefixed;
use memory_source::storage::excerpts_jsonl;
use memory_source::{
    ArtifactStore, CaptureMethod, CaptureRequestSnapshot, CaptureResponseSnapshot, CaptureStatus, ExcerptLocator,
    ExcerptMatchKind, ExcerptRecord, ExtractedTextStorage, RawStorage, SourceArtifactId, WebCaptureArtifact,
    WebCaptureManifest,
};
use memory_substrate::{InitOptions, Roots, Substrate};
use memoryd::handlers::handle_request;
use memoryd::protocol::{
    GovernanceRefusalReason, GovernanceStatus, RequestEnvelope, RequestPayload, ResponsePayload, ResponseResult,
};

#[tokio::test]
async fn t20_web_source_grounding_accepts_verified_and_refuses_corrupt_artifact() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = Substrate::init(
        Roots::new(temp.path().join("repo"), temp.path().join("runtime")),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_t20webcap".to_string()) },
    )
    .await
    .expect("init substrate");
    let store = ArtifactStore::new(substrate.roots().repo.clone());
    let source_ref = write_artifact(&store);

    let accepted = write(&substrate, "t20-accepted", &source_ref).await;
    assert!(matches!(accepted.status, GovernanceStatus::Promoted | GovernanceStatus::Candidate));
    assert_ne!(accepted.reason, Some(GovernanceRefusalReason::Grounding));

    let artifact_id =
        SourceArtifactId::try_new(source_ref.trim_start_matches("webcap:").split('#').next().unwrap()).unwrap();
    let path = store.find_artifact_path(&artifact_id).expect("artifact path");
    std::fs::write(substrate.roots().repo.join(path.relative()).join("extracted.txt"), "corrupt").expect("corrupt");

    let refused = write(&substrate, "t20-refused", &source_ref).await;
    assert_eq!(refused.status, GovernanceStatus::Refused);
    assert_eq!(refused.reason, Some(GovernanceRefusalReason::Grounding));
    println!("MEMORUM_EVAL_ASSERTIONS=5");
}

async fn write(substrate: &Substrate, id: &str, source_ref: &str) -> memoryd::protocol::GovernanceWriteResponse {
    let response = handle_request(
        substrate,
        RequestEnvelope::new(
            id,
            RequestPayload::WriteMemory {
                body: "The web capture quote supports this eval memory.".to_string(),
                title: Some("T20 web source grounding".to_string()),
                tags: Vec::new(),
                meta: serde_json::json!({
                    "namespace": "project",
                    "canonical_namespace_id": "proj_eval_t20",
                    "namespace_alias": "eval-t20",
                    "type": "claim",
                    "summary": "T20 web source grounding",
                    "confidence": 0.95,
                    "sensitivity": "internal",
                    "source_kind": "web_capture",
                    "source_ref": source_ref
                }),
            },
        ),
    )
    .await;
    let ResponseResult::Success(ResponsePayload::GovernanceWrite(write)) = response.result else {
        panic!("expected governance write, got {:?}", response.result);
    };
    write
}

fn write_artifact(store: &ArtifactStore) -> String {
    let artifact_id = SourceArtifactId::try_new("src_01J0Z7Y8Q9R0ABCDE123456789").unwrap();
    let extracted_text = "web capture quote supports this eval".to_string();
    let excerpts = vec![ExcerptRecord {
        excerpt_id: "quote_0001".to_string(),
        artifact_id: artifact_id.clone(),
        quote: "web capture quote".to_string(),
        quote_sha256: sha256_prefixed("web capture quote".as_bytes()),
        locator: ExcerptLocator::ByteRange { start: 0, end: 17 },
        match_kind: ExcerptMatchKind::Exact,
        created_at: Utc::now(),
    }];
    let excerpts_text = excerpts_jsonl(&excerpts).unwrap();
    let manifest = WebCaptureManifest {
        schema_version: 2,
        artifact_id: artifact_id.clone(),
        kind: "web_capture".to_string(),
        original_url: "https://example.com/report".to_string(),
        final_url: "https://example.com/report".to_string(),
        redirect_chain: Vec::new(),
        captured_at: Utc::now(),
        capture_method: CaptureMethod::HttpStaticV1,
        request: CaptureRequestSnapshot::default(),
        response: CaptureResponseSnapshot { http_status: 200, ..CaptureResponseSnapshot::default() },
        raw_sha256: Some(sha256_prefixed(b"raw")),
        raw_zstd_sha256: None,
        raw_encrypted_sha256: None,
        raw_storage: RawStorage::OmittedPrivacy,
        raw_omitted_reason: Some("privacy".to_string()),
        extracted_text_storage: ExtractedTextStorage::Plaintext,
        encryption_envelope: None,
        extracted_text_sha256: Some(sha256_prefixed(extracted_text.as_bytes())),

        extracted_text_encrypted_sha256: None,
        excerpts_sha256: sha256_prefixed(excerpts_text.as_bytes()),
        raw_byte_len: 3,
        extracted_text_byte_len: Some(extracted_text.len()),

        extracted_text_encrypted_byte_len: None,
        capture_status: CaptureStatus::CompleteTextOnly,
        warnings: Vec::new(),
        merge_conflict: None,
    };
    store
        .write_web_capture(&WebCaptureArtifact {
            manifest,
            extracted_text,
            excerpts,
            raw_bytes: None,
            encrypted_extracted_bytes: None,
            encrypted_raw_bytes: None,
        })
        .expect("write artifact");
    format!("webcap:{artifact_id}#quote_0001")
}
