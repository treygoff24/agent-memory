use std::path::Path;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use memorum_coordination::spawn_stale_session_cleanup_task;
use memory_substrate::git::{commit_substrate_writes, count_substrate_write_changes, CommitOutcome};
use memory_substrate::Substrate;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::watch;
use tokio::time::{interval_at, Instant, MissedTickBehavior};

use crate::embedding::EmbeddingProvider;
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
    let socket_path = socket_path.as_ref().to_path_buf();
    let state = Arc::new(state_for_substrate(&substrate)?);
    let substrate = Arc::new(substrate);
    spawn_notification_dispatcher(&state);
    for (proposal_id, error) in crate::dream::merge::reconcile_applying(&substrate, Some(state.as_ref())).await {
        // Reconcile left the proposal in a non-quarantined failure state. Try to
        // quarantine it now and emit a notification; only if even the quarantine
        // write fails do we log and continue serving.
        let store = crate::dream::merge::MergeProposalStore::new(&substrate.roots().runtime);
        match store.load(&proposal_id) {
            Ok(proposal) if proposal.status == crate::dream::merge::MergeProposalStatus::Quarantined => {
                // `reconcile_applying` already emitted a notification for this
                // quarantined proposal; just make sure it is recorded.
                tracing::error!(%proposal_id, %error, "merge proposal startup reconciliation failed (quarantined)");
            }
            Ok(mut proposal) => {
                proposal.status = crate::dream::merge::MergeProposalStatus::Quarantined;
                if let Err(save_error) = store.save(&proposal) {
                    tracing::error!(%proposal_id, %error, %save_error, "merge proposal startup reconciliation failed and quarantine write failed");
                } else {
                    state.emit_notification(crate::protocol::NotificationEvent::OperatorActionRequired {
                        message: format!("merge proposal {proposal_id} is quarantined: {error}"),
                    });
                }
            }
            Err(load_error) => {
                tracing::error!(%proposal_id, %error, %load_error, "merge proposal startup reconciliation failed and could not load proposal");
            }
        }
    }
    emit_startup_reconcile_notifications(&substrate, &state);
    spawn_coordination_cleanup_for_state(state.clone(), shutdown.clone());
    fire_reality_check_due_on_startup(&substrate, &state);
    spawn_reality_check_scheduler(substrate.clone(), state.clone(), shutdown.clone());
    crate::harvest::spawn_harvest_scheduler(
        substrate.roots().runtime.clone(),
        substrate.roots().repo.clone(),
        socket_path.clone(),
        shutdown.clone(),
    );
    spawn_embedding_worker(substrate.clone(), state.embedding_provider_slot(), shutdown.clone());
    // The commit worker runs until the server loop returns. Give it a dedicated
    // shutdown channel we own rather than the global one: the global `shutdown` sender
    // lives in the signal task and only fires on SIGINT/SIGTERM, so if
    // `serve_with_dispatcher` returns early with a socket error, joining on the global
    // channel would block forever. Signalling here covers the graceful AND error paths.
    let (worker_shutdown_tx, worker_shutdown_rx) = watch::channel(false);
    let commit_worker = spawn_substrate_commit_worker(substrate.clone(), worker_shutdown_rx);
    let result = serve_with_dispatcher(&socket_path, Dispatch::Substrate { substrate, state }, options, shutdown).await;
    // Tell the worker to drain-and-exit (it runs a final `flush_substrate_commits` on
    // its way out), then join so that last commit lands before the process exits.
    // `JoinHandle::join` blocks, so hand it to the blocking pool; a worker panic is
    // ignored — durability still rests on disk + index + event log (I-F1.3).
    let _ = worker_shutdown_tx.send(true);
    let _ = tokio::task::spawn_blocking(move || commit_worker.join()).await;
    result
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

fn emit_startup_reconcile_notifications(substrate: &Substrate, state: &HandlerState) {
    let report = substrate.startup_reconcile_report();
    if report.recovery_required {
        state.emit_notification(NotificationEvent::OperatorActionRequired {
            message: "Startup recovery is required: a crash marker or in-progress git merge was found; resolve it manually before relying on sync.".to_string(),
        });
    }
    for path in &report.blocking_conflicts {
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

/// Idle-unload window for the API embedding lane: warm by default.
///
/// Dormancy exists to reclaim the multi-GB footprint of a local model; the API
/// provider holds an HTTP client and a key handle, so unloading it saves ~KB
/// while costing a degraded (FTS-only) prompt after every idle window. An
/// explicitly set `MEMORUM_EMBED_IDLE_UNLOAD_SECS` still wins (`0` = never
/// unload, same as the local-lane contract).
fn api_lane_idle_window(env_value: Option<String>) -> crate::embedding::EmbeddingIdleWindow {
    use std::time::Duration;
    match env_value {
        Some(raw) => match raw.parse::<u64>() {
            Ok(0) => crate::embedding::EmbeddingIdleWindow::from_duration(None, "env"),
            Ok(secs) => crate::embedding::EmbeddingIdleWindow::from_duration(Some(Duration::from_secs(secs)), "env"),
            Err(error) => {
                tracing::warn!(value = %raw, %error, "invalid MEMORUM_EMBED_IDLE_UNLOAD_SECS; API lane stays warm");
                crate::embedding::EmbeddingIdleWindow::from_duration(None, "api_lane_default")
            }
        },
        None => crate::embedding::EmbeddingIdleWindow::from_duration(None, "api_lane_default"),
    }
}

/// Spawn the background embedding worker.
///
/// The provider lifecycle is configured here but starts dormant: the real model
/// loads only after demand (queued embedding jobs or a degraded vector recall
/// request). If the model cannot load — no weights, no network on first use,
/// unsupported device — the lifecycle keeps the existing slow retry backoff
/// while recall degrades to FTS-only; status/doctor expose the state.
/// Exception: the API lane eager-loads and stays warm (see
/// [`api_lane_idle_window`]) — its "load" is a key read + HTTP client build.
fn spawn_embedding_worker(
    substrate: Arc<Substrate>,
    provider_slot: crate::embedding::EmbeddingProviderSlot,
    shutdown: watch::Receiver<bool>,
) {
    // Operational opt-out: skip the worker (and its first-use model download /
    // load) on constrained hosts or in test/CI daemons that don't exercise
    // vector recall. Recall degrades to FTS-only, surfaced in `doctor`.
    if std::env::var_os("MEMORUM_DISABLE_EMBEDDING_WORKER").is_some() {
        tracing::info!("embedding worker disabled via MEMORUM_DISABLE_EMBEDDING_WORKER");
        provider_slot.mark_failed("embedding worker disabled via MEMORUM_DISABLE_EMBEDDING_WORKER");
        return;
    }
    let runtime_root = substrate.roots().runtime.clone();
    let triple = match substrate.active_embedding_triple() {
        Ok(triple) => triple,
        Err(error) => {
            tracing::warn!(%error, "embedding worker not started: cannot read active triple");
            provider_slot.mark_failed(format!("cannot read active triple: {error}"));
            return;
        }
    };
    if crate::embedding::is_fastembed_candle_triple(&triple) {
        let idle_window = crate::embedding::EmbeddingIdleWindow::from_env();
        provider_slot.configure_loader(triple.clone(), idle_window, move || {
            let provider = crate::embedding::FastembedProvider::load_for_runtime(&runtime_root, triple.clone())?;
            tracing::info!(
                model = %provider.triple().model_ref,
                dimension = provider.triple().dimension,
                device = provider.device().label(),
                "embedding worker model loaded"
            );
            Ok(Arc::new(provider))
        });
    } else if crate::embedding::is_gemini_api_triple(&triple) {
        // Consent gate: the CLI ceremony records `api_embedding_consent: true` in
        // config.yaml; an API triple that arrived any other way (hand edit, merge,
        // clone of a repo missing the flag) must not start sending plaintext.
        if !memory_substrate::config::load_api_embedding_consent(&substrate.roots().repo) {
            tracing::warn!(
                provider = %triple.provider,
                "embedding worker not started: API lane active without recorded consent"
            );
            provider_slot.mark_failed(
                "API embedding lane is active but api_embedding_consent is not recorded in config.yaml; \
                 run `memoryd config embedding-lane --lane gemini-api` to consent, or switch back to the local lane",
            );
            return;
        }
        // The API provider is an HTTP client + key handle (~KB), not a local
        // model (~GB), so the dormancy machinery buys nothing and its cost is
        // real: every idle-unload makes the next prompt's recall degrade to
        // FTS-only while the slot reloads. Stay warm by default; the env knob
        // still forces a window for anyone who explicitly sets one.
        let idle_window = api_lane_idle_window(std::env::var("MEMORUM_EMBED_IDLE_UNLOAD_SECS").ok());
        provider_slot.configure_loader(triple.clone(), idle_window, move || {
            let provider = crate::embedding::ApiEmbeddingProvider::load_for_runtime(&runtime_root, triple.clone())?;
            tracing::info!(
                model = %provider.triple().model_ref,
                dimension = provider.triple().dimension,
                provider = %provider.triple().provider,
                "embedding worker API provider loaded"
            );
            Ok(Arc::new(provider))
        });
        // Eager-load so the first prompt after daemon start gets vector
        // recall instead of a Dormant marker; loading is a key read + client
        // build, not a model load.
        provider_slot.ensure_loaded_in_background();
    } else {
        tracing::warn!(
            provider = %triple.provider,
            supported_local_provider = crate::embedding::FASTEMBED_CANDLE_PROVIDER,
            supported_api_provider = crate::embedding::GEMINI_API_PROVIDER,
            "embedding worker not started: active embedding provider is unsupported by this daemon"
        );
        provider_slot.mark_failed(format!(
            "active embedding provider `{}` is unsupported by this daemon; expected `{}` or `{}`",
            triple.provider,
            crate::embedding::FASTEMBED_CANDLE_PROVIDER,
            crate::embedding::GEMINI_API_PROVIDER
        ));
        return;
    }
    crate::embedding::worker::spawn_embedding_worker(substrate, provider_slot, shutdown);
}

fn spawn_substrate_commit_worker(
    substrate: Arc<Substrate>,
    mut shutdown: watch::Receiver<bool>,
) -> thread::JoinHandle<()> {
    let repo = substrate.roots().repo.clone();
    let runtime = substrate.roots().runtime.clone();
    let debounce = substrate_commit_debounce(substrate.as_ref());
    thread::spawn(move || {
        loop {
            if wait_or_shutdown(&mut shutdown, debounce) {
                flush_substrate_commits(&repo, &runtime);
                return;
            }

            // Do not use `Substrate::watch()`: daemon-authored writes are
            // self-suppressed by watcher/subscription.rs, and those are exactly
            // the writes this worker must commit.
            //
            // `count_substrate_write_changes` runs OUTSIDE the git lock (the lock is
            // taken inside `flush_substrate_commit_count`), so a write landing between
            // the count and the commit only makes the `<n>` in the commit message
            // stale — advisory text, never a gate on what is staged.
            match count_substrate_write_changes(&repo) {
                Ok(0) => {}
                Ok(write_count) => flush_substrate_commit_count(&repo, &runtime, write_count),
                Err(error) => tracing::warn!(%error, "substrate commit worker status poll failed"),
            }
        }
    })
}

fn substrate_commit_debounce(substrate: &Substrate) -> Duration {
    let debounce_ms = memory_substrate::config::load_config(&substrate.roots().repo, &substrate.roots().runtime, None)
        .map(|config| config.synced.substrate.commit_debounce_ms)
        .unwrap_or_else(|error| {
            tracing::warn!(%error, "using default substrate commit debounce");
            2000
        });
    Duration::from_millis(u64::from(debounce_ms.max(10)))
}

fn wait_or_shutdown(shutdown: &mut watch::Receiver<bool>, duration: Duration) -> bool {
    let deadline = std::time::Instant::now() + duration;
    loop {
        if shutdown_requested(shutdown) {
            return true;
        }
        let now = std::time::Instant::now();
        if now >= deadline {
            return false;
        }
        thread::sleep((deadline - now).min(Duration::from_millis(50)));
    }
}

fn shutdown_requested(shutdown: &mut watch::Receiver<bool>) -> bool {
    if *shutdown.borrow() {
        return true;
    }
    match shutdown.has_changed() {
        Ok(true) => *shutdown.borrow_and_update(),
        Ok(false) => false,
        Err(_) => true,
    }
}

fn flush_substrate_commits(repo: &Path, runtime: &Path) {
    match count_substrate_write_changes(repo) {
        Ok(0) => {}
        Ok(write_count) => flush_substrate_commit_count(repo, runtime, write_count),
        Err(error) => tracing::warn!(%error, "substrate commit worker final status poll failed"),
    }
}

fn flush_substrate_commit_count(repo: &Path, runtime: &Path, write_count: usize) {
    let lock = match crate::substrate_git_lock::acquire_substrate_git_lock(runtime) {
        Ok(lock) => lock,
        Err(error) => {
            tracing::warn!(%error, "substrate commit worker could not acquire git lock");
            return;
        }
    };
    match commit_substrate_writes(repo, write_count) {
        Ok(CommitOutcome::Committed { sha }) => tracing::info!(%sha, write_count, "committed substrate writes"),
        Ok(CommitOutcome::NoChanges) => {}
        Err(error) => tracing::warn!(%error, "substrate commit worker commit failed; will retry"),
    }
    drop(lock);
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

/// Bind the daemon's Unix socket and make it owner-only.
///
/// Binding the final path directly preserves Unix socket exclusivity: if another
/// daemon wins the path between stale-socket cleanup and bind, this bind fails
/// with `EADDRINUSE` instead of replacing that live socket — so no temporary
/// socket or replace-on-rename activation race is introduced.
///
/// The socket node is chmodded to `0o600` immediately after bind rather than
/// relying on a restrictive process umask. `serve_substrate_with` spawns
/// background tasks (notification dispatcher, embedding worker, schedulers)
/// before this bind runs, so the daemon is already multi-threaded at bind time;
/// a process-global umask window could leak its restrictive mode onto unrelated
/// files those tasks create. A per-node `set_permissions` has no such global
/// side effect. The sub-millisecond bind-then-chmod window is itself covered for
/// daemon-created parents by the `0o700` parent directory (see
/// `harden_created_socket_parent`); `warn_on_loose_socket_parent` surfaces the
/// residual exposure when the operator supplies a loose pre-existing parent.
#[cfg(unix)]
fn bind_owner_only_socket(socket_path: &Path) -> Result<UnixListener> {
    let listener = UnixListener::bind(socket_path).with_context(|| format!("bind socket {}", socket_path.display()))?;
    std::fs::set_permissions(socket_path, std::fs::Permissions::from_mode(0o600))
        .with_context(|| format!("chmod owner-only socket {}", socket_path.display()))?;
    Ok(listener)
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
        ..StatusResponse::default()
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
    use serial_test::serial;

    #[test]
    fn api_lane_idle_window_defaults_to_always_warm() {
        assert_eq!(super::api_lane_idle_window(None).seconds(), None);
        assert_eq!(super::api_lane_idle_window(None).source(), "api_lane_default");
        // Explicit env override still wins, with the local-lane semantics.
        assert_eq!(super::api_lane_idle_window(Some("300".to_string())).seconds(), Some(300));
        assert_eq!(super::api_lane_idle_window(Some("0".to_string())).seconds(), None);
        // Garbage input fails toward warm, never toward a surprise unload.
        assert_eq!(super::api_lane_idle_window(Some("banana".to_string())).seconds(), None);
    }

    async fn substrate_with_active_embedding(
        triple: memory_substrate::EmbeddingTriple,
        device_id: &str,
    ) -> TestSubstrate {
        let temp = tempfile::tempdir().expect("tempdir");
        let repo = temp.path().join("repo");
        let runtime = temp.path().join("runtime");
        std::fs::create_dir_all(&repo).expect("repo dir");
        std::fs::write(
            repo.join("config.yaml"),
            format!(
                "schema_version: 1\nactive_embedding:\n  provider: {}\n  model_ref: {}\n  dimension: {}\n",
                triple.provider, triple.model_ref, triple.dimension
            ),
        )
        .expect("write config");

        let substrate = memory_substrate::Substrate::init(
            memory_substrate::Roots::new(&repo, &runtime),
            memory_substrate::InitOptions { force_unsafe_durability: true, device_id: Some(device_id.to_string()) },
        )
        .await
        .expect("substrate init");

        TestSubstrate { _temp: temp, substrate: std::sync::Arc::new(substrate) }
    }

    struct TestSubstrate {
        _temp: tempfile::TempDir,
        substrate: std::sync::Arc<memory_substrate::Substrate>,
    }

    struct EnvVarGuard {
        name: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvVarGuard {
        fn remove(name: &'static str) -> Self {
            let previous = std::env::var_os(name);
            std::env::remove_var(name);
            Self { name, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var(self.name, value),
                None => std::env::remove_var(self.name),
            }
        }
    }

    #[tokio::test]
    #[serial]
    async fn gemini_api_embedding_worker_missing_key_marks_slot_failed() {
        let _api_key = EnvVarGuard::remove("MEMORUM_GEMINI_API_KEY");
        let _disable_worker = EnvVarGuard::remove("MEMORUM_DISABLE_EMBEDDING_WORKER");
        let fixture = substrate_with_active_embedding(
            memory_substrate::EmbeddingTriple {
                provider: crate::embedding::GEMINI_API_PROVIDER.to_string(),
                model_ref: "gemini-embedding-2".to_string(),
                dimension: 768,
            },
            "dev_servergemini",
        )
        .await;
        memory_substrate::config::record_api_embedding_consent(&fixture.substrate.roots().repo)
            .expect("record consent");
        let slot = crate::embedding::EmbeddingProviderSlot::empty();
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        super::spawn_embedding_worker(fixture.substrate.clone(), slot.clone(), shutdown_rx);

        assert!(slot.has_loader_configured(), "gemini-api should configure a provider loader");
        let load = slot.ensure_loaded().await;
        let _ = shutdown_tx.send(true);

        assert!(load.is_err(), "missing Gemini credentials should fail the provider load cleanly");
        let snapshot = slot.snapshot();
        assert_eq!(snapshot.state, "failed");
        assert!(
            snapshot.last_error.as_deref().is_some_and(|error| error.contains("Gemini API key not found")),
            "missing-key failure should be recorded on the lifecycle slot: {snapshot:?}"
        );
    }

    #[tokio::test]
    #[serial]
    async fn gemini_api_embedding_worker_without_consent_refuses_to_start() {
        let _api_key = EnvVarGuard::remove("MEMORUM_GEMINI_API_KEY");
        let _disable_worker = EnvVarGuard::remove("MEMORUM_DISABLE_EMBEDDING_WORKER");
        let fixture = substrate_with_active_embedding(
            memory_substrate::EmbeddingTriple {
                provider: crate::embedding::GEMINI_API_PROVIDER.to_string(),
                model_ref: "gemini-embedding-2".to_string(),
                dimension: 768,
            },
            "dev_servergeminiconsent",
        )
        .await;
        // No consent recorded: an API triple that arrived via hand edit or merge
        // must not configure a provider loader at all.
        let slot = crate::embedding::EmbeddingProviderSlot::empty();
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        super::spawn_embedding_worker(fixture.substrate.clone(), slot.clone(), shutdown_rx);
        let _ = shutdown_tx.send(true);

        assert!(!slot.has_loader_configured(), "consent-less API lane must not configure a loader");
        let snapshot = slot.snapshot();
        assert_eq!(snapshot.state, "failed");
        assert!(
            snapshot.last_error.as_deref().is_some_and(|error| error.contains("api_embedding_consent")),
            "consent refusal should be recorded on the lifecycle slot: {snapshot:?}"
        );
    }

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

        assert_eq!(std::fs::read_dir(dir.path()).expect("list socket dir").count(), 1, "only final socket is created");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn bind_owner_only_socket_refuses_to_replace_existing_live_socket() {
        use std::os::unix::fs::FileTypeExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let socket_path = dir.path().join("memoryd.sock");
        let existing = tokio::net::UnixListener::bind(&socket_path).expect("existing daemon socket binds");

        let error = super::bind_owner_only_socket(&socket_path).expect_err("second bind must fail");
        let error_text = format!("{error:#}");
        assert!(error_text.contains("bind socket"), "{error_text}");
        let metadata = std::fs::symlink_metadata(&socket_path).expect("socket still exists");
        assert!(metadata.file_type().is_socket(), "existing live socket remains in place");

        let _connection =
            tokio::net::UnixStream::connect(&socket_path).await.expect("existing listener still reachable");
        drop(existing);
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
