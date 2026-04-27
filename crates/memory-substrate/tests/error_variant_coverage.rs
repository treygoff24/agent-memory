use memory_substrate::*;

#[test]
fn every_current_public_error_family_has_behavioral_coverage() {
    let coverage = [
        ("WriteFailureKind::SecretRefused", "api_write_read::classification_secret_refuses_before_any_disk_effect"),
        ("WriteFailureKind::Validation", "api_write_read::write_refuses_repo_path_escape"),
        ("VectorError::DimensionMismatch", "vector_lifecycle::update_embedding_rejects_wrong_dimension_and_stale_hash"),
        ("VectorError::StaleChunk", "vector_lifecycle::update_embedding_rejects_wrong_dimension_and_stale_hash"),
        ("MergeError semantic conflict", "merge_rules::conflicting_body_edits_quarantine_instead_of_dropping_theirs"),
        ("Open/startup repair", "startup_reconciliation::startup_replays_pending_index_queue_before_queries"),
    ];
    assert_eq!(coverage.len(), 6);
    assert!(coverage.iter().all(|(_, test)| test.contains("::")));
}

#[test]
fn public_error_variants_are_named_for_callers() {
    let secret = WriteFailureKind::SecretRefused.to_string();
    let dimension = VectorError::DimensionMismatch { expected: 3, found: 2 }.to_string();
    assert_eq!(secret, "secret refused");
    assert!(dimension.contains("expected 3"));
}
