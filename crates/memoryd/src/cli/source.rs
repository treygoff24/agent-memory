use crate::cli::exit::EXIT_INVALID_INPUT;
use crate::cli::output::{emit_and_exit, emit_client_error_and_exit, emit_transport_error_and_exit};
use crate::cli::{SourceArgs, SourceCaptureArgs, SourceCaptureCliMode, SourceCommand};
use crate::client;
use crate::paths::resolve_socket_arg;
use crate::protocol::{CaptureSourceMode, RequestPayload, SourceCapturePayload};

pub async fn run(args: SourceArgs) -> anyhow::Result<()> {
    match args.command {
        SourceCommand::Capture(capture) => {
            let socket = resolve_socket_arg(&capture.socket);
            let request = match source_capture_payload(capture) {
                Ok(request) => request,
                Err(error) => emit_client_error_and_exit(
                    "invalid_request",
                    error.to_string(),
                    EXIT_INVALID_INPUT,
                    Some(
                        "provide exactly one of --url (with --mode http-static) or --file (with a local mode)"
                            .to_string(),
                    ),
                ),
            };
            match client::request(&socket, "cli-source-capture", RequestPayload::CaptureSource(request)).await {
                Ok(response) => emit_and_exit(response),
                Err(error) => emit_transport_error_and_exit(error, &socket),
            }
        }
    }
}

fn source_capture_payload(args: SourceCaptureArgs) -> anyhow::Result<SourceCapturePayload> {
    let mode = match args.mode {
        SourceCaptureCliMode::HttpStatic => CaptureSourceMode::HttpStatic,
        SourceCaptureCliMode::LocalArtifact => CaptureSourceMode::LocalArtifact,
        SourceCaptureCliMode::PdfText => CaptureSourceMode::PdfText,
        SourceCaptureCliMode::BrowserRendered => CaptureSourceMode::BrowserRendered,
        SourceCaptureCliMode::Screenshot => CaptureSourceMode::Screenshot,
        SourceCaptureCliMode::Authenticated => CaptureSourceMode::Authenticated,
    };
    let source = match (&args.url, &args.file) {
        (Some(url), None) => url.clone(),
        (None, Some(path)) => path.display().to_string(),
        (Some(_), Some(_)) => anyhow::bail!("provide exactly one of --url or --file"),
        (None, None) => anyhow::bail!("provide exactly one of --url or --file"),
    };
    if args.file.is_some() && mode == CaptureSourceMode::HttpStatic {
        anyhow::bail!("--file requires --mode local-artifact or another explicit local capture mode");
    }
    if args.url.is_some() && mode != CaptureSourceMode::HttpStatic {
        anyhow::bail!("--url only supports --mode http-static");
    }
    Ok(SourceCapturePayload { source, mode, excerpts: args.excerpts, note: args.note, local_path: args.file })
}
