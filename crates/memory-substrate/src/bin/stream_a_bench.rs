//! Real Stream A benchmark harness used by the shell bench gate.
//!
//! Spec §17.6: synthetic vectors come from `memory-test-support::perf` only.
//! Spec §17.6 / §18.9: baselines are immutable; this binary refuses to write
//! to any path matching `baseline.<profile>.json`.

use std::path::{Path, PathBuf};
use std::time::Instant;

use memory_substrate::events::{rewrite_events, Event, EventKind};
use memory_substrate::index::chunk_memory;
use memory_substrate::{
    Author, AuthorKind, ChunkQuery, ClassificationOutcome, DeviceId, EmbeddingTriple, EmbeddingUpdate, EventId,
    Frontmatter, InitOptions, Memory, MemoryId, MemoryQuery, MemoryStatus, MemoryType, OperationId, RepoPath,
    RetrievalPolicy, Roots, Scope, Sensitivity, Source, SourceKind, Substrate, TrustLevel, WritePolicy,
};
use memory_test_support::perf::{corpus_sha256, synthetic_vector};
use serde::Serialize;
use serde_json::json;

const BENCH_EMBEDDING_PROVIDER: &str = "synthetic";
const BENCH_EMBEDDING_MODEL_REF: &str = "stream-a-test";
const BENCH_EMBEDDING_DIMENSION: u32 = 32;

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    let args = parse_args()?;
    guard_baseline_path(&args.output)?;

    let input = generate_bench_input(args.seed, args.corpus, args.runs);
    let fixture = Fixture::build(&input).await?;
    // The initialized fixture is a Git repository; hash only generated memories,
    // not random Git object metadata that is outside the measured corpus.
    let corpus_hash = corpus_sha256(&fixture.roots.repo.join("agent/patterns"));
    let metrics = run_iterations(&fixture, &input.workload).await?;
    write_report(&args, &metrics, &corpus_hash, &args.output)?;
    println!("bench gate wrote {}", args.output.display());
    Ok(())
}

struct BenchArgs {
    tier: String,
    profile: String,
    output: PathBuf,
    runs: usize,
    corpus: usize,
    seed: u64,
}

#[derive(Debug, Serialize)]
pub(crate) struct GeneratedBenchInput {
    memories: Vec<Memory>,
    events: Vec<Event>,
    embedding_vectors: Vec<Vec<f32>>,
    workload: Vec<BenchRun>,
}

#[derive(Debug, Serialize)]
struct BenchRun {
    query_id: MemoryId,
    metadata_tag: String,
    fts_text: String,
    query_vector: Vec<f32>,
}

pub(crate) fn generate_bench_input(seed: u64, corpus: usize, runs: usize) -> GeneratedBenchInput {
    let memories = (0..corpus).map(|index| sample_memory(index, seed)).collect::<Vec<_>>();
    let device = DeviceId::try_new("dev_bench").expect("static bench device id"); // expect-justified: static literal id

    let events =
        memories.iter().enumerate().map(|(index, memory)| sample_event(memory, &device, index, seed)).collect();
    let embedding_vectors =
        (0..corpus.min(32)).map(|index| synthetic_vector(seed, BENCH_EMBEDDING_DIMENSION as usize, index)).collect();
    let last_index = corpus.saturating_sub(1);
    let workload = (0..runs)
        .map(|run_index| BenchRun {
            query_id: MemoryId::new(format!("mem_20260424_a1b2c3d4e5f60718_{last_index:06}")),
            metadata_tag: "bucket-7".to_string(),
            fts_text: last_index.to_string(),
            query_vector: synthetic_vector(seed, BENCH_EMBEDDING_DIMENSION as usize, run_index),
        })
        .collect();

    GeneratedBenchInput { memories, events, embedding_vectors, workload }
}

fn parse_args() -> Result<BenchArgs, String> {
    let mut tier = String::new();
    let mut profile = String::new();
    let mut output = PathBuf::new();
    let mut runs = 0usize;
    let mut corpus = 0usize;
    let mut seed = memory_test_support::perf::SEED_SMOKE;
    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--tier" => tier = args.next().ok_or("--tier requires value")?,
            "--profile" => profile = args.next().ok_or("--profile requires value")?,
            "--output" => output = PathBuf::from(args.next().ok_or("--output requires value")?),
            "--runs" => {
                runs = args.next().ok_or("--runs requires value")?.parse::<usize>().map_err(|e| e.to_string())?
            }
            "--corpus" => {
                corpus = args.next().ok_or("--corpus requires value")?.parse::<usize>().map_err(|e| e.to_string())?
            }
            "--seed" => {
                let raw = args.next().ok_or("--seed requires value")?;
                seed = u64::from_str_radix(raw.trim_start_matches("0x"), 16).map_err(|e| e.to_string())?;
            }
            "--smoke" => {
                tier = "smoke".to_string();
                profile = "dev".to_string();
                output = PathBuf::from("/tmp/stream-a-bench-smoke.json");
                runs = 3;
                corpus = 50;
                seed = memory_test_support::perf::SEED_SMOKE;
            }
            other => return Err(format!("unknown arg: {other}")),
        }
    }

    if tier.is_empty() || profile.is_empty() || output.as_os_str().is_empty() || runs == 0 || corpus == 0 {
        return Err(
            "usage: stream_a_bench --tier smoke|release --profile PROFILE --output PATH --runs N --corpus N [--seed HEX] | --smoke"
                .to_string(),
        );
    }

    Ok(BenchArgs { tier, profile, output, runs, corpus, seed })
}

/// Refuse if the output path looks like a baseline file.
///
/// Baselines are immutable absent explicit human commits (CLAUDE.md invariant 7).
/// `bench-gate.sh` also guards this, but defense-in-depth at the harness layer
/// catches `cargo run --bin stream_a_bench -- --output bench/baseline.*.json`.
fn guard_baseline_path(output: &Path) -> Result<(), String> {
    if output.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("baseline.") && n.ends_with(".json")) {
        Err(format!(
            "refusing to write to baseline path '{}'; baselines are immutable (spec §17.6/§18.9)",
            output.display()
        ))
    } else {
        Ok(())
    }
}

struct Metrics {
    cold_reindex: Vec<f64>,
    query_by_id: Vec<f64>,
    filtered_metadata_query: Vec<f64>,
    fts_chunk_query: Vec<f64>,
    vector_chunk_query: Vec<f64>,
    tree_validator: Vec<f64>,
}

async fn run_iterations(fixture: &Fixture, workload: &[BenchRun]) -> Result<Metrics, String> {
    let mut cold_reindex = Vec::with_capacity(workload.len());
    let mut query_by_id = Vec::with_capacity(workload.len());
    let mut filtered_metadata_query = Vec::with_capacity(workload.len());
    let mut fts_chunk_query = Vec::with_capacity(workload.len());
    let mut vector_chunk_query = Vec::with_capacity(workload.len());
    let mut tree_validator = Vec::with_capacity(workload.len());

    for run in workload {
        cold_reindex.push(measure_async(|| async { fixture.substrate.reindex().await.map(|_| ()) }).await?);
        query_by_id.push(
            measure_async(|| async {
                fixture
                    .substrate
                    .query_memory(MemoryQuery {
                        id: Some(run.query_id.clone()),
                        tag: None,
                        include_metadata_only: false,
                        ..MemoryQuery::default()
                    })
                    .await
                    .map(|_| ())
            })
            .await?,
        );
        filtered_metadata_query.push(
            measure_async(|| async {
                fixture
                    .substrate
                    .query_memory(MemoryQuery {
                        id: None,
                        tag: Some(run.metadata_tag.clone()),
                        include_metadata_only: true,
                        ..MemoryQuery::default()
                    })
                    .await
                    .map(|_| ())
            })
            .await?,
        );
        fts_chunk_query.push(
            measure_async(|| async {
                fixture
                    .substrate
                    .query_chunks(ChunkQuery {
                        text: Some(run.fts_text.clone()),
                        triple: None,
                        vector: None,
                        namespaces: None,
                    })
                    .await
                    .map(|_| ())
            })
            .await?,
        );
        vector_chunk_query.push(
            measure_async(|| async {
                fixture
                    .substrate
                    .query_chunks(ChunkQuery {
                        text: None,
                        triple: Some(fixture.triple.clone()),
                        vector: Some(run.query_vector.clone()),
                        namespaces: None,
                    })
                    .await
                    .map(|_| ())
            })
            .await?,
        );
        tree_validator.push(measure(|| {
            memory_substrate::tree::validate_tree(
                &fixture.roots.repo,
                memory_substrate::tree::TreeValidationMode::PartialSync,
            )
            .map(|_| ())
        })?);
    }

    Ok(Metrics {
        cold_reindex,
        query_by_id,
        filtered_metadata_query,
        fts_chunk_query,
        vector_chunk_query,
        tree_validator,
    })
}

fn write_report(args: &BenchArgs, metrics: &Metrics, corpus_hash: &str, output: &Path) -> Result<(), String> {
    let report = json!({
        "schema": 1,
        "tier": args.tier,
        "profile": args.profile,
        "runs": args.runs,
        "corpus_size": args.corpus,
        "seed": format!("0x{:x}", args.seed),
        // B-RT-4: corpus_sha256 + active_triple required by spec §17.6.
        "corpus_sha256": corpus_hash,
        "vector_dimension": BENCH_EMBEDDING_DIMENSION,
        "active_triple": {
            "provider": BENCH_EMBEDDING_PROVIDER,
            "model_ref": BENCH_EMBEDDING_MODEL_REF,
            "dimension": BENCH_EMBEDDING_DIMENSION
        },
        "corpus_variants": [
            "long_bodies", "large_bodies", "aliases", "entity_aliases", "regressions",
            "prospective", "tombstones", "encrypted_metadata_only"
        ],
        "metrics": {
            "cold_reindex": metric(metrics.cold_reindex.clone()),
            "query_by_id": metric(metrics.query_by_id.clone()),
            "filtered_metadata_query": metric(metrics.filtered_metadata_query.clone()),
            "fts_chunk_query": metric(metrics.fts_chunk_query.clone()),
            "vector_chunk_query": metric(metrics.vector_chunk_query.clone()),
            "tree_validator": metric(metrics.tree_validator.clone()),
        }
    });

    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(output, serde_json::to_string_pretty(&report).map_err(|e| e.to_string())? + "\n")
        .map_err(|e| e.to_string())
}

struct Fixture {
    roots: Roots,
    substrate: Substrate,
    triple: EmbeddingTriple,
}

impl Fixture {
    async fn build(input: &GeneratedBenchInput) -> Result<Self, String> {
        // Use tempfile::TempDir to avoid PID races (R-RT-3).
        let root = tempfile::tempdir().map_err(|e| e.to_string())?;
        let roots = Roots::new(root.path().join("repo"), root.path().join("runtime"));
        let triple = bench_embedding_triple();
        write_bench_config(&roots)?;
        let substrate = Substrate::init(
            roots.clone(),
            InitOptions { force_unsafe_durability: true, device_id: Some("dev_bench".into()) },
        )
        .await
        .map_err(|e| e.to_string())?;

        // The production write API intentionally stamps random audit UUIDs and
        // wall-clock times. Fixture setup is not measured, so materialize the
        // seed-derived canonical records directly before building the index.
        write_generated_corpus(&roots, input)?;
        substrate.reindex().await.map_err(|e| e.to_string())?;
        substrate.doctor_reindex_events_log().map_err(|e| e.to_string())?;

        for (memory, vector) in input.memories.iter().zip(&input.embedding_vectors) {
            let chunk = chunk_memory(memory).into_iter().next().ok_or("fixture memory has no chunk")?;
            substrate
                .update_embedding(EmbeddingUpdate {
                    chunk_id: chunk.chunk_id,
                    expected_chunk_hash: chunk.body_hash,
                    triple: triple.clone(),
                    vector: vector.clone(),
                })
                .await
                .map_err(|e| e.to_string())?;
        }

        // Keep `root` alive by leaking it (the `Fixture` owns the substrate which
        // holds open file handles to the temp dir).
        std::mem::forget(root);
        Ok(Self { roots, substrate, triple })
    }
}

fn write_generated_corpus(roots: &Roots, input: &GeneratedBenchInput) -> Result<(), String> {
    for memory in &input.memories {
        let relative = memory.path.as_ref().ok_or("fixture memory has no path")?;
        let path = roots.repo.join(relative.as_path());
        let parent = path.parent().ok_or("fixture memory path has no parent")?;
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let document = memory_substrate::frontmatter::serialize_document(memory).map_err(|e| e.to_string())?;
        std::fs::write(path, document).map_err(|e| e.to_string())?;
    }
    rewrite_events(&roots.repo.join("events/dev_bench.jsonl"), &input.events).map_err(|e| e.to_string())
}

fn bench_embedding_triple() -> EmbeddingTriple {
    EmbeddingTriple {
        provider: BENCH_EMBEDDING_PROVIDER.to_string(),
        model_ref: BENCH_EMBEDDING_MODEL_REF.to_string(),
        dimension: BENCH_EMBEDDING_DIMENSION,
    }
}

fn write_bench_config(roots: &Roots) -> Result<(), String> {
    std::fs::create_dir_all(&roots.repo).map_err(|e| e.to_string())?;
    std::fs::write(
        roots.repo.join("config.yaml"),
        format!(
            "schema_version: 1\nactive_embedding:\n  provider: {BENCH_EMBEDDING_PROVIDER}\n  model_ref: {BENCH_EMBEDDING_MODEL_REF}\n  dimension: {BENCH_EMBEDDING_DIMENSION}\n",
        ),
    )
    .map_err(|e| e.to_string())
}

fn sample_memory(index: usize, seed: u64) -> Memory {
    let now = chrono::DateTime::<chrono::Utc>::from(std::time::UNIX_EPOCH);
    let id = format!("mem_20260424_a1b2c3d4e5f60718_{index:06}");
    Memory {
        frontmatter: Frontmatter {
            schema_version: 1,
            id: MemoryId::new(id.clone()),
            memory_type: MemoryType::Pattern,
            scope: Scope::Agent,
            summary: format!("bench fixture {index}"),
            confidence: 1.0,
            original_confidence: None,
            trust_level: TrustLevel::Trusted,
            sensitivity: Sensitivity::Internal,
            status: MemoryStatus::Active,
            created_at: now,
            updated_at: now,
            observed_at: None,
            author: Author {
                kind: AuthorKind::System,
                user_handle: None,
                harness: None,
                harness_version: None,
                session_id: None,
                subagent_id: None,
                phase: None,
                component: Some("bench".to_string()),
            },
            namespace: None,
            canonical_namespace_id: None,
            tags: vec!["bench".to_string(), format!("bucket-{}", index % 10)],
            entities: Vec::new(),
            aliases: vec![format!("alias-{index}")],
            source: Source {
                kind: SourceKind::Import,
                reference: None,
                harness: None,
                harness_version: None,
                session_id: None,
                subagent_id: None,
                device: None,
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
                max_scope: Scope::Agent,
                mask_personal_for_synthesis: false,
                index_body: true,
                index_embeddings: true,
            },
            write_policy: WritePolicy {
                human_review_required: false,
                policy_applied: "default-v1".to_string(),
                expected_base_hash: None,
            },
            merge_diagnostics: None,
            abstraction: None,
            cues: Vec::new(),
            extras: std::collections::BTreeMap::new(),
        },
        body: format!("needle-stream-a fixture body {index} seed {seed:x}. {}", "long body ".repeat(index % 8 + 1)),
        path: Some(RepoPath::new(format!("agent/patterns/{id}.md"))),
    }
}

fn sample_event(memory: &Memory, device: &DeviceId, index: usize, seed: u64) -> Event {
    let at = chrono::DateTime::<chrono::Utc>::from(std::time::UNIX_EPOCH)
        + chrono::TimeDelta::microseconds((seed % 1_000_000) as i64);
    Event {
        schema: memory_substrate::SUBSTRATE_SCHEMA_VERSION,
        id: EventId::new(format!("evt_bench_{seed:016x}_{index:08}")),
        at,
        device: device.clone(),
        seq: index as u64 + 1,
        operation_id: Some(OperationId::new(format!("op_bench_{seed:016x}_{index:08}"))),
        kind: EventKind::WriteCommitted {
            id: memory.frontmatter.id.clone(),
            path: memory.path.clone().expect("sample_memory always assigns a path"), // expect-justified: fixture invariant
            classification: ClassificationOutcome::Trusted,
        },
        crc32c: 0,
    }
}

async fn measure_async<F, Fut, E>(work: F) -> Result<f64, String>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<(), E>>,
    E: std::fmt::Display,
{
    let start = Instant::now();
    work().await.map_err(|e| e.to_string())?;
    Ok(start.elapsed().as_secs_f64() * 1000.0)
}

fn measure<E>(work: impl FnOnce() -> Result<(), E>) -> Result<f64, String>
where
    E: std::fmt::Display,
{
    let start = Instant::now();
    work().map_err(|e| e.to_string())?;
    Ok(start.elapsed().as_secs_f64() * 1000.0)
}

/// Compute p50/p95/p99 for a set of values.
///
/// R-RT-4: `noise_floor_ms` is a baseline property, not a result property.
/// It is dropped from results here; `bench-gate.sh` reads it from the baseline.
fn metric(mut values: Vec<f64>) -> serde_json::Value {
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    json!({
        "p50_ms": percentile(&values, 0.50),
        "p95_ms": percentile(&values, 0.95),
        "p99_ms": percentile(&values, 0.99),
    })
}

fn percentile(values: &[f64], quantile: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let rank = ((values.len() - 1) as f64 * quantile).ceil() as usize;
    (values[rank] * 1000.0).round() / 1000.0
}
