use chrono::Utc;
use memory_source::hash::sha256_prefixed;
use memory_source::storage::excerpts_jsonl;
use memory_source::{
    ArtifactStore, CaptureMethod, CaptureRequestSnapshot, CaptureResponseSnapshot, CaptureStatus, ExcerptLocator,
    ExcerptMatchKind, ExcerptRecord, ExtractedTextStorage, RawStorage, SourceArtifactId, WebCaptureArtifact,
    WebCaptureManifest,
};
use memory_substrate::{InitOptions, MemoryId, Roots, Substrate};
use memoryd::handlers::handle_request;
use memoryd::protocol::{
    GovernanceRefusalReason, GovernanceStatus, RequestEnvelope, RequestPayload, ResponsePayload, ResponseResult,
};

const TEST_PROJECT_CANONICAL_ID: &str = "proj_web_capture_e2e";
const TEST_PROJECT_ALIAS: &str = "web-capture-e2e";

#[tokio::test]
async fn governed_write_accepts_verified_webcap_ref() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp).await;
    let source_ref = write_artifact(&substrate, CaptureStatus::Complete);

    let write = write_web_memory(&substrate, "write-webcap", &source_ref).await;

    assert!(matches!(write.status, GovernanceStatus::Promoted | GovernanceStatus::Candidate));
    let id = write.id.expect("write persisted");
    let saved = substrate.read_memory(&MemoryId::new(&id)).await.expect("read memory");
    assert_eq!(saved.frontmatter.source.kind, memory_substrate::SourceKind::Web);
    assert_eq!(saved.frontmatter.source.reference.as_deref(), Some(source_ref.as_str()));
}

#[tokio::test]
async fn governed_write_refuses_naked_or_corrupt_webcap_ref() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp).await;
    let naked = write_web_memory(&substrate, "write-naked-url", "https://example.com/report").await;
    assert_eq!(naked.status, GovernanceStatus::Refused);
    assert_eq!(naked.reason, Some(GovernanceRefusalReason::Grounding));

    let source_ref = write_artifact(&substrate, CaptureStatus::Complete);
    let artifact_id =
        SourceArtifactId::try_new(source_ref.trim_start_matches("webcap:").split('#').next().unwrap()).unwrap();
    let artifact_path = ArtifactStore::new(substrate.roots().repo.clone()).find_artifact_path(&artifact_id).unwrap();
    std::fs::write(substrate.roots().repo.join(artifact_path.relative()).join("extracted.txt"), "tampered").unwrap();
    let corrupt = write_web_memory(&substrate, "write-corrupt-webcap", &source_ref).await;
    assert_eq!(corrupt.status, GovernanceStatus::Refused);
    assert_eq!(corrupt.reason, Some(GovernanceRefusalReason::Grounding));
}

#[tokio::test]
async fn governed_write_refuses_missing_excerpt_or_partial_capture() {
    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp).await;
    let source_ref = write_artifact(&substrate, CaptureStatus::Complete);
    let no_quote = source_ref.split('#').next().unwrap().to_string();
    let missing_excerpt = write_web_memory(&substrate, "write-missing-excerpt", &no_quote).await;
    assert_eq!(missing_excerpt.status, GovernanceStatus::Refused);
    assert_eq!(missing_excerpt.reason, Some(GovernanceRefusalReason::Grounding));

    let temp = tempfile::tempdir().expect("tempdir");
    let substrate = init_substrate(&temp).await;
    let partial_ref = write_partial_artifact(&substrate);
    let partial = write_web_memory(&substrate, "write-partial-webcap", &partial_ref).await;
    assert_eq!(partial.status, GovernanceStatus::Refused);
    assert_eq!(partial.reason, Some(GovernanceRefusalReason::Grounding));
}

async fn init_substrate(temp: &tempfile::TempDir) -> Substrate {
    Substrate::init(
        Roots::new(temp.path().join("repo"), temp.path().join("runtime")),
        InitOptions { force_unsafe_durability: true, device_id: Some("dev_webcaptest".to_owned()) },
    )
    .await
    .expect("init substrate")
}

async fn write_web_memory(
    substrate: &Substrate,
    request_id: &str,
    source_ref: &str,
) -> memoryd::protocol::GovernanceWriteResponse {
    let response = handle_request(
        substrate,
        RequestEnvelope::new(
            request_id,
            RequestPayload::WriteMemory {
                body: "The exact web quote supports this saved memory.".to_string(),
                title: Some("Web grounded memory".to_string()),
                tags: vec!["webcap".to_string()],
                meta: serde_json::json!({
                    "namespace": "project",
                    "type": "claim",
                    "summary": "Web grounded memory",
                    "canonical_namespace_id": TEST_PROJECT_CANONICAL_ID,
                    "namespace_alias": TEST_PROJECT_ALIAS,
                    "confidence": 0.95,
                    "sensitivity": "internal",
                    "source_kind": "web_capture",
                    "source_ref": source_ref,
                    "explicit_user_context": false
                }),
            },
        ),
    )
    .await;
    let ResponseResult::Success(ResponsePayload::GovernanceWrite(write)) = response.result else {
        panic!("expected governance write response, got {:?}", response.result);
    };
    write
}

fn write_artifact(substrate: &Substrate, status: CaptureStatus) -> String {
    let artifact = artifact_fixture(status);
    let artifact_id = artifact.manifest.artifact_id.clone();
    ArtifactStore::new(substrate.roots().repo.clone()).write_web_capture(&artifact).expect("write artifact");
    format!("webcap:{artifact_id}#quote_0001")
}

fn write_partial_artifact(substrate: &Substrate) -> String {
    let artifact = artifact_fixture(CaptureStatus::Partial);
    let store = ArtifactStore::new(substrate.roots().repo.clone());
    let path = store.source_artifact_path(&artifact.manifest.artifact_id, artifact.manifest.captured_at);
    let dir = substrate.roots().repo.join(path.relative());
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("manifest.json"), serde_json::to_vec(&artifact.manifest).unwrap()).unwrap();
    std::fs::write(dir.join("extracted.txt"), &artifact.extracted_text).unwrap();
    std::fs::write(dir.join("excerpts.jsonl"), excerpts_jsonl(&artifact.excerpts).unwrap()).unwrap();
    format!("webcap:{}#quote_0001", artifact.manifest.artifact_id)
}

fn artifact_fixture(status: CaptureStatus) -> WebCaptureArtifact {
    let artifact_id = SourceArtifactId::try_new("src_01J0Z7Y8Q9R0ABCDE123456789").unwrap();
    let extracted_text = "The exact web quote supports this saved memory.".to_string();
    let excerpts = vec![ExcerptRecord {
        excerpt_id: "quote_0001".to_string(),
        artifact_id: artifact_id.clone(),
        quote: "exact web quote".to_string(),
        quote_sha256: sha256_prefixed("exact web quote".as_bytes()),
        locator: ExcerptLocator::ByteRange { start: 4, end: 19 },
        match_kind: ExcerptMatchKind::Exact,
        created_at: Utc::now(),
    }];
    let excerpts_text = excerpts_jsonl(&excerpts).unwrap();
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
        capture_status: status,
        warnings: Vec::new(),
        merge_conflict: None,
    };
    WebCaptureArtifact {
        manifest,
        extracted_text,
        excerpts,
        raw_bytes: None,
        encrypted_extracted_bytes: None,
        encrypted_raw_bytes: None,
    }
}
