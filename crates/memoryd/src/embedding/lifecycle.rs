use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use memory_substrate::EmbeddingTriple;
use tokio::sync::Notify;

use super::{EmbeddingError, EmbeddingProvider};

const DEFAULT_IDLE_UNLOAD_SECS: u64 = 900;
pub const MODEL_LOAD_RETRY_BACKOFF: Duration = Duration::from_secs(300);

type ProviderLoader = Arc<dyn Fn() -> Result<Arc<dyn EmbeddingProvider>, EmbeddingError> + Send + Sync>;

/// Effective idle-unload setting read once at daemon startup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmbeddingIdleWindow {
    duration: Option<Duration>,
    source: &'static str,
}

impl EmbeddingIdleWindow {
    pub fn from_env() -> Self {
        match std::env::var("MEMORUM_EMBED_IDLE_UNLOAD_SECS") {
            Ok(raw) => match raw.parse::<u64>() {
                Ok(0) => Self { duration: None, source: "env" },
                Ok(secs) => Self { duration: Some(Duration::from_secs(secs)), source: "env" },
                Err(error) => {
                    tracing::warn!(
                        value = %raw,
                        %error,
                        default_secs = DEFAULT_IDLE_UNLOAD_SECS,
                        "invalid MEMORUM_EMBED_IDLE_UNLOAD_SECS; using default"
                    );
                    Self {
                        duration: Some(Duration::from_secs(DEFAULT_IDLE_UNLOAD_SECS)),
                        source: "env_invalid_default",
                    }
                }
            },
            Err(_) => Self { duration: Some(Duration::from_secs(DEFAULT_IDLE_UNLOAD_SECS)), source: "default" },
        }
    }

    pub fn from_duration(duration: Option<Duration>, source: &'static str) -> Self {
        Self { duration, source }
    }

    pub fn seconds(&self) -> Option<u64> {
        self.duration.map(|duration| duration.as_secs())
    }

    pub fn source(&self) -> &'static str {
        self.source
    }
}

impl Default for EmbeddingIdleWindow {
    fn default() -> Self {
        Self { duration: Some(Duration::from_secs(DEFAULT_IDLE_UNLOAD_SECS)), source: "default" }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmbeddingLifecycleSnapshot {
    pub state: String,
    pub load_count: u64,
    pub unload_count: u64,
    pub idle_unload_secs: Option<u64>,
    pub idle_unload_source: &'static str,
    pub in_flight: usize,
    pub last_error: Option<String>,
}

pub enum EmbeddingProviderAcquire {
    Active(ProviderGuard),
    Dormant,
    Loading,
    Failed { last_error: Option<String> },
}

/// Lifecycle owner for the embedding provider.
///
/// The historical name is kept because handler tests already use
/// `embedding_provider_slot()`. The implementation is no longer a passive slot:
/// it is the sole long-lived owner, and callers get short-lived guards.
#[derive(Clone)]
pub struct EmbeddingProviderSlot {
    inner: Arc<Inner>,
}

struct Inner {
    state: Mutex<State>,
    notify: Notify,
}

struct State {
    phase: Phase,
    provider: Option<Arc<dyn EmbeddingProvider>>,
    loader: Option<ProviderLoader>,
    active_triple: Option<EmbeddingTriple>,
    idle_window: EmbeddingIdleWindow,
    in_flight: usize,
    load_count: u64,
    unload_count: u64,
    last_activity: Instant,
    last_error: Option<String>,
    last_failure: Option<Instant>,
    /// Backoff duration before a failed load can be retried. Defaults to
    /// `MODEL_LOAD_RETRY_BACKOFF` but is injectable for tests via
    /// [`EmbeddingProviderSlot::set_load_retry_backoff_for_tests`].
    load_retry_backoff: Duration,
    /// Whether an idle-check timer task is currently armed. Ensures at most
    /// one parked sleeper per slot regardless of acquire/load frequency,
    /// preventing unbounded no-op timer accumulation under steady traffic.
    idle_check_armed: bool,
    /// Monotonic generation counter bumped whenever the lifecycle is
    /// reconfigured (`configure_loader`, `set`) or transitions Active→Dormant
    /// via idle unload. Spawned tasks (load completions, idle timers) capture
    /// the generation at dispatch time and no-op if it no longer matches,
    /// so a stale task from a previous configuration cannot clobber newer
    /// state or unload a freshly-reloaded provider.
    generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Dormant,
    Loading,
    Active,
    Failed,
}

impl Default for EmbeddingProviderSlot {
    fn default() -> Self {
        Self {
            inner: Arc::new(Inner {
                state: Mutex::new(State {
                    phase: Phase::Dormant,
                    provider: None,
                    loader: None,
                    active_triple: None,
                    idle_window: EmbeddingIdleWindow::default(),
                    in_flight: 0,
                    load_count: 0,
                    unload_count: 0,
                    last_activity: Instant::now(),
                    last_error: None,
                    last_failure: None,
                    load_retry_backoff: MODEL_LOAD_RETRY_BACKOFF,
                    idle_check_armed: false,
                    generation: 0,
                }),
                notify: Notify::new(),
            }),
        }
    }
}

impl EmbeddingProviderSlot {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn configure_loader(
        &self,
        active_triple: EmbeddingTriple,
        idle_window: EmbeddingIdleWindow,
        loader: impl Fn() -> Result<Arc<dyn EmbeddingProvider>, EmbeddingError> + Send + Sync + 'static,
    ) {
        match self.inner.state.lock() {
            Ok(mut state) => {
                state.phase = Phase::Dormant;
                state.provider = None;
                state.loader = Some(Arc::new(loader));
                state.active_triple = Some(active_triple);
                state.idle_window = idle_window;
                state.in_flight = 0;
                state.last_error = None;
                state.last_failure = None;
                state.last_activity = Instant::now();
                state.load_retry_backoff = MODEL_LOAD_RETRY_BACKOFF;
                state.idle_check_armed = false;
                state.generation = state.generation.wrapping_add(1);
                super::clear_model_load_failure();
            }
            Err(error) => tracing::error!(%error, "embedding lifecycle lock poisoned while configuring loader"),
        }
    }

    /// Test seam: publish a fixture provider as active without running a loader.
    pub fn set(&self, provider: Arc<dyn EmbeddingProvider>) {
        match self.inner.state.lock() {
            Ok(mut state) => {
                state.active_triple = Some(provider.triple().clone());
                state.provider = Some(provider);
                state.phase = Phase::Active;
                state.load_count = state.load_count.saturating_add(1);
                state.in_flight = 0;
                state.last_error = None;
                state.last_failure = None;
                state.last_activity = Instant::now();
                state.idle_check_armed = false;
                state.generation = state.generation.wrapping_add(1);
            }
            Err(error) => tracing::error!(%error, "embedding lifecycle lock poisoned while publishing provider"),
        }
        self.schedule_idle_check();
    }

    pub fn set_idle_window_for_tests(&self, idle_window: EmbeddingIdleWindow) {
        match self.inner.state.lock() {
            Ok(mut state) => state.idle_window = idle_window,
            Err(error) => tracing::error!(%error, "embedding lifecycle lock poisoned while setting idle window"),
        }
    }

    /// Test seam: override the failure-retry backoff duration so tests can
    /// exercise the reject-before-backoff and retry-after-backoff boundary
    /// without sleeping for the production 300s default.
    pub fn set_load_retry_backoff_for_tests(&self, backoff: Duration) {
        match self.inner.state.lock() {
            Ok(mut state) => state.load_retry_backoff = backoff,
            Err(error) => tracing::error!(%error, "embedding lifecycle lock poisoned while setting backoff"),
        }
    }

    pub fn mark_failed(&self, message: impl Into<String>) {
        let message = message.into();
        // Failed transition and generation bump under ONE lock: an in-flight
        // load whose captured generation still matched could otherwise land
        // finish_load_success in the gap between the two and flip an
        // intentionally-disabled slot back to Active.
        match self.inner.state.lock() {
            Ok(mut state) => {
                state.generation = state.generation.wrapping_add(1);
                state.provider = None;
                state.phase = Phase::Failed;
                state.last_error = Some(message.clone());
                state.last_failure = Some(Instant::now());
            }
            Err(error) => tracing::error!(%error, "embedding lifecycle lock poisoned while marking failed"),
        }
        super::record_model_load_failure(message);
    }

    pub fn acquire(&self) -> EmbeddingProviderAcquire {
        match self.inner.state.lock() {
            Ok(mut state) => match state.phase {
                Phase::Active => {
                    let Some(provider) = state.provider.clone() else {
                        state.phase = Phase::Dormant;
                        return EmbeddingProviderAcquire::Dormant;
                    };
                    state.in_flight = state.in_flight.saturating_add(1);
                    state.last_activity = Instant::now();
                    drop(state);
                    self.schedule_idle_check();
                    EmbeddingProviderAcquire::Active(ProviderGuard { owner: self.clone(), provider: Some(provider) })
                }
                Phase::Loading => EmbeddingProviderAcquire::Loading,
                Phase::Failed => EmbeddingProviderAcquire::Failed { last_error: state.last_error.clone() },
                Phase::Dormant => {
                    if state.loader.is_some() {
                        EmbeddingProviderAcquire::Dormant
                    } else {
                        EmbeddingProviderAcquire::Failed { last_error: state.last_error.clone() }
                    }
                }
            },
            Err(error) => {
                tracing::error!(%error, "embedding lifecycle lock poisoned while acquiring provider");
                EmbeddingProviderAcquire::Failed { last_error: Some("embedding lifecycle lock poisoned".to_string()) }
            }
        }
    }

    pub fn acquire_or_trigger_load(&self) -> EmbeddingProviderAcquire {
        let acquired = self.acquire();
        match &acquired {
            EmbeddingProviderAcquire::Active(_) => {}
            EmbeddingProviderAcquire::Dormant | EmbeddingProviderAcquire::Loading => self.ensure_loaded_in_background(),
            EmbeddingProviderAcquire::Failed { .. } => {
                if self.has_loader() {
                    self.ensure_loaded_in_background();
                }
            }
        }
        acquired
    }

    pub fn ensure_loaded_in_background(&self) {
        let slot = self.clone();
        if tokio::runtime::Handle::try_current().is_ok() {
            tokio::spawn(async move {
                let _ = slot.ensure_loaded().await;
            });
        } else {
            tracing::warn!("ensure_loaded_in_background called outside a tokio runtime; load request dropped");
        }
    }

    pub async fn ensure_loaded(&self) -> Result<(), EmbeddingError> {
        loop {
            let loader = {
                let mut state = self.lock_state()?;
                state.last_activity = Instant::now();
                match state.phase {
                    Phase::Active => {
                        drop(state);
                        self.schedule_idle_check();
                        return Ok(());
                    }
                    Phase::Loading => None,
                    Phase::Failed if !failure_backoff_elapsed(&state) => {
                        let error =
                            state.last_error.clone().unwrap_or_else(|| "embedding model load failed".to_string());
                        return Err(EmbeddingError::Load(error));
                    }
                    Phase::Dormant | Phase::Failed => match state.loader.clone() {
                        Some(loader) => {
                            state.phase = Phase::Loading;
                            // Capture the current generation so the spawned
                            // completion task can detect that a reconfigure
                            // happened between dispatch and completion. A stale
                            // completion (generation mismatch) discards the
                            // provider but still notifies waiters so they
                            // re-check state.
                            let generation = state.generation;
                            Some((loader, generation))
                        }
                        None => {
                            state.phase = Phase::Failed;
                            state.last_error = Some("embedding lifecycle loader is not configured".to_string());
                            state.last_failure = Some(Instant::now());
                            return Err(EmbeddingError::Load(
                                "embedding lifecycle loader is not configured".to_string(),
                            ));
                        }
                    },
                }
            };

            let Some((loader, generation)) = loader else {
                // A completion can land between observing `Loading` and this
                // future being registered. Pair the notification with a tiny
                // timeout so coalesced callers never start a second load and
                // also never wait forever on a lost wake.
                tokio::select! {
                    _ = self.inner.notify.notified() => {}
                    _ = tokio::time::sleep(Duration::from_millis(10)) => {}
                }
                continue;
            };

            // F2: decouple completion from the caller's future. The load +
            // finish_* run inside a dedicated spawned task, so cancelling the
            // caller's future (e.g. via `JoinHandle::abort`) never strands the
            // phase at `Loading`. The spawned task always transitions state
            // regardless of caller cancellation, preserving coalescing (still
            // exactly one load), waiters seeing Failed (not hanging) on load
            // failure, and the returned Result semantics for the direct awaiter.
            let slot = self.clone();
            tokio::spawn(async move {
                let loaded = tokio::task::spawn_blocking(move || loader()).await;
                match loaded {
                    Ok(Ok(provider)) => {
                        let _ = slot.finish_load_success(provider, generation);
                    }
                    Ok(Err(error)) => {
                        let message = error.to_string();
                        slot.finish_load_failure_with_generation(message, generation);
                    }
                    Err(join_error) => {
                        let message = format!("model load task panicked: {join_error}");
                        slot.finish_load_failure_with_generation(message, generation);
                    }
                }
                slot.inner.notify.notify_waiters();
            });

            // Wait for the spawned load task to complete, then observe the
            // result. The select handles a lost wake between the spawn and
            // this await point — either branch proceeds to re-check the phase.
            tokio::select! {
                _ = self.inner.notify.notified() => {}
                _ = tokio::time::sleep(Duration::from_millis(10)) => {}
            }
            // Re-check state: if the load succeeded, phase is Active → Ok; if
            // it failed, phase is Failed → Err. This also handles the rare
            // case where the notification was from a different transition.
            let state = self.lock_state()?;
            return match state.phase {
                Phase::Active => {
                    drop(state);
                    self.schedule_idle_check();
                    Ok(())
                }
                Phase::Failed => {
                    let error = state.last_error.clone().unwrap_or_else(|| "embedding model load failed".to_string());
                    Err(EmbeddingError::Load(error))
                }
                // The notification was not from our load (e.g. an idle-unload
                // raced). Loop back to re-evaluate and possibly re-trigger.
                _ => continue,
            };
        }
    }

    pub fn snapshot(&self) -> EmbeddingLifecycleSnapshot {
        match self.inner.state.lock() {
            Ok(state) => {
                let mut phase = match state.phase {
                    Phase::Dormant => "dormant",
                    Phase::Loading => "loading",
                    Phase::Active => "active",
                    Phase::Failed => "failed",
                };
                if state.phase == Phase::Active
                    && state.in_flight > 0
                    && state.idle_window.duration.is_some_and(|window| state.last_activity.elapsed() >= window)
                {
                    phase = "unloading-pending-inflight";
                }
                EmbeddingLifecycleSnapshot {
                    state: phase.to_string(),
                    load_count: state.load_count,
                    unload_count: state.unload_count,
                    idle_unload_secs: state.idle_window.seconds(),
                    idle_unload_source: state.idle_window.source(),
                    in_flight: state.in_flight,
                    last_error: state.last_error.clone(),
                }
            }
            Err(error) => {
                tracing::error!(%error, "embedding lifecycle lock poisoned while snapshotting");
                EmbeddingLifecycleSnapshot {
                    state: "failed".to_string(),
                    load_count: 0,
                    unload_count: 0,
                    idle_unload_secs: None,
                    idle_unload_source: "unknown",
                    in_flight: 0,
                    last_error: Some("embedding lifecycle lock poisoned".to_string()),
                }
            }
        }
    }

    pub fn check_idle_now(&self) {
        self.unload_if_idle();
    }

    fn finish_load_success(&self, provider: Arc<dyn EmbeddingProvider>, generation: u64) -> Result<(), EmbeddingError> {
        let mut state = self.lock_state()?;
        // F1: generation guard. A stale completion from a prior configuration
        // must not clobber newer state (e.g. a reconfigured loader's provider
        // that is now Active). Discard the stale provider and still notify
        // waiters so they re-check the current phase.
        if state.generation != generation {
            tracing::debug!(
                current_generation = state.generation,
                stale_generation = generation,
                "discarding stale load completion: generation mismatch"
            );
            return Ok(());
        }
        // Failure paths mutate under the SAME held lock (generation already
        // verified above) — dropping and re-locking would open a window where
        // a concurrent configure_loader/set gets clobbered to Failed.
        let Some(active_triple) = state.active_triple.clone() else {
            let message = "active embedding triple is not configured".to_string();
            record_failure_locked(&mut state, message.clone());
            drop(state);
            super::record_model_load_failure(message.clone());
            return Err(EmbeddingError::Load(message));
        };
        if provider.triple() != &active_triple {
            let message =
                format!("provider triple {:?} does not match active triple {:?}", provider.triple(), active_triple);
            record_failure_locked(&mut state, message.clone());
            drop(state);
            super::record_model_load_failure(message.clone());
            return Err(EmbeddingError::Load(message));
        }
        state.provider = Some(provider);
        state.phase = Phase::Active;
        state.load_count = state.load_count.saturating_add(1);
        state.last_error = None;
        state.last_failure = None;
        state.last_activity = Instant::now();
        super::clear_model_load_failure();
        drop(state);
        self.schedule_idle_check();
        Ok(())
    }

    /// Generation-guarded failure completion for spawned load tasks. If the
    /// generation no longer matches (a reconfigure happened between dispatch
    /// and completion), the stale failure is discarded — it must not clobber
    /// a newer Active/Failed state. Waiters are still notified (by the caller)
    /// so they re-check the current phase.
    fn finish_load_failure_with_generation(&self, message: String, generation: u64) {
        match self.inner.state.lock() {
            Ok(mut state) => {
                if state.generation != generation {
                    tracing::debug!(
                        current_generation = state.generation,
                        stale_generation = generation,
                        "discarding stale load failure: generation mismatch"
                    );
                    return;
                }
                state.provider = None;
                state.phase = Phase::Failed;
                state.last_error = Some(message.clone());
                state.last_failure = Some(Instant::now());
            }
            Err(error) => tracing::error!(%error, "embedding lifecycle lock poisoned while recording load failure"),
        }
        super::record_model_load_failure(message);
    }

    fn schedule_idle_check(&self) {
        let (window, generation) = match self.inner.state.lock() {
            Ok(mut state) => {
                // F4: at most one armed idle-check task per slot. If one is
                // already armed, don't spawn another — steady query traffic
                // would otherwise park hundreds of no-op sleeper tasks.
                if state.idle_check_armed {
                    return;
                }
                let Some(window) = state.idle_window.duration else {
                    return;
                };
                state.idle_check_armed = true;
                // F2: capture the current generation so the armed timer can
                // detect a reconfigure that happened while it was sleeping. A
                // stale timer must not clear the armed flag, unload a freshly
                // reloaded provider, or re-arm itself.
                (window, state.generation)
            }
            Err(error) => {
                tracing::error!(%error, "embedding lifecycle lock poisoned while scheduling idle check");
                return;
            }
        };
        let slot = self.clone();
        if tokio::runtime::Handle::try_current().is_ok() {
            tokio::spawn(async move {
                tokio::time::sleep(window).await;
                slot.fire_idle_check(generation);
            });
        } else {
            // No runtime: clear the armed flag so a future call can re-arm.
            if let Ok(mut state) = slot.inner.state.lock() {
                state.idle_check_armed = false;
            }
        }
    }

    /// Called by the single armed idle-check timer task when it fires. Clears
    /// the armed flag, checks idle, and re-arms if the provider is still
    /// Active (so a fresh timer covers the next idle window).
    ///
    /// F2: `generation` is the generation captured when the timer was armed.
    /// If it no longer matches `state.generation` (a reconfigure or idle-unload
    /// happened while the timer was sleeping), the stale timer no-ops: it
    /// does NOT clear `idle_check_armed`, does NOT unload, and does NOT
    /// re-arm. The newer configuration owns its own armed-flag lifecycle.
    fn fire_idle_check(&self, generation: u64) {
        // F2: check generation before touching anything. A stale timer from a
        // prior configuration must not clear the armed flag (which the newer
        // configuration may have set for its own timer), unload a freshly
        // reloaded provider, or re-arm.
        let current_generation = match self.inner.state.lock() {
            Ok(state) => state.generation,
            Err(error) => {
                tracing::error!(%error, "embedding lifecycle lock poisoned while checking idle generation");
                return;
            }
        };
        if current_generation != generation {
            tracing::debug!(
                current_generation,
                stale_generation = generation,
                "stale idle timer no-op: generation mismatch"
            );
            return;
        }
        // Clear the armed flag first so schedule_idle_check can re-arm.
        if let Ok(mut state) = self.inner.state.lock() {
            state.idle_check_armed = false;
        }
        self.unload_if_idle();
        let still_active = matches!(self.inner.state.lock().map(|s| s.phase), Ok(Phase::Active));
        if still_active {
            self.schedule_idle_check();
        }
    }

    fn unload_if_idle(&self) {
        match self.inner.state.lock() {
            Ok(mut state) => {
                let Some(window) = state.idle_window.duration else {
                    return;
                };
                if state.phase != Phase::Active || state.last_activity.elapsed() < window {
                    return;
                }
                if state.in_flight > 0 {
                    return;
                }
                state.provider = None;
                state.phase = Phase::Dormant;
                state.unload_count = state.unload_count.saturating_add(1);
                // F2: bump generation so any idle timer that was armed before
                // this unload (from the pre-unload era) can't touch the armed
                // flag or double-unload after a reload.
                state.generation = state.generation.wrapping_add(1);
                // The guard-drop path reaches here without going through
                // fire_idle_check, and the now-stale timer will no-op on the
                // generation mismatch without clearing the flag. Clear it here
                // or the next Active cycle never arms a timer and idle unload
                // is permanently disarmed.
                state.idle_check_armed = false;
            }
            Err(error) => tracing::error!(%error, "embedding lifecycle lock poisoned while unloading idle provider"),
        }
    }

    /// Whether a loader has been configured on this slot. Used by doctor to
    /// distinguish "dormant with a loader" (healthy, will load on demand) from
    /// "no loader configured" (disabled/never-armed).
    pub fn has_loader_configured(&self) -> bool {
        self.has_loader()
    }

    fn has_loader(&self) -> bool {
        self.inner.state.lock().map(|state| state.loader.is_some()).unwrap_or(false)
    }

    fn lock_state(&self) -> Result<std::sync::MutexGuard<'_, State>, EmbeddingError> {
        self.inner
            .state
            .lock()
            .map_err(|error| EmbeddingError::Load(format!("embedding lifecycle lock poisoned: {error}")))
    }
}

/// Record a load failure on already-held state. Callers pass the lock they
/// hold so the transition can't race a concurrent reconfiguration.
fn record_failure_locked(state: &mut State, message: String) {
    state.provider = None;
    state.phase = Phase::Failed;
    state.last_error = Some(message);
    state.last_failure = Some(Instant::now());
}

fn failure_backoff_elapsed(state: &State) -> bool {
    state.last_failure.is_none_or(|failed_at| failed_at.elapsed() >= state.load_retry_backoff)
}

impl std::fmt::Debug for EmbeddingProviderSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let snapshot = self.snapshot();
        f.debug_struct("EmbeddingProviderSlot")
            .field("state", &snapshot.state)
            .field("in_flight", &snapshot.in_flight)
            .finish()
    }
}

pub struct ProviderGuard {
    owner: EmbeddingProviderSlot,
    provider: Option<Arc<dyn EmbeddingProvider>>,
}

impl ProviderGuard {
    pub fn provider(&self) -> &Arc<dyn EmbeddingProvider> {
        self.provider.as_ref().expect("provider guard always owns a provider until drop")
    }

    pub fn provider_arc(&self) -> Arc<dyn EmbeddingProvider> {
        Arc::clone(self.provider())
    }

    pub fn triple(&self) -> &EmbeddingTriple {
        self.provider().triple()
    }

    pub fn embed_query(&self, text: &str) -> Result<Vec<f32>, EmbeddingError> {
        self.provider().embed_query(text)
    }
}

impl Drop for ProviderGuard {
    fn drop(&mut self) {
        self.provider.take();
        match self.owner.inner.state.lock() {
            Ok(mut state) => {
                state.in_flight = state.in_flight.saturating_sub(1);
            }
            Err(error) => tracing::error!(%error, "embedding lifecycle lock poisoned while releasing provider"),
        }
        self.owner.unload_if_idle();
    }
}
