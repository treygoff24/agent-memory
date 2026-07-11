#![allow(clippy::too_many_lines)]

use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

const SPEC_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/specs/stream-a-core-substrate-v1.2.md");
const COVERAGE_REVIEW_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/reviews/stream-a-test-coverage-review.md");

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CoverageStatus {
    Covered,
    Gap,
}

#[derive(Clone, Copy, Debug)]
struct SpecCoverageEntry {
    section: &'static str,
    bullet_hash: &'static str,
    status: CoverageStatus,
    evidence: &'static str,
}

impl SpecCoverageEntry {
    const fn covered(section: &'static str, bullet_hash: &'static str, test_path: &'static str) -> Self {
        Self { section, bullet_hash, status: CoverageStatus::Covered, evidence: test_path }
    }
}

const SPEC_COVERAGE: &[SpecCoverageEntry] = &[
    SpecCoverageEntry::covered(
        "5.5 Acceptance signals",
        "cc8e53253dfd66dd",
        "tree_validation::fresh_init_creates_working_tree_dirs_and_tracked_bootstrap_files",
    ),
    SpecCoverageEntry::covered(
        "5.5 Acceptance signals",
        "9791e52dc99eb512",
        "git_adoption::fresh_clone_adoption_regenerates_local_identity_event_log_and_merge_config",
    ),
    SpecCoverageEntry::covered(
        "5.5 Acceptance signals",
        "afbbc5d7b5d51a25",
        "tree_validation::duplicate_frontmatter_ids_fail_validation",
    ),
    SpecCoverageEntry::covered(
        "5.5 Acceptance signals",
        "15b108ad0628f6d1",
        "tree_validation::case_only_path_collision_fixture_fails_validation",
    ),
    SpecCoverageEntry::covered(
        "5.5 Acceptance signals",
        "8d94ef8f9003f778",
        "tree_validation::supersession_cycle_fails_cross_file_validation",
    ),
    SpecCoverageEntry::covered(
        "6.13 Acceptance signals",
        "3e8cc771de16c310",
        "frontmatter_schema::frontmatter_field_rule_matrix_accepts_and_rejects_known_shapes",
    ),
    SpecCoverageEntry::covered(
        "6.13 Acceptance signals",
        "e9fbea59cc15d90e",
        "frontmatter_schema::prospective_memory_with_time_event_and_conditional_triggers_validates",
    ),
    SpecCoverageEntry::covered(
        "6.13 Acceptance signals",
        "fe278b311fd0863c",
        "frontmatter_schema::tombstoned_memory_with_two_events_validates_and_round_trips",
    ),
    SpecCoverageEntry::covered(
        "6.13 Acceptance signals",
        "5695510e95374b94",
        "frontmatter_schema::merge_driver_quarantine_output_validates",
    ),
    SpecCoverageEntry::covered(
        "6.13 Acceptance signals",
        "30970c3aad9c9440",
        "frontmatter_schema::preserves_unknown_v1_extras",
    ),
    SpecCoverageEntry::covered(
        "6.13 Acceptance signals",
        "c9b5b2cf3d44b963",
        "tree_validation::supersession_cycle_fails_cross_file_validation",
    ),
    SpecCoverageEntry::covered(
        "7.4 Acceptance signals",
        "8805581ecc4bca35",
        "id_sequence::sequential_ids_on_one_device_are_unique_and_monotonic",
    ),
    SpecCoverageEntry::covered(
        "7.4 Acceptance signals",
        "20bc4f4b7fab637a",
        "id_sequence::two_devices_with_different_shards_mint_50k_each_without_collision",
    ),
    SpecCoverageEntry::covered(
        "7.4 Acceptance signals",
        "f1118fcd442bcf4a",
        "git_adoption::adoption_force_new_device_regenerates_local_identity_before_writes",
    ),
    SpecCoverageEntry::covered(
        "7.4 Acceptance signals",
        "98a51dcc69647ed5",
        "id_sequence::sequence_999999_succeeds_then_1000000_is_exhausted",
    ),
    SpecCoverageEntry::covered(
        "7.4 Acceptance signals",
        "e977aaaca0984652",
        "id_sequence::copied_device_duplicate_ids_are_repaired_to_repo_visible_free_ids",
    ),
    SpecCoverageEntry::covered(
        "7.4 Acceptance signals",
        "cb2d2388470a83b8",
        "id_sequence::stale_sequence_state_advances_past_repo_visible_high_water",
    ),
    SpecCoverageEntry::covered(
        "8.6 Acceptance signals",
        "af116d6fa4bf2637",
        "atomic_write::atomic_write_stages_temp_in_target_parent_without_cross_device_rename",
    ),
    SpecCoverageEntry::covered(
        "8.6 Acceptance signals",
        "299b13e389256a84",
        "crash_matrix::deterministic_crash_matrix_converges_to_documented_write_states",
    ),
    SpecCoverageEntry::covered(
        "8.6 Acceptance signals",
        "6814aeabc1f06946",
        "api_write_read::stale_base_replace_leaves_existing_file_unchanged",
    ),
    SpecCoverageEntry::covered(
        "8.6 Acceptance signals",
        "ba75028135610a0c",
        "api_write_read::encrypted_write_uses_encrypted_path_and_metadata_only_index",
    ),
    SpecCoverageEntry::covered(
        "8.6 Acceptance signals",
        "f47c4634944cb6ee",
        "api_write_read::event_after_commit_failure_returns_committed_indexed_repair_outcome",
    ),
    SpecCoverageEntry::covered(
        "8.6 Acceptance signals",
        "b668996b11b9f909",
        "api_write_read::classification_secret_refuses_before_any_disk_effect",
    ),
    SpecCoverageEntry::covered(
        "8.6 Acceptance signals",
        "4dd04e17685f236b",
        "api_write_read::plaintext_requires_encryption_classification_is_refused_before_disk_effect",
    ),
    SpecCoverageEntry::covered(
        "8.6 Acceptance signals",
        "8fcb7750e2fe38ba",
        "api_write_read::trusted_classification_cannot_persist_confidential_plaintext",
    ),
    SpecCoverageEntry::covered(
        "9.4 Acceptance signals",
        "f831ef51512379d0",
        "frontmatter_schema::frontmatter_field_rule_matrix_accepts_and_rejects_known_shapes",
    ),
    SpecCoverageEntry::covered(
        "9.4 Acceptance signals",
        "9fc2e5b15e4806b8",
        "frontmatter_schema::parses_missing_nullable_fields_with_typed_defaults_and_warnings",
    ),
    SpecCoverageEntry::covered(
        "9.4 Acceptance signals",
        "d0612eb81071fba3",
        "frontmatter_schema::higher_schema_version_is_rejected_before_mutation",
    ),
    SpecCoverageEntry::covered(
        "9.4 Acceptance signals",
        "405415be8e8e55e2",
        "tree_validation::inverse_supersession_mismatch_fails_when_both_endpoints_exist",
    ),
    SpecCoverageEntry::covered(
        "10.6 Acceptance signals",
        "025a1008512aa0a6",
        "release_gate_contracts::release_gate_script_runs_release_perf_and_regression_contracts",
    ),
    SpecCoverageEntry::covered(
        "10.6 Acceptance signals",
        "e6d9f719e7d57176",
        "api_write_read::fts_mutation_removes_old_terms_after_replace",
    ),
    SpecCoverageEntry::covered(
        "10.6 Acceptance signals",
        "be3374079a838a73",
        "index_mutation::vacuum_preserves_chunk_fts_matches",
    ),
    SpecCoverageEntry::covered(
        "10.6 Acceptance signals",
        "ac66e61124c47fe9",
        "vector_reconciliation::vector_orphan_and_missing_reconciliation_deletes_orphans_and_queues_jobs",
    ),
    SpecCoverageEntry::covered(
        "10.6 Acceptance signals",
        "8267d1f5ddf659e0",
        "vector_lifecycle::update_embedding_rejects_wrong_dimension_and_stale_hash",
    ),
    SpecCoverageEntry::covered(
        "10.6 Acceptance signals",
        "f82f595a9c45f845",
        "vector_reconciliation::vector_orphan_and_missing_reconciliation_deletes_orphans_and_queues_jobs",
    ),
    SpecCoverageEntry::covered(
        "10.6 Acceptance signals",
        "c7754241563bfade",
        "release_gate_contracts::release_gate_script_runs_release_perf_and_regression_contracts",
    ),
    SpecCoverageEntry::covered(
        "10.6 Acceptance signals",
        "08b59a9a70adca47",
        "reindex_reconciliation::rename_plus_id_change_removes_old_id_and_indexes_new_id",
    ),
    SpecCoverageEntry::covered(
        "10.6 Acceptance signals",
        "8631bf2196763fed",
        "api_write_read::encrypted_write_uses_encrypted_path_and_metadata_only_index",
    ),
    SpecCoverageEntry::covered(
        "10.6 Acceptance signals",
        "64178865a54d5c2b",
        "vector_lifecycle::update_embedding_rejects_wrong_dimension_and_stale_hash",
    ),
    SpecCoverageEntry::covered(
        "10.6 Acceptance signals",
        "cb43fc628d606ddd",
        "vector_reconciliation::active_triple_switch_queues_chunks_for_new_embedding_triple",
    ),
    SpecCoverageEntry::covered(
        "10.6 Acceptance signals",
        "13fa4cb05aa70fc1",
        "vector_lifecycle::dropped_triple_returns_unknown_and_cannot_be_recreated_by_stale_worker",
    ),
    SpecCoverageEntry::covered(
        "11.4 Acceptance signals",
        "1a874c7b33bc8b4c",
        "api_write_read::write_read_query_and_event_round_trip_through_public_api",
    ),
    SpecCoverageEntry::covered(
        "11.4 Acceptance signals",
        "3266e0deb90c8b20",
        "reindex_reconciliation::external_edit_to_same_path_is_indexed_by_reindex",
    ),
    SpecCoverageEntry::covered(
        "11.4 Acceptance signals",
        "96b1a66b8dc9e36a",
        "watcher_lifecycle::watcher_overflow_event_requests_rescan",
    ),
    SpecCoverageEntry::covered(
        "11.4 Acceptance signals",
        "02a92c71a18a2787",
        "reindex_reconciliation::mass_changes_converge_to_fresh_reindex_state",
    ),
    SpecCoverageEntry::covered(
        "11.4 Acceptance signals",
        "6eb3befe90339494",
        "watcher_lifecycle::watch_subscription_outlives_substrate_until_unsubscribe",
    ),
    SpecCoverageEntry::covered(
        "12.5 Acceptance signals",
        "58e13929016f382a",
        "event_log_recovery::event_log_recovery_truncates_one_malformed_trailing_line",
    ),
    SpecCoverageEntry::covered(
        "12.5 Acceptance signals",
        "ba5ec42e1c1da181",
        "startup_reconciliation::startup_replay_skips_pending_event_already_in_log",
    ),
    SpecCoverageEntry::covered(
        "12.5 Acceptance signals",
        "b7e891f01dd0767c",
        "event_log_identity::same_device_duplicate_logs_are_refused_until_adoption_repair",
    ),
    SpecCoverageEntry::covered(
        "12.5 Acceptance signals",
        "2637615d39d4ebc8",
        "startup_reconciliation::startup_replays_pending_event_queue_and_compacts_it",
    ),
    SpecCoverageEntry::covered(
        "13.7 Acceptance signals",
        "9a8441efbc359f2f",
        "git_adoption::fresh_clone_without_adoption_preflight_returns_repair_instruction",
    ),
    SpecCoverageEntry::covered(
        "13.7 Acceptance signals",
        "f6639a42c6430ec8",
        "git_adoption::fresh_clone_with_adoption_can_perform_semantic_same_file_merge",
    ),
    SpecCoverageEntry::covered(
        "13.7 Acceptance signals",
        "35f6d6772d35422d",
        "release_gate_contracts::two_clone_convergence_script_reaches_fixed_point",
    ),
    SpecCoverageEntry::covered(
        "13.7 Acceptance signals",
        "e36c7690a6c4edcd",
        "git_preflight::missing_merge_driver_binary_refuses_before_merge",
    ),
    SpecCoverageEntry::covered(
        "13.7 Acceptance signals",
        "d0e3ade3e2c60f0f",
        "startup_reconciliation::startup_replay_skips_pending_event_already_in_log",
    ),
    SpecCoverageEntry::covered(
        "13.7 Acceptance signals",
        "a994398dbd4317bd",
        "release_gate_contracts::two_clone_convergence_script_reaches_fixed_point",
    ),
    SpecCoverageEntry::covered(
        "14.8 Acceptance signals",
        "a1273cc9ca6e585d",
        "merge_rules::lifecycle_pair_fixture_matrix_outputs_valid_markdown",
    ),
    SpecCoverageEntry::covered(
        "14.8 Acceptance signals",
        "63d15c46ae2a35c3",
        "merge_rules::independent_scalar_edits_both_survive",
    ),
    SpecCoverageEntry::covered(
        "14.8 Acceptance signals",
        "fc4207d10e3221a4",
        "merge_rules::conflicting_body_edits_quarantine_instead_of_dropping_theirs",
    ),
    SpecCoverageEntry::covered(
        "14.8 Acceptance signals",
        "386a91a9a742cb95",
        "merge_rules::evidence_id_collision_emits_near_duplicate_diagnostic",
    ),
    SpecCoverageEntry::covered(
        "14.8 Acceptance signals",
        "8e15e0aad1e6988b",
        "merge_rules::regression_occurrence_counts_merge_by_id_with_max_count",
    ),
    SpecCoverageEntry::covered(
        "14.8 Acceptance signals",
        "8005d42943ff60fd",
        "merge_rules::unknown_fields_use_true_three_way_per_key",
    ),
    SpecCoverageEntry::covered(
        "14.8 Acceptance signals",
        "0bac3ec7abe7a9d7",
        "merge_rules::add_add_same_path_quarantine_preserves_alternates_with_raw_bytes",
    ),
    SpecCoverageEntry::covered(
        "14.8 Acceptance signals",
        "92ae95c3a0fda84b",
        "merge_rules::add_add_same_path_quarantine_preserves_alternates_with_raw_bytes",
    ),
    SpecCoverageEntry::covered(
        "14.8 Acceptance signals",
        "cdf8060ec83f5280",
        "frontmatter_schema::merge_driver_quarantine_output_validates",
    ),
    SpecCoverageEntry::covered(
        "14.8 Acceptance signals",
        "09a3112efb9460c7",
        "merge_rules::sensitivity_conflict_resolves_to_more_restrictive_with_diagnostics",
    ),
    SpecCoverageEntry::covered(
        "14.8 Acceptance signals",
        "281c16801862c484",
        "merge_driver_cli::merge_driver_schema_version_gate_exits_one_without_writing_ours",
    ),
    SpecCoverageEntry::covered(
        "14.8 Acceptance signals",
        "b93df2aa3287daae",
        "merge_rules::merge_driver_fuzz_smoke_never_panics_and_outputs_valid_yaml",
    ),
    SpecCoverageEntry::covered(
        "15.4 Acceptance signals",
        "980e18779f182572",
        "config_loading::fresh_clone_has_synced_config_but_no_local_device_until_adoption",
    ),
    SpecCoverageEntry::covered(
        "15.4 Acceptance signals",
        "1d47e7dea9b87397",
        "config_loading::loading_config_never_copies_device_id_from_synced_repo_state",
    ),
    SpecCoverageEntry::covered(
        "15.4 Acceptance signals",
        "fb622411e99d0699",
        "config_loading::env_overrides_are_visible_but_not_serialized_to_synced_config",
    ),
    SpecCoverageEntry::covered(
        "15.4 Acceptance signals",
        "c3a124dcb24c727b",
        "config_loading::local_roots_win_over_synced_defaults_without_mutating_synced_config",
    ),
    SpecCoverageEntry::covered(
        "16.7 Acceptance signals",
        "27a097a9c8401e5b",
        "api_contracts::public_api_contracts_compile",
    ),
    SpecCoverageEntry::covered(
        "16.7 Acceptance signals",
        "76ea4f6b20eceea5",
        "error_variant_coverage::every_current_public_error_family_has_behavioral_coverage",
    ),
    SpecCoverageEntry::covered(
        "16.7 Acceptance signals",
        "ea2c31d950f27473",
        "async_cancellation::dropping_unpolled_write_future_has_no_repo_index_or_event_effects",
    ),
    SpecCoverageEntry::covered(
        "16.7 Acceptance signals",
        "6d764c5ce4f0dee1",
        "api_write_read::write_outcomes_distinguish_not_committed_full_commit_and_event_repair_states",
    ),
    SpecCoverageEntry::covered(
        "17.7 Overall acceptance",
        "7193e73cb24c3f09",
        "spec_coverage_manifest::release_certification_has_no_known_spec_coverage_gaps",
    ),
    SpecCoverageEntry::covered(
        "17.7 Overall acceptance",
        "483d4f4346667bea",
        "release_gate_contracts::check_script_contains_release_test_doc_spec_and_convergence_gates",
    ),
    SpecCoverageEntry::covered(
        "17.7 Overall acceptance",
        "ab949d77703c2367",
        "release_gate_contracts::fuzz_workflow_runs_both_targets_for_ten_minutes",
    ),
    SpecCoverageEntry::covered(
        "17.7 Overall acceptance",
        "76eb66f4aae74b2d",
        "release_gate_contracts::two_clone_convergence_script_reaches_fixed_point",
    ),
    SpecCoverageEntry::covered(
        "17.7 Overall acceptance",
        "d145936fde58a15d",
        "release_gate_contracts::check_script_contains_release_test_doc_spec_and_convergence_gates",
    ),
    SpecCoverageEntry::covered(
        "17.7 Overall acceptance",
        "2959ba35f1c86926",
        "release_gate_contracts::release_gate_script_runs_release_perf_and_regression_contracts",
    ),
    SpecCoverageEntry::covered(
        "17.7 Overall acceptance",
        "576beb1eec9781c7",
        "release_gate_contracts::durability_probe_gate_exercises_full_refused_and_best_effort_matrix",
    ),
    SpecCoverageEntry::covered(
        "17.7 Overall acceptance",
        "f2893478d9479792",
        "crash_matrix::deterministic_crash_matrix_converges_to_documented_write_states",
    ),
    SpecCoverageEntry::covered(
        "17.7 Overall acceptance",
        "bb8df91f06152f42",
        "spec_coverage_manifest::release_certification_has_no_known_spec_coverage_gaps",
    ),
    SpecCoverageEntry::covered(
        "17.7 Overall acceptance",
        "8f130a0905ecfaf2",
        "spec_coverage_manifest::final_review_records_no_blocking_findings",
    ),
    SpecCoverageEntry::covered(
        "17.8 Acceptance",
        "ba006e63631ed625",
        "spec_coverage_manifest::spec_acceptance_signals_have_named_tests",
    ),
    SpecCoverageEntry::covered(
        "17.8 Acceptance",
        "2f82d0d9b3c4aadd",
        "spec_coverage_manifest::spec_acceptance_signals_have_named_tests",
    ),
    SpecCoverageEntry::covered(
        "17.8 Acceptance",
        "ad632c62e0a1fe36",
        "spec_coverage_manifest::covered_manifest_references_existing_tests",
    ),
    SpecCoverageEntry::covered(
        "10.6 Acceptance signals",
        "00ce217474efea9b",
        "dream/abstraction_compile::user_scoped_secret_cue_refuses_before_disk",
    ),
    SpecCoverageEntry::covered(
        "10.6 Acceptance signals",
        "6c8cede48034047c",
        "dream/abstraction_compile::sensitive_generated_fields_persist_body_only_without_aux_state",
    ),
    SpecCoverageEntry::covered(
        "10.6 Acceptance signals",
        "28c1089159ffa68c",
        "vector_lifecycle::auxiliary_vectors_are_stale_fenced_queryable_and_invalidated_on_hash_change",
    ),
    SpecCoverageEntry::covered(
        "10.6 Acceptance signals",
        "7f45d41c5015e55a",
        "merge_rules::cue_union_converges_with_overflow_and_casing_only_duplicates",
    ),
    SpecCoverageEntry::covered(
        "10.6 Acceptance signals",
        "5ab8b0f208a7bc3d",
        "merge_rules::abstraction_conflict_uses_updated_at_then_side_independent_hash_and_preserves_loser",
    ),
    SpecCoverageEntry::covered(
        "10.6 Acceptance signals",
        "e0a42dbe8ef46d01",
        "index_migration_v6::migrate_v6_is_idempotent_and_preserves_representative_data_and_rollback_is_readable",
    ),
    SpecCoverageEntry::covered(
        "10.6 Acceptance signals",
        "a88a4a3f79f8e798",
        "reindex_reconciliation::reindex_from_files_rebuilds_semantic_rows_and_jobs",
    ),
    SpecCoverageEntry::covered(
        "10.6 Acceptance signals",
        "cc5e566ae895e66b",
        "vector_lifecycle::aux_hash_change_between_fetch_and_update_rejects_stale_and_preserves_replacement_job",
    ),
    SpecCoverageEntry::covered(
        "10.6 Acceptance signals",
        "592dd0d0bdf1c351",
        "vector_lifecycle::auxiliary_vectors_are_stale_fenced_queryable_and_invalidated_on_hash_change",
    ),
    SpecCoverageEntry::covered(
        "10.6 Acceptance signals",
        "032c6e620bb0455c",
        "vector_lifecycle::active_triple_switch_reenqueues_abstraction_and_cue_rows",
    ),
    SpecCoverageEntry::covered(
        "10.6 Acceptance signals",
        "eeaaaa10f842b7f7",
        "handlers/doctor::doctor_reports_embedding_counts_for_each_row_kind",
    ),
];

#[derive(Debug, Eq, Ord, PartialEq, PartialOrd)]
struct SpecBullet {
    section: String,
    bullet_hash: String,
}

#[test]
fn spec_acceptance_signals_have_named_tests() {
    let spec_keys: BTreeSet<_> = parse_spec_bullets(Path::new(SPEC_PATH)).into_iter().collect();
    let manifest_keys = manifest_keys();

    let missing: Vec<_> = spec_keys.difference(&manifest_keys).collect();
    assert!(missing.is_empty(), "spec acceptance bullets missing manifest entries: {missing:#?}");

    let stale: Vec<_> = manifest_keys.difference(&spec_keys).collect();
    assert!(stale.is_empty(), "manifest entries whose section/hash no longer exist in spec: {stale:#?}");
}

#[test]
fn covered_manifest_references_existing_tests() {
    let repo_root = repo_root();
    let mut missing = Vec::new();

    for entry in SPEC_COVERAGE.iter().filter(|entry| entry.status == CoverageStatus::Covered) {
        if !test_reference_exists(&repo_root, entry.evidence) {
            missing.push((entry.section, entry.bullet_hash, entry.evidence));
        }
    }

    assert!(missing.is_empty(), "covered manifest entries reference missing tests: {missing:#?}");
}

#[test]
fn known_gap_entries_are_explicitly_marked_for_release_closeout() {
    let gaps: Vec<_> = SPEC_COVERAGE.iter().filter(|entry| entry.status == CoverageStatus::Gap).collect();
    if gaps.is_empty() {
        return;
    }
    assert!(
        gaps.iter().all(|entry| entry.evidence.starts_with("release_gap:")),
        "gap entries must use release_gap: evidence labels: {gaps:#?}"
    );

    let coverage_review = fs::read_to_string(COVERAGE_REVIEW_PATH).expect("coverage review doc should be readable");
    assert!(
        coverage_review.contains("Remaining"),
        "coverage review must explicitly document that gap entries are not release certification"
    );
}

#[test]
fn release_certification_has_no_known_spec_coverage_gaps() {
    let gaps: Vec<_> = SPEC_COVERAGE.iter().filter(|entry| entry.status == CoverageStatus::Gap).collect();
    assert!(gaps.is_empty(), "remaining spec coverage gaps: {gaps:#?}");
}

#[test]
fn final_review_records_no_blocking_findings() {
    let final_review_path = repo_root().join("docs/reviews/stream-a-final-review.md");
    let final_review = fs::read_to_string(final_review_path).expect("final review doc should be readable");
    assert!(final_review.contains("Status: release-certification candidate"));
    assert!(final_review.contains("Independent review: no blocking findings"));
    assert!(final_review.contains("Encrypted writes"));
    assert!(final_review.contains("Startup reconciliation"));
}

fn parse_spec_bullets(path: &Path) -> Vec<SpecBullet> {
    let spec = fs::read_to_string(path).expect("stream-a spec should be readable");
    let mut current_section: Option<String> = None;
    let mut in_acceptance_sub_block = false;
    let mut bullets = Vec::new();

    for raw_line in spec.lines() {
        let line = raw_line.trim_end();

        if let Some(section) = parse_acceptance_heading(line) {
            current_section = Some(section);
            in_acceptance_sub_block = false;
            continue;
        }

        if line == "Acceptance:" {
            current_section = Some("17.8 Acceptance".to_owned());
            in_acceptance_sub_block = true;
            continue;
        }

        if current_section.is_some() && (line.starts_with("## ") || line.starts_with("### ")) {
            current_section = None;
            in_acceptance_sub_block = false;
        }

        if in_acceptance_sub_block && line == "---" {
            current_section = None;
            in_acceptance_sub_block = false;
        }

        let Some(section) = current_section.as_ref() else {
            continue;
        };
        let Some(bullet_text) = strip_bullet_prefix(line) else {
            continue;
        };

        bullets.push(SpecBullet { section: section.clone(), bullet_hash: short_hash(bullet_text.trim()) });
    }

    bullets
}

fn parse_acceptance_heading(line: &str) -> Option<String> {
    let heading = line.strip_prefix("### ")?;
    if heading == "Acceptance signals" {
        return Some("unnumbered Acceptance signals".to_owned());
    }

    if let Some(section) = heading.strip_suffix(" Acceptance signals") {
        if is_numbered_section(section) {
            return Some(format!("{section} Acceptance signals"));
        }
    }

    let section = heading.strip_suffix(" Overall acceptance")?;
    is_numbered_section(section).then(|| format!("{section} Overall acceptance"))
}

fn strip_bullet_prefix(line: &str) -> Option<&str> {
    if let Some(text) = line.strip_prefix("- ") {
        return Some(text);
    }

    let (number, text) = line.split_once(". ")?;
    number.chars().all(|character| character.is_ascii_digit()).then_some(text)
}

fn is_numbered_section(section: &str) -> bool {
    let Some((major, minor)) = section.split_once('.') else {
        return false;
    };
    !major.is_empty()
        && !minor.is_empty()
        && major.chars().all(|character| character.is_ascii_digit())
        && minor.chars().all(|character| character.is_ascii_digit())
}

fn short_hash(text: &str) -> String {
    let digest = Sha256::digest(text.as_bytes());
    hex::encode(digest)[..16].to_owned()
}

fn manifest_keys() -> BTreeSet<SpecBullet> {
    let mut counts: BTreeMap<(&str, &str), usize> = BTreeMap::new();
    for entry in SPEC_COVERAGE {
        *counts.entry((entry.section, entry.bullet_hash)).or_default() += 1;
    }

    let duplicates: Vec<_> =
        counts.iter().filter(|(_, count)| **count > 1).map(|(key, count)| (*key, *count)).collect();
    assert!(duplicates.is_empty(), "duplicate manifest entries: {duplicates:#?}");

    SPEC_COVERAGE
        .iter()
        .map(|entry| SpecBullet { section: entry.section.to_owned(), bullet_hash: entry.bullet_hash.to_owned() })
        .collect()
}

fn test_reference_exists(repo_root: &Path, test_path: &str) -> bool {
    let Some((module, test_name)) = test_path.split_once("::") else {
        return false;
    };

    test_file_candidates(repo_root, module)
        .iter()
        .any(|path| fs::read_to_string(path).map(|source| source.contains(&format!("fn {test_name}"))).unwrap_or(false))
}

fn test_file_candidates(repo_root: &Path, module: &str) -> Vec<PathBuf> {
    vec![
        repo_root.join("crates/memory-substrate/tests").join(format!("{module}.rs")),
        repo_root.join("crates/memory-merge-driver/tests").join(format!("{module}.rs")),
        repo_root.join("crates/memoryd/src").join(format!("{module}.rs")),
    ]
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crate dir should have crates parent")
        .parent()
        .expect("crates dir should have repo parent")
        .to_path_buf()
}
