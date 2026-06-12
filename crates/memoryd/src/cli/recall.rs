use crate::cli::exit::{exit_protocol_error, exit_recall_unavailable};
use crate::cli::{RecallArgs, RecallCommand};
use crate::client;
use crate::paths::resolve_socket_with_runtime;
use crate::protocol::{RequestPayload, ResponsePayload, ResponseResult};
use crate::recall::{DeltaRequest, StartupRequest};

pub async fn run(args: RecallArgs) -> anyhow::Result<()> {
    match args.command {
        RecallCommand::StartupBlock(args) => {
            let socket = resolve_socket_with_runtime(&args.socket.socket, &args.socket.runtime);
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
            let socket = resolve_socket_with_runtime(&args.socket.socket, &args.socket.runtime);
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
    }
    Ok(())
}

fn print_recall_startup(response: anyhow::Result<crate::protocol::ResponseEnvelope>) -> anyhow::Result<()> {
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

fn print_recall_delta(response: anyhow::Result<crate::protocol::ResponseEnvelope>) -> anyhow::Result<()> {
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
