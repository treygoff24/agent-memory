use crate::cli::output::print_response;
use crate::cli::paths::resolve_socket_arg;
use crate::cli::{ReviewArgs, ReviewCommand};
use crate::client;
use crate::protocol::RequestPayload;

pub async fn run(args: ReviewArgs) -> anyhow::Result<()> {
    match args.command {
        ReviewCommand::Queue(queue) => {
            print_response(
                client::request(
                    resolve_socket_arg(&queue.socket),
                    "cli-review-queue",
                    RequestPayload::ReviewQueue { limit: queue.limit },
                )
                .await?,
            )?;
        }
        ReviewCommand::Approve(approve) => {
            print_response(
                client::request(
                    resolve_socket_arg(&approve.socket),
                    "cli-review-approve",
                    RequestPayload::ReviewApprove { id: approve.id },
                )
                .await?,
            )?;
        }
        ReviewCommand::Reject(reject) => {
            print_response(
                client::request(
                    resolve_socket_arg(&reject.socket),
                    "cli-review-reject",
                    RequestPayload::ReviewReject { id: reject.id, reason: reject.reason },
                )
                .await?,
            )?;
        }
    }
    Ok(())
}
