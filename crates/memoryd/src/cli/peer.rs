use crate::cli::paths::resolve_socket_arg;
use crate::cli::{PeerArgs, PeerCommand, PeerReleaseLockArgs};
use crate::client;
use crate::protocol::{
    render_peer_activity_human, render_peer_status_human, PeerActivityFormat, PeerReleaseLockStatus, RequestPayload,
    ResponseEnvelope, ResponsePayload, ResponseResult,
};

pub async fn run(args: PeerArgs) -> anyhow::Result<()> {
    match args.command {
        PeerCommand::Status(args) => {
            print_peer_status(
                client::request(resolve_socket_arg(&args.socket), "cli-peer-status", RequestPayload::PeerStatus).await,
            );
        }
        PeerCommand::Activity(args) => {
            print_peer_activity(
                client::request(
                    resolve_socket_arg(&args.socket),
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
    }
    Ok(())
}

fn print_peer_status(response: anyhow::Result<ResponseEnvelope>) -> ! {
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

fn print_peer_activity(response: anyhow::Result<ResponseEnvelope>, format: PeerActivityFormat) -> ! {
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

async fn run_peer_release_lock(args: PeerReleaseLockArgs) -> anyhow::Result<()> {
    if !args.yes {
        let status = match client::request(
            resolve_socket_arg(&args.socket),
            "cli-peer-release-status",
            RequestPayload::PeerStatus,
        )
        .await
        {
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
        resolve_socket_arg(&args.socket),
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
