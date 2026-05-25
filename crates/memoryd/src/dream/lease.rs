use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::future::Future;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::Duration as StdDuration;

use chrono::{DateTime, Duration, NaiveDate, Utc};
use memory_substrate::config::load_local_device_config;
use memory_substrate::git::LeaseCommitAction;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::dream::git::{LeaseGit, NativeLeaseGit};
use crate::dream::scope::DreamScope;
use crate::protocol::{CandidateWriteResult, DreamRunReport, LeaseRecord, PassOutcome, PassStatus};

const MAX_PUSH_RETRIES: usize = 3;

#[derive(Debug, Clone)]
pub struct LeaseAcquireRequest {
    pub repo: PathBuf,
    pub runtime: PathBuf,
    pub scope: String,
    pub force: bool,
    pub now: DateTime<Utc>,
    pub lease_window_seconds: u64,
    pub cli_used: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct LeaseCommit<'a> {
    pub action: LeaseCommitAction,
    pub scope: &'a str,
    pub device_id: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaseAcquired {
    pub record: LeaseRecord,
    pub report: DreamRunReport,
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum LeaseError {
    #[error("lease_held: active lease for {scope} is held by {by_device}")]
    Held { scope: String, by_device: String },
    #[error("lease_unavailable: {message}")]
    Unavailable { message: String },
    #[error("lease_dirty_tree: {message}")]
    DirtyTree { message: String },
    #[error("invalid_request: {message}")]
    InvalidRequest { message: String },
}

impl LeaseError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Held { .. } => "lease_held",
            Self::Unavailable { .. } => "lease_unavailable",
            Self::DirtyTree { .. } => "lease_dirty_tree",
            Self::InvalidRequest { .. } => "invalid_request",
        }
    }

    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::Unavailable { message: message.into() }
    }

    pub fn cli_exit_code(&self) -> i32 {
        match self {
            Self::Held { .. } | Self::Unavailable { .. } | Self::DirtyTree { .. } => 5,
            Self::InvalidRequest { .. } => 1,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScheduledLeaseRequest {
    pub acquire: LeaseAcquireRequest,
    pub retry_window_minutes: u16,
    pub run_date: NaiveDate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduledLeaseReport {
    pub attempts: u32,
    pub outcome: ScheduledLeaseOutcome,
    pub consecutive_missed_runs: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduledLeaseOutcome {
    Success,
    Missed,
    Held,
}

pub trait LeaseSleeper {
    fn sleep_minutes(&self, minutes: u16);
}

#[derive(Debug, Clone, Copy)]
pub struct RealLeaseSleeper;

impl LeaseSleeper for RealLeaseSleeper {
    fn sleep_minutes(&self, minutes: u16) {
        std::thread::sleep(StdDuration::from_secs(u64::from(minutes) * 60));
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ImmediateLeaseSleeper;

impl LeaseSleeper for ImmediateLeaseSleeper {
    fn sleep_minutes(&self, _minutes: u16) {}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CleanupSummary {
    device: String,
    date: String,
    scope_summaries: Vec<CleanupScopeSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CleanupScopeSummary {
    scope: String,
    outcome: String,
    attempts: u32,
    consecutive_missed_runs: u32,
    error_code: Option<String>,
}

struct CleanupWrite<'a> {
    request: &'a LeaseAcquireRequest,
    date: NaiveDate,
    device_id: &'a str,
    report: &'a ScheduledLeaseReport,
    error_code: Option<&'a str>,
}

pub fn acquire_manual_lease(request: LeaseAcquireRequest) -> Result<LeaseAcquired, LeaseError> {
    acquire_manual_lease_with_git(&mut NativeLeaseGit, request)
}

pub fn release_manual_lease(request: LeaseAcquireRequest) -> Result<(), LeaseError> {
    release_manual_lease_with_git(&mut NativeLeaseGit, request)
}

pub fn acquire_manual_lease_with_git(
    git: &mut impl LeaseGit,
    request: LeaseAcquireRequest,
) -> Result<LeaseAcquired, LeaseError> {
    validate_scope(&request.scope)?;
    let device_id = load_device_id(&request.runtime)?;
    let lease_path = request.repo.join("leases/journal.lease");

    for attempt in 0..=MAX_PUSH_RETRIES {
        git.fetch_origin(&request.repo)?;
        if !request.force {
            if let Some(active) = active_lease(&lease_path, &request.scope, request.now)? {
                if active.device == device_id {
                    return Ok(LeaseAcquired { record: active, report: stub_report(&request) });
                }
                return Err(LeaseError::Held { scope: request.scope, by_device: active.device });
            }
        }
        if git.has_dirty_user_work(&request.repo)? {
            return Err(LeaseError::DirtyTree {
                message: "working tree has uncommitted user changes outside leases/journal.lease".to_string(),
            });
        }

        let record = lease_record(&request, &device_id);
        append_lease_record(&lease_path, &record)?;
        git.commit_lease(
            &request.repo,
            &LeaseCommit { action: LeaseCommitAction::Acquire, scope: &request.scope, device_id: &device_id },
        )?;
        match git.push(&request.repo) {
            Ok(()) => return Ok(LeaseAcquired { report: stub_report(&request), record }),
            Err(err) => {
                git.rollback_failed_lease_attempt(&request.repo)?;
                if matches!(err, LeaseError::Unavailable { .. }) && attempt < MAX_PUSH_RETRIES {
                    continue;
                }
                return Err(err);
            }
        }
    }

    Err(LeaseError::unavailable("push race exhausted retry budget"))
}

pub fn release_manual_lease_with_git(git: &mut impl LeaseGit, request: LeaseAcquireRequest) -> Result<(), LeaseError> {
    validate_scope(&request.scope)?;
    let device_id = load_device_id(&request.runtime)?;
    let lease_path = request.repo.join("leases/journal.lease");

    git.fetch_origin(&request.repo)?;
    if git.has_dirty_user_work(&request.repo)? {
        return Err(LeaseError::DirtyTree {
            message: "working tree has uncommitted user changes outside leases/journal.lease".to_string(),
        });
    }

    let record = release_record(&request, &device_id);
    append_lease_record(&lease_path, &record)?;
    git.commit_lease(
        &request.repo,
        &LeaseCommit { action: LeaseCommitAction::Release, scope: &request.scope, device_id: &device_id },
    )?;
    match git.push(&request.repo) {
        Ok(()) => Ok(()),
        Err(err) => {
            git.rollback_failed_lease_attempt(&request.repo)?;
            Err(err)
        }
    }
}

pub fn run_scheduled_lease(
    git: &mut impl LeaseGit,
    request: ScheduledLeaseRequest,
) -> Result<ScheduledLeaseReport, LeaseError> {
    run_scheduled_lease_with_runner_and_sleeper(git, request, &RealLeaseSleeper, |lease| Ok(lease.report.clone()))
}

pub fn run_scheduled_lease_with_runner(
    git: &mut impl LeaseGit,
    request: ScheduledLeaseRequest,
    mut run_dream: impl FnMut(&LeaseAcquired) -> Result<DreamRunReport, LeaseError>,
) -> Result<ScheduledLeaseReport, LeaseError> {
    run_scheduled_lease_with_runner_and_sleeper(git, request, &RealLeaseSleeper, |lease| run_dream(lease))
}

pub fn run_scheduled_lease_with_runner_and_sleeper(
    git: &mut impl LeaseGit,
    request: ScheduledLeaseRequest,
    sleeper: &impl LeaseSleeper,
    mut run_dream: impl FnMut(&LeaseAcquired) -> Result<DreamRunReport, LeaseError>,
) -> Result<ScheduledLeaseReport, LeaseError> {
    let device_id = load_device_id(&request.acquire.runtime)?;
    let retry_offsets = retry_offsets(request.retry_window_minutes);
    let mut attempts = 0;
    let mut last_error = None;
    let mut previous_offset = 0;

    for offset in retry_offsets {
        if attempts > 0 {
            sleeper.sleep_minutes(offset.saturating_sub(previous_offset));
        }
        previous_offset = offset;
        attempts += 1;
        match acquire_manual_lease_with_git(git, request.acquire.clone()) {
            Ok(lease) => {
                if let Err(err) = run_dream(&lease) {
                    if let Err(_release_err) = release_manual_lease_with_git(git, request.acquire.clone()) {
                        // Preserve the original dream failure for operator diagnostics. A release-side
                        // failure is secondary; the lease will still expire under the normal lease window.
                    }
                    return Err(err);
                }
                let report = ScheduledLeaseReport {
                    attempts,
                    outcome: ScheduledLeaseOutcome::Success,
                    consecutive_missed_runs: 0,
                };
                write_cleanup_summary(CleanupWrite {
                    request: &request.acquire,
                    date: request.run_date,
                    device_id: &device_id,
                    report: &report,
                    error_code: None,
                })?;
                return Ok(report);
            }
            Err(LeaseError::Unavailable { message }) => {
                last_error = Some(format!("lease_unavailable: {message}"));
            }
            Err(LeaseError::Held { .. }) => {
                let report =
                    ScheduledLeaseReport { attempts, outcome: ScheduledLeaseOutcome::Held, consecutive_missed_runs: 0 };
                write_cleanup_summary(CleanupWrite {
                    request: &request.acquire,
                    date: request.run_date,
                    device_id: &device_id,
                    report: &report,
                    error_code: Some("lease_held"),
                })?;
                return Ok(report);
            }
            Err(err) => return Err(err),
        }
    }

    let consecutive_missed_runs =
        previous_missed_runs(&request.acquire.repo, &device_id, &request.acquire.scope).saturating_add(1);
    let report = ScheduledLeaseReport { attempts, outcome: ScheduledLeaseOutcome::Missed, consecutive_missed_runs };
    write_cleanup_summary(CleanupWrite {
        request: &request.acquire,
        date: request.run_date,
        device_id: &device_id,
        report: &report,
        error_code: Some(last_error.as_deref().unwrap_or("lease_unavailable")),
    })?;
    Ok(report)
}

pub async fn run_scheduled_lease_with_async_runner_and_sleeper<G, S, F, Fut>(
    git: &mut G,
    request: ScheduledLeaseRequest,
    sleeper: &S,
    mut run_dream: F,
) -> Result<ScheduledLeaseReport, LeaseError>
where
    G: LeaseGit,
    S: LeaseSleeper,
    F: FnMut(LeaseAcquired) -> Fut,
    Fut: Future<Output = Result<DreamRunReport, LeaseError>>,
{
    let device_id = load_device_id(&request.acquire.runtime)?;
    let retry_offsets = retry_offsets(request.retry_window_minutes);
    let mut attempts = 0;
    let mut last_error = None;
    let mut previous_offset = 0;

    for offset in retry_offsets {
        if attempts > 0 {
            sleeper.sleep_minutes(offset.saturating_sub(previous_offset));
        }
        previous_offset = offset;
        attempts += 1;
        match acquire_manual_lease_with_git(git, request.acquire.clone()) {
            Ok(lease) => {
                if let Err(err) = run_dream(lease).await {
                    if let Err(_release_err) = release_manual_lease_with_git(git, request.acquire.clone()) {
                        // Preserve the original dream failure for operator diagnostics. A release-side
                        // failure is secondary; the lease will still expire under the normal lease window.
                    }
                    return Err(err);
                }
                let report = ScheduledLeaseReport {
                    attempts,
                    outcome: ScheduledLeaseOutcome::Success,
                    consecutive_missed_runs: 0,
                };
                write_cleanup_summary(CleanupWrite {
                    request: &request.acquire,
                    date: request.run_date,
                    device_id: &device_id,
                    report: &report,
                    error_code: None,
                })?;
                return Ok(report);
            }
            Err(LeaseError::Unavailable { message }) => {
                last_error = Some(format!("lease_unavailable: {message}"));
            }
            Err(LeaseError::Held { .. }) => {
                let report =
                    ScheduledLeaseReport { attempts, outcome: ScheduledLeaseOutcome::Held, consecutive_missed_runs: 0 };
                write_cleanup_summary(CleanupWrite {
                    request: &request.acquire,
                    date: request.run_date,
                    device_id: &device_id,
                    report: &report,
                    error_code: Some("lease_held"),
                })?;
                return Ok(report);
            }
            Err(err) => return Err(err),
        }
    }

    let consecutive_missed_runs =
        previous_missed_runs(&request.acquire.repo, &device_id, &request.acquire.scope).saturating_add(1);
    let report = ScheduledLeaseReport { attempts, outcome: ScheduledLeaseOutcome::Missed, consecutive_missed_runs };
    write_cleanup_summary(CleanupWrite {
        request: &request.acquire,
        date: request.run_date,
        device_id: &device_id,
        report: &report,
        error_code: Some(last_error.as_deref().unwrap_or("lease_unavailable")),
    })?;
    Ok(report)
}

fn retry_offsets(window_minutes: u16) -> Vec<u16> {
    if window_minutes == 0 {
        return vec![0];
    }

    let mut offsets = vec![0];
    let mut elapsed = 0;
    let mut delay = 1;
    while elapsed + delay <= window_minutes {
        elapsed += delay;
        offsets.push(elapsed);
        delay = (delay * 2).min(32);
    }
    offsets
}

fn active_lease(lease_path: &Path, scope: &str, now: DateTime<Utc>) -> Result<Option<LeaseRecord>, LeaseError> {
    let records = read_lease_records(lease_path)?;
    Ok(records.into_iter().rev().find(|record| record.scope == scope).filter(|record| record.expires_at > now))
}

fn lease_record(request: &LeaseAcquireRequest, device_id: &str) -> LeaseRecord {
    LeaseRecord {
        device: device_id.to_string(),
        scope: request.scope.clone(),
        acquired_at: request.now,
        expires_at: request.now + Duration::seconds(request.lease_window_seconds as i64),
        run_id: lease_record_id("run", request, device_id),
    }
}

fn release_record(request: &LeaseAcquireRequest, device_id: &str) -> LeaseRecord {
    LeaseRecord {
        device: device_id.to_string(),
        scope: request.scope.clone(),
        acquired_at: request.now,
        expires_at: request.now,
        run_id: lease_record_id("release", request, device_id),
    }
}

fn lease_record_id(prefix: &str, request: &LeaseAcquireRequest, device_id: &str) -> String {
    let timestamp = request
        .now
        .timestamp_nanos_opt()
        .map(|nanos| nanos.unsigned_abs().to_string())
        .unwrap_or_else(|| request.now.format("%Y%m%dT%H%M%S%9fZ").to_string());
    format!("{prefix}_{device_id}_{timestamp}")
}

fn append_lease_record(path: &Path, record: &LeaseRecord) -> Result<(), LeaseError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| LeaseError::unavailable(err.to_string()))?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|err| LeaseError::unavailable(err.to_string()))?;
    writeln!(file, "{}", serde_json::to_string(record).map_err(|err| LeaseError::unavailable(err.to_string()))?)
        .map_err(|err| LeaseError::unavailable(err.to_string()))
}

fn read_lease_records(path: &Path) -> Result<Vec<LeaseRecord>, LeaseError> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = fs::File::open(path).map_err(|err| LeaseError::unavailable(err.to_string()))?;
    let reader = BufReader::new(file);
    let mut records = Vec::new();
    for line in reader.lines() {
        let line = line.map_err(|err| LeaseError::unavailable(err.to_string()))?;
        if line.trim().is_empty() {
            continue;
        }
        records.push(serde_json::from_str(&line).map_err(|err| LeaseError::unavailable(err.to_string()))?);
    }
    Ok(records)
}

fn load_device_id(runtime: &Path) -> Result<String, LeaseError> {
    let config = load_local_device_config(runtime)
        .map_err(LeaseError::unavailable)?
        .ok_or_else(|| LeaseError::InvalidRequest { message: "local-device.yaml is missing".to_string() })?;
    Ok(config.device.id)
}

fn validate_scope(scope: &str) -> Result<(), LeaseError> {
    DreamScope::parse(scope).map(|_| ()).map_err(|err| LeaseError::InvalidRequest { message: err.to_string() })
}

fn stub_report(request: &LeaseAcquireRequest) -> DreamRunReport {
    DreamRunReport {
        scope: request.scope.clone(),
        cli_used: request.cli_used.clone(),
        pass_1: skipped_pass(),
        pass_2: skipped_pass(),
        pass_2_refusal_counts_by_reason: std::collections::BTreeMap::new(),
        pass_3: skipped_pass(),
        duration_ms: 0,
    }
}

fn skipped_pass() -> PassOutcome {
    PassOutcome {
        status: PassStatus::Skipped,
        output_path: None,
        candidate_results: Vec::<CandidateWriteResult>::new(),
        error_code: None,
        duration_ms: 0,
    }
}

fn write_cleanup_summary(write: CleanupWrite<'_>) -> Result<(), LeaseError> {
    let dir = write.request.repo.join("dreams/cleanup").join(write.device_id);
    fs::create_dir_all(&dir).map_err(|err| LeaseError::unavailable(err.to_string()))?;
    let path = dir.join(format!("{}.json", write.date.format("%Y-%m-%d")));
    let summary = CleanupSummary {
        device: write.device_id.to_string(),
        date: write.date.format("%Y-%m-%d").to_string(),
        scope_summaries: vec![CleanupScopeSummary {
            scope: write.request.scope.clone(),
            outcome: match write.report.outcome {
                ScheduledLeaseOutcome::Success => "success",
                ScheduledLeaseOutcome::Missed => "missed",
                ScheduledLeaseOutcome::Held => "held",
            }
            .to_string(),
            attempts: write.report.attempts,
            consecutive_missed_runs: write.report.consecutive_missed_runs,
            error_code: write.error_code.map(str::to_string),
        }],
    };
    let json = serde_json::to_string(&summary).map_err(|err| LeaseError::unavailable(err.to_string()))?;
    fs::write(path, json).map_err(|err| LeaseError::unavailable(err.to_string()))
}

fn previous_missed_runs(repo: &Path, device_id: &str, scope: &str) -> u32 {
    let dir = repo.join("dreams/cleanup").join(device_id);
    let Ok(entries) = fs::read_dir(dir) else { return 0 };
    let mut summaries = BTreeMap::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else { continue };
        let Ok(summary) = serde_json::from_str::<CleanupSummary>(&text) else { continue };
        summaries.insert(summary.date.clone(), summary);
    }
    summaries
        .values()
        .rev()
        .find_map(|summary| summary.scope_summaries.iter().find(|item| item.scope == scope))
        .map(|summary| summary.consecutive_missed_runs)
        .unwrap_or(0)
}
