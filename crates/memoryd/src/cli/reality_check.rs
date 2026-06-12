use crate::cli::{RealityCheckArgs, RealityCheckCommand};
use crate::client;
use crate::paths::resolve_socket_arg;
use crate::protocol::{
    RealityCheckRequest, RealityCheckResponse, RequestPayload, ResponseEnvelope, ResponsePayload, ResponseResult,
};

pub async fn run(args: RealityCheckArgs) -> anyhow::Result<()> {
    match args.command {
        RealityCheckCommand::Run(args) => {
            let namespace = args.namespace;
            let limit = args.top_n;
            let request = if args.json {
                RequestPayload::RealityCheck(RealityCheckRequest::List { namespace, limit })
            } else {
                RequestPayload::RealityCheck(RealityCheckRequest::Run { session_id: None, namespace, limit })
            };
            let response = client::request(resolve_socket_arg(&args.socket), "cli-reality-check-run", request).await;
            print_reality_check_run(response, args.json, args.tui);
        }
        RealityCheckCommand::Skip(args) => {
            let response = client::request(
                resolve_socket_arg(&args.socket),
                "cli-reality-check-skip",
                RequestPayload::RealityCheck(RealityCheckRequest::Skip),
            )
            .await;
            print_reality_check_skip(response);
        }
        RealityCheckCommand::Snooze(args) => {
            let until = match crate::cli::validate_snooze_until(args.until.as_deref()) {
                Ok(until) => until.map(|date| date.and_hms_opt(0, 0, 0).expect("midnight is valid").and_utc()),
                Err(_) => {
                    eprintln!("invalid date: --until must be YYYY-MM-DD");
                    std::process::exit(1);
                }
            };
            let response = client::request(
                resolve_socket_arg(&args.socket),
                "cli-reality-check-snooze",
                RequestPayload::RealityCheck(RealityCheckRequest::Snooze { until }),
            )
            .await;
            print_reality_check_snooze(response);
        }
    }
}

fn print_reality_check_run(response: anyhow::Result<ResponseEnvelope>, json: bool, tui: bool) -> ! {
    match response {
        Ok(envelope) => match envelope.result {
            ResponseResult::Success(ResponsePayload::RealityCheck(reality_check)) => {
                if json {
                    println!("{}", serde_json::to_string_pretty(&reality_check).expect("reality check serializes"));
                    std::process::exit(0);
                }
                print_reality_check_summary(&reality_check, tui);
            }
            ResponseResult::Error(error) => {
                eprintln!("{}: {}", error.code, error.message);
                std::process::exit(reality_check_error_exit_code(&error.code));
            }
            other => {
                eprintln!("internal_error: daemon returned non-reality-check response: {other:?}");
                std::process::exit(3);
            }
        },
        Err(error) => {
            eprintln!("daemon_unavailable: {error:#}");
            std::process::exit(3);
        }
    }
}

fn print_reality_check_summary(reality_check: &RealityCheckResponse, tui: bool) -> ! {
    match reality_check {
        RealityCheckResponse::Pending { session_id, items, total_scored, .. } => {
            if items.is_empty() {
                println!("No Reality Check items.");
                std::process::exit(1);
            }
            if tui {
                println!("Reality Check routed to TUI panel 8.");
            }
            println!("Reality Check session: {}", session_id.as_deref().unwrap_or("preview"));
            println!("Items: {} of {total_scored}", items.len());
            for item in items {
                println!("- {} [{}] score {:.2}: {}", item.memory_id.as_str(), item.namespace, item.score, item.title);
            }
            std::process::exit(0);
        }
        other => {
            println!("{}", serde_json::to_string_pretty(other).expect("reality check serializes"));
            std::process::exit(0);
        }
    }
}

fn print_reality_check_skip(response: anyhow::Result<ResponseEnvelope>) -> ! {
    match response {
        Ok(envelope) => match envelope.result {
            ResponseResult::Success(ResponsePayload::RealityCheck(RealityCheckResponse::Skipped { skipped_until })) => {
                println!("Reality Check skipped until {skipped_until}");
                std::process::exit(0);
            }
            ResponseResult::Error(error) => {
                eprintln!("{}: {}", error.code, error.message);
                std::process::exit(reality_check_error_exit_code(&error.code));
            }
            other => {
                eprintln!("internal_error: daemon returned non-skip response: {other:?}");
                std::process::exit(2);
            }
        },
        Err(error) => {
            eprintln!("daemon_unavailable: {error:#}");
            std::process::exit(2);
        }
    }
}

fn print_reality_check_snooze(response: anyhow::Result<ResponseEnvelope>) -> ! {
    match response {
        Ok(envelope) => match envelope.result {
            ResponseResult::Success(ResponsePayload::RealityCheck(RealityCheckResponse::Snoozed { snooze_until })) => {
                println!("Reality Check snoozed until {snooze_until}");
                std::process::exit(0);
            }
            ResponseResult::Error(error) => {
                eprintln!("{}: {}", error.code, error.message);
                std::process::exit(reality_check_error_exit_code(&error.code));
            }
            other => {
                eprintln!("internal_error: daemon returned non-snooze response: {other:?}");
                std::process::exit(2);
            }
        },
        Err(error) => {
            eprintln!("daemon_unavailable: {error:#}");
            std::process::exit(2);
        }
    }
}

fn reality_check_error_exit_code(code: &str) -> i32 {
    match code {
        "no_items" => 1,
        "session_abandoned" => 2,
        "invalid_request" => 1,
        _ => 3,
    }
}
