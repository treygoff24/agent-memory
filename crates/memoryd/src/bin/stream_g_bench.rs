use std::collections::BTreeMap;
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use anyhow::{bail, Context};
use chrono::{DateTime, Utc};
use clap::Parser;
use memory_substrate::index::{open_index, Index};
use memory_substrate::{
    InitOptions, MemoryId, MemoryStatus, RecallIndexRow, RepoPath, Roots, Scope, Sensitivity, SourceKind, Substrate,
};
use memoryd::notifications::PassiveQueue;
use memoryd::reality_check::{score_memories_at, ScoredMemory, ScoringConfig};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const FIXTURE_VERSION: &str = "stream-g-observability-v0.1-task-17";
const RUN_DATE: &str = "2026-05-02";
const RUN_AT: &str = "2026-05-02T12:00:00Z";

const SCORING_MEMORY_COUNT: usize = 10_000;
const SCORING_SAMPLE_COUNT: usize = 5;
const TOP_N_SAMPLE_COUNT: usize = 21;
const SESSION_RESUME_SAMPLE_COUNT: usize = 21;
const TUI_SAMPLE_COUNT: usize = 80;
const ENTITY_GRAPH_NODE_COUNT: usize = 5_000;
const ENTITY_GRAPH_SAMPLE_COUNT: usize = 15;
const STATUS_SAMPLE_COUNT: usize = 101;
const PASSIVE_QUEUE_SAMPLE_COUNT: usize = 1_001;
const SLACK_DISPATCH_SAMPLE_COUNT: usize = 7;

const SCORING_BUDGET_MS: f64 = 500.0;
const TOP_N_BUDGET_MS: f64 = 50.0;
const SESSION_RESUME_BUDGET_MS: f64 = 100.0;
const TUI_PANEL_SWITCH_BUDGET_MS: f64 = 16.0;
const TUI_DETAIL_MODAL_BUDGET_MS: f64 = 32.0;
const TUI_TYPEAHEAD_BUDGET_MS: f64 = 100.0;
const TUI_TYPEAHEAD_DEBOUNCE_MS: u64 = 96;
const ENTITY_GRAPH_BUDGET_MS: f64 = 200.0;
const STATUS_BUDGET_MS: f64 = 50.0;
const PASSIVE_QUEUE_BUDGET_MS: f64 = 1.0;
const SLACK_DISPATCH_BUDGET_MS: f64 = 2_000.0;
const DEFAULT_TOP_N: usize = 12;

#[derive(Debug, Parser)]
struct Args {
    /// Hardware/profile label recorded in the JSON fixture.
    #[arg(long)]
    profile: String,

    /// Assert current measurements against Stream G budgets and an existing baseline contract.
    #[arg(long)]
    assert: bool,

    /// Existing baseline JSON to validate in assert mode.
    #[arg(long)]
    baseline: Option<PathBuf>,

    /// Explicit release/update mode target. Without --promote-canonical this writes PATH.proposed only.
    #[arg(long, alias = "write-output")]
    output: Option<PathBuf>,

    /// Promote --output directly to the canonical file.
    ///
    /// This must only be run from a human shell session after reviewing the proposed file;
    /// automation should omit this flag so canonical Stream G baselines are never self-promoted.
    #[arg(long)]
    promote_canonical: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BenchReport {
    schema_version: u32,
    fixture_version: String,
    profile: String,
    runs: usize,
    platform: PlatformReport,
    fixture: FixtureReport,
    measurements: Vec<BenchMeasurement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PlatformReport {
    os: String,
    arch: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct FixtureReport {
    run_date: String,
    scoring_memory_count: usize,
    entity_graph_node_count: usize,
    tui_panel_count: usize,
    typeahead_debounce_ms: u64,
    passive_queue_sample_count: usize,
    slack_dispatch_sample_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BenchMeasurement {
    name: String,
    description: String,
    statistic: Statistic,
    measured_ms: f64,
    budget_ms: f64,
    budget_operator: BudgetOperator,
    sample_count: usize,
    pass: bool,
    details: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Statistic {
    P95,
    P99,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum BudgetOperator {
    LessThanOrEqual,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    validate_mode(&args)?;

    let report = run_benchmarks(&args.profile).await?;

    if args.assert {
        let baseline_path = args.baseline.as_ref().context("baseline is required in assert mode")?;
        if baseline_requires_bootstrap(baseline_path)? {
            enforce_budgets(&report)?;
            let proposed_path = proposed_baseline_path(baseline_path);
            write_report(&proposed_path, &report)?;
            eprintln!("first run — wrote .proposed; commit as baseline once verified.");
            println!("{}", serde_json::to_string_pretty(&report)?);
            return Ok(());
        }

        let baseline = read_baseline(baseline_path)?;
        validate_baseline_contract(&baseline, &report, baseline_path)?;
        enforce_budgets(&baseline).context("baseline contains failing Stream G measurements")?;
        enforce_budgets(&report)?;
    }

    if let Some(output_path) = args.output.as_ref() {
        guard_baseline_path(output_path)?;
        enforce_budgets(&report)?;
        let destination = output_destination(output_path, args.promote_canonical);
        write_report(&destination, &report)?;
        if args.promote_canonical {
            eprintln!("promoted canonical Stream G benchmark output to {}", destination.display());
        } else {
            eprintln!(
                "wrote proposed Stream G benchmark output to {}; rerun with --promote-canonical from a human shell to update {}",
                destination.display(),
                output_path.display()
            );
        }
    }

    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn validate_mode(args: &Args) -> anyhow::Result<()> {
    if args.profile.trim().is_empty() {
        bail!("--profile requires a non-empty value");
    }
    if args.assert == args.output.is_some() {
        bail!("choose exactly one mode: --assert with --baseline, or --output <path> [--promote-canonical]");
    }
    if args.assert && args.baseline.is_none() {
        bail!("--assert requires --baseline <path>");
    }
    if !args.assert && args.baseline.is_some() {
        bail!("--baseline is only valid with --assert");
    }
    if args.promote_canonical && args.output.is_none() {
        bail!("--promote-canonical requires --output <path>");
    }
    Ok(())
}

async fn run_benchmarks(profile: &str) -> anyhow::Result<BenchReport> {
    let scoring_fixture = ScoringFixture::new().await?;
    let tui_fixture = SyntheticTuiFixture::new();
    let entity_graph = EntityGraphPayload::fixture(ENTITY_GRAPH_NODE_COUNT);
    let status = StatusPayload::fixture(run_instant()?);

    let measurements = vec![
        measure_scoring_10k(&scoring_fixture)?,
        measure_top_n_selection()?,
        measure_session_resume()?,
        measure_tui_panel_switch(&tui_fixture)?,
        measure_tui_detail_modal(&tui_fixture)?,
        measure_tui_typeahead(&tui_fixture)?,
        measure_entity_graph_serialization(&entity_graph)?,
        measure_status_serialization(&status)?,
        measure_passive_queue_append()?,
        measure_slack_dispatch().await?,
    ];

    Ok(BenchReport {
        schema_version: 1,
        fixture_version: FIXTURE_VERSION.to_owned(),
        profile: profile.to_owned(),
        runs: 1,
        platform: PlatformReport { os: std::env::consts::OS.to_owned(), arch: std::env::consts::ARCH.to_owned() },
        fixture: FixtureReport {
            run_date: RUN_DATE.to_owned(),
            scoring_memory_count: SCORING_MEMORY_COUNT,
            entity_graph_node_count: ENTITY_GRAPH_NODE_COUNT,
            tui_panel_count: 8,
            typeahead_debounce_ms: TUI_TYPEAHEAD_DEBOUNCE_MS,
            passive_queue_sample_count: PASSIVE_QUEUE_SAMPLE_COUNT,
            slack_dispatch_sample_count: SLACK_DISPATCH_SAMPLE_COUNT,
        },
        measurements,
    })
}

fn measure_scoring_10k(fixture: &ScoringFixture) -> anyhow::Result<BenchMeasurement> {
    let now = run_instant()?;
    let mut durations = Vec::with_capacity(SCORING_SAMPLE_COUNT);
    let mut result_count = 0usize;
    let config = ScoringConfig::with_top_n(SCORING_MEMORY_COUNT);

    for _ in 0..SCORING_SAMPLE_COUNT {
        let started = Instant::now();
        let scored = score_memories_at(&fixture.rows, &fixture.substrate, &config, now)?;
        durations.push(started.elapsed());
        result_count = scored.len();
        black_box(score_checksum(&scored));
    }

    let mut details = BTreeMap::new();
    details.insert("input_memory_count".to_owned(), json!(fixture.rows.len()));
    details.insert("scored_memory_count".to_owned(), json!(result_count));
    details.insert("event_log_recall_hits".to_owned(), json!(fixture.recall_hit_count));
    details.insert("supersession_edges".to_owned(), json!(fixture.supersession_count));
    details.insert("implementation_path".to_owned(), json!("memoryd::reality_check::score_memories_at"));
    Ok(budgeted_measurement(
        MeasurementInput {
            name: "scoring_10k_memories",
            description: "Reality Check score computation over 10,000 indexed active/pinned memories.",
            statistic: Statistic::P95,
            durations,
            details,
        },
        SCORING_BUDGET_MS,
    ))
}

fn measure_top_n_selection() -> anyhow::Result<BenchMeasurement> {
    let fixture = top_n_fixture();
    let mut durations = Vec::with_capacity(TOP_N_SAMPLE_COUNT);
    let mut selected_count = 0usize;

    for _ in 0..TOP_N_SAMPLE_COUNT {
        let mut candidates = fixture.clone();
        let started = Instant::now();
        candidates.sort_by(compare_top_n_candidates);
        let selected = candidates.into_iter().take(DEFAULT_TOP_N).collect::<Vec<_>>();
        durations.push(started.elapsed());
        selected_count = selected.len();
        black_box(selected);
    }

    let mut details = BTreeMap::new();
    details.insert("candidate_count".to_owned(), json!(fixture.len()));
    details.insert("selected_count".to_owned(), json!(selected_count));
    details.insert("selection".to_owned(), json!("sort_descending_score_then_take_default_top_n"));
    Ok(budgeted_measurement(
        MeasurementInput {
            name: "top_n_selection_10k",
            description: "Top-N sort and take over 10,000 pre-scored memories.",
            statistic: Statistic::P95,
            durations,
            details,
        },
        TOP_N_BUDGET_MS,
    ))
}

fn measure_session_resume() -> anyhow::Result<BenchMeasurement> {
    let temp = tempfile::tempdir()?;
    let now = run_instant()?;
    let session = BenchSessionState {
        version: 1,
        session_id: "rcs_stream_g_bench".to_owned(),
        started_at: now,
        items_total: SCORING_MEMORY_COUNT,
        items_reviewed: (0..64).map(memory_id).collect(),
        items_deferred: (64..96).map(memory_id).collect(),
        items_remaining: (96..SCORING_MEMORY_COUNT).map(memory_id).collect(),
        current_index: 96,
    };
    let session_path = temp.path().join("state").join("reality-check-session.json");
    std::fs::create_dir_all(session_path.parent().context("session path has parent")?)?;
    std::fs::write(&session_path, format!("{}\n", serde_json::to_string_pretty(&session)?))?;

    let mut durations = Vec::with_capacity(SESSION_RESUME_SAMPLE_COUNT);
    let mut remaining_count = 0usize;
    for _ in 0..SESSION_RESUME_SAMPLE_COUNT {
        let started = Instant::now();
        let bytes = std::fs::read(&session_path)?;
        let loaded = serde_json::from_slice::<BenchSessionState>(&bytes)?;
        durations.push(started.elapsed());
        if now.signed_duration_since(loaded.started_at) > chrono::Duration::days(7) {
            bail!("bench session fixture unexpectedly expired");
        }
        remaining_count = loaded.items_remaining.len();
        black_box(loaded);
    }

    let mut details = BTreeMap::new();
    details.insert("items_total".to_owned(), json!(session.items_total));
    details.insert("items_remaining".to_owned(), json!(remaining_count));
    details.insert("state_file".to_owned(), json!("reality-check-session.json"));
    Ok(budgeted_measurement(
        MeasurementInput {
            name: "session_resume_from_persisted_state",
            description: "Deserialize a persisted Reality Check session with 10k item ids.",
            statistic: Statistic::P95,
            durations,
            details,
        },
        SESSION_RESUME_BUDGET_MS,
    ))
}

fn measure_tui_panel_switch(fixture: &SyntheticTuiFixture) -> anyhow::Result<BenchMeasurement> {
    let mut app = fixture.app.clone();
    let mut durations = Vec::with_capacity(TUI_SAMPLE_COUNT);
    let mut rendered_bytes = 0usize;

    for index in 0..TUI_SAMPLE_COUNT {
        let panel = index % fixture.panel_count;
        let started = Instant::now();
        rendered_bytes = app.switch_panel(panel);
        durations.push(started.elapsed());
        black_box(rendered_bytes);
    }

    let mut details = BTreeMap::new();
    details.insert("panel_count".to_owned(), json!(fixture.panel_count));
    details.insert("rendered_bytes_last_frame".to_owned(), json!(rendered_bytes));
    details.insert("fixture".to_owned(), json!("synthetic in-process key event to frame render"));
    Ok(budgeted_measurement(
        MeasurementInput {
            name: "tui_panel_switch",
            description: "Synthetic TUI key-event-to-frame panel switch latency.",
            statistic: Statistic::P95,
            durations,
            details,
        },
        TUI_PANEL_SWITCH_BUDGET_MS,
    ))
}

fn measure_tui_detail_modal(fixture: &SyntheticTuiFixture) -> anyhow::Result<BenchMeasurement> {
    let app = fixture.app.clone();
    let mut durations = Vec::with_capacity(TUI_SAMPLE_COUNT);
    let mut rendered_bytes = 0usize;

    for index in 0..TUI_SAMPLE_COUNT {
        let memory_id = memory_id(index % fixture.detail_count);
        let started = Instant::now();
        rendered_bytes = app.open_detail_modal(&memory_id);
        durations.push(started.elapsed());
        black_box(rendered_bytes);
    }

    let mut details = BTreeMap::new();
    details.insert("detail_records".to_owned(), json!(fixture.detail_count));
    details.insert("rendered_bytes_last_modal".to_owned(), json!(rendered_bytes));
    details.insert("fixture".to_owned(), json!("synthetic trust artifact fetch from in-memory daemon fixture"));
    Ok(budgeted_measurement(
        MeasurementInput {
            name: "tui_detail_modal_open",
            description: "Synthetic memory detail modal open round-trip.",
            statistic: Statistic::P95,
            durations,
            details,
        },
        TUI_DETAIL_MODAL_BUDGET_MS,
    ))
}

fn measure_tui_typeahead(fixture: &SyntheticTuiFixture) -> anyhow::Result<BenchMeasurement> {
    let app = fixture.app.clone();
    let queries = ["stream", "stream g", "entity-4", "namespace:project", "memory"];
    let mut durations = Vec::with_capacity(TUI_SAMPLE_COUNT);
    let mut result_count = 0usize;

    for index in 0..TUI_SAMPLE_COUNT {
        let started = Instant::now();
        result_count = app.typeahead(queries[index % queries.len()]);
        let measured = started.elapsed() + Duration::from_millis(TUI_TYPEAHEAD_DEBOUNCE_MS);
        durations.push(measured);
        black_box(result_count);
    }

    let mut details = BTreeMap::new();
    details.insert("entity_records".to_owned(), json!(fixture.entity_count));
    details.insert("debounce_window_ms".to_owned(), json!(TUI_TYPEAHEAD_DEBOUNCE_MS));
    details.insert("last_result_count".to_owned(), json!(result_count));
    details.insert("fixture".to_owned(), json!("synthetic typeahead filter + render including debounce window"));
    Ok(budgeted_measurement(
        MeasurementInput {
            name: "tui_entity_typeahead",
            description: "Synthetic entity search typeahead latency including debounce.",
            statistic: Statistic::P95,
            durations,
            details,
        },
        TUI_TYPEAHEAD_BUDGET_MS,
    ))
}

fn measure_entity_graph_serialization(graph: &EntityGraphPayload) -> anyhow::Result<BenchMeasurement> {
    let mut durations = Vec::with_capacity(ENTITY_GRAPH_SAMPLE_COUNT);
    let mut serialized_bytes = 0usize;

    for _ in 0..ENTITY_GRAPH_SAMPLE_COUNT {
        let started = Instant::now();
        let bytes = serde_json::to_vec(graph)?;
        durations.push(started.elapsed());
        serialized_bytes = bytes.len();
        black_box(bytes);
    }

    let mut details = BTreeMap::new();
    details.insert("node_count".to_owned(), json!(graph.nodes.len()));
    details.insert("edge_count".to_owned(), json!(graph.edges.len()));
    details.insert("serialized_bytes".to_owned(), json!(serialized_bytes));
    details.insert("measurement".to_owned(), json!("server-side serde_json serialization"));
    Ok(budgeted_measurement(
        MeasurementInput {
            name: "web_entity_graph_serialization_5k",
            description: "Serialize web entity graph response with 5,000 nodes.",
            statistic: Statistic::P95,
            durations,
            details,
        },
        ENTITY_GRAPH_BUDGET_MS,
    ))
}

fn measure_status_serialization(status: &StatusPayload) -> anyhow::Result<BenchMeasurement> {
    let mut durations = Vec::with_capacity(STATUS_SAMPLE_COUNT);
    let mut serialized_bytes = 0usize;

    for _ in 0..STATUS_SAMPLE_COUNT {
        let started = Instant::now();
        let bytes = serde_json::to_vec(status)?;
        durations.push(started.elapsed());
        serialized_bytes = bytes.len();
        black_box(bytes);
    }

    let mut details = BTreeMap::new();
    details.insert("serialized_bytes".to_owned(), json!(serialized_bytes));
    details.insert("measurement".to_owned(), json!("GET /api/status payload serialization with mock daemon response"));
    Ok(budgeted_measurement(
        MeasurementInput {
            name: "web_status_p99",
            description: "Serialize web status response payload under no load with mock daemon data.",
            statistic: Statistic::P99,
            durations,
            details,
        },
        STATUS_BUDGET_MS,
    ))
}

fn measure_passive_queue_append() -> anyhow::Result<BenchMeasurement> {
    let queue = PassiveQueue::new();
    let now = run_instant()?;
    let mut durations = Vec::with_capacity(PASSIVE_QUEUE_SAMPLE_COUNT);

    for index in 0..PASSIVE_QUEUE_SAMPLE_COUNT {
        let started = Instant::now();
        queue.append_at(format!("Stream G passive notification {index}"), now);
        durations.push(started.elapsed());
    }
    let entries = queue.entries();
    let retained_count = entries.len();
    let checksum = entries.iter().fold(0usize, |sum, entry| {
        sum.wrapping_add(entry.message.len()).wrapping_add(entry.created_at.timestamp() as usize)
    });
    black_box(checksum);

    let mut details = BTreeMap::new();
    details.insert("append_count".to_owned(), json!(PASSIVE_QUEUE_SAMPLE_COUNT));
    details.insert("retained_capacity".to_owned(), json!(retained_count));
    details.insert("implementation_path".to_owned(), json!("memoryd::notifications::PassiveQueue::append_at"));
    Ok(budgeted_measurement(
        MeasurementInput {
            name: "passive_notification_queue_append",
            description: "In-process passive notification queue append latency.",
            statistic: Statistic::P95,
            durations,
            details,
        },
        PASSIVE_QUEUE_BUDGET_MS,
    ))
}

async fn measure_slack_dispatch() -> anyhow::Result<BenchMeasurement> {
    let mock = LocalSlackMock::default();
    let event = BenchNotificationEvent::RealityCheckDue { due_at: run_instant()? };
    let mut durations = Vec::with_capacity(SLACK_DISPATCH_SAMPLE_COUNT);

    for _ in 0..SLACK_DISPATCH_SAMPLE_COUNT {
        let started = Instant::now();
        dispatch_slack_mock(&mock, &event).await?;
        durations.push(started.elapsed());
    }

    let mut details = BTreeMap::new();
    details.insert("attempts".to_owned(), json!(mock.attempts()));
    details.insert("retry_max".to_owned(), json!(1));
    details.insert("passive_failures".to_owned(), json!(0));
    details.insert("fixture".to_owned(), json!("local in-process SlackWebhook mock"));
    Ok(budgeted_measurement(
        MeasurementInput {
            name: "slack_mock_first_dispatch",
            description: "External Slack notification first dispatch through local mock webhook.",
            statistic: Statistic::P95,
            durations,
            details,
        },
        SLACK_DISPATCH_BUDGET_MS,
    ))
}

struct MeasurementInput<'a> {
    name: &'a str,
    description: &'a str,
    statistic: Statistic,
    durations: Vec<Duration>,
    details: BTreeMap<String, Value>,
}

fn budgeted_measurement(input: MeasurementInput<'_>, budget_ms: f64) -> BenchMeasurement {
    let measured_ms = round3(millis(percentile(input.durations.clone(), input.statistic)));
    BenchMeasurement {
        name: input.name.to_owned(),
        description: input.description.to_owned(),
        statistic: input.statistic,
        measured_ms,
        budget_ms,
        budget_operator: BudgetOperator::LessThanOrEqual,
        sample_count: input.durations.len(),
        pass: measured_ms <= budget_ms,
        details: input.details,
    }
}

fn percentile(mut durations: Vec<Duration>, statistic: Statistic) -> Duration {
    durations.sort_unstable();
    let quantile = match statistic {
        Statistic::P95 => 0.95,
        Statistic::P99 => 0.99,
    };
    let index = ((durations.len().saturating_sub(1)) as f64 * quantile).ceil() as usize;
    durations[index.min(durations.len().saturating_sub(1))]
}

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

fn round3(value: f64) -> f64 {
    (value * 1_000.0).round() / 1_000.0
}

fn enforce_budgets(report: &BenchReport) -> anyhow::Result<()> {
    let failures = report
        .measurements
        .iter()
        .filter(|measurement| !measurement.pass)
        .map(|measurement| {
            format!(
                "{} {:?}={}ms budget<= {}ms samples={}",
                measurement.name,
                measurement.statistic,
                measurement.measured_ms,
                measurement.budget_ms,
                measurement.sample_count
            )
        })
        .collect::<Vec<_>>();
    if failures.is_empty() {
        Ok(())
    } else {
        bail!("Stream G benchmark budget failures:\n{}", failures.join("\n"));
    }
}

fn read_baseline(path: &Path) -> anyhow::Result<BenchReport> {
    let text = std::fs::read_to_string(path).with_context(|| format!("read baseline {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parse baseline {}", path.display()))
}

fn write_report(path: &Path, report: &BenchReport) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    std::fs::write(path, format!("{}\n", serde_json::to_string_pretty(report)?))
        .with_context(|| format!("write {}", path.display()))
}

fn baseline_requires_bootstrap(path: &Path) -> anyhow::Result<bool> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(true),
        Err(error) => return Err(error).with_context(|| format!("read baseline {}", path.display())),
    };
    let value = serde_json::from_str::<Value>(&text).with_context(|| format!("parse baseline {}", path.display()))?;
    Ok(value.get("runs").and_then(Value::as_u64).is_some_and(|runs| runs == 0))
}

fn proposed_baseline_path(path: &Path) -> PathBuf {
    let mut proposed = path.as_os_str().to_os_string();
    proposed.push(".proposed");
    PathBuf::from(proposed)
}

fn output_destination(path: &Path, promote_canonical: bool) -> PathBuf {
    if promote_canonical {
        path.to_path_buf()
    } else {
        proposed_baseline_path(path)
    }
}

fn guard_baseline_path(path: &Path) -> anyhow::Result<()> {
    if path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("baseline.") && name.ends_with(".json"))
    {
        bail!("refusing to write Stream G output to immutable baseline path {}", path.display());
    }
    Ok(())
}

fn validate_baseline_contract(baseline: &BenchReport, current: &BenchReport, path: &Path) -> anyhow::Result<()> {
    if baseline.schema_version != current.schema_version {
        bail!("baseline {} schema_version mismatch", path.display());
    }
    if baseline.fixture_version != current.fixture_version {
        bail!("baseline {} fixture_version mismatch", path.display());
    }
    if baseline.profile != current.profile {
        bail!("baseline {} profile mismatch: {} != {}", path.display(), baseline.profile, current.profile);
    }
    if baseline.runs == 0 {
        bail!("baseline {} is a placeholder; rerun assert to emit .proposed", path.display());
    }
    if baseline.fixture != current.fixture {
        bail!("baseline {} fixture shape does not match current Stream G fixture", path.display());
    }

    let baseline_contract = measurement_contract(&baseline.measurements);
    let current_contract = measurement_contract(&current.measurements);
    if baseline_contract != current_contract {
        bail!("baseline {} measurement contract does not match current Stream G fixture", path.display());
    }
    Ok(())
}

fn measurement_contract(measurements: &[BenchMeasurement]) -> Vec<(String, Statistic, f64)> {
    measurements
        .iter()
        .map(|measurement| (measurement.name.clone(), measurement.statistic, measurement.budget_ms))
        .collect()
}

struct ScoringFixture {
    rows: Vec<RecallIndexRow>,
    recall_hit_count: usize,
    supersession_count: usize,
    substrate: Substrate,
    _temp: tempfile::TempDir,
}

impl ScoringFixture {
    async fn new() -> anyhow::Result<Self> {
        let temp = tempfile::tempdir()?;
        let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
        let substrate = Substrate::init(
            roots.clone(),
            InitOptions { force_unsafe_durability: true, device_id: Some("dev_streamgbench".to_owned()) },
        )
        .await?;
        let index_path = roots.runtime.join("index.sqlite");
        let index = Index::new(open_index(&index_path)?);
        let now = run_instant()?;
        let rows = (0..SCORING_MEMORY_COUNT).map(|index| recall_row(index, now)).collect::<Vec<_>>();
        let (recall_hit_count, supersession_count) = insert_scoring_rows(&index, &rows, now)?;
        Ok(Self { rows, recall_hit_count, supersession_count, substrate, _temp: temp })
    }
}

fn score_checksum(scored: &[ScoredMemory]) -> usize {
    scored.iter().fold(0usize, |sum, item| {
        let recalled = item.last_recalled_at.map_or(0, |value| value.timestamp() as usize);
        sum.wrapping_add(item.memory_id.as_str().len())
            .wrapping_add((item.score * 1_000.0) as usize)
            .wrapping_add((item.component_scores.sensitivity_weight * 10.0) as usize)
            .wrapping_add(item.recall_count_30d as usize)
            .wrapping_add(recalled)
            .wrapping_add(item.last_observed_at.timestamp() as usize)
            .wrapping_add(item.encrypted as usize)
    })
}

fn insert_scoring_rows(index: &Index, rows: &[RecallIndexRow], now: DateTime<Utc>) -> anyhow::Result<(usize, usize)> {
    let connection = index.connection();
    connection.execute_batch("BEGIN IMMEDIATE TRANSACTION")?;

    let result = (|| -> anyhow::Result<(usize, usize)> {
        let mut memory_statement = connection.prepare(
            "INSERT OR REPLACE INTO memories(
                id, path, schema_version, type, scope, namespace, canonical_namespace_id,
                summary, confidence, original_confidence, trust_level, sensitivity, status, review_state,
                requires_user_confirmation, created_at, updated_at,
                observed_at, valid_from, valid_until, ttl,
                author, source_kind, source_harness, source_device,
                body_hash, frontmatter_json, file_hash, file_mtime_ns, indexed_at, metadata_only,
                passive_recall, index_body, human_review_required, max_scope
             ) VALUES (
                ?1, ?2, 1, 'pattern', ?3, NULL, ?4,
                ?5, ?6, ?7, 'trusted', ?8, ?9, NULL,
                0, ?10, ?11,
                ?12, NULL, NULL, NULL,
                'agent', 'agent-primary', ?13, ?14,
                ?15, '{}', ?16, 0, ?17, ?18,
                ?19, ?20, 0, ?21
             )",
        )?;
        let mut event_statement = connection.prepare(
            "INSERT OR REPLACE INTO events_log(event_id, device, seq, kind, memory_id, ts, payload_json)
             VALUES (?1, 'dev_streamgbench', ?2, 'recall_hit', ?3, ?4, '{}')",
        )?;
        let mut supersession_statement = connection
            .prepare("INSERT OR IGNORE INTO memory_supersession(memory_id, supersedes_id) VALUES (?1, ?2)")?;

        let mut recall_hit_count = 0usize;
        let mut supersession_count = 0usize;
        for (index, row) in rows.iter().enumerate() {
            let observed_at = now - chrono::Duration::days((index % 120) as i64);
            let source_harness = source_harness(index);
            memory_statement.execute(params![
                row.id.as_str(),
                row.path.as_str(),
                scope_str(row.scope),
                row.canonical_namespace_id.as_deref(),
                row.summary.as_str(),
                row.confidence,
                original_confidence(index),
                sensitivity_str(row.sensitivity),
                status_str(row.status),
                row.updated_at.to_rfc3339(),
                row.updated_at.to_rfc3339(),
                observed_at.to_rfc3339(),
                source_harness,
                source_device(index),
                format!("body_hash_{index:08}"),
                format!("file_hash_{index:08}"),
                row.indexed_at.to_rfc3339(),
                metadata_only(index) as i64,
                row.passive_recall as i64,
                row.index_body as i64,
                scope_str(row.max_scope),
            ])?;

            for hit_index in 0..(index % 7) {
                event_statement.execute((
                    format!("evt_stream_g_{index:05}_{hit_index:02}"),
                    recall_hit_count as i64,
                    row.id.as_str(),
                    (now - chrono::Duration::days(hit_index as i64)).to_rfc3339(),
                ))?;
                recall_hit_count += 1;
            }

            if index % 4 == 0 && index > 0 {
                let supersedes_index = index - 1;
                if supersedes_index != index {
                    supersession_statement.execute((row.id.as_str(), rows[supersedes_index].id.as_str()))?;
                    supersession_count += 1;
                }
            }
        }
        Ok((recall_hit_count, supersession_count))
    })();

    match result {
        Ok(counts) => {
            connection.execute_batch("COMMIT")?;
            Ok(counts)
        }
        Err(error) => {
            let _ = connection.execute_batch("ROLLBACK");
            Err(error)
        }
    }
}

fn recall_row(index: usize, now: DateTime<Utc>) -> RecallIndexRow {
    let id = memory_id(index);
    let scope = match index % 3 {
        0 => Scope::User,
        1 => Scope::Project,
        _ => Scope::Agent,
    };
    RecallIndexRow {
        id: MemoryId::new(id.clone()),
        path: RepoPath::new(format!("me/{id}.md")),
        summary: format!("Stream G deterministic scoring fixture memory {index}"),
        status: if index % 199 == 0 { MemoryStatus::Pinned } else { MemoryStatus::Active },
        scope,
        canonical_namespace_id: (scope == Scope::Project).then(|| "agent-memory".to_owned()),
        updated_at: now - chrono::Duration::hours((index % 240) as i64),
        indexed_at: now,
        confidence: 0.45 + f64::from((index % 50) as u32) / 100.0,
        source_kind: SourceKind::AgentPrimary,
        source_device: source_device(index).map(str::to_owned),
        source_harness: None,
        source_session_id: None,
        author_harness: None,
        author_session_id: None,
        sensitivity: sensitivity_for_index(index),
        passive_recall: true,
        index_body: !metadata_only(index),
        requires_user_confirmation: false,
        review_state: None,
        human_review_required: false,
        max_scope: scope,
        merge_diagnostics_json: None,
        tags: vec!["stream-g".to_owned(), format!("bucket-{}", index % 32)],
        aliases: vec![format!("stream-g-bench-{index}")],
        entities: vec![memory_substrate::Entity {
            id: format!("ent_stream_g_{:04}", index % 256),
            label: format!("Stream G Entity {}", index % 256),
            aliases: vec![format!("stream-g-entity-{}", index % 256)],
        }],
    }
}

fn sensitivity_for_index(index: usize) -> Sensitivity {
    match index % 4 {
        0 => Sensitivity::Public,
        1 => Sensitivity::Internal,
        2 => Sensitivity::Confidential,
        _ => Sensitivity::Personal,
    }
}

fn metadata_only(index: usize) -> bool {
    index % 23 == 0
}

fn original_confidence(index: usize) -> Option<f64> {
    (index % 3 == 0).then(|| 0.65 + f64::from((index % 20) as u32) / 100.0)
}

fn source_harness(index: usize) -> Option<&'static str> {
    match index % 5 {
        0 => None,
        1 | 3 => Some("codex"),
        _ => Some("claude-code"),
    }
}

fn source_device(index: usize) -> Option<&'static str> {
    match index % 4 {
        0 => Some("macbook-pro"),
        1 => Some("mac-studio"),
        _ => None,
    }
}

fn scope_str(value: Scope) -> &'static str {
    match value {
        Scope::User => "user",
        Scope::Project => "project",
        Scope::Org => "org",
        Scope::Agent => "agent",
        Scope::Subagent => "subagent",
    }
}

fn sensitivity_str(value: Sensitivity) -> &'static str {
    match value {
        Sensitivity::Public => "public",
        Sensitivity::Internal => "internal",
        Sensitivity::Confidential => "confidential",
        Sensitivity::Personal => "personal",
    }
}

fn status_str(value: MemoryStatus) -> &'static str {
    match value {
        MemoryStatus::Candidate => "candidate",
        MemoryStatus::Active => "active",
        MemoryStatus::Pinned => "pinned",
        MemoryStatus::Superseded => "superseded",
        MemoryStatus::Archived => "archived",
        MemoryStatus::Tombstoned => "tombstoned",
        MemoryStatus::Quarantined => "quarantined",
    }
}

#[derive(Clone)]
struct TopNCandidate {
    memory_id: String,
    score: f64,
    pinned: bool,
}

fn top_n_fixture() -> Vec<TopNCandidate> {
    (0..SCORING_MEMORY_COUNT)
        .map(|index| TopNCandidate {
            memory_id: memory_id(index),
            score: f64::from(((index * 37) % 10_000) as u32) / 10_000.0,
            pinned: index % 199 == 0,
        })
        .collect()
}

fn compare_top_n_candidates(left: &TopNCandidate, right: &TopNCandidate) -> std::cmp::Ordering {
    right
        .pinned
        .cmp(&left.pinned)
        .then_with(|| right.score.total_cmp(&left.score))
        .then_with(|| left.memory_id.cmp(&right.memory_id))
}

#[derive(Clone)]
struct SyntheticTuiFixture {
    app: SyntheticTuiApp,
    panel_count: usize,
    detail_count: usize,
    entity_count: usize,
}

impl SyntheticTuiFixture {
    fn new() -> Self {
        let details = (0..256)
            .map(|index| {
                (
                    memory_id(index),
                    format!(
                        "Trust artifact for Stream G memory {index}: status=active sensitivity={} score={:.3}",
                        match index % 4 {
                            0 => "public",
                            1 => "internal",
                            2 => "confidential",
                            _ => "personal",
                        },
                        f64::from(((index * 37) % 1_000) as u32) / 1_000.0
                    ),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let entities = (0..2_048)
            .map(|index| format!("entity-{index:04} stream g namespace:project memory {}", index % 64))
            .collect::<Vec<_>>();
        Self {
            app: SyntheticTuiApp { active_panel: 0, panels: panel_names(), details, entities },
            panel_count: 8,
            detail_count: 256,
            entity_count: 2_048,
        }
    }
}

#[derive(Clone)]
struct SyntheticTuiApp {
    active_panel: usize,
    panels: Vec<&'static str>,
    details: BTreeMap<String, String>,
    entities: Vec<String>,
}

impl SyntheticTuiApp {
    fn switch_panel(&mut self, panel: usize) -> usize {
        self.active_panel = panel % self.panels.len();
        self.render_frame().len()
    }

    fn open_detail_modal(&self, memory_id: &str) -> usize {
        let detail = self.details.get(memory_id).map_or("missing detail", String::as_str);
        format!("modal title=Memory Detail id={memory_id}\n{detail}\n{}", self.render_footer()).len()
    }

    fn typeahead(&self, query: &str) -> usize {
        self.entities
            .iter()
            .filter(|entity| entity.contains(query))
            .take(16)
            .map(|entity| format!("result:{entity}\n"))
            .collect::<String>()
            .len()
    }

    fn render_frame(&self) -> String {
        format!(
            "panel={} active={} rows={} footer={}",
            self.active_panel,
            self.panels[self.active_panel],
            self.details.len(),
            self.render_footer()
        )
    }

    fn render_footer(&self) -> String {
        self.panels.iter().enumerate().map(|(index, name)| format!("[{}:{name}]", index + 1)).collect()
    }
}

fn panel_names() -> Vec<&'static str> {
    vec!["overview", "timeline", "review", "policy", "entities", "conflicts", "namespace", "reality-check"]
}

#[derive(Clone, Serialize)]
struct EntityGraphPayload {
    nodes: Vec<EntityNode>,
    edges: Vec<EntityEdge>,
}

#[derive(Clone, Serialize)]
struct EntityNode {
    id: String,
    label: String,
    namespace: String,
    memory_count: u32,
}

#[derive(Clone, Serialize)]
struct EntityEdge {
    source: String,
    target: String,
    kind: String,
    weight: f64,
    temporal_from: Option<String>,
    temporal_to: Option<String>,
}

impl EntityGraphPayload {
    fn fixture(node_count: usize) -> Self {
        let nodes = (0..node_count)
            .map(|index| EntityNode {
                id: format!("ent_stream_g_{index:04}"),
                label: format!("Stream G Entity {index}"),
                namespace: format!("project:agent-memory:bucket-{}", index % 32),
                memory_count: (1 + index % 128) as u32,
            })
            .collect::<Vec<_>>();
        let edges = (0..node_count.saturating_sub(1))
            .map(|index| EntityEdge {
                source: format!("ent_stream_g_{index:04}"),
                target: format!("ent_stream_g_{:04}", index + 1),
                kind: if index % 11 == 0 { "supersedes" } else { "co_mentioned" }.to_owned(),
                weight: f64::from(((index * 17) % 100) as u32) / 100.0,
                temporal_from: (index % 11 == 0).then(|| "2026-05-01".to_owned()),
                temporal_to: None,
            })
            .collect::<Vec<_>>();
        Self { nodes, edges }
    }
}

#[derive(Clone, Serialize)]
struct StatusPayload {
    daemon: DaemonStatusPayload,
    socket: String,
    index: IndexStatusPayload,
    sync: SyncStatusPayload,
    review: ReviewStatusPayload,
    conflicts: u32,
    active_sessions: Vec<ActiveSessionPayload>,
    dreaming: DreamingStatusPayload,
    recall: RecallStatusPayload,
}

#[derive(Clone, Serialize)]
struct DaemonStatusPayload {
    version: String,
    pid: u32,
    uptime_seconds: u64,
}

#[derive(Clone, Serialize)]
struct IndexStatusPayload {
    active_memories: u64,
    last_reindex: DateTime<Utc>,
}

#[derive(Clone, Serialize)]
struct SyncStatusPayload {
    ahead: u32,
    behind: u32,
    last_push: DateTime<Utc>,
    remote: String,
}

#[derive(Clone, Serialize)]
struct ReviewStatusPayload {
    candidate: u32,
    quarantined: u32,
    dream_low_confidence: u32,
}

#[derive(Clone, Serialize)]
struct ActiveSessionPayload {
    harness: String,
    session_id: String,
}

#[derive(Clone, Serialize)]
struct DreamingStatusPayload {
    status: String,
    next_run: DateTime<Utc>,
    last_run: DreamRunSummaryPayload,
}

#[derive(Clone, Serialize)]
struct DreamRunSummaryPayload {
    at: DateTime<Utc>,
    promoted: u32,
    queued: u32,
    dropped: u32,
}

#[derive(Clone, Serialize)]
struct RecallStatusPayload {
    startup_total: u32,
    delta_total: u32,
    peer_update_total: u32,
}

impl StatusPayload {
    fn fixture(now: DateTime<Utc>) -> Self {
        Self {
            daemon: DaemonStatusPayload { version: "0.1.0-bench".to_owned(), pid: 7137, uptime_seconds: 302_440 },
            socket: "ok".to_owned(),
            index: IndexStatusPayload { active_memories: 10_000, last_reindex: now },
            sync: SyncStatusPayload {
                ahead: 2,
                behind: 0,
                last_push: now,
                remote: "git@github.com:trey/agent-memory.git".to_owned(),
            },
            review: ReviewStatusPayload { candidate: 3, quarantined: 2, dream_low_confidence: 2 },
            conflicts: 1,
            active_sessions: vec![
                ActiveSessionPayload {
                    harness: "claude-code".to_owned(),
                    session_id: "session_claude_fixture".to_owned(),
                },
                ActiveSessionPayload {
                    harness: "codex-cli".to_owned(),
                    session_id: "session_codex_fixture".to_owned(),
                },
            ],
            dreaming: DreamingStatusPayload {
                status: "scheduled".to_owned(),
                next_run: now + chrono::Duration::hours(15),
                last_run: DreamRunSummaryPayload {
                    at: now - chrono::Duration::hours(9),
                    promoted: 3,
                    queued: 1,
                    dropped: 0,
                },
            },
            recall: RecallStatusPayload { startup_total: 42, delta_total: 119, peer_update_total: 8 },
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct BenchSessionState {
    version: u32,
    session_id: String,
    started_at: DateTime<Utc>,
    items_total: usize,
    items_reviewed: Vec<String>,
    items_deferred: Vec<String>,
    items_remaining: Vec<String>,
    current_index: usize,
}

#[derive(Clone, Debug)]
enum BenchNotificationEvent {
    RealityCheckDue { due_at: DateTime<Utc> },
}

#[derive(Clone, Debug, Serialize)]
struct BenchSlackPayload {
    text: String,
    blocks: Vec<BenchSlackBlock>,
}

#[derive(Clone, Debug, Serialize)]
struct BenchSlackBlock {
    #[serde(rename = "type")]
    kind: &'static str,
    text: BenchSlackText,
}

#[derive(Clone, Debug, Serialize)]
struct BenchSlackText {
    #[serde(rename = "type")]
    kind: &'static str,
    text: String,
}

#[derive(Default)]
struct LocalSlackMock {
    attempts: AtomicUsize,
}

impl LocalSlackMock {
    fn attempts(&self) -> usize {
        self.attempts.load(Ordering::SeqCst)
    }
}

impl LocalSlackMock {
    async fn post(&self, webhook_url: &str, payload: BenchSlackPayload) -> anyhow::Result<()> {
        if webhook_url.trim().is_empty() {
            bail!("mock Slack webhook URL must be non-empty");
        }
        self.attempts.fetch_add(1, Ordering::SeqCst);
        black_box(payload);
        Ok(())
    }
}

async fn dispatch_slack_mock(mock: &LocalSlackMock, event: &BenchNotificationEvent) -> anyhow::Result<()> {
    let payload = slack_payload(event);
    mock.post("http://127.0.0.1/mock-slack", payload).await
}

fn slack_payload(event: &BenchNotificationEvent) -> BenchSlackPayload {
    let summary = match event {
        BenchNotificationEvent::RealityCheckDue { due_at } => {
            format!("Weekly Reality Check is ready at {}.", due_at.format("%Y-%m-%d %H:%M UTC"))
        }
    };
    BenchSlackPayload {
        text: format!("Memorum: {summary}"),
        blocks: vec![BenchSlackBlock {
            kind: "section",
            text: BenchSlackText {
                kind: "mrkdwn",
                text: format!(
                    "*Memorum Notification*\n{summary}\nRun `memoryd reality-check run` or open the dashboard."
                ),
            },
        }],
    }
}

fn memory_id(index: usize) -> String {
    format!("mem_20260502_a1b2c3d4e5f60718_{index:06}")
}

fn run_instant() -> anyhow::Result<DateTime<Utc>> {
    Ok(DateTime::parse_from_rfc3339(RUN_AT)?.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn output_without_promote_targets_proposed_file() {
        let args = Args::try_parse_from([
            "stream_g_bench",
            "--profile",
            "darwin-arm64",
            "--output",
            "bench/stream-g-observability-results.darwin-arm64.json",
        ])
        .expect("args parse");

        validate_mode(&args).expect("mode valid");
        assert!(!args.promote_canonical);
        assert_eq!(
            output_destination(args.output.as_deref().expect("output"), false),
            PathBuf::from("bench/stream-g-observability-results.darwin-arm64.json.proposed")
        );
    }

    #[test]
    fn promote_canonical_requires_explicit_flag() {
        let args = Args::try_parse_from([
            "stream_g_bench",
            "--profile",
            "darwin-arm64",
            "--output",
            "bench/stream-g-observability-results.darwin-arm64.json",
            "--promote-canonical",
        ])
        .expect("args parse");

        validate_mode(&args).expect("mode valid");
        assert_eq!(
            output_destination(args.output.as_deref().expect("output"), args.promote_canonical),
            PathBuf::from("bench/stream-g-observability-results.darwin-arm64.json")
        );
    }
}
