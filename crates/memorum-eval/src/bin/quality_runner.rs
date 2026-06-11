//! Standalone golden-corpus quality-metrics runner (Task 4.2).
//!
//! Loads the golden corpus into a scratch substrate, replays every labeled query
//! through the real recall ranking seams (bm25 FTS + structural points), and
//! emits the metrics report as JSON. Dynamics strength is pinned off for this
//! quality gate so ambient `MEMORUM_DYNAMICS` cannot change the report. The
//! baseline gate (the test target) is what enforces no-regression in CI; this
//! binary is for producing a JSON report that a human may review and commit as
//! `bench/quality-baseline.json`, and for ad-hoc local runs.
//!
//! Usage:
//!   memorum-eval-quality                       # JSON to stdout
//!   memorum-eval-quality --output-file PATH     # also write JSON to PATH
//!   memorum-eval-quality --check                # also compare to baseline
//!   memorum-eval-quality --corpus-root DIR      # bring-your-own corpus
//!   memorum-eval-quality --dump-cases PATH      # per-case outcome JSON
//!   memorum-eval-quality --fused-report PATH    # write fused search metrics
//!   memorum-eval-quality --embedding fixture    # fused provider selector
//!
//! `--corpus-root` replays an arbitrary corpus (`DIR/memories` +
//! `DIR/queries.yaml`) through the same seams — e.g. a private, machine-local
//! corpus distilled from real projects. The regression gate ignores it and
//! stays pinned to the committed fixtures; `--check` is refused under a custom
//! root since the committed baseline only describes the committed corpus.
//!
//! It never writes the baseline file — that is human-committed only.

use std::path::PathBuf;
use std::process::ExitCode;

use memorum_eval::quality::{
    self, compare_to_baseline, report_to_json, run_fused_quality_report_for_root,
    run_quality_report_with_cases_for_root, GateOutcome, GoldenCorpus, QualityEmbeddingProvider, DEFAULT_TOLERANCE,
};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut output_file: Option<PathBuf> = None;
    let mut check = false;
    let mut corpus_root: Option<PathBuf> = None;
    let mut dump_cases: Option<PathBuf> = None;
    let mut fused_report: Option<PathBuf> = None;
    let mut embedding_provider = QualityEmbeddingProvider::Fixture;
    let mut embedding_flag_seen = false;

    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--output-file" => {
                output_file = iter.next().map(PathBuf::from);
                if output_file.is_none() {
                    eprintln!("--output-file requires a path");
                    return ExitCode::from(2);
                }
            }
            "--corpus-root" => {
                corpus_root = iter.next().map(PathBuf::from);
                if corpus_root.is_none() {
                    eprintln!("--corpus-root requires a directory");
                    return ExitCode::from(2);
                }
            }
            "--dump-cases" => {
                dump_cases = iter.next().map(PathBuf::from);
                if dump_cases.is_none() {
                    eprintln!("--dump-cases requires a path");
                    return ExitCode::from(2);
                }
            }
            "--fused-report" => {
                fused_report = iter.next().map(PathBuf::from);
                if fused_report.is_none() {
                    eprintln!("--fused-report requires a path");
                    return ExitCode::from(2);
                }
            }
            "--embedding" => {
                let Some(value) = iter.next() else {
                    eprintln!("--embedding requires one of: fixture, real");
                    return ExitCode::from(2);
                };
                embedding_flag_seen = true;
                embedding_provider = match value.as_str() {
                    "fixture" => QualityEmbeddingProvider::Fixture,
                    "real" => QualityEmbeddingProvider::Real,
                    other => {
                        eprintln!("unknown --embedding value `{other}`; expected fixture or real");
                        return ExitCode::from(2);
                    }
                };
            }
            "--check" => check = true,
            "-h" | "--help" => {
                println!(
                    "memorum-eval-quality [--output-file PATH] [--check] [--corpus-root DIR] \
                     [--dump-cases PATH] [--fused-report PATH] [--embedding fixture|real]"
                );
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("unknown argument: {other}");
                return ExitCode::from(2);
            }
        }
    }

    if check && corpus_root.is_some() {
        eprintln!("--check is only meaningful against the committed corpus; drop it or drop --corpus-root");
        return ExitCode::from(2);
    }
    if check && fused_report.is_some() {
        eprintln!("--check cannot be combined with --fused-report; fused reports are off-gate measurements");
        return ExitCode::from(2);
    }
    if embedding_flag_seen && fused_report.is_none() {
        eprintln!("--embedding only applies when --fused-report is present");
        return ExitCode::from(2);
    }

    let root = corpus_root.unwrap_or_else(GoldenCorpus::fixtures_root);
    let (report, case_outcomes) = match memorum_eval::block_on(run_quality_report_with_cases_for_root(&root)) {
        Ok(pair) => pair,
        Err(error) => {
            eprintln!("quality run failed: {error}");
            return ExitCode::FAILURE;
        }
    };

    if let Some(path) = &dump_cases {
        let json = match serde_json::to_string_pretty(&case_outcomes) {
            Ok(json) => json,
            Err(error) => {
                eprintln!("failed to serialize case outcomes: {error}");
                return ExitCode::FAILURE;
            }
        };
        if let Err(error) = std::fs::write(path, format!("{json}\n")) {
            eprintln!("failed to write {}: {error}", path.display());
            return ExitCode::FAILURE;
        }
        eprintln!("wrote {} case outcomes to {}", case_outcomes.len(), path.display());
    }

    let json = report_to_json(&report);
    println!("{json}");

    if let Some(path) = &output_file {
        if let Err(error) = std::fs::write(path, format!("{json}\n")) {
            eprintln!("failed to write {}: {error}", path.display());
            return ExitCode::FAILURE;
        }
        eprintln!("wrote quality report to {}", path.display());
    }

    // Human-readable headline summary to stderr (keeps stdout pure JSON).
    print_headline(&report);

    if let Some(path) = &fused_report {
        let fused = match memorum_eval::block_on(run_fused_quality_report_for_root(&root, embedding_provider)) {
            Ok(report) => report,
            Err(error) => {
                eprintln!("fused quality run failed: {error}");
                return ExitCode::FAILURE;
            }
        };
        let fused_json = match serde_json::to_string_pretty(&fused) {
            Ok(json) => json,
            Err(error) => {
                eprintln!("failed to serialize fused report: {error}");
                return ExitCode::FAILURE;
            }
        };
        if let Err(error) = std::fs::write(path, format!("{fused_json}\n")) {
            eprintln!("failed to write {}: {error}", path.display());
            return ExitCode::FAILURE;
        }
        eprintln!("wrote fused quality report to {}", path.display());
        print_fused_headline(&fused);
    }

    if check {
        match compare_to_baseline(&report, DEFAULT_TOLERANCE) {
            Ok(GateOutcome::SkippedNoBaseline) => {
                eprintln!(
                    "baseline gate SKIPPED: {} not present yet (commit this run's JSON to establish it).",
                    quality::baseline_path().display()
                );
            }
            Ok(GateOutcome::Pass) => eprintln!("baseline gate PASS (within tolerance {DEFAULT_TOLERANCE})."),
            Ok(GateOutcome::Regressed(regressions)) => {
                eprintln!("baseline gate FAIL — {} regression(s):", regressions.len());
                for regression in &regressions {
                    eprintln!("  - {regression}");
                }
                return ExitCode::FAILURE;
            }
            Err(error) => {
                eprintln!("baseline comparison error: {error}");
                return ExitCode::FAILURE;
            }
        }
    }

    ExitCode::SUCCESS
}

fn print_fused_headline(report: &quality::FusedQualityReport) {
    let m = &report.metrics;
    eprintln!(
        "  [fused_search:{}] scored={} nDCG@5={:.4} recall@5={:.4} precision@5={:.4} MRR={:.4} trap-rate@5={:.4}",
        report.embedding_provider,
        m.scored_cases,
        m.ndcg_at_k.get("5").copied().unwrap_or_default(),
        m.recall_at_k.get("5").copied().unwrap_or_default(),
        m.precision_at_k.get("5").copied().unwrap_or_default(),
        m.mrr,
        m.trap_rate_at_5,
    );
    let trapped_abstentions = report.abstentions.iter().filter(|outcome| outcome.surfaced_trap_at_5).count();
    eprintln!(
        "  [fused_search:{}] abstentions={} trapped_abstentions={}",
        report.embedding_provider, report.abstention_cases, trapped_abstentions
    );
}

fn print_headline(report: &quality::QualityReport) {
    eprintln!("--- quality headline ({}) ---", report.ranking_lane);
    eprintln!("cases: {} total, {} abstention", report.total_cases, report.abstention_cases);
    for (seam, m) in &report.seams {
        eprintln!(
            "  [{seam}] scored={} nDCG@5={:.4} recall@5={:.4} precision@5={:.4} MRR={:.4} trap-rate@5={:.4}",
            m.scored_cases,
            m.ndcg_at_k.get("5").copied().unwrap_or_default(),
            m.recall_at_k.get("5").copied().unwrap_or_default(),
            m.precision_at_k.get("5").copied().unwrap_or_default(),
            m.mrr,
            m.trap_rate_at_5,
        );
    }
}
