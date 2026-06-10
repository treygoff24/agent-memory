//! Quality-metrics baseline gate (Task 4.2).
//!
//! Runs the golden-corpus quality report through the REAL recall ranking seams
//! (bm25 FTS + structural points) and compares it against the human-committed
//! baseline at `bench/quality-baseline.json`.
//!
//! ## No-baseline skip
//!
//! Per the CLAUDE.md `bench/baseline.*` convention, the baseline is
//! **human-committed only** — the runner never writes it. So when the baseline
//! file does not exist yet, this gate **skips cleanly** with a clear message
//! rather than failing: Trey commits the initial baseline after reviewing the
//! first run's emitted JSON (`memorum-eval-quality --output-file ...`). Once the
//! baseline exists, this test enforces no-regression within a tolerance band.
//!
//! The report-shape sanity assertions below always run (no baseline needed), so
//! a broken runner still fails the gate even before a baseline exists.

#![cfg(feature = "quality")]

use memorum_eval::quality::{
    compare_to_baseline, run_quality_report, GateOutcome, QualityReport, DEFAULT_TOLERANCE, K_VALUES,
};

fn build_report() -> QualityReport {
    memorum_eval::block_on(run_quality_report()).expect("quality report runs against the golden corpus")
}

#[test]
fn report_is_well_formed() {
    let report = build_report();

    // 50 labeled cases. Abstention = empty essential AND useful: q47-q50 plus
    // the two correction/tombstone-trap cases (q45 current-machine, q46
    // third-payment-processor) whose only correct answer is "nothing current"
    // (the README notes some abstention cases still list traps). That is 6, not
    // the 4 the task brief estimated. Pin the real count so a corpus edit that
    // changes it surfaces here.
    assert_eq!(report.total_cases, 50, "golden corpus should hold 50 labeled query cases");
    assert_eq!(report.abstention_cases, 6, "expected 6 abstention cases (q45, q46, q47-q50)");
    assert!(report.seams.contains_key("search"), "search seam must be present");
    assert!(report.seams.contains_key("startup"), "startup seam must be present");

    let scored = report.total_cases - report.abstention_cases;
    for (seam, metrics) in &report.seams {
        assert_eq!(metrics.scored_cases, scored, "{seam}: every non-abstention case must be scored");
        for k in K_VALUES {
            let key = k.to_string();
            let p = metrics.precision_at_k[&key];
            let r = metrics.recall_at_k[&key];
            let n = metrics.ndcg_at_k[&key];
            assert!((0.0..=1.0).contains(&p), "{seam}: precision@{k} out of range: {p}");
            assert!((0.0..=1.0).contains(&r), "{seam}: recall@{k} out of range: {r}");
            assert!((0.0..=1.0).contains(&n), "{seam}: ndcg@{k} out of range: {n}");
        }
        assert!((0.0..=1.0).contains(&metrics.mrr), "{seam}: mrr out of range: {}", metrics.mrr);
        assert!(
            (0.0..=1.0).contains(&metrics.trap_rate_at_5),
            "{seam}: trap_rate@5 out of range: {}",
            metrics.trap_rate_at_5
        );
    }

    // Abstention outcomes: one entry per (case, seam).
    assert_eq!(
        report.abstentions.len(),
        report.abstention_cases * report.seams.len(),
        "one abstention outcome per (case, seam)"
    );
}

#[test]
fn startup_seam_carries_graded_relevance_signal() {
    // The startup seam runs the real structural points ranking over
    // namespace-scoped candidates. Across 44 scored cases it must surface
    // *some* relevant memories — a nonzero MRR and nDCG@5 prove the ranking
    // path ran end-to-end against the indexed corpus (a no-op reimplementation
    // or an unindexed corpus would score a flat zero). The exact values land in
    // the committed baseline; here we only assert the instrument is live.
    let report = build_report();
    let startup = &report.seams["startup"];
    assert!(startup.mrr > 0.0, "startup seam MRR should be > 0 (real ranking must surface relevant memories)");
    assert!(startup.ndcg_at_k["5"] > 0.0, "startup seam nDCG@5 should be > 0");

    // The search seam is keyword bm25 fed natural-language queries; near-zero is
    // its honest behavior (documented in report.seam_notes), so we do NOT assert
    // a floor on it — only that the report carries the explanatory note.
    assert!(
        report.seam_notes.get("search").is_some_and(|n| n.contains("bm25")),
        "search seam must carry its bm25/FTS-AND interpretation note"
    );
}

#[test]
fn baseline_gate_skips_cleanly_when_absent_else_enforces() {
    let report = build_report();
    match compare_to_baseline(&report, DEFAULT_TOLERANCE).expect("baseline comparison runs") {
        GateOutcome::SkippedNoBaseline => {
            // Expected before the human commits the first baseline. Not a
            // failure — the first run's JSON is reviewed and committed by hand.
            eprintln!(
                "quality baseline not present yet — gate skipped. \
                 Run `cargo run -p memorum-eval --bin memorum-eval-quality -- --output-file bench/quality-baseline.json` \
                 and commit the reviewed JSON to establish the baseline."
            );
        }
        GateOutcome::Pass => {
            eprintln!("quality baseline gate PASS (within tolerance {DEFAULT_TOLERANCE}).");
        }
        GateOutcome::Regressed(regressions) => {
            panic!("quality metrics regressed beyond tolerance:\n  - {}", regressions.join("\n  - "));
        }
    }
}
