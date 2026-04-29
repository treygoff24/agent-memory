use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use memory_substrate::Substrate;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::watch;

use crate::handlers;
use crate::protocol::{RequestEnvelope, RequestPayload, ResponseEnvelope, ResponsePayload, StatusResponse};

pub use crate::protocol::MAX_FRAME_BYTES;

/// Tunable knobs for the daemon connection layer. Defaults are appropriate
/// for production; tests typically override `idle_frame_timeout` to keep
/// runtime short.
#[derive(Clone, Debug)]
pub struct ServerOptions {
    /// Maximum time a connection may stay silent — i.e., not produce any
    /// new bytes — before it is treated as a dead peer and closed. The clock
    /// resets on every successful read, so a slow-but-live peer is fine.
    pub idle_frame_timeout: Duration,
}

impl Default for ServerOptions {
    fn default() -> Self {
        Self { idle_frame_timeout: Duration::from_secs(300) }
    }
}

/// Run the standalone (substrate-less) daemon forever. Status is the only
/// supported request; everything else returns `not_implemented`.
pub async fn serve(socket_path: impl AsRef<Path>) -> Result<()> {
    // The sender stays alive on this stack frame; since this future runs
    // forever (no caller-driven shutdown), the receiver will simply block on
    // `changed()` for the lifetime of the daemon.
    let (_shutdown_tx, shutdown_rx) = watch::channel(false);
    serve_with_dispatcher(socket_path.as_ref(), Dispatch::Standalone, ServerOptions::default(), shutdown_rx).await
}

/// Run the substrate-backed daemon forever with default options. For test
/// harnesses or supervised lifecycles that need to drive shutdown, use
/// [`serve_substrate_with`].
pub async fn serve_substrate(socket_path: impl AsRef<Path>, substrate: Substrate) -> Result<()> {
    let (_shutdown_tx, shutdown_rx) = watch::channel(false);
    serve_with_dispatcher(
        socket_path.as_ref(),
        Dispatch::Substrate(Arc::new(substrate)),
        ServerOptions::default(),
        shutdown_rx,
    )
    .await
}

/// Run the substrate-backed daemon with caller-supplied options and a
/// shutdown signal.
///
/// When `true` is published on `shutdown` — or the sender is dropped — the
/// accept loop stops taking new connections and any in-flight connection read
/// returns immediately so the per-connection task exits cleanly. In-flight
/// requests that have already been read from the socket are still handled to
/// completion.
pub async fn serve_substrate_with(
    socket_path: impl AsRef<Path>,
    substrate: Substrate,
    options: ServerOptions,
    shutdown: watch::Receiver<bool>,
) -> Result<()> {
    serve_with_dispatcher(socket_path.as_ref(), Dispatch::Substrate(Arc::new(substrate)), options, shutdown).await
}

async fn serve_with_dispatcher(
    socket_path: &Path,
    dispatch: Dispatch,
    options: ServerOptions,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()> {
    remove_stale_socket(socket_path).await?;

    let listener = UnixListener::bind(socket_path).with_context(|| format!("bind socket {}", socket_path.display()))?;

    loop {
        tokio::select! {
            biased;
            _ = shutdown.changed() => return Ok(()),
            accept = listener.accept() => {
                let (stream, _) = accept.context("accept daemon connection")?;
                let dispatch = dispatch.clone();
                let options = options.clone();
                let conn_shutdown = shutdown.clone();
                tokio::spawn(async move {
                    if let Err(error) = handle_connection(stream, dispatch, options, conn_shutdown).await {
                        eprintln!("memoryd connection failed: {error:#}");
                    }
                });
            }
        }
    }
}

#[derive(Clone)]
enum Dispatch {
    Standalone,
    Substrate(Arc<Substrate>),
}

/// Outcome of reading one newline-delimited frame from the socket.
enum ReadFrame {
    /// A complete frame was read.
    Frame(Vec<u8>),
    /// The frame exceeded MAX_FRAME_BYTES; the oversized data was drained to the newline.
    TooLarge,
    /// The peer closed the connection cleanly, or stayed silent past the idle timeout.
    Eof,
}

async fn handle_connection(
    stream: UnixStream,
    dispatch: Dispatch,
    options: ServerOptions,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()> {
    let mut reader = BufReader::new(stream);

    loop {
        let frame = tokio::select! {
            biased;
            _ = shutdown.changed() => return Ok(()),
            result = read_frame(&mut reader, options.idle_frame_timeout) => {
                match result? {
                    ReadFrame::Frame(bytes) => bytes,
                    ReadFrame::TooLarge => {
                        // The frame was too large to decode, so we have no id to echo back.
                        // Use an empty string; clients should correlate on the error code.
                        let response = ResponseEnvelope::error(
                            "",
                            "frame_too_large",
                            format!("request frame exceeded the {MAX_FRAME_BYTES}-byte limit"),
                            false,
                        );
                        let response_line = response.to_json_line().context("serialize frame_too_large error")?;
                        reader.get_mut().write_all(response_line.as_bytes()).await.context("write frame_too_large error")?;
                        continue;
                    }
                    ReadFrame::Eof => return Ok(()),
                }
            }
        };

        let response = match serde_json::from_slice::<RequestEnvelope>(&frame) {
            Ok(request) => handle_request(&dispatch, request).await,
            Err(parse_err) => {
                // Best-effort: extract the `id` field from the raw bytes so the client
                // can correlate the error response even though the envelope is malformed.
                let error_id = extract_id_best_effort(&frame);
                ResponseEnvelope::error(
                    error_id,
                    "invalid_request",
                    format!("request JSON is malformed: {parse_err}"),
                    false,
                )
            }
        };
        let response_line = response.to_json_line().context("serialize response frame")?;
        reader.get_mut().write_all(response_line.as_bytes()).await.context("write response frame")?;
    }
}

/// Read exactly one newline-terminated frame, capping at MAX_FRAME_BYTES.
///
/// If the frame grows beyond the cap the remaining bytes up to (and including)
/// the newline are consumed so the connection stays usable for subsequent
/// requests.
///
/// Each underlying socket read is guarded by `idle_timeout`. The clock resets
/// on every successful read, so a slow peer that trickles bytes is fine; only
/// a peer that goes silent past the timeout window is closed.
async fn read_frame(reader: &mut BufReader<UnixStream>, idle_timeout: Duration) -> Result<ReadFrame> {
    let mut frame = Vec::new();
    let mut oversized = false;

    loop {
        let available = match tokio::time::timeout(idle_timeout, reader.fill_buf()).await {
            Ok(result) => result.context("read request frame")?,
            Err(_elapsed) => return Ok(ReadFrame::Eof),
        };
        if available.is_empty() {
            return Ok(ReadFrame::Eof);
        }

        // Find a newline in the current buffer window.
        let newline_pos = available.iter().position(|byte| *byte == b'\n');

        let (consumed, line_complete) = match newline_pos {
            Some(pos) => (pos + 1, true),
            None => (available.len(), false),
        };

        if !oversized {
            frame.extend_from_slice(&available[..consumed]);
            if frame.len() > MAX_FRAME_BYTES {
                oversized = true;
                frame.clear(); // release memory early; we won't use the bytes
            }
        }

        reader.consume(consumed);

        if line_complete {
            return if oversized { Ok(ReadFrame::TooLarge) } else { Ok(ReadFrame::Frame(frame)) };
        }
    }
}

/// Best-effort extraction of the `"id"` field from a raw JSON bytes slice.
///
/// Returns an empty string when extraction fails; this is acceptable because the
/// client should still display the error message even without a matching id.
fn extract_id_best_effort(bytes: &[u8]) -> String {
    // Parse as a generic JSON value and pull out the top-level "id" string, if present.
    if let Ok(serde_json::Value::Object(map)) = serde_json::from_slice(bytes) {
        if let Some(serde_json::Value::String(id)) = map.get("id") {
            return id.clone();
        }
    }
    String::new()
}

async fn handle_request(dispatch: &Dispatch, request: RequestEnvelope) -> ResponseEnvelope {
    match dispatch {
        Dispatch::Substrate(substrate) => handlers::handle_request(substrate, request).await,
        Dispatch::Standalone => match request.request {
            RequestPayload::Status => ResponseEnvelope::success(request.id, ResponsePayload::Status(healthy_status())),
            _ => ResponseEnvelope::error(
                request.id,
                "not_implemented",
                "request requires a substrate-backed daemon",
                false,
            ),
        },
    }
}

fn healthy_status() -> StatusResponse {
    StatusResponse {
        state: "healthy".to_owned(),
        guidance: "memoryd local daemon is accepting requests; substrate is not attached yet".to_owned(),
    }
}

async fn remove_stale_socket(socket_path: &Path) -> Result<()> {
    match tokio::fs::remove_file(socket_path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("remove stale socket {}", socket_path.display())),
    }
}
