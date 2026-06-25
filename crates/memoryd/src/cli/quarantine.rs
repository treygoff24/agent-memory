use crate::cli::output::print_response;
use crate::cli::{QuarantineArgs, QuarantineCommand};
use crate::client;
use crate::paths::resolve_socket_arg;
use crate::protocol::RequestPayload;

pub async fn run(args: QuarantineArgs) -> anyhow::Result<()> {
    match args.command {
        QuarantineCommand::List(list) => {
            print_response(
                client::request(
                    resolve_socket_arg(&list.socket),
                    "cli-quarantine-list",
                    RequestPayload::ConflictsList { limit: list.limit },
                )
                .await?,
            )?;
        }
        QuarantineCommand::Resolve(resolve) => {
            let socket = resolve_socket_arg(&resolve.socket);
            let mode = resolve.mode();
            print_response(
                client::request(
                    socket,
                    "cli-quarantine-resolve",
                    RequestPayload::QuarantineResolve { id: resolve.id, mode },
                )
                .await?,
            )?;
        }
    }
    Ok(())
}
