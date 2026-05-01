use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context};
use chrono::{DateTime, Utc};
use clap::Parser;
use memory_privacy::{DeterministicPrivacyClassifier, PrivacyClassifier, PrivacyNamespace, PrivacySpan};
use memory_substrate::events::{append_event, Event, EventKind, EVENT_SCHEMA_VERSION};
use memory_substrate::frontmatter::serialize_document;
use memory_substrate::{
    Author, AuthorKind, ClassificationOutcome, DeviceId, EventContext, EventId, Frontmatter, InitOptions, Memory,
    MemoryId, MemoryStatus, MemoryType, ObserveKind, OperationId, PrivacySpanRecord, RepoPath, RetrievalPolicy, Roots,
    Scope, Sensitivity, Source, SourceKind, Substrate, SubstrateFragmentAppendRequest, SubstrateFragmentPayload,
    SubstrateFragmentRecord, TrustLevel, WriteMode, WritePolicy, WriteRequest,
};
use memoryd::dream::cleanup::{run_cleanup, CleanupConfig};
use memoryd::dream::harness::EchoCli;
use memoryd::dream::lease::{acquire_manual_lease, LeaseAcquireRequest};
use memoryd::dream::run::{DreamActiveMemoryInput, DreamRunOptions, DreamRunner, DreamSubstrateFragmentInput};
use memoryd::dream::scope::DreamScope;
use memoryd::dream::types::{ActiveMemory, SubstrateFragment};
use memoryd::recall::{build_startup_response, StartupRequest};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const FIXTURE_VERSION: &str = "stream-f-dreaming-v0.2-task-15";
const RUN_DATE: &str = "2026-04-30";
const RUN_AT: &str = "2026-04-30T12:00:00Z";

const PASS_1_FRAGMENT_COUNT: usize = 1_000;
const PASS_1_ACTIVE_MEMORY_COUNT: usize = 64;
const PROMPT_ASSEMBLY_SAMPLE_COUNT: usize = 15;

const LEASE_SAMPLE_COUNT: usize = 7;
const LEASE_BUDGET_MS: f64 = 2_000.0;

const SUBSTRATE_WRITE_SAMPLE_COUNT: usize = 41;
const SUBSTRATE_WRITE_BUDGET_MS: f64 = 5.0;

const CLEANUP_CANONICAL_MEMORY_COUNT: usize = 10_000;
const CLEANUP_SUBSTRATE_FRAGMENT_COUNT: usize = 100_000;
const CLEANUP_OLD_EVENT_COUNT: usize = 256;
const CLEANUP_LIVE_EVENT_COUNT: usize = 32;
const CLEANUP_BUDGET_MS: f64 = 60_000.0;

const RECALL_BASE_MEMORY_COUNT: usize = 80;
const RECALL_QUESTION_RECORD_COUNT: usize = 90;
const RECALL_OVERHEAD_SAMPLE_COUNT: usize = 21;
const RECALL_OVERHEAD_BUDGET_MS: f64 = 5.0;

#[derive(Debug, Parser)]
struct Args {
    /// Hardware/profile label recorded in the JSON fixture.
    #[arg(long)]
    profile: String,

    /// Assert current measurements against v0.2 budgets and an existing baseline contract.
    #[arg(long)]
    assert: bool,

    /// Existing baseline JSON to validate in assert mode.
    #[arg(long)]
    baseline: Option<PathBuf>,

    /// Explicit release/update mode. This is the only mode that writes the baseline file.
    #[arg(long)]
    write_output: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BenchReport {
    schema_version: u32,
    fixture_version: String,
    profile: String,
    platform: PlatformReport,
    fixture: FixtureReport,
    measurements: Vec<BenchMeasurement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PlatformReport {
    os: String,
    arch: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FixtureReport {
    run_date: String,
    pass_1_fragment_count: usize,
    pass_1_active_memory_count: usize,
    cleanup_canonical_memory_count: usize,
    cleanup_substrate_fragment_count: usize,
    cleanup_old_event_count: usize,
    recall_question_record_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BenchMeasurement {
    name: String,
    description: String,
    measured_p95_ms: f64,
    budget_ms: Option<f64>,
    budget_operator: Option<BudgetOperator>,
    sample_count: usize,
    pass: bool,
    details: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum BudgetOperator {
    LessThan,
    LessThanOrEqual,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    validate_mode(&args)?;

    let report = run_benchmarks(&args.profile).await?;

    if args.assert {
        let baseline_path = args.baseline.as_ref().expect("baseline is required in assert mode");
        let baseline = read_baseline(baseline_path)?;
        validate_baseline_contract(&baseline, &report, baseline_path)?;
        enforce_budgets(&baseline).context("baseline contains failing measurements")?;
        enforce_budgets(&report).context("current Stream F benchmark failed")?;
    }

    if let Some(output_path) = args.write_output.as_ref() {
        enforce_budgets(&report).context("refusing to write failing Stream F benchmark baseline")?;
        write_report(output_path, &report)?;
    }

    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn validate_mode(args: &Args) -> anyhow::Result<()> {
    if args.assert == args.write_output.is_some() {
        anyhow::bail!("choose exactly one mode: --assert with --baseline, or --write-output <path>");
    }
    if args.assert && args.baseline.is_none() {
        anyhow::bail!("--assert requires --baseline <path>");
    }
    if !args.assert && args.baseline.is_some() {
        anyhow::bail!("--baseline is only valid with --assert");
    }
    Ok(())
}

async fn run_benchmarks(profile: &str) -> anyhow::Result<BenchReport> {
    let measurements = vec![
        measure_pass_1_prompt_assembly()?,
        measure_lease_acquisition().await?,
        measure_substrate_fragment_writes().await?,
        measure_cleanup_full_pass().await?,
        measure_stream_e_dream_question_overhead().await?,
    ];

    Ok(BenchReport {
        schema_version: 1,
        fixture_version: FIXTURE_VERSION.to_owned(),
        profile: profile.to_owned(),
        platform: PlatformReport { os: std::env::consts::OS.to_owned(), arch: std::env::consts::ARCH.to_owned() },
        fixture: FixtureReport {
            run_date: RUN_DATE.to_owned(),
            pass_1_fragment_count: PASS_1_FRAGMENT_COUNT,
            pass_1_active_memory_count: PASS_1_ACTIVE_MEMORY_COUNT,
            cleanup_canonical_memory_count: CLEANUP_CANONICAL_MEMORY_COUNT,
            cleanup_substrate_fragment_count: CLEANUP_SUBSTRATE_FRAGMENT_COUNT,
            cleanup_old_event_count: CLEANUP_OLD_EVENT_COUNT,
            recall_question_record_count: RECALL_QUESTION_RECORD_COUNT,
        },
        measurements,
    })
}

fn measure_pass_1_prompt_assembly() -> anyhow::Result<BenchMeasurement> {
    let options = pass_1_options()?;
    let mut durations = Vec::with_capacity(PROMPT_ASSEMBLY_SAMPLE_COUNT);
    let mut prompt_bytes = 0usize;

    for _ in 0..PROMPT_ASSEMBLY_SAMPLE_COUNT {
        let started = Instant::now();
        let prompt = DreamRunner::<memoryd::dream::run::NoopCandidateWriter>::preview_pass_1_prompt(&options)
            .map_err(|err| anyhow!("pass 1 prompt assembly failed: {err}"))?;
        durations.push(started.elapsed());
        prompt_bytes = prompt.len();
    }

    let mut details = BTreeMap::new();
    details.insert("fragment_count".to_owned(), json!(PASS_1_FRAGMENT_COUNT));
    details.insert("active_memory_count".to_owned(), json!(PASS_1_ACTIVE_MEMORY_COUNT));
    details.insert("prompt_bytes".to_owned(), json!(prompt_bytes));
    Ok(informational_measurement(MeasurementInput {
        name: "pass_1_prompt_assembly_1k_fragments",
        description: "1k-fragment Pass 1 prompt assembly.",
        durations,
        details,
    }))
}

async fn measure_lease_acquisition() -> anyhow::Result<BenchMeasurement> {
    let fixture = GitLeaseFixture::new().await?;
    let mut durations = Vec::with_capacity(LEASE_SAMPLE_COUNT);

    for index in 0..LEASE_SAMPLE_COUNT {
        let started = Instant::now();
        acquire_manual_lease(LeaseAcquireRequest {
            repo: fixture.repo.clone(),
            runtime: fixture.runtime.clone(),
            scope: format!("project:benchlease{index}"),
            force: false,
            now: instant(RUN_AT) + chrono::Duration::seconds(index as i64),
            lease_window_seconds: 60,
            cli_used: Some("echo".to_owned()),
        })
        .with_context(|| format!("lease acquisition sample {index} failed"))?;
        durations.push(started.elapsed());
    }

    let mut details = BTreeMap::new();
    details.insert("git_remote".to_owned(), json!("local_bare_origin"));
    details.insert("lease_records_written".to_owned(), json!(LEASE_SAMPLE_COUNT));
    Ok(budgeted_measurement(
        MeasurementInput {
            name: "lease_acquisition",
            description: "Manual lease acquisition with local git fetch/read/append/commit/push.",
            durations,
            details,
        },
        BudgetThreshold { ms: LEASE_BUDGET_MS, operator: BudgetOperator::LessThan },
    ))
}

async fn measure_substrate_fragment_writes() -> anyhow::Result<BenchMeasurement> {
    let temp = tempfile::tempdir()?;
    let substrate = init_substrate(temp.path(), "dev_observebench").await?;
    let classifier = DeterministicPrivacyClassifier::new();
    let mut durations = Vec::with_capacity(SUBSTRATE_WRITE_SAMPLE_COUNT);

    for index in 0..SUBSTRATE_WRITE_SAMPLE_COUNT {
        let text = format!("deterministic Stream F observe fixture {index} for auth rotation");
        let privacy = classifier.classify(&text, PrivacyNamespace::Agent, None)?;
        if privacy.storage_action.refuses_storage() || privacy.storage_action.requires_encryption() {
            anyhow::bail!("trusted write fixture unexpectedly required non-plaintext storage");
        }
        let started = Instant::now();
        substrate
            .append_substrate_fragment(SubstrateFragmentAppendRequest {
                id: Some(substrate_id(200_000 + index)),
                at: instant(RUN_AT) + chrono::Duration::milliseconds(index as i64),
                session: Some("sess_stream_f_bench".to_owned()),
                harness: Some("codex".to_owned()),
                scope: "project:proj_stream_f_bench".to_owned(),
                entities: vec!["ent_auth_flow".to_owned()],
                kind: ObserveKind::Pattern,
                source_ref: Some(format!("session:sess_stream_f_bench:turn:{index}")),
                privacy_spans: privacy_span_records(&privacy.spans)?,
                payload: SubstrateFragmentPayload::Plaintext { text },
                classification: ClassificationOutcome::Trusted,
                operation_id: None,
            })
            .await
            .with_context(|| format!("substrate fragment append sample {index} failed"))?;
        durations.push(started.elapsed());
    }

    let mut details = BTreeMap::new();
    details.insert("write_surface".to_owned(), json!("Substrate::append_substrate_fragment public write path"));
    details.insert("classification".to_owned(), json!("trusted_plaintext"));
    details.insert(
        "durability_mode".to_owned(),
        json!("best_effort_fixture; full-durability repositories still fsync append and event records"),
    );
    Ok(budgeted_measurement(
        MeasurementInput {
            name: "substrate_fragment_write_memory_observe",
            description: "Substrate-fragment write throughput through the public append API.",
            durations,
            details,
        },
        BudgetThreshold { ms: SUBSTRATE_WRITE_BUDGET_MS, operator: BudgetOperator::LessThan },
    ))
}

async fn measure_cleanup_full_pass() -> anyhow::Result<BenchMeasurement> {
    let fixture = CleanupFixture::new().await?;
    let config = CleanupConfig {
        device_id: "dev_cleanupbench".to_owned(),
        now: instant(RUN_AT),
        fragment_lifetime_days: 14,
        candidate_stale_days: 30,
        event_compaction_days: 90,
    };

    let started = Instant::now();
    let report = run_cleanup(&fixture.substrate, config).await.context("cleanup full-pass fixture failed")?;
    let duration = started.elapsed();

    let mut details = BTreeMap::new();
    details.insert("canonical_memory_count".to_owned(), json!(CLEANUP_CANONICAL_MEMORY_COUNT));
    details.insert("substrate_fragment_count".to_owned(), json!(CLEANUP_SUBSTRATE_FRAGMENT_COUNT));
    details.insert("fragments_archived".to_owned(), json!(report.operations.fragments_archived));
    details.insert("entity_index_rows".to_owned(), json!(report.operations.entity_index_rows));
    details.insert("events_compacted".to_owned(), json!(report.operations.events_compacted));
    details.insert("commit_deferred".to_owned(), json!(report.commit_deferred));
    Ok(budgeted_measurement(
        MeasurementInput {
            name: "cleanup_full_pass_representative",
            description:
                "Cleanup full pass over 10k canonical memories, 100k substrate fragments, and compactable events.",
            durations: vec![duration],
            details,
        },
        BudgetThreshold { ms: CLEANUP_BUDGET_MS, operator: BudgetOperator::LessThan },
    ))
}

async fn measure_stream_e_dream_question_overhead() -> anyhow::Result<BenchMeasurement> {
    let without_questions = RecallFixture::new(false).await?;
    let with_questions = RecallFixture::new(true).await?;
    let mut overheads = Vec::with_capacity(RECALL_OVERHEAD_SAMPLE_COUNT);
    let mut with_question_p95_samples = Vec::with_capacity(RECALL_OVERHEAD_SAMPLE_COUNT);
    let mut without_question_p95_samples = Vec::with_capacity(RECALL_OVERHEAD_SAMPLE_COUNT);

    for _ in 0..RECALL_OVERHEAD_SAMPLE_COUNT {
        let without = time_startup_recall(&without_questions.substrate, &without_questions.repo).await?;
        let with = time_startup_recall(&with_questions.substrate, &with_questions.repo).await?;
        without_question_p95_samples.push(without);
        with_question_p95_samples.push(with);
        overheads.push(with.saturating_sub(without));
    }

    let without_p95 = millis(p95(without_question_p95_samples));
    let with_p95 = millis(p95(with_question_p95_samples));
    let overhead_p95_ms = round3((with_p95 - without_p95).max(0.0));
    let mut details = BTreeMap::new();
    details.insert("base_memory_count".to_owned(), json!(RECALL_BASE_MEMORY_COUNT));
    details.insert("question_record_count".to_owned(), json!(RECALL_QUESTION_RECORD_COUNT));
    details.insert("startup_without_questions_p95_ms".to_owned(), json!(round3(without_p95)));
    details.insert("startup_with_questions_p95_ms".to_owned(), json!(round3(with_p95)));
    details.insert("paired_overhead_samples_p95_ms".to_owned(), json!(round3(millis(p95(overheads)))));
    Ok(BenchMeasurement {
        name: "stream_e_pending_attention_question_read_overhead".to_owned(),
        description: "Added Stream E startup p95 overhead from reading Pass-3 dream questions.".to_owned(),
        measured_p95_ms: overhead_p95_ms,
        budget_ms: Some(RECALL_OVERHEAD_BUDGET_MS),
        budget_operator: Some(BudgetOperator::LessThanOrEqual),
        sample_count: RECALL_OVERHEAD_SAMPLE_COUNT,
        pass: overhead_p95_ms <= RECALL_OVERHEAD_BUDGET_MS,
        details,
    })
}

async fn time_startup_recall(substrate: &Substrate, repo: &Path) -> anyhow::Result<Duration> {
    let started = Instant::now();
    let _ = build_startup_response(
        substrate,
        StartupRequest {
            cwd: repo.to_string_lossy().into_owned(),
            session_id: "sess_stream_f_bench".to_owned(),
            harness: "codex".to_owned(),
            harness_version: None,
            include_recent: true,
            since_event_id: None,
            budget_tokens: Some(3_600),
        },
    )
    .await?;
    Ok(started.elapsed())
}

fn privacy_span_records(spans: &[PrivacySpan]) -> anyhow::Result<Vec<PrivacySpanRecord>> {
    spans
        .iter()
        .map(|span| {
            let label = serde_json::to_value(span.label)
                .and_then(serde_json::from_value)
                .context("serialize privacy span label")?;
            Ok(PrivacySpanRecord { label, start: span.start, end: span.end })
        })
        .collect()
}

struct MeasurementInput<'a> {
    name: &'a str,
    description: &'a str,
    durations: Vec<Duration>,
    details: BTreeMap<String, Value>,
}

struct BudgetThreshold {
    ms: f64,
    operator: BudgetOperator,
}

fn budgeted_measurement(input: MeasurementInput<'_>, budget: BudgetThreshold) -> BenchMeasurement {
    let measured_p95_ms = round3(millis(p95(input.durations.clone())));
    let pass = match budget.operator {
        BudgetOperator::LessThan => measured_p95_ms < budget.ms,
        BudgetOperator::LessThanOrEqual => measured_p95_ms <= budget.ms,
    };
    BenchMeasurement {
        name: input.name.to_owned(),
        description: input.description.to_owned(),
        measured_p95_ms,
        budget_ms: Some(budget.ms),
        budget_operator: Some(budget.operator),
        sample_count: input.durations.len(),
        pass,
        details: input.details,
    }
}

fn informational_measurement(input: MeasurementInput<'_>) -> BenchMeasurement {
    BenchMeasurement {
        name: input.name.to_owned(),
        description: input.description.to_owned(),
        measured_p95_ms: round3(millis(p95(input.durations.clone()))),
        budget_ms: None,
        budget_operator: None,
        sample_count: input.durations.len(),
        pass: true,
        details: input.details,
    }
}

fn p95(mut durations: Vec<Duration>) -> Duration {
    durations.sort_unstable();
    let index = ((durations.len().saturating_sub(1)) as f64 * 0.95).ceil() as usize;
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
                "{} p95={}ms budget={:?} {:?}",
                measurement.name, measurement.measured_p95_ms, measurement.budget_ms, measurement.budget_operator
            )
        })
        .collect::<Vec<_>>();
    if failures.is_empty() {
        Ok(())
    } else {
        anyhow::bail!("Stream F benchmark budget failures:\n{}", failures.join("\n"));
    }
}

fn read_baseline(path: &Path) -> anyhow::Result<BenchReport> {
    let text = fs::read_to_string(path).with_context(|| format!("read baseline {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parse baseline {}", path.display()))
}

fn write_report(path: &Path, report: &BenchReport) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(path, format!("{}\n", serde_json::to_string_pretty(report)?))
        .with_context(|| format!("write {}", path.display()))
}

fn validate_baseline_contract(baseline: &BenchReport, current: &BenchReport, path: &Path) -> anyhow::Result<()> {
    if baseline.schema_version != current.schema_version {
        anyhow::bail!("baseline {} has schema_version {}", path.display(), baseline.schema_version);
    }
    if baseline.fixture_version != current.fixture_version {
        anyhow::bail!(
            "baseline {} fixture_version mismatch: {} != {}",
            path.display(),
            baseline.fixture_version,
            current.fixture_version
        );
    }
    if baseline.profile != current.profile {
        anyhow::bail!("baseline {} profile mismatch: {} != {}", path.display(), baseline.profile, current.profile);
    }
    if baseline.fixture.run_date != current.fixture.run_date
        || baseline.fixture.pass_1_fragment_count != current.fixture.pass_1_fragment_count
        || baseline.fixture.cleanup_canonical_memory_count != current.fixture.cleanup_canonical_memory_count
        || baseline.fixture.cleanup_substrate_fragment_count != current.fixture.cleanup_substrate_fragment_count
        || baseline.fixture.recall_question_record_count != current.fixture.recall_question_record_count
    {
        anyhow::bail!("baseline {} fixture shape does not match current fixture", path.display());
    }

    let baseline_names = baseline.measurements.iter().map(|measurement| &measurement.name).collect::<Vec<_>>();
    let current_names = current.measurements.iter().map(|measurement| &measurement.name).collect::<Vec<_>>();
    if baseline_names != current_names {
        anyhow::bail!("baseline {} measurement set does not match current fixture", path.display());
    }
    Ok(())
}

fn pass_1_options() -> anyhow::Result<DreamRunOptions> {
    let harness = Arc::new(EchoCli::default());
    Ok(DreamRunOptions {
        repo_root: PathBuf::from("/tmp/stream-f-dream-bench"),
        scope: DreamScope::Project("proj_stream_f_bench".to_owned()),
        run_date: run_date()?,
        run_id: "run_stream_f_bench".to_owned(),
        harness,
        pass_timeout: Duration::from_secs(300),
        pass_2_max_candidates: 8,
        substrate_fragments: (0..PASS_1_FRAGMENT_COUNT).map(dream_substrate_fragment_input).collect(),
        active_memories: (0..PASS_1_ACTIVE_MEMORY_COUNT).map(dream_active_memory_input).collect(),
        previous_questions: vec!["What fixture assumption is hidden?".to_owned()],
    })
}

fn dream_substrate_fragment_input(index: usize) -> DreamSubstrateFragmentInput {
    DreamSubstrateFragmentInput {
        fragment: SubstrateFragment {
            id: substrate_id(index),
            kind: "pattern".to_owned(),
            ts: (instant(RUN_AT) + chrono::Duration::seconds(index as i64)).to_rfc3339(),
            entities: vec![format!("ent_stream_f_{:03}", index % 32)],
            text: format!(
                "Repeated deterministic Stream F substrate observation {index}: auth rotation pattern recurs in repo shard {}.",
                index % 17
            ),
        },
        text_spans: Vec::<PrivacySpan>::new(),
    }
}

fn dream_active_memory_input(index: usize) -> DreamActiveMemoryInput {
    DreamActiveMemoryInput {
        memory: ActiveMemory {
            id: memory_id(index),
            namespace: "project:proj_stream_f_bench".to_owned(),
            kind: "pattern".to_owned(),
            entities: vec![format!("ent_stream_f_{:03}", index % 32)],
            summary: format!("Deterministic active-memory summary {index} for Stream F prompt assembly."),
        },
        summary_spans: Vec::<PrivacySpan>::new(),
    }
}

struct GitLeaseFixture {
    repo: PathBuf,
    runtime: PathBuf,
    _temp: tempfile::TempDir,
}

impl GitLeaseFixture {
    async fn new() -> anyhow::Result<Self> {
        let temp = tempfile::tempdir()?;
        let origin = temp.path().join("origin.git");
        let repo = temp.path().join("repo");
        let runtime = temp.path().join("runtime");
        run_git_at(temp.path(), &["init", "--bare", "--initial-branch=main", path_str(&origin)?])?;
        let _substrate = Substrate::init(
            Roots::new(&repo, &runtime),
            InitOptions { force_unsafe_durability: true, device_id: Some("dev_leasebench".to_owned()) },
        )
        .await?;
        configure_git_repo(&repo)?;
        ensure_origin(&repo, &origin)?;
        run_git(&repo, &["add", "-A"])?;
        run_git(&repo, &["commit", "--allow-empty", "-m", "stream f lease bench baseline"])?;
        run_git(&repo, &["branch", "-M", "main"])?;
        run_git(&repo, &["push", "-u", "origin", "main"])?;
        Ok(Self { repo, runtime, _temp: temp })
    }
}

struct CleanupFixture {
    substrate: Substrate,
    _temp: tempfile::TempDir,
}

impl CleanupFixture {
    async fn new() -> anyhow::Result<Self> {
        let temp = tempfile::tempdir()?;
        let substrate = init_substrate(temp.path(), "dev_cleanupbench").await?;
        configure_git_repo(&substrate.roots().repo)?;
        write_cleanup_memories(&substrate.roots().repo)?;
        write_cleanup_substrate_fragments(&substrate.roots().repo)?;
        write_cleanup_events(&substrate.roots().repo)?;
        Ok(Self { substrate, _temp: temp })
    }
}

struct RecallFixture {
    substrate: Substrate,
    repo: PathBuf,
    _temp: tempfile::TempDir,
}

impl RecallFixture {
    async fn new(with_questions: bool) -> anyhow::Result<Self> {
        let temp = tempfile::tempdir()?;
        let substrate = init_substrate(temp.path(), "dev_recallbench").await?;
        let repo = substrate.roots().repo.clone();
        fs::write(repo.join(".memory-project.yaml"), "canonical_id: proj_stream_f_bench\nalias: Stream F Bench\n")?;
        for index in 0..RECALL_BASE_MEMORY_COUNT {
            substrate
                .write_memory(WriteRequest {
                    operation_id: None,
                    memory: recall_memory(index),
                    expected_base_hash: None,
                    write_mode: WriteMode::CreateNew,
                    index_projection: None,
                    event_context: EventContext::default(),
                    allow_best_effort_durability: true,
                    classification: ClassificationOutcome::Trusted,
                })
                .await?;
        }
        if with_questions {
            write_recall_questions(&repo)?;
        }
        Ok(Self { substrate, repo, _temp: temp })
    }
}

async fn init_substrate(temp_root: &Path, device_id: &str) -> anyhow::Result<Substrate> {
    let repo = temp_root.join("repo");
    let runtime = temp_root.join("runtime");
    Substrate::init(
        Roots::new(repo, runtime),
        InitOptions { force_unsafe_durability: true, device_id: Some(device_id.to_owned()) },
    )
    .await
    .map_err(Into::into)
}

fn write_cleanup_memories(repo: &Path) -> anyhow::Result<()> {
    let root = repo.join("me");
    fs::create_dir_all(&root)?;
    for index in 0..CLEANUP_CANONICAL_MEMORY_COUNT {
        let memory = cleanup_memory(index);
        let path = memory.path.as_ref().context("cleanup memory path")?;
        fs::write(repo.join(path.as_path()), serialize_document(&memory)?)?;
    }
    Ok(())
}

fn write_cleanup_substrate_fragments(repo: &Path) -> anyhow::Result<()> {
    let dir = repo.join("substrate/dev_cleanupbench");
    fs::create_dir_all(&dir)?;
    let file = File::create(dir.join("2026-03-01.jsonl"))?;
    let mut writer = BufWriter::new(file);
    for index in 0..CLEANUP_SUBSTRATE_FRAGMENT_COUNT {
        let record = SubstrateFragmentRecord {
            id: substrate_id(index),
            ts: instant("2026-03-01T12:00:00Z") + chrono::Duration::seconds(index as i64),
            device: DeviceId::new("dev_cleanupbench"),
            session: Some("sess_cleanup_bench".to_owned()),
            harness: Some("codex".to_owned()),
            scope: "project:proj_stream_f_bench".to_owned(),
            entities: vec![format!("ent_stream_f_{:03}", index % 64)],
            kind: ObserveKind::Pattern,
            text: format!("cleanup substrate fragment {index}"),
            source_ref: Some(format!("session:sess_cleanup_bench:turn:{index}")),
            privacy_spans: Vec::<PrivacySpanRecord>::new(),
        };
        writeln!(writer, "{}", serde_json::to_string(&record)?)?;
    }
    writer.flush()?;
    Ok(())
}

fn write_cleanup_events(repo: &Path) -> anyhow::Result<()> {
    let log = repo.join("events/dev_cleanupbench.jsonl");
    for index in 0..CLEANUP_OLD_EVENT_COUNT {
        append_event(&log, &bench_event(index, instant("2025-12-15T12:00:00Z")))?;
    }
    for index in 0..CLEANUP_LIVE_EVENT_COUNT {
        append_event(&log, &bench_event(CLEANUP_OLD_EVENT_COUNT + index, instant("2026-04-29T12:00:00Z")))?;
    }
    Ok(())
}

fn write_recall_questions(repo: &Path) -> anyhow::Result<()> {
    let question_sets = [
        ("dreams/questions/me/2026-04-30.jsonl", "me"),
        ("dreams/questions/agent/2026-04-30.jsonl", "agent"),
        ("dreams/questions/project/proj_stream_f_bench/2026-04-30.jsonl", "project"),
    ];
    for (path, scope) in question_sets {
        let absolute = repo.join(path);
        if let Some(parent) = absolute.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = File::create(absolute)?;
        let mut writer = BufWriter::new(file);
        for index in 0..(RECALL_QUESTION_RECORD_COUNT / question_sets.len()) {
            let record = json!({
                "entities": [format!("ent_stream_f_{:03}", index % 16)],
                "question": format!("What {scope} Stream F benchmark assumption number {index} are we avoiding?")
            });
            writeln!(writer, "{record}")?;
        }
        writer.flush()?;
    }
    Ok(())
}

fn cleanup_memory(index: usize) -> Memory {
    let mut memory = bench_memory(index, Scope::User, None);
    if index % 20 == 0 {
        memory.frontmatter.status = MemoryStatus::Candidate;
        memory.frontmatter.trust_level = TrustLevel::Candidate;
        memory.frontmatter.requires_user_confirmation = true;
        memory.frontmatter.review_state = Some("candidate".to_owned());
        memory.frontmatter.created_at = instant("2026-03-01T12:00:00Z");
        memory.frontmatter.updated_at = instant("2026-03-01T12:00:00Z");
    }
    memory.path = Some(RepoPath::new(format!("me/{}.md", memory.frontmatter.id.as_str())));
    memory
}

fn recall_memory(index: usize) -> Memory {
    bench_memory(index, Scope::Project, Some("proj_stream_f_bench"))
}

fn bench_memory(index: usize, scope: Scope, canonical_project_id: Option<&str>) -> Memory {
    let id = memory_id(index);
    let status = MemoryStatus::Active;
    Memory {
        frontmatter: Frontmatter {
            schema_version: 1,
            id: MemoryId::new(&id),
            memory_type: MemoryType::Project,
            scope,
            summary: format!("Stream F deterministic benchmark memory {index} references auth rotation."),
            confidence: 0.85,
            trust_level: TrustLevel::Trusted,
            sensitivity: Sensitivity::Internal,
            status,
            created_at: instant("2026-04-01T12:00:00Z"),
            updated_at: instant(RUN_AT) + chrono::Duration::seconds(index as i64),
            author: Author {
                kind: AuthorKind::Agent,
                user_handle: None,
                harness: Some("codex".to_owned()),
                harness_version: None,
                session_id: Some("sess_stream_f_bench".to_owned()),
                subagent_id: None,
                phase: None,
                component: None,
            },
            namespace: canonical_project_id.map(|_| "project".to_owned()),
            canonical_namespace_id: canonical_project_id.map(str::to_owned),
            tags: vec!["stream-f".to_owned(), "bench".to_owned()],
            entities: vec![memory_substrate::Entity {
                id: format!("ent_stream_f_{:03}", index % 16),
                label: format!("Stream F Entity {}", index % 16),
                aliases: vec![format!("stream-f-entity-{}", index % 16)],
            }],
            aliases: vec![format!("stream-f-bench-{index}")],
            source: Source {
                kind: SourceKind::AgentPrimary,
                reference: None,
                harness: Some("codex".to_owned()),
                harness_version: None,
                session_id: Some("sess_stream_f_bench".to_owned()),
                subagent_id: None,
                device: Some("dev_bench".to_owned()),
            },
            evidence: Vec::new(),
            requires_user_confirmation: false,
            review_state: None,
            supersedes: Vec::new(),
            superseded_by: Vec::new(),
            related: Vec::new(),
            tombstone_events: Vec::new(),
            retrieval_policy: RetrievalPolicy {
                passive_recall: true,
                max_scope: scope,
                mask_personal_for_synthesis: false,
                index_body: true,
                index_embeddings: true,
            },
            write_policy: WritePolicy {
                human_review_required: false,
                policy_applied: "stream-f-bench".to_owned(),
                expected_base_hash: None,
            },
            merge_diagnostics: None,
            extras: BTreeMap::new(),
        },
        body: format!("Stream F benchmark body {index} with deterministic auth rotation detail."),
        path: Some(RepoPath::new(format!("me/{id}.md"))),
    }
}

fn bench_event(index: usize, at: DateTime<Utc>) -> Event {
    Event {
        schema: EVENT_SCHEMA_VERSION,
        id: EventId::new(format!("evt_stream_f_bench_{index:06}")),
        at,
        device: DeviceId::new("dev_cleanupbench"),
        seq: index as u64,
        operation_id: Some(OperationId::new(format!("op_stream_f_bench_{index:06}"))),
        kind: EventKind::WriteCommitted {
            id: MemoryId::new(memory_id(index)),
            path: RepoPath::new(format!("me/{}.md", memory_id(index))),
            classification: ClassificationOutcome::Trusted,
        },
        crc32c: 0,
    }
}

fn configure_git_repo(repo: &Path) -> anyhow::Result<()> {
    if !repo.join(".git").exists() {
        run_git(repo, &["init", "--initial-branch=main"])?;
    }
    run_git(repo, &["config", "user.email", "codex-bench@example.com"])?;
    run_git(repo, &["config", "user.name", "Codex Bench"])?;
    run_git(repo, &["config", "commit.gpgsign", "false"])?;
    Ok(())
}

fn ensure_origin(repo: &Path, origin: &Path) -> anyhow::Result<()> {
    let origin = path_str(origin)?;
    match run_git(repo, &["remote", "add", "origin", origin]) {
        Ok(()) => Ok(()),
        Err(_) => run_git(repo, &["remote", "set-url", "origin", origin]),
    }
}

fn run_git(repo: &Path, args: &[&str]) -> anyhow::Result<()> {
    run_git_at(repo, args)
}

fn run_git_at(cwd: &Path, args: &[&str]) -> anyhow::Result<()> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_NAMESPACE")
        .output()
        .with_context(|| format!("run git {args:?} in {}", cwd.display()))?;
    if output.status.success() {
        Ok(())
    } else {
        anyhow::bail!("git {:?} failed in {}: {}", args, cwd.display(), String::from_utf8_lossy(&output.stderr));
    }
}

fn path_str(path: &Path) -> anyhow::Result<&str> {
    path.to_str().ok_or_else(|| anyhow!("non-UTF-8 path {}", path.display()))
}

fn memory_id(index: usize) -> String {
    format!("mem_20260430_{:016x}_{:06}", index as u64 + 0x5F17EA, index % 1_000_000)
}

fn substrate_id(index: usize) -> String {
    const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let mut value = index as u128 + 1_000_000;
    let mut encoded = [b'0'; 26];
    for slot in encoded.iter_mut().rev() {
        *slot = ALPHABET[(value % 32) as usize];
        value /= 32;
    }
    format!("sub_{}", String::from_utf8_lossy(&encoded))
}

fn run_date() -> anyhow::Result<chrono::NaiveDate> {
    chrono::NaiveDate::parse_from_str(RUN_DATE, "%Y-%m-%d").map_err(Into::into)
}

fn instant(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value).expect("fixture timestamp parses").with_timezone(&Utc)
}
