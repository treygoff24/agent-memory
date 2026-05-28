use std::path::PathBuf;
use std::process::{Command as ProcessCommand, Stdio};
use std::time::Duration;

use memory_substrate::{Roots, Substrate};

use super::{DoctorArgs, McpArgs, SocketArgs};
use crate::cli::exit::doctor_cli_exit_code;
use crate::cli::output::print_response;
use crate::cli::paths::resolve_socket_arg;
use crate::client;
use crate::protocol::RequestPayload;

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
    print_response(client::request(resolve_socket_arg(&args.socket), "cli-status", RequestPayload::Status).await?)?;
    Ok(())
}

pub async fn run_doctor(args: DoctorArgs) -> anyhow::Result<()> {
    let substrate = Substrate::open(Roots::new(args.repo, args.runtime)).await?;
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

async fn auto_start_daemon(repo: &PathBuf, runtime: &PathBuf, socket: &PathBuf) -> anyhow::Result<()> {
    let exe = std::env::current_exe()?;
    let mut child = ProcessCommand::new(exe)
        .arg("serve")
        .arg("--repo")
        .arg(repo)
        .arg("--runtime")
        .arg(runtime)
        .arg("--init")
        .arg("--socket")
        .arg(socket)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        if matches!(crate::socket::probe_live_socket(socket), crate::socket::SocketProbe::Live) {
            return Ok(());
        }
        if let Some(status) = child.try_wait()? {
            anyhow::bail!("memoryd auto-start exited before readiness: {status}");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    anyhow::bail!("memoryd auto-start did not become ready within 10s at {}", socket.display())
}
