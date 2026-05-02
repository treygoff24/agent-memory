use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{anyhow, bail, Context};
use chrono::{DateTime, Duration, TimeZone, Utc};
use clap::Parser;
use memorum_coordination::gate::CandidateEmbedding;
use memorum_coordination::{CoordinationConfig, PeerWriteCandidate, QueryEmbedding, RelevanceGate, SessionContext};
use memory_substrate::{
    EmbeddingTriple, Entity, MemoryId, MemoryStatus, RecallIndexRow, RepoPath, Scope, Sensitivity, SourceKind,
};
use serde::{Deserialize, Serialize};

const FIXTURE_VERSION: &str = "stream-i-cross-session-v0.1-task-21";
const RUN_DATE: &str = "2026-05-02";
const RUN_AT: &str = "2026-05-02T12:00:00Z";
const CANDIDATE_COUNT: usize = 100;
const WITHIN_RECENCY_COUNT: usize = 50;
const OUTSIDE_RECENCY_COUNT: usize = 50;
const SALIENT_ENTITY_COUNT: usize = 10;
const SALIENT_PATH_COUNT: usize = 10;
const SAMPLE_COUNT: usize = 301;
const RELEVANCE_GATE_BUDGET_MS: f64 = 5.0;
const EMBEDDING_DIMENSION: usize = 16;

#[derive(Debug, Parser)]
struct Args {
    /// Hardware/profile label recorded in the JSON fixture.
    #[arg(long)]
    profile: String,

    /// Assert current measurements against Stream I budgets and an existing baseline contract.
    #[arg(long)]
    assert: bool,

    /// Existing baseline JSON to validate in assert mode.
    #[arg(long)]
    baseline: Option<PathBuf>,

    /// Explicit release/update mode. This is the only mode that writes the canonical output file.
    #[arg(long)]
    write_output: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BenchReport {
    schema_version: u32,
    fixture_version: String,
    profile: String,
    runs: usize,
    platform: PlatformReport,
    fixture: FixtureReport,
    peer_relevance_gate: PeerRelevanceGateReport,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PlatformReport {
    os: String,
    arch: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct FixtureReport {
    run_date: String,
    run_at: String,
    candidate_count: usize,
    within_recency_count: usize,
    outside_recency_count: usize,
    salient_entity_count: usize,
    salient_path_count: usize,
    precomputed_embedding_dimension: usize,
    sample_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PeerRelevanceGateReport {
    description: String,
    statistic_unit: String,
    p50_ms: f64,
    p95_ms: f64,
    p99_ms: f64,
    budget_ms: f64,
    budget_operator: BudgetOperator,
    sample_count: usize,
    pass: bool,
    selected_peer_updates: usize,
    capped_peer_updates: u32,
    embedding_worker_wait_excluded: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum BudgetOperator {
    LessThanOrEqual,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    validate_mode(&args)?;

    let report = run_benchmark(&args.profile)?;

    if args.assert {
        let baseline_path = args.baseline.as_ref().context("baseline is required in assert mode")?;
        if baseline_requires_bootstrap(baseline_path)? {
            enforce_budget(&report)?;
            let proposed_path = proposed_baseline_path(baseline_path);
            write_report(&proposed_path, &report)?;
            eprintln!("first run — wrote .proposed; commit as baseline once verified.");
            println!("{}", serde_json::to_string_pretty(&report)?);
            return Ok(());
        }

        let baseline = read_baseline(baseline_path)?;
        validate_baseline_contract(&baseline, &report, baseline_path)?;
        enforce_budget(&baseline).context("baseline contains failing Stream I peer relevance measurement")?;
        enforce_budget(&report)?;
    }

    if let Some(output_path) = args.write_output.as_ref() {
        guard_immutable_baseline_path(output_path)?;
        enforce_budget(&report)?;
        write_report(output_path, &report)?;
    }

    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn validate_mode(args: &Args) -> anyhow::Result<()> {
    if args.profile.trim().is_empty() {
        bail!("--profile requires a non-empty value");
    }
    if args.assert == args.write_output.is_some() {
        bail!("choose exactly one mode: --assert with --baseline, or --write-output <path>");
    }
    if args.assert && args.baseline.is_none() {
        bail!("--assert requires --baseline <path>");
    }
    if !args.assert && args.baseline.is_some() {
        bail!("--baseline is only valid with --assert");
    }
    Ok(())
}

fn run_benchmark(profile: &str) -> anyhow::Result<BenchReport> {
    let fixture = BenchFixture::new()?;
    let peer_relevance_gate = measure_peer_relevance_gate(&fixture)?;

    Ok(BenchReport {
        schema_version: 1,
        fixture_version: FIXTURE_VERSION.to_owned(),
        profile: profile.to_owned(),
        runs: 1,
        platform: PlatformReport { os: std::env::consts::OS.to_owned(), arch: std::env::consts::ARCH.to_owned() },
        fixture: FixtureReport {
            run_date: RUN_DATE.to_owned(),
            run_at: RUN_AT.to_owned(),
            candidate_count: CANDIDATE_COUNT,
            within_recency_count: WITHIN_RECENCY_COUNT,
            outside_recency_count: OUTSIDE_RECENCY_COUNT,
            salient_entity_count: SALIENT_ENTITY_COUNT,
            salient_path_count: SALIENT_PATH_COUNT,
            precomputed_embedding_dimension: EMBEDDING_DIMENSION,
            sample_count: SAMPLE_COUNT,
        },
        peer_relevance_gate,
    })
}

fn measure_peer_relevance_gate(fixture: &BenchFixture) -> anyhow::Result<PeerRelevanceGateReport> {
    let gate = RelevanceGate::new(CoordinationConfig::default());
    let mut samples_ms = Vec::with_capacity(SAMPLE_COUNT);
    let mut selected_peer_updates = 0usize;
    let mut capped_peer_updates = 0u32;

    for _ in 0..SAMPLE_COUNT {
        let mut session = fixture.session.clone();
        let started = Instant::now();
        let insertion = gate.evaluate(&mut session, &fixture.candidates, fixture.now);
        let elapsed = started.elapsed();

        selected_peer_updates = insertion.peer_updates.len();
        capped_peer_updates = insertion.capped_peer_updates;
        samples_ms.push(elapsed.as_secs_f64() * 1_000.0 / fixture.candidates.len() as f64);
        black_box(insertion);
    }

    validate_gate_shape(selected_peer_updates, capped_peer_updates)?;
    let p95_ms = round6(percentile(samples_ms.clone(), 0.95)?);

    Ok(PeerRelevanceGateReport {
        description: "Per-candidate in-memory relevance gate latency over 100 fixed peer-write candidates.".to_owned(),
        statistic_unit: "milliseconds_per_candidate".to_owned(),
        p50_ms: round6(percentile(samples_ms.clone(), 0.50)?),
        p95_ms,
        p99_ms: round6(percentile(samples_ms, 0.99)?),
        budget_ms: RELEVANCE_GATE_BUDGET_MS,
        budget_operator: BudgetOperator::LessThanOrEqual,
        sample_count: SAMPLE_COUNT,
        pass: p95_ms <= RELEVANCE_GATE_BUDGET_MS,
        selected_peer_updates,
        capped_peer_updates,
        embedding_worker_wait_excluded: true,
    })
}

fn validate_gate_shape(selected_peer_updates: usize, capped_peer_updates: u32) -> anyhow::Result<()> {
    let expected_capped =
        u32::try_from(WITHIN_RECENCY_COUNT.saturating_sub(CoordinationConfig::default().relevance_gate.per_turn_cap))
            .context("expected capped peer-update count must fit in u32")?;

    if selected_peer_updates != CoordinationConfig::default().relevance_gate.per_turn_cap {
        bail!("fixture selected {selected_peer_updates} peer updates; expected per-turn cap");
    }
    if capped_peer_updates != expected_capped {
        bail!("fixture capped {capped_peer_updates} peer updates; expected {expected_capped}");
    }
    Ok(())
}

fn percentile(mut samples: Vec<f64>, quantile: f64) -> anyhow::Result<f64> {
    if samples.is_empty() {
        bail!("cannot compute percentile of an empty sample set");
    }
    samples.sort_by(f64::total_cmp);
    let index = ((samples.len().saturating_sub(1)) as f64 * quantile).ceil() as usize;
    samples.get(index.min(samples.len().saturating_sub(1))).copied().context("percentile index must exist")
}

fn round6(value: f64) -> f64 {
    (value * 1_000_000.0).round() / 1_000_000.0
}

fn enforce_budget(report: &BenchReport) -> anyhow::Result<()> {
    if report.peer_relevance_gate.pass {
        Ok(())
    } else {
        bail!(
            "Stream I peer relevance gate p95={}ms exceeds budget <= {}ms",
            report.peer_relevance_gate.p95_ms,
            report.peer_relevance_gate.budget_ms
        );
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
    let value = serde_json::from_str::<serde_json::Value>(&text)
        .with_context(|| format!("parse baseline {}", path.display()))?;
    Ok(value.get("runs").and_then(serde_json::Value::as_u64).is_some_and(|runs| runs == 0))
}

fn proposed_baseline_path(path: &Path) -> PathBuf {
    let mut proposed = path.as_os_str().to_os_string();
    proposed.push(".proposed");
    PathBuf::from(proposed)
}

fn guard_immutable_baseline_path(path: &Path) -> anyhow::Result<()> {
    if path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("baseline.") && name.ends_with(".json"))
    {
        bail!("refusing to write Stream I output to immutable baseline path {}", path.display());
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
        bail!("baseline {} fixture shape does not match current Stream I fixture", path.display());
    }
    if baseline.peer_relevance_gate.budget_ms != current.peer_relevance_gate.budget_ms {
        bail!("baseline {} peer relevance budget does not match current fixture", path.display());
    }
    Ok(())
}

struct BenchFixture {
    now: DateTime<Utc>,
    session: SessionContext,
    candidates: Vec<PeerWriteCandidate>,
}

impl BenchFixture {
    fn new() -> anyhow::Result<Self> {
        let now = fixture_now()?;
        let session = session_context();
        let candidates = peer_write_candidates(now)?;

        Ok(Self { now, session, candidates })
    }
}

fn session_context() -> SessionContext {
    let triple = embedding_triple();
    let mut session = SessionContext {
        session_id: "bench_current_session".to_owned(),
        harness: "codex".to_owned(),
        recent_query_embedding: Some(QueryEmbedding { triple, vector: embedding_vector(0) }),
        ..SessionContext::default()
    };
    session.salient_entities = (0..SALIENT_ENTITY_COUNT).map(entity_id).collect();
    session.salient_paths = (0..SALIENT_PATH_COUNT).map(path_id).collect();
    session
}

fn peer_write_candidates(now: DateTime<Utc>) -> anyhow::Result<Vec<PeerWriteCandidate>> {
    let mut candidates = Vec::with_capacity(CANDIDATE_COUNT);
    for index in 0..CANDIDATE_COUNT {
        candidates.push(peer_write_candidate(index, now)?);
    }
    Ok(candidates)
}

fn peer_write_candidate(index: usize, now: DateTime<Utc>) -> anyhow::Result<PeerWriteCandidate> {
    let memory_id = memory_id(index)?;
    let indexed_at = if index < WITHIN_RECENCY_COUNT {
        now - Duration::seconds(index as i64)
    } else {
        now - Duration::minutes(31) - Duration::seconds(index as i64)
    };

    Ok(PeerWriteCandidate {
        memory_id: memory_id.clone(),
        row: recall_row(&memory_id, index, now, indexed_at)?,
        paths: candidate_paths(index),
        harness: "claude-code".to_owned(),
        session_id: format!("peer_session_{:03}", index % 4),
        namespace: "project:stream-i".to_owned(),
        embedding: Some(CandidateEmbedding { triple: embedding_triple(), vector: embedding_vector(index) }),
    })
}

fn recall_row(
    memory_id: &MemoryId,
    index: usize,
    now: DateTime<Utc>,
    indexed_at: DateTime<Utc>,
) -> anyhow::Result<RecallIndexRow> {
    Ok(RecallIndexRow {
        id: memory_id.clone(),
        path: RepoPath::try_new(format!("projects/stream-i/peer-{index:03}.md"))
            .map_err(|err| anyhow!("invalid fixture repo path: {err}"))?,
        summary: format!("Peer update fixture {index:03}"),
        status: MemoryStatus::Active,
        scope: Scope::Project,
        canonical_namespace_id: Some("stream-i".to_owned()),
        updated_at: now - Duration::seconds(index as i64),
        indexed_at,
        confidence: 0.9,
        source_kind: SourceKind::AgentPrimary,
        source_device: Some(format!("device_{}", index % 3)),
        sensitivity: Sensitivity::Internal,
        passive_recall: true,
        index_body: true,
        requires_user_confirmation: false,
        review_state: None,
        human_review_required: false,
        max_scope: Scope::Project,
        tags: vec!["stream-i".to_owned(), "bench".to_owned()],
        aliases: Vec::new(),
        entities: candidate_entities(index),
    })
}

fn memory_id(index: usize) -> anyhow::Result<MemoryId> {
    let value = format!("mem_20260502_a1b2c3d4e5f60718_{index:06}");
    MemoryId::try_new(value).map_err(|err| anyhow!("invalid fixture memory id: {err}"))
}

fn candidate_entities(index: usize) -> Vec<Entity> {
    (0..4)
        .map(|offset| {
            let id = entity_id((index + offset) % SALIENT_ENTITY_COUNT);
            Entity { label: id.clone(), id, aliases: Vec::new() }
        })
        .collect()
}

fn candidate_paths(index: usize) -> Vec<String> {
    vec![path_id(index % SALIENT_PATH_COUNT)]
}

fn entity_id(index: usize) -> String {
    format!("ent_{index:02}")
}

fn path_id(index: usize) -> String {
    format!("projects/stream-i/path-{index:02}.md")
}

fn embedding_triple() -> EmbeddingTriple {
    EmbeddingTriple {
        provider: "local-fixture".to_owned(),
        model_ref: "stream-i-precomputed-v1".to_owned(),
        dimension: EMBEDDING_DIMENSION as u32,
    }
}

fn embedding_vector(index: usize) -> Vec<f32> {
    let rotation = index % EMBEDDING_DIMENSION;
    (0..EMBEDDING_DIMENSION).map(|dimension| if dimension == rotation { 1.0 } else { 0.25 }).collect()
}

fn fixture_now() -> anyhow::Result<DateTime<Utc>> {
    Utc.with_ymd_and_hms(2026, 5, 2, 12, 0, 0).single().context("fixed Stream I bench timestamp must be valid")
}
