//! Device-local scheduler for importing harness auto-memory through memoryd.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Utc};
use memory_substrate::config::{load_local_device_config, HarvestConfig};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use crate::import::pipeline::{
    run_import_session_with_lock_timeout, ExecuteOptions, ImportOptions, SocketDaemonClient,
};
use crate::import::project_map::{FixedDispositionBackend, PromptedDisposition};
use crate::import::report::{HarnessCounters, ImportReport};
use crate::import::ImportError;
use crate::protocol::HarvestHarnessCounts;

const DISABLED_RECHECK: Duration = Duration::from_secs(5 * 60);
const FIRST_WAKE: Duration = Duration::from_secs(1);
const MIN_SLEEP: Duration = Duration::from_secs(1);
const IMPORT_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const MAX_ERROR_BYTES: usize = 500;
const STATE_FILE_NAME: &str = "harvest-state.json";
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct HarvestState {
    pub schema_version: u32,
    pub last_attempt_at: DateTime<Utc>,
    pub last_success_at: Option<DateTime<Utc>>,
    pub next_due: Option<DateTime<Utc>>,
    pub harnesses: BTreeMap<String, HarvestHarnessCounts>,
    pub last_error: Option<String>,
    pub active_embedding_lane: Option<String>,
}

enum TickOutcome {
    Completed(Box<Result<crate::import::pipeline::ExecuteResult, ImportError>>),
    Contended,
    TimedOut,
    Shutdown,
}

pub(crate) fn spawn_harvest_scheduler(
    runtime_root: PathBuf,
    repo_root: PathBuf,
    socket_path: PathBuf,
    mut shutdown: watch::Receiver<bool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut state = None;
        let mut state_loaded = false;
        let mut sleep_for = FIRST_WAKE;

        loop {
            tokio::select! {
                _ = shutdown.changed() => return,
                _ = tokio::time::sleep(sleep_for) => {}
            }

            let config = match load_harvest_config(&runtime_root) {
                Ok(config) => config,
                Err(error) => {
                    tracing::warn!(%error, "harvest config unreadable; treating harvest as disabled for this tick");
                    sleep_for = DISABLED_RECHECK;
                    continue;
                }
            };
            if !config.enabled {
                sleep_for = DISABLED_RECHECK;
                continue;
            }
            if !state_loaded {
                state = match read_harvest_state(&runtime_root) {
                    Ok(Some(state)) => Some(state),
                    Ok(None) => {
                        tracing::warn!(path = %state_path(&runtime_root).display(), "harvest state missing; treating as never run");
                        None
                    }
                    Err(error) => {
                        tracing::warn!(%error, "harvest state unreadable; treating as never run");
                        None
                    }
                };
                state_loaded = true;
            }

            let now = Utc::now();
            if let Some(delay) = delay_until_due(state.as_ref(), config, now) {
                sleep_for = delay;
                continue;
            }

            let attempted_at = Utc::now();
            match run_tick(&repo_root, &socket_path, &mut shutdown).await {
                TickOutcome::Shutdown => return,
                TickOutcome::Contended => {
                    tracing::debug!("harvest skipped because another import owns the lock");
                    sleep_for = MIN_SLEEP;
                }
                TickOutcome::TimedOut => {
                    tracing::warn!("harvest import timed out after 600 seconds");
                    let next = state_after_error(
                        state.as_ref(),
                        config,
                        attempted_at,
                        "scheduled import timed out after 600 seconds",
                        active_embedding_lane(&repo_root),
                    );
                    persist_state(&runtime_root, &next);
                    state = Some(next);
                    // Paced from completion, not attempt start: a tick whose own
                    // runtime exceeds a short interval must not retry at the
                    // sleep floor (re-review residual).
                    sleep_for = duration_until(next_due_after(Utc::now(), config), Utc::now());
                }
                TickOutcome::Completed(result) => match *result {
                    Err(error) => {
                        tracing::warn!(%error, "harvest import failed");
                        let next = state_after_error(
                            state.as_ref(),
                            config,
                            attempted_at,
                            &error.to_string(),
                            active_embedding_lane(&repo_root),
                        );
                        persist_state(&runtime_root, &next);
                        state = Some(next);
                        // Same completion-based pacing as the timeout arm.
                        sleep_for = duration_until(next_due_after(Utc::now(), config), Utc::now());
                    }
                    Ok(result) => {
                        let next = state_after_report(config, attempted_at, &result.report, &repo_root);
                        let written: usize = next.harnesses.values().map(|counts| counts.written).sum();
                        persist_state(&runtime_root, &next);
                        state = Some(next);
                        if written > 0 {
                            tracing::info!(written, "harvest import completed");
                        } else {
                            tracing::debug!("harvest import completed without new writes");
                        }
                        sleep_for = duration_until(state.as_ref().and_then(|value| value.next_due), Utc::now());
                    }
                },
            }
        }
    })
}

fn load_harvest_config(runtime_root: &Path) -> Result<HarvestConfig, String> {
    Ok(load_local_device_config(runtime_root)?.and_then(|config| config.harvest).unwrap_or_default())
}

/// Pacing runs off `last_attempt_at`, not `last_success_at`: a failed or
/// timed-out attempt must wait a full interval like a successful one, or a
/// persistent failure with no prior success would retry at the loop's 1s floor
/// forever (review M1). Successful attempts set both timestamps equal, so
/// success pacing is unchanged.
fn delay_until_due(state: Option<&HarvestState>, config: HarvestConfig, now: DateTime<Utc>) -> Option<Duration> {
    let due = state.map(|value| value.last_attempt_at + chrono::Duration::minutes(i64::from(config.interval_minutes)));
    due.filter(|due_at| *due_at > now).map(|due_at| duration_until(Some(due_at), now))
}

fn duration_until(due: Option<DateTime<Utc>>, now: DateTime<Utc>) -> Duration {
    let millis = due.map(|value| (value - now).num_milliseconds()).unwrap_or(0).max(1000) as u64;
    Duration::from_millis(millis)
}

async fn run_tick(repo_root: &Path, socket_path: &Path, shutdown: &mut watch::Receiver<bool>) -> TickOutcome {
    let mut prompts = FixedDispositionBackend::new(PromptedDisposition::DeriveProject);
    let mut client = SocketDaemonClient::new(socket_path.to_path_buf());
    let import = run_import_session_with_lock_timeout(
        repo_root,
        ImportOptions { quiet: true, ..ImportOptions::default() },
        &mut prompts,
        &mut client,
        ExecuteOptions { dry_run: false, verbose_progress: false },
        Some(Duration::ZERO),
    );

    tokio::select! {
        _ = shutdown.changed() => TickOutcome::Shutdown,
        result = tokio::time::timeout(IMPORT_TIMEOUT, import) => match result {
            Err(_) => TickOutcome::TimedOut,
            Ok(Err(ImportError::AnotherImportInProgress { .. })) => TickOutcome::Contended,
            Ok(result) => TickOutcome::Completed(Box::new(result)),
        }
    }
}

/// The next wake the scheduler will actually honor: one interval after the
/// attempt that just finished, successful or not. Note the interval is measured
/// from tick *start*; a tick can run up to 600s, which is negligible against
/// the 5-minute interval floor at the documented corpus scale.
fn next_due_after(attempted_at: DateTime<Utc>, config: HarvestConfig) -> Option<DateTime<Utc>> {
    Some(attempted_at + chrono::Duration::minutes(i64::from(config.interval_minutes)))
}

fn state_after_report(
    config: HarvestConfig,
    attempted_at: DateTime<Utc>,
    report: &ImportReport,
    repo_root: &Path,
) -> HarvestState {
    HarvestState {
        schema_version: 1,
        last_attempt_at: attempted_at,
        last_success_at: Some(attempted_at),
        next_due: next_due_after(attempted_at, config),
        harnesses: report
            .harnesses
            .iter()
            .map(|(harness, counters)| (harness.clone(), harvest_counts(counters)))
            .collect(),
        last_error: report_error(report),
        active_embedding_lane: active_embedding_lane(repo_root),
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "flat inputs of one state transition; a params struct would add ceremony"
)]
fn state_after_error(
    previous: Option<&HarvestState>,
    config: HarvestConfig,
    attempted_at: DateTime<Utc>,
    error: &str,
    active_embedding_lane: Option<String>,
) -> HarvestState {
    HarvestState {
        schema_version: 1,
        last_attempt_at: attempted_at,
        last_success_at: previous.and_then(|state| state.last_success_at),
        next_due: next_due_after(attempted_at, config),
        harnesses: previous.map(|state| state.harnesses.clone()).unwrap_or_else(|| {
            ["claude-code", "codex"]
                .into_iter()
                .map(|harness| (harness.to_string(), HarvestHarnessCounts::default()))
                .collect()
        }),
        last_error: Some(bound_error(error)),
        active_embedding_lane,
    }
}

fn harvest_counts(counters: &HarnessCounters) -> HarvestHarnessCounts {
    HarvestHarnessCounts {
        parsed: counters.parsed,
        written: counters.written_new + counters.superseded + counters.written_candidate,
        refused: counters.refused_privacy
            + counters.refused_contradiction
            + counters.refused_tombstone
            + counters.refused_grounding
            + counters.refused_policy
            + counters.refused_other,
        quarantined: counters.quarantined,
        skipped: counters.dedup_existing
            + counters.skipped_idempotent
            + counters.skipped_by_prompt
            + counters.ambiguous,
    }
}

fn report_error(report: &ImportReport) -> Option<String> {
    if report.parse_errors.is_empty() {
        return None;
    }
    Some(bound_error(
        &report
            .parse_errors
            .iter()
            .map(|error| format!("{}: {}", error.source_key, error.message))
            .collect::<Vec<_>>()
            .join("; "),
    ))
}

fn active_embedding_lane(repo_root: &Path) -> Option<String> {
    memory_substrate::config::load_active_embedding(repo_root).ok().map(|triple| triple.provider)
}

fn bound_error(error: &str) -> String {
    if error.len() <= MAX_ERROR_BYTES {
        return error.to_string();
    }
    let mut end = MAX_ERROR_BYTES;
    while !error.is_char_boundary(end) {
        end -= 1;
    }
    error[..end].to_string()
}

fn persist_state(runtime_root: &Path, state: &HarvestState) {
    if let Err(error) = write_harvest_state(runtime_root, state) {
        tracing::warn!(%error, "failed to persist harvest state");
    }
}

pub(crate) fn read_harvest_state(runtime_root: &Path) -> Result<Option<HarvestState>, String> {
    let path = state_path(runtime_root);
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("read {}: {error}", path.display())),
    };
    let state: HarvestState =
        serde_json::from_str(&raw).map_err(|error| format!("parse {}: {error}", path.display()))?;
    if state.schema_version != 1 {
        return Err(format!("parse {}: unsupported schema_version {}", path.display(), state.schema_version));
    }
    Ok(Some(state))
}

fn write_harvest_state(runtime_root: &Path, state: &HarvestState) -> Result<(), String> {
    std::fs::create_dir_all(runtime_root).map_err(|error| format!("create {}: {error}", runtime_root.display()))?;
    let path = state_path(runtime_root);
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    let temp = runtime_root.join(format!(".{STATE_FILE_NAME}.{}.{nonce}.{sequence}.tmp", std::process::id()));
    let body = serde_json::to_vec_pretty(state).map_err(|error| error.to_string())?;
    std::fs::write(&temp, body).map_err(|error| format!("write {}: {error}", temp.display()))?;
    std::fs::rename(&temp, &path).map_err(|error| format!("rename {} -> {}: {error}", temp.display(), path.display()))
}

fn state_path(runtime_root: &Path) -> PathBuf {
    runtime_root.join(STATE_FILE_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 19, 12, minute, 0).single().expect("time")
    }

    fn state(last_success_at: Option<DateTime<Utc>>) -> HarvestState {
        HarvestState {
            schema_version: 1,
            last_attempt_at: at(0),
            last_success_at,
            next_due: None,
            harnesses: BTreeMap::new(),
            last_error: None,
            active_embedding_lane: None,
        }
    }

    #[test]
    fn due_computation_handles_never_recent_and_overdue_runs() {
        let config = HarvestConfig { enabled: true, interval_minutes: 30 };
        assert_eq!(delay_until_due(None, config, at(10)), None);
        assert_eq!(delay_until_due(Some(&state(Some(at(0)))), config, at(10)), Some(Duration::from_secs(20 * 60)));
        assert_eq!(delay_until_due(Some(&state(Some(at(0)))), config, at(30)), None);
    }

    #[test]
    fn failed_attempt_paces_a_full_interval_instead_of_hot_retrying() {
        // Review M1: last_success_at = None with a recent attempt must NOT be
        // immediately due, or a persistent failure retries at the loop floor.
        let config = HarvestConfig { enabled: true, interval_minutes: 30 };
        assert_eq!(delay_until_due(Some(&state(None)), config, at(10)), Some(Duration::from_secs(20 * 60)));
        assert_eq!(delay_until_due(Some(&state(None)), config, at(30)), None);
    }

    #[test]
    fn error_state_preserves_prior_counts_and_next_due_matches_pacing() {
        let config = HarvestConfig { enabled: true, interval_minutes: 30 };
        let mut previous = state(Some(at(0)));
        previous.harnesses.insert(
            "claude-code".to_string(),
            HarvestHarnessCounts { parsed: 7, written: 2, refused: 0, quarantined: 1, skipped: 4 },
        );
        let next = state_after_error(Some(&previous), config, at(40), "boom", Some("gemini-api".to_string()));
        assert_eq!(next.last_success_at, Some(at(0)));
        assert_eq!(next.next_due, Some(at(40) + chrono::Duration::minutes(30)));
        assert_eq!(next.harnesses.get("claude-code").map(|counts| counts.parsed), Some(7));
        assert_eq!(next.active_embedding_lane.as_deref(), Some("gemini-api"));
    }

    #[test]
    fn config_load_observes_disable_and_interval_edits() {
        let temp = tempfile::tempdir().expect("temp");
        std::fs::write(
            temp.path().join("local-device.yaml"),
            "schema_version: 1\ndevice:\n  id: dev_harvest\n  name: test\n  shard: test\npaths: {}\nprivacy: {}\nharvest:\n  enabled: true\n  interval_minutes: 30\n",
        )
        .expect("config");
        assert_eq!(
            load_harvest_config(temp.path()).expect("first load"),
            HarvestConfig { enabled: true, interval_minutes: 30 }
        );

        memory_substrate::config::store_harvest_config(
            temp.path(),
            HarvestConfig { enabled: false, interval_minutes: 90 },
        )
        .expect("edit config");
        assert_eq!(
            load_harvest_config(temp.path()).expect("second load"),
            HarvestConfig { enabled: false, interval_minutes: 90 }
        );
    }

    #[test]
    fn bounded_error_respects_utf8_and_byte_limit() {
        let bounded = bound_error(&"é".repeat(300));
        assert!(bounded.len() <= 500);
        assert!(bounded.is_char_boundary(bounded.len()));
    }

    #[test]
    fn state_write_is_atomic_and_round_trips_without_temp_residue() {
        let temp = tempfile::tempdir().expect("temp");
        let expected = state(Some(at(0)));
        write_harvest_state(temp.path(), &expected).expect("write");
        assert_eq!(read_harvest_state(temp.path()).expect("read"), Some(expected));
        assert_eq!(std::fs::read_dir(temp.path()).expect("list").count(), 1);
    }

    #[test]
    fn report_counts_collapse_existing_import_counters() {
        let counts = harvest_counts(&HarnessCounters {
            parsed: 10,
            written_new: 1,
            superseded: 2,
            written_candidate: 3,
            quarantined: 1,
            refused_privacy: 1,
            refused_other: 2,
            dedup_existing: 1,
            skipped_idempotent: 2,
            skipped_by_prompt: 3,
            ambiguous: 4,
            ..HarnessCounters::default()
        });
        assert_eq!(counts, HarvestHarnessCounts { parsed: 10, written: 6, refused: 3, quarantined: 1, skipped: 10 });
    }

    #[tokio::test]
    async fn lock_contention_skips_without_touching_harvest_state() {
        let temp = tempfile::tempdir().expect("temp");
        let repo = temp.path().join("repo");
        let runtime = temp.path().join("runtime");
        std::fs::create_dir_all(&repo).expect("repo");
        let import_state = repo.join(".memorum/import-state.json");
        let _lock = crate::import::state::ImportLockGuard::acquire(&import_state).expect("lock");
        let (_shutdown_tx, mut shutdown_rx) = watch::channel(false);

        let outcome = run_tick(&repo, &runtime.join("missing.sock"), &mut shutdown_rx).await;

        assert!(matches!(outcome, TickOutcome::Contended));
        assert!(!runtime.join(STATE_FILE_NAME).exists());
    }
}
