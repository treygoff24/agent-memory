use std::io::IsTerminal;
use std::path::PathBuf;
use std::time::Duration;

use chrono::{DateTime, Utc};
use clap::Parser;
use memory_privacy::{
    DeterministicPrivacyClassifier, FileKeyProvider, KeyProvider, PrivacyClassifier, PrivacyNamespace,
};
use memory_substrate::{InitOptions, Roots, Substrate};
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::watch;

use memoryd::cli::{
    Cli, Command, DeviceCommand, DreamCommand, PeerCommand, PrivacyCommand, PrivacyFilterCommand, RealityCheckCommand,
    RecallCommand, ReviewCommand, SourceCommand, WebCommand,
};
use memoryd::client;
use memoryd::protocol::{
    render_peer_activity_human, render_peer_status_human, DreamRunReport, PassStatus, PeerActivityFormat,
    PeerReleaseLockStatus, RequestPayload, ResponsePayload, ResponseResult,
};
use memoryd::recall::{DeltaRequest, StartupRequest};
use memoryd::server::{self, ServerOptions};

#[allow(dead_code)]
mod state;

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Serve(args) => {
            let roots = Roots::new(args.repo, args.runtime);
            let substrate = if args.init {
                Substrate::init(roots, InitOptions { force_unsafe_durability: true, device_id: None }).await?
            } else {
                Substrate::open(roots).await?
            };
            let runtime_root = substrate.roots().runtime.clone();
            let _daemon_state = state::DaemonState::load(&runtime_root);
            if let Err(error) = state::RcSessionStore::new(&runtime_root).load_if_recent(Utc::now()) {
                eprintln!("warning: failed to recover daemon session state: {error}");
            }

            let (shutdown_tx, shutdown_rx) = watch::channel(false);
            tokio::spawn(install_termination_handler(shutdown_tx));

            server::serve_substrate_with(args.socket, substrate, ServerOptions::default(), shutdown_rx).await?;
        }
        Command::Mcp(args) => {
            memoryd::mcp_stdio::serve_stdio(&args.socket).await?;
        }
        Command::Status(args) => {
            print_response(client::request(&args.socket, "cli-status", RequestPayload::Status).await?)?;
        }
        Command::Doctor(args) => {
            let substrate = Substrate::open(Roots::new(args.repo, args.runtime)).await?;
            let response = memoryd::handlers::handle_request(
                &substrate,
                memoryd::protocol::RequestEnvelope::new("cli-doctor", RequestPayload::Doctor),
            )
            .await;
            let exit_code = doctor_cli_exit_code(&response);
            print_response(response)?;
            if exit_code != 0 {
                std::process::exit(exit_code);
            }
        }
        Command::Search(args) => {
            print_response(
                client::request(
                    &args.socket,
                    "cli-search",
                    RequestPayload::Search {
                        query: args.query,
                        limit: Some(args.limit),
                        include_body: args.include_body,
                    },
                )
                .await?,
            )?;
        }
        Command::Get(args) => {
            print_response(
                client::request(
                    &args.socket,
                    "cli-get",
                    RequestPayload::Get { id: args.id, include_provenance: args.include_provenance },
                )
                .await?,
            )?;
        }
        Command::WriteNote(args) => {
            print_response(
                client::request(&args.socket, "cli-write-note", RequestPayload::WriteNote { text: args.text }).await?,
            )?;
        }
        Command::Write(args) => {
            print_response(
                client::request(
                    &args.socket,
                    "cli-write",
                    RequestPayload::WriteMemory {
                        body: args.body,
                        title: args.title,
                        tags: args.tags,
                        meta: parse_meta(args.meta)?,
                    },
                )
                .await?,
            )?;
        }
        Command::Source(args) => match args.command {
            SourceCommand::Capture(capture) => {
                print_response(
                    client::request(
                        &capture.socket,
                        "cli-source-capture",
                        RequestPayload::CaptureSource {
                            url: capture.url,
                            excerpts: capture.excerpts,
                            note: capture.note,
                        },
                    )
                    .await?,
                )?;
            }
        },
        Command::Supersede(args) => {
            print_response(
                client::request(
                    &args.socket,
                    "cli-supersede",
                    RequestPayload::Supersede {
                        old_id: args.old_id,
                        content: args.content,
                        reason: args.reason,
                        meta: parse_meta(args.meta)?,
                    },
                )
                .await?,
            )?;
        }
        Command::Forget(args) => {
            print_response(
                client::request(
                    &args.socket,
                    "cli-forget",
                    RequestPayload::Forget { id: args.id, reason: args.reason },
                )
                .await?,
            )?;
        }
        Command::Review(args) => match args.command {
            ReviewCommand::Queue(queue) => {
                print_response(
                    client::request(
                        &queue.socket,
                        "cli-review-queue",
                        RequestPayload::ReviewQueue { limit: queue.limit },
                    )
                    .await?,
                )?;
            }
            ReviewCommand::Approve(approve) => {
                print_response(
                    client::request(
                        &approve.socket,
                        "cli-review-approve",
                        RequestPayload::ReviewApprove { id: approve.id },
                    )
                    .await?,
                )?;
            }
            ReviewCommand::Reject(reject) => {
                print_response(
                    client::request(
                        &reject.socket,
                        "cli-review-reject",
                        RequestPayload::ReviewReject { id: reject.id, reason: reject.reason },
                    )
                    .await?,
                )?;
            }
        },
        Command::Recall(args) => match args.command {
            RecallCommand::StartupBlock(args) => {
                let socket = recall_socket_path(&args.socket);
                let response = client::request(
                    &socket,
                    "cli-recall-startup",
                    RequestPayload::Startup(StartupRequest {
                        cwd: args.cwd.to_string_lossy().into_owned(),
                        session_id: args.session_id,
                        harness: args.harness,
                        harness_version: args.harness_version,
                        include_recent: args.include_recent && !args.no_include_recent,
                        since_event_id: None,
                        budget_tokens: args.budget_tokens,
                    }),
                )
                .await;
                print_recall_startup(response)?;
            }
            RecallCommand::DeltaBlock(args) => {
                let socket = recall_socket_path(&args.socket);
                let response = client::request(
                    &socket,
                    "cli-recall-delta",
                    RequestPayload::Delta(DeltaRequest {
                        cwd: args.cwd.to_string_lossy().into_owned(),
                        session_id: args.session_id,
                        harness: args.harness,
                        message: args.message,
                        budget_tokens: args.budget_tokens,
                    }),
                )
                .await;
                print_recall_delta(response)?;
            }
        },
        Command::Dream(args) => match args.command {
            DreamCommand::Status(args) => {
                let report = memoryd::dream::status::build_dream_status_report(&args.repo, &args.runtime)
                    .await
                    .map_err(anyhow::Error::msg)?;
                if args.json {
                    println!("{}", serde_json::to_string_pretty(&report)?);
                } else {
                    print!("{}", memoryd::dream::status::render_human_status(&report));
                }
            }
            DreamCommand::Now(args) => {
                let report = run_manual_dream(args).await?;
                println!("{}", serde_json::to_string_pretty(&report)?);
                if dream_report_failed(&report) {
                    std::process::exit(4);
                }
            }
            DreamCommand::Scheduled(args) => {
                let report = run_scheduled_dream(args).await?;
                println!("{}", serde_json::to_string_pretty(&report)?);
            }
            DreamCommand::Cleanup(args) => {
                let report = run_dream_cleanup(args).await?;
                println!("{}", serde_json::to_string_pretty(&report)?);
            }
            DreamCommand::Review(args) => {
                let report = memoryd::dream::review::collect_review(&args.repo, &args.since, args.scope.as_deref())
                    .map_err(anyhow::Error::msg)?;
                print!("{}", memoryd::dream::review::render_human_review(&report));
            }
            DreamCommand::Enable(args) => {
                println!("{}", memoryd::dream::status::PRIVACY_DISCLOSURE);
                memoryd::dream::status::enable_device(&args.runtime)?;
                println!(
                    "enabled: removed device-local sentinel {}",
                    memoryd::dream::status::disabled_sentinel_path(&args.runtime).display()
                );
            }
            DreamCommand::Disable(args) => {
                let path = memoryd::dream::status::disable_device(&args.runtime)?;
                println!("disabled: created device-local sentinel {}", path.display());
            }
        },
        Command::Peer(args) => match args.command {
            PeerCommand::Status(args) => {
                print_peer_status(client::request(&args.socket, "cli-peer-status", RequestPayload::PeerStatus).await);
            }
            PeerCommand::Activity(args) => {
                print_peer_activity(
                    client::request(
                        &args.socket,
                        "cli-peer-activity",
                        RequestPayload::PeerActivity {
                            session: args.session,
                            since: args.since,
                            limit: Some(args.limit),
                            format: args.format,
                        },
                    )
                    .await,
                    args.format,
                );
            }
            PeerCommand::ReleaseLock(args) => {
                run_peer_release_lock(args).await?;
            }
        },
        Command::Ui(args) => {
            run_tui(args)?;
        }
        Command::Web(args) => match args.command {
            WebCommand::Enable(args) => {
                let response = client::request(
                    &args.socket,
                    "cli-web-enable",
                    RequestPayload::WebEnable {
                        port: args.port,
                        socket_path: args.socket.to_string_lossy().into_owned(),
                    },
                )
                .await;
                print_web_response(response, WebOperation::Enable);
            }
            WebCommand::Disable(args) => {
                let response = client::request(&args.socket, "cli-web-disable", RequestPayload::WebDisable).await;
                print_web_response(response, WebOperation::Disable);
            }
            WebCommand::Status(args) => {
                let response = client::request(&args.socket, "cli-web-status", RequestPayload::WebStatus).await;
                print_web_status(response, args.json);
            }
        },
        Command::RealityCheck(args) => match args.command {
            RealityCheckCommand::Run(args) => {
                let request = if args.json {
                    RequestPayload::RealityCheck(memoryd::protocol::RealityCheckRequest::List {
                        namespace: args.namespace.clone(),
                        limit: args.top_n,
                    })
                } else {
                    RequestPayload::RealityCheck(memoryd::protocol::RealityCheckRequest::Run {
                        session_id: None,
                        namespace: args.namespace.clone(),
                        limit: args.top_n,
                    })
                };
                let response = client::request(&args.socket, "cli-reality-check-run", request).await;
                print_reality_check_run(response, args.json, args.tui);
            }
            RealityCheckCommand::Skip(args) => {
                let response = client::request(
                    &args.socket,
                    "cli-reality-check-skip",
                    RequestPayload::RealityCheck(memoryd::protocol::RealityCheckRequest::Skip),
                )
                .await;
                print_reality_check_skip(response);
            }
            RealityCheckCommand::Snooze(args) => {
                let until = match memoryd::cli::validate_snooze_until(args.until.as_deref()) {
                    Ok(until) => until.map(|date| date.and_hms_opt(0, 0, 0).expect("midnight is valid").and_utc()),
                    Err(_) => {
                        eprintln!("invalid date: --until must be YYYY-MM-DD");
                        std::process::exit(1);
                    }
                };
                let response = client::request(
                    &args.socket,
                    "cli-reality-check-snooze",
                    RequestPayload::RealityCheck(memoryd::protocol::RealityCheckRequest::Snooze { until }),
                )
                .await;
                print_reality_check_snooze(response);
            }
        },
        Command::Privacy(args) => match args.command {
            PrivacyCommand::Status(args) => {
                let key_provider = FileKeyProvider::runtime_default(&args.runtime);
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "stream": "D",
                        "layer1": "enabled",
                        "privacy_filter": "disabled",
                        "encrypted_key_available": key_provider.load_key().is_ok(),
                        "guidance": "Layer 1 regex/entropy scanning is always on; optional Privacy Filter is disabled unless configured."
                    }))?
                );
            }
            PrivacyCommand::Scan(scan) => {
                let text = match (scan.text, scan.file) {
                    (Some(text), None) => text,
                    (None, Some(path)) => std::fs::read_to_string(path)?,
                    _ => anyhow::bail!("provide exactly one of --text or --file"),
                };
                let classifier = DeterministicPrivacyClassifier::new();
                let decision = classifier.classify(&text, PrivacyNamespace::Project, None)?;
                println!("{}", serde_json::to_string_pretty(&decision)?);
            }
            PrivacyCommand::ScanDelta(args) => {
                let output = std::process::Command::new("git")
                    .args(["-C", args.repo.to_string_lossy().as_ref(), "diff", "--cached", "--no-ext-diff", "-U0"])
                    .output()?;
                if !output.status.success() {
                    anyhow::bail!("git diff --cached failed");
                }
                let text = String::from_utf8(output.stdout)?;
                let classifier = DeterministicPrivacyClassifier::new();
                let decision = classifier.classify(&text, PrivacyNamespace::Project, None)?;
                println!("{}", serde_json::to_string_pretty(&decision)?);
                if decision.tier == memory_privacy::PrivacyTier::Secret {
                    anyhow::bail!("staged delta contains secret-like material");
                }
            }
        },
        Command::PrivacyFilter(args) => match args.command {
            PrivacyFilterCommand::Install => {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "status": "not_installed",
                        "guidance": "No model weights are downloaded by normal tests. Install the optional OpenAI Privacy Filter out of band, then enable the provider."
                    }))?
                );
            }
            PrivacyFilterCommand::Enable => {
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &serde_json::json!({"status": "disabled", "reason": "provider runtime not configured"})
                    )?
                );
            }
            PrivacyFilterCommand::Disable => {
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({"status": "disabled"}))?);
            }
            PrivacyFilterCommand::Status => {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({"status": "disabled", "layer1": "enabled"}))?
                );
            }
        },
        Command::Device(args) => match args.command {
            DeviceCommand::Onboard(args) | DeviceCommand::RotateKeys(args) => {
                let provider = FileKeyProvider::runtime_default(&args.runtime);
                let key = provider.onboard_local_file()?;
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "status": "ok",
                        "recipient": key.recipient,
                        "key_path": provider.path(),
                        "guidance": "Local Stream D key material created for encrypted-tier writes."
                    }))?
                );
            }
            DeviceCommand::Revoke(args) => {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "status": "operator_required",
                        "device_id": args.device_id,
                        "runtime": args.runtime,
                        "guidance": "Remove the device recipient from trusted devices and rotate keys."
                    }))?
                );
            }
        },
    }
    Ok(())
}

async fn run_manual_dream(args: memoryd::cli::DreamNowArgs) -> anyhow::Result<DreamRunReport> {
    let config = memory_substrate::config::load_config(&args.repo, &args.runtime, None).map_err(anyhow::Error::msg)?;
    if !config.synced.dreams.enabled || memoryd::dream::status::disabled_sentinel_path(&args.runtime).exists() {
        eprintln!("dream_disabled: dreaming is disabled on this device");
        std::process::exit(1);
    }
    let cli_used = args.cli_used();
    let now = chrono::Utc::now();
    let result = memoryd::dream::lease::acquire_manual_lease(memoryd::dream::lease::LeaseAcquireRequest {
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
        let _ = memoryd::dream::lease::release_manual_lease(memoryd::dream::lease::LeaseAcquireRequest {
            repo: args.repo,
            runtime: args.runtime,
            scope: args.scope,
            force: false,
            now: chrono::Utc::now(),
            lease_window_seconds: u64::from(config.synced.dreams.lease_window_seconds),
            cli_used,
        });
        let message = error.to_string();
        if let Some(rest) = message.strip_prefix("invalid_request: dream_unavailable: ") {
            eprintln!("dream_unavailable: {rest}");
            std::process::exit(2);
        }
        if message.contains("unknown harness CLI override") {
            eprintln!("invalid_request: {message}");
            std::process::exit(1);
        }
    }

    run_result
}

fn run_tui(args: memoryd::cli::UiArgs) -> anyhow::Result<()> {
    if let Err(error) = memoryd::cli::validate_ui_stdin(std::io::stdin().is_terminal()) {
        eprintln!("{}", error.message());
        std::process::exit(error.exit_code());
    }

    let current_exe = std::env::current_exe()?;
    let path_env = std::env::var_os("PATH");
    let binary = match memoryd::cli::resolve_memoryd_tui_binary(&current_exe, path_env.as_deref()) {
        Ok(binary) => binary,
        Err(error) => {
            eprintln!("{}", error.message());
            std::process::exit(error.exit_code());
        }
    };

    let status = std::process::Command::new(binary).args(memoryd::cli::ui_subprocess_args(&args)).status()?;
    std::process::exit(status.code().unwrap_or(1));
}

#[derive(Debug, Clone, Copy)]
enum WebOperation {
    Enable,
    Disable,
}

fn print_web_response(response: anyhow::Result<memoryd::protocol::ResponseEnvelope>, operation: WebOperation) -> ! {
    match response {
        Ok(envelope) => match envelope.result {
            ResponseResult::Success(ResponsePayload::WebStatus(status)) => {
                match operation {
                    WebOperation::Enable => {
                        let url = status.url.as_deref().unwrap_or("http://localhost:7137");
                        println!("Web dashboard enabled at {url}");
                    }
                    WebOperation::Disable => println!("Web dashboard disabled"),
                }
                std::process::exit(0);
            }
            ResponseResult::Error(error) => {
                eprintln!("{}: {}", error.code, error.message);
                std::process::exit(web_protocol_exit_code(&error.code, operation));
            }
            other => {
                eprintln!("internal_error: daemon returned non-web response: {other:?}");
                std::process::exit(1);
            }
        },
        Err(error) => {
            eprintln!("daemon_unavailable: {error:#}");
            std::process::exit(match operation {
                WebOperation::Enable => 3,
                WebOperation::Disable => 1,
            });
        }
    }
}

fn print_web_status(response: anyhow::Result<memoryd::protocol::ResponseEnvelope>, json: bool) -> ! {
    match response {
        Ok(envelope) => match envelope.result {
            ResponseResult::Success(ResponsePayload::WebStatus(status)) => {
                if json {
                    println!("{}", serde_json::to_string_pretty(&status).expect("web status serializes"));
                } else if status.running {
                    println!("Web dashboard: running");
                    if let Some(url) = &status.url {
                        println!("URL: {url}");
                    }
                    if let Some(port) = status.port {
                        println!("Port: {port}");
                    }
                    if let Some(uptime) = status.uptime_seconds {
                        println!("Uptime: {uptime}s");
                    }
                    println!("Active connections: {}", status.active_connections);
                } else {
                    println!("Web dashboard: stopped");
                }
                std::process::exit(0);
            }
            ResponseResult::Error(error) => {
                eprintln!("{}: {}", error.code, error.message);
                std::process::exit(1);
            }
            other => {
                eprintln!("internal_error: daemon returned non-web-status response: {other:?}");
                std::process::exit(1);
            }
        },
        Err(error) => {
            eprintln!("daemon_unavailable: {error:#}");
            std::process::exit(1);
        }
    }
}

fn web_protocol_exit_code(code: &str, operation: WebOperation) -> i32 {
    match (operation, code) {
        (WebOperation::Enable, "port_in_use") => 1,
        (WebOperation::Enable, "invalid_request") => 2,
        (WebOperation::Enable, _) => 3,
        (WebOperation::Disable, _) => 1,
    }
}

fn print_reality_check_run(response: anyhow::Result<memoryd::protocol::ResponseEnvelope>, json: bool, tui: bool) -> ! {
    match response {
        Ok(envelope) => match envelope.result {
            ResponseResult::Success(ResponsePayload::RealityCheck(reality_check)) => {
                if json {
                    println!("{}", serde_json::to_string_pretty(&reality_check).expect("reality check serializes"));
                    std::process::exit(0);
                }
                print_reality_check_summary(&reality_check, tui);
            }
            ResponseResult::Error(error) => {
                eprintln!("{}: {}", error.code, error.message);
                std::process::exit(reality_check_error_exit_code(&error.code));
            }
            other => {
                eprintln!("internal_error: daemon returned non-reality-check response: {other:?}");
                std::process::exit(3);
            }
        },
        Err(error) => {
            eprintln!("daemon_unavailable: {error:#}");
            std::process::exit(3);
        }
    }
}

fn print_reality_check_summary(reality_check: &memoryd::protocol::RealityCheckResponse, tui: bool) -> ! {
    match reality_check {
        memoryd::protocol::RealityCheckResponse::Pending { session_id, items, total_scored, .. } => {
            if items.is_empty() {
                println!("No Reality Check items.");
                std::process::exit(1);
            }
            if tui {
                println!("Reality Check routed to TUI panel 8.");
            }
            println!("Reality Check session: {}", session_id.as_deref().unwrap_or("preview"));
            println!("Items: {} of {total_scored}", items.len());
            for item in items {
                println!("- {} [{}] score {:.2}: {}", item.memory_id.as_str(), item.namespace, item.score, item.title);
            }
            std::process::exit(0);
        }
        other => {
            println!("{}", serde_json::to_string_pretty(other).expect("reality check serializes"));
            std::process::exit(0);
        }
    }
}

fn print_reality_check_skip(response: anyhow::Result<memoryd::protocol::ResponseEnvelope>) -> ! {
    match response {
        Ok(envelope) => match envelope.result {
            ResponseResult::Success(ResponsePayload::RealityCheck(
                memoryd::protocol::RealityCheckResponse::Skipped { skipped_until },
            )) => {
                println!("Reality Check skipped until {skipped_until}");
                std::process::exit(0);
            }
            ResponseResult::Error(error) => {
                eprintln!("{}: {}", error.code, error.message);
                std::process::exit(reality_check_error_exit_code(&error.code));
            }
            other => {
                eprintln!("internal_error: daemon returned non-skip response: {other:?}");
                std::process::exit(2);
            }
        },
        Err(error) => {
            eprintln!("daemon_unavailable: {error:#}");
            std::process::exit(2);
        }
    }
}

fn print_reality_check_snooze(response: anyhow::Result<memoryd::protocol::ResponseEnvelope>) -> ! {
    match response {
        Ok(envelope) => match envelope.result {
            ResponseResult::Success(ResponsePayload::RealityCheck(
                memoryd::protocol::RealityCheckResponse::Snoozed { snooze_until },
            )) => {
                println!("Reality Check snoozed until {snooze_until}");
                std::process::exit(0);
            }
            ResponseResult::Error(error) => {
                eprintln!("{}: {}", error.code, error.message);
                std::process::exit(reality_check_error_exit_code(&error.code));
            }
            other => {
                eprintln!("internal_error: daemon returned non-snooze response: {other:?}");
                std::process::exit(2);
            }
        },
        Err(error) => {
            eprintln!("daemon_unavailable: {error:#}");
            std::process::exit(2);
        }
    }
}

fn reality_check_error_exit_code(code: &str) -> i32 {
    match code {
        "no_items" => 1,
        "session_abandoned" => 2,
        "invalid_request" => 1,
        _ => 3,
    }
}

async fn run_scheduled_dream(
    args: memoryd::cli::DreamScheduledArgs,
) -> anyhow::Result<memoryd::dream::lease::ScheduledLeaseReport> {
    let config = memory_substrate::config::load_config(&args.repo, &args.runtime, None).map_err(anyhow::Error::msg)?;
    if !config.synced.dreams.enabled || memoryd::dream::status::disabled_sentinel_path(&args.runtime).exists() {
        eprintln!("dream_disabled: dreaming is disabled on this device");
        std::process::exit(1);
    }

    let now = Utc::now();
    let cli_used = args.cli_used();
    let request = memoryd::dream::lease::ScheduledLeaseRequest {
        acquire: memoryd::dream::lease::LeaseAcquireRequest {
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
    let mut git = memoryd::dream::git::NativeLeaseGit;
    let sleeper = memoryd::dream::lease::RealLeaseSleeper;
    let report = memoryd::dream::lease::run_scheduled_lease_with_async_runner_and_sleeper(
        &mut git,
        request,
        &sleeper,
        |lease| {
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
        },
    )
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
    let scope = memoryd::dream::scope::DreamScope::parse(&invocation.raw_scope)
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;
    let build = memoryd::dream::orchestration::build_dream_run(
        &substrate,
        memoryd::dream::orchestration::DreamRunBuildRequest {
            scope,
            run_id: invocation.run_id,
            run_date: invocation.run_date,
            pass_timeout: Duration::from_secs(u64::from(invocation.dreams.per_pass_timeout_seconds)),
            pass_2_max_candidates: invocation.dreams.pass_2_max_candidates as usize,
            pass_1_window_days: invocation.dreams.pass_1_window_days,
        },
    )
    .await
    .map_err(|err| anyhow::anyhow!(err.to_string()))?;
    let harness = memoryd::dream::orchestration::select_harness(
        invocation.cli_used.as_deref(),
        &invocation.dreams.default_cli_priority,
        &build.options,
    )
    .await
    .map_err(|err| anyhow::anyhow!(err.to_string()))?;
    memoryd::dream::run::DreamRunner::new(build.options.with_harness(harness), build.writer)
        .run()
        .await
        .map_err(|err| anyhow::anyhow!(err.to_string()))
}

fn dream_run_error_to_lease_error(error: anyhow::Error) -> memoryd::dream::lease::LeaseError {
    let message = error.to_string();
    if message.contains("unknown harness CLI override") {
        return memoryd::dream::lease::LeaseError::InvalidRequest { message };
    }
    memoryd::dream::lease::LeaseError::unavailable(message)
}

async fn run_dream_cleanup(
    args: memoryd::cli::DreamCleanupArgs,
) -> anyhow::Result<memoryd::dream::report::CleanupReport> {
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
    memoryd::dream::cleanup::run_cleanup(
        &substrate,
        memoryd::dream::cleanup::CleanupConfig {
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

fn print_response(response: memoryd::protocol::ResponseEnvelope) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}

fn doctor_cli_exit_code(response: &memoryd::protocol::ResponseEnvelope) -> i32 {
    match &response.result {
        ResponseResult::Success(ResponsePayload::Doctor(doctor)) if !doctor.healthy => 1,
        _ => 0,
    }
}

fn print_recall_startup(response: anyhow::Result<memoryd::protocol::ResponseEnvelope>) -> anyhow::Result<()> {
    match response {
        Ok(envelope) => match envelope.result {
            ResponseResult::Success(ResponsePayload::Startup(startup)) => {
                print!("{}", startup.recall_block);
                Ok(())
            }
            ResponseResult::Error(error) => exit_protocol_error(error),
            other => anyhow::bail!("daemon returned non-startup response: {other:?}"),
        },
        Err(error) => exit_recall_unavailable(error),
    }
}

fn print_recall_delta(response: anyhow::Result<memoryd::protocol::ResponseEnvelope>) -> anyhow::Result<()> {
    match response {
        Ok(envelope) => match envelope.result {
            ResponseResult::Success(ResponsePayload::Delta(delta)) => {
                print!("{}", delta.delta_block);
                Ok(())
            }
            ResponseResult::Error(error) => exit_protocol_error(error),
            other => anyhow::bail!("daemon returned non-delta response: {other:?}"),
        },
        Err(error) => exit_recall_unavailable(error),
    }
}

fn print_peer_status(response: anyhow::Result<memoryd::protocol::ResponseEnvelope>) -> ! {
    match response {
        Ok(envelope) => match envelope.result {
            ResponseResult::Success(ResponsePayload::PeerStatus(status)) => {
                print!("{}", render_peer_status_human(&status));
                std::process::exit(0);
            }
            ResponseResult::Error(error) => {
                eprintln!("{}: {}", error.code, error.message);
                std::process::exit(2);
            }
            other => {
                eprintln!("internal_error: daemon returned non-peer-status response: {other:?}");
                std::process::exit(2);
            }
        },
        Err(error) => {
            eprintln!("peer_unreachable: {error:#}");
            std::process::exit(1);
        }
    }
}

fn print_peer_activity(response: anyhow::Result<memoryd::protocol::ResponseEnvelope>, format: PeerActivityFormat) -> ! {
    match response {
        Ok(envelope) => match envelope.result {
            ResponseResult::Success(ResponsePayload::PeerActivity(activity)) => {
                match format {
                    PeerActivityFormat::Human => print!("{}", render_peer_activity_human(&activity)),
                    PeerActivityFormat::Json => {
                        for entry in activity.entries {
                            match serde_json::to_string(&entry) {
                                Ok(line) => println!("{line}"),
                                Err(error) => {
                                    eprintln!("internal_error: failed to serialize peer activity: {error}");
                                    std::process::exit(2);
                                }
                            }
                        }
                    }
                }
                std::process::exit(0);
            }
            ResponseResult::Error(error) => {
                eprintln!("{}: {}", error.code, error.message);
                std::process::exit(2);
            }
            other => {
                eprintln!("internal_error: daemon returned non-peer-activity response: {other:?}");
                std::process::exit(2);
            }
        },
        Err(error) => {
            eprintln!("peer_unreachable: {error:#}");
            std::process::exit(1);
        }
    }
}

async fn run_peer_release_lock(args: memoryd::cli::PeerReleaseLockArgs) -> anyhow::Result<()> {
    if !args.yes {
        let status = match client::request(&args.socket, "cli-peer-release-status", RequestPayload::PeerStatus).await {
            Ok(envelope) => envelope,
            Err(error) => {
                eprintln!("peer_unreachable: {error:#}");
                std::process::exit(2);
            }
        };
        let ResponseResult::Success(ResponsePayload::PeerStatus(status)) = status.result else {
            eprintln!("internal_error: daemon did not return peer status before release-lock");
            std::process::exit(2);
        };
        let Some(lock) = status.claim_locks.iter().find(|lock| lock.memory_id == args.memory_id) else {
            eprintln!("no_lock_found: no active claim lock for {}", args.memory_id);
            std::process::exit(1);
        };
        eprint!(
            "Release claim lock on {} held by {}:{}? [y/N] ",
            lock.memory_id, lock.holder_harness, lock.holder_session_id
        );
        if !confirmed_on_stdin()? {
            eprintln!("aborted");
            std::process::exit(1);
        }
    }

    let response = match client::request(
        &args.socket,
        "cli-peer-release-lock",
        RequestPayload::PeerReleaseLock { memory_id: args.memory_id },
    )
    .await
    {
        Ok(response) => response,
        Err(error) => {
            eprintln!("peer_unreachable: {error:#}");
            std::process::exit(2);
        }
    };

    match response.result {
        ResponseResult::Success(ResponsePayload::PeerReleaseLock(release)) => match release.status {
            PeerReleaseLockStatus::Released => {
                println!("Released.");
                std::process::exit(0);
            }
            PeerReleaseLockStatus::NoLockFound => {
                eprintln!("no_lock_found: no active claim lock for {}", release.memory_id);
                std::process::exit(1);
            }
        },
        ResponseResult::Error(error) => {
            eprintln!("{}: {}", error.code, error.message);
            std::process::exit(2);
        }
        other => {
            eprintln!("internal_error: daemon returned non-peer-release-lock response: {other:?}");
            std::process::exit(2);
        }
    }
}

fn confirmed_on_stdin() -> anyhow::Result<bool> {
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    Ok(matches!(input.trim(), "y" | "Y" | "yes" | "YES"))
}

fn exit_protocol_error(error: memoryd::protocol::ProtocolError) -> ! {
    eprintln!("{}: {}", error.code, error.message);
    std::process::exit(recall_exit_code(&error.code));
}

fn exit_recall_unavailable(error: anyhow::Error) -> ! {
    eprintln!("recall_unavailable: {error:#}");
    std::process::exit(2);
}

fn exit_dream_error(error: memoryd::dream::lease::LeaseError) -> ! {
    eprintln!("{}: {}", error.code(), error);
    std::process::exit(error.cli_exit_code());
}

fn recall_exit_code(code: &str) -> i32 {
    match code {
        "invalid_request" => 1,
        "dream_disabled" => 1,
        "substrate_error" | "recall_unavailable" | "dream_unavailable" => 2,
        "privacy_error" => 3,
        "not_implemented" | "dream_pass_failed" => 4,
        "lease_held" | "lease_unavailable" | "lease_dirty_tree" => 5,
        _ => 1,
    }
}

fn recall_socket_path(args: &memoryd::cli::RecallSocketArgs) -> std::path::PathBuf {
    args.socket.clone().unwrap_or_else(|| args.runtime.join("memoryd.sock"))
}

fn parse_meta(meta: Option<String>) -> anyhow::Result<serde_json::Value> {
    match meta {
        Some(meta) => Ok(serde_json::from_str(&meta)?),
        None => Ok(serde_json::Value::Null),
    }
}

/// Wait for the first SIGINT or SIGTERM and signal the daemon to shut down.
async fn install_termination_handler(shutdown: watch::Sender<bool>) {
    let mut sigint = match signal(SignalKind::interrupt()) {
        Ok(handler) => handler,
        Err(error) => {
            eprintln!("memoryd: failed to install SIGINT handler: {error}");
            return;
        }
    };
    let mut sigterm = match signal(SignalKind::terminate()) {
        Ok(handler) => handler,
        Err(error) => {
            eprintln!("memoryd: failed to install SIGTERM handler: {error}");
            return;
        }
    };

    tokio::select! {
        _ = sigint.recv() => {}
        _ = sigterm.recv() => {}
    }
    let _ = shutdown.send(true);
}
