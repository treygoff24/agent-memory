use std::collections::VecDeque;
use std::fmt;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use chrono::SecondsFormat;
use clap::ValueEnum;

use crate::daemon_scaffold::DaemonScaffold;
use crate::harness_runner::{HarnessRunner, MockHarness, RealHarness, TestOutcome};
use crate::support::{block_on, json_escape};

const CLAUDE_KEY_ENV: &str = "MEMORUM_EVAL_CLAUDE_KEY";
const CODEX_KEY_ENV: &str = "MEMORUM_EVAL_CODEX_KEY";
const SKIP_NO_AUTH: &str = "SKIP_NO_AUTH";
const STREAM_I_DEPS_DISABLED: &str = "STREAM_I_DEPS_DISABLED";
const MOCK_HARNESS_SEMANTIC_NOT_EXERCISED: &str = "MOCK_HARNESS_SEMANTIC_NOT_EXERCISED";
const STREAM_D_ROTATION_CONTRACT_NOT_SHIPPED: &str = "STREAM_D_ROTATION_CONTRACT_NOT_SHIPPED";
const SEMANTIC_PARTIAL_LEASE_REENTRANCY_NOT_SHIPPED: &str = "SEMANTIC_PARTIAL_LEASE_REENTRANCY_NOT_SHIPPED";
/// Marker printed to stdout by a test that wants a clean skip rather than a pass.
/// Format: `MEMORUM_EVAL_SKIP:<reason>` on its own line.
const CARGO_TEST_SKIP_MARKER: &str = "MEMORUM_EVAL_SKIP:";
/// Marker printed by tests to report actual assertion count for JSON output accuracy.
/// Format: `MEMORUM_EVAL_ASSERTIONS=<n>` on its own line. Used by `eval_assert_count!`.
pub const EVAL_ASSERTION_COUNT_MARKER: &str = "MEMORUM_EVAL_ASSERTIONS=";
/// Marker printed by tests for non-fatal eval-output warnings (recorded, not gating).
/// Format: `MEMORUM_EVAL_WARNING:<message>` on its own line. Lands in the
/// orchestrator-scanned stdout channel so warnings (e.g. a recall parse-retry)
/// are not silent on stderr alone.
pub const EVAL_WARNING_MARKER: &str = "MEMORUM_EVAL_WARNING:";

#[derive(Debug, Default)]
pub struct EvalOrchestrator;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvalRunConfig {
    pub harness_mode: HarnessMode,
    pub filter: Option<String>,
    pub timeout_seconds: Option<u64>,
    pub workers: usize,
    pub no_cleanup: bool,
    pub verbose: bool,
    pub required_release_set: Option<RequiredReleaseSet>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrchestratorError {
    InvalidWorkerCount,
    NoTestsMatched { filter: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvalReport {
    pub run_id: String,
    pub started_at: String,
    pub finished_at: String,
    pub harness_mode: HarnessMode,
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub partial: bool,
    pub missing_credentials: Vec<String>,
    pub required_release_set: Option<RequiredReleaseSet>,
    pub release_blockers: Vec<String>,
    pub tests: Vec<EvalTestResult>,
    pub timed_out: bool,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct EvalRunSummary {
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvalTestResult {
    pub number: u8,
    pub name: &'static str,
    pub group: CatalogGroup,
    pub mode: CatalogMode,
    pub deferred: bool,
    pub status: TestStatus,
    pub duration_ms: u128,
    pub assertions: usize,
    pub assertions_passed: usize,
    pub assertions_failed: usize,
    pub failure_detail: Option<String>,
    pub skip_reason: Option<String>,
    pub skip_kind: Option<SkipKind>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CatalogEntry {
    pub number: u8,
    pub name: &'static str,
    pub group: CatalogGroup,
    pub mode: CatalogMode,
    pub deferred: bool,
    pub execution_group: ExecutionGroup,
    /// `cargo test --test <target>` target this entry dispatches to.
    pub cargo_target: &'static str,
    /// Test-name filter passed to `cargo test` for this entry.
    pub cargo_filter: &'static str,
    /// Real-harness CLIs that must be present for a `RealHarness` entry to
    /// dispatch (empty for simulator entries and real-harness entries that need
    /// no specific CLI).
    pub required_harnesses: &'static [RealHarness],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogGroup {
    Handbook,
    Domain,
    Regression,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogMode {
    Simulator,
    RealHarness,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionGroup {
    Parallel,
    Serial,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum HarnessMode {
    Claude,
    Codex,
    All,
    Mock,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    Json,
    Text,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum RequiredReleaseSet {
    Alpha,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestStatus {
    Passed,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipKind {
    AuthMissing,
    FeatureDeferred,
    RuntimeSelfSkip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CargoTestDispatch {
    target: &'static str,
    filter: &'static str,
}

impl Default for EvalRunConfig {
    fn default() -> Self {
        Self {
            harness_mode: HarnessMode::Mock,
            filter: None,
            timeout_seconds: None,
            workers: 4,
            no_cleanup: false,
            verbose: false,
            required_release_set: None,
        }
    }
}

impl EvalOrchestrator {
    pub fn run(&self) -> EvalRunSummary {
        match self.run_with_config(EvalRunConfig::default()) {
            Ok(report) => EvalRunSummary { passed: report.passed, failed: report.failed, skipped: report.skipped },
            Err(_) => EvalRunSummary::default(),
        }
    }

    pub fn run_with_config(&self, config: EvalRunConfig) -> Result<EvalReport, OrchestratorError> {
        if config.workers == 0 {
            return Err(OrchestratorError::InvalidWorkerCount);
        }

        let started = timestamp_string();
        let selected = select_tests(config.filter.as_deref())?;
        let missing_credentials = missing_credentials(config.harness_mode);
        let run_context = RunContext {
            harness_mode: config.harness_mode,
            timeout_seconds: config.timeout_seconds,
            verbose: config.verbose,
            missing_credentials: missing_credentials.clone(),
        };

        let mut tests = run_parallel_tests(&selected, &run_context, config.workers);
        tests.extend(run_serial_tests(&selected, &run_context));
        tests.sort_by_key(|test| test.number);

        let passed = tests.iter().filter(|test| test.status == TestStatus::Passed).count();
        let failed = tests.iter().filter(|test| test.status == TestStatus::Failed).count();
        let skipped = tests.iter().filter(|test| test.status == TestStatus::Skipped).count();
        let partial = skipped > 0;
        let timed_out = tests.iter().any(|test| test.failure_detail.as_deref() == Some("TIMEOUT"));
        let missing_credentials = if tests.iter().any(|test| test.skip_reason.as_deref() == Some(SKIP_NO_AUTH)) {
            missing_credentials
        } else {
            Vec::new()
        };
        let release_blockers = release_blockers(config.required_release_set, &tests);

        Ok(EvalReport {
            run_id: new_run_id(),
            started_at: started,
            finished_at: timestamp_string(),
            harness_mode: config.harness_mode,
            total: tests.len(),
            passed,
            failed,
            skipped,
            partial,
            missing_credentials,
            required_release_set: config.required_release_set,
            release_blockers,
            tests,
            timed_out,
        })
    }
}

#[derive(Debug, Clone)]
struct RunContext {
    harness_mode: HarnessMode,
    timeout_seconds: Option<u64>,
    verbose: bool,
    missing_credentials: Vec<String>,
}

pub const TEST_CATALOG: [CatalogEntry; 20] = [
    CatalogEntry {
        number: 1,
        name: "exact_identifier_recall",
        group: CatalogGroup::Handbook,
        mode: CatalogMode::Simulator,
        deferred: false,
        execution_group: ExecutionGroup::Parallel,
        cargo_target: "handbook",
        cargo_filter: "exact_identifier_survives_startup_recall_and_search",
        required_harnesses: &[],
    },
    CatalogEntry {
        number: 2,
        name: "superseded_fact_handling",
        group: CatalogGroup::Handbook,
        mode: CatalogMode::Simulator,
        deferred: false,
        execution_group: ExecutionGroup::Parallel,
        cargo_target: "handbook",
        cargo_filter: "superseded_fact_loses_to_replacement_in_search_and_recall",
        required_harnesses: &[],
    },
    CatalogEntry {
        number: 3,
        name: "cross_project_entity_collision",
        group: CatalogGroup::Handbook,
        mode: CatalogMode::Simulator,
        deferred: false,
        execution_group: ExecutionGroup::Parallel,
        cargo_target: "handbook",
        cargo_filter: "project_binding_filters_project_memory_from_other_project_recall",
        required_harnesses: &[],
    },
    CatalogEntry {
        number: 4,
        name: "abstention",
        group: CatalogGroup::Handbook,
        mode: CatalogMode::Simulator,
        deferred: false,
        execution_group: ExecutionGroup::Parallel,
        cargo_target: "handbook",
        cargo_filter: "novel_topic_search_and_startup_abstain_without_error",
        required_harnesses: &[],
    },
    CatalogEntry {
        number: 5,
        name: "poisoned_candidate",
        group: CatalogGroup::Handbook,
        mode: CatalogMode::Simulator,
        deferred: false,
        execution_group: ExecutionGroup::Parallel,
        cargo_target: "handbook",
        cargo_filter: "low_confidence_poisoned_candidate_is_not_promoted_or_recalled",
        required_harnesses: &[],
    },
    CatalogEntry {
        number: 6,
        name: "tool_output_preservation",
        group: CatalogGroup::Handbook,
        mode: CatalogMode::Simulator,
        deferred: false,
        execution_group: ExecutionGroup::Parallel,
        cargo_target: "handbook",
        cargo_filter: "artifact_memory_preserves_tool_output_handle_through_recall_search_and_get",
        required_harnesses: &[],
    },
    CatalogEntry {
        number: 7,
        name: "subagent_writeback",
        group: CatalogGroup::Handbook,
        mode: CatalogMode::Simulator,
        deferred: false,
        execution_group: ExecutionGroup::Parallel,
        cargo_target: "handbook",
        cargo_filter: "subagent_writeback_requires_a_spawn_registry_before_parent_recall",
        required_harnesses: &[],
    },
    CatalogEntry {
        number: 8,
        name: "deletion_and_tombstone",
        group: CatalogGroup::Handbook,
        mode: CatalogMode::Simulator,
        deferred: false,
        execution_group: ExecutionGroup::Parallel,
        cargo_target: "handbook",
        cargo_filter: "forgotten_agent_memory_is_tombstoned_hidden_and_blocks_reinsertion",
        required_harnesses: &[],
    },
    CatalogEntry {
        number: 9,
        name: "recall_budget_pressure",
        group: CatalogGroup::Handbook,
        mode: CatalogMode::Simulator,
        deferred: false,
        execution_group: ExecutionGroup::Parallel,
        cargo_target: "handbook",
        cargo_filter: "recall_budget_pressure_keeps_high_value_gold_memory_and_reports_omissions",
        required_harnesses: &[],
    },
    CatalogEntry {
        number: 10,
        name: "compaction_resumption",
        group: CatalogGroup::Handbook,
        mode: CatalogMode::Simulator,
        deferred: false,
        execution_group: ExecutionGroup::Parallel,
        cargo_target: "handbook",
        cargo_filter: "simulated_compaction_resumption_preserves_active_working_state_without_duplicates",
        required_harnesses: &[],
    },
    CatalogEntry {
        number: 11,
        name: "self_poisoning",
        group: CatalogGroup::Handbook,
        mode: CatalogMode::Simulator,
        deferred: false,
        execution_group: ExecutionGroup::Parallel,
        cargo_target: "handbook",
        cargo_filter: "self_poisoned_candidate_cannot_ground_its_own_confidence_escalation",
        required_harnesses: &[],
    },
    CatalogEntry {
        number: 12,
        name: "temporal_validity",
        group: CatalogGroup::Handbook,
        mode: CatalogMode::Simulator,
        deferred: false,
        execution_group: ExecutionGroup::Parallel,
        cargo_target: "handbook",
        cargo_filter: "temporal_validity_fields_are_not_silently_ignored_and_fresh_memory_is_currently_recalled",
        required_harnesses: &[],
    },
    CatalogEntry {
        number: 13,
        name: "cross_harness_substrate_sharing",
        group: CatalogGroup::Domain,
        mode: CatalogMode::RealHarness,
        deferred: false,
        execution_group: ExecutionGroup::Serial,
        cargo_target: "domain",
        cargo_filter: "t13_cross_harness_substrate_sharing",
        required_harnesses: &[RealHarness::Claude, RealHarness::Codex],
    },
    CatalogEntry {
        number: 14,
        name: "merge_driver_semantic_correctness",
        group: CatalogGroup::Domain,
        mode: CatalogMode::Simulator,
        deferred: false,
        execution_group: ExecutionGroup::Serial,
        cargo_target: "domain",
        cargo_filter: "t14_merge_driver_preserves_two_device_semantic_edits",
        required_harnesses: &[],
    },
    CatalogEntry {
        number: 15,
        name: "privacy_filter_refusal_retry",
        group: CatalogGroup::Domain,
        mode: CatalogMode::RealHarness,
        deferred: false,
        execution_group: ExecutionGroup::Serial,
        cargo_target: "domain",
        cargo_filter: "t15_privacy_filter_refusal_and_retry",
        required_harnesses: &[RealHarness::Claude],
    },
    CatalogEntry {
        number: 16,
        name: "reality_check_drift_scoring_sanity",
        group: CatalogGroup::Domain,
        mode: CatalogMode::Simulator,
        deferred: false,
        execution_group: ExecutionGroup::Parallel,
        cargo_target: "domain",
        cargo_filter: "t16_reality_check_drift_scores_order_and_explain_components",
        required_harnesses: &[],
    },
    CatalogEntry {
        number: 17,
        name: "lease_contention_resolution",
        group: CatalogGroup::Domain,
        mode: CatalogMode::Simulator,
        deferred: false,
        execution_group: ExecutionGroup::Serial,
        cargo_target: "domain",
        cargo_filter: "t17_preseeded_two_device_lease_blocks_loser_and_allows_retry_after_release",
        required_harnesses: &[],
    },
    CatalogEntry {
        number: 18,
        name: "encrypted_tier_key_rotation",
        group: CatalogGroup::Domain,
        mode: CatalogMode::Simulator,
        deferred: false,
        execution_group: ExecutionGroup::Serial,
        cargo_target: "domain",
        cargo_filter: "t18_encrypted_tier_key_rotation_preserves_reads_and_forward_secrecy",
        required_harnesses: &[],
    },
    CatalogEntry {
        number: 19,
        name: "peer_update_framing_correctness",
        group: CatalogGroup::Regression,
        mode: CatalogMode::RealHarness,
        deferred: false,
        execution_group: ExecutionGroup::Serial,
        cargo_target: "t19_peer_update_framing",
        cargo_filter: "t19_peer_update_framing_sampling_matrix",
        required_harnesses: &[RealHarness::Claude, RealHarness::Codex],
    },
    CatalogEntry {
        number: 20,
        name: "web_source_grounding",
        group: CatalogGroup::Domain,
        mode: CatalogMode::Simulator,
        deferred: false,
        execution_group: ExecutionGroup::Serial,
        cargo_target: "domain",
        cargo_filter: "t20_web_source_grounding",
        required_harnesses: &[],
    },
];

pub fn format_catalog() -> String {
    let mut output = String::new();
    for entry in TEST_CATALOG {
        output.push_str(&format!(
            "#{:02} {} [group: {}, mode: {}, deferred: {}, execution: {}]\n",
            entry.number, entry.name, entry.group, entry.mode, entry.deferred, entry.execution_group
        ));
    }
    output
}

pub fn report_to_json(report: &EvalReport) -> String {
    let tests_json = report.tests.iter().map(test_result_to_json).collect::<Vec<_>>().join(",\n");
    format!(
        concat!(
            "{{\n",
            "  \"run_id\": \"{}\",\n",
            "  \"started_at\": \"{}\",\n",
            "  \"finished_at\": \"{}\",\n",
            "  \"harness_mode\": \"{}\",\n",
            "  \"total\": {},\n",
            "  \"passed\": {},\n",
            "  \"failed\": {},\n",
            "  \"skipped\": {},\n",
            "  \"partial\": {},\n",
            "  \"missing_credentials\": {},\n",
            "  \"required_release_set\": {},\n",
            "  \"release_blockers\": {},\n",
            "  \"tests\": [\n{}\n  ]\n",
            "}}\n"
        ),
        json_escape(&report.run_id),
        json_escape(&report.started_at),
        json_escape(&report.finished_at),
        report.harness_mode,
        report.total,
        report.passed,
        report.failed,
        report.skipped,
        report.partial,
        string_array_to_json(&report.missing_credentials),
        optional_release_set_to_json(report.required_release_set),
        string_array_to_json(&report.release_blockers),
        tests_json
    )
}

pub fn report_to_text(report: &EvalReport) -> String {
    let mut output = format!(
        "memorum-eval {}: {} passed, {} failed, {} skipped{}.\n",
        report.run_id,
        report.passed,
        report.failed,
        report.skipped,
        if report.partial { " (partial)" } else { "" }
    );

    for test in &report.tests {
        output.push_str(&format!("#{:02} {} [{}] {}\n", test.number, test.name, test.mode, test.status));
    }

    output
}

pub fn exit_code_for_report(report: &EvalReport) -> u8 {
    if report.timed_out {
        return 3;
    }
    if report.failed > 0 {
        return 1;
    }
    if report.partial && report.harness_mode != HarnessMode::Mock {
        return 1;
    }
    if !report.release_blockers.is_empty() {
        return 1;
    }
    0
}

fn release_blockers(required_release_set: Option<RequiredReleaseSet>, tests: &[EvalTestResult]) -> Vec<String> {
    match required_release_set {
        None => Vec::new(),
        Some(RequiredReleaseSet::Alpha) => alpha_release_blockers(tests),
    }
}

fn alpha_release_blockers(tests: &[EvalTestResult]) -> Vec<String> {
    tests
        .iter()
        .filter_map(|test| {
            let catalog_entry = TEST_CATALOG.iter().find(|entry| entry.number == test.number)?;
            if catalog_entry.deferred {
                return Some(format!("#{:02} {} remains catalog-deferred", test.number, test.name));
            }
            if test.skip_kind == Some(SkipKind::FeatureDeferred) {
                return Some(format!(
                    "#{:02} {} skipped required alpha coverage: {}",
                    test.number,
                    test.name,
                    test.skip_reason.as_deref().unwrap_or("feature deferred")
                ));
            }
            None
        })
        .collect()
}

fn select_tests(filter: Option<&str>) -> Result<Vec<CatalogEntry>, OrchestratorError> {
    let selected = TEST_CATALOG
        .into_iter()
        .filter(|entry| filter.is_none_or(|pattern| matches_filter(*entry, pattern)))
        .collect::<Vec<_>>();

    if selected.is_empty() {
        let filter = filter.unwrap_or_default().to_owned();
        return Err(OrchestratorError::NoTestsMatched { filter });
    }

    Ok(selected)
}

fn run_parallel_tests(selected: &[CatalogEntry], context: &RunContext, workers: usize) -> Vec<EvalTestResult> {
    let entries = selected
        .iter()
        .copied()
        .filter(|entry| entry.execution_group == ExecutionGroup::Parallel)
        .collect::<VecDeque<_>>();
    if entries.is_empty() {
        return Vec::new();
    }

    let queue = Arc::new(Mutex::new(entries));
    let results = Arc::new(Mutex::new(Vec::new()));
    let worker_count = workers.min(selected.len()).max(1);

    std::thread::scope(|scope| {
        for _ in 0..worker_count {
            let queue = Arc::clone(&queue);
            let results = Arc::clone(&results);
            scope.spawn(move || loop {
                let entry = {
                    let mut queue = queue.lock().expect("parallel test queue mutex should not be poisoned");
                    queue.pop_front()
                };
                let Some(entry) = entry else {
                    break;
                };
                let result = run_catalog_entry(entry, context);
                results.lock().expect("parallel test results mutex should not be poisoned").push(result);
            });
        }
    });

    let mut results = results.lock().expect("parallel test results mutex should not be poisoned").clone();
    results.sort_by_key(|test| test.number);
    results
}

fn run_serial_tests(selected: &[CatalogEntry], context: &RunContext) -> Vec<EvalTestResult> {
    selected
        .iter()
        .copied()
        .filter(|entry| entry.execution_group == ExecutionGroup::Serial)
        .map(|entry| run_catalog_entry(entry, context))
        .collect()
}

fn run_catalog_entry(entry: CatalogEntry, context: &RunContext) -> EvalTestResult {
    if context.verbose {
        eprintln!("running #{:02} {} ({}, {})", entry.number, entry.name, entry.mode, entry.execution_group);
    }

    let started = Instant::now();
    if context.timeout_seconds == Some(0) {
        return failed_result(entry, started.elapsed(), "TIMEOUT");
    }

    match dispatch_for_entry(entry, context) {
        CatalogDispatch::CargoTest(dispatch) => run_cargo_test(entry, started, dispatch),
        CatalogDispatch::MockHarness => run_mock_harness(entry, started),
        CatalogDispatch::Skip(reason) => skipped_result(entry, started.elapsed(), reason),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CatalogDispatch {
    CargoTest(CargoTestDispatch),
    MockHarness,
    Skip(&'static str),
}

fn dispatch_for_entry(entry: CatalogEntry, context: &RunContext) -> CatalogDispatch {
    if entry.mode == CatalogMode::RealHarness && context.harness_mode == HarnessMode::Mock {
        return CatalogDispatch::MockHarness;
    }

    if let Some(reason) = semantic_skip_reason(entry) {
        return CatalogDispatch::Skip(reason);
    }

    // The `cargo test` dispatch (target + name filter) is co-located with the
    // catalog row, so every Simulator/RealHarness entry structurally carries
    // one — no number-keyed match and no runtime `unreachable!` fallback.
    let cargo_dispatch = CargoTestDispatch { target: entry.cargo_target, filter: entry.cargo_filter };
    match entry.mode {
        CatalogMode::Simulator => CatalogDispatch::CargoTest(cargo_dispatch),
        CatalogMode::RealHarness if !context.missing_credentials.is_empty() => CatalogDispatch::Skip(SKIP_NO_AUTH),
        CatalogMode::RealHarness if missing_real_harness_cli(entry) => CatalogDispatch::Skip(SKIP_NO_AUTH),
        CatalogMode::RealHarness => CatalogDispatch::CargoTest(cargo_dispatch),
    }
}

fn semantic_skip_reason(entry: CatalogEntry) -> Option<&'static str> {
    match entry.number {
        // T17 and T18 unconditional skips removed by H-B1.
        // Those tests carry honest internal skip guards (T17 checks re-entrant
        // lease support; T18 checks for the Stream D rotation contract files).
        // Let those guards drive skip/pass behavior instead of the orchestrator.
        19 if !cfg!(feature = "stream-i-deps") => Some(STREAM_I_DEPS_DISABLED),
        _ => None,
    }
}

fn missing_real_harness_cli(entry: CatalogEntry) -> bool {
    entry.required_harnesses.iter().any(|harness| !real_harness_cli_available(*harness))
}

fn real_harness_cli_available(harness: RealHarness) -> bool {
    matches!(HarnessRunner::detect_cli(harness), Ok(Some(_)))
}

fn run_cargo_test(entry: CatalogEntry, started: Instant, dispatch: CargoTestDispatch) -> EvalTestResult {
    let cargo = std::env::var_os("MEMORUM_EVAL_CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .args(["test", "-p", "memorum-eval", "--test", dispatch.target, dispatch.filter, "--", "--nocapture"])
        .output();

    match output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);

            // Check if the test printed a skip marker even though cargo reported success.
            // Tests that lack a required runtime dependency (e.g. T16 without Stream G)
            // print `MEMORUM_EVAL_SKIP:<reason>` so the orchestrator records a real
            // skipped result rather than a silent pass. (H-R4)
            if let Some(reason) = extract_skip_marker(&stdout) {
                return skipped_result(entry, started.elapsed(), reason);
            }

            // Parse the assertion count emitted by `eval_assert_count!` calls.
            // Tests print `MEMORUM_EVAL_ASSERTIONS=<n>` to stdout; the orchestrator
            // picks it up here so JSON output reflects real per-test granularity
            // rather than a hardcoded 1. (H-B3)
            let assertions = extract_assertion_count(&stdout).unwrap_or(1);
            passed_result_with_count(entry, started.elapsed(), assertions)
        }
        Ok(output) => failed_result(entry, started.elapsed(), &cargo_failure_detail(&output)),
        Err(error) => failed_result(entry, started.elapsed(), &format!("failed to run cargo test: {error}")),
    }
}

/// Extract a skip reason from a `MEMORUM_EVAL_SKIP:<reason>` marker in cargo test stdout.
fn extract_skip_marker(stdout: &str) -> Option<&str> {
    stdout.lines().find_map(|line| {
        let trimmed = line.trim();
        trimmed.strip_prefix(CARGO_TEST_SKIP_MARKER).map(str::trim)
    })
}

/// Extract the total assertion count from a `MEMORUM_EVAL_ASSERTIONS=<n>` marker.
fn extract_assertion_count(stdout: &str) -> Option<usize> {
    stdout.lines().find_map(|line| {
        let trimmed = line.trim();
        trimmed.strip_prefix(EVAL_ASSERTION_COUNT_MARKER).and_then(|s| s.trim().parse().ok())
    })
}

fn cargo_failure_detail(output: &std::process::Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    format!("cargo test failed with status {}\nstdout:\n{}\nstderr:\n{}", output.status, stdout.trim(), stderr.trim())
}

fn run_mock_harness(entry: CatalogEntry, started: Instant) -> EvalTestResult {
    let scaffold = block_on(DaemonScaffold::fresh());
    match MockHarness.run_test(entry.number, &scaffold) {
        Ok(TestOutcome::Passed { metadata, output }) => {
            outcome_passed_result(entry, started.elapsed(), metadata, output)
        }
        Ok(TestOutcome::Skipped { reason, .. }) => {
            skipped_result(entry, started.elapsed(), normalize_skip_reason(&reason))
        }
        Err(error) => failed_result(entry, started.elapsed(), &error.to_string()),
    }
}

fn normalize_skip_reason(reason: &str) -> &'static str {
    if reason.contains("stream-i-deps feature disabled") {
        STREAM_I_DEPS_DISABLED
    } else if reason.contains(MOCK_HARNESS_SEMANTIC_NOT_EXERCISED) {
        MOCK_HARNESS_SEMANTIC_NOT_EXERCISED
    } else {
        "SKIP"
    }
}

/// Base result carrying the entry-derived identity fields plus duration, with
/// neutral defaults (passed, zero assertion counts, no failure/skip detail).
/// Callers override only their genuinely-distinct fields via struct-update, so
/// the six identity fields are spelled out once instead of in four constructors.
fn base_result(entry: CatalogEntry, duration: Duration) -> EvalTestResult {
    EvalTestResult {
        number: entry.number,
        name: entry.name,
        group: entry.group,
        mode: entry.mode,
        deferred: entry.deferred,
        status: TestStatus::Passed,
        duration_ms: duration.as_millis(),
        assertions: 0,
        assertions_passed: 0,
        assertions_failed: 0,
        failure_detail: None,
        skip_reason: None,
        skip_kind: None,
    }
}

/// Build a passed result with an explicit assertion count from the test's
/// `MEMORUM_EVAL_ASSERTIONS=<n>` stdout marker (H-B3), defaulting to 1 when
/// no marker is present.
fn passed_result_with_count(entry: CatalogEntry, duration: Duration, assertions: usize) -> EvalTestResult {
    EvalTestResult {
        status: TestStatus::Passed,
        assertions,
        assertions_passed: assertions,
        assertions_failed: 0,
        ..base_result(entry, duration)
    }
}

fn outcome_passed_result(
    entry: CatalogEntry,
    duration: Duration,
    metadata: std::collections::HashMap<String, String>,
    output: std::collections::HashMap<String, String>,
) -> EvalTestResult {
    let assertions = (metadata.len() + output.len()).max(1);
    EvalTestResult {
        status: TestStatus::Passed,
        assertions,
        assertions_passed: assertions,
        assertions_failed: 0,
        ..base_result(entry, duration)
    }
}

fn failed_result(entry: CatalogEntry, duration: Duration, reason: &str) -> EvalTestResult {
    EvalTestResult {
        status: TestStatus::Failed,
        assertions: 1,
        assertions_passed: 0,
        assertions_failed: 1,
        failure_detail: Some(reason.to_owned()),
        ..base_result(entry, duration)
    }
}

fn skipped_result(entry: CatalogEntry, duration: Duration, reason: &str) -> EvalTestResult {
    EvalTestResult {
        status: TestStatus::Skipped,
        skip_reason: Some(reason.to_owned()),
        skip_kind: Some(skip_kind_for_reason(reason)),
        ..base_result(entry, duration)
    }
}

fn skip_kind_for_reason(reason: &str) -> SkipKind {
    if reason == SKIP_NO_AUTH {
        SkipKind::AuthMissing
    } else if [
        STREAM_D_ROTATION_CONTRACT_NOT_SHIPPED,
        SEMANTIC_PARTIAL_LEASE_REENTRANCY_NOT_SHIPPED,
        STREAM_I_DEPS_DISABLED,
    ]
    .iter()
    .any(|prefix| reason.starts_with(prefix))
    {
        SkipKind::FeatureDeferred
    } else {
        SkipKind::RuntimeSelfSkip
    }
}

fn matches_filter(entry: CatalogEntry, pattern: &str) -> bool {
    let normalized = pattern.trim().to_ascii_lowercase();
    if normalized.is_empty() || normalized == "*" {
        return true;
    }

    let targets = [
        entry.name.to_ascii_lowercase(),
        format!("t{:02}", entry.number),
        format!("#{:02}", entry.number),
        entry.number.to_string(),
        format!("{}/{}", entry.group, entry.name),
        format!("{}/t{:02}", entry.group, entry.number),
    ];

    targets.iter().any(|target| glob_like_match(target, &normalized))
}

fn glob_like_match(target: &str, pattern: &str) -> bool {
    if !pattern.contains('*') {
        return target.contains(pattern);
    }

    let parts = pattern.split('*').filter(|part| !part.is_empty()).collect::<Vec<_>>();
    if parts.is_empty() {
        return true;
    }

    let mut rest = target;
    for part in parts {
        let Some(index) = rest.find(part) else {
            return false;
        };
        rest = &rest[index + part.len()..];
    }
    true
}

fn missing_credentials(harness_mode: HarnessMode) -> Vec<String> {
    let required = match harness_mode {
        HarnessMode::Claude => vec![CLAUDE_KEY_ENV],
        HarnessMode::Codex => vec![CODEX_KEY_ENV],
        HarnessMode::All | HarnessMode::Mock => vec![CLAUDE_KEY_ENV, CODEX_KEY_ENV],
    };

    required.into_iter().filter(|name| std::env::var_os(name).is_none()).map(str::to_owned).collect()
}

fn test_result_to_json(test: &EvalTestResult) -> String {
    format!(
        concat!(
            "    {{\n",
            "      \"number\": {},\n",
            "      \"name\": \"{}\",\n",
            "      \"group\": \"{}\",\n",
            "      \"mode\": \"{}\",\n",
            "      \"deferred\": {},\n",
            "      \"status\": \"{}\",\n",
            "      \"duration_ms\": {},\n",
            "      \"assertions\": {},\n",
            "      \"assertions_passed\": {},\n",
            "      \"assertions_failed\": {},\n",
            "      \"failure_detail\": {},\n",
            "      \"skip_reason\": {},\n",
            "      \"skip_kind\": {}\n",
            "    }}"
        ),
        test.number,
        json_escape(test.name),
        test.group,
        test.mode,
        test.deferred,
        test.status,
        test.duration_ms,
        test.assertions,
        test.assertions_passed,
        test.assertions_failed,
        optional_string_to_json(test.failure_detail.as_deref()),
        optional_string_to_json(test.skip_reason.as_deref()),
        optional_skip_kind_to_json(test.skip_kind)
    )
}

fn string_array_to_json(values: &[String]) -> String {
    let body = values.iter().map(|value| format!("\"{}\"", json_escape(value))).collect::<Vec<_>>().join(", ");
    format!("[{body}]")
}

fn optional_string_to_json(value: Option<&str>) -> String {
    value.map_or_else(|| "null".to_owned(), |value| format!("\"{}\"", json_escape(value)))
}

fn optional_skip_kind_to_json(value: Option<SkipKind>) -> String {
    value.map_or_else(|| "null".to_owned(), |value| format!("\"{}\"", value))
}

fn optional_release_set_to_json(value: Option<RequiredReleaseSet>) -> String {
    value.map_or_else(|| "null".to_owned(), |value| format!("\"{}\"", value))
}

fn new_run_id() -> String {
    format!("eval-{}", unix_millis())
}

fn timestamp_string() -> String {
    // Spec §6.2 requires ISO 8601 timestamps in JSON output. (H-nit)
    chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn unix_millis() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).expect("system clock is after Unix epoch").as_millis()
}

impl fmt::Display for OrchestratorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidWorkerCount => formatter.write_str("--workers must be greater than zero"),
            Self::NoTestsMatched { filter } => write!(formatter, "no eval tests matched filter `{filter}`"),
        }
    }
}

impl std::error::Error for OrchestratorError {}

impl fmt::Display for CatalogGroup {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Handbook => formatter.write_str("handbook"),
            Self::Domain => formatter.write_str("domain"),
            Self::Regression => formatter.write_str("regression"),
        }
    }
}

impl fmt::Display for CatalogMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Simulator => formatter.write_str("simulator"),
            Self::RealHarness => formatter.write_str("real_harness"),
        }
    }
}

impl fmt::Display for ExecutionGroup {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parallel => formatter.write_str("parallel"),
            Self::Serial => formatter.write_str("serial"),
        }
    }
}

impl fmt::Display for HarnessMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Claude => formatter.write_str("claude"),
            Self::Codex => formatter.write_str("codex"),
            Self::All => formatter.write_str("all"),
            Self::Mock => formatter.write_str("mock"),
        }
    }
}

impl fmt::Display for OutputFormat {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json => formatter.write_str("json"),
            Self::Text => formatter.write_str("text"),
        }
    }
}

impl fmt::Display for RequiredReleaseSet {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Alpha => formatter.write_str("alpha"),
        }
    }
}

impl fmt::Display for TestStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Passed => formatter.write_str("passed"),
            Self::Failed => formatter.write_str("failed"),
            Self::Skipped => formatter.write_str("skipped"),
        }
    }
}

impl fmt::Display for SkipKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AuthMissing => formatter.write_str("auth_missing"),
            Self::FeatureDeferred => formatter.write_str("feature_deferred"),
            Self::RuntimeSelfSkip => formatter.write_str("runtime_self_skip"),
        }
    }
}
