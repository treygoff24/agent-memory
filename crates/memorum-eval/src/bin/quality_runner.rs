//! Standalone golden-corpus quality-metrics runner (Task 4.2).
//!
//! Loads the golden corpus into a scratch substrate, replays every labeled query
//! through the real recall ranking seams (bm25 FTS + structural points), and
//! emits the metrics report as JSON. The baseline gate (the test target) is what
//! enforces no-regression in CI; this binary is for producing the first run's
//! JSON (which a human reviews and commits as `bench/quality-baseline.json`) and
//! for ad-hoc local runs.
//!
//! Usage:
//!   memorum-eval-quality                       # JSON to stdout
//!   memorum-eval-quality --output-file PATH     # also write JSON to PATH
//!   memorum-eval-quality --check                # also compare to baseline
//!
//! It never writes the baseline file — that is human-committed only.

use std::path::PathBuf;
use std::process::ExitCode;

use memorum_eval::quality::{
    self, compare_to_baseline, report_to_json, run_quality_report, GateOutcome, DEFAULT_TOLERANCE,
};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut output_file: Option<PathBuf> = None;
    let mut check = false;

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
            "--check" => check = true,
            "-h" | "--help" => {
                println!("memorum-eval-quality [--output-file PATH] [--check]");
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("unknown argument: {other}");
                return ExitCode::from(2);
            }
        }
    }

    let report = match memorum_eval::block_on(run_quality_report()) {
        Ok(report) => report,
        Err(error) => {
            eprintln!("quality run failed: {error}");
            return ExitCode::FAILURE;
        }
    };

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
