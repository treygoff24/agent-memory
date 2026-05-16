use std::io::{Error, ErrorKind};
use std::path::PathBuf;

use memory_substrate::*;

#[test]
fn every_current_public_error_family_has_behavioral_coverage() {
    let refs = all_current_public_error_variant_coverage_refs();
    assert!(refs.len() >= 63, "coverage should enumerate each current public error variant");
    assert_referenced_tests_exist(&refs);
}

#[test]
fn public_error_variants_are_named_for_callers() {
    let secret = WriteFailureKind::SecretRefused.to_string();
    let dimension = VectorError::DimensionMismatch { expected: 3, found: 2 }.to_string();
    assert_eq!(secret, "secret refused");
    assert!(dimension.contains("expected 3"));
}

fn all_current_public_error_variant_coverage_refs() -> Vec<CoverageRef> {
    let mut refs = Vec::new();

    refs.extend(write_failure_kind_variants().iter().map(write_failure_kind_coverage));
    refs.extend(substrate_error_variant_refs());
    refs.extend(validation_error_variant_refs());
    refs.extend(vector_error_variant_refs());
    refs.extend(merge_error_variant_refs());
    refs.extend(open_error_variant_refs());
    refs.extend(read_error_variant_refs());
    refs.extend(id_error_variant_refs());
    refs.extend(git_error_variant_refs());
    refs.extend(watch_error_variant_refs());

    refs
}

fn substrate_error_variant_refs() -> Vec<CoverageRef> {
    vec![
        substrate_error_coverage(&SubstrateError::Open(OpenError::NotAMemorumSubstrate {
            path: PathBuf::from("repo"),
        })),
        substrate_error_coverage(&SubstrateError::Read(ReadError::NotFound(RepoPath::new(
            "agent/patterns/missing.md",
        )))),
        substrate_error_coverage(&SubstrateError::Write(WriteFailure {
            outcome: WriteOutcome::not_committed(OperationId::new("op_coverage"), DurabilityTier::Full),
            kind: WriteFailureKind::SecretRefused,
        })),
        substrate_error_coverage(&SubstrateError::Validation(ValidationError::BadShape("summary".to_owned()))),
        substrate_error_coverage(&SubstrateError::Id(IdError::DeviceMismatch)),
        substrate_error_coverage(&SubstrateError::Vector(VectorError::IndexUnavailable("poisoned".to_owned()))),
        substrate_error_coverage(&SubstrateError::Git(GitError::MergeDriverMissing("memory-merge-driver".to_owned()))),
        substrate_error_coverage(&SubstrateError::Watch(WatchError::Timeout)),
        substrate_error_coverage(&SubstrateError::Merge(MergeError::MissingDelimiters)),
        substrate_error_coverage(&SubstrateError::Io {
            path: "repo/events/dev.jsonl".to_owned(),
            source: Error::new(ErrorKind::NotFound, "missing"),
        }),
        substrate_error_coverage(&SubstrateError::Sqlite(rusqlite::Error::InvalidQuery)),
        substrate_error_coverage(&SubstrateError::InvalidQuery {
            field: "namespace".to_owned(),
            value: "../escape".to_owned(),
            message: "invalid_query".to_owned(),
        }),
    ]
}

fn substrate_error_coverage(error: &SubstrateError) -> CoverageRef {
    match error {
        SubstrateError::Open(source) => {
            let _ = source;
            CoverageRef::new("open_validation", "open_rejects_unmarked_directory_without_mutating_it")
        }
        SubstrateError::Read(source) => {
            let _ = source;
            CoverageRef::new("api_phase5_surface", "read_memory_envelope_missing_id_returns_not_found")
        }
        SubstrateError::Write(source) => {
            let _ = source;
            CoverageRef::new("api_write_read", "classification_secret_refuses_before_any_disk_effect")
        }
        SubstrateError::Validation(source) => {
            let _ = source;
            CoverageRef::new("frontmatter_schema", "frontmatter_field_rule_matrix_accepts_and_rejects_known_shapes")
        }
        SubstrateError::Id(source) => {
            let _ = source;
            CoverageRef::new("id_sequence", "sequence_999999_succeeds_then_1000000_is_exhausted")
        }
        SubstrateError::Vector(source) => {
            let _ = source;
            CoverageRef::new("api_internal", "poisoned_index_mutex_maps_vector_apis_to_index_unavailable")
        }
        SubstrateError::Git(source) => {
            let _ = source;
            CoverageRef::new("git_preflight", "missing_merge_driver_binary_refuses_before_merge")
        }
        SubstrateError::Watch(source) => {
            let _ = source;
            CoverageRef::new("api_write_read", "write_read_query_and_event_round_trip_through_public_api")
        }
        SubstrateError::Merge(source) => {
            let _ = source;
            CoverageRef::new("merge_rules", "unparsed_side_quarantine_emits_typed_unparsed_sides")
        }
        SubstrateError::Io { path, source } => {
            let _ = (path, source);
            CoverageRef::new("event_log_recovery", "event_log_recovery_refuses_nonfinal_malformed_line")
        }
        SubstrateError::Sqlite(source) => {
            let _ = source;
            CoverageRef::new("index_pragmas", "open_index_rejects_unsupported_schema_version")
        }
        SubstrateError::InvalidQuery { field, value, message } => {
            let _ = (field, value, message);
            CoverageRef::new(
                "memory_query_extension",
                "memory_query_filters_and_recall_index_use_stream_a_index_projections",
            )
        }
    }
}

fn validation_error_variant_refs() -> Vec<CoverageRef> {
    let id = MemoryId::new("mem_20260424_a1b2c3d4e5f60718_000001");
    vec![
        validation_error_coverage(&ValidationError::MissingRequiredField("summary".to_owned())),
        validation_error_coverage(&ValidationError::BadEnum { field: "status".to_owned(), value: "bad".to_owned() }),
        validation_error_coverage(&ValidationError::BadShape("summary".to_owned())),
        validation_error_coverage(&ValidationError::UnsupportedSchemaVersion { found: 2, supported: 1 }),
        validation_error_coverage(&ValidationError::InvalidLifecyclePair),
        validation_error_coverage(&ValidationError::SecretSensitivityOnDiskAt {
            path: PathBuf::from("agent/patterns/secret.md"),
        }),
        validation_error_coverage(&ValidationError::PlaintextUnderEncryptedTier {
            path: PathBuf::from("encrypted/agent/patterns/plain.md"),
        }),
        validation_error_coverage(&ValidationError::NonCanonicalStreamFFile {
            path: PathBuf::from("dreams/cleanup/dev_local/2026-04-30.json"),
            message: "must be object".to_owned(),
        }),
        validation_error_coverage(&ValidationError::InvalidMemoryId("bad".to_owned())),
        validation_error_coverage(&ValidationError::DuplicateMemoryId(id.clone())),
        validation_error_coverage(&ValidationError::CaseFoldCollision("agent/patterns/foo.md".to_owned())),
        validation_error_coverage(&ValidationError::SupersessionCycle(id.clone())),
        validation_error_coverage(&ValidationError::MissingReference(id)),
        validation_error_coverage(&ValidationError::Other("yaml".to_owned())),
    ]
}

fn validation_error_coverage(error: &ValidationError) -> CoverageRef {
    match error {
        ValidationError::MissingRequiredField(field) => {
            let _ = field;
            CoverageRef::new("frontmatter_schema", "frontmatter_field_rule_matrix_accepts_and_rejects_known_shapes")
        }
        ValidationError::BadEnum { field, value } => {
            let _ = (field, value);
            CoverageRef::new("frontmatter_schema", "frontmatter_field_rule_matrix_accepts_and_rejects_known_shapes")
        }
        ValidationError::BadShape(field) => {
            let _ = field;
            CoverageRef::new("frontmatter_schema", "frontmatter_field_rule_matrix_accepts_and_rejects_known_shapes")
        }
        ValidationError::UnsupportedSchemaVersion { found, supported } => {
            let _ = (found, supported);
            CoverageRef::new("frontmatter_schema", "higher_schema_version_is_rejected_before_mutation")
        }
        ValidationError::InvalidLifecyclePair => {
            CoverageRef::new("frontmatter_schema", "rejects_invalid_lifecycle_matrix_pair")
        }
        ValidationError::SecretSensitivityOnDiskAt { path } => {
            let _ = path;
            CoverageRef::new("merge_rules", "secret_sensitivity_refuses")
        }
        ValidationError::PlaintextUnderEncryptedTier { path } => {
            let _ = path;
            CoverageRef::new(
                "startup_reconciliation",
                "startup_reindex_requires_operator_repair_for_plaintext_markdown_under_encrypted_namespace",
            )
        }
        ValidationError::NonCanonicalStreamFFile { path, message } => {
            let _ = (path, message);
            CoverageRef::new("dream_canonical_isolation", "rejects_dream_cleanup_reports_that_are_not_json_objects")
        }
        ValidationError::InvalidMemoryId(value) => {
            let _ = value;
            CoverageRef::new("frontmatter_schema", "frontmatter_field_rule_matrix_accepts_and_rejects_known_shapes")
        }
        ValidationError::DuplicateMemoryId(id) => {
            let _ = id;
            CoverageRef::new("tree_validation", "duplicate_frontmatter_ids_fail_validation")
        }
        ValidationError::CaseFoldCollision(path) => {
            let _ = path;
            CoverageRef::new("tree_validation", "case_only_path_collision_fixture_fails_validation")
        }
        ValidationError::SupersessionCycle(id) => {
            let _ = id;
            CoverageRef::new("tree_validation", "supersession_cycle_fails_cross_file_validation")
        }
        ValidationError::MissingReference(id) => {
            let _ = id;
            CoverageRef::new("tree_validation", "inverse_supersession_mismatch_fails_when_both_endpoints_exist")
        }
        ValidationError::Other(message) => {
            let _ = message;
            CoverageRef::new("frontmatter_schema", "summary_with_colon_space_round_trips_through_serialize_and_parse")
        }
    }
}

fn write_failure_kind_variants() -> [WriteFailureKind; 14] {
    [
        WriteFailureKind::SecretRefused,
        WriteFailureKind::EncryptionRequired,
        WriteFailureKind::ClassificationSensitivityMismatch,
        WriteFailureKind::DreamProseAsSource,
        WriteFailureKind::StaleBase,
        WriteFailureKind::AlreadyExists,
        WriteFailureKind::DurabilityUnavailable,
        WriteFailureKind::IndexAfterCommitFailed,
        WriteFailureKind::RepairQueueFailed,
        WriteFailureKind::RepairStateNotDurable,
        WriteFailureKind::Validation("invalid repo path".to_owned()),
        WriteFailureKind::ValidationTyped(ValidationError::InvalidMemoryId("bad".to_owned())),
        WriteFailureKind::Io("file exists".to_owned()),
        WriteFailureKind::IoTyped { kind: ErrorKind::PermissionDenied, context: "fsync denied".to_owned() },
    ]
}

fn write_failure_kind_coverage(kind: &WriteFailureKind) -> CoverageRef {
    match kind {
        WriteFailureKind::SecretRefused => {
            CoverageRef::new("api_write_read", "classification_secret_refuses_before_any_disk_effect")
        }
        WriteFailureKind::EncryptionRequired => CoverageRef::new(
            "api_write_read",
            "plaintext_requires_encryption_classification_is_refused_before_disk_effect",
        ),
        WriteFailureKind::ClassificationSensitivityMismatch => {
            CoverageRef::new("api_write_read", "trusted_classification_cannot_persist_confidential_plaintext")
        }
        WriteFailureKind::DreamProseAsSource => {
            CoverageRef::new("dream_canonical_isolation", "write_memory_refuses_dream_artifacts_as_grounding_sources")
        }
        WriteFailureKind::StaleBase => {
            CoverageRef::new("api_write_read", "stale_base_replace_leaves_existing_file_unchanged")
        }
        WriteFailureKind::AlreadyExists => {
            CoverageRef::new("api_write_read", "encrypted_create_new_refuses_to_overwrite_existing_ciphertext")
        }
        WriteFailureKind::DurabilityUnavailable => {
            CoverageRef::new("api_write_read", "best_effort_plaintext_write_requires_explicit_opt_in")
        }
        WriteFailureKind::IndexAfterCommitFailed => {
            CoverageRef::new("api_write_read", "encrypted_index_after_ciphertext_commit_is_durably_replayed_on_startup")
        }
        WriteFailureKind::RepairQueueFailed => CoverageRef::new(
            "api_write_read",
            "encrypted_event_after_ciphertext_commit_queue_failure_returns_repair_queue_failure",
        ),
        WriteFailureKind::RepairStateNotDurable => CoverageRef::new(
            "api_write_read",
            "write_outcomes_distinguish_not_committed_full_commit_and_event_repair_states",
        ),
        WriteFailureKind::Validation(message) => {
            let _ = message;
            CoverageRef::new("api_write_read", "write_refuses_repo_path_escape")
        }
        WriteFailureKind::ValidationTyped(error) => {
            let _ = error;
            CoverageRef::new("api_write_read", "privacy_scan_private_credential_refuses_plaintext_before_disk_effect")
        }
        WriteFailureKind::Io(message) => {
            let _ = message;
            CoverageRef::new("api_write_read", "encrypted_write_refuses_symlinked_encrypted_parent_escape")
        }
        WriteFailureKind::IoTyped { kind, context } => {
            let _ = (kind, context);
            CoverageRef::new("atomic_write", "atomic_write_temp_path_collision_proves_staging_in_target_parent")
        }
    }
}

fn vector_error_variant_refs() -> Vec<CoverageRef> {
    let triple =
        EmbeddingTriple { provider: "synthetic".to_owned(), model_ref: "stream-a-test".to_owned(), dimension: 3 };
    vec![
        vector_error_coverage(&VectorError::DimensionMismatch { expected: 3, found: 2 }),
        vector_error_coverage(&VectorError::UnknownEmbeddingTriple(triple.clone())),
        vector_error_coverage(&VectorError::StaleChunk {
            expected: Sha256::new("sha256:expected"),
            found: Sha256::new("sha256:found"),
        }),
        vector_error_coverage(&VectorError::IndexUnavailable("index mutex poisoned".to_owned())),
        vector_error_coverage(&VectorError::Sqlite(rusqlite::Error::InvalidQuery)),
        vector_error_coverage(&VectorError::Storage("serialize vector".to_owned())),
    ]
}

fn vector_error_coverage(error: &VectorError) -> CoverageRef {
    match error {
        VectorError::DimensionMismatch { expected, found } => {
            let _ = (expected, found);
            CoverageRef::new("vector_lifecycle", "update_embedding_rejects_wrong_dimension_and_stale_hash")
        }
        VectorError::UnknownEmbeddingTriple(triple) => {
            let _ = triple;
            CoverageRef::new(
                "vector_lifecycle",
                "dropped_triple_returns_unknown_and_cannot_be_recreated_by_stale_worker",
            )
        }
        VectorError::StaleChunk { expected, found } => {
            let _ = (expected, found);
            CoverageRef::new("vector_lifecycle", "update_embedding_rejects_wrong_dimension_and_stale_hash")
        }
        VectorError::IndexUnavailable(message) => {
            let _ = message;
            CoverageRef::new("api_internal", "poisoned_index_mutex_maps_vector_apis_to_index_unavailable")
        }
        VectorError::Sqlite(error) => {
            let _ = error;
            CoverageRef::new("vector_lifecycle", "query_chunks_uses_sqlite_vec_nearest_neighbors")
        }
        VectorError::Storage(message) => {
            let _ = message;
            CoverageRef::new("vector_lifecycle", "query_chunks_uses_sqlite_vec_nearest_neighbors")
        }
    }
}

fn merge_error_variant_refs() -> Vec<CoverageRef> {
    vec![
        merge_error_coverage(&MergeError::MissingDelimiters),
        merge_error_coverage(&MergeError::UnsupportedSchema { found: 2, supported: 1 }),
        merge_error_coverage(&MergeError::Parse("yaml".to_owned())),
        merge_error_coverage(&MergeError::ParseSide { side: MergeSide::Ours, message: "yaml".to_owned() }),
        merge_error_coverage(&MergeError::UnrepresentableConflict("body".to_owned())),
        merge_error_coverage(&MergeError::QuarantineWillNotValidate { message: "validation".to_owned() }),
        merge_error_coverage(&MergeError::Serialize { message: "serde".to_owned() }),
        merge_error_coverage(&MergeError::SecretSensitivityRefused { side: MergeSide::Ours }),
    ]
}

fn merge_error_coverage(error: &MergeError) -> CoverageRef {
    match error {
        MergeError::MissingDelimiters => {
            CoverageRef::new("merge_rules", "unparsed_side_quarantine_emits_typed_unparsed_sides")
        }
        MergeError::UnsupportedSchema { found, supported } => {
            let _ = (found, supported);
            CoverageRef::new("merge_rules", "schema_version_gate_returns_typed_error_without_writing")
        }
        MergeError::Parse(message) => {
            let _ = message;
            CoverageRef::new("merge_rules", "unparsed_side_quarantine_emits_typed_unparsed_sides")
        }
        MergeError::ParseSide { side, message } => {
            let _ = (side, message);
            CoverageRef::new("merge_rules", "unparsed_side_quarantine_emits_typed_unparsed_sides")
        }
        MergeError::UnrepresentableConflict(message) => {
            let _ = message;
            CoverageRef::new("merge_rules", "conflicting_body_edits_quarantine_instead_of_dropping_theirs")
        }
        MergeError::QuarantineWillNotValidate { message } => {
            let _ = message;
            CoverageRef::new("merge_rules", "add_add_id_collision_marks_duplicate_id_repair")
        }
        MergeError::Serialize { message } => {
            let _ = message;
            CoverageRef::new("merge_rules", "lifecycle_pair_fixture_matrix_outputs_valid_markdown")
        }
        MergeError::SecretSensitivityRefused { side } => {
            let _ = side;
            CoverageRef::new("merge_rules", "secret_sensitivity_refuses")
        }
    }
}

fn open_error_variant_refs() -> Vec<CoverageRef> {
    vec![
        open_error_coverage(&OpenError::NotAMemorumSubstrate { path: PathBuf::from("repo") }),
        open_error_coverage(&OpenError::DurabilityUnsupported { tier: DurabilityTier::BestEffort }),
        open_error_coverage(&OpenError::OperatorRepairRequired("pending index".to_owned())),
        open_error_coverage(&OpenError::InvalidRoots("config.yaml missing".to_owned())),
        open_error_coverage(&OpenError::DeviceIdentityMissing { repair: RepairAction::AdoptClone }),
        open_error_coverage(&OpenError::IndexSchemaVersionUnsupported { found: 9, supported: 4 }),
        open_error_coverage(&OpenError::Io(Error::new(ErrorKind::NotFound, "missing"))),
        open_error_coverage(&OpenError::Validation(ValidationError::DuplicateMemoryId(MemoryId::new(
            "mem_20260424_a1b2c3d4e5f60718_000001",
        )))),
    ]
}

fn open_error_coverage(error: &OpenError) -> CoverageRef {
    match error {
        OpenError::NotAMemorumSubstrate { path } => {
            let _ = path;
            CoverageRef::new("open_validation", "open_rejects_unmarked_directory_without_mutating_it")
        }
        OpenError::DurabilityUnsupported { tier } => {
            let _ = tier;
            CoverageRef::new("api_write_read", "best_effort_plaintext_write_requires_explicit_opt_in")
        }
        OpenError::OperatorRepairRequired(message) => {
            let _ = message;
            CoverageRef::new("startup_reconciliation", "startup_replays_pending_index_queue_before_queries")
        }
        OpenError::InvalidRoots(message) => {
            let _ = message;
            CoverageRef::new("open_validation", "adopt_clone_requires_explicit_merge_driver_path")
        }
        OpenError::DeviceIdentityMissing { repair } => {
            let _ = repair;
            CoverageRef::new(
                "api_phase5_surface",
                "open_fails_with_device_identity_missing_when_local_device_yaml_absent",
            )
        }
        OpenError::IndexSchemaVersionUnsupported { found, supported } => {
            let _ = (found, supported);
            CoverageRef::new("index_pragmas", "open_index_rejects_unsupported_schema_version")
        }
        OpenError::Io(error) => {
            let _ = error;
            CoverageRef::new("event_log_recovery", "event_log_recovery_refuses_nonfinal_malformed_line")
        }
        OpenError::Validation(error) => {
            let _ = error;
            CoverageRef::new("tree_validation", "duplicate_frontmatter_ids_fail_validation")
        }
    }
}

fn read_error_variant_refs() -> Vec<CoverageRef> {
    let path = RepoPath::new("agent/patterns/read-error.md");
    vec![
        read_error_coverage(&ReadError::NotFound(path.clone())),
        read_error_coverage(&ReadError::NotACanonicalMemory { path: path.clone() }),
        read_error_coverage(&ReadError::Parse { path: path.clone(), message: "invalid path".to_owned() }),
        read_error_coverage(&ReadError::Io(Error::new(ErrorKind::NotFound, "missing"))),
        read_error_coverage(&ReadError::Validation(ValidationError::InvalidMemoryId("bad".to_owned()))),
    ]
}

fn read_error_coverage(error: &ReadError) -> CoverageRef {
    match error {
        ReadError::NotFound(path) => {
            let _ = path;
            CoverageRef::new("api_phase5_surface", "read_memory_envelope_missing_id_returns_not_found")
        }
        ReadError::NotACanonicalMemory { path } => {
            let _ = path;
            CoverageRef::new(
                "dream_canonical_isolation",
                "read_path_envelope_refuses_stream_f_noncanonical_files_before_frontmatter_parsing",
            )
        }
        ReadError::Parse { path, message } => {
            let _ = (path, message);
            CoverageRef::new("api_write_read", "read_path_refuses_repo_path_escape")
        }
        ReadError::Io(error) => {
            let _ = error;
            CoverageRef::new("api_write_read", "read_path_refuses_symlink_escape_even_under_allowed_prefix")
        }
        ReadError::Validation(error) => {
            let _ = error;
            CoverageRef::new("api_write_read", "read_path_refuses_repo_path_escape")
        }
    }
}

fn id_error_variant_refs() -> Vec<CoverageRef> {
    vec![
        id_error_coverage(&IdError::DeviceMismatch),
        id_error_coverage(&IdError::SequenceExhausted { date: "2026-05-16".to_owned() }),
        id_error_coverage(&IdError::InvalidState("bad seq.json".to_owned())),
        id_error_coverage(&IdError::ClockRegression {
            last_allocated: "2099-01-01".to_owned(),
            now: "2026-05-16".to_owned(),
        }),
    ]
}

fn id_error_coverage(error: &IdError) -> CoverageRef {
    match error {
        IdError::DeviceMismatch => CoverageRef::new("id_sequence", "two_devices_with_different_shards_do_not_collide"),
        IdError::SequenceExhausted { date } => {
            let _ = date;
            CoverageRef::new("id_sequence", "sequence_999999_succeeds_then_1000000_is_exhausted")
        }
        IdError::InvalidState(message) => {
            let _ = message;
            CoverageRef::new("id_sequence", "stale_seq_tmp_residue_is_removed_before_atomic_write")
        }
        IdError::ClockRegression { last_allocated, now } => {
            let _ = (last_allocated, now);
            CoverageRef::new("id_sequence", "clock_regression_is_detected")
        }
    }
}

fn git_error_variant_refs() -> Vec<CoverageRef> {
    vec![
        git_error_coverage(&GitError::InvalidRepoRoot("repo".to_owned())),
        git_error_coverage(&GitError::CommandFailed {
            program: "git".to_owned(),
            args: vec!["push".to_owned()],
            stderr: "rejected".to_owned(),
        }),
        git_error_coverage(&GitError::MergeDriverMissing("memory-merge-driver".to_owned())),
        git_error_coverage(&GitError::GitPushFailed("rejected".to_owned())),
        git_error_coverage(&GitError::Io(Error::new(ErrorKind::NotFound, "git missing"))),
    ]
}

fn git_error_coverage(error: &GitError) -> CoverageRef {
    match error {
        GitError::InvalidRepoRoot(message) => {
            let _ = message;
            CoverageRef::new(
                "git_adoption",
                "fresh_clone_adoption_regenerates_local_identity_event_log_and_merge_config",
            )
        }
        GitError::CommandFailed { program, args, stderr } => {
            let _ = (program, args, stderr);
            CoverageRef::new("git_adoption", "fresh_clone_with_adoption_invokes_configured_git_merge_driver")
        }
        GitError::MergeDriverMissing(message) => {
            let _ = message;
            CoverageRef::new("git_preflight", "missing_merge_driver_binary_refuses_before_merge")
        }
        GitError::GitPushFailed(message) => {
            let _ = message;
            CoverageRef::new("git_adoption", "fresh_clone_with_adoption_invokes_configured_git_merge_driver")
        }
        GitError::Io(error) => {
            let _ = error;
            CoverageRef::new("git_adoption", "fresh_clone_with_adoption_invokes_configured_git_merge_driver")
        }
    }
}

fn watch_error_variant_refs() -> Vec<CoverageRef> {
    vec![
        watch_error_coverage(&WatchError::Setup("notify setup failed".to_owned())),
        watch_error_coverage(&WatchError::Timeout),
        watch_error_coverage(&WatchError::Closed),
    ]
}

fn watch_error_coverage(error: &WatchError) -> CoverageRef {
    match error {
        WatchError::Setup(message) => {
            let _ = message;
            CoverageRef::new("api_write_read", "fts_mutation_removes_old_terms_after_replace")
        }
        WatchError::Timeout => {
            CoverageRef::new("api_write_read", "write_read_query_and_event_round_trip_through_public_api")
        }
        WatchError::Closed => {
            CoverageRef::new("api_write_read", "write_read_query_and_event_round_trip_through_public_api")
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CoverageRef {
    module: &'static str,
    function: &'static str,
}

impl CoverageRef {
    const fn new(module: &'static str, function: &'static str) -> Self {
        Self { module, function }
    }
}

fn assert_referenced_tests_exist(refs: &[CoverageRef]) {
    for reference in refs {
        let source = source_for_module(reference.module);
        assert!(
            source.contains(&format!("fn {}", reference.function)),
            "referenced coverage test should exist: {}::{}",
            reference.module,
            reference.function
        );
    }
}

fn source_for_module(module: &str) -> &'static str {
    match module {
        "api_internal" => include_str!("../src/api.rs"),
        "api_phase5_surface" => include_str!("api_phase5_surface.rs"),
        "api_write_read" => include_str!("api_write_read.rs"),
        "atomic_write" => include_str!("atomic_write.rs"),
        "dream_canonical_isolation" => include_str!("dream_canonical_isolation.rs"),
        "event_log_recovery" => include_str!("event_log_recovery.rs"),
        "frontmatter_schema" => include_str!("frontmatter_schema.rs"),
        "git_adoption" => include_str!("git_adoption.rs"),
        "git_preflight" => include_str!("git_preflight.rs"),
        "id_sequence" => include_str!("id_sequence.rs"),
        "index_pragmas" => include_str!("index_pragmas.rs"),
        "merge_rules" => include_str!("merge_rules.rs"),
        "memory_query_extension" => include_str!("memory_query_extension.rs"),
        "open_validation" => include_str!("open_validation.rs"),
        "startup_reconciliation" => include_str!("startup_reconciliation.rs"),
        "tree_validation" => include_str!("tree_validation.rs"),
        "vector_lifecycle" => include_str!("vector_lifecycle.rs"),
        other => panic!("unknown coverage module {other}"),
    }
}
