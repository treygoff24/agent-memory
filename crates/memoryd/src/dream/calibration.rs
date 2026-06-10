//! Review-decision calibration log (memory-dynamics-v0.1 §6).
//!
//! Every review decision (accept / reject / edit) on a dream-sourced or
//! quarantined candidate appends one JSONL record to
//! `dreams/calibration/<device_id>.jsonl`. The file is git-synced and per-device:
//! like the event log, two devices never write the same path, so concurrent
//! devices' records merge by plain concatenation.
//!
//! **Privacy posture (dynamics spec §6):** ids and metadata only — *no memory
//! content* ever crosses into this file, because it syncs in plaintext. The
//! record carries the candidate id, scope string, author kind, the candidate's
//! self-reported confidence, the decision, an optional edit-distance ratio, the
//! decision timestamp, and an optional session id. None of those is memory body.
//!
//! **Not gated by `dynamics.enabled`** (spec §7): review-outcome collection must
//! never silently stop, so the write is unconditional on every eligible decision.
//!
//! The consumer is `memoryd dream calibration`, which reads every device's file
//! directly (no daemon round-trip) and reports accept-rate per confidence decile.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use memory_substrate::model::{AuthorKind, Scope};
use memory_substrate::DurabilityTier;
use serde::{Deserialize, Serialize};

/// Calibration log record schema version. Bump only on a breaking shape change;
/// the reader tolerates and skips records it cannot decode.
const CALIBRATION_RECORD_VERSION: u32 = 1;

/// Repo-relative directory holding per-device calibration logs.
pub const CALIBRATION_DIR: &str = "dreams/calibration";

/// A single review decision, as it lands in the calibration log.
///
/// Matches dynamics-spec §6. `edit_distance_ratio` is present iff
/// `decision == Edit`; `session_id` is present when the decision path carries a
/// session id (the current daemon approve/reject protocol does not, so it is
/// commonly `None`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CalibrationRecord {
    /// Schema version of this record.
    pub v: u32,
    /// Candidate memory id the decision was made against.
    pub candidate_id: String,
    /// Scope string, `<kind>` or `<kind>:<namespace_id>` (e.g. `project:proj_a3f2`).
    pub scope: String,
    /// Author principal kind of the candidate (snake_case: `dreaming`, ...).
    pub author_kind: AuthorKind,
    /// The candidate's own self-reported confidence at decision time, `[0,1]`.
    pub self_reported_confidence: f64,
    /// The review decision.
    pub decision: Decision,
    /// `levenshtein(old,new)/max(len)` — present only for `Edit` decisions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edit_distance_ratio: Option<f64>,
    /// When the decision was made (UTC, RFC3339).
    pub decided_at: DateTime<Utc>,
    /// Session id of the deciding session, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

/// A review decision class.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    /// Candidate approved into the active set.
    Accept,
    /// Candidate rejected (archived).
    Reject,
    /// Candidate approved after human edits.
    Edit,
}

/// Render a `(Scope, namespace_id)` pair to the dynamics-spec scope string.
///
/// Mirrors `dream::scope::DreamScope::as_str`: `me` / `agent` for the scopeless
/// kinds, `project:<id>` / `org:<id>` for the namespaced kinds. A namespaced
/// scope missing its id degrades to the bare kind rather than emitting a
/// dangling `:`.
pub fn scope_string(scope: Scope, namespace_id: Option<&str>) -> String {
    match scope {
        Scope::User => "me".to_string(),
        Scope::Agent => "agent".to_string(),
        Scope::Subagent => "subagent".to_string(),
        Scope::Project => prefixed("project", namespace_id),
        Scope::Org => prefixed("org", namespace_id),
    }
}

fn prefixed(kind: &str, namespace_id: Option<&str>) -> String {
    match namespace_id {
        Some(id) if !id.is_empty() => format!("{kind}:{id}"),
        _ => kind.to_string(),
    }
}

/// Inputs to one calibration append, gathered at the review-decision site.
pub struct DecisionRecord {
    /// Candidate memory id.
    pub candidate_id: String,
    /// Scope string (build with [`scope_string`]).
    pub scope: String,
    /// Candidate author kind.
    pub author_kind: AuthorKind,
    /// Candidate self-reported confidence.
    pub self_reported_confidence: f64,
    /// Decision class.
    pub decision: Decision,
    /// Edit-distance ratio (only meaningful for `Edit`).
    pub edit_distance_ratio: Option<f64>,
    /// Decision timestamp.
    pub decided_at: DateTime<Utc>,
    /// Optional deciding session id.
    pub session_id: Option<String>,
}

/// Repo-relative path to a device's calibration log.
pub fn calibration_log_path(device_id: &str) -> String {
    format!("{CALIBRATION_DIR}/{device_id}.jsonl")
}

/// Append one decision record to `dreams/calibration/<device_id>.jsonl` under
/// `repo`.
///
/// Atomic-ish on the same footing as the substrate event log
/// ([`api::append_jsonl_record`]): the record is serialized to a single buffer
/// with its trailing newline and written with one `write_all`, so a concurrent
/// reader never sees a half-written line. On the `Full` durability tier the file
/// and its parent directory are fsync'd, matching the event log's durability
/// contract; lower tiers skip the fsync exactly as best-effort event appends do.
pub fn append_decision(
    repo: &Path,
    device_id: &str,
    durability: DurabilityTier,
    record: &DecisionRecord,
) -> std::io::Result<()> {
    let record = CalibrationRecord {
        v: CALIBRATION_RECORD_VERSION,
        candidate_id: record.candidate_id.clone(),
        scope: record.scope.clone(),
        author_kind: record.author_kind,
        self_reported_confidence: record.self_reported_confidence,
        decision: record.decision,
        // Edit-distance is meaningful only for edits; drop it for accept/reject
        // so the record shape matches the spec ("present iff decision == edit").
        edit_distance_ratio: match record.decision {
            Decision::Edit => record.edit_distance_ratio,
            Decision::Accept | Decision::Reject => None,
        },
        decided_at: record.decided_at,
        session_id: record.session_id.clone(),
    };

    let dir = repo.join(CALIBRATION_DIR);
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{device_id}.jsonl"));

    // Serialize the whole line (record + newline) into one buffer, then write it
    // with a single syscall so a concurrent reader sees whole lines only.
    let mut line = serde_json::to_vec(&record).map_err(std::io::Error::other)?;
    line.push(b'\n');

    let mut file = fs::OpenOptions::new().create(true).append(true).open(&path)?;
    file.write_all(&line)?;
    if matches!(durability, DurabilityTier::Full) {
        file.sync_all()?;
        if let Some(parent) = path.parent() {
            fs::File::open(parent)?.sync_all()?;
        }
    }
    Ok(())
}

/// Number of confidence deciles (`[0.0,0.1)`, ..., `[0.9,1.0]`).
pub const DECILE_COUNT: usize = 10;

/// Per-decile aggregate for the calibration report.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DecileBucket {
    /// Decile index `0..=9`.
    pub decile: usize,
    /// Inclusive-exclusive confidence range, `[lower, upper)` — the top bucket
    /// is `[0.9, 1.0]` (inclusive upper).
    pub lower: f64,
    /// Upper bound of the bucket.
    pub upper: f64,
    /// Total decisions in this bucket.
    pub total: usize,
    /// Accept decisions (`Accept` only — edits are a distinct outcome).
    pub accepts: usize,
    /// Reject decisions.
    pub rejects: usize,
    /// Edit decisions.
    pub edits: usize,
    /// Accept rate = accepts / total, `None` when the bucket is empty.
    pub accept_rate: Option<f64>,
    /// Edit rate = edits / total, `None` when empty.
    pub edit_rate: Option<f64>,
}

/// Full calibration report: per-decile buckets plus corpus totals.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CalibrationReport {
    /// Total records across all device files.
    pub total: usize,
    /// Records skipped because they could not be decoded.
    pub skipped: usize,
    /// Total accepts across all buckets.
    pub accepts: usize,
    /// Total rejects.
    pub rejects: usize,
    /// Total edits.
    pub edits: usize,
    /// Overall accept rate, `None` when there are no records.
    pub accept_rate: Option<f64>,
    /// The ten confidence deciles, lowest first.
    pub deciles: Vec<DecileBucket>,
}

/// Map a confidence in `[0,1]` to its decile index `0..=9`.
///
/// Buckets are `[0.0,0.1)`, `[0.1,0.2)`, ..., `[0.9,1.0]`. Out-of-range values
/// clamp: anything `<= 0.0` lands in bucket 0, anything `>= 1.0` (including the
/// exact upper edge) lands in bucket 9. This keeps `1.0` — a legitimate
/// confidence — out of a phantom 11th bucket.
pub fn decile_index(confidence: f64) -> usize {
    if confidence.is_nan() || confidence <= 0.0 {
        return 0;
    }
    if confidence >= 1.0 {
        return DECILE_COUNT - 1;
    }
    // `confidence` is strictly in (0.0, 1.0) here, so the product is in (0, 10).
    ((confidence * DECILE_COUNT as f64).floor() as usize).min(DECILE_COUNT - 1)
}

/// Compute a report from an iterator of records (the testable core).
pub fn report_from_records<I>(records: I) -> CalibrationReport
where
    I: IntoIterator<Item = CalibrationRecord>,
{
    let mut buckets: Vec<(usize, usize, usize)> = vec![(0, 0, 0); DECILE_COUNT]; // (accepts, rejects, edits)
    let mut total = 0usize;
    for record in records {
        let bucket = &mut buckets[decile_index(record.self_reported_confidence)];
        match record.decision {
            Decision::Accept => bucket.0 += 1,
            Decision::Reject => bucket.1 += 1,
            Decision::Edit => bucket.2 += 1,
        }
        total += 1;
    }

    let mut accepts = 0;
    let mut rejects = 0;
    let mut edits = 0;
    let deciles = buckets
        .iter()
        .enumerate()
        .map(|(decile, &(a, r, e))| {
            accepts += a;
            rejects += r;
            edits += e;
            let bucket_total = a + r + e;
            DecileBucket {
                decile,
                lower: decile as f64 / DECILE_COUNT as f64,
                upper: (decile + 1) as f64 / DECILE_COUNT as f64,
                total: bucket_total,
                accepts: a,
                rejects: r,
                edits: e,
                accept_rate: rate(a, bucket_total),
                edit_rate: rate(e, bucket_total),
            }
        })
        .collect();

    CalibrationReport { total, skipped: 0, accepts, rejects, edits, accept_rate: rate(accepts, total), deciles }
}

fn rate(numerator: usize, denominator: usize) -> Option<f64> {
    (denominator > 0).then(|| numerator as f64 / denominator as f64)
}

/// Read every device's calibration log under `repo/dreams/calibration/` and
/// compute the report. Lines that fail to decode are counted in `skipped`
/// rather than failing the whole report — a forward-compatible-version or a
/// merge-corrupted tail should not blind the operator to the rest of the data.
pub fn build_report(repo: &Path) -> std::io::Result<CalibrationReport> {
    let dir = repo.join(CALIBRATION_DIR);
    let mut records = Vec::new();
    let mut skipped = 0usize;
    for path in device_log_paths(&dir)? {
        let contents = match fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => return Err(err),
        };
        for line in contents.lines() {
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<CalibrationRecord>(line) {
                Ok(record) => records.push(record),
                Err(_) => skipped += 1,
            }
        }
    }
    let mut report = report_from_records(records);
    report.skipped = skipped;
    Ok(report)
}

/// All `*.jsonl` files directly under the calibration dir, sorted for
/// deterministic ordering. A missing directory yields an empty list.
fn device_log_paths(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(paths),
        Err(err) => return Err(err),
    };
    for entry in entries {
        let path = entry?.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

/// Render the report as a human-readable table.
pub fn render_human_report(report: &CalibrationReport) -> String {
    let mut out = String::new();
    out.push_str("Review-decision calibration\n");
    out.push_str(&format!(
        "  total={}  accept={}  reject={}  edit={}",
        report.total, report.accepts, report.rejects, report.edits
    ));
    match report.accept_rate {
        Some(rate) => out.push_str(&format!("  accept_rate={rate:.2}\n")),
        None => out.push('\n'),
    }
    if report.skipped > 0 {
        out.push_str(&format!("  skipped (undecodable) lines: {}\n", report.skipped));
    }
    out.push('\n');
    out.push_str("  confidence    n   acc  rej  edt  accept_rate\n");
    out.push_str("  ----------  ----  ---  ---  ---  -----------\n");
    for bucket in &report.deciles {
        let accept_rate = match bucket.accept_rate {
            Some(rate) => format!("{rate:.2}"),
            None => "-".to_string(),
        };
        out.push_str(&format!(
            "  [{:.1},{:.1}{}  {:>4}  {:>3}  {:>3}  {:>3}  {:>11}\n",
            bucket.lower,
            bucket.upper,
            if bucket.decile == DECILE_COUNT - 1 { "]" } else { ")" },
            bucket.total,
            bucket.accepts,
            bucket.rejects,
            bucket.edits,
            accept_rate,
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn record(confidence: f64, decision: Decision) -> CalibrationRecord {
        CalibrationRecord {
            v: CALIBRATION_RECORD_VERSION,
            candidate_id: "mem_test".to_string(),
            scope: "me".to_string(),
            author_kind: AuthorKind::Dreaming,
            self_reported_confidence: confidence,
            decision,
            edit_distance_ratio: matches!(decision, Decision::Edit).then_some(0.2),
            decided_at: Utc.with_ymd_and_hms(2026, 6, 9, 19, 4, 11).unwrap(),
            session_id: None,
        }
    }

    #[test]
    fn decile_index_buckets_edges() {
        assert_eq!(decile_index(0.0), 0);
        assert_eq!(decile_index(-0.5), 0, "negative clamps to 0");
        assert_eq!(decile_index(0.05), 0);
        assert_eq!(decile_index(0.1), 1, "lower edge belongs to the higher bucket");
        assert_eq!(decile_index(0.19), 1);
        assert_eq!(decile_index(0.5), 5);
        assert_eq!(decile_index(0.9), 9);
        assert_eq!(decile_index(0.99), 9);
        assert_eq!(decile_index(1.0), 9, "exact 1.0 stays in the top bucket, not an 11th");
        assert_eq!(decile_index(1.5), 9, "over-range clamps to top");
        assert_eq!(decile_index(f64::NAN), 0, "NaN clamps to 0");
    }

    #[test]
    fn report_math_accept_rate_per_bucket() {
        let records = vec![
            record(0.05, Decision::Accept),
            record(0.05, Decision::Reject),
            record(0.85, Decision::Accept),
            record(0.85, Decision::Accept),
            record(0.85, Decision::Reject),
            record(0.95, Decision::Accept),
            record(0.95, Decision::Edit),
        ];
        let report = report_from_records(records);
        assert_eq!(report.total, 7);
        assert_eq!(report.accepts, 4);
        assert_eq!(report.rejects, 2);
        assert_eq!(report.edits, 1);
        // bucket 0 [0.0,0.1): 1 accept / 1 reject → 0.5
        assert_eq!(report.deciles[0].total, 2);
        assert_eq!(report.deciles[0].accept_rate, Some(0.5));
        // bucket 8 [0.8,0.9): 2 accept / 1 reject → 0.666...
        assert_eq!(report.deciles[8].total, 3);
        assert_eq!(report.deciles[8].accepts, 2);
        assert!((report.deciles[8].accept_rate.unwrap() - 2.0 / 3.0).abs() < 1e-9);
        // bucket 9 [0.9,1.0]: 1 accept / 1 edit → accept_rate 0.5, edit_rate 0.5
        assert_eq!(report.deciles[9].total, 2);
        assert_eq!(report.deciles[9].accept_rate, Some(0.5));
        assert_eq!(report.deciles[9].edit_rate, Some(0.5));
        // empty bucket → None, not 0.0
        assert_eq!(report.deciles[1].total, 0);
        assert_eq!(report.deciles[1].accept_rate, None);
    }

    #[test]
    fn append_then_build_round_trips() {
        let temp = tempfile::tempdir().expect("tempdir");
        let repo = temp.path();
        let device = "dev_abc123";
        for (confidence, decision) in [(0.82, Decision::Accept), (0.30, Decision::Reject), (0.91, Decision::Edit)] {
            append_decision(
                repo,
                device,
                DurabilityTier::BestEffort,
                &DecisionRecord {
                    candidate_id: "mem_round".to_string(),
                    scope: "project:proj_a3f2".to_string(),
                    author_kind: AuthorKind::Dreaming,
                    self_reported_confidence: confidence,
                    decision,
                    edit_distance_ratio: matches!(decision, Decision::Edit).then_some(0.18),
                    decided_at: Utc::now(),
                    session_id: Some("sess_x".to_string()),
                },
            )
            .expect("append");
        }
        let report = build_report(repo).expect("build report");
        assert_eq!(report.total, 3);
        assert_eq!(report.accepts, 1);
        assert_eq!(report.rejects, 1);
        assert_eq!(report.edits, 1);
        assert_eq!(report.skipped, 0);
        // 0.82 → bucket 8 accept; 0.30 → bucket 3 reject; 0.91 → bucket 9 edit.
        assert_eq!(report.deciles[8].accepts, 1);
        assert_eq!(report.deciles[3].rejects, 1);
        assert_eq!(report.deciles[9].edits, 1);
    }

    #[test]
    fn accept_and_reject_records_omit_edit_distance() {
        let temp = tempfile::tempdir().expect("tempdir");
        let repo = temp.path();
        append_decision(
            repo,
            "dev_a",
            DurabilityTier::BestEffort,
            &DecisionRecord {
                candidate_id: "mem_a".to_string(),
                scope: "me".to_string(),
                author_kind: AuthorKind::Dreaming,
                self_reported_confidence: 0.5,
                decision: Decision::Accept,
                // A stray ratio supplied on an accept must be dropped, per spec.
                edit_distance_ratio: Some(0.99),
                decided_at: Utc::now(),
                session_id: None,
            },
        )
        .expect("append");
        let path = repo.join(calibration_log_path("dev_a"));
        let contents = fs::read_to_string(path).expect("read");
        assert!(!contents.contains("edit_distance_ratio"), "accept record must not carry an edit distance");
        assert!(!contents.contains("session_id"), "absent session id must be omitted");
    }

    #[test]
    fn build_report_skips_undecodable_lines() {
        let temp = tempfile::tempdir().expect("tempdir");
        let repo = temp.path();
        let dir = repo.join(CALIBRATION_DIR);
        fs::create_dir_all(&dir).expect("mkdir");
        let good = serde_json::to_string(&record(0.5, Decision::Accept)).expect("serialize");
        let body = format!("{good}\n{{not json}}\n\n{good}\n");
        fs::write(dir.join("dev_x.jsonl"), body).expect("write");
        let report = build_report(repo).expect("build");
        assert_eq!(report.total, 2);
        assert_eq!(report.skipped, 1);
    }

    #[test]
    fn build_report_merges_multiple_device_files() {
        let temp = tempfile::tempdir().expect("tempdir");
        let repo = temp.path();
        append_decision(
            repo,
            "dev_one",
            DurabilityTier::BestEffort,
            &DecisionRecord {
                candidate_id: "mem_1".to_string(),
                scope: "me".to_string(),
                author_kind: AuthorKind::Dreaming,
                self_reported_confidence: 0.5,
                decision: Decision::Accept,
                edit_distance_ratio: None,
                decided_at: Utc::now(),
                session_id: None,
            },
        )
        .expect("append one");
        append_decision(
            repo,
            "dev_two",
            DurabilityTier::BestEffort,
            &DecisionRecord {
                candidate_id: "mem_2".to_string(),
                scope: "me".to_string(),
                author_kind: AuthorKind::Dreaming,
                self_reported_confidence: 0.5,
                decision: Decision::Reject,
                edit_distance_ratio: None,
                decided_at: Utc::now(),
                session_id: None,
            },
        )
        .expect("append two");
        let report = build_report(repo).expect("build");
        assert_eq!(report.total, 2);
        assert_eq!(report.accepts, 1);
        assert_eq!(report.rejects, 1);
    }

    #[test]
    fn empty_dir_yields_empty_report() {
        let temp = tempfile::tempdir().expect("tempdir");
        let report = build_report(temp.path()).expect("build");
        assert_eq!(report.total, 0);
        assert_eq!(report.accept_rate, None);
        assert_eq!(report.deciles.len(), DECILE_COUNT);
    }

    #[test]
    fn scope_string_matches_dream_scope_convention() {
        assert_eq!(scope_string(Scope::User, None), "me");
        assert_eq!(scope_string(Scope::Agent, Some("ignored")), "agent");
        assert_eq!(scope_string(Scope::Project, Some("proj_a3f2")), "project:proj_a3f2");
        assert_eq!(scope_string(Scope::Org, Some("org_x")), "org:org_x");
        assert_eq!(scope_string(Scope::Project, None), "project", "missing id degrades to bare kind");
    }

    #[test]
    fn human_report_renders_all_deciles() {
        let report = report_from_records(vec![record(0.85, Decision::Accept), record(0.85, Decision::Reject)]);
        let rendered = render_human_report(&report);
        assert!(rendered.contains("total=2"));
        assert!(rendered.contains("accept_rate=0.50"));
        // ten decile rows plus header lines.
        assert_eq!(rendered.matches('[').count(), DECILE_COUNT);
    }
}
