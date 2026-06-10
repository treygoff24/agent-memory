use std::path::PathBuf;
use std::time::Duration;

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
                execute_dream_run(DreamRunInvocation {
                    repo,
                    runtime,
                    raw_scope: scope,
                    run_id: lease.record.run_id,
                    run_date: now.date_naive(),
                    dreams,
                    cli_used,
                })
                .await
                .map_err(dream_run_error_to_lease_error)
            }
        })
        .await;
    match report {
        Ok(report) => Ok(report),
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
