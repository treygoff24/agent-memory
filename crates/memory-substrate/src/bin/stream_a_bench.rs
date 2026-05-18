//! Real Stream A benchmark harness used by the shell bench gate.
//!
//! Spec §17.6: synthetic vectors come from `memory-test-support::perf` only.
//! Spec §17.6 / §18.9: baselines are immutable; this binary refuses to write
//! to any path matching `baseline.<profile>.json`.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use memory_substrate::index::chunk_memory;
use memory_substrate::{
    Author, AuthorKind, ChunkQuery, ClassificationOutcome, EmbeddingTriple, EmbeddingUpdate, EventContext, Frontmatter,
    InitOptions, Memory, MemoryId, MemoryQuery, MemoryStatus, MemoryType, RepoPath, RetrievalPolicy, Roots, Scope,
    Sensitivity, Source, SourceKind, Substrate, TrustLevel, WriteMode, WritePolicy, WriteRequest,
};
use memory_test_support::perf::{corpus_sha256, synthetic_vector};
use serde_json::json;

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    let args = parse_args()?;
    guard_output_path(&args.output)?;

    let fixture = Fixture::build(args.seed, args.corpus).await?;
    let corpus_hash = corpus_sha256(&fixture.roots.repo);
    let metrics = run_iterations(&fixture, args.runs, args.seed).await?;
    write_report(&args, &metrics, &fixture.coverage(), &corpus_hash, &args.output)?;
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

fn guard_output_path(output: &Path) -> Result<(), String> {
    guard_baseline_path(output)?;
    match std::fs::symlink_metadata(output) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(format!(
            "refusing to write to symlink output '{}'; benchmark reports must not follow links",
            output.display()
        )),
        Ok(_) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(format!("inspect {}: {err}", output.display())),
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

async fn run_iterations(fixture: &Fixture, runs: usize, seed: u64) -> Result<Metrics, String> {
    let corpus = fixture.corpus_size;
    let mut cold_reindex = Vec::with_capacity(runs);
    let mut query_by_id = Vec::with_capacity(runs);
    let mut filtered_metadata_query = Vec::with_capacity(runs);
    let mut fts_chunk_query = Vec::with_capacity(runs);
    let mut vector_chunk_query = Vec::with_capacity(runs);
    let mut tree_validator = Vec::with_capacity(runs);

    for run_index in 0..runs {
        cold_reindex.push(measure_async(|| async { fixture.substrate.reindex().await.map(|_| ()) }).await?);
        query_by_id.push(
            measure_async(|| async {
                fixture
                    .substrate
                    .query_memory(MemoryQuery {
                        id: Some(MemoryId::new(format!(
                            "mem_20260424_a1b2c3d4e5f60718_{:06}",
                            corpus.saturating_sub(1)
                        ))),
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
                        tag: Some("bucket-7".to_string()),
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
                        text: Some(format!("{}", corpus.saturating_sub(1))),
                        triple: None,
                        vector: None,
                    })
                    .await
                    .map(|_| ())
            })
            .await?,
        );
        // B-RT-3: use sanctioned synthetic_vector from memory-test-support (spec §17.6).
        let query_vector = synthetic_vector(seed, fixture.triple.dimension as usize, run_index);
        vector_chunk_query.push(
            measure_async(|| async {
                fixture
                    .substrate
                    .query_chunks(ChunkQuery {
                        text: None,
                        triple: Some(fixture.triple.clone()),
                        vector: Some(query_vector),
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

#[allow(clippy::too_many_arguments)]
fn write_report(
    args: &BenchArgs,
    metrics: &Metrics,
    coverage: &FixtureCoverage,
    corpus_hash: &str,
    output: &Path,
) -> Result<(), String> {
    let report = json!({
        "schema": 1,
        "tier": args.tier,
        "profile": args.profile,
        "runs": args.runs,
        "corpus_size": coverage.corpus_size,
        "seed": format!("0x{:x}", args.seed),
        // B-RT-4: corpus_sha256 + active_triple required by spec §17.6.
        "corpus_sha256": corpus_hash,
        "vector_dimension": 32,
        "vectorized_chunks": coverage.vectorized_chunks,
        "active_triple": { "provider": "synthetic", "model_ref": "stream-a-test", "dimension": 32 },
        "corpus_variants": coverage.corpus_variants,
        "metrics": {
            "cold_reindex": metric(metrics.cold_reindex.clone()),
            "query_by_id": metric(metrics.query_by_id.clone()),
            "filtered_metadata_query": metric(metrics.filtered_metadata_query.clone()),
            "fts_chunk_query": metric(metrics.fts_chunk_query.clone()),
            "vector_chunk_query": metric(metrics.vector_chunk_query.clone()),
            "tree_validator": metric(metrics.tree_validator.clone()),
        }
    });

    write_report_file(output, &(serde_json::to_string_pretty(&report).map_err(|e| e.to_string())? + "\n"))
}

fn write_report_file(output: &Path, content: &str) -> Result<(), String> {
    guard_output_path(output)?;
    if let Some(parent) = non_empty_parent(output) {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let directory = non_empty_parent(output).unwrap_or_else(|| Path::new("."));
    let file_name = output.file_name().ok_or_else(|| format!("output path '{}' has no file name", output.display()))?;
    let temp_path = directory.join(format!(".{}.{}.tmp", file_name.to_string_lossy(), std::process::id()));

    let write_result = (|| -> Result<(), std::io::Error> {
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&temp_path)?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp_path, output)
    })();

    if let Err(err) = write_result {
        let _ = std::fs::remove_file(&temp_path);
        return Err(err.to_string());
    }

    Ok(())
}

fn non_empty_parent(path: &Path) -> Option<&Path> {
    path.parent().filter(|parent| !parent.as_os_str().is_empty())
}

struct Fixture {
    roots: Roots,
    substrate: Substrate,
    // Declared after substrate so Substrate drops before the TempDir cleanup.
    _root: tempfile::TempDir,
    triple: EmbeddingTriple,
    corpus_size: usize,
    vectorized_chunks: usize,
}

impl Fixture {
    async fn build(seed: u64, corpus: usize) -> Result<Self, String> {
        // Use tempfile::TempDir to avoid PID races (R-RT-3).
        let root = tempfile::tempdir().map_err(|e| e.to_string())?;
        let roots = Roots::new(root.path().join("repo"), root.path().join("runtime"));
        let substrate = Substrate::init(
            roots.clone(),
            InitOptions { force_unsafe_durability: true, device_id: Some("dev_bench".into()) },
        )
        .await
        .map_err(|e| e.to_string())?;
        let triple = EmbeddingTriple {
            provider: "synthetic".to_string(),
            model_ref: "stream-a-test".to_string(),
            dimension: 32,
        };

        let mut vectorized_chunks = 0usize;
        for index in 0..corpus {
            let memory = sample_memory(index, seed);
            let chunk = chunk_memory(&memory).into_iter().next().ok_or("fixture memory has no chunk")?;
            substrate
                .write_memory(WriteRequest {
                    operation_id: None,
                    memory: memory.clone(),
                    expected_base_hash: None,
                    write_mode: WriteMode::CreateNew,
                    index_projection: None,
                    event_context: EventContext::default(),
                    allow_best_effort_durability: true,
                    classification: ClassificationOutcome::Trusted,
                })
                .await
                .map_err(|e| e.to_string())?;
            if index < 32 {
                substrate
                    .update_embedding(EmbeddingUpdate {
                        chunk_id: chunk.chunk_id,
                        expected_chunk_hash: chunk.body_hash,
                        triple: triple.clone(),
                        // B-RT-3: sanctioned synthetic_vector from memory-test-support.
                        vector: synthetic_vector(seed, triple.dimension as usize, index),
                    })
                    .await
                    .map_err(|e| e.to_string())?;
                vectorized_chunks += 1;
            }
        }

        Ok(Self { roots, substrate, _root: root, triple, corpus_size: corpus, vectorized_chunks })
    }

    fn coverage(&self) -> FixtureCoverage {
        FixtureCoverage {
            corpus_size: self.corpus_size,
            vectorized_chunks: self.vectorized_chunks,
            corpus_variants: ACTUAL_CORPUS_VARIANTS,
        }
    }
}

struct FixtureCoverage {
    corpus_size: usize,
    vectorized_chunks: usize,
    corpus_variants: &'static [&'static str],
}

const ACTUAL_CORPUS_VARIANTS: &[&str] =
    &["active_plaintext_internal", "aliases", "tag_buckets", "variable_body_lengths"];

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
            extras: std::collections::BTreeMap::new(),
        },
        body: format!("needle-stream-a fixture body {index} seed {seed:x}. {}", "long body ".repeat(index % 8 + 1)),
        path: Some(RepoPath::new(format!("agent/patterns/{id}.md"))),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_args(output: PathBuf) -> BenchArgs {
        BenchArgs {
            tier: "smoke".to_string(),
            profile: "test".to_string(),
            output,
            runs: 1,
            corpus: 40,
            seed: memory_test_support::perf::SEED_SMOKE,
        }
    }

    fn sample_metrics() -> Metrics {
        Metrics {
            cold_reindex: vec![1.0],
            query_by_id: vec![1.0],
            filtered_metadata_query: vec![1.0],
            fts_chunk_query: vec![1.0],
            vector_chunk_query: vec![1.0],
            tree_validator: vec![1.0],
        }
    }

    #[test]
    fn write_report_records_actual_fixture_coverage() {
        let temp = tempfile::tempdir().expect("tempdir");
        let output = temp.path().join("report.json");
        let coverage =
            FixtureCoverage { corpus_size: 40, vectorized_chunks: 32, corpus_variants: ACTUAL_CORPUS_VARIANTS };

        write_report(&sample_args(output.clone()), &sample_metrics(), &coverage, "hash", &output)
            .expect("write report");

        let report: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(output).expect("read report")).expect("parse report");
        assert_eq!(report["corpus_size"], 40);
        assert_eq!(report["vectorized_chunks"], 32);
        assert_eq!(
            report["corpus_variants"],
            json!(["active_plaintext_internal", "aliases", "tag_buckets", "variable_body_lengths"])
        );
    }

    #[test]
    fn write_report_file_accepts_bare_output_filename() {
        let _guard = current_dir_lock().lock().expect("current dir lock");
        let original_dir = std::env::current_dir().expect("current dir");
        let temp = tempfile::tempdir().expect("tempdir");
        std::env::set_current_dir(temp.path()).expect("switch to temp dir");

        let result = write_report_file(Path::new("report.json"), "{}\n");
        let written = std::fs::read_to_string("report.json");

        std::env::set_current_dir(original_dir).expect("restore current dir");
        result.expect("write bare report");
        assert_eq!(written.expect("read bare report"), "{}\n");
    }

    #[test]
    fn guard_output_path_rejects_baseline_filename() {
        let err = guard_output_path(Path::new("bench/baseline.dev.json")).expect_err("baseline rejected");
        assert!(err.contains("refusing to write to baseline path"));
    }

    #[cfg(unix)]
    #[test]
    fn write_report_file_rejects_symlink_output_without_touching_target() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().expect("tempdir");
        let target = temp.path().join("baseline.dev.json");
        let link = temp.path().join("latest.json");
        std::fs::write(&target, "baseline\n").expect("write target");
        symlink(&target, &link).expect("symlink");

        let err = write_report_file(&link, "new report\n").expect_err("symlink output rejected");

        assert!(err.contains("refusing to write to symlink output"));
        assert_eq!(std::fs::read_to_string(target).expect("read target"), "baseline\n");
    }

    fn current_dir_lock() -> &'static std::sync::Mutex<()> {
        static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
        LOCK.get_or_init(|| std::sync::Mutex::new(()))
    }
}
