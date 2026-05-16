use chrono::Utc;
use memory_source::hash::sha256_prefixed;
use memory_source::storage::excerpts_jsonl;
use memory_source::{
    ArtifactStore, CaptureMethod, CaptureRequestSnapshot, CaptureResponseSnapshot, CaptureStatus, ExcerptLocator,
    ExcerptMatchKind, ExcerptRecord, RawStorage, SourceArtifactId, WebCaptureArtifact, WebCaptureManifest,
};
use memory_substrate::merge::{merge_markdown, MergeInput, MergeResult};

#[test]
fn source_manifest_clean_fastpaths_and_divergent_quarantines() {
    let base = manifest_json(CaptureStatus::Complete, &[]);
    let ours = manifest_json(CaptureStatus::Complete, &["ours"]);
    assert_eq!(
        merge("sources/web/2026/05/src_01J0Z7Y8Q9R0ABCDE123456789/manifest.json", &base, &ours, &base),
        MergeResult::Clean(ours.clone())
    );

    let theirs = manifest_json(CaptureStatus::Complete, &["theirs"]);
    let result = merge("sources/web/2026/05/src_01J0Z7Y8Q9R0ABCDE123456789/manifest.json", &base, &ours, &theirs);
    let MergeResult::Quarantine(text) = result else { panic!("expected quarantine") };
    assert!(text.contains("\"capture_status\": \"partial\""));
    assert!(text.contains("source_artifact_merge_conflict"));
}

#[test]
fn source_excerpts_unique_concat_and_conflict_quarantine() {
    let a = excerpt_jsonl("quote_0001", "quote one", 0);
    let b = excerpt_jsonl("quote_0002", "quote two", 0);
    let result = merge("sources/web/2026/05/src_01J0Z7Y8Q9R0ABCDE123456789/excerpts.jsonl", "", &a, &b);
    let MergeResult::Clean(text) = result else { panic!("expected clean") };
    assert!(text.contains("quote_0001"));
    assert!(text.contains("quote_0002"));

    let conflict = excerpt_jsonl("quote_0001", "different", 0);
    let result = merge("sources/web/2026/05/src_01J0Z7Y8Q9R0ABCDE123456789/excerpts.jsonl", "", &a, &conflict);
    let MergeResult::Quarantine(text) = result else { panic!("expected quarantine") };
    assert!(text.contains("merge_conflict"));
}

#[test]
fn divergent_extracted_text_quarantines_without_source_text() {
    let base = "base sensitive long body";
    let ours = "ours sensitive long body";
    let theirs = "theirs sensitive long body";
    let result = merge("sources/web/2026/05/src_01J0Z7Y8Q9R0ABCDE123456789/extracted.txt", base, ours, theirs);
    let MergeResult::Quarantine(text) = result else { panic!("expected quarantine") };
    assert!(text.contains("source_artifact_merge_conflict"));
    for source_body in [base, ours, theirs] {
        assert!(!text.contains(source_body), "quarantine text leaked source body: {source_body}");
    }
}

#[test]
fn quarantined_outputs_do_not_verify_as_groundable() {
    let temp = tempfile::tempdir().unwrap();
    let store = ArtifactStore::new(temp.path());
    let artifact = artifact_fixture();
    let path = store.write_web_capture(&artifact).unwrap();
    let manifest_path = temp.path().join(path.relative()).join("manifest.json");
    let base = std::fs::read_to_string(&manifest_path).unwrap();
    let ours = base.replace("\"warnings\": []", "\"warnings\": [\"ours\"]");
    let theirs = base.replace("\"warnings\": []", "\"warnings\": [\"theirs\"]");
    let MergeResult::Quarantine(text) =
        merge(path.relative().join("manifest.json").to_string_lossy().as_ref(), &base, &ours, &theirs)
    else {
        panic!("expected quarantine")
    };
    std::fs::write(manifest_path, text).unwrap();
    assert!(store.verify_web_capture(&path).is_err());
}

fn merge(path: &str, base: &str, ours: &str, theirs: &str) -> MergeResult {
    merge_markdown(MergeInput { base, ours, theirs, path }).expect("merge")
}

fn artifact_id() -> SourceArtifactId {
    SourceArtifactId::try_new("src_01J0Z7Y8Q9R0ABCDE123456789").unwrap()
}

fn artifact_fixture() -> WebCaptureArtifact {
    let artifact_id = artifact_id();
    let captured_at = chrono::DateTime::parse_from_rfc3339("2026-05-01T12:00:00Z").unwrap().with_timezone(&Utc);
    let extracted_text = "quote one".to_string();
    let excerpts = vec![ExcerptRecord {
        excerpt_id: "quote_0001".to_string(),
        artifact_id: artifact_id.clone(),
        quote: "quote one".to_string(),
        quote_sha256: sha256_prefixed("quote one".as_bytes()),
        locator: ExcerptLocator::ByteRange { start: 0, end: 9 },
        match_kind: ExcerptMatchKind::Exact,
        created_at: captured_at,
    }];
    let raw = b"quote one".to_vec();
    let raw_zstd = zstd::encode_all(raw.as_slice(), 0).unwrap();
    let excerpts_text = excerpts_jsonl(&excerpts).unwrap();
    let manifest = WebCaptureManifest {
        schema_version: 1,
        artifact_id,
        kind: "web_capture".to_string(),
        original_url: "https://example.com".to_string(),
        final_url: "https://example.com".to_string(),
        redirect_chain: Vec::new(),
        captured_at,
        capture_method: CaptureMethod::HttpStaticV1,
        request: CaptureRequestSnapshot::default(),
        response: CaptureResponseSnapshot { http_status: 200, ..CaptureResponseSnapshot::default() },
        raw_sha256: Some(sha256_prefixed(&raw)),
        raw_zstd_sha256: Some(sha256_prefixed(&raw_zstd)),
        raw_storage: RawStorage::Stored,
        raw_omitted_reason: None,
        extracted_text_sha256: sha256_prefixed(extracted_text.as_bytes()),
        excerpts_sha256: sha256_prefixed(excerpts_text.as_bytes()),
        raw_byte_len: raw.len(),
        extracted_text_byte_len: extracted_text.len(),
        capture_status: CaptureStatus::Complete,
        warnings: Vec::new(),
        merge_conflict: None,
    };
    WebCaptureArtifact { manifest, extracted_text, excerpts, raw_bytes: Some(raw) }
}

fn manifest_json(status: CaptureStatus, warnings: &[&str]) -> String {
    let mut artifact = artifact_fixture();
    artifact.manifest.capture_status = status;
    artifact.manifest.warnings = warnings.iter().map(|warning| (*warning).to_string()).collect();
    let mut text = serde_json::to_string_pretty(&artifact.manifest).unwrap();
    text.push('\n');
    text
}

fn excerpt_jsonl(excerpt_id: &str, quote: &str, start: usize) -> String {
    let record = ExcerptRecord {
        excerpt_id: excerpt_id.to_string(),
        artifact_id: artifact_id(),
        quote: quote.to_string(),
        quote_sha256: sha256_prefixed(quote.as_bytes()),
        locator: ExcerptLocator::ByteRange { start, end: start + quote.len() },
        match_kind: ExcerptMatchKind::Exact,
        created_at: Utc::now(),
    };
    format!("{}\n", serde_json::to_string(&record).unwrap())
}
