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
use std::os::unix::fs::{FileTypeExt, PermissionsExt};

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
    spawn_embedding_worker(substrate.clone(), state.embedding_provider_slot(), shutdown.clone());
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

/// Spawn the background embedding worker.
///
/// Loads the production embedding provider (Qwen3 via fastembed) on a blocking
/// thread so daemon startup is never gated on a first-use model download, then
/// starts the drain loop. If the model cannot load — no weights, no network on
/// first use, unsupported device — the loader retries on a slow backoff while
/// recall degrades to FTS-only; the empty vector table and pending backlog
/// surface in `doctor` rather than crashing the daemon.
fn spawn_embedding_worker(
    substrate: Arc<Substrate>,
    provider_slot: crate::embedding::EmbeddingProviderSlot,
    shutdown: watch::Receiver<bool>,
) {
    const MODEL_LOAD_RETRY_BACKOFF: Duration = Duration::from_secs(300);

    // Operational opt-out: skip the worker (and its first-use model download /
    // load) on constrained hosts or in test/CI daemons that don't exercise
    // vector recall. Recall degrades to FTS-only, surfaced in `doctor`.
    if std::env::var_os("MEMORUM_DISABLE_EMBEDDING_WORKER").is_some() {
        tracing::info!("embedding worker disabled via MEMORUM_DISABLE_EMBEDDING_WORKER");
        return;
    }
    let runtime_root = substrate.roots().runtime.clone();
    tokio::spawn(async move {
        // Let the daemon bind its socket and serve before this task starts the
        // CPU/GPU-heavy first-use model load. Loading immediately would compete
        // with `serve_with_dispatcher`'s bind on the same runtime; a short grace
        // delay keeps daemon startup responsive while costing the first
        // embedding only a couple seconds of additional latency.
        tokio::time::sleep(Duration::from_secs(1)).await;
        let triple = match substrate.active_embedding_triple() {
            Ok(triple) => triple,
            Err(error) => {
                tracing::warn!(%error, "embedding worker not started: cannot read active triple");
                crate::embedding::record_model_load_failure(format!("cannot read active triple: {error}"));
                return;
            }
        };
        if !crate::embedding::is_fastembed_candle_triple(&triple) {
            tracing::warn!(
                provider = %triple.provider,
                supported_provider = crate::embedding::FASTEMBED_CANDLE_PROVIDER,
                "embedding worker not started: active embedding provider is unsupported by this daemon"
            );
            return;
        }

        let mut load_shutdown = shutdown.clone();
        loop {
            if *load_shutdown.borrow() {
                return;
            }
            let load_triple = triple.clone();
            let load_runtime_root = runtime_root.clone();
            let load = tokio::task::spawn_blocking(move || {
                crate::embedding::FastembedProvider::load_for_runtime(&load_runtime_root, load_triple)
            })
            .await;
            let provider: Arc<dyn crate::embedding::EmbeddingProvider> = match load {
                Ok(Ok(provider)) => {
                    crate::embedding::clear_model_load_failure();
                    tracing::info!(
                        model = %triple.model_ref,
                        dimension = triple.dimension,
                        device = provider.device().label(),
                        "embedding worker online"
                    );
                    Arc::new(provider)
                }
                Ok(Err(error)) => {
                    crate::embedding::record_model_load_failure(error.to_string());
                    tracing::warn!(
                        %error,
                        retry_seconds = MODEL_LOAD_RETRY_BACKOFF.as_secs(),
                        "embedding worker model load failed; recall stays FTS-only until retry succeeds"
                    );
                    if sleep_or_shutdown(&mut load_shutdown, MODEL_LOAD_RETRY_BACKOFF).await {
                        return;
                    }
                    continue;
                }
                Err(join_error) => {
                    crate::embedding::record_model_load_failure(format!("model load task panicked: {join_error}"));
                    tracing::warn!(
                        %join_error,
                        retry_seconds = MODEL_LOAD_RETRY_BACKOFF.as_secs(),
                        "embedding worker model load task panicked; retrying"
                    );
                    if sleep_or_shutdown(&mut load_shutdown, MODEL_LOAD_RETRY_BACKOFF).await {
                        return;
                    }
                    continue;
                }
            };
            // Publish the loaded provider so the governance write path can embed
            // contradiction candidates with the same model that populates the vec
            // table. Done before the drain loop starts so similarity is available
            // as soon as the model is up.
            provider_slot.set(Arc::clone(&provider));
            crate::embedding::worker::spawn_embedding_worker(substrate, provider, shutdown);
            return;
        }
    });
}

async fn sleep_or_shutdown(shutdown: &mut watch::Receiver<bool>, duration: Duration) -> bool {
    tokio::select! {
        biased;
        _ = shutdown.changed() => true,
        _ = tokio::time::sleep(duration) => false,
    }
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

    let listener = bind_owner_only_socket(socket_path)?;

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

/// Bind the daemon's Unix socket so it is never momentarily group/world-connectable.
///
/// On unix, `UnixListener::bind` creates the socket node with the process umask,
/// leaving a brief window in which the socket may be group/world-connectable
/// before a follow-up `chmod 0o600` lands. To eliminate that TOCTOU window we
/// bind to a unique sibling temp name, tighten its mode to `0o600` while no
/// well-known path points at it, then atomically `rename` it into the final
/// socket path. Listeners survive `rename(2)`, so the rebind is seamless.
#[cfg(unix)]
fn bind_owner_only_socket(socket_path: &Path) -> Result<UnixListener> {
    let temp_path = owner_only_socket_temp_path(socket_path);
    // A stale temp from a crashed predecessor would make bind fail with EADDRINUSE.
    let _ = std::fs::remove_file(&temp_path);

    let listener = UnixListener::bind(&temp_path).with_context(|| format!("bind socket {}", temp_path.display()))?;

    if let Err(error) = std::fs::set_permissions(&temp_path, std::fs::Permissions::from_mode(0o600))
        .with_context(|| format!("chmod owner-only socket {}", temp_path.display()))
        .and_then(|()| {
            std::fs::rename(&temp_path, socket_path)
                .with_context(|| format!("activate owner-only socket {}", socket_path.display()))
        })
    {
        let _ = std::fs::remove_file(&temp_path);
        return Err(error);
    }

    Ok(listener)
}

#[cfg(unix)]
fn owner_only_socket_temp_path(socket_path: &Path) -> std::path::PathBuf {
    let mut file_name = socket_path.file_name().map(|name| name.to_os_string()).unwrap_or_default();
    file_name.push(format!(".tmp.{}", std::process::id()));
    match socket_path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(file_name),
        _ => std::path::PathBuf::from(file_name),
    }
}

#[cfg(not(unix))]
fn bind_owner_only_socket(socket_path: &Path) -> Result<UnixListener> {
    UnixListener::bind(socket_path).with_context(|| format!("bind socket {}", socket_path.display()))
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
    String::new()
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
        daemon: Some(crate::protocol::DaemonProcessStatus {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            pid: std::process::id(),
            uptime_seconds: None,
        }),
        dashboard_warnings: Vec::new(),
        recall: Default::default(),
        dreams: Default::default(),
        passive_notifications: Default::default(),
        index_stats: None,
        review_queue_counts: None,
        conflicts_count: None,
        peer_sessions: Vec::new(),
        peer_update_count: None,
        compact_dream_status: None,
    }
}

async fn prepare_socket_path(socket_path: &Path) -> Result<()> {
    if let Some(parent) = socket_parent(socket_path) {
        prepare_socket_parent(parent).await?;
    }

    match probe_live_socket(socket_path) {
        SocketProbe::Live => anyhow::bail!("socket_in_use: live memoryd already owns {}", socket_path.display()),
        SocketProbe::Absent => Ok(()),
        SocketProbe::Stale => remove_stale_socket(socket_path).await,
    }
}

fn socket_parent(socket_path: &Path) -> Option<&Path> {
    socket_path.parent().filter(|parent| !parent.as_os_str().is_empty())
}

async fn prepare_socket_parent(parent: &Path) -> Result<()> {
    match tokio::fs::metadata(parent).await {
        Ok(metadata) if metadata.is_dir() => {
            warn_on_loose_socket_parent(parent, &metadata);
            Ok(())
        }
        Ok(_) => anyhow::bail!("socket_parent_not_directory: {}", parent.display()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("create socket parent {}", parent.display()))?;
            harden_created_socket_parent(parent)
        }
        Err(error) => Err(error).with_context(|| format!("inspect socket parent {}", parent.display())),
    }
}

/// Owner-only mode for parent directories the daemon creates itself.
///
/// Pre-existing directories are deliberately NOT repaired: the socket path is
/// caller-supplied, so its parent can be an arbitrary shared directory (`/tmp`
/// being the catastrophic case) that the daemon must never chmod. The pinned
/// contract is `server_does_not_chmod_existing_socket_parent_directory`. The
/// socket node itself is always `0o600` regardless of where it lives; a loose
/// pre-existing parent only weakens deny-by-traversal defense in depth, which
/// `warn_on_loose_socket_parent` surfaces to the operator instead.
#[cfg(unix)]
fn harden_created_socket_parent(parent: &Path) -> Result<()> {
    std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
        .with_context(|| format!("chmod owner-only newly-created socket parent {}", parent.display()))
}

#[cfg(not(unix))]
fn harden_created_socket_parent(_parent: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn warn_on_loose_socket_parent(parent: &Path, metadata: &std::fs::Metadata) {
    use std::os::unix::fs::PermissionsExt;
    if metadata.permissions().mode() & 0o077 != 0 {
        tracing::warn!(
            parent = %parent.display(),
            "socket parent directory is group/world-accessible; the socket itself is 0o600, but a dedicated 0o700 runtime dir is recommended"
        );
    }
}

#[cfg(not(unix))]
fn warn_on_loose_socket_parent(_parent: &Path, _metadata: &std::fs::Metadata) {}

async fn remove_stale_socket(socket_path: &Path) -> Result<()> {
    match tokio::fs::symlink_metadata(socket_path).await {
        Ok(metadata) if stale_path_is_socket(&metadata) => match tokio::fs::remove_file(socket_path).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error).with_context(|| format!("remove stale socket {}", socket_path.display())),
        },
        Ok(_) => anyhow::bail!("refusing to remove non-socket path passed as daemon socket: {}", socket_path.display()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("inspect stale socket path {}", socket_path.display())),
    }
}

#[cfg(unix)]
fn stale_path_is_socket(metadata: &std::fs::Metadata) -> bool {
    metadata.file_type().is_socket()
}

#[cfg(not(unix))]
fn stale_path_is_socket(_metadata: &std::fs::Metadata) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::response_line_with_frame_cap;
    use crate::protocol::{ResponseEnvelope, ResponsePayload, ResponseResult};
    use crate::recall::{DeltaResponse, StartupResponse};

    #[cfg(unix)]
    #[tokio::test]
    async fn bind_owner_only_socket_creates_socket_with_0o600_and_no_temp_residue() {
        use std::os::unix::fs::{FileTypeExt, PermissionsExt};

        let dir = tempfile::tempdir().expect("tempdir");
        let socket_path = dir.path().join("memoryd.sock");

        let _listener = super::bind_owner_only_socket(&socket_path).expect("bind owner-only socket");

        let metadata = std::fs::symlink_metadata(&socket_path).expect("socket metadata");
        assert!(metadata.file_type().is_socket(), "final path is a socket node");
        assert_eq!(
            metadata.permissions().mode() & 0o777,
            0o600,
            "socket is owner-only the instant it appears at its final path"
        );

        // The bind/chmod/rename dance must leave no group/world-connectable temp behind.
        let temp_path = super::owner_only_socket_temp_path(&socket_path);
        assert!(!temp_path.exists(), "temp socket residue removed after rename");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn prepare_socket_parent_does_not_chmod_a_preexisting_loose_dir() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let parent = dir.path().join("runtime");
        std::fs::create_dir(&parent).expect("create runtime dir");
        // A caller-supplied socket path can point into a shared dir the daemon
        // does not own (the `/tmp` case). prepare must warn but never chmod it —
        // mirrors server_does_not_chmod_existing_socket_parent_directory.
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o755)).expect("loosen perms");

        super::prepare_socket_parent(&parent).await.expect("prepare existing parent");

        let mode = std::fs::metadata(&parent).expect("parent metadata").permissions().mode() & 0o777;
        assert_eq!(mode, 0o755, "pre-existing parent mode is left untouched");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn prepare_socket_parent_hardens_a_dir_it_creates_itself() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let parent = dir.path().join("runtime");
        // Not pre-created: prepare_socket_parent creates it and owns the mode.
        super::prepare_socket_parent(&parent).await.expect("create and prepare parent");

        let mode = std::fs::metadata(&parent).expect("parent metadata").permissions().mode() & 0o777;
        assert_eq!(mode, 0o700, "a daemon-created parent is owner-only");
    }

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
                vector_recall_degraded: None,
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
