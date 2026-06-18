use crate::cli::{WebArgs, WebCommand};
use crate::client;
use crate::paths::resolve_socket_arg;
use crate::protocol::{RequestPayload, ResponseEnvelope, ResponsePayload, ResponseResult};

pub async fn run(args: WebArgs) -> anyhow::Result<()> {
    match args.command {
        WebCommand::Enable(args) => {
            let response = client::request(
                resolve_socket_arg(&args.socket),
                "cli-web-enable",
                RequestPayload::WebEnable {
                    port: args.port,
                    socket_path: resolve_socket_arg(&args.socket).to_string_lossy().into_owned(),
                },
            )
            .await;
            print_web_response(response, WebOperation::Enable);
        }
        WebCommand::Disable(args) => {
            let response =
                client::request(resolve_socket_arg(&args.socket), "cli-web-disable", RequestPayload::WebDisable).await;
            print_web_response(response, WebOperation::Disable);
        }
        WebCommand::Status(args) => {
            let response =
                client::request(resolve_socket_arg(&args.socket), "cli-web-status", RequestPayload::WebStatus).await;
            print_web_status(response, args.json);
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum WebOperation {
    Enable,
    Disable,
}

fn print_web_response(response: anyhow::Result<ResponseEnvelope>, operation: WebOperation) -> ! {
    match response {
        Ok(envelope) => match envelope.result {
            ResponseResult::Success(ResponsePayload::WebStatus(status)) => {
                match operation {
                    WebOperation::Enable => {
                        let url =
                            status.launch_url.as_deref().or(status.url.as_deref()).unwrap_or("http://localhost:7137");
                        println!("Web dashboard enabled at {url}");
                    }
                    WebOperation::Disable => println!("Web dashboard disabled"),
                }
                std::process::exit(0);
            }
            ResponseResult::Error(error) => {
                eprintln!("{}: {}", error.code, error.message);
                std::process::exit(web_protocol_exit_code(&error.code, operation));
            }
            other => {
                eprintln!("internal_error: daemon returned non-web response: {other:?}");
                std::process::exit(1);
            }
        },
        Err(error) => {
            eprintln!("daemon_unavailable: {error:#}");
            std::process::exit(match operation {
                WebOperation::Enable => 3,
                WebOperation::Disable => 1,
            });
        }
    }
}

fn print_web_status(response: anyhow::Result<ResponseEnvelope>, json: bool) -> ! {
    match response {
        Ok(envelope) => match envelope.result {
            ResponseResult::Success(ResponsePayload::WebStatus(status)) => {
                if json {
                    println!("{}", serde_json::to_string_pretty(&status).expect("web status serializes"));
                } else if status.running {
                    println!("Web dashboard: running");
                    if let Some(url) = &status.url {
                        println!("URL: {url}");
                    }
                    if let Some(port) = status.port {
                        println!("Port: {port}");
                    }
                    if let Some(uptime) = status.uptime_seconds {
                        println!("Uptime: {uptime}s");
                    }
                    println!("Active connections: {}", status.active_connections);
                } else {
                    println!("Web dashboard: stopped");
                }
                std::process::exit(0);
            }
            ResponseResult::Error(error) => {
                eprintln!("{}: {}", error.code, error.message);
                std::process::exit(1);
            }
            other => {
                eprintln!("internal_error: daemon returned non-web-status response: {other:?}");
                std::process::exit(1);
            }
        },
        Err(error) => {
            eprintln!("daemon_unavailable: {error:#}");
            std::process::exit(1);
        }
    }
}

fn web_protocol_exit_code(code: &str, operation: WebOperation) -> i32 {
    match (operation, code) {
        (WebOperation::Enable, "port_in_use") => 1,
        (WebOperation::Enable, "invalid_request") => 2,
        (WebOperation::Enable, _) => 3,
        (WebOperation::Disable, _) => 1,
    }
}
