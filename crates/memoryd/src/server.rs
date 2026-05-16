use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use memorum_coordination::spawn_stale_session_cleanup_task;
use memory_substrate::Substrate;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::watch;
use tokio::time::{interval_at, Instant, MissedTickBehavior};

use crate::handlers::{self, HandlerState};
use crate::notifications::config::NotificationConfig;
use crate::notifications::NotificationDispatcher;
use crate::protocol::{
    NotificationEvent, RequestEnvelope, RequestPayload, ResponseEnvelope, ResponsePayload, StatusResponse,
};
use crate::socket::{probe_live_socket, SocketProbe};

pub use crate::protocol::MAX_FRAME_BYTES;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// Tunable knobs for the daemon connection layer. Defaults are appropriate
/// for production; tests typically override `idle_frame_timeout` to keep
/// runtime short.
#[derive(Clone, Debug)]
pub struct ServerOptions {
    /// Maximum time a connection may stay silent — i.e., not produce any
    /// new bytes — before it is treated as a dead peer and closed. The clock
    /// resets on every successful read, so a slow-but-live peer is fine.
    pub idle_frame_timeout: Duration,
    /// Optional supervisor/test hook notified after the accept loop accepts a
    /// connection. Production leaves this unset.
    pub accepted_connection_notify: Option<Arc<tokio::sync::Notify>>,
}

impl Default for ServerOptions {
    fn default() -> Self {
        Self { idle_frame_timeout: Duration::from_secs(300), accepted_connection_notify: None }
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
    let substrate = Arc::new(substrate);
    let state = Arc::new(state_for_substrate(substrate.as_ref())?);
    spawn_notification_dispatcher(&state);
    emit_startup_blocking_conflicts(&substrate, &state);
    spawn_coordination_cleanup_for_state(state.clone(), shutdown_rx.clone());
    fire_reality_check_due_on_startup(&substrate, &state);
    spawn_reality_check_scheduler(substrate.clone(), state.clone(), shutdown_rx.clone());
    serve_with_dispatcher(
        socket_path.as_ref(),
        Dispatch::Substrate { substrate, state },
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
    let substrate = Arc::new(substrate);
    let state = Arc::new(state_for_substrate(substrate.as_ref())?);
    spawn_notification_dispatcher(&state);
    emit_startup_blocking_conflicts(&substrate, &state);
    spawn_coordination_cleanup_for_state(state.clone(), shutdown.clone());
    fire_reality_check_due_on_startup(&substrate, &state);
    spawn_reality_check_scheduler(substrate.clone(), state.clone(), shutdown.clone());
    serve_with_dispatcher(socket_path.as_ref(), Dispatch::Substrate { substrate, state }, options, shutdown).await
}

fn state_for_substrate(substrate: &Substrate) -> Result<HandlerState> {
    let config = crate::coordination_config::load_coordination_config(&substrate.roots().repo)
        .map_err(anyhow::Error::msg)
        .context("load coordination config")?;
    Ok(HandlerState::with_coordination_config(config))
}

fn spawn_notification_dispatcher(state: &HandlerState) {
    let receiver = state.subscribe_notifications();
    let dispatcher = NotificationDispatcher::production(state.passive_notifications(), NotificationConfig::default());
    tokio::spawn(dispatcher.run(receiver));
}

fn emit_startup_blocking_conflicts(substrate: &Substrate, state: &HandlerState) {
    for path in &substrate.startup_reconcile_report().blocking_conflicts {
        state.emit_notification(NotificationEvent::BlockingMergeConflict { path: path.clone() });
    }
}

pub fn spawn_coordination_cleanup_for_state(
    state: Arc<HandlerState>,
    shutdown: watch::Receiver<bool>,
) -> tokio::task::JoinHandle<()> {
    spawn_stale_session_cleanup_task(
        state.presence_registry(),
        state.claim_lock_registry(),
        state.presence_config(),
        shutdown,
    )
}

fn fire_reality_check_due_on_startup(substrate: &Substrate, state: &HandlerState) {
    let daemon_state = crate::state::DaemonState::load(&substrate.roots().runtime);
    state.fire_reality_check_due_if_due(&daemon_state.reality_check, chrono::Utc::now());
}

fn spawn_reality_check_scheduler(
    substrate: Arc<Substrate>,
    state: Arc<HandlerState>,
    mut shutdown: watch::Receiver<bool>,
) {
    tokio::spawn(async move {
        let mut hourly = interval_at(Instant::now() + Duration::from_secs(3600), Duration::from_secs(3600));
        hourly.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = shutdown.changed() => break,
                _ = hourly.tick() => {
                    let daemon_state = crate::state::DaemonState::load(&substrate.roots().runtime);
                    state.fire_reality_check_due_if_due(&daemon_state.reality_check, chrono::Utc::now());
                }
            }
        }
    });
}

async fn serve_with_dispatcher(
    socket_path: &Path,
    dispatch: Dispatch,
    options: ServerOptions,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()> {
    prepare_socket_path(socket_path).await?;

    let listener = UnixListener::bind(socket_path).with_context(|| format!("bind socket {}", socket_path.display()))?;
    harden_socket_permissions(socket_path)?;

    loop {
        tokio::select! {
            biased;
            _ = shutdown.changed() => return Ok(()),
            accept = listener.accept() => {
                let (stream, _) = accept.context("accept daemon connection")?;
                if let Some(notify) = &options.accepted_connection_notify {
                    notify.notify_one();
                }
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

#[cfg(unix)]
fn harden_socket_permissions(socket_path: &Path) -> Result<()> {
    if let Some(parent) = socket_path.parent() {
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
            .with_context(|| format!("chmod owner-only socket parent {}", parent.display()))?;
    }
    std::fs::set_permissions(socket_path, std::fs::Permissions::from_mode(0o600))
        .with_context(|| format!("chmod owner-only socket {}", socket_path.display()))
}

#[cfg(not(unix))]
fn harden_socket_permissions(_socket_path: &Path) -> Result<()> {
    Ok(())
}

#[derive(Clone)]
enum Dispatch {
    Standalone,
    Substrate { substrate: Arc<Substrate>, state: Arc<HandlerState> },
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
        let response_line = response_line_with_frame_cap(response).context("serialize response frame")?;
        reader.get_mut().write_all(response_line.as_bytes()).await.context("write response frame")?;
    }
}

fn response_line_with_frame_cap(response: ResponseEnvelope) -> serde_json::Result<String> {
    let line = response.to_json_line()?;
    if line.len() <= MAX_FRAME_BYTES {
        return Ok(line);
    }

    ResponseEnvelope::error(
        response.id,
        "response_frame_too_large",
        format!("response frame exceeded the {MAX_FRAME_BYTES}-byte limit"),
        false,
    )
    .to_json_line()
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
    extract_id_from_malformed_json(bytes).unwrap_or_default()
}

fn extract_id_from_malformed_json(bytes: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(bytes).ok()?;
    let marker = r#""id""#;
    let marker_start = text.find(marker)?;
    let rest = text[marker_start + marker.len()..].trim_start();
    let raw = rest.strip_prefix(':')?.trim_start().strip_prefix('"')?;
    parse_json_string_prefix(raw)
}

fn parse_json_string_prefix(raw: &str) -> Option<String> {
    let mut value = String::new();
    let mut chars = raw.chars();
    while let Some(character) = chars.next() {
        match character {
            '"' => return Some(value),
            '\\' => match chars.next()? {
                '"' => value.push('"'),
                '\\' => value.push('\\'),
                '/' => value.push('/'),
                'b' => value.push('\u{0008}'),
                'f' => value.push('\u{000c}'),
                'n' => value.push('\n'),
                'r' => value.push('\r'),
                't' => value.push('\t'),
                'u' => {
                    let codepoint = parse_json_codepoint(&mut chars)?;
                    value.push(char::from_u32(codepoint)?);
                }
                _ => return None,
            },
            other => value.push(other),
        }
    }
    None
}

fn parse_json_codepoint(chars: &mut impl Iterator<Item = char>) -> Option<u32> {
    let mut codepoint = 0;
    for _ in 0..4 {
        codepoint = (codepoint << 4) + chars.next()?.to_digit(16)?;
    }
    Some(codepoint)
}

async fn handle_request(dispatch: &Dispatch, request: RequestEnvelope) -> ResponseEnvelope {
    match dispatch {
        Dispatch::Substrate { substrate, state } => {
            handlers::handle_request_with_state(substrate, state, request).await
        }
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
        recall: Default::default(),
        dreams: Default::default(),
        passive_notifications: Default::default(),
    }
}

async fn prepare_socket_path(socket_path: &Path) -> Result<()> {
    if let Some(parent) = socket_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("create socket parent {}", parent.display()))?;
    }

    match probe_live_socket(socket_path) {
        SocketProbe::Live => anyhow::bail!("socket_in_use: live memoryd already owns {}", socket_path.display()),
        SocketProbe::Absent => Ok(()),
        SocketProbe::Stale => match tokio::fs::remove_file(socket_path).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error).with_context(|| format!("remove stale socket {}", socket_path.display())),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::response_line_with_frame_cap;
    use crate::protocol::{ResponseEnvelope, ResponsePayload, ResponseResult};
    use crate::recall::{DeltaResponse, StartupResponse};

    #[test]
    fn oversized_startup_response_is_replaced_with_bounded_protocol_error() {
        let response = ResponseEnvelope::success(
            "req-huge-startup",
            ResponsePayload::Startup(Box::new(StartupResponse {
                session_binding: serde_json::from_value(serde_json::json!({
                    "session_id": "sess",
                    "harness": "codex",
                    "cwd": "/tmp",
                    "namespaces_in_scope": ["me"]
                }))
                .expect("session binding fixture"),
                recall_block: "x".repeat(crate::protocol::MAX_FRAME_BYTES),
                budget_used_tokens: 0,
                recall_explanation: crate::recall::RecallExplanation::empty(3_600),
                guidance: "fixture".to_owned(),
                dream_question_omissions: Default::default(),
            })),
        );

        let line = response_line_with_frame_cap(response).expect("bounded response serializes");
        assert!(line.len() <= crate::protocol::MAX_FRAME_BYTES);

        let decoded = ResponseEnvelope::from_json_line(&line).expect("bounded response decodes");
        match decoded.result {
            ResponseResult::Error(error) => assert_eq!(error.code, "response_frame_too_large"),
            other => panic!("expected bounded error response, got {other:?}"),
        }
    }

    #[test]
    fn empty_delta_response_remains_byte_identical_after_protocol_roundtrip() {
        let response = ResponseEnvelope::success(
            "req-empty-delta",
            ResponsePayload::Delta(DeltaResponse {
                delta_block: "<memory-delta empty=\"true\" />\n".to_owned(),
                budget_used_tokens: 0,
                guidance: "No passive recall delta matched this turn.".to_owned(),
            }),
        );

        let line = response.to_json_line().expect("delta response serializes");
        let decoded = ResponseEnvelope::from_json_line(&line).expect("delta response decodes");
        match decoded.result {
            ResponseResult::Success(ResponsePayload::Delta(delta)) => {
                assert_eq!(delta.delta_block, "<memory-delta empty=\"true\" />\n");
            }
            other => panic!("expected delta response, got {other:?}"),
        }
    }
}
