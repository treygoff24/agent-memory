use std::path::Path;

use memory_substrate::{OpenError, Roots, Substrate};

use super::{DoctorArgs, McpArgs, SocketArgs};
use crate::cli::exit::doctor_cli_exit_code;
use crate::cli::output::print_response;
use crate::cli::paths::{resolve_repo_runtime_paths, resolve_status_socket_arg};
use crate::client;
use crate::protocol::RequestPayload;
use crate::socket::{await_socket_ready, spawn_serve_child, DaemonReadiness, DAEMON_READY_TIMEOUT};

pub async fn run_mcp(args: McpArgs) -> anyhow::Result<()> {
    let socket = args.socket.clone().unwrap_or_else(|| crate::socket::resolve_socket_path(&args.runtime));
    if args.auto_start && !matches!(crate::socket::probe_live_socket(&socket), crate::socket::SocketProbe::Live) {
        auto_start_daemon(&args.repo, &args.runtime, &socket).await?;
    }
    crate::mcp_stdio::serve_stdio_with_options(
        &socket,
        crate::mcp_stdio::StdioOptions { allow_reveal: args.allow_reveal },
    )
    .await?;
    Ok(())
}

pub async fn run_status(args: SocketArgs) -> anyhow::Result<()> {
    print_response(
        client::request(resolve_status_socket_arg(&args.socket), "cli-status", RequestPayload::Status).await?,
    )?;
    Ok(())
}

pub async fn run_doctor(args: DoctorArgs) -> anyhow::Result<()> {
    let (repo, runtime) = resolve_repo_runtime_paths(args.repo, args.runtime);
    let substrate = match Substrate::open(Roots::new(repo.clone(), runtime.clone())).await {
        Err(OpenError::NotAMemorumSubstrate { .. }) => {
            anyhow::bail!(
                "not a Memorum substrate at {}; run `memoryd init` to create one",
                repo.display()
            );
        }
        Err(other) => return Err(other.into()),
        Ok(substrate) => substrate,
    };
    if args.reindex {
        let rebuilt = substrate.doctor_reindex_events_log()?;
        eprintln!("doctor reindexed {rebuilt} canonical event log entries into SQLite");
    }
    let response = crate::handlers::handle_request(
        &substrate,
        crate::protocol::RequestEnvelope::new("cli-doctor", RequestPayload::Doctor),
    )
    .await;
    let exit_code = doctor_cli_exit_code(&response);
    print_response(response)?;
    if exit_code != 0 {
        std::process::exit(exit_code);
    }
    Ok(())
}

async fn auto_start_daemon(repo: &Path, runtime: &Path, socket: &Path) -> anyhow::Result<()> {
    let mut child = spawn_serve_child(repo, runtime, socket)?;
    match await_socket_ready(&mut child, socket, DAEMON_READY_TIMEOUT).await {
        DaemonReadiness::Ready => Ok(()),
        DaemonReadiness::ExitedEarly(status) => {
            anyhow::bail!("memoryd auto-start exited before readiness: {status}")
        }
        DaemonReadiness::PollFailed(error) => Err(error.into()),
        DaemonReadiness::TimedOut => {
            anyhow::bail!("memoryd auto-start did not become ready within 10s at {}", socket.display())
        }
    }
}
