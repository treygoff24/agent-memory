use std::path::Path;

use memory_substrate::{OpenError, Roots, Substrate};

use super::{DoctorArgs, McpArgs, SocketArgs};
use crate::cli::exit::doctor_cli_exit_code;
use crate::cli::output::print_response;
use crate::client;
use crate::paths::{resolve_repo_runtime_paths, resolve_socket_arg, resolve_socket_with_runtime};
use crate::protocol::RequestPayload;
use crate::socket::{await_socket_ready, spawn_serve_child, DaemonReadiness, DAEMON_READY_TIMEOUT};

pub async fn run_mcp(args: McpArgs) -> anyhow::Result<()> {
    let socket_arg = args.socket;
    let runtime_overridden = args.runtime.is_some();
    let repo_overridden = args.repo.is_some();
    let (repo, runtime) = resolve_repo_runtime_paths(args.repo, args.runtime);
    let socket = if runtime_overridden || repo_overridden {
        resolve_socket_with_runtime(&socket_arg, &runtime)
    } else {
        resolve_socket_arg(&socket_arg)
    };
    if args.auto_start && !matches!(crate::socket::probe_live_socket(&socket), crate::socket::SocketProbe::Live) {
        auto_start_daemon(&repo, &runtime, &socket).await?;
    }
    crate::mcp_stdio::serve_stdio_with_options(
        &socket,
        crate::mcp_stdio::StdioOptions { allow_reveal: args.allow_reveal },
    )
    .await?;
    Ok(())
}

pub async fn run_status(args: SocketArgs) -> anyhow::Result<()> {
    let socket = resolve_socket_arg(&args.socket);
    match client::request(&socket, "cli-status", RequestPayload::Status).await {
        Ok(response) => crate::cli::output::emit_and_exit(response),
        Err(error) => crate::cli::output::emit_transport_error_and_exit(error, &socket),
    }
}

pub async fn run_doctor(args: DoctorArgs) -> anyhow::Result<()> {
    let (repo, runtime) = resolve_repo_runtime_paths(args.repo, args.runtime);
    let substrate = match Substrate::open(Roots::new(repo.clone(), runtime.clone())).await {
        Err(OpenError::NotAMemorumSubstrate { .. }) => {
            anyhow::bail!("not a Memorum substrate at {}; run `memoryd init` to create one", repo.display());
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
