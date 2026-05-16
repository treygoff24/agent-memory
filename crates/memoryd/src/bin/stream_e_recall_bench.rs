use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use clap::Parser;
use memory_substrate::{
    Author, AuthorKind, ClassificationOutcome, EventContext, Frontmatter, InitOptions, Memory, MemoryId, MemoryStatus,
    MemoryType, RepoPath, RetrievalPolicy, Roots, Scope, Sensitivity, Source, SourceKind, Substrate, TrustLevel,
    WriteMode, WritePolicy, WriteRequest,
};
use memoryd::recall::{build_delta_response, build_startup_response, DeltaRequest, StartupRequest};
use serde::Serialize;

const SEED_SMOKE: u64 = 169_300_215;
const SEED_RELEASE: u64 = 693_467_474_526;

#[derive(Debug, Parser)]
#[command(group(clap::ArgGroup::new("mode").args(["smoke", "release"]).multiple(false)))]
struct Args {
    #[arg(long, default_value = "200,1000")]
    sizes: String,
    #[arg(long, default_value_t = 3)]
    warm_runs: usize,
    #[arg(long)]
    smoke: bool,
    #[arg(long)]
    release: bool,
}

#[derive(Debug, Serialize)]
struct BenchReport {
    mode: String,
    hardware_profile: String,
    budget_tokens: usize,
    results: Vec<BenchResult>,
}

#[derive(Debug, Serialize)]
struct BenchResult {
    memory_count: usize,
    encrypted_metadata_only_count: usize,
    candidate_quarantine_count: usize,
    selected_memory_count: usize,
    omitted_memory_count: usize,
    cold_start_ms: f64,
    cold_start_samples: usize,
    startup_warm_p95_ms: f64,
    startup_warm_samples: usize,
    delta_no_match_p95_ms: f64,
    delta_no_match_samples: usize,
    delta_five_entity_match_p95_ms: f64,
    delta_five_entity_match_samples: usize,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let mode = if args.release { "release" } else { "smoke" };
    let sizes = args.sizes.split(',').map(|size| size.trim().parse::<usize>()).collect::<Result<Vec<_>, _>>()?;
    let mut results = Vec::new();
    for size in sizes {
        results.push(run_size(size, args.warm_runs.max(1)).await?);
    }

    let report = BenchReport {
        mode: mode.to_owned(),
        hardware_profile: std::env::var("BENCH_PROFILE").unwrap_or_else(|_| std::env::consts::ARCH.to_owned()),
        budget_tokens: 3_600,
        results,
    };

    enforce_thresholds(&report, args.release)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

async fn run_size(size: usize, warm_runs: usize) -> anyhow::Result<BenchResult> {
    let temp = tempfile::tempdir()?;
    let repo = temp.path().join("repo");
    let runtime = temp.path().join("runtime");
    let substrate = Substrate::init(
        Roots::new(&repo, &runtime),
        InitOptions { force_unsafe_durability: true, device_id: Some(format!("dev_bench{size}")) },
    )
    .await?;

    let mut encrypted = 0usize;
    let mut attention = 0usize;
    for index in 0..size {
        let memory = bench_memory(index);
        if !memory.frontmatter.retrieval_policy.index_body {
            encrypted += 1;
        }
        if matches!(memory.frontmatter.status, MemoryStatus::Candidate | MemoryStatus::Quarantined) {
            attention += 1;
        }
        substrate
            .write_memory(WriteRequest {
                operation_id: None,
                memory,
                expected_base_hash: None,
                write_mode: WriteMode::CreateNew,
                index_projection: None,
                event_context: EventContext::default(),
                allow_best_effort_durability: true,
                classification: ClassificationOutcome::Trusted,
            })
            .await?;
    }

    let cold_start_ms = timed_startup(&substrate, &repo).await?.as_secs_f64() * 1000.0;
    let mut startup = Vec::new();
    let mut selected_memory_count = 0usize;
    let mut omitted_memory_count = 0usize;
    for _ in 0..warm_runs {
        let started = Instant::now();
        let response = startup_response(&substrate, &repo).await?;
        startup.push(started.elapsed());
        selected_memory_count =
            response.recall_explanation.sections.iter().map(|section| section.selected_ids.len()).sum();
        omitted_memory_count = response.recall_explanation.omitted.len();
    }

    let delta_no_match_p95_ms = timed_delta(&substrate, &repo, "definitely-no-match", warm_runs).await?;
    let delta_five_entity_match_p95_ms =
        timed_delta(&substrate, &repo, "entity-alpha fixture recall", warm_runs).await?;

    Ok(BenchResult {
        memory_count: size,
        encrypted_metadata_only_count: encrypted,
        candidate_quarantine_count: attention,
        selected_memory_count,
        omitted_memory_count,
        cold_start_ms,
        cold_start_samples: 1,
        startup_warm_p95_ms: p95(startup).as_secs_f64() * 1000.0,
        startup_warm_samples: warm_runs,
        delta_no_match_p95_ms,
        delta_no_match_samples: warm_runs,
        delta_five_entity_match_p95_ms,
        delta_five_entity_match_samples: warm_runs,
    })
}

async fn timed_startup(substrate: &Substrate, repo: &std::path::Path) -> anyhow::Result<Duration> {
    let started = Instant::now();
    let _ = startup_response(substrate, repo).await?;
    Ok(started.elapsed())
}

async fn timed_delta(
    substrate: &Substrate,
    repo: &std::path::Path,
    message: &str,
    warm_runs: usize,
) -> anyhow::Result<f64> {
    let mut durations = Vec::new();
    for _ in 0..warm_runs {
        let started = Instant::now();
        let _ = build_delta_response(
            substrate,
            DeltaRequest {
                cwd: repo.to_string_lossy().into_owned(),
                session_id: "bench".to_owned(),
                harness: "codex".to_owned(),
                message: message.to_owned(),
                budget_tokens: Some(400),
            },
        )
        .await?;
        durations.push(started.elapsed());
    }
    Ok(p95(durations).as_secs_f64() * 1000.0)
}

async fn startup_response(
    substrate: &Substrate,
    repo: &std::path::Path,
) -> anyhow::Result<memoryd::recall::StartupResponse> {
    Ok(build_startup_response(
        substrate,
        StartupRequest {
            cwd: repo.to_string_lossy().into_owned(),
            session_id: "bench".to_owned(),
            harness: "codex".to_owned(),
            harness_version: None,
            include_recent: true,
            since_event_id: None,
            budget_tokens: Some(3_600),
        },
    )
    .await?)
}

fn bench_memory(index: usize) -> Memory {
    let status = match index % 20 {
        0 => MemoryStatus::Candidate,
        1 => MemoryStatus::Quarantined,
        2 => MemoryStatus::Pinned,
        _ => MemoryStatus::Active,
    };
    let scope = match index % 3 {
        0 => Scope::User,
        1 => Scope::Project,
        _ => Scope::Agent,
    };
    let encrypted_like = index % 17 == 0;
    let id = format!("mem_20260430_{:016x}_{:06}", index as u64 + SEED_SMOKE + SEED_RELEASE, index % 1_000_000);
    Memory {
        frontmatter: Frontmatter {
            schema_version: 1,
            id: MemoryId::new(&id),
            memory_type: MemoryType::Project,
            scope,
            summary: format!("entity-alpha fixture recall memory {index}"),
            confidence: 0.5 + ((index % 50) as f64 / 100.0),
            original_confidence: None,
            trust_level: trust_for_status(status),
            sensitivity: Sensitivity::Internal,
            status,
            created_at: instant("2026-04-30T12:00:00Z"),
            updated_at: instant("2026-04-30T12:00:00Z") + chrono::Duration::seconds(index as i64),
            observed_at: None,
            author: Author {
                kind: AuthorKind::Agent,
                user_handle: None,
                harness: Some("bench".to_owned()),
                harness_version: None,
                session_id: Some("bench".to_owned()),
                subagent_id: None,
                phase: None,
                component: None,
            },
            namespace: (scope == Scope::Project).then(|| "project".to_owned()),
            canonical_namespace_id: (scope == Scope::Project).then(|| "proj_bench".to_owned()),
            tags: vec!["entity-alpha".to_owned(), format!("tag-{}", index % 8)],
            entities: vec![memory_substrate::Entity {
                id: "ent_alpha".to_owned(),
                label: "Entity Alpha".to_owned(),
                aliases: vec!["entity-alpha".to_owned()],
            }],
            aliases: vec![format!("bench-{}", index % 16)],
            source: Source {
                kind: SourceKind::AgentPrimary,
                reference: None,
                harness: Some("bench".to_owned()),
                harness_version: None,
                session_id: Some("bench".to_owned()),
                subagent_id: None,
                device: None,
            },
            evidence: Vec::new(),
            requires_user_confirmation: matches!(status, MemoryStatus::Candidate | MemoryStatus::Quarantined),
            review_state: matches!(status, MemoryStatus::Candidate | MemoryStatus::Quarantined)
                .then(|| "pending".to_owned()),
            supersedes: Vec::new(),
            superseded_by: Vec::new(),
            related: Vec::new(),
            tombstone_events: Vec::new(),
            retrieval_policy: RetrievalPolicy {
                passive_recall: index % 19 != 0,
                max_scope: scope,
                mask_personal_for_synthesis: encrypted_like,
                index_body: !encrypted_like,
                index_embeddings: !encrypted_like,
            },
            write_policy: WritePolicy {
                human_review_required: matches!(status, MemoryStatus::Candidate | MemoryStatus::Quarantined),
                policy_applied: "stream-e-bench".to_owned(),
                expected_base_hash: None,
            },
            merge_diagnostics: matches!(status, MemoryStatus::Quarantined).then(|| serde_json::json!({"bench": true})),
            extras: BTreeMap::new(),
        },
        body: format!("Entity Alpha deterministic benchmark body {index}"),
        path: Some(RepoPath::new(format!("me/{id}.md"))),
    }
}

fn trust_for_status(status: MemoryStatus) -> TrustLevel {
    match status {
        MemoryStatus::Pinned => TrustLevel::Pinned,
        MemoryStatus::Candidate => TrustLevel::Candidate,
        MemoryStatus::Quarantined => TrustLevel::Quarantined,
        _ => TrustLevel::Trusted,
    }
}

fn p95(mut values: Vec<Duration>) -> Duration {
    values.sort_unstable();
    let index = ((values.len().saturating_sub(1)) as f64 * 0.95).ceil() as usize;
    values[index.min(values.len().saturating_sub(1))]
}

fn enforce_thresholds(report: &BenchReport, release: bool) -> anyhow::Result<()> {
    if !release {
        return Ok(());
    }
    for result in &report.results {
        let startup_cap = if result.memory_count <= 200 { 80.0 } else { 250.0 };
        if result.startup_warm_p95_ms > startup_cap {
            anyhow::bail!("startup warm p95 exceeded cap for {} memories", result.memory_count);
        }
        if result.cold_start_ms > 600.0 {
            anyhow::bail!("cold startup exceeded cap for {} memories", result.memory_count);
        }
        if result.delta_no_match_p95_ms > 60.0 {
            anyhow::bail!("delta no-match p95 exceeded cap for {} memories", result.memory_count);
        }
        if result.delta_five_entity_match_p95_ms > 120.0 {
            anyhow::bail!("delta five-entity p95 exceeded cap for {} memories", result.memory_count);
        }
    }
    Ok(())
}

fn instant(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value).expect("fixture timestamp parses").with_timezone(&Utc)
}
