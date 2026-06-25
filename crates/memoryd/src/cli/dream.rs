use std::path::PathBuf;
use std::time::Duration;

use crate::substrate_git_lock::flush_substrate_writes;
use chrono::{DateTime, Utc};
use memory_substrate::{Roots, Substrate};

use crate::cli::exit::exit_dream_error;
use crate::protocol::{DreamRunReport, PassStatus};

pub async fn run(args: crate::cli::DreamArgs) -> anyhow::Result<()> {
    match args.command {
        crate::cli::DreamCommand::Status(args) => {
            let report = crate::dream::status::build_dream_status_report(&args.repo, &args.runtime)
                .await
                .map_err(anyhow::Error::msg)?;
            if args.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print!("{}", crate::dream::status::render_human_status(&report));
            }
        }
        crate::cli::DreamCommand::Now(args) => {
            let report = run_manual_dream(args).await?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            if dream_report_failed(&report) {
                std::process::exit(4);
            }
        }
        crate::cli::DreamCommand::Scheduled(args) => {
            let report = run_scheduled_dream(args).await?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        crate::cli::DreamCommand::Cleanup(args) => {
            let report = run_dream_cleanup(args).await?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        crate::cli::DreamCommand::Review(args) => {
            let report = crate::dream::review::collect_review(&args.repo, &args.since, args.scope.as_deref())
                .map_err(anyhow::Error::msg)?;
            print!("{}", crate::dream::review::render_human_review(&report));
        }
        crate::cli::DreamCommand::Calibration(args) => {
            // Read every device's calibration log directly off the synced tree —
            // no daemon round-trip — and report accept-rate per confidence decile.
            let report = crate::dream::calibration::build_report(&args.repo)?;
            if args.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print!("{}", crate::dream::calibration::render_human_report(&report));
            }
        }
        crate::cli::DreamCommand::Enable(args) => {
            println!("{}", crate::dream::status::PRIVACY_DISCLOSURE);
            crate::dream::status::enable_device(&args.runtime)?;
            println!(
                "enabled: removed device-local sentinel {}",
                crate::dream::status::disabled_sentinel_path(&args.runtime).display()
            );
        }
        crate::cli::DreamCommand::Disable(args) => {
            let path = crate::dream::status::disable_device(&args.runtime)?;
            println!("disabled: created device-local sentinel {}", path.display());
        }
    }
    Ok(())
}

async fn run_manual_dream(args: crate::cli::DreamNowArgs) -> anyhow::Result<DreamRunReport> {
    let config = memory_substrate::config::load_config(&args.repo, &args.runtime, None).map_err(anyhow::Error::msg)?;
    if !config.synced.dreams.enabled || crate::dream::status::disabled_sentinel_path(&args.runtime).exists() {
        eprintln!("dream_disabled: dreaming is disabled on this device");
        std::process::exit(1);
    }
    flush_substrate_writes(&args.repo, &args.runtime)?;
    let cli_used = args.cli_used();
    let now = chrono::Utc::now();
    let result = crate::dream::lease::acquire_manual_lease(crate::dream::lease::LeaseAcquireRequest {
        repo: args.repo.clone(),
        runtime: args.runtime.clone(),
        scope: args.scope.clone(),
        force: args.force,
        now,
        lease_window_seconds: u64::from(config.synced.dreams.lease_window_seconds),
        cli_used: cli_used.clone(),
    });
    let acquired = match result {
        Ok(acquired) => acquired,
        Err(error) => exit_dream_error(error),
    };

    let run_result = async {
        execute_dream_run(DreamRunInvocation {
            repo: args.repo.clone(),
            runtime: args.runtime.clone(),
            raw_scope: args.scope.clone(),
            run_id: acquired.record.run_id,
            run_date: now.date_naive(),
            dreams: config.synced.dreams.clone(),
            cli_used: cli_used.clone(),
        })
        .await
    }
    .await;

    let post_flush = flush_substrate_writes(&args.repo, &args.runtime);
    if let Err(error) = &run_result {
        let _ = crate::dream::lease::release_manual_lease(crate::dream::lease::LeaseAcquireRequest {
            repo: args.repo,
            runtime: args.runtime,
            scope: args.scope,
            force: false,
            now: chrono::Utc::now(),
            lease_window_seconds: u64::from(config.synced.dreams.lease_window_seconds),
            cli_used,
        });
        match error.downcast_ref::<crate::dream::types::DreamError>() {
            Some(crate::dream::types::DreamError::Unavailable { message }) => {
                eprintln!("dream_unavailable: {message}");
                std::process::exit(2);
            }
            Some(crate::dream::types::DreamError::UnknownHarnessOverride { .. }) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
            _ => {}
        }
    }
    post_flush?;

    run_result
}

async fn run_scheduled_dream(
    args: crate::cli::DreamScheduledArgs,
) -> anyhow::Result<crate::dream::lease::ScheduledLeaseReport> {
    let config = memory_substrate::config::load_config(&args.repo, &args.runtime, None).map_err(anyhow::Error::msg)?;
    if !config.synced.dreams.enabled || crate::dream::status::disabled_sentinel_path(&args.runtime).exists() {
        eprintln!("dream_disabled: dreaming is disabled on this device");
        std::process::exit(1);
    }

    let now = Utc::now();
    let cli_used = args.cli_used();
    flush_substrate_writes(&args.repo, &args.runtime)?;
    let request = crate::dream::lease::ScheduledLeaseRequest {
        acquire: crate::dream::lease::LeaseAcquireRequest {
            repo: args.repo.clone(),
            runtime: args.runtime.clone(),
            scope: args.scope.clone(),
            force: false,
            now,
            lease_window_seconds: u64::from(config.synced.dreams.lease_window_seconds),
            cli_used: cli_used.clone(),
        },
        retry_window_minutes: u16::try_from(config.synced.dreams.dream_retry_window_minutes)
            .map_err(|_| anyhow::anyhow!("dream_retry_window_minutes exceeds u16"))?,
        run_date: now.date_naive(),
    };
    let mut git = crate::dream::git::NativeLeaseGit;
    let sleeper = crate::dream::lease::RealLeaseSleeper;
    let report =
        crate::dream::lease::run_scheduled_lease_with_async_runner_and_sleeper(&mut git, request, &sleeper, |lease| {
            let repo = args.repo.clone();
            let runtime = args.runtime.clone();
            let scope = args.scope.clone();
            let dreams = config.synced.dreams.clone();
            let cli_used = cli_used.clone();
            async move {
                let run_result = execute_dream_run(DreamRunInvocation {
                    repo: repo.clone(),
                    runtime: runtime.clone(),
                    raw_scope: scope,
                    run_id: lease.record.run_id,
                    run_date: now.date_naive(),
                    dreams,
                    cli_used,
                })
                .await;
                let flush_result = flush_substrate_writes(&repo, &runtime)
                    .map_err(|error| crate::dream::lease::LeaseError::unavailable(error.to_string()));
                match (run_result, flush_result) {
                    (Ok(report), Ok(())) => Ok(report),
                    (Ok(_), Err(error)) => Err(error),
                    (Err(error), _) => Err(dream_run_error_to_lease_error(error)),
                }
            }
        })
        .await;
    match report {
        Ok(report) => {
            flush_substrate_writes(&args.repo, &args.runtime)?;
            Ok(report)
        }
        Err(error) => exit_dream_error(error),
    }
}

struct DreamRunInvocation {
    repo: PathBuf,
    runtime: PathBuf,
    raw_scope: String,
    run_id: String,
    run_date: chrono::NaiveDate,
    dreams: memory_substrate::config::DreamsConfig,
    cli_used: Option<String>,
}

async fn execute_dream_run(invocation: DreamRunInvocation) -> anyhow::Result<DreamRunReport> {
    let substrate = Substrate::open(Roots::new(invocation.repo.clone(), invocation.runtime)).await?;
    let scope = crate::dream::scope::DreamScope::parse(&invocation.raw_scope)
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;
    let build = crate::dream::orchestration::build_dream_run(
        &substrate,
        crate::dream::orchestration::DreamRunBuildRequest {
            scope,
            run_id: invocation.run_id,
            run_date: invocation.run_date,
            prompt_version: invocation.dreams.prompt_version,
            notifications: None,
            pass_timeout: Duration::from_secs(u64::from(invocation.dreams.per_pass_timeout_seconds)),
            pass_2_max_candidates: invocation.dreams.pass_2_max_candidates as usize,
            pass_1_window_days: invocation.dreams.pass_1_window_days,
        },
    )
    .await
    .map_err(|err| anyhow::anyhow!(err.to_string()))?;
    let harness = crate::dream::orchestration::select_harness(
        invocation.cli_used.as_deref(),
        &invocation.dreams.default_cli_priority,
        &build.options,
    )
    .await
    // Preserve the typed `DreamError` (rather than flattening to a string) so the
    // manual/scheduled dispatch sites can match its variant instead of sniffing text.
    .map_err(anyhow::Error::new)?;
    crate::dream::run::DreamRunner::new(build.options.with_harness(harness), build.writer)
        .run()
        .await
        .map_err(|err| anyhow::anyhow!(err.to_string()))
}

fn dream_run_error_to_lease_error(error: anyhow::Error) -> crate::dream::lease::LeaseError {
    if matches!(
        error.downcast_ref::<crate::dream::types::DreamError>(),
        Some(crate::dream::types::DreamError::UnknownHarnessOverride { .. })
    ) {
        return crate::dream::lease::LeaseError::InvalidRequest { message: error.to_string() };
    }
    crate::dream::lease::LeaseError::unavailable(error.to_string())
}

async fn run_dream_cleanup(args: crate::cli::DreamCleanupArgs) -> anyhow::Result<crate::dream::report::CleanupReport> {
    let loaded = memory_substrate::config::load_config(&args.repo, &args.runtime, None).map_err(anyhow::Error::msg)?;
    let device_id = match args.device_id {
        Some(device_id) => device_id,
        None => loaded
            .local
            .as_ref()
            .map(|local| local.device.id.clone())
            .ok_or_else(|| anyhow::anyhow!("local-device.yaml is missing; pass --device-id"))?,
    };
    let now = parse_cleanup_now(args.now)?;
    let substrate = Substrate::open(Roots::new(args.repo, args.runtime)).await?;
    crate::dream::cleanup::run_cleanup(
        &substrate,
        crate::dream::cleanup::CleanupConfig {
            device_id,
            now,
            fragment_lifetime_days: i64::from(loaded.synced.dreams.fragment_lifetime_days),
            candidate_stale_days: i64::from(loaded.synced.dreams.candidate_stale_days),
            event_compaction_days: i64::from(loaded.synced.events.compaction_days),
        },
    )
    .await
    .map_err(|err| anyhow::anyhow!(err.to_string()))
}

fn parse_cleanup_now(raw: Option<String>) -> anyhow::Result<DateTime<Utc>> {
    match raw {
        Some(raw) => Ok(DateTime::parse_from_rfc3339(&raw)?.with_timezone(&Utc)),
        None => Ok(Utc::now()),
    }
}

fn dream_report_failed(report: &DreamRunReport) -> bool {
    [report.pass_1.status, report.pass_2.status, report.pass_3.status].contains(&PassStatus::Failed)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use chrono::Utc;
    use memory_substrate::git::{commit_substrate_writes, count_substrate_write_changes};
    use memory_substrate::tree::bootstrap_repo_tree;
    use memory_substrate::{
        ClassificationOutcome, ObserveKind, Roots, Substrate, SubstrateFragmentAppendRequest, SubstrateFragmentPayload,
    };
    use tempfile::TempDir;

    use super::{flush_substrate_writes, run_manual_dream, run_scheduled_dream};

    #[tokio::test]
    async fn pre_dream_flush_in_manual_dream_leaves_clean_tree() {
        let env = DreamFlushEnv::new("dev_manualpre").await;
        env.write("sources/web/pre-manual/manifest.json", "{}\n");

        let report = run_manual_dream(env.now_args("me")).await.expect("manual dream succeeds");

        assert_eq!(report.scope, "me");
        assert_eq!(env.git(["status", "--porcelain"]), "");
        assert!(
            env.git(["log", "--format=%s"]).lines().any(|subject| subject.starts_with("substrate: commit")),
            "pre-dream substrate flush should commit before lease acquisition"
        );
    }

    #[tokio::test]
    async fn pre_dream_flush_in_scheduled_dream_leaves_clean_tree() {
        let env = DreamFlushEnv::new("dev_scheduledpre").await;
        env.write("sources/web/pre-scheduled/manifest.json", "{}\n");

        let report = run_scheduled_dream(env.scheduled_args("me")).await.expect("scheduled dream succeeds");

        assert_eq!(report.outcome, crate::dream::lease::ScheduledLeaseOutcome::Success);
        assert_eq!(env.git(["status", "--porcelain"]), "");
    }

    #[tokio::test]
    async fn post_dream_flush_commits_candidate_writes_before_return() {
        let env = DreamFlushEnv::new("dev_candidatepost").await;
        env.append_fragment("agent").await;

        let report = run_manual_dream(env.now_args("agent")).await.expect("manual dream succeeds");

        assert!(report.pass_2.candidate_results.iter().any(|result| result.accepted), "{report:?}");
        let candidate = env
            .first_file_under("agent/decisions")
            .unwrap_or_else(|| panic!("candidate file missing under agent/decisions"));
        env.git(["ls-files", "--error-unmatch", candidate.as_str()]);
        assert_eq!(env.git(["status", "--porcelain"]), "");
    }

    #[tokio::test]
    async fn partial_dream_writes_committed_before_release_on_error() {
        let env = DreamFlushEnv::new("dev_partialerror").await;
        crate::dream::lease::acquire_manual_lease(env.lease_request("me")).expect("lease acquired");
        env.write("me/knowledge/partial-candidate.md", "---\nsummary: partial\n---\npartial\n");

        flush_substrate_writes(&env.repo, &env.runtime).expect("post-dream flush");
        crate::dream::lease::release_manual_lease(env.lease_request("me")).expect("lease released");

        let subjects = env.git(["log", "-3", "--format=%s"]);
        let mut lines = subjects.lines();
        assert!(lines.next().expect("release commit").starts_with("dream: lease release"), "{subjects}");
        assert!(lines.next().expect("substrate commit").starts_with("substrate: commit"), "{subjects}");
        env.git(["ls-files", "--error-unmatch", "me/knowledge/partial-candidate.md"]);
        assert_eq!(env.git(["status", "--porcelain"]), "");
    }

    struct DreamFlushEnv {
        _temp: TempDir,
        repo: PathBuf,
        runtime: PathBuf,
    }

    impl DreamFlushEnv {
        async fn new(device: &str) -> Self {
            let temp = tempfile::tempdir().expect("tempdir");
            let repo = temp.path().join("repo");
            let runtime = temp.path().join("runtime");
            bootstrap_repo_tree(&repo).expect("bootstrap repo");
            std::fs::write(
                repo.join("config.yaml"),
                "schema_version: 1\nactive_embedding:\n  provider: synthetic\n  model_ref: stream-a-test\n  dimension: 32\nsubstrate:\n  commit_debounce_ms: 10\n",
            )
            .expect("config");
            command(&repo, "git", ["init"]);
            commit_substrate_writes(&repo, 1).expect("baseline commit");
            std::fs::create_dir_all(&runtime).expect("runtime");
            std::fs::write(
                runtime.join("local-device.yaml"),
                format!("schema_version: 1\ndevice:\n  id: {device}\n  name: local\n  shard: test\n"),
            )
            .expect("local device");
            let substrate = Substrate::open(Roots::new(repo.clone(), runtime.clone())).await.expect("open substrate");
            drop(substrate);
            let write_count = count_substrate_write_changes(&repo).expect("post-open count");
            if write_count > 0 {
                commit_substrate_writes(&repo, write_count).expect("post-open commit");
            }
            Self { _temp: temp, repo, runtime }
        }

        fn now_args(&self, scope: &str) -> crate::cli::DreamNowArgs {
            crate::cli::DreamNowArgs {
                repo: self.repo.clone(),
                runtime: self.runtime.clone(),
                scope: scope.to_string(),
                force: false,
                cli_override: Some("echo".to_string()),
                json: true,
            }
        }

        fn scheduled_args(&self, scope: &str) -> crate::cli::DreamScheduledArgs {
            crate::cli::DreamScheduledArgs {
                repo: self.repo.clone(),
                runtime: self.runtime.clone(),
                scope: scope.to_string(),
                cli_override: Some("echo".to_string()),
                json: true,
            }
        }

        fn lease_request(&self, scope: &str) -> crate::dream::lease::LeaseAcquireRequest {
            crate::dream::lease::LeaseAcquireRequest {
                repo: self.repo.clone(),
                runtime: self.runtime.clone(),
                scope: scope.to_string(),
                force: false,
                now: Utc::now(),
                lease_window_seconds: 3_600,
                cli_used: Some("echo".to_string()),
            }
        }

        async fn append_fragment(&self, scope: &str) {
            let substrate = Substrate::open(Roots::new(self.repo.clone(), self.runtime.clone())).await.expect("open");
            substrate
                .append_substrate_fragment(SubstrateFragmentAppendRequest {
                    id: None,
                    at: Utc::now(),
                    session: Some("sess_flush".to_string()),
                    harness: Some("codex".to_string()),
                    scope: scope.to_string(),
                    entities: vec!["ent_flush".to_string()],
                    kind: ObserveKind::Observation,
                    source_ref: None,
                    privacy_spans: Vec::new(),
                    payload: SubstrateFragmentPayload::Plaintext { text: "flush candidate evidence".to_string() },
                    classification: ClassificationOutcome::Trusted,
                    operation_id: None,
                })
                .await
                .expect("append fragment");
        }

        fn write(&self, relative: &str, text: &str) {
            let path = self.repo.join(relative);
            std::fs::create_dir_all(path.parent().expect("relative path has parent")).expect("parent dir");
            std::fs::write(path, text).expect("write file");
        }

        fn first_file_under(&self, relative: &str) -> Option<String> {
            let root = self.repo.join(relative);
            let mut stack = vec![root];
            while let Some(path) = stack.pop() {
                let entries = std::fs::read_dir(path).ok()?;
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        stack.push(path);
                    } else if path.extension().is_some_and(|ext| ext == "md") {
                        return path.strip_prefix(&self.repo).ok()?.to_str().map(str::to_string);
                    }
                }
            }
            None
        }

        fn git<const N: usize>(&self, args: [&str; N]) -> String {
            command(&self.repo, "git", args)
        }
    }

    fn command<const N: usize>(cwd: &Path, program: &str, args: [&str; N]) -> String {
        let output = Command::new(program).args(args).current_dir(cwd).output().expect("command starts");
        assert!(
            output.status.success(),
            "{program} failed in {}\nstdout:\n{}\nstderr:\n{}",
            cwd.display(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }
}
