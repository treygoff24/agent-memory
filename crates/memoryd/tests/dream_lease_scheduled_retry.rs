use chrono::{Duration, TimeZone, Utc};
use memoryd::dream::git::ScriptedLeaseGit;
use memoryd::dream::lease::{
    acquire_manual_lease_with_git, run_scheduled_lease_with_runner_and_sleeper, ImmediateLeaseSleeper,
    LeaseAcquireRequest, LeaseError, LeaseSleeper, ScheduledLeaseOutcome, ScheduledLeaseRequest,
};
use memoryd::protocol::{CandidateWriteResult, DreamRunReport, LeaseRecord, PassOutcome, PassStatus};

#[test]
fn scheduled_transient_lease_unavailable_eventually_succeeds_within_retry_window() {
    let env = ScheduledEnv::new();
    let mut git = ScriptedLeaseGit::new()
        .with_fetch_results([Err("network blip".to_string()), Ok(())])
        .with_push_results([Ok(())]);

    let report = run_scheduled_lease_with_runner_and_sleeper(
        &mut git,
        ScheduledLeaseRequest {
            acquire: env.acquire_request("me"),
            retry_window_minutes: 3,
            run_date: fixed_now().date_naive(),
        },
        &ImmediateLeaseSleeper,
        |lease| Ok(lease.report.clone()),
    )
    .expect("transient fetch failure recovers inside retry window");

    assert_eq!(report.attempts, 2);
    assert_eq!(report.consecutive_missed_runs, 0);
    assert_eq!(git.fetch_calls(), 2);
    assert!(env.cleanup_summary().contains("\"outcome\":\"success\""));
}

#[test]
fn recovered_scheduled_lease_invokes_dream_run_callback() {
    let env = ScheduledEnv::new();
    let mut git = ScriptedLeaseGit::new()
        .with_fetch_results([Err("network blip".to_string()), Ok(())])
        .with_push_results([Ok(())]);
    let mut callback_runs = Vec::new();

    let report = run_scheduled_lease_with_runner_and_sleeper(
        &mut git,
        ScheduledLeaseRequest {
            acquire: env.acquire_request("me"),
            retry_window_minutes: 3,
            run_date: fixed_now().date_naive(),
        },
        &ImmediateLeaseSleeper,
        |lease| {
            callback_runs.push((lease.record.scope.clone(), lease.record.run_id.clone()));
            Ok(non_stub_dream_report(&lease.record.scope))
        },
    )
    .expect("transient lease failure recovers and runs dream callback");

    assert_eq!(report.outcome, ScheduledLeaseOutcome::Success);
    assert_eq!(callback_runs.len(), 1, "scheduled lease success must invoke the dream runner seam");
    assert_eq!(callback_runs[0].0, "me");
}

#[test]
fn post_acquire_dream_failure_releases_lease_before_returning_error() {
    let env = ScheduledEnv::new();
    let mut git = ScriptedLeaseGit::new().with_fetch_results([Ok(()), Ok(())]).with_push_results([Ok(()), Ok(())]);

    let err = run_scheduled_lease_with_runner_and_sleeper(
        &mut git,
        ScheduledLeaseRequest {
            acquire: env.acquire_request("me"),
            retry_window_minutes: 3,
            run_date: fixed_now().date_naive(),
        },
        &ImmediateLeaseSleeper,
        |_| Err(LeaseError::unavailable("dream_unavailable: harness selection failed")),
    )
    .expect_err("post-acquire dream failure is returned after release");

    assert!(matches!(err, LeaseError::Unavailable { message } if message.contains("dream_unavailable")));
    acquire_manual_lease_with_git(
        &mut git,
        LeaseAcquireRequest { now: fixed_now() + Duration::minutes(1), ..env.acquire_request("me") },
    )
    .expect("release record from failed dream run must leave no active lease behind");
}

#[test]
fn dream_failure_error_is_preserved_when_release_fails() {
    let env = ScheduledEnv::new();
    let mut git = ScriptedLeaseGit::new()
        .with_fetch_results([Ok(()), Ok(())])
        .with_push_results([Ok(()), Err("release push failed".to_string())]);

    let err = run_scheduled_lease_with_runner_and_sleeper(
        &mut git,
        ScheduledLeaseRequest {
            acquire: env.acquire_request("me"),
            retry_window_minutes: 3,
            run_date: fixed_now().date_naive(),
        },
        &ImmediateLeaseSleeper,
        |_| Err(LeaseError::unavailable("dream_unavailable: pass 2 timed out")),
    )
    .expect_err("dream failure should be returned even if release also fails");

    assert!(matches!(err, LeaseError::Unavailable { message } if message.contains("pass 2 timed out")));
}

#[test]
fn scheduled_retry_sleeps_between_attempts_with_exponential_backoff() {
    let env = ScheduledEnv::new();
    let mut git = ScriptedLeaseGit::new().with_fetch_results([
        Err("down".to_string()),
        Err("down".to_string()),
        Err("down".to_string()),
        Err("down".to_string()),
        Err("down".to_string()),
        Err("down".to_string()),
        Err("down".to_string()),
        Ok(()),
    ]);
    let sleeper = RecordingSleeper::default();

    let report = run_scheduled_lease_with_runner_and_sleeper(
        &mut git,
        ScheduledLeaseRequest {
            acquire: env.acquire_request("me"),
            retry_window_minutes: 95,
            run_date: fixed_now().date_naive(),
        },
        &sleeper,
        |lease| Ok(lease.report.clone()),
    )
    .expect("scheduled retry eventually succeeds");

    assert_eq!(report.outcome, ScheduledLeaseOutcome::Success);
    assert_eq!(sleeper.sleeps(), vec![1, 2, 4, 8, 16, 32, 32]);
}

#[test]
fn lease_run_ids_are_device_prefixed_and_never_collapse_to_run_zero() {
    let env = ScheduledEnv::new();
    let mut git = ScriptedLeaseGit::new().with_fetch_results([Ok(())]).with_push_results([Ok(())]);

    acquire_manual_lease_with_git(&mut git, env.acquire_request("me")).expect("lease acquired");

    let lease_text = std::fs::read_to_string(env.repo.join("leases/journal.lease")).expect("lease file");
    assert!(lease_text.contains("\"run_id\":\"run_dev_local_"), "{lease_text}");
    assert!(!lease_text.contains("\"run_id\":\"run_0\""), "{lease_text}");
}

#[derive(Default)]
struct RecordingSleeper {
    sleeps: std::sync::Mutex<Vec<u16>>,
}

impl RecordingSleeper {
    fn sleeps(&self) -> Vec<u16> {
        self.sleeps.lock().expect("sleeps lock").clone()
    }
}

impl LeaseSleeper for RecordingSleeper {
    fn sleep_minutes(&self, minutes: u16) {
        self.sleeps.lock().expect("sleeps lock").push(minutes);
    }
}

#[test]
fn scheduled_persistent_failure_records_missed_run_summary() {
    let env = ScheduledEnv::new();
    let mut git = ScriptedLeaseGit::new().with_fetch_results([
        Err("down".to_string()),
        Err("down".to_string()),
        Err("down".to_string()),
    ]);

    let report = run_scheduled_lease_with_runner_and_sleeper(
        &mut git,
        ScheduledLeaseRequest {
            acquire: env.acquire_request("me"),
            retry_window_minutes: 3,
            run_date: fixed_now().date_naive(),
        },
        &ImmediateLeaseSleeper,
        |lease| Ok(lease.report.clone()),
    )
    .expect("persistent failure is summarized, not thrown");

    assert_eq!(report.attempts, 3);
    assert_eq!(report.consecutive_missed_runs, 1);
    let summary = env.cleanup_summary();
    assert!(summary.contains("\"outcome\":\"missed\""));
    assert!(summary.contains("\"consecutive_missed_runs\":1"));
}

#[test]
fn scheduled_lease_held_is_not_retried_and_does_not_run_callback() {
    let env = ScheduledEnv::new();
    env.append_lease(LeaseRecord {
        device: "dev_foreign".to_string(),
        scope: "me".to_string(),
        acquired_at: fixed_now() - Duration::minutes(1),
        expires_at: Utc::now() + Duration::days(1),
        run_id: "run_foreign".to_string(),
    });
    let mut git = ScriptedLeaseGit::new().with_fetch_results([Ok(()), Ok(())]);
    let mut callback_runs = 0;

    let report = run_scheduled_lease_with_runner_and_sleeper(
        &mut git,
        ScheduledLeaseRequest {
            acquire: env.acquire_request("me"),
            retry_window_minutes: 3,
            run_date: fixed_now().date_naive(),
        },
        &ImmediateLeaseSleeper,
        |_| {
            callback_runs += 1;
            Ok(non_stub_dream_report("me"))
        },
    )
    .expect("held scheduled lease is summarized");

    assert_eq!(report.attempts, 1);
    assert_eq!(report.outcome, ScheduledLeaseOutcome::Held);
    assert_eq!(git.fetch_calls(), 1, "lease_held must not consume the retry window");
    assert_eq!(callback_runs, 0, "held leases must not invoke the dream runner");
}

#[test]
fn success_next_day_resets_consecutive_missed_runs() {
    let env = ScheduledEnv::new();
    let mut failing_git = ScriptedLeaseGit::new().with_fetch_results([
        Err("down".to_string()),
        Err("down".to_string()),
        Err("down".to_string()),
    ]);
    run_scheduled_lease_with_runner_and_sleeper(
        &mut failing_git,
        ScheduledLeaseRequest {
            acquire: env.acquire_request("me"),
            retry_window_minutes: 3,
            run_date: fixed_now().date_naive(),
        },
        &ImmediateLeaseSleeper,
        |lease| Ok(lease.report.clone()),
    )
    .expect("missed day summarized");

    let next_day = fixed_now() + Duration::days(1);
    let mut succeeding_git = ScriptedLeaseGit::new().with_fetch_results([Ok(())]).with_push_results([Ok(())]);
    let report = run_scheduled_lease_with_runner_and_sleeper(
        &mut succeeding_git,
        ScheduledLeaseRequest {
            acquire: LeaseAcquireRequest { now: next_day, ..env.acquire_request("me") },
            retry_window_minutes: 3,
            run_date: next_day.date_naive(),
        },
        &ImmediateLeaseSleeper,
        |lease| Ok(lease.report.clone()),
    )
    .expect("next day success resets missed counter");

    assert_eq!(report.consecutive_missed_runs, 0);
    assert!(env.cleanup_summary_for(next_day.date_naive()).contains("\"consecutive_missed_runs\":0"));
}

#[test]
fn retry_window_zero_disables_scheduled_retries() {
    let env = ScheduledEnv::new();
    let mut git = ScriptedLeaseGit::new().with_fetch_results([Err("first and only failure".to_string()), Ok(())]);
    let mut callback_runs = 0;

    let report = run_scheduled_lease_with_runner_and_sleeper(
        &mut git,
        ScheduledLeaseRequest {
            acquire: env.acquire_request("me"),
            retry_window_minutes: 0,
            run_date: fixed_now().date_naive(),
        },
        &ImmediateLeaseSleeper,
        |_| {
            callback_runs += 1;
            Ok(non_stub_dream_report("me"))
        },
    )
    .expect("zero retry window writes summary after first failure");

    assert_eq!(report.attempts, 1);
    assert_eq!(git.fetch_calls(), 1);
    assert_eq!(report.consecutive_missed_runs, 1);
    assert_eq!(callback_runs, 0, "failed no-retry scheduled leases must not invoke the dream runner");
}

fn fixed_now() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 4, 30, 3, 0, 0).single().expect("valid fixed time")
}

struct ScheduledEnv {
    _temp: tempfile::TempDir,
    repo: std::path::PathBuf,
    runtime: std::path::PathBuf,
}

impl ScheduledEnv {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let repo = temp.path().join("repo");
        let runtime = temp.path().join("runtime");
        std::fs::create_dir_all(repo.join("leases")).expect("leases");
        std::fs::create_dir_all(runtime.join("dream-state")).expect("runtime");
        std::fs::write(
            runtime.join("local-device.yaml"),
            format!(
                "schema_version: 1\ndevice:\n  id: dev_local\n  name: test\n  shard: test\npaths:\n  memory_root: {}\n  runtime_root: {}\n",
                repo.display(),
                runtime.display()
            ),
        )
        .expect("local device");
        std::fs::write(repo.join("leases/journal.lease"), "").expect("lease file");
        Self { _temp: temp, repo, runtime }
    }

    fn acquire_request(&self, scope: &str) -> LeaseAcquireRequest {
        LeaseAcquireRequest {
            repo: self.repo.clone(),
            runtime: self.runtime.clone(),
            scope: scope.to_string(),
            force: false,
            now: fixed_now(),
            lease_window_seconds: 3_600,
            cli_used: None,
        }
    }

    fn append_lease(&self, record: LeaseRecord) {
        use std::io::Write;

        let mut file =
            std::fs::OpenOptions::new().append(true).open(self.repo.join("leases/journal.lease")).expect("lease file");
        writeln!(file, "{}", serde_json::to_string(&record).expect("lease record")).expect("append lease");
    }

    fn cleanup_summary(&self) -> String {
        self.cleanup_summary_for(fixed_now().date_naive())
    }

    fn cleanup_summary_for(&self, date: chrono::NaiveDate) -> String {
        std::fs::read_to_string(
            self.repo.join("dreams/cleanup/dev_local").join(format!("{}.json", date.format("%Y-%m-%d"))),
        )
        .expect("cleanup summary")
    }
}

fn non_stub_dream_report(scope: &str) -> DreamRunReport {
    DreamRunReport {
        scope: scope.to_string(),
        cli_used: Some("test-runner".to_string()),
        pass_1: PassOutcome {
            status: PassStatus::Success,
            output_path: Some("dreams/journal/dev_local/2026-04-30.md".to_string()),
            candidate_results: Vec::<CandidateWriteResult>::new(),
            error_code: None,
            duration_ms: 1,
        },
        pass_2: skipped_pass(),
        pass_2_refusal_counts_by_reason: std::collections::BTreeMap::new(),
        pass_3: skipped_pass(),
        duration_ms: 3,
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
