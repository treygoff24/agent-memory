use clap::Parser;
use memory_privacy::{
    DeterministicPrivacyClassifier, FileKeyProvider, KeyProvider, PrivacyClassifier, PrivacyNamespace,
};
use memory_substrate::{InitOptions, Roots, Substrate};
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::watch;

use memoryd::cli::{Cli, Command, DeviceCommand, PrivacyCommand, PrivacyFilterCommand, ReviewCommand};
use memoryd::client;
use memoryd::protocol::RequestPayload;
use memoryd::server::{self, ServerOptions};

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

            let (shutdown_tx, shutdown_rx) = watch::channel(false);
            tokio::spawn(install_termination_handler(shutdown_tx));

            server::serve_substrate_with(args.socket, substrate, ServerOptions::default(), shutdown_rx).await?;
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
            print_response(response)?;
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

fn print_response(response: memoryd::protocol::ResponseEnvelope) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
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
