use clap::Parser;
use memory_substrate::{InitOptions, Roots, Substrate};
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::watch;

use memoryd::cli::{Cli, Command, ReviewCommand};
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
