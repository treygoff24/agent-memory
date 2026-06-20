//! Shared scaffolding for the per-stream benchmark binaries.
//!
//! The three bench bins (`stream_f_dream_bench`, `stream_g_bench`,
//! `stream_e_recall_bench`) historically each re-declared their own report
//! types, CLI args, and baseline-contract drivers. Those report shapes have
//! diverged on disk (Stream F emits `measured_p95_ms` with optional budgets and
//! no `runs`; Stream G emits `measured_ms` + a required `statistic` + `runs`;
//! Stream E uses a wholly different `BenchResult` table), so the report types and
//! CLI surface intentionally stay per-bin to preserve each baseline's byte-exact
//! serde representation.
//!
//! Only the genuinely identical, behavior-preserving primitives live here: the
//! numeric percentile-to-millis helpers and the immutable-baseline path guard.

use std::path::Path;
use std::time::Duration;

use anyhow::bail;

/// Convert a duration to milliseconds as `f64`.
pub fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

/// Round a millisecond value to three decimal places.
pub fn round3(value: f64) -> f64 {
    (value * 1_000.0).round() / 1_000.0
}

/// Refuse to write a benchmark report to a human-committed `baseline.*.json`
/// path. The `stream_label` is interpolated into the error message so each bin
/// keeps its exact diagnostic (e.g. "Stream F output", "Stream G output").
pub fn guard_immutable_baseline_path(path: &Path, stream_label: &str) -> anyhow::Result<()> {
    if path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("baseline.") && name.ends_with(".json"))
    {
        bail!("refusing to write {stream_label} output to immutable baseline path {}", path.display());
    }
    Ok(())
}
