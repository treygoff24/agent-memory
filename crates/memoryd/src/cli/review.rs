use crate::cli::output::print_response;
use crate::cli::{ReviewArgs, ReviewCommand, ReviewMergesCommand};
use crate::client;
use crate::paths::resolve_socket_arg;
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
        ReviewCommand::Merges(merges) => {
            let (socket, request, request_id) = match merges.command {
                ReviewMergesCommand::List(args) => (args.socket, RequestPayload::ReviewMerges, "cli-review-merges"),
                ReviewMergesCommand::Approve(args) => (
                    args.socket,
                    RequestPayload::ReviewMergeApprove {
                        proposal_id: args.proposal_id,
                        approve_pinned: args.approve_pinned,
                    },
                    "cli-review-merge-approve",
                ),
                ReviewMergesCommand::Reject(args) => (
                    args.socket,
                    RequestPayload::ReviewMergeReject { proposal_id: args.proposal_id },
                    "cli-review-merge-reject",
                ),
            };
            print_response(client::request(resolve_socket_arg(&socket), request_id, request).await?)?;
        }
    }
    Ok(())
}
