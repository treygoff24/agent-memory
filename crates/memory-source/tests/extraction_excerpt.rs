use chrono::Utc;
use memory_source::excerpt::create_excerpt_records;
use memory_source::extract::extract_text;
use memory_source::SourceArtifactId;

#[test]
fn html_extraction_removes_nonvisible_content_and_normalizes() {
    let html = br#"<html><head><style>.x{}</style><script>secret()</script></head><body><h1> Title </h1><template>hidden</template><p>Visible   text</p><noscript>no</noscript><span hidden>hide</span></body></html>"#;
    let extracted = extract_text(Some("text/html; charset=utf-8"), html).unwrap();
    assert!(extracted.text.contains("Title Visible text"), "{}", extracted.text);
    assert!(!extracted.text.contains("secret"));
    assert!(!extracted.text.contains("hidden"));
}

#[test]
fn plain_text_and_invalid_bytes_are_decoded_deterministically() {
    let extracted = extract_text(Some("text/plain; charset=utf-8"), b"hello\xffworld").unwrap();
    assert!(extracted.text.contains("hello"));
    assert!(extracted.warnings.contains(&"decode_replacement_characters".to_string()));
}

#[test]
fn unsupported_content_type_is_explicit() {
    let extracted = extract_text(Some("application/pdf"), b"%PDF").unwrap();
    assert!(!extracted.is_supported());
}

#[test]
fn exact_excerpt_anchoring_records_byte_range() {
    let artifact_id = SourceArtifactId::try_new("src_01J0Z7Y8Q9R0ABCDE123456789").unwrap();
    let records =
        create_excerpt_records(&artifact_id, "alpha exact quote omega", &["exact quote".to_string()], Utc::now())
            .unwrap();
    assert_eq!(records[0].excerpt_id, "quote_0001");
    assert!(serde_json::to_string(&records[0].locator).unwrap().contains("byte_range"));
    assert!(create_excerpt_records(&artifact_id, "alpha", &["missing".to_string()], Utc::now()).is_err());
    assert!(
        create_excerpt_records(&artifact_id, "SSN 123-45-6789", &["SSN 123-45-6789".to_string()], Utc::now()).is_err()
    );
}
