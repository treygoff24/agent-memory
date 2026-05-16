use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};
use std::fs::OpenOptions;
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use memorum_coordination::claim_lock::{
    ClaimLockAcquireRequest, ClaimLockAcquireResult, ClaimLockClock, ClaimLockRegistry,
};
use memorum_coordination::presence::ClaimLockHeartbeatRenewal;
use memorum_coordination::{
    handle_peer_heartbeat as coordination_handle_peer_heartbeat, ClaimLockInfo, CoordinationConfig, PeerHeartbeatError,
    PeerHeartbeatOptions, PresenceConfig, PresenceRegistry,
};
use memory_governance::review::{over_threshold, REVIEW_QUEUE_DOGFOOD_THRESHOLD};
use memory_governance::{
    CandidateContext, CandidateMemory, ContradictionTiebreaker, ExistingMemorySummary, FileSourceResolver,
    GovernanceEngine, GovernanceProviders, GovernanceRefusalReason, GovernanceWriteDecision, GroundingVerifier,
    PolicySet, PolicySource, ReviewMemoryEnvelope, ReviewQueue, Scope as GovernanceScope, SessionSpawnResolver,
    SimilaritySearch, Source as GovernanceSource, SourceKind as GovernanceSourceKind, TiebreakOutcome, TombstoneIndex,
    TombstoneKind, TombstoneRule,
};
use memory_privacy::{
    safe_descriptor_projection, safe_plaintext_fragment, CallerSensitivity, DeterministicPrivacyClassifier,
    EncryptedPayload, FileKeyProvider, PrivacyClassifier, PrivacyDecision, PrivacyEncryptor, PrivacyNamespace,
    PrivacyStorageAction, SafeFragmentDecision,
};
use memory_source::{capture_web_source, ArtifactStore, CaptureWebSourceRequest, SourceError};
use memory_substrate::{
    events::EventKind, Author, AuthorKind, ChunkQuery, ClassificationOutcome, EncryptedSubstrateDescriptor,
    EncryptedWriteRequest, EventContext, Frontmatter, IndexProjection, Memory, MemoryContent, MemoryId, MemoryQuery,
    MemoryStatus, MemoryType, ObserveKind, PrivacySpanRecord, RecallIndexQuery, RepoPath, RetrievalPolicy, Scope,
    Sensitivity, Source, SourceKind, Substrate, SubstrateFragmentAppendRequest, SubstrateFragmentEncryption,
    SubstrateFragmentPayload, SupersedeRequest as SubstrateSupersedeRequest, TombstoneRequest, TrustLevel, WriteMode,
    WritePolicy, WriteRequest as SubstrateWriteRequest,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{broadcast, Mutex};

use crate::dream::rehydration;
use crate::protocol::{
    CaptureSourceResponse, ClaimLockWarning, ConflictSummary, ConflictsListResponse, EntitySummary, EventLogEntry,
    EventsLogPageResponse, GetProvenance, GetResponse, GovernanceForgetResponse, GovernancePolicySnapshot,
    GovernancePolicySummary, GovernanceStatus, GovernanceSupersedeResponse, GovernanceWriteResponse,
    InjectableEventKind, InspectEntitiesResponse, NamespaceNode, NamespaceTreeResponse, NotificationEvent,
    ObserveResponse, ObserveTarget, PassiveNotificationStatus, PeerActivityResponse, PeerDeliveryAuditEntry,
    PeerReleaseLockExpectedHolder, PeerReleaseLockResponse, PeerReleaseLockStatus, PeerSessionStatus,
    PeerStatusResponse, RealityCheckAction, RealityCheckRequest, RealityCheckResponse, RequestEnvelope, RequestPayload,
    RespondRefusalKind, ResponseEnvelope, ResponsePayload, RevealResponse, ReviewDecisionResponse,
    ReviewQueueItemResponse, ReviewQueueResponse, SearchHit, SearchResponse, StatusResponse, WebDashboardStatus,
    WriteNoteResponse, MAX_FRAME_BYTES, NOTIFICATION_CHANNEL_CAPACITY,
};
use crate::reality_check::{RcAdvanceRequest, RcRunRequest, RcSessionAdvance, RcSessionHandler};
use crate::recall::{
    build_delta_response_with_coordination, build_startup_response_with_coordination_config, ConcurrentSessionMode,
    DeltaCoordinationContext, DeltaPeerCooldownStore, DeltaPeerDelivery, DeltaPeerDeliveryRecorder, OmissionReason,
    RecallError, SessionBinding, SharedRecallCounters, StartupResponse,
};

mod doctor;

use doctor::doctor_response;

const SEARCH_LIMIT_DEFAULT: usize = 10;
const SEARCH_LIMIT_MAX: usize = 20;
const SEARCH_SNIPPET_MAX: usize = 240;
const GET_BODY_MAX: usize = 4_096;
const OBSERVE_TEXT_MAX_BYTES: usize = 16 * 1024;
const OBSERVE_ENTITIES_MAX: usize = 32;
const OBSERVE_ENTITY_MAX_BYTES: usize = 128;
const OBSERVE_ENTITY_BODY_MAX_BYTES: usize = 124;
const OBSERVE_BINDING_FIELD_MAX_BYTES: usize = 128;
const CLAIM_LOCK_IDENTITY_MAX_BYTES: usize = 128;
const REVIEW_QUEUE_LIMIT_DEFAULT: usize = 50;
const REVIEW_QUEUE_LIMIT_MAX: usize = 100;
const REVIEW_QUEUE_SUMMARY_MAX: usize = 512;
const REVIEW_QUEUE_POLICY_MAX: usize = 128;
const REVIEW_QUEUE_REASON_MAX: usize = 512;
const REVIEW_QUEUE_BODY_MAX: usize = 1024;
const REVIEW_QUEUE_ACTION_MAX: usize = 96;
const REVIEW_DECISION_SUMMARY_MAX: usize = 512;
const REVIEW_RESPONSE_FRAME_BUDGET: usize = MAX_FRAME_BYTES - 1024;
const DEFAULT_PROJECT_NAMESPACE: &str = "agent-memory";
const REVEAL_REASON_MAX_CHARS: usize = 512;
const REDACTED_FORGET_REASON: &str = "[redacted]";
const FORGET_REASON_MAX_CHARS: usize = 160;
const DEFAULT_SUPERSEDE_SESSION_ID: &str = "synthetic-memory-supersede";
const DEFAULT_SUPERSEDE_HARNESS: &str = "unknown";
const PEER_DELIVERY_AUDIT_CAPACITY: usize = 200;
const PEER_ACTIVITY_LIMIT_DEFAULT: usize = 50;
const PEER_ACTIVITY_LIMIT_MAX: usize = 200;
const PEER_STATUS_RECENT_DELIVERIES: usize = 5;
const WEB_DASHBOARD_READY_TIMEOUT: Duration = Duration::from_millis(750);
const WEB_DASHBOARD_READY_POLL: Duration = Duration::from_millis(25);

#[derive(Debug)]
pub struct HandlerState {
    recall: SharedRecallCounters,
    reality_check_lock: Mutex<()>,
    notifications: broadcast::Sender<crate::protocol::NotificationEvent>,
    passive_notifications: crate::notifications::PassiveQueue,
    presence: Arc<PresenceRegistry>,
    claim_locks: Arc<ClaimLockRegistry>,
    peer_deliveries: Arc<PeerDeliveryAudit>,
    peer_update_cooldowns: Arc<PeerUpdateCooldowns>,
    web_dashboard: StdMutex<WebDashboardRuntime>,
    coordination_config: CoordinationConfig,
}

impl HandlerState {
    pub fn new() -> Self {
        Self::with_coordination_config(CoordinationConfig::default())
    }

    pub fn with_coordination_level(coordination_level: u8) -> Self {
        let config = CoordinationConfig { level: coordination_level, ..CoordinationConfig::default() };
        Self::with_coordination_config(config)
    }

    pub fn with_coordination_config(coordination_config: CoordinationConfig) -> Self {
        let (notifications, _) = broadcast::channel(NOTIFICATION_CHANNEL_CAPACITY);
        Self {
            recall: SharedRecallCounters::default(),
            reality_check_lock: Mutex::new(()),
            notifications,
            passive_notifications: crate::notifications::PassiveQueue::new(),
            presence: Arc::new(PresenceRegistry::new()),
            claim_locks: Arc::new(ClaimLockRegistry::new()),
            peer_deliveries: Arc::new(PeerDeliveryAudit::new()),
            peer_update_cooldowns: Arc::new(PeerUpdateCooldowns::new()),
            web_dashboard: StdMutex::new(WebDashboardRuntime::default()),
            coordination_config,
        }
    }

    pub fn subscribe_notifications(&self) -> broadcast::Receiver<crate::protocol::NotificationEvent> {
        self.notifications.subscribe()
    }

    pub fn passive_notifications(&self) -> crate::notifications::PassiveQueue {
        self.passive_notifications.clone()
    }

    pub fn claim_locks(&self) -> &ClaimLockRegistry {
        self.claim_locks.as_ref()
    }

    pub fn claim_lock_registry(&self) -> Arc<ClaimLockRegistry> {
        self.claim_locks.clone()
    }

    pub fn presence(&self) -> &PresenceRegistry {
        self.presence.as_ref()
    }

    pub fn presence_registry(&self) -> Arc<PresenceRegistry> {
        self.presence.clone()
    }

    pub fn record_peer_delivery(&self, entry: PeerDeliveryAuditEntry) {
        self.peer_deliveries.record(entry);
    }

    pub fn claim_lock_ttl(&self) -> Duration {
        self.coordination_config.claim_lock.ttl()
    }

    pub fn presence_config(&self) -> PresenceConfig {
        self.coordination_config.presence.clone()
    }

    pub fn coordination_config(&self) -> &CoordinationConfig {
        &self.coordination_config
    }

    pub fn coordination_level(&self) -> u8 {
        self.coordination_config.level
    }

    fn effective_coordination_level(&self, meta: &GovernanceMeta) -> u8 {
        match meta.concurrent_session_mode {
            Some(ConcurrentSessionMode::Minimal) => 1,
            Some(ConcurrentSessionMode::Default) => 2,
            Some(ConcurrentSessionMode::Collaborative) => 3,
            None => self.coordination_level(),
        }
    }

    pub fn fire_reality_check_due_if_due(
        &self,
        reality_check: &crate::state::RealityCheckState,
        now: chrono::DateTime<chrono::Utc>,
    ) -> bool {
        crate::reality_check::RcScheduler::default().check_and_fire_if_due(reality_check, now, &self.notifications)
    }

    pub(crate) fn emit_notification(&self, event: NotificationEvent) {
        let _ = self.notifications.send(event);
    }
}

impl DeltaPeerDeliveryRecorder for HandlerState {
    fn record_delta_peer_delivery(&self, delivery: DeltaPeerDelivery) {
        self.record_peer_delivery(PeerDeliveryAuditEntry {
            delivered_at: delivery.delivered_at,
            from_harness: delivery.from_harness,
            from_session_id: delivery.from_session_id,
            to_harness: delivery.to_harness,
            to_session_id: delivery.to_session_id,
            memory_id: delivery.memory_id,
            relevance: delivery.relevance,
            summary: delivery.summary,
        });
    }
}

impl DeltaPeerCooldownStore for HandlerState {
    fn surfaced_peer_writes(&self, session_binding: &SessionBinding) -> HashSet<String> {
        self.peer_update_cooldowns.surfaced_peer_writes(session_binding)
    }

    fn record_surfaced_peer_writes(&self, session_binding: &SessionBinding, memory_ids: &[String]) {
        self.peer_update_cooldowns.record_surfaced_peer_writes(session_binding, memory_ids);
    }
}

#[derive(Debug, Default)]
struct PeerDeliveryAudit {
    entries: StdMutex<VecDeque<PeerDeliveryAuditEntry>>,
}

#[derive(Debug, Default)]
struct PeerUpdateCooldowns {
    surfaced: StdMutex<BTreeMap<PeerUpdateCooldownKey, BTreeSet<String>>>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct PeerUpdateCooldownKey {
    harness: String,
    session_id: String,
    namespaces: Vec<String>,
}

trait WebDashboardLauncher: std::fmt::Debug + Send + Sync {
    fn ensure_port_available(&self, port: u16) -> Result<(), String>;
    fn spawn(&self, socket_path: &str, port: u16, repo: &Path) -> Result<Box<dyn WebDashboardChild>, String>;
    fn wait_until_ready(&self, port: u16, child: &mut dyn WebDashboardChild) -> Result<(), String>;
}

trait WebDashboardChild: std::fmt::Debug + Send {
    fn try_wait(&mut self) -> Result<Option<String>, String>;
    fn kill(&mut self) -> Result<(), String>;
    fn wait(&mut self) -> Result<(), String>;
}

#[derive(Debug)]
struct OsWebDashboardLauncher;

impl WebDashboardLauncher for OsWebDashboardLauncher {
    fn ensure_port_available(&self, port: u16) -> Result<(), String> {
        ensure_web_dashboard_port_available(port)
    }

    fn spawn(&self, socket_path: &str, port: u16, repo: &Path) -> Result<Box<dyn WebDashboardChild>, String> {
        let binary = resolve_memoryd_web_binary()?;
        let child = Command::new(binary)
            .arg("--socket")
            .arg(socket_path)
            .arg("--port")
            .arg(port.to_string())
            .arg("--repo")
            .arg(repo)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("start memoryd-web: {error}"))?;
        Ok(Box::new(OsWebDashboardChild { child }))
    }

    fn wait_until_ready(&self, port: u16, child: &mut dyn WebDashboardChild) -> Result<(), String> {
        wait_for_web_dashboard_ready(port, child)
    }
}

#[derive(Debug)]
struct OsWebDashboardChild {
    child: Child,
}

impl WebDashboardChild for OsWebDashboardChild {
    fn try_wait(&mut self) -> Result<Option<String>, String> {
        self.child.try_wait().map(|status| status.map(|status| status.to_string())).map_err(|error| error.to_string())
    }

    fn kill(&mut self) -> Result<(), String> {
        self.child.kill().map_err(|error| error.to_string())
    }

    fn wait(&mut self) -> Result<(), String> {
        self.child.wait().map(drop).map_err(|error| error.to_string())
    }
}

#[derive(Debug)]
struct WebDashboardRuntime {
    port: Option<u16>,
    enabled_at: Option<chrono::DateTime<chrono::Utc>>,
    child: Option<Box<dyn WebDashboardChild>>,
    launcher: Arc<dyn WebDashboardLauncher>,
}

#[derive(Clone, Copy)]
struct WebDashboardLaunchConfig<'a> {
    port: u16,
    socket_path: &'a str,
    repo: &'a Path,
}

impl Default for WebDashboardRuntime {
    fn default() -> Self {
        Self { port: None, enabled_at: None, child: None, launcher: Arc::new(OsWebDashboardLauncher) }
    }
}

impl WebDashboardRuntime {
    #[cfg(test)]
    fn with_launcher(launcher: Arc<dyn WebDashboardLauncher>) -> Self {
        Self { port: None, enabled_at: None, child: None, launcher }
    }

    fn enable(
        &mut self,
        launch: WebDashboardLaunchConfig<'_>,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<WebDashboardStatus, HandlerError> {
        if self.child.as_mut().is_some_and(|child| child.try_wait().ok().flatten().is_none())
            && self.port == Some(launch.port)
        {
            return Ok(self.status(now));
        }
        self.stop_child();
        self.launcher.ensure_port_available(launch.port).map_err(HandlerError::port_in_use)?;
        let mut child =
            self.launcher.spawn(launch.socket_path, launch.port, launch.repo).map_err(HandlerError::web_unavailable)?;
        if let Err(error) = self.launcher.wait_until_ready(launch.port, child.as_mut()) {
            terminate_web_dashboard_child(child);
            return Err(HandlerError::web_unavailable(error));
        }
        self.port = Some(launch.port);
        self.enabled_at = Some(now);
        self.child = Some(child);
        Ok(self.status(now))
    }

    fn disable(&mut self) -> WebDashboardStatus {
        self.stop_child();
        self.port = None;
        self.enabled_at = None;
        WebDashboardStatus::stopped()
    }

    fn status(&self, now: chrono::DateTime<chrono::Utc>) -> WebDashboardStatus {
        let Some(port) = self.port else {
            return WebDashboardStatus::stopped();
        };
        let uptime_seconds = self
            .enabled_at
            .map(|started_at| now.signed_duration_since(started_at).num_seconds().max(0) as u64)
            .unwrap_or(0);
        WebDashboardStatus::running(port, uptime_seconds)
    }

    fn refresh_status(&mut self, now: chrono::DateTime<chrono::Utc>) -> WebDashboardStatus {
        if self.child.as_mut().and_then(|child| child.try_wait().ok().flatten()).is_some() {
            self.child = None;
            self.port = None;
            self.enabled_at = None;
            return WebDashboardStatus::stopped();
        }
        self.status(now)
    }

    fn stop_child(&mut self) {
        let Some(child) = self.child.take() else {
            return;
        };
        terminate_web_dashboard_child(child);
    }
}

fn ensure_web_dashboard_port_available(port: u16) -> Result<(), String> {
    let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    TcpListener::bind(address)
        .map(drop)
        .map_err(|error| format!("web dashboard port {address} is unavailable before start: {error}"))
}

fn wait_for_web_dashboard_ready(port: u16, child: &mut dyn WebDashboardChild) -> Result<(), String> {
    let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    let started_at = Instant::now();
    while started_at.elapsed() < WEB_DASHBOARD_READY_TIMEOUT {
        if let Some(status) =
            child.try_wait().map_err(|error| format!("check memoryd-web readiness status: {error}"))?
        {
            return Err(format!("memoryd-web exited before binding {address}: {status}"));
        }
        if TcpStream::connect_timeout(&address, WEB_DASHBOARD_READY_POLL).is_ok() {
            return Ok(());
        }
        std::thread::sleep(WEB_DASHBOARD_READY_POLL);
    }
    Err(format!("memoryd-web did not bind {address} before readiness timeout"))
}

fn terminate_web_dashboard_child(mut child: Box<dyn WebDashboardChild>) {
    if child.try_wait().ok().flatten().is_none() {
        let _ = child.kill();
    }
    let _ = child.wait();
}

impl Drop for WebDashboardRuntime {
    fn drop(&mut self) {
        self.stop_child();
    }
}

impl PeerDeliveryAudit {
    fn new() -> Self {
        Self::default()
    }

    fn record(&self, entry: PeerDeliveryAuditEntry) {
        let mut entries = self.entries.lock().expect("peer delivery audit lock poisoned");
        if entries.len() == PEER_DELIVERY_AUDIT_CAPACITY {
            entries.pop_front();
        }
        entries.push_back(entry);
    }

    fn snapshot(&self) -> Vec<PeerDeliveryAuditEntry> {
        self.entries.lock().expect("peer delivery audit lock poisoned").iter().cloned().collect()
    }
}

impl PeerUpdateCooldowns {
    fn new() -> Self {
        Self::default()
    }

    fn surfaced_peer_writes(&self, session_binding: &SessionBinding) -> HashSet<String> {
        self.surfaced
            .lock()
            .expect("peer update cooldown lock poisoned")
            .get(&PeerUpdateCooldownKey::from(session_binding))
            .into_iter()
            .flatten()
            .cloned()
            .collect()
    }

    fn record_surfaced_peer_writes(&self, session_binding: &SessionBinding, memory_ids: &[String]) {
        if memory_ids.is_empty() {
            return;
        }
        let mut surfaced = self.surfaced.lock().expect("peer update cooldown lock poisoned");
        surfaced.entry(PeerUpdateCooldownKey::from(session_binding)).or_default().extend(memory_ids.iter().cloned());
    }
}

impl PeerUpdateCooldownKey {
    fn from(session_binding: &SessionBinding) -> Self {
        let mut namespaces = session_binding.namespaces_in_scope.clone();
        namespaces.sort();
        namespaces.dedup();
        Self { harness: session_binding.harness.clone(), session_id: session_binding.session_id.clone(), namespaces }
    }
}

impl Default for HandlerState {
    fn default() -> Self {
        Self::new()
    }
}

pub async fn handle_request(substrate: &Substrate, envelope: RequestEnvelope) -> ResponseEnvelope {
    handle_request_with_state(substrate, &HandlerState::new(), envelope).await
}

pub async fn handle_request_with_state(
    substrate: &Substrate,
    state: &HandlerState,
    envelope: RequestEnvelope,
) -> ResponseEnvelope {
    let id = envelope.id;
    match dispatch(substrate, state, envelope.request).await {
        Ok(payload) => ResponseEnvelope::success(id, payload),
        Err(error) => ResponseEnvelope::error(id, error.code, error.message, error.retryable),
    }
}

async fn dispatch(
    substrate: &Substrate,
    state: &HandlerState,
    request: RequestPayload,
) -> Result<ResponsePayload, HandlerError> {
    match request {
        RequestPayload::Status => Ok(ResponsePayload::Status(status_response(state))),
        RequestPayload::Doctor => Ok(ResponsePayload::Doctor(doctor_response(substrate).await)),
        RequestPayload::Search { query, limit, include_body } => {
            search_response(substrate, &query, limit, include_body).await
        }
        RequestPayload::Get { id, include_provenance } => get_response(substrate, &id, include_provenance).await,
        RequestPayload::TrustArtifact { id } => trust_artifact_response(substrate, state, &id).await,
        RequestPayload::CaptureSource { url, excerpts, note } => {
            capture_source_response(substrate, url, excerpts, note).await
        }
        RequestPayload::RecallHits { since, limit } => recall_hits_response(substrate, since, limit).await,
        RequestPayload::Reveal { id, reason } => reveal_response(substrate, &id, &reason).await,
        RequestPayload::WriteNote { text } => write_note_response(substrate, &text).await,
        RequestPayload::WriteMemory { body, title, tags, meta } => {
            governance_write_response(substrate, GovernanceWriteRequest { body, title, tags, meta }).await
        }
        RequestPayload::Supersede { old_id, content, reason, meta } => {
            governance_supersede_response(
                substrate,
                Some(state),
                GovernanceSupersedeRequest { old_id, content, reason, meta },
            )
            .await
        }
        RequestPayload::Forget { id, reason } => governance_forget_response(substrate, id, reason).await,
        RequestPayload::ReviewQueue { limit } => review_queue_response(substrate, state, limit).await,
        RequestPayload::ReviewApprove { id } => review_decision_response(substrate, &id, ReviewDecision::Approve).await,
        RequestPayload::ReviewReject { id, reason } => {
            review_decision_response(substrate, &id, ReviewDecision::Reject { reason }).await
        }
        RequestPayload::Startup(request) => startup_response(substrate, state, request).await,
        RequestPayload::Delta(request) => delta_response(substrate, state, request).await,
        RequestPayload::PeerHeartbeat(heartbeat) => peer_heartbeat_response(substrate, state, heartbeat).await,
        RequestPayload::PeerStatus => Ok(ResponsePayload::PeerStatus(peer_status_response(state))),
        RequestPayload::PeerActivity { session, since, limit, format: _ } => Ok(ResponsePayload::PeerActivity(
            peer_activity_response(state, session.as_deref(), since.as_deref(), limit)?,
        )),
        RequestPayload::PeerReleaseLock { memory_id, expected_holder } => Ok(ResponsePayload::PeerReleaseLock(
            peer_release_lock_response(state, &memory_id, expected_holder.as_ref())?,
        )),
        RequestPayload::Observe { text, kind, entities, cwd, session_id, harness, harness_version } => {
            observe_response(
                substrate,
                ObserveRequestFields { text, kind, entities, cwd, session_id, harness, harness_version },
            )
            .await
        }
        RequestPayload::DreamNow { scope, force, cli_override } => {
            dream_now_response(substrate, state, DreamNowRequest { scope, force, cli_override }).await
        }
        RequestPayload::DreamStatus {} => dream_status_response(substrate).await,
        RequestPayload::WebEnable { port, socket_path } => web_enable_response(substrate, state, port, &socket_path),
        RequestPayload::WebDisable => web_disable_response(state),
        RequestPayload::WebStatus => web_status_response(state),
        RequestPayload::RealityCheck(request) => reality_check_response(substrate, state, request).await,
        RequestPayload::InspectEntities { limit, prefix } => inspect_entities_response(substrate, limit, prefix).await,
        RequestPayload::EventsLogPage { since, limit, kind_filter } => {
            events_log_page_response(substrate, since, limit, kind_filter)
        }
        RequestPayload::NamespaceTree { root, depth } => namespace_tree_response(substrate, root, depth).await,
        RequestPayload::GovernancePolicyDump => governance_policy_dump_response(substrate),
        RequestPayload::ConflictsList { limit } => conflicts_list_response(substrate, limit).await,
        RequestPayload::TestInjectEvent { kind, memory_id, ts, harness, session_id } => {
            test_inject_event_response(substrate, TestInjectEventRequest { kind, memory_id, ts, harness, session_id })
                .await
        }
    }
}

async fn recall_hits_response(
    substrate: &Substrate,
    since: Option<chrono::DateTime<chrono::Utc>>,
    limit: Option<usize>,
) -> Result<ResponsePayload, HandlerError> {
    crate::recall_hits::recent_recall_hits(substrate, since, limit)
        .map(ResponsePayload::RecallHits)
        .map_err(HandlerError::substrate)
}

async fn inspect_entities_response(
    substrate: &Substrate,
    limit: Option<usize>,
    prefix: Option<String>,
) -> Result<ResponsePayload, HandlerError> {
    let rows = substrate
        .query_recall_index_including_metadata_only(RecallIndexQuery::default())
        .await
        .map_err(HandlerError::substrate)?;
    let prefix = prefix.map(|value| value.to_ascii_lowercase());
    let mut by_id: BTreeMap<String, EntitySummary> = BTreeMap::new();
    for row in rows {
        for entity in row.entities {
            if prefix.as_ref().is_some_and(|prefix| !entity_matches_prefix(&entity, prefix)) {
                continue;
            }
            let entry = by_id.entry(entity.id.clone()).or_insert_with(|| EntitySummary {
                entity_id: entity.id.clone(),
                label: entity.label.clone(),
                aliases: Vec::new(),
                memory_count: 0,
                recent_memory_ids: Vec::new(),
            });
            entry.memory_count += 1;
            entry.recent_memory_ids.push(row.id.clone());
            for alias in entity.aliases {
                if !entry.aliases.contains(&alias) {
                    entry.aliases.push(alias);
                }
            }
        }
    }
    let mut entities = by_id.into_values().collect::<Vec<_>>();
    entities.sort_by(|left, right| {
        right.memory_count.cmp(&left.memory_count).then_with(|| left.entity_id.cmp(&right.entity_id))
    });
    entities.truncate(limit.unwrap_or(50).min(200));
    Ok(ResponsePayload::InspectEntities(InspectEntitiesResponse { entities }))
}

fn entity_matches_prefix(entity: &memory_substrate::Entity, prefix: &str) -> bool {
    entity.id.to_ascii_lowercase().starts_with(prefix)
        || entity.label.to_ascii_lowercase().starts_with(prefix)
        || entity.aliases.iter().any(|alias| alias.to_ascii_lowercase().starts_with(prefix))
}

fn events_log_page_response(
    substrate: &Substrate,
    since: Option<crate::protocol::EventId>,
    limit: usize,
    kind_filter: Option<Vec<EventKind>>,
) -> Result<ResponsePayload, HandlerError> {
    let filter_labels = kind_filter.map(|kinds| kinds.iter().map(event_kind_label).collect::<HashSet<_>>());
    let mut entries = substrate
        .events()
        .map_err(HandlerError::substrate)?
        .into_iter()
        .filter(|event| since.as_ref().is_none_or(|cursor| event.id.as_str() > cursor.as_str()))
        .filter(|event| filter_labels.as_ref().is_none_or(|labels| labels.contains(event_kind_label(&event.kind))))
        .map(|event| EventLogEntry {
            event_id: event.id,
            ts: event.at,
            device: event.device.to_string(),
            seq: event.seq,
            memory_id: memory_id_from_event_kind(&event.kind),
            summary: event_kind_summary(&event.kind),
            kind: event.kind,
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| right.ts.cmp(&left.ts).then_with(|| right.seq.cmp(&left.seq)));
    entries.truncate(limit.min(200));
    let next_since = entries.last().map(|entry| entry.event_id.clone());
    Ok(ResponsePayload::EventsLogPage(EventsLogPageResponse { entries, next_since }))
}

async fn namespace_tree_response(
    substrate: &Substrate,
    root: Option<String>,
    depth: Option<usize>,
) -> Result<ResponsePayload, HandlerError> {
    let root = root.unwrap_or_else(|| "all".to_string());
    let include_children = depth.unwrap_or(1) > 0;
    let rows = substrate
        .query_recall_index_including_metadata_only(RecallIndexQuery::default())
        .await
        .map_err(HandlerError::substrate)?;
    let mut counts = BTreeMap::<String, usize>::new();
    for row in rows {
        let namespace = namespace_for_row(&row);
        if root != "all" && !namespace.starts_with(&root) {
            continue;
        }
        *counts.entry(namespace).or_default() += 1;
    }
    let children = if include_children {
        counts
            .into_iter()
            .map(|(path, memory_count)| NamespaceNode {
                name: leaf_name(&path),
                path,
                memory_count,
                children: Vec::new(),
            })
            .collect()
    } else {
        Vec::new()
    };
    let memory_count = children.iter().map(|child: &NamespaceNode| child.memory_count).sum();
    Ok(ResponsePayload::NamespaceTree(NamespaceTreeResponse {
        root: NamespaceNode { name: leaf_name(&root), path: root, memory_count, children },
    }))
}

fn governance_policy_dump_response(substrate: &Substrate) -> Result<ResponsePayload, HandlerError> {
    let (policies, source) = load_policy_set(substrate.roots().repo.as_path())?;
    let scopes = [GovernanceScope::Me, GovernanceScope::Project, GovernanceScope::Agent, GovernanceScope::Dreaming];
    let mut summaries = Vec::new();
    for scope in scopes {
        let policy =
            policies.policy_for_scope(scope).map_err(|error| HandlerError::invalid_request(error.to_string()))?;
        let preview = policy.dry_run(&CandidateContext::new(scope).with_confidence(0.0).with_grounding(false));
        summaries.push(GovernancePolicySummary {
            scope: format!("{scope:?}").to_ascii_lowercase(),
            selected_policy: preview.selected_policy,
            policy_source: format!("{:?}", preview.policy_source).to_ascii_lowercase(),
            confidence_floor: preview.confidence_floor,
            review_gates: preview.triggered_review_gates,
            requires_grounding: preview.requires_grounding,
        });
    }
    Ok(ResponsePayload::GovernancePolicyDump(GovernancePolicySnapshot {
        source: policy_source_string(source),
        raw_yaml: first_policy_yaml(substrate.roots().repo.as_path()),
        policies: summaries,
    }))
}

async fn conflicts_list_response(substrate: &Substrate, limit: Option<usize>) -> Result<ResponsePayload, HandlerError> {
    let rows = substrate
        .query_memory(MemoryQuery {
            id: None,
            tag: None,
            status: Some(MemoryStatus::Quarantined),
            include_metadata_only: true,
            namespace_prefix: None,
            passive_recall_only: false,
            updated_since: None,
        })
        .await
        .map_err(HandlerError::substrate)?;
    let mut conflicts = Vec::new();
    for row in rows.into_iter().take(limit.unwrap_or(50).min(200)) {
        let envelope = substrate.read_memory_envelope(&row.id).await.map_err(HandlerError::substrate)?;
        conflicts.push(ConflictSummary {
            id: row.id,
            path: row.path.to_string(),
            summary: bounded(&envelope.metadata.frontmatter.summary, REVIEW_QUEUE_SUMMARY_MAX),
            reason: envelope.metadata.frontmatter.merge_diagnostics.map(|value| bounded(&value.to_string(), 240)),
            updated_at: envelope.metadata.frontmatter.updated_at,
        });
    }
    Ok(ResponsePayload::ConflictsList(ConflictsListResponse { conflicts }))
}

fn namespace_for_row(row: &memory_substrate::RecallIndexRow) -> String {
    match row.scope {
        Scope::User => "me".to_string(),
        Scope::Agent => "agent".to_string(),
        Scope::Subagent => "subagent".to_string(),
        Scope::Project => format!("project:{}", row.canonical_namespace_id.as_deref().unwrap_or("unknown")),
        Scope::Org => format!("org:{}", row.canonical_namespace_id.as_deref().unwrap_or("unknown")),
    }
}

fn leaf_name(path: &str) -> String {
    path.rsplit([':', '/']).next().filter(|name| !name.is_empty()).unwrap_or(path).to_string()
}

fn first_policy_yaml(repo: &Path) -> Option<String> {
    let policy_dir = repo.join("policies");
    let mut paths = std::fs::read_dir(policy_dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "yaml"))
        .collect::<Vec<_>>();
    paths.sort();
    paths.into_iter().next().and_then(|path| std::fs::read_to_string(path).ok())
}

fn memory_id_from_event_kind(kind: &EventKind) -> Option<MemoryId> {
    match kind {
        EventKind::WriteCommitted { id, .. }
        | EventKind::EncryptedWriteCommitted { id, .. }
        | EventKind::TombstoneCommitted { id }
        | EventKind::RecallHit { id, .. }
        | EventKind::RealityCheckConfirmed { id, .. }
        | EventKind::RealityCheckForgotten { id, .. }
        | EventKind::RealityCheckNotRelevant { id, .. } => Some(id.clone()),
        EventKind::DuplicateIdRepaired { new_id, .. } => Some(new_id.clone()),
        EventKind::ClaimLockContention { memory_id, .. } => Some(memory_id.clone()),
        EventKind::EmbeddingModelChanged { .. }
        | EventKind::StartupReconciliationCompleted { .. }
        | EventKind::OperatorRepairRequired { .. }
        | EventKind::GitPushFailed { .. }
        | EventKind::WriteRefused { .. }
        | EventKind::EncryptedContentRevealed { .. }
        | EventKind::SubstrateFragmentWritten { .. } => None,
    }
}

fn event_kind_summary(kind: &EventKind) -> String {
    match kind {
        EventKind::WriteCommitted { id, .. } => format!("memory write committed: {id}"),
        EventKind::EncryptedWriteCommitted { id, .. } => format!("encrypted memory write committed: {id}"),
        EventKind::TombstoneCommitted { id } => format!("memory tombstoned: {id}"),
        EventKind::DuplicateIdRepaired { old_id, new_id } => format!("duplicate id repaired: {old_id} -> {new_id}"),
        EventKind::EmbeddingModelChanged { chunks_requeued } => {
            format!("embedding model changed; {chunks_requeued} chunks requeued")
        }
        EventKind::StartupReconciliationCompleted { reindexed, repaired_events } => {
            format!("startup reconciliation completed; reindexed={reindexed}, repaired_events={repaired_events}")
        }
        EventKind::OperatorRepairRequired { reason }
        | EventKind::GitPushFailed { reason }
        | EventKind::EncryptedContentRevealed { reason, .. } => reason.clone(),
        EventKind::WriteRefused { reason, .. } => format!("write refused: {reason}"),
        EventKind::SubstrateFragmentWritten { id, path, .. } => format!("substrate fragment written: {id} at {path}"),
        EventKind::RecallHit { id, .. } => format!("memory recalled: {id}"),
        EventKind::RealityCheckConfirmed { id, .. } => format!("reality check confirmed: {id}"),
        EventKind::RealityCheckForgotten { id, .. } => format!("reality check forgot: {id}"),
        EventKind::RealityCheckNotRelevant { id, .. } => format!("reality check not relevant: {id}"),
        EventKind::ClaimLockContention { memory_id, .. } => format!("claim-lock contention: {memory_id}"),
    }
}

fn event_kind_label(kind: &EventKind) -> &'static str {
    match kind {
        EventKind::WriteCommitted { .. } => "write_committed",
        EventKind::EncryptedWriteCommitted { .. } => "encrypted_write_committed",
        EventKind::TombstoneCommitted { .. } => "tombstone_committed",
        EventKind::DuplicateIdRepaired { .. } => "duplicate_id_repaired",
        EventKind::EmbeddingModelChanged { .. } => "embedding_model_changed",
        EventKind::StartupReconciliationCompleted { .. } => "startup_reconciliation_completed",
        EventKind::OperatorRepairRequired { .. } => "operator_repair_required",
        EventKind::GitPushFailed { .. } => "git_push_failed",
        EventKind::WriteRefused { .. } => "write_refused",
        EventKind::EncryptedContentRevealed { .. } => "encrypted_content_revealed",
        EventKind::SubstrateFragmentWritten { .. } => "substrate_fragment_written",
        EventKind::RecallHit { .. } => "recall_hit",
        EventKind::RealityCheckConfirmed { .. } => "reality_check_confirmed",
        EventKind::RealityCheckForgotten { .. } => "reality_check_forgotten",
        EventKind::RealityCheckNotRelevant { .. } => "reality_check_not_relevant",
        EventKind::ClaimLockContention { .. } => "claim_lock_contention",
    }
}

fn web_enable_response(
    substrate: &Substrate,
    state: &HandlerState,
    port: u16,
    socket_path: &str,
) -> Result<ResponsePayload, HandlerError> {
    if port < 1024 {
        return Err(HandlerError::invalid_request("web dashboard port must be in 1024..=65535"));
    }
    let mut dashboard = state.web_dashboard.lock().expect("web dashboard lock poisoned");
    Ok(ResponsePayload::WebStatus(dashboard.enable(
        WebDashboardLaunchConfig { port, socket_path, repo: substrate.roots().repo.as_path() },
        chrono::Utc::now(),
    )?))
}

fn web_disable_response(state: &HandlerState) -> Result<ResponsePayload, HandlerError> {
    let mut dashboard = state.web_dashboard.lock().expect("web dashboard lock poisoned");
    Ok(ResponsePayload::WebStatus(dashboard.disable()))
}

fn web_status_response(state: &HandlerState) -> Result<ResponsePayload, HandlerError> {
    let mut dashboard = state.web_dashboard.lock().expect("web dashboard lock poisoned");
    Ok(ResponsePayload::WebStatus(dashboard.refresh_status(chrono::Utc::now())))
}

async fn trust_artifact_response(
    substrate: &Substrate,
    state: &HandlerState,
    id: &str,
) -> Result<ResponsePayload, HandlerError> {
    let memory_id = MemoryId::try_new(id.to_owned()).map_err(|err| HandlerError::invalid_request(err.to_string()))?;
    let artifact = crate::trust_artifact::TrustArtifactBuilder::new(substrate)
        .with_claim_locks(state.claim_locks())
        .build(&memory_id)
        .await
        .map_err(HandlerError::trust_artifact)?;
    Ok(ResponsePayload::TrustArtifact(Box::new(artifact)))
}

async fn capture_source_response(
    substrate: &Substrate,
    url: String,
    excerpts: Vec<String>,
    note: Option<String>,
) -> Result<ResponsePayload, HandlerError> {
    if excerpts.is_empty() {
        return Err(HandlerError::invalid_request("source capture requires at least one excerpt"));
    }
    if excerpts.len() > 8 {
        return Err(HandlerError::invalid_request("source capture accepts at most 8 excerpts"));
    }
    for excerpt in &excerpts {
        if excerpt.trim().is_empty() {
            return Err(HandlerError::invalid_request("source capture excerpts must be non-empty"));
        }
        if excerpt.len() > 2 * 1024 {
            return Err(HandlerError::invalid_request("source capture excerpts must be at most 2 KiB"));
        }
    }
    if let Some(note) = &note {
        if note.len() > 2 * 1024 {
            return Err(HandlerError::invalid_request("source capture note must be at most 2 KiB"));
        }
        if !is_safe_plaintext_for_indexing(note) {
            return Err(HandlerError::invalid_request("source capture note must not contain sensitive material"));
        }
    }
    let response = capture_web_source(substrate.roots().repo.clone(), CaptureWebSourceRequest { url, excerpts, note })
        .await
        .map_err(HandlerError::source_capture)?;
    Ok(ResponsePayload::CaptureSource(CaptureSourceResponse {
        artifact_id: response.artifact_id,
        source_refs: response.source_refs,
        final_url: response.final_url,
        captured_at: response.captured_at,
        capture_status: response.capture_status,
        warnings: response.warnings,
    }))
}

async fn peer_heartbeat_response(
    substrate: &Substrate,
    state: &HandlerState,
    heartbeat: crate::protocol::PeerHeartbeat,
) -> Result<ResponsePayload, HandlerError> {
    let clock = ClaimLockClock::now();
    let mut ack = coordination_handle_peer_heartbeat(
        state.presence(),
        heartbeat.clone(),
        PeerHeartbeatOptions {
            default_level: state.coordination_level(),
            now: clock.instant,
            stale_threshold: state.presence_config().stale_after(),
            claim_lock_renewal: Some(ClaimLockHeartbeatRenewal {
                registry: state.claim_locks(),
                ttl: state.claim_lock_ttl(),
                clock,
            }),
        },
    )
    .map_err(peer_heartbeat_error)?;
    // INVARIANT: `conflicting_claim_locks` is populated only at Level 3.
    // Level 1/2 acknowledgements intentionally return `[]`; Level 2 dogfood
    // keeps same-device claim-lock conflict signals dormant until full
    // concurrent-session mode is explicitly enabled.
    // See docs/specs/stream-i-cross-session-v0.1.md §6 and
    // docs/api/stream-i-cross-session-api.md (heartbeat notes).
    if ack.active_level == 3 {
        ack.conflicting_claim_locks = conflicting_claim_locks_for_heartbeat(substrate, state, &heartbeat).await;
    }
    Ok(ResponsePayload::PeerHeartbeat(ack))
}

fn peer_heartbeat_error(error: PeerHeartbeatError) -> HandlerError {
    match error {
        PeerHeartbeatError::InvalidRequest { message } => HandlerError::invalid_request(message),
    }
}

async fn conflicting_claim_locks_for_heartbeat(
    substrate: &Substrate,
    state: &HandlerState,
    heartbeat: &crate::protocol::PeerHeartbeat,
) -> Vec<ClaimLockInfo> {
    let heartbeat_entities = heartbeat
        .salient_entities
        .iter()
        .map(|entity| entity.trim().to_ascii_lowercase())
        .filter(|entity| !entity.is_empty())
        .collect::<HashSet<_>>();
    if heartbeat_entities.is_empty() {
        return Vec::new();
    }

    let mut locks = Vec::new();
    for lock in state.claim_locks().active_locks() {
        if lock.holder_harness == heartbeat.harness && lock.holder_session_id == heartbeat.session_id {
            continue;
        }
        let Ok(memory_id) = MemoryId::try_new(lock.memory_id.clone()) else {
            continue;
        };
        let Ok(memory) = substrate.read_memory(&memory_id).await else {
            continue;
        };
        let intersects = memory.frontmatter.entities.iter().any(|entity| {
            heartbeat_entities.contains(entity.id.trim().to_ascii_lowercase().as_str())
                || entity
                    .aliases
                    .iter()
                    .any(|alias| heartbeat_entities.contains(alias.trim().to_ascii_lowercase().as_str()))
        });
        if intersects {
            locks.push(lock);
        }
    }
    locks.sort_by(|left, right| {
        left.memory_id
            .cmp(&right.memory_id)
            .then_with(|| left.holder_harness.cmp(&right.holder_harness))
            .then_with(|| left.holder_session_id.cmp(&right.holder_session_id))
    });
    locks
}

fn peer_status_response(state: &HandlerState) -> PeerStatusResponse {
    let now = Instant::now();
    let stale_after = state.presence_config().stale_after();
    let mut active_sessions = state
        .presence()
        .all_records()
        .into_iter()
        .filter(|record| now.duration_since(record.last_heartbeat_at) <= stale_after)
        .map(|record| PeerSessionStatus {
            session_id: record.session_id,
            harness: record.harness,
            namespace: record.namespace,
            salient_entities: record.salient_entities.into_iter().take(5).collect(),
            started_at: record.started_at,
            last_heartbeat_age_seconds: now.duration_since(record.last_heartbeat_at).as_secs(),
        })
        .collect::<Vec<_>>();
    active_sessions.sort_by(|left, right| {
        left.harness
            .cmp(&right.harness)
            .then_with(|| left.session_id.cmp(&right.session_id))
            .then_with(|| left.namespace.cmp(&right.namespace))
    });

    let mut claim_locks = state.claim_locks().active_locks();
    claim_locks.sort_by(|left, right| {
        left.memory_id
            .cmp(&right.memory_id)
            .then_with(|| left.holder_harness.cmp(&right.holder_harness))
            .then_with(|| left.holder_session_id.cmp(&right.holder_session_id))
    });

    let recent_deliveries = recent_deliveries(state, PEER_STATUS_RECENT_DELIVERIES);

    PeerStatusResponse {
        coordination_level: state.coordination_level(),
        active_sessions,
        claim_locks,
        recent_deliveries,
    }
}

fn peer_activity_response(
    state: &HandlerState,
    session: Option<&str>,
    since: Option<&str>,
    limit: Option<usize>,
) -> Result<PeerActivityResponse, HandlerError> {
    let limit = limit.unwrap_or(PEER_ACTIVITY_LIMIT_DEFAULT).min(PEER_ACTIVITY_LIMIT_MAX);
    let since = since.map(parse_peer_activity_since).transpose()?;
    let mut entries = state
        .peer_deliveries
        .snapshot()
        .into_iter()
        .filter(|entry| session.is_none_or(|session| peer_delivery_matches_session(entry, session)))
        .filter(|entry| since.is_none_or(|since| entry.delivered_at >= since))
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        right
            .delivered_at
            .cmp(&left.delivered_at)
            .then_with(|| left.from_session_id.cmp(&right.from_session_id))
            .then_with(|| left.to_session_id.cmp(&right.to_session_id))
            .then_with(|| left.memory_id.cmp(&right.memory_id))
    });
    let total_recorded = entries.len();
    entries.truncate(limit);
    Ok(PeerActivityResponse { entries, limit, total_recorded })
}

fn peer_release_lock_response(
    state: &HandlerState,
    memory_id: &str,
    expected_holder: Option<&PeerReleaseLockExpectedHolder>,
) -> Result<PeerReleaseLockResponse, HandlerError> {
    let memory_id = memory_id.trim();
    if memory_id.is_empty() {
        return Err(HandlerError::invalid_request("memory_id must not be empty"));
    }

    let Some(lock) = state.claim_locks().get(memory_id) else {
        return Ok(PeerReleaseLockResponse {
            memory_id: memory_id.to_owned(),
            status: PeerReleaseLockStatus::NoLockFound,
            released: None,
        });
    };
    if let Some(expected) = expected_holder {
        if lock.holder_harness != expected.holder_harness || lock.holder_session_id != expected.holder_session_id {
            return Ok(PeerReleaseLockResponse {
                memory_id: memory_id.to_owned(),
                status: PeerReleaseLockStatus::LockChanged,
                released: None,
            });
        }
    }

    let released = state.claim_locks().release(memory_id, &lock.holder_harness, &lock.holder_session_id);
    let status = if released.is_some() {
        PeerReleaseLockStatus::Released
    } else if expected_holder.is_some() && state.claim_locks().get(memory_id).is_some() {
        PeerReleaseLockStatus::LockChanged
    } else {
        PeerReleaseLockStatus::NoLockFound
    };

    Ok(PeerReleaseLockResponse { memory_id: memory_id.to_owned(), status, released })
}

fn recent_deliveries(state: &HandlerState, limit: usize) -> Vec<PeerDeliveryAuditEntry> {
    let mut deliveries = state.peer_deliveries.snapshot();
    deliveries.sort_by(|left, right| right.delivered_at.cmp(&left.delivered_at));
    deliveries.truncate(limit);
    deliveries
}

fn peer_delivery_matches_session(entry: &PeerDeliveryAuditEntry, session: &str) -> bool {
    entry.from_session_id == session || entry.to_session_id == session
}

fn parse_peer_activity_since(raw: &str) -> Result<chrono::DateTime<chrono::Utc>, HandlerError> {
    if let Ok(date_time) = chrono::DateTime::parse_from_rfc3339(raw) {
        return Ok(date_time.with_timezone(&chrono::Utc));
    }
    if let Ok(date) = chrono::NaiveDate::parse_from_str(raw, "%Y-%m-%d") {
        let Some(date_time) = date.and_hms_opt(0, 0, 0) else {
            return Err(HandlerError::invalid_request("invalid peer activity --since date"));
        };
        return Ok(date_time.and_utc());
    }
    if let Ok(time) = chrono::NaiveTime::parse_from_str(raw, "%H:%M") {
        let date = chrono::Utc::now().date_naive();
        return Ok(date.and_time(time).and_utc());
    }
    Err(HandlerError::invalid_request("peer activity --since must be HH:MM, YYYY-MM-DD, or RFC3339"))
}

fn status_response(state: &HandlerState) -> StatusResponse {
    StatusResponse {
        state: "ready".to_string(),
        guidance: "memoryd handlers are backed by the Stream A substrate.".to_string(),
        recall: state.recall.snapshot(),
        dreams: Default::default(),
        passive_notifications: state
            .passive_notifications
            .entries()
            .into_iter()
            .map(|entry| PassiveNotificationStatus { message: entry.message, created_at: entry.created_at })
            .collect(),
    }
}

async fn dream_status_response(substrate: &Substrate) -> Result<ResponsePayload, HandlerError> {
    crate::dream::status::build_dream_status_report(&substrate.roots().repo, &substrate.roots().runtime)
        .await
        .map(|report| ResponsePayload::DreamStatus(Box::new(report)))
        .map_err(HandlerError::substrate)
}

async fn reality_check_response(
    substrate: &Substrate,
    state: &HandlerState,
    request: RealityCheckRequest,
) -> Result<ResponsePayload, HandlerError> {
    match request {
        RealityCheckRequest::List { namespace, limit } => {
            let handler = RcSessionHandler::new(substrate);
            let now = chrono::Utc::now();
            let response = handler.list(namespace, limit, now).await.map_err(HandlerError::substrate)?;
            Ok(ResponsePayload::RealityCheck(response))
        }
        mutating_request => {
            let _guard = state.reality_check_lock.lock().await;
            reality_check_mutating_response(substrate, mutating_request).await
        }
    }
}

async fn reality_check_mutating_response(
    substrate: &Substrate,
    request: RealityCheckRequest,
) -> Result<ResponsePayload, HandlerError> {
    let handler = RcSessionHandler::new(substrate);
    let now = chrono::Utc::now();
    let response = match request {
        RealityCheckRequest::List { .. } => unreachable!("list requests are handled without the mutation lock"),
        RealityCheckRequest::Run { session_id, namespace, limit } => handler
            .run(RcRunRequest { requested_session_id: session_id, namespace, limit, now })
            .await
            .map_err(HandlerError::substrate)?,
        RealityCheckRequest::Respond { session_id, memory_id, action } => {
            reality_check_respond(RealityCheckRespondRequest {
                substrate,
                handler: &handler,
                session_id,
                memory_id,
                action,
                now,
            })
            .await?
        }
        RealityCheckRequest::Skip => {
            let skipped_until = now + chrono::Duration::days(7);
            let mut state = crate::state::DaemonState::load(&substrate.roots().runtime);
            state.reality_check.snooze_until = Some(skipped_until);
            state.save(&substrate.roots().runtime).map_err(HandlerError::substrate)?;
            RealityCheckResponse::Skipped { skipped_until }
        }
        RealityCheckRequest::Snooze { until } => {
            let snooze_until = until.unwrap_or_else(|| now + chrono::Duration::days(7));
            let mut state = crate::state::DaemonState::load(&substrate.roots().runtime);
            state.reality_check.snooze_until = Some(snooze_until);
            state.save(&substrate.roots().runtime).map_err(HandlerError::substrate)?;
            RealityCheckResponse::Snoozed { snooze_until }
        }
        RealityCheckRequest::Reset => {
            let cleared_session = crate::state::RcSessionStore::new(&substrate.roots().runtime)
                .load_if_recent(now)
                .ok()
                .flatten()
                .is_some();
            crate::state::RcSessionStore::new(&substrate.roots().runtime).delete().map_err(HandlerError::substrate)?;
            crate::state::RcPendingCache::delete(&substrate.roots().runtime).map_err(HandlerError::substrate)?;
            RealityCheckResponse::Reset { cleared_pending: 0, cleared_session }
        }
    };
    Ok(ResponsePayload::RealityCheck(response))
}

async fn reality_check_respond(request: RealityCheckRespondRequest<'_>) -> Result<RealityCheckResponse, HandlerError> {
    let RealityCheckRespondRequest { substrate, handler, session_id, memory_id, action, now } = request;
    let session = match handler.load_session_for_response(&session_id, &memory_id, now) {
        Ok(session) => session,
        Err(response) => return Ok(*response),
    };

    let advance = match action {
        RealityCheckAction::Confirm => {
            confirm_reality_check_item(substrate, &session_id, &memory_id, now).await?;
            RcSessionAdvance::Reviewed
        }
        RealityCheckAction::Correct { new_body } => {
            match correct_reality_check_item(substrate, &session_id, &memory_id, new_body).await? {
                None => {
                    return Ok(reality_check_refused(
                        &session_id,
                        &memory_id,
                        "correction refused",
                        RespondRefusalKind::GovernanceRefused,
                    ))
                }
                Some(response) => {
                    if let RealityCheckResponse::RespondRefused { .. } = response {
                        return Ok(response);
                    }
                }
            }
            RcSessionAdvance::Reviewed
        }
        RealityCheckAction::Forget { reason } => {
            if reason.trim().len() < 3 {
                return Ok(reality_check_refused(
                    &session_id,
                    &memory_id,
                    "reason too short",
                    RespondRefusalKind::InvalidAction,
                ));
            }
            forget_reality_check_item(substrate, &session_id, &memory_id, sanitize_forget_reason(&reason)).await?;
            RcSessionAdvance::Reviewed
        }
        RealityCheckAction::NotRelevant => {
            not_relevant_reality_check_item(substrate, &session_id, &memory_id).await?;
            RcSessionAdvance::Reviewed
        }
        RealityCheckAction::SkipThisWeek => RcSessionAdvance::Deferred,
    };

    handler.advance(RcAdvanceRequest { session, memory_id, advance, now }).await.map_err(HandlerError::substrate)
}

struct RealityCheckRespondRequest<'a> {
    substrate: &'a Substrate,
    handler: &'a RcSessionHandler<'a>,
    session_id: String,
    memory_id: MemoryId,
    action: RealityCheckAction,
    now: chrono::DateTime<chrono::Utc>,
}

async fn confirm_reality_check_item(
    substrate: &Substrate,
    session_id: &str,
    memory_id: &MemoryId,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<(), HandlerError> {
    mutate_reality_check_metadata(substrate, memory_id, |memory| {
        memory.frontmatter.updated_at = now;
        memory.frontmatter.observed_at = Some(now);
        memory.frontmatter.confidence = (memory.frontmatter.confidence + 0.02).min(1.0);
    })
    .await?;
    substrate
        .record_event_best_effort(EventKind::RealityCheckConfirmed {
            id: memory_id.clone(),
            session_id: session_id.to_owned(),
        })
        .map_err(|error| HandlerError::substrate(format!("record reality check confirmation: {error}")))
}

async fn correct_reality_check_item(
    substrate: &Substrate,
    session_id: &str,
    memory_id: &MemoryId,
    new_body: String,
) -> Result<Option<RealityCheckResponse>, HandlerError> {
    if new_body.trim().is_empty() {
        return Ok(Some(reality_check_refused(
            session_id,
            memory_id,
            "correction body must not be empty",
            RespondRefusalKind::InvalidAction,
        )));
    }
    let old = substrate.read_memory_envelope(memory_id).await.map_err(HandlerError::substrate)?.metadata;
    let response = governance_supersede_response(
        substrate,
        None,
        GovernanceSupersedeRequest {
            old_id: memory_id.as_str().to_owned(),
            content: new_body,
            reason: "reality check correction".to_owned(),
            meta: serde_json::json!({
                "namespace": governance_namespace_meta(&old.frontmatter),
                "type": governance_type_meta(old.frontmatter.memory_type),
                "summary": old.frontmatter.summary,
                "confidence": old.frontmatter.confidence,
                "sensitivity": sensitivity_meta(old.frontmatter.sensitivity),
                "source_kind": "user",
                "explicit_user_context": true
            }),
        },
    )
    .await?;
    let ResponsePayload::GovernanceSupersede(supersede) = response else {
        return Ok(Some(reality_check_refused(
            session_id,
            memory_id,
            "unexpected correction response",
            RespondRefusalKind::GovernanceRefused,
        )));
    };
    if supersede.status == GovernanceStatus::Promoted {
        return Ok(Some(RealityCheckResponse::Pending {
            session_id: Some(session_id.to_owned()),
            items: Vec::new(),
            total_scored: 0,
            last_completed_at: None,
        }));
    }

    let kind = if supersede.reason == Some(GovernanceRefusalReason::Tombstone) {
        RespondRefusalKind::TombstoneMatch
    } else {
        RespondRefusalKind::GovernanceRefused
    };
    Ok(Some(reality_check_refused(
        session_id,
        memory_id,
        format!("governance refused correction: {:?}", supersede.reason),
        kind,
    )))
}

async fn forget_reality_check_item(
    substrate: &Substrate,
    session_id: &str,
    memory_id: &MemoryId,
    reason: String,
) -> Result<(), HandlerError> {
    let response = governance_forget_response(substrate, memory_id.as_str().to_owned(), reason.clone()).await?;
    let ResponsePayload::GovernanceForget(forget) = response else {
        return Err(HandlerError::substrate("unexpected forget response"));
    };
    if forget.status != GovernanceStatus::Tombstoned {
        return Err(HandlerError::substrate("governance did not tombstone memory"));
    }
    substrate
        .record_event_best_effort(EventKind::RealityCheckForgotten {
            id: memory_id.clone(),
            session_id: session_id.to_owned(),
            reason,
        })
        .map_err(|error| HandlerError::substrate(format!("record reality check forgotten: {error}")))
}

async fn not_relevant_reality_check_item(
    substrate: &Substrate,
    session_id: &str,
    memory_id: &MemoryId,
) -> Result<(), HandlerError> {
    mutate_reality_check_metadata(substrate, memory_id, |memory| {
        memory.frontmatter.updated_at = chrono::Utc::now();
        memory.frontmatter.retrieval_policy.passive_recall = false;
        if !memory.frontmatter.tags.iter().any(|tag| tag == "reality_check_not_relevant") {
            memory.frontmatter.tags.push("reality_check_not_relevant".to_owned());
        }
    })
    .await?;
    substrate
        .record_event_best_effort(EventKind::RealityCheckNotRelevant {
            id: memory_id.clone(),
            session_id: session_id.to_owned(),
        })
        .map_err(|error| HandlerError::substrate(format!("record reality check not relevant: {error}")))
}

async fn mutate_reality_check_metadata(
    substrate: &Substrate,
    memory_id: &MemoryId,
    mutate: impl FnOnce(&mut Memory),
) -> Result<(), HandlerError> {
    let envelope = substrate.read_memory_envelope(memory_id).await.map_err(HandlerError::substrate)?;
    if !matches!(envelope.content, MemoryContent::Plaintext(_)) {
        return substrate.update_encrypted_memory_metadata(memory_id, mutate).await.map_err(HandlerError::substrate);
    }
    let mut memory = envelope.metadata;
    mutate(&mut memory);
    substrate
        .write_memory(SubstrateWriteRequest {
            operation_id: None,
            memory,
            expected_base_hash: None,
            write_mode: WriteMode::AdminRepair,
            index_projection: None,
            event_context: EventContext {
                actor: Some("memoryd-reality-check".to_owned()),
                reason: Some("reality check metadata update".to_owned()),
            },
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Trusted,
        })
        .await
        .map(|_| ())
        .map_err(HandlerError::substrate)
}

fn reality_check_refused(
    session_id: &str,
    memory_id: &MemoryId,
    reason: impl Into<String>,
    kind: RespondRefusalKind,
) -> RealityCheckResponse {
    RealityCheckResponse::RespondRefused {
        session_id: session_id.to_owned(),
        memory_id: memory_id.clone(),
        reason: reason.into(),
        kind,
    }
}

fn governance_namespace_meta(frontmatter: &Frontmatter) -> &'static str {
    match frontmatter.scope {
        Scope::User => "me",
        Scope::Project | Scope::Org => "project",
        Scope::Agent | Scope::Subagent => "agent",
    }
}

fn governance_type_meta(memory_type: MemoryType) -> &'static str {
    match memory_type {
        MemoryType::Claim => "claim",
        MemoryType::Decision => "decision",
        MemoryType::Pattern => "pattern",
        MemoryType::Playbook => "playbook",
        MemoryType::Procedure => "procedure",
        MemoryType::Artifact => "artifact",
        MemoryType::Project => "project",
        MemoryType::Person
        | MemoryType::Episode
        | MemoryType::Prospective
        | MemoryType::Postmortem
        | MemoryType::AntiPattern
        | MemoryType::Heuristic
        | MemoryType::Regression
        | MemoryType::Correction
        | MemoryType::Invariant
        | MemoryType::OpenQuestion => "claim",
    }
}

fn sensitivity_meta(sensitivity: Sensitivity) -> &'static str {
    match sensitivity {
        Sensitivity::Public => "public",
        Sensitivity::Internal => "internal",
        Sensitivity::Confidential => "confidential",
        Sensitivity::Personal => "personal",
    }
}

fn sanitize_forget_reason(reason: &str) -> String {
    let trimmed = reason.trim();
    if trimmed.is_empty() {
        return REDACTED_FORGET_REASON.to_owned();
    }
    if !is_safe_plaintext_for_indexing(trimmed) || contains_secret_or_pii_marker(trimmed) {
        return REDACTED_FORGET_REASON.to_owned();
    }
    trimmed.chars().take(FORGET_REASON_MAX_CHARS).collect()
}

fn contains_secret_or_pii_marker(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("sk-")
        || lower.contains("api key")
        || lower.contains("secret")
        || lower.contains("token")
        || contains_email_like_token(text)
        || contains_phone_like_token(text)
}

fn contains_email_like_token(text: &str) -> bool {
    text.split_whitespace().any(|token| {
        let token = token.trim_matches(|ch: char| ch.is_ascii_punctuation() && ch != '@' && ch != '.');
        token.contains('@') && token.contains('.')
    })
}

fn contains_phone_like_token(text: &str) -> bool {
    let digit_count = text.chars().filter(|ch| ch.is_ascii_digit()).count();
    digit_count >= 7 && text.chars().any(|ch| matches!(ch, '-' | '(' | ')' | '+' | '.'))
}

/// Inject a synthetic event-log entry with a controlled timestamp.
///
/// This handler is only functional when `memoryd` is compiled with the
/// `test-utils` feature flag; without it, the protocol variant still exists
/// (so the crate compiles) but the handler returns `method_not_allowed`. This
/// keeps the test-only surface invisible in production daemon builds while
/// letting Stream H eval tests exercise events-log-derived metrics
/// deterministically. (H-R1)
#[cfg_attr(not(feature = "test-utils"), allow(dead_code))]
struct TestInjectEventRequest {
    kind: InjectableEventKind,
    memory_id: MemoryId,
    ts: chrono::DateTime<chrono::Utc>,
    harness: Option<String>,
    session_id: Option<String>,
}

async fn test_inject_event_response(
    substrate: &Substrate,
    request: TestInjectEventRequest,
) -> Result<ResponsePayload, HandlerError> {
    #[cfg(not(feature = "test-utils"))]
    {
        let _ = (substrate, request);
        Err(HandlerError::invalid_request(
            "TestInjectEvent requires the memoryd `test-utils` feature; \
             this daemon was compiled without it",
        ))
    }

    #[cfg(feature = "test-utils")]
    {
        let event_kind = match request.kind {
            InjectableEventKind::RecallHit => {
                EventKind::RecallHit { id: request.memory_id.clone(), recalled_at: request.ts }
            }
            InjectableEventKind::WriteCommitted => {
                // Synthetic WriteCommitted: we use a placeholder path derived from the
                // memory_id since we don't re-query the substrate for the actual file path.
                // The cross_source_corroboration metric only counts distinct devices/harnesses
                // that produced WriteCommitted events for a given memory_id; the path field
                // is not used for scoring. Source attribution (harness) is available in the
                // harness parameter but WriteCommitted's schema does not carry it (§12.1).
                let synthetic_path =
                    memory_substrate::RepoPath::new(format!("synthetic-test-inject/{}.md", request.memory_id.as_str()));
                EventKind::WriteCommitted {
                    id: request.memory_id.clone(),
                    path: synthetic_path,
                    classification: memory_substrate::ClassificationOutcome::Trusted,
                }
            }
        };
        let _ = (request.harness, request.session_id); // reserved for future provenance embedding
        substrate.record_event_best_effort(event_kind).map_err(HandlerError::substrate)?;
        let event_id = format!("injected-{}-{}", kind_label(request.kind), request.memory_id.as_str());
        Ok(ResponsePayload::TestInjectEvent(crate::protocol::TestInjectEventResponse {
            event_id,
            injected_kind: request.kind,
            memory_id: request.memory_id,
        }))
    }
}

#[cfg(feature = "test-utils")]
fn kind_label(kind: InjectableEventKind) -> &'static str {
    match kind {
        InjectableEventKind::RecallHit => "recall-hit",
        InjectableEventKind::WriteCommitted => "write-committed",
    }
}

struct DreamNowRequest {
    scope: String,
    force: bool,
    cli_override: Option<String>,
}

async fn dream_now_response(
    substrate: &Substrate,
    state: &HandlerState,
    request: DreamNowRequest,
) -> Result<ResponsePayload, HandlerError> {
    let DreamNowRequest { scope, force, cli_override } = request;
    let config = memory_substrate::config::load_config(&substrate.roots().repo, &substrate.roots().runtime, None)
        .map_err(HandlerError::invalid_request)?;
    if !config.synced.dreams.enabled
        || crate::dream::status::disabled_sentinel_path(&substrate.roots().runtime).exists()
    {
        return Err(HandlerError::dream_disabled("dreaming is disabled on this device"));
    }
    let scope = crate::dream::scope::DreamScope::parse(&scope).map_err(HandlerError::from_dream)?;
    validate_dream_cli_override(cli_override.as_deref())?;
    let now = chrono::Utc::now();
    let acquired = crate::dream::lease::acquire_manual_lease(crate::dream::lease::LeaseAcquireRequest {
        repo: substrate.roots().repo.clone(),
        runtime: substrate.roots().runtime.clone(),
        scope: scope.as_str(),
        force,
        now,
        lease_window_seconds: u64::from(config.synced.dreams.lease_window_seconds),
        cli_used: cli_override.clone(),
    })
    .map_err(HandlerError::from_lease)?;

    let result = async {
        let build = crate::dream::orchestration::build_dream_run(
            substrate,
            crate::dream::orchestration::DreamRunBuildRequest {
                scope: scope.clone(),
                run_id: acquired.record.run_id,
                run_date: now.date_naive(),
                prompt_version: config.synced.dreams.prompt_version,
                notifications: Some(state.notifications.clone()),
                pass_timeout: std::time::Duration::from_secs(u64::from(config.synced.dreams.per_pass_timeout_seconds)),
                pass_2_max_candidates: config.synced.dreams.pass_2_max_candidates as usize,
                pass_1_window_days: config.synced.dreams.pass_1_window_days,
            },
        )
        .await
        .map_err(HandlerError::from_dream)?;
        let harness = crate::dream::orchestration::select_harness(
            cli_override.as_deref(),
            &config.synced.dreams.default_cli_priority,
            &build.options,
        )
        .await
        .map_err(dream_error_to_handler)?;
        crate::dream::run::DreamRunner::new(build.options.with_harness(harness), build.writer)
            .run()
            .await
            .map(|report| ResponsePayload::DreamNow(Box::new(report)))
            .map_err(HandlerError::from_dream)
    }
    .await;

    if result.is_err() {
        let _ = crate::dream::lease::release_manual_lease(crate::dream::lease::LeaseAcquireRequest {
            repo: substrate.roots().repo.clone(),
            runtime: substrate.roots().runtime.clone(),
            scope: scope.as_str(),
            force: false,
            now: chrono::Utc::now(),
            lease_window_seconds: u64::from(config.synced.dreams.lease_window_seconds),
            cli_used: cli_override,
        });
    }

    result
}

fn dream_error_to_handler(error: crate::dream::types::DreamError) -> HandlerError {
    let message = error.to_string();
    if let Some(rest) = message.strip_prefix("invalid_request: dream_unavailable: ") {
        HandlerError::dream_unavailable(rest.to_string())
    } else {
        HandlerError::from_dream(error)
    }
}

fn validate_dream_cli_override(cli_override: Option<&str>) -> Result<(), HandlerError> {
    let Some(name) = cli_override else {
        return Ok(());
    };
    if name == "echo" && crate::dream::orchestration::echo_cli_override_enabled() {
        return Ok(());
    }
    let registry = crate::dream::registry::HarnessCliRegistry::builtin_v0_2();
    if registry.get(name).is_some() || registry.disabled_adapters().any(|adapter| adapter.name == name) {
        Ok(())
    } else {
        Err(HandlerError::invalid_request(format!("unknown harness CLI override `{name}`")))
    }
}

async fn delta_response(
    substrate: &Substrate,
    state: &HandlerState,
    request: crate::recall::DeltaRequest,
) -> Result<ResponsePayload, HandlerError> {
    let coordination = DeltaCoordinationContext {
        config: state.coordination_config(),
        presence: state.presence(),
        claim_locks: state.claim_locks(),
        delivery_recorder: Some(state),
        peer_cooldown: Some(state),
    };
    match build_delta_response_with_coordination(substrate, request, coordination).await {
        Ok(response) => {
            state.recall.record_delta_success();
            Ok(ResponsePayload::Delta(response))
        }
        Err(error) => {
            state.recall.record_delta_failure(error.protocol_code());
            Err(HandlerError::from_recall(error))
        }
    }
}

async fn startup_response(
    substrate: &Substrate,
    state: &HandlerState,
    request: crate::recall::StartupRequest,
) -> Result<ResponsePayload, HandlerError> {
    match build_startup_response_with_coordination_config(substrate, request, state.coordination_config().clone()).await
    {
        Ok(response) => {
            record_budget_exhaustions(state, &response);
            state.recall.record_dream_question_omissions(&response.dream_question_omissions);
            state.recall.record_startup_success();
            Ok(ResponsePayload::Startup(Box::new(response)))
        }
        Err(error) => {
            state.recall.record_startup_failure(error.protocol_code());
            Err(HandlerError::from_recall(error))
        }
    }
}

fn record_budget_exhaustions(state: &HandlerState, response: &StartupResponse) {
    for omission in &response.recall_explanation.omitted {
        if omission.reason == OmissionReason::BudgetExhausted {
            state.recall.record_budget_exhausted(omission.section.as_str());
        }
    }
}

async fn search_response(
    substrate: &Substrate,
    query: &str,
    limit: Option<usize>,
    include_body: bool,
) -> Result<ResponsePayload, HandlerError> {
    let query = query.trim();
    if query.is_empty() {
        return Err(HandlerError::invalid_request("search query must not be empty"));
    }

    let limit = limit.unwrap_or(SEARCH_LIMIT_DEFAULT).min(SEARCH_LIMIT_MAX);
    let chunks = substrate
        .query_chunks(ChunkQuery { text: Some(query.to_string()), triple: None, vector: None })
        .await
        .map_err(HandlerError::substrate)?;
    let total = chunks.len();
    let mut hits = Vec::new();
    for chunk in chunks.into_iter().take(limit) {
        let body = if include_body {
            match MemoryId::try_new(chunk.memory_id.as_str().to_string()) {
                Ok(id) => substrate.read_memory_envelope(&id).await.ok().and_then(|envelope| match envelope.content {
                    MemoryContent::Plaintext(body) => Some(body),
                    MemoryContent::Ciphertext { .. } | MemoryContent::MetadataOnly => None,
                }),
                Err(_) => None,
            }
        } else {
            None
        };
        hits.push(SearchHit {
            id: chunk.memory_id.as_str().to_string(),
            summary: bounded(&chunk.text, SEARCH_SNIPPET_MAX),
            snippet: bounded(&chunk.text, SEARCH_SNIPPET_MAX),
            body,
            score: chunk.score,
        });
    }

    let guidance = if include_body {
        "Search returns bounded matching chunks; call memory_get for the bounded record preview.".to_string()
    } else {
        "Bounded snippets only; call memory_get for full body access when policy allows.".to_string()
    };
    Ok(ResponsePayload::Search(SearchResponse { hits, total, guidance }))
}

async fn get_response(
    substrate: &Substrate,
    id: &str,
    include_provenance: bool,
) -> Result<ResponsePayload, HandlerError> {
    let memory_id = MemoryId::try_new(id.to_string()).map_err(|err| HandlerError::invalid_request(err.to_string()))?;
    let envelope = substrate.read_memory_envelope(&memory_id).await.map_err(HandlerError::substrate)?;
    let provenance = include_provenance.then(|| get_provenance(&envelope.metadata));
    let body = match envelope.content {
        MemoryContent::Plaintext(body) => body,
        MemoryContent::MetadataOnly => String::new(),
        MemoryContent::Ciphertext { .. } => "[encrypted content omitted]".to_string(),
    };
    let (body, truncated) = bounded_with_truncation(&body, GET_BODY_MAX);
    Ok(ResponsePayload::Get(GetResponse {
        id: envelope.metadata.frontmatter.id.as_str().to_string(),
        summary: envelope.metadata.frontmatter.summary,
        body,
        truncated,
        provenance,
        guidance: "Returned a bounded Stream A record preview.".to_string(),
    }))
}

fn get_provenance(memory: &Memory) -> GetProvenance {
    GetProvenance {
        path: memory.path.as_ref().map(|path| path.as_str().to_string()),
        source_kind: serialized_enum_value(&memory.frontmatter.source.kind),
        source_ref: memory.frontmatter.source.reference.clone(),
        author_kind: serialized_enum_value(&memory.frontmatter.author.kind),
        harness: memory.frontmatter.author.harness.clone().or_else(|| memory.frontmatter.source.harness.clone()),
        session_id: memory
            .frontmatter
            .author
            .session_id
            .clone()
            .or_else(|| memory.frontmatter.source.session_id.clone()),
        evidence_refs: memory.frontmatter.evidence.iter().map(|evidence| evidence.reference.clone()).collect(),
    }
}

fn serialized_enum_value<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| "unknown".to_string())
}

async fn reveal_response(substrate: &Substrate, id: &str, reason: &str) -> Result<ResponsePayload, HandlerError> {
    let reason = reason.trim();
    if reason.is_empty() {
        return Err(HandlerError::invalid_request("reveal reason must not be empty"));
    }
    if reason.chars().count() > REVEAL_REASON_MAX_CHARS {
        return Err(HandlerError::invalid_request("reveal reason must be at most 512 characters"));
    }
    let memory_id = MemoryId::try_new(id.to_string()).map_err(|err| HandlerError::invalid_request(err.to_string()))?;
    let envelope = substrate.read_memory_envelope(&memory_id).await.map_err(HandlerError::substrate)?;
    let MemoryContent::Ciphertext { bytes, encryption } = envelope.content else {
        return Err(HandlerError::invalid_request("memory_reveal requires an encrypted memory"));
    };
    let encryptor = PrivacyEncryptor::new(FileKeyProvider::runtime_default(&substrate.roots().runtime));
    let body = encryptor
        .decrypt(&EncryptedPayload {
            ciphertext: bytes,
            envelope: encryption.metadata.unwrap_or_else(|| {
                serde_json::json!({
                    "scheme": encryption.scheme,
                    "recipient": encryption.recipient,
                })
            }),
        })
        .map_err(HandlerError::privacy)?;
    substrate
        .record_encrypted_content_revealed(memory_id, audit_reveal_reason(reason))
        .map_err(|err| HandlerError::substrate(format!("record encrypted reveal audit event: {err}")))?;
    let (body, truncated) = bounded_with_truncation(&body, GET_BODY_MAX);
    Ok(ResponsePayload::Reveal(RevealResponse {
        id: envelope.metadata.frontmatter.id.as_str().to_string(),
        summary: envelope.metadata.frontmatter.summary,
        body,
        truncated,
        guidance: "Returned decrypted content through explicit memory_reveal; plaintext was not re-indexed."
            .to_string(),
    }))
}

async fn write_note_response(substrate: &Substrate, text: &str) -> Result<ResponsePayload, HandlerError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(HandlerError::invalid_request("note text must not be empty"));
    }
    let privacy = classify_privacy(text, PrivacyNamespace::Agent, None)?;
    if privacy.storage_action.refuses_storage() {
        return Err(HandlerError::invalid_request("privacy refused secret note before disk effects"));
    }

    let memory_id = substrate.next_memory_id().await.map_err(HandlerError::substrate)?;
    let memory = candidate_memory(memory_id, text, privacy.storage_action);
    let id = memory.frontmatter.id.as_str().to_string();
    let summary = memory.frontmatter.summary.clone();
    write_privacy_memory(
        substrate,
        memory,
        &privacy,
        EventContext { actor: Some("memoryd-note".to_string()), reason: Some("privacy-mediated note".to_string()) },
    )
    .await?;
    Ok(ResponsePayload::WriteNote(WriteNoteResponse { id, summary }))
}

#[derive(Debug)]
struct ObserveRequestFields {
    text: String,
    kind: ObserveKind,
    entities: Vec<String>,
    cwd: String,
    session_id: String,
    harness: String,
    harness_version: Option<String>,
}

async fn observe_response(
    substrate: &Substrate,
    request: ObserveRequestFields,
) -> Result<ResponsePayload, HandlerError> {
    let text = validated_observe_text(request.text)?;
    let entities = validated_observe_entities(request.entities)?;
    let session_id = validated_observe_binding_field("session_id", request.session_id)?;
    let harness = validated_observe_binding_field("harness", request.harness)?;
    let harness_version = request
        .harness_version
        .map(|version| validated_observe_binding_field("harness_version", version))
        .transpose()?;
    let mut binding = crate::recall::binding::validate_session_fields(&request.cwd, &session_id, &harness)
        .await
        .map_err(HandlerError::from_recall)?;
    binding.harness_version = harness_version;
    let privacy = classify_privacy(&text, PrivacyNamespace::Agent, None)?;
    if privacy.storage_action.refuses_storage() {
        return Err(HandlerError::privacy("secret refused before substrate fragment write"));
    }

    let kind = request.kind;
    let (payload, classification, target) = if privacy.storage_action.requires_encryption() {
        (
            encrypted_observe_payload(substrate, &text, kind)?,
            ClassificationOutcome::RequiresEncryption,
            ObserveTarget::EncryptedSubstrate,
        )
    } else {
        (
            SubstrateFragmentPayload::Plaintext { text },
            ClassificationOutcome::Trusted,
            ObserveTarget::PlaintextSubstrate,
        )
    };
    let outcome = substrate
        .append_substrate_fragment(SubstrateFragmentAppendRequest {
            id: None,
            at: chrono::Utc::now(),
            session: Some(binding.session_id.clone()),
            harness: Some(binding.harness.clone()),
            scope: observe_scope(&binding),
            entities,
            kind,
            source_ref: Some(observe_source_ref(&binding)),
            privacy_spans: privacy_span_records(&privacy),
            payload,
            classification,
            operation_id: None,
        })
        .await
        .map_err(HandlerError::substrate)?;

    Ok(ResponsePayload::Observe(ObserveResponse { fragment_id: outcome.id, target }))
}

fn validated_observe_text(text: String) -> Result<String, HandlerError> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err(HandlerError::invalid_request("observe text must not be empty"));
    }
    if text.len() > OBSERVE_TEXT_MAX_BYTES {
        return Err(HandlerError::invalid_request("observe text exceeds 16 KiB"));
    }
    Ok(text)
}

fn validated_observe_entities(entities: Vec<String>) -> Result<Vec<String>, HandlerError> {
    if entities.len() > OBSERVE_ENTITIES_MAX {
        return Err(HandlerError::invalid_request("observe entities exceeds 32 entries"));
    }
    for entity in &entities {
        validate_observe_entity_id(entity)?;
    }
    Ok(entities)
}

fn validate_observe_entity_id(entity: &str) -> Result<(), HandlerError> {
    if entity.trim() != entity {
        return Err(HandlerError::invalid_request(
            "observe entity ids must not include leading or trailing whitespace",
        ));
    }
    if entity.len() > OBSERVE_ENTITY_MAX_BYTES {
        return Err(HandlerError::invalid_request("observe entity exceeds 128 UTF-8 bytes"));
    }
    let Some(body) = entity.strip_prefix("ent_") else {
        return Err(HandlerError::invalid_request("observe entities must be canonical ent_ ids"));
    };
    if body.is_empty() || body.len() > OBSERVE_ENTITY_BODY_MAX_BYTES {
        return Err(HandlerError::invalid_request("observe entities must be canonical ent_ ids"));
    }
    if !body.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b':' | b'-')) {
        return Err(HandlerError::invalid_request("observe entities must be canonical ent_ ids"));
    }
    validate_observe_metadata_is_safe("observe entity", entity)?;
    Ok(())
}

fn validated_observe_binding_field(name: &str, value: String) -> Result<String, HandlerError> {
    if value.trim() != value {
        return Err(HandlerError::invalid_request(format!("{name} must not include leading or trailing whitespace")));
    }
    if value.is_empty() {
        return Err(HandlerError::invalid_request(format!("{name} must be non-empty")));
    }
    if value.len() > OBSERVE_BINDING_FIELD_MAX_BYTES {
        return Err(HandlerError::invalid_request(format!("{name} must be at most 128 bytes")));
    }
    if !value.bytes().all(is_observe_binding_byte) {
        return Err(HandlerError::invalid_request(format!("{name} must contain only safe id characters")));
    }
    validate_observe_metadata_is_safe(name, &value)?;
    Ok(value)
}

fn is_observe_binding_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b':' | b'-')
}

fn validated_claim_lock_identity_field(name: &str, value: String) -> Result<String, HandlerError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(HandlerError::invalid_request(format!("{name} must be non-empty")));
    }
    if trimmed.len() > CLAIM_LOCK_IDENTITY_MAX_BYTES {
        return Err(HandlerError::invalid_request(format!("{name} must be at most 128 bytes")));
    }
    if !trimmed.bytes().all(is_observe_binding_byte) {
        return Err(HandlerError::invalid_request(format!("{name} must contain only safe id characters")));
    }
    validate_observe_metadata_is_safe(name, trimmed)?;
    if contains_secret_or_pii_marker(trimmed) {
        return Err(HandlerError::invalid_request(format!("{name} must not contain sensitive material")));
    }
    Ok(trimmed.to_string())
}

fn validate_observe_metadata_is_safe(name: &str, value: &str) -> Result<(), HandlerError> {
    if !is_safe_plaintext_for_indexing(value) || contains_observe_metadata_canary(value) {
        return Err(HandlerError::invalid_request(format!("{name} must not contain sensitive material")));
    }
    Ok(())
}

fn contains_observe_metadata_canary(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    value.contains('@')
        || contains_aws_access_key(value)
        || contains_us_phone_number(value)
        || lower.contains("ghp_")
        || lower.contains("sk_live_")
}

fn contains_aws_access_key(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.windows(4).enumerate().any(|(index, window)| {
        window == b"AKIA"
            && bytes.get(index + 4..index + 20).is_some_and(|suffix| suffix.iter().all(u8::is_ascii_alphanumeric))
    })
}

fn contains_us_phone_number(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.windows(12).any(|window| {
        window[0..3].iter().all(u8::is_ascii_digit)
            && window[3] == b'-'
            && window[4..7].iter().all(u8::is_ascii_digit)
            && window[7] == b'-'
            && window[8..12].iter().all(u8::is_ascii_digit)
    })
}

fn observe_scope(binding: &crate::recall::SessionBinding) -> String {
    binding
        .project
        .as_ref()
        .map(|project| format!("project:{}", project.canonical_id))
        .unwrap_or_else(|| "agent".to_string())
}

fn observe_source_ref(binding: &crate::recall::SessionBinding) -> String {
    format!("session:{}:memory_observe", binding.session_id)
}

fn encrypted_observe_payload(
    substrate: &Substrate,
    text: &str,
    kind: ObserveKind,
) -> Result<SubstrateFragmentPayload, HandlerError> {
    let encryptor = PrivacyEncryptor::new(FileKeyProvider::runtime_default(&substrate.roots().runtime));
    let encrypted = encryptor.encrypt(text).map_err(HandlerError::privacy)?;
    Ok(SubstrateFragmentPayload::Encrypted {
        encryption: SubstrateFragmentEncryption {
            recipient: encrypted.envelope.get("recipient").and_then(Value::as_str).unwrap_or("age-x25519").to_string(),
            ciphertext_b64: base64_encode(&encrypted.ciphertext),
        },
        descriptor: content_aware_encrypted_observe_descriptor(text, kind),
    })
}

fn content_aware_encrypted_observe_descriptor(text: &str, kind: ObserveKind) -> EncryptedSubstrateDescriptor {
    let tag = observe_kind_tag(kind);
    let fallback_tags = vec![tag.to_string()];
    let fallback_summary = format!("encrypted {tag} substrate fragment");
    let projection =
        safe_descriptor_projection(&DeterministicPrivacyClassifier::new(), text, &fallback_summary, &fallback_tags);
    EncryptedSubstrateDescriptor { summary_safe: projection.summary_safe, tag_safe: projection.tag_safe }
}

fn observe_kind_tag(kind: ObserveKind) -> &'static str {
    match kind {
        ObserveKind::Observation => "observation",
        ObserveKind::Pattern => "pattern",
        ObserveKind::Signal => "signal",
    }
}

fn privacy_span_records(privacy: &PrivacyDecision) -> Vec<PrivacySpanRecord> {
    privacy
        .spans
        .iter()
        .map(|span| PrivacySpanRecord {
            label: serde_json::to_value(span.label)
                .ok()
                .and_then(|value| value.as_str().map(str::to_string))
                .unwrap_or_else(|| format!("{:?}", span.label)),
            start: span.start,
            end: span.end,
        })
        .collect()
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);
        encoded.push(TABLE[(b0 >> 2) as usize] as char);
        encoded.push(TABLE[(((b0 & 0b0000_0011) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            encoded.push(TABLE[(((b1 & 0b0000_1111) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            encoded.push('=');
        }
        if chunk.len() > 2 {
            encoded.push(TABLE[(b2 & 0b0011_1111) as usize] as char);
        } else {
            encoded.push('=');
        }
    }
    encoded
}

async fn governance_write_response(
    substrate: &Substrate,
    request: GovernanceWriteRequest,
) -> Result<ResponsePayload, HandlerError> {
    let input = GovernanceWriteInput::parse(GovernanceWriteInputParts {
        body: request.body,
        title: request.title,
        tags: request.tags,
        meta: request.meta,
        source: MetaSource::McpHumanWrite,
    })?;
    let privacy = classify_input_privacy(&input)?;
    if let Some(response) = input.privacy_refusal(&privacy) {
        return Ok(ResponsePayload::GovernanceWrite(response));
    }

    let id = substrate.next_memory_id().await.map_err(HandlerError::substrate)?;
    let candidate = input.candidate(id.as_str());
    let (policies, policy_source) = match load_policy_set(substrate.roots().repo.as_path()) {
        Ok(loaded) => loaded,
        Err(error) => {
            return Ok(ResponsePayload::GovernanceWrite(policy_refusal(input.response_namespace(), error.message)))
        }
    };
    let tombstones = match load_tombstone_index(substrate.roots().repo.as_path()) {
        Ok(index) => index,
        Err(error) => {
            return Ok(ResponsePayload::GovernanceWrite(tombstone_refusal(
                input.response_namespace(),
                error.message,
                policy_source,
            )));
        }
    };
    let active = active_memory_summaries(substrate).await?;
    let engine = governance_engine(GovernanceEngineInput {
        policies,
        active,
        tombstones,
        tiebreak_mode: TiebreakMode::Unclear,
        allow_top_k: false,
        repo_root: substrate.roots().repo.clone(),
    });
    let decision = engine.evaluate_write(&candidate);
    let response =
        execute_write_decision(substrate, WriteExecution { input, id, decision, policy_source, privacy }).await?;
    Ok(ResponsePayload::GovernanceWrite(response))
}

async fn governance_supersede_response(
    substrate: &Substrate,
    state: Option<&HandlerState>,
    request: GovernanceSupersedeRequest,
) -> Result<ResponsePayload, HandlerError> {
    let GovernanceSupersedeRequest { old_id, content, reason, meta } = request;
    let old_memory_id =
        MemoryId::try_new(old_id.clone()).map_err(|err| HandlerError::invalid_request(err.to_string()))?;
    let input = GovernanceWriteInput::parse(GovernanceWriteInputParts {
        body: content,
        title: None,
        tags: Vec::new(),
        meta,
        source: MetaSource::Default,
    })?;
    let privacy = classify_input_privacy(&input)?;
    if let Some(refusal) = input.privacy_refusal(&privacy) {
        return Ok(ResponsePayload::GovernanceSupersede(GovernanceSupersedeResponse {
            status: GovernanceStatus::Refused,
            new_id: None,
            old_id: Some(old_id),
            reason: refusal.reason,
            chain: None,
            policy_applied: refusal.policy_applied,
            policy_source: refusal.policy_source,
            warning: None,
        }));
    }

    let new_id = substrate.next_memory_id().await.map_err(HandlerError::substrate)?;
    let candidate = input.candidate(new_id.as_str());
    let (policies, policy_source) = match load_policy_set(substrate.roots().repo.as_path()) {
        Ok(loaded) => loaded,
        Err(error) => {
            return Ok(ResponsePayload::GovernanceSupersede(GovernanceSupersedeResponse {
                status: GovernanceStatus::Refused,
                new_id: None,
                old_id: Some(old_id),
                reason: Some(GovernanceRefusalReason::Policy),
                chain: None,
                policy_applied: None,
                policy_source: Some(error.message),
                warning: None,
            }));
        }
    };
    let tombstones = match load_tombstone_index(substrate.roots().repo.as_path()) {
        Ok(index) => index,
        Err(error) => {
            return Ok(ResponsePayload::GovernanceSupersede(GovernanceSupersedeResponse {
                status: GovernanceStatus::Refused,
                new_id: None,
                old_id: Some(old_id),
                reason: Some(GovernanceRefusalReason::Tombstone),
                chain: None,
                policy_applied: None,
                policy_source: Some(error.message),
                warning: None,
            }));
        }
    };
    let old_envelope = substrate.read_memory_envelope(&old_memory_id).await.map_err(HandlerError::substrate)?;

    // The contradiction detector compares the new candidate against the old body. For
    // encrypted-old memories we can't read the body without an explicit reveal, so we
    // skip body-based contradiction and let the explicit supersede call carry intent:
    // the user has named `old_id`, so we trust the target and only verify the new
    // content passes grounding + policy on its own.
    let old_plaintext_body = match &old_envelope.content {
        MemoryContent::Plaintext(body) => Some(body.clone()),
        MemoryContent::Ciphertext { .. } | MemoryContent::MetadataOnly => None,
    };
    let old_is_encrypted = old_plaintext_body.is_none();

    let (active, tiebreak_mode, allow_top_k) = match &old_plaintext_body {
        Some(body) => (
            vec![existing_summary_from_memory(old_envelope.metadata.clone(), body.clone())],
            TiebreakMode::Contradiction { existing_id: old_id.clone() },
            true,
        ),
        None => (Vec::new(), TiebreakMode::Unclear, false),
    };
    let engine = governance_engine(GovernanceEngineInput {
        policies,
        active,
        tombstones,
        tiebreak_mode,
        allow_top_k,
        repo_root: substrate.roots().repo.clone(),
    });
    let decision = engine.evaluate_write(&candidate);

    let policy_applied = match (old_is_encrypted, decision) {
        (false, GovernanceWriteDecision::Supersession { existing_id, policy_applied, .. }) => {
            if existing_id != old_id {
                return Ok(ResponsePayload::GovernanceSupersede(GovernanceSupersedeResponse {
                    status: GovernanceStatus::Refused,
                    new_id: None,
                    old_id: Some(old_id),
                    reason: Some(GovernanceRefusalReason::Contradiction),
                    chain: None,
                    policy_applied: Some(policy_applied),
                    policy_source: Some(policy_source_string(policy_source)),
                    warning: None,
                }));
            }
            policy_applied
        }
        (false, other) => {
            return Ok(ResponsePayload::GovernanceSupersede(supersede_refusal(old_id, other, policy_source)));
        }
        // Encrypted-old path: `active = []` means contradiction detection can't fire,
        // so engine.evaluate_write returns Promoted or Candidate when the candidate
        // passes grounding + policy. Either is "the new content is acceptable; proceed
        // with the explicit supersede". Refusals (missing grounding, secret material,
        // tombstone match) still surface.
        (true, GovernanceWriteDecision::Promoted { policy_applied, .. })
        | (true, GovernanceWriteDecision::Candidate { policy_applied, .. })
        | (true, GovernanceWriteDecision::Supersession { policy_applied, .. }) => policy_applied,
        (true, other) => {
            return Ok(ResponsePayload::GovernanceSupersede(supersede_refusal(old_id, other, policy_source)));
        }
    };

    let mut replacement = input.to_memory(
        new_id.clone(),
        GovernedLifecycle::new(MemoryStatus::Active, TrustLevel::Trusted, policy_applied.clone()),
        &privacy,
    );
    replacement.frontmatter.supersedes.push(old_memory_id.clone());

    let claim_lock = match state {
        Some(state) => acquire_claim_lock_for_supersede(substrate, state, &old_memory_id, &input.meta),
        None => SupersedeClaimLock::inactive(),
    };

    // Write the replacement + mark the old superseded. Stream A's `supersede_memory`
    // is plaintext-only (`read_memory_with_hash` skips encrypted/ paths and
    // `write_memory` refuses RequiresEncryption classifications), so for the three
    // mixed cases we route the writes ourselves and call the existing `write_privacy_memory`
    // and `update_encrypted_memory_metadata` primitives — same building blocks the
    // governance write + forget paths already use for encrypted records.
    let new_is_encrypted = privacy.storage_action.requires_encryption();
    if !old_is_encrypted && !new_is_encrypted {
        substrate
            .supersede_memory(SubstrateSupersedeRequest {
                old_id: old_memory_id.clone(),
                replacement,
                reason: reason.clone(),
                classification: privacy.tier.classification(),
                allow_best_effort_durability: true,
            })
            .await
            .map_err(HandlerError::substrate)?;
    } else {
        write_privacy_memory(
            substrate,
            replacement,
            &privacy,
            EventContext { actor: Some("memoryd-supersede".to_string()), reason: Some(reason.clone()) },
        )
        .await?;
        mark_old_superseded(
            substrate,
            MarkOldSuperseded { old_id: &old_memory_id, new_id: &new_id, old_is_encrypted, reason: &reason },
        )
        .await?;
    }

    let warning = claim_lock.release_after_success();

    Ok(ResponsePayload::GovernanceSupersede(GovernanceSupersedeResponse {
        status: GovernanceStatus::Promoted,
        new_id: Some(new_id.as_str().to_string()),
        old_id: Some(old_id.clone()),
        reason: None,
        chain: Some(serde_json::json!({ "supersedes": [old_id] })),
        policy_applied: Some(policy_applied),
        policy_source: Some(policy_source_string(policy_source)),
        warning,
    }))
}

struct MarkOldSuperseded<'a> {
    old_id: &'a MemoryId,
    new_id: &'a MemoryId,
    old_is_encrypted: bool,
    reason: &'a str,
}

/// Mark the old memory as `Superseded` and append `new_id` to its `superseded_by`
/// chain. Used by the mixed-encryption supersede paths, where Stream A's atomic
/// `supersede_memory` can't drive the two-write pair because either the old read
/// or the new write would land under `encrypted/`. Routes through the appropriate
/// Stream A primitive based on whether the old record is encrypted on disk.
async fn mark_old_superseded(
    substrate: &Substrate,
    MarkOldSuperseded { old_id, new_id, old_is_encrypted, reason }: MarkOldSuperseded<'_>,
) -> Result<(), HandlerError> {
    let new_id_for_chain = new_id.clone();
    if old_is_encrypted {
        substrate
            .update_encrypted_memory_metadata(old_id, |old| {
                old.frontmatter.status = MemoryStatus::Superseded;
                old.frontmatter.updated_at = chrono::Utc::now();
                if !old.frontmatter.superseded_by.contains(&new_id_for_chain) {
                    old.frontmatter.superseded_by.push(new_id_for_chain);
                }
            })
            .await
            .map_err(|err| HandlerError::substrate(format!("update encrypted metadata: {err:?}")))?;
        return Ok(());
    }
    // Plaintext old + encrypted new: rewrite the plaintext old in place. We pass
    // `expected_base_hash: None` here — Stream A's public surface doesn't expose
    // the read-hash, and the supersede call is daemon-mediated and synchronous,
    // so the TOCTOU window is tight. The same trade-off applies to the equivalent
    // path in `governance_forget_response` and `review_decision_response`.
    let mut old_memory = substrate
        .read_memory(old_id)
        .await
        .map_err(|err| HandlerError::substrate(format!("read old memory for supersede: {err:?}")))?;
    old_memory.frontmatter.status = MemoryStatus::Superseded;
    old_memory.frontmatter.updated_at = chrono::Utc::now();
    if !old_memory.frontmatter.superseded_by.contains(&new_id_for_chain) {
        old_memory.frontmatter.superseded_by.push(new_id_for_chain);
    }
    substrate
        .write_memory(SubstrateWriteRequest {
            operation_id: None,
            memory: old_memory,
            expected_base_hash: None,
            write_mode: WriteMode::ReplaceExisting,
            index_projection: None,
            event_context: EventContext {
                actor: Some("memoryd-supersede".to_string()),
                reason: Some(reason.to_string()),
            },
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Trusted,
        })
        .await
        .map_err(|err| HandlerError::substrate(format!("mark old superseded: {err:?}")))?;
    Ok(())
}

fn acquire_claim_lock_for_supersede<'a>(
    substrate: &Substrate,
    state: &'a HandlerState,
    memory_id: &MemoryId,
    meta: &GovernanceMeta,
) -> SupersedeClaimLock<'a> {
    if state.effective_coordination_level(meta) < 2 {
        return SupersedeClaimLock::inactive();
    }

    let result = state.claim_locks.acquire(ClaimLockAcquireRequest::new(
        memory_id.as_str(),
        meta.session_id.as_str(),
        meta.harness.as_str(),
        state.claim_lock_ttl(),
    ));
    match result {
        ClaimLockAcquireResult::Acquired(_) => SupersedeClaimLock::acquired(state, memory_id, meta),
        ClaimLockAcquireResult::AlreadyHeld(_) => SupersedeClaimLock::already_held(state, memory_id, meta),
        ClaimLockAcquireResult::Contended(contention) => {
            let holder = contention.holder_label();
            let contender = contention.contender_label();
            if let Err(error) = substrate.record_event_best_effort(EventKind::ClaimLockContention {
                memory_id: memory_id.clone(),
                holder: holder.clone(),
                contender,
            }) {
                tracing::warn!(
                    memory_id = memory_id.as_str(),
                    "claim-lock contention event append failed; proceeding with advisory warning: {error}"
                );
            }

            SupersedeClaimLock::contended(
                state,
                SupersedeClaimIdentity::new(memory_id, meta),
                contention.holder,
                ClaimLockWarning { code: contention.warning_code.to_string(), message: contention.message, holder },
            )
        }
    }
}

enum ClaimLockRollback {
    None,
    ReleaseAcquired,
    RestorePrevious(ClaimLockInfo),
}

struct SupersedeClaimLock<'a> {
    state: Option<&'a HandlerState>,
    memory_id: String,
    harness: String,
    session_id: String,
    release_on_success: bool,
    rollback: ClaimLockRollback,
    warning: Option<ClaimLockWarning>,
    completed: bool,
}

impl<'a> SupersedeClaimLock<'a> {
    fn inactive() -> Self {
        Self {
            state: None,
            memory_id: String::new(),
            harness: String::new(),
            session_id: String::new(),
            release_on_success: false,
            rollback: ClaimLockRollback::None,
            warning: None,
            completed: true,
        }
    }

    fn acquired(state: &'a HandlerState, memory_id: &MemoryId, meta: &GovernanceMeta) -> Self {
        Self::active(state, SupersedeClaimIdentity::new(memory_id, meta), ClaimLockRollback::ReleaseAcquired, None)
    }

    fn already_held(state: &'a HandlerState, memory_id: &MemoryId, meta: &GovernanceMeta) -> Self {
        Self::active(state, SupersedeClaimIdentity::new(memory_id, meta), ClaimLockRollback::None, None)
    }

    fn contended(
        state: &'a HandlerState,
        identity: SupersedeClaimIdentity,
        previous_holder: ClaimLockInfo,
        warning: ClaimLockWarning,
    ) -> Self {
        Self::active(state, identity, ClaimLockRollback::RestorePrevious(previous_holder), Some(warning))
    }

    fn active(
        state: &'a HandlerState,
        identity: SupersedeClaimIdentity,
        rollback: ClaimLockRollback,
        warning: Option<ClaimLockWarning>,
    ) -> Self {
        Self {
            state: Some(state),
            memory_id: identity.memory_id,
            harness: identity.harness,
            session_id: identity.session_id,
            release_on_success: true,
            rollback,
            warning,
            completed: false,
        }
    }

    fn release_after_success(mut self) -> Option<ClaimLockWarning> {
        if self.release_on_success {
            if let Some(state) = self.state {
                state.claim_locks.release(&self.memory_id, &self.harness, &self.session_id);
            }
        }
        self.completed = true;
        self.warning.take()
    }
}

struct SupersedeClaimIdentity {
    memory_id: String,
    harness: String,
    session_id: String,
}

impl SupersedeClaimIdentity {
    fn new(memory_id: &MemoryId, meta: &GovernanceMeta) -> Self {
        Self {
            memory_id: memory_id.as_str().to_string(),
            harness: meta.harness.clone(),
            session_id: meta.session_id.clone(),
        }
    }
}

impl Drop for SupersedeClaimLock<'_> {
    fn drop(&mut self) {
        if self.completed {
            return;
        }

        let Some(state) = self.state else {
            return;
        };

        match &self.rollback {
            ClaimLockRollback::None => {}
            ClaimLockRollback::ReleaseAcquired => {
                state.claim_locks.release(&self.memory_id, &self.harness, &self.session_id);
            }
            ClaimLockRollback::RestorePrevious(previous_holder) => {
                state.claim_locks.release(&self.memory_id, &self.harness, &self.session_id);
                let _restored = state.claim_locks.restore(previous_holder.clone());
            }
        }
    }
}

async fn governance_forget_response(
    substrate: &Substrate,
    id: String,
    reason: String,
) -> Result<ResponsePayload, HandlerError> {
    if reason.trim().is_empty() {
        return Err(HandlerError::invalid_request("forget reason must not be empty"));
    }
    let reason = sanitize_forget_reason(&reason);
    let memory_id = MemoryId::try_new(id.clone()).map_err(|err| HandlerError::invalid_request(err.to_string()))?;
    let envelope = substrate.read_memory_envelope(&memory_id).await.map_err(HandlerError::substrate)?;
    let tombstone_claim = match &envelope.content {
        MemoryContent::Plaintext(body) if !body.is_empty() => body.clone(),
        MemoryContent::Ciphertext { .. } | MemoryContent::MetadataOnly | MemoryContent::Plaintext(_) => {
            envelope.metadata.frontmatter.summary.clone()
        }
    };
    substrate
        .tombstone_memory(TombstoneRequest { id: memory_id, reason: reason.clone() })
        .await
        .map_err(HandlerError::substrate)?;
    write_tombstone_rule(substrate.roots().repo.as_path(), &envelope.metadata, &tombstone_claim, &reason)?;
    Ok(ResponsePayload::GovernanceForget(GovernanceForgetResponse {
        status: GovernanceStatus::Tombstoned,
        id,
        tombstone_ref: Some("tombstone:stream-a".to_string()),
        reason: None,
    }))
}

async fn execute_write_decision(
    substrate: &Substrate,
    execution: WriteExecution,
) -> Result<GovernanceWriteResponse, HandlerError> {
    let WriteExecution { input, id, decision, policy_source, privacy } = execution;
    match decision {
        GovernanceWriteDecision::Promoted { namespace, policy_applied, .. } => {
            let memory = input.to_memory(
                id.clone(),
                GovernedLifecycle::new(MemoryStatus::Active, TrustLevel::Trusted, policy_applied.clone()),
                &privacy,
            );
            write_governed_memory(substrate, memory, &privacy).await?;
            Ok(GovernanceWriteResponse {
                status: GovernanceStatus::Promoted,
                id: Some(id.as_str().to_string()),
                namespace: Some(namespace),
                reason: None,
                next_actions: Vec::new(),
                policy_applied: Some(policy_applied),
                policy_source: Some(policy_source_string(policy_source)),
                existing_id: None,
            })
        }
        GovernanceWriteDecision::Candidate { reason, policy_applied, .. } => {
            let memory = input.to_memory(
                id.clone(),
                GovernedLifecycle::new(MemoryStatus::Candidate, TrustLevel::Candidate, policy_applied.clone()),
                &privacy,
            );
            write_governed_memory(substrate, memory, &privacy).await?;
            Ok(GovernanceWriteResponse {
                status: GovernanceStatus::Candidate,
                id: Some(id.as_str().to_string()),
                namespace: Some(input.response_namespace()),
                reason: None,
                next_actions: vec![reason],
                policy_applied: Some(policy_applied),
                policy_source: Some(policy_source_string(policy_source)),
                existing_id: None,
            })
        }
        GovernanceWriteDecision::Quarantined { reason, policy_applied, .. } => {
            let memory = input.to_memory(
                id.clone(),
                GovernedLifecycle::new(MemoryStatus::Quarantined, TrustLevel::Quarantined, policy_applied.clone()),
                &privacy,
            );
            write_governed_memory(substrate, memory, &privacy).await?;
            Ok(GovernanceWriteResponse {
                status: GovernanceStatus::Quarantined,
                id: Some(id.as_str().to_string()),
                namespace: Some(input.response_namespace()),
                reason: None,
                next_actions: vec![reason],
                policy_applied: Some(policy_applied),
                policy_source: Some(policy_source_string(policy_source)),
                existing_id: None,
            })
        }
        GovernanceWriteDecision::Duplicate { existing_id, .. } => Ok(GovernanceWriteResponse {
            status: GovernanceStatus::Promoted,
            id: Some(existing_id.clone()),
            namespace: Some(input.response_namespace()),
            reason: None,
            next_actions: Vec::new(),
            policy_applied: None,
            policy_source: Some(policy_source_string(policy_source)),
            existing_id: Some(existing_id),
        }),
        GovernanceWriteDecision::Refinement { existing_id, .. } => Ok(GovernanceWriteResponse {
            status: GovernanceStatus::Promoted,
            id: Some(existing_id.clone()),
            namespace: Some(input.response_namespace()),
            reason: None,
            next_actions: vec!["merge_evidence".to_string()],
            policy_applied: None,
            policy_source: Some(policy_source_string(policy_source)),
            existing_id: Some(existing_id),
        }),
        GovernanceWriteDecision::Supersession { existing_id, policy_applied, .. } => Ok(GovernanceWriteResponse {
            status: GovernanceStatus::Candidate,
            id: Some(id.as_str().to_string()),
            namespace: Some(input.response_namespace()),
            reason: None,
            next_actions: vec!["memory_supersede".to_string()],
            policy_applied: Some(policy_applied),
            policy_source: Some(policy_source_string(policy_source)),
            existing_id: Some(existing_id),
        }),
        GovernanceWriteDecision::Refused { reason, .. } => Ok(GovernanceWriteResponse {
            status: GovernanceStatus::Refused,
            id: None,
            namespace: Some(input.response_namespace()),
            reason: Some(reason),
            next_actions: Vec::new(),
            policy_applied: None,
            policy_source: Some(policy_source_string(policy_source)),
            existing_id: None,
        }),
    }
}

async fn write_governed_memory(
    substrate: &Substrate,
    memory: Memory,
    privacy: &PrivacyDecision,
) -> Result<(), HandlerError> {
    write_privacy_memory(
        substrate,
        memory,
        privacy,
        EventContext {
            actor: Some("memoryd-governance".to_string()),
            reason: Some("governed privacy-mediated write".to_string()),
        },
    )
    .await
}

async fn write_privacy_memory(
    substrate: &Substrate,
    mut memory: Memory,
    privacy: &PrivacyDecision,
    event_context: EventContext,
) -> Result<(), HandlerError> {
    if privacy.storage_action.refuses_storage() {
        return Err(HandlerError::invalid_request("privacy refused secret before disk effects"));
    }
    attach_privacy_scan(&mut memory, privacy);
    if privacy.storage_action.requires_encryption() {
        let encryptor = PrivacyEncryptor::new(FileKeyProvider::runtime_default(&substrate.roots().runtime));
        let encrypted = encryptor.encrypt(&memory.body).map_err(HandlerError::privacy)?;
        memory.frontmatter.extras.insert("encryption".to_string(), encrypted.envelope);
        let safe_index_projection = safe_index_projection(&memory);
        substrate
            .write_encrypted(EncryptedWriteRequest {
                operation_id: None,
                metadata_memory: memory,
                ciphertext: encrypted.ciphertext,
                // Stream D: encrypted records index only descriptors already proven safe.
                // Do NOT project raw or masked body text here; see stream-d-security-review P0.
                safe_index_projection,
                event_context,
                allow_best_effort_durability: true,
                classification: ClassificationOutcome::RequiresEncryption,
            })
            .await
            .map(|_| ())
            .map_err(HandlerError::substrate)
    } else {
        substrate
            .write_memory(SubstrateWriteRequest {
                operation_id: None,
                memory,
                expected_base_hash: None,
                write_mode: WriteMode::CreateNew,
                index_projection: None,
                event_context,
                allow_best_effort_durability: true,
                classification: privacy.tier.classification(),
            })
            .await
            .map(|_| ())
            .map_err(HandlerError::substrate)
    }
}

fn classify_input_privacy(input: &GovernanceWriteInput) -> Result<PrivacyDecision, HandlerError> {
    classify_privacy(&input.privacy_scan_text(), input.privacy_namespace(), input.caller_sensitivity())
}

fn classify_privacy(
    text: &str,
    namespace: PrivacyNamespace,
    caller: Option<CallerSensitivity>,
) -> Result<PrivacyDecision, HandlerError> {
    DeterministicPrivacyClassifier::new().classify(text, namespace, caller).map_err(HandlerError::privacy)
}

fn attach_privacy_scan(memory: &mut Memory, privacy: &PrivacyDecision) {
    memory.frontmatter.extras.insert(
        "privacy_scan".to_string(),
        serde_json::to_value(&privacy.scan).expect("privacy scan always serializes"),
    );
}

async fn review_queue_response(
    substrate: &Substrate,
    state: &HandlerState,
    limit: Option<usize>,
) -> Result<ResponsePayload, HandlerError> {
    let mut envelopes = Vec::new();
    let mut bodies_by_id: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for path in memory_substrate::tree::relative_memory_paths(substrate.roots().repo.as_path()) {
        let repo_path = RepoPath::new(path.to_string_lossy().replace('\\', "/"));
        let envelope = substrate.read_path_envelope(&repo_path).await.map_err(HandlerError::substrate)?;
        let id = envelope.metadata.frontmatter.id.as_str().to_string();
        bodies_by_id.insert(id, body_text_for_review(&envelope.content));
        envelopes.push(review_envelope_from_memory(envelope.metadata));
    }

    let mut queue = ReviewQueue::from_memory_envelopes(envelopes);
    if over_threshold(&queue) {
        state.emit_notification(NotificationEvent::ReviewQueueOverThreshold {
            count: queue.items.len(),
            threshold: REVIEW_QUEUE_DOGFOOD_THRESHOLD,
        });
    }
    queue.items.truncate(limit.unwrap_or(REVIEW_QUEUE_LIMIT_DEFAULT).min(REVIEW_QUEUE_LIMIT_MAX));

    let mut items = queue
        .items
        .into_iter()
        .map(|item| {
            let (body, body_truncated) = bodies_by_id
                .remove(&item.id)
                .map(|body| bounded_with_truncation(&body, REVIEW_QUEUE_BODY_MAX))
                .unwrap_or_default();
            ReviewQueueItemResponse {
                body,
                body_truncated,
                id: item.id,
                summary: bounded(&item.summary, REVIEW_QUEUE_SUMMARY_MAX),
                status: item.status.as_str().to_string(),
                policy_applied: bounded(&item.policy_applied, REVIEW_QUEUE_POLICY_MAX),
                reason: item.reason.map(|reason| bounded(&reason, REVIEW_QUEUE_REASON_MAX)),
                next_actions: item
                    .next_actions
                    .into_iter()
                    .take(4)
                    .map(|action| bounded(&action, REVIEW_QUEUE_ACTION_MAX))
                    .collect(),
            }
        })
        .collect::<Vec<_>>();
    while serialized_payload_len(&ResponsePayload::ReviewQueue(ReviewQueueResponse { items: items.clone() }))
        > REVIEW_RESPONSE_FRAME_BUDGET
    {
        if items.pop().is_none() {
            break;
        }
    }

    Ok(ResponsePayload::ReviewQueue(ReviewQueueResponse { items }))
}

async fn review_decision_response(
    substrate: &Substrate,
    id: &str,
    decision: ReviewDecision,
) -> Result<ResponsePayload, HandlerError> {
    let memory_id = MemoryId::try_new(id.to_string()).map_err(|err| HandlerError::invalid_request(err.to_string()))?;
    let envelope = substrate.read_memory_envelope(&memory_id).await.map_err(HandlerError::substrate)?;
    if !matches!(envelope.content, MemoryContent::Plaintext(_)) {
        return Err(HandlerError::invalid_request(
            "encrypted review decisions require an encrypted lifecycle update API",
        ));
    }
    let mut memory = envelope.metadata;
    if !matches!(memory.frontmatter.status, MemoryStatus::Candidate | MemoryStatus::Quarantined)
        || !review_queue_contains(&memory)
    {
        return Err(HandlerError::invalid_request("memory is not eligible for the review queue"));
    }
    if matches!((&decision, memory.frontmatter.status), (ReviewDecision::Approve, MemoryStatus::Quarantined)) {
        return Err(HandlerError::invalid_request("quarantined memories must be resubmitted through governance"));
    }
    if matches!(decision, ReviewDecision::Approve)
        && rehydration::requires_rehydration(&memory)
        && rehydration::verify_dream_candidate(substrate, &memory).await.is_err()
    {
        let summary = bounded(&memory.frontmatter.summary, REVIEW_DECISION_SUMMARY_MAX);
        quarantine_for_grounding_rehydration(substrate, memory).await?;
        let response = ReviewDecisionResponse { id: id.to_string(), status: "quarantined".to_string(), summary };
        return Ok(ResponsePayload::ReviewApprove(response));
    }
    let status = decision.apply(&mut memory);
    let summary = bounded(&memory.frontmatter.summary, REVIEW_DECISION_SUMMARY_MAX);

    substrate
        .write_memory(SubstrateWriteRequest {
            operation_id: None,
            memory,
            expected_base_hash: None,
            write_mode: WriteMode::ReplaceExisting,
            index_projection: None,
            event_context: EventContext {
                actor: Some("memoryd-review".to_string()),
                reason: Some(format!("review {status}")),
            },
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Trusted,
        })
        .await
        .map_err(HandlerError::substrate)?;

    let response = ReviewDecisionResponse { id: id.to_string(), status: status.to_string(), summary };
    match decision {
        ReviewDecision::Approve => Ok(ResponsePayload::ReviewApprove(response)),
        ReviewDecision::Reject { .. } => Ok(ResponsePayload::ReviewReject(response)),
    }
}

async fn quarantine_for_grounding_rehydration(substrate: &Substrate, mut memory: Memory) -> Result<(), HandlerError> {
    memory.frontmatter.updated_at = chrono::Utc::now();
    memory.frontmatter.status = MemoryStatus::Quarantined;
    memory.frontmatter.trust_level = TrustLevel::Quarantined;
    memory.frontmatter.requires_user_confirmation = true;
    memory.frontmatter.review_state = Some("quarantined".to_string());
    memory.frontmatter.retrieval_policy.index_body = false;
    memory.frontmatter.retrieval_policy.index_embeddings = false;
    memory.frontmatter.write_policy.human_review_required = true;
    memory
        .frontmatter
        .extras
        .insert("governance_reason".to_string(), serde_json::json!("grounding_rehydration_failed"));
    memory.frontmatter.merge_diagnostics = Some(serde_json::json!({
        "human_reason": "grounding_rehydration_failed",
        "preserved_sources": [],
        "lifecycle_notes": ["dream grounding rehydration failed before review approval"],
        "evidence_near_duplicates": []
    }));

    substrate
        .write_memory(SubstrateWriteRequest {
            operation_id: None,
            memory,
            expected_base_hash: None,
            write_mode: WriteMode::ReplaceExisting,
            index_projection: None,
            event_context: EventContext {
                actor: Some("memoryd-review".to_string()),
                reason: Some("review grounding_rehydration_failed".to_string()),
            },
            allow_best_effort_durability: true,
            classification: ClassificationOutcome::Trusted,
        })
        .await
        .map_err(HandlerError::substrate)?;
    Ok(())
}

fn body_text_for_review(content: &MemoryContent) -> String {
    match content {
        MemoryContent::Plaintext(text) => text.clone(),
        MemoryContent::Ciphertext { .. } => "[encrypted memory — use reveal flow to view body]".to_string(),
        MemoryContent::MetadataOnly => "[metadata-only memory — body not stored]".to_string(),
    }
}

fn review_envelope_from_memory(memory: Memory) -> ReviewMemoryEnvelope {
    ReviewMemoryEnvelope {
        id: memory.frontmatter.id.as_str().to_string(),
        summary: memory.frontmatter.summary,
        status: serde_json::to_value(memory.frontmatter.status)
            .ok()
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_else(|| "active".to_string()),
        requires_user_confirmation: memory.frontmatter.requires_user_confirmation,
        review_state: memory.frontmatter.review_state,
        policy_applied: memory.frontmatter.write_policy.policy_applied,
        reason: memory.frontmatter.extras.get("governance_reason").and_then(|value| value.as_str()).map(str::to_string),
    }
}

fn review_queue_contains(memory: &Memory) -> bool {
    let envelope = review_envelope_from_memory(memory.clone());
    ReviewQueue::from_memory_envelopes(vec![envelope])
        .items
        .iter()
        .any(|item| item.id == memory.frontmatter.id.as_str())
}

fn serialized_payload_len(payload: &ResponsePayload) -> usize {
    serde_json::to_vec(payload).map_or(MAX_FRAME_BYTES, |bytes| bytes.len())
}

fn load_policy_set(repo: &Path) -> Result<(PolicySet, PolicySource), HandlerError> {
    let policy_dir = repo.join("policies");
    let has_yaml = std::fs::read_dir(&policy_dir)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .any(|entry| entry.path().extension().is_some_and(|extension| extension == "yaml"));

    if has_yaml {
        match PolicySet::load_from_dir(&policy_dir) {
            Ok(policies) => return Ok((policies, PolicySource::Disk)),
            Err(error) => return Err(HandlerError::invalid_request(format!("invalid governance policy: {error}"))),
        }
    }

    Ok((PolicySet::builtin(), PolicySource::BuiltInFallback))
}

fn load_tombstone_index(repo: &Path) -> Result<TombstoneIndex, HandlerError> {
    let tombstone_dir = repo.join("tombstones");
    if !tombstone_dir.exists() {
        return Ok(TombstoneIndex::default());
    }
    TombstoneIndex::load_jsonl_dir(&tombstone_dir)
        .map_err(|error| HandlerError::invalid_request(format!("invalid tombstone rules: {error}")))
}

fn policy_refusal(namespace: String, message: String) -> GovernanceWriteResponse {
    GovernanceWriteResponse {
        status: GovernanceStatus::Refused,
        id: None,
        namespace: Some(namespace),
        reason: Some(GovernanceRefusalReason::Policy),
        next_actions: vec![message],
        policy_applied: None,
        policy_source: None,
        existing_id: None,
    }
}

fn tombstone_refusal(namespace: String, message: String, policy_source: PolicySource) -> GovernanceWriteResponse {
    GovernanceWriteResponse {
        status: GovernanceStatus::Refused,
        id: None,
        namespace: Some(namespace),
        reason: Some(GovernanceRefusalReason::Tombstone),
        next_actions: vec![message],
        policy_applied: None,
        policy_source: Some(policy_source_string(policy_source)),
        existing_id: None,
    }
}

fn supersede_refusal(
    old_id: String,
    decision: GovernanceWriteDecision,
    policy_source: PolicySource,
) -> GovernanceSupersedeResponse {
    let (reason, policy_applied) = match decision {
        GovernanceWriteDecision::Refused { reason, .. } => (reason, None),
        GovernanceWriteDecision::Duplicate { .. } => (GovernanceRefusalReason::Superseded, None),
        GovernanceWriteDecision::Refinement { .. } => (GovernanceRefusalReason::Contradiction, None),
        GovernanceWriteDecision::Candidate { policy_applied, .. }
        | GovernanceWriteDecision::Quarantined { policy_applied, .. }
        | GovernanceWriteDecision::Promoted { policy_applied, .. } => {
            (GovernanceRefusalReason::Contradiction, Some(policy_applied))
        }
        GovernanceWriteDecision::Supersession { policy_applied, .. } => {
            (GovernanceRefusalReason::Contradiction, Some(policy_applied))
        }
    };
    GovernanceSupersedeResponse {
        status: GovernanceStatus::Refused,
        new_id: None,
        old_id: Some(old_id),
        reason: Some(reason),
        chain: None,
        policy_applied,
        policy_source: Some(policy_source_string(policy_source)),
        warning: None,
    }
}

fn existing_summary_from_memory(memory: Memory, body: String) -> ExistingMemorySummary {
    ExistingMemorySummary::new(
        memory.frontmatter.id.as_str().to_string(),
        namespace_for_frontmatter(&memory.frontmatter),
        body,
        1.0,
    )
    .with_entity_ids(entity_ids(&memory.frontmatter))
}

fn write_tombstone_rule(repo: &Path, memory: &Memory, claim: &str, reason: &str) -> Result<(), HandlerError> {
    let tombstone_dir = repo.join("tombstones");
    std::fs::create_dir_all(&tombstone_dir)
        .map_err(|error| HandlerError::substrate(format!("create tombstone dir: {error}")))?;
    let key = memory_governance::CandidateTombstoneKey::from_claim(claim, entity_ids(&memory.frontmatter))
        .with_target_memory_id(memory.frontmatter.id.as_str().to_string());
    let rule = TombstoneRule {
        id: format!("tomb_{}", memory.frontmatter.id.as_str()),
        target_memory_id: Some(memory.frontmatter.id.as_str().to_string()),
        content_hash: key.content_hash,
        entity_hash: key.entity_hash,
        reason: TombstoneKind::UserForget,
        reason_text: Some(reason.to_string()),
        active: true,
    };
    let path = tombstone_dir.join("memoryd-forget.jsonl");
    let mut file =
        OpenOptions::new().create(true).append(true).open(&path).map_err(|error| {
            HandlerError::substrate(format!("open tombstone rule file {}: {error}", path.display()))
        })?;
    let line = serde_json::to_string(&rule)
        .map_err(|error| HandlerError::substrate(format!("serialize tombstone rule: {error}")))?;
    writeln!(file, "{line}")
        .map_err(|error| HandlerError::substrate(format!("append tombstone rule file {}: {error}", path.display())))?;
    Ok(())
}

struct GovernanceEngineInput {
    policies: PolicySet,
    active: Vec<ExistingMemorySummary>,
    tombstones: TombstoneIndex,
    tiebreak_mode: TiebreakMode,
    allow_top_k: bool,
    repo_root: PathBuf,
}

fn governance_engine(
    input: GovernanceEngineInput,
) -> GovernanceEngine<MemorydSimilaritySearch, MemorydTiebreaker, MemorydSessionResolver, ArtifactStore> {
    GovernanceEngine::new(
        input.policies,
        GroundingVerifier::new_with_web_capture_resolver(
            FileSourceResolver,
            MemorydSessionResolver,
            ArtifactStore::new(input.repo_root),
        ),
        input.tombstones,
        GovernanceProviders::new(
            MemorydSimilaritySearch { active: input.active, allow_top_k: input.allow_top_k },
            MemorydTiebreaker { tiebreak_mode: input.tiebreak_mode },
        ),
    )
}

async fn active_memory_summaries(substrate: &Substrate) -> Result<Vec<ExistingMemorySummary>, HandlerError> {
    let mut summaries = Vec::new();
    for path in memory_substrate::tree::relative_memory_paths(substrate.roots().repo.as_path()) {
        let repo_path = RepoPath::new(path.to_string_lossy().replace('\\', "/"));
        let envelope = substrate.read_path_envelope(&repo_path).await.map_err(HandlerError::substrate)?;
        if !matches!(envelope.metadata.frontmatter.status, MemoryStatus::Active) {
            continue;
        }
        let MemoryContent::Plaintext(body) = envelope.content else {
            continue;
        };
        summaries.push(
            ExistingMemorySummary::new(
                envelope.metadata.frontmatter.id.as_str().to_string(),
                namespace_for_frontmatter(&envelope.metadata.frontmatter),
                body,
                1.0,
            )
            .with_entity_ids(entity_ids(&envelope.metadata.frontmatter)),
        );
    }
    Ok(summaries)
}

#[derive(Clone, Debug)]
struct MemorydSimilaritySearch {
    active: Vec<ExistingMemorySummary>,
    allow_top_k: bool,
}

impl SimilaritySearch for MemorydSimilaritySearch {
    fn find_active_by_claim_hash(&self, candidate: &CandidateMemory) -> Option<ExistingMemorySummary> {
        self.active
            .iter()
            .find(|memory| {
                memory.canonical_claim_hash() == candidate.canonical_claim_hash()
                    && memory.entity_hash() == candidate.entity_hash()
                    && memory.namespace() == candidate.namespace()
            })
            .cloned()
    }

    fn top_k(&self, _candidate: &CandidateMemory, limit: usize) -> Vec<ExistingMemorySummary> {
        if !self.allow_top_k {
            return Vec::new();
        }
        self.active.iter().take(limit).cloned().collect()
    }
}

#[derive(Clone, Debug)]
struct MemorydTiebreaker {
    tiebreak_mode: TiebreakMode,
}

#[derive(Clone, Debug)]
enum TiebreakMode {
    Unclear,
    Contradiction { existing_id: String },
}

impl ContradictionTiebreaker for MemorydTiebreaker {
    fn tiebreak(&self, _candidate: &CandidateMemory, _hits: &[ExistingMemorySummary]) -> TiebreakOutcome {
        match &self.tiebreak_mode {
            TiebreakMode::Unclear => TiebreakOutcome::Unclear,
            TiebreakMode::Contradiction { existing_id } => {
                TiebreakOutcome::Contradiction { existing_id: existing_id.clone() }
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct MemorydSessionResolver;

impl SessionSpawnResolver for MemorydSessionResolver {
    fn spawned_in_session(&self, _spawn_id: &str) -> bool {
        false
    }
}

#[derive(Clone, Debug)]
struct GovernanceWriteRequest {
    body: String,
    title: Option<String>,
    tags: Vec<String>,
    meta: Value,
}

#[derive(Clone, Debug)]
struct GovernanceSupersedeRequest {
    old_id: String,
    content: String,
    reason: String,
    meta: Value,
}

#[derive(Clone, Debug)]
struct WriteExecution {
    input: GovernanceWriteInput,
    id: MemoryId,
    decision: GovernanceWriteDecision,
    policy_source: PolicySource,
    privacy: PrivacyDecision,
}

#[derive(Clone, Debug)]
struct GovernedLifecycle {
    status: MemoryStatus,
    trust_level: TrustLevel,
    policy_applied: String,
}

impl GovernedLifecycle {
    fn new(status: MemoryStatus, trust_level: TrustLevel, policy_applied: String) -> Self {
        Self { status, trust_level, policy_applied }
    }
}

#[derive(Clone, Debug)]
struct GovernanceWriteInput {
    body: String,
    title: Option<String>,
    tags: Vec<String>,
    meta: GovernanceMeta,
}

struct GovernanceWriteInputParts {
    body: String,
    title: Option<String>,
    tags: Vec<String>,
    meta: Value,
    source: MetaSource,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct GovernanceMeta {
    namespace: GovernanceNamespace,
    #[serde(rename = "type")]
    memory_type: GovernanceMemoryType,
    summary: Option<String>,
    confidence: f64,
    sensitivity: Option<GovernanceSensitivity>,
    source_kind: GovernanceSourceKindMeta,
    source_ref: Option<String>,
    explicit_user_context: bool,
    privacy_descriptors: Option<PrivacyDescriptors>,
    #[serde(default = "default_supersede_session_id")]
    session_id: String,
    #[serde(default = "default_supersede_harness")]
    harness: String,
    concurrent_session_mode: Option<ConcurrentSessionMode>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct PrivacyDescriptors {
    subject: Option<String>,
    role: Option<String>,
    organization: Option<String>,
    office: Option<String>,
    value_kind: Option<String>,
    lookup_hints: Vec<String>,
}

impl PrivacyDescriptors {
    fn values(&self) -> Vec<String> {
        let mut values = [
            self.subject.clone(),
            self.role.clone(),
            self.organization.clone(),
            self.office.clone(),
            self.value_kind.clone(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        values.extend(self.lookup_hints.iter().cloned());
        values
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GovernanceNamespace {
    Me,
    Project,
    Agent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum GovernanceMemoryType {
    Project,
    Claim,
    Decision,
    Pattern,
    Playbook,
    Procedure,
    Artifact,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum GovernanceSensitivity {
    Public,
    Internal,
    Confidential,
    Personal,
    Sensitive,
    Secret,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum GovernanceSourceKindMeta {
    User,
    AgentPrimary,
    Subagent,
    File,
    WebCapture,
}

impl Default for GovernanceMeta {
    fn default() -> Self {
        Self {
            namespace: GovernanceNamespace::Project,
            memory_type: GovernanceMemoryType::Project,
            summary: None,
            confidence: 0.85,
            sensitivity: None,
            source_kind: GovernanceSourceKindMeta::User,
            source_ref: None,
            explicit_user_context: false,
            privacy_descriptors: None,
            session_id: default_supersede_session_id(),
            harness: default_supersede_harness(),
            concurrent_session_mode: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MetaSource {
    Default,
    McpHumanWrite,
}

impl GovernanceMeta {
    fn empty_for(source: MetaSource) -> Self {
        match source {
            MetaSource::Default => Self::default(),
            MetaSource::McpHumanWrite => Self::for_mcp_human_write(),
        }
    }

    fn for_mcp_human_write() -> Self {
        Self { explicit_user_context: true, confidence: 0.9, ..Self::default() }
    }
}

fn default_supersede_session_id() -> String {
    DEFAULT_SUPERSEDE_SESSION_ID.to_owned()
}

fn default_supersede_harness() -> String {
    DEFAULT_SUPERSEDE_HARNESS.to_owned()
}

impl Default for GovernanceNamespace {
    fn default() -> Self {
        Self::Project
    }
}

impl<'de> Deserialize<'de> for GovernanceNamespace {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        match value.as_str() {
            "me" | "user" => Ok(Self::Me),
            "project" => Ok(Self::Project),
            "agent" => Ok(Self::Agent),
            other => Err(serde::de::Error::custom(format!("unsupported namespace `{other}`"))),
        }
    }
}

fn parse_governance_meta(meta: Value, source: MetaSource) -> Result<GovernanceMeta, HandlerError> {
    if meta.is_null() {
        return Ok(GovernanceMeta::empty_for(source));
    }

    let mut meta = meta;
    if source == MetaSource::McpHumanWrite {
        let Value::Object(fields) = &mut meta else {
            return Err(HandlerError::invalid_request("governance meta must be an object or null"));
        };
        fields.entry("explicit_user_context".to_string()).or_insert(Value::Bool(true));
        fields.entry("confidence".to_string()).or_insert(serde_json::json!(0.9));
    }
    serde_json::from_value(meta).map_err(|err| HandlerError::invalid_request(err.to_string()))
}

impl GovernanceWriteInput {
    fn parse(parts: GovernanceWriteInputParts) -> Result<Self, HandlerError> {
        let GovernanceWriteInputParts { body, title, tags, meta, source } = parts;
        let body = body.trim().to_string();
        if body.is_empty() {
            return Err(HandlerError::invalid_request("memory body must not be empty"));
        }
        let mut meta = parse_governance_meta(meta, source)?;
        meta.session_id = validated_claim_lock_identity_field("session_id", meta.session_id)?;
        meta.harness = validated_claim_lock_identity_field("harness", meta.harness)?;
        if !meta.confidence.is_finite() || !(0.0..=1.0).contains(&meta.confidence) {
            return Err(HandlerError::invalid_request("confidence must be finite and between 0.0 and 1.0"));
        }
        Ok(Self { body, title, tags, meta })
    }

    fn privacy_scan_text(&self) -> String {
        let mut fields = vec![self.body.as_str()];
        if let Some(title) = &self.title {
            fields.push(title.as_str());
        }
        if let Some(summary) = &self.meta.summary {
            fields.push(summary.as_str());
        }
        if !matches!(self.meta.source_kind, GovernanceSourceKindMeta::WebCapture) {
            if let Some(source_ref) = &self.meta.source_ref {
                fields.push(source_ref.as_str());
            }
        }
        fields.extend(self.tags.iter().map(String::as_str));
        let mut text = fields.join("\n");
        if let Some(descriptors) = &self.meta.privacy_descriptors {
            for value in descriptors.values() {
                text.push('\n');
                text.push_str(&value);
            }
        }
        text
    }

    fn privacy_refusal(&self, privacy: &PrivacyDecision) -> Option<GovernanceWriteResponse> {
        match privacy.storage_action {
            PrivacyStorageAction::Refuse => Some(GovernanceWriteResponse {
                status: GovernanceStatus::Refused,
                id: None,
                namespace: Some(self.response_namespace()),
                reason: Some(GovernanceRefusalReason::Privacy),
                next_actions: vec!["remove_secret_material".to_string()],
                policy_applied: None,
                policy_source: None,
                existing_id: None,
            }),
            PrivacyStorageAction::Plaintext | PrivacyStorageAction::EncryptAtRest => None,
        }
    }

    fn candidate(&self, id: &str) -> CandidateMemory {
        let mut candidate =
            CandidateMemory::new(id, self.response_namespace(), self.body.clone(), self.governance_scope())
                .with_confidence(self.meta.confidence as f32)
                .with_sources(self.governance_sources());
        if self.meta.explicit_user_context {
            candidate = candidate.with_explicit_user_context();
        }
        candidate
    }

    fn to_memory(&self, id: MemoryId, lifecycle: GovernedLifecycle, privacy: &PrivacyDecision) -> Memory {
        let now = chrono::Utc::now();
        let summary = self.summary(privacy.storage_action);
        let requires_review = matches!(lifecycle.status, MemoryStatus::Candidate | MemoryStatus::Quarantined);
        let review_state = match lifecycle.status {
            MemoryStatus::Candidate => Some("candidate".to_string()),
            MemoryStatus::Quarantined => Some("quarantined".to_string()),
            _ => None,
        };
        let mut extras = BTreeMap::new();
        if matches!(lifecycle.status, MemoryStatus::Quarantined) {
            extras.insert("governance_reason".to_string(), serde_json::json!("governance quarantine"));
        }

        let sensitivity = privacy.tier.persisted_sensitivity().unwrap_or(Sensitivity::Internal);
        let encrypted = privacy.storage_action.requires_encryption();
        let indexable = !encrypted && !matches!(lifecycle.status, MemoryStatus::Quarantined);
        if let Some(descriptors) = self.safe_privacy_descriptors_value() {
            extras.insert("privacy_descriptors".to_string(), descriptors);
        }
        Memory {
            frontmatter: Frontmatter {
                schema_version: memory_substrate::SUBSTRATE_SCHEMA_VERSION,
                id: id.clone(),
                memory_type: self.memory_type(),
                scope: self.substrate_scope(),
                summary,
                confidence: self.meta.confidence,
                original_confidence: None,
                trust_level: lifecycle.trust_level,
                sensitivity,
                status: lifecycle.status,
                created_at: now,
                updated_at: now,
                observed_at: None,
                author: self.author(),
                namespace: self.substrate_namespace(),
                canonical_namespace_id: self.substrate_namespace(),
                tags: self.persisted_tags(privacy.storage_action),
                entities: Vec::new(),
                aliases: Vec::new(),
                source: self.substrate_source(privacy.storage_action),
                evidence: Vec::new(),
                requires_user_confirmation: requires_review,
                review_state,
                supersedes: Vec::new(),
                superseded_by: Vec::new(),
                related: Vec::new(),
                tombstone_events: Vec::new(),
                retrieval_policy: RetrievalPolicy {
                    passive_recall: !matches!(lifecycle.status, MemoryStatus::Quarantined),
                    max_scope: self.substrate_scope(),
                    mask_personal_for_synthesis: encrypted,
                    index_body: indexable,
                    index_embeddings: indexable,
                },
                write_policy: WritePolicy {
                    human_review_required: requires_review,
                    policy_applied: lifecycle.policy_applied,
                    expected_base_hash: None,
                },
                merge_diagnostics: matches!(lifecycle.status, MemoryStatus::Quarantined).then(|| {
                    serde_json::json!({
                        "human_reason": "governance quarantine",
                        "preserved_sources": [],
                        "lifecycle_notes": [],
                        "evidence_near_duplicates": []
                    })
                }),
                extras,
            },
            body: self.body.clone(),
            path: Some(self.repo_path(id.as_str())),
        }
    }

    fn summary(&self, storage_action: PrivacyStorageAction) -> String {
        let candidate = self.meta.summary.clone().or_else(|| self.title.clone());
        if storage_action.requires_encryption() {
            return candidate
                .filter(|value| is_safe_plaintext_for_indexing(value))
                .unwrap_or_else(|| "encrypted memory".to_string());
        }
        candidate.unwrap_or_else(|| bounded(&self.body, 120))
    }

    fn persisted_tags(&self, storage_action: PrivacyStorageAction) -> Vec<String> {
        if storage_action.requires_encryption() {
            self.tags.iter().filter(|tag| is_safe_plaintext_for_indexing(tag)).cloned().collect()
        } else {
            self.tags.clone()
        }
    }

    fn response_namespace(&self) -> String {
        match self.meta.namespace {
            GovernanceNamespace::Me => "me".to_string(),
            GovernanceNamespace::Project => "project".to_string(),
            GovernanceNamespace::Agent => "agent".to_string(),
        }
    }

    fn governance_scope(&self) -> memory_governance::Scope {
        match self.meta.namespace {
            GovernanceNamespace::Me => memory_governance::Scope::Me,
            GovernanceNamespace::Project => memory_governance::Scope::Project,
            GovernanceNamespace::Agent => memory_governance::Scope::Agent,
        }
    }

    fn privacy_namespace(&self) -> PrivacyNamespace {
        match self.meta.namespace {
            GovernanceNamespace::Me => PrivacyNamespace::Me,
            GovernanceNamespace::Project => PrivacyNamespace::Project,
            GovernanceNamespace::Agent => PrivacyNamespace::Agent,
        }
    }

    fn caller_sensitivity(&self) -> Option<CallerSensitivity> {
        self.meta.sensitivity.map(|sensitivity| match sensitivity {
            GovernanceSensitivity::Public => CallerSensitivity::Public,
            GovernanceSensitivity::Internal => CallerSensitivity::Internal,
            GovernanceSensitivity::Confidential => CallerSensitivity::Confidential,
            GovernanceSensitivity::Personal => CallerSensitivity::Personal,
            GovernanceSensitivity::Sensitive => CallerSensitivity::Sensitive,
            GovernanceSensitivity::Secret => CallerSensitivity::Secret,
        })
    }

    fn substrate_scope(&self) -> Scope {
        match self.meta.namespace {
            GovernanceNamespace::Me => Scope::User,
            GovernanceNamespace::Project => Scope::Project,
            GovernanceNamespace::Agent => Scope::Agent,
        }
    }

    fn substrate_namespace(&self) -> Option<String> {
        matches!(self.meta.namespace, GovernanceNamespace::Project).then(|| DEFAULT_PROJECT_NAMESPACE.to_string())
    }

    fn governance_sources(&self) -> Vec<GovernanceSource> {
        let kind = match self.meta.source_kind {
            GovernanceSourceKindMeta::User => GovernanceSourceKind::User,
            GovernanceSourceKindMeta::Subagent => GovernanceSourceKind::Subagent,
            GovernanceSourceKindMeta::WebCapture => GovernanceSourceKind::WebCapture,
            GovernanceSourceKindMeta::AgentPrimary | GovernanceSourceKindMeta::File => {
                GovernanceSourceKind::AgentPrimary
            }
        };
        vec![GovernanceSource::new(kind, self.meta.source_ref.clone())]
    }

    fn substrate_source(&self, storage_action: PrivacyStorageAction) -> Source {
        let kind = match self.meta.source_kind {
            GovernanceSourceKindMeta::User => SourceKind::User,
            GovernanceSourceKindMeta::Subagent => SourceKind::AgentSubagent,
            GovernanceSourceKindMeta::WebCapture => SourceKind::Web,
            GovernanceSourceKindMeta::File => SourceKind::File,
            GovernanceSourceKindMeta::AgentPrimary => SourceKind::AgentPrimary,
        };
        Source {
            kind,
            reference: if storage_action.requires_encryption() {
                self.meta
                    .source_ref
                    .clone()
                    .filter(|reference| is_safe_plaintext_for_indexing(reference))
                    .or_else(|| Some("memoryd.governance".to_string()))
            } else {
                self.meta.source_ref.clone().or_else(|| Some("memoryd.governance".to_string()))
            },
            harness: None,
            harness_version: None,
            session_id: None,
            subagent_id: None,
            device: None,
        }
    }

    fn safe_privacy_descriptors_value(&self) -> Option<Value> {
        let descriptors = self.meta.privacy_descriptors.as_ref()?;
        let mut object = serde_json::Map::new();
        insert_safe_descriptor(&mut object, "subject", descriptors.subject.as_deref());
        insert_safe_descriptor(&mut object, "role", descriptors.role.as_deref());
        insert_safe_descriptor(&mut object, "organization", descriptors.organization.as_deref());
        insert_safe_descriptor(&mut object, "office", descriptors.office.as_deref());
        insert_safe_descriptor(&mut object, "value_kind", descriptors.value_kind.as_deref());
        let hints = descriptors
            .lookup_hints
            .iter()
            .filter(|hint| is_safe_plaintext_for_indexing(hint))
            .cloned()
            .map(Value::String)
            .collect::<Vec<_>>();
        if !hints.is_empty() {
            object.insert("lookup_hints".to_string(), Value::Array(hints));
        }
        (!object.is_empty()).then_some(Value::Object(object))
    }

    fn author(&self) -> Author {
        match self.meta.source_kind {
            GovernanceSourceKindMeta::User => Author {
                kind: AuthorKind::User,
                user_handle: Some("memoryd-user".to_string()),
                harness: None,
                harness_version: None,
                session_id: None,
                subagent_id: None,
                phase: None,
                component: None,
            },
            GovernanceSourceKindMeta::Subagent => Author {
                kind: AuthorKind::Subagent,
                user_handle: None,
                harness: Some("memoryd".to_string()),
                harness_version: Some(env!("CARGO_PKG_VERSION").to_string()),
                session_id: Some("memoryd-session".to_string()),
                subagent_id: Some("memoryd-subagent".to_string()),
                phase: None,
                component: None,
            },
            GovernanceSourceKindMeta::AgentPrimary
            | GovernanceSourceKindMeta::File
            | GovernanceSourceKindMeta::WebCapture => Author {
                kind: AuthorKind::Agent,
                user_handle: None,
                harness: Some("memoryd".to_string()),
                harness_version: Some(env!("CARGO_PKG_VERSION").to_string()),
                session_id: Some("memoryd-session".to_string()),
                subagent_id: None,
                phase: None,
                component: None,
            },
        }
    }

    fn memory_type(&self) -> MemoryType {
        match self.meta.memory_type {
            GovernanceMemoryType::Claim => MemoryType::Claim,
            GovernanceMemoryType::Decision => MemoryType::Decision,
            GovernanceMemoryType::Pattern => MemoryType::Pattern,
            GovernanceMemoryType::Playbook => MemoryType::Playbook,
            GovernanceMemoryType::Procedure => MemoryType::Procedure,
            GovernanceMemoryType::Artifact => MemoryType::Artifact,
            GovernanceMemoryType::Project => MemoryType::Project,
        }
    }

    fn repo_path(&self, id: &str) -> RepoPath {
        match self.meta.namespace {
            GovernanceNamespace::Me => RepoPath::new(format!("me/knowledge/{id}.md")),
            GovernanceNamespace::Project => {
                RepoPath::new(format!("projects/{DEFAULT_PROJECT_NAMESPACE}/decisions/{id}.md"))
            }
            GovernanceNamespace::Agent => RepoPath::new(format!("agent/patterns/{id}.md")),
        }
    }
}

fn policy_source_string(source: PolicySource) -> String {
    match source {
        PolicySource::Disk => "disk".to_string(),
        PolicySource::BuiltInFallback => "built_in_fallback".to_string(),
    }
}

fn namespace_for_frontmatter(frontmatter: &Frontmatter) -> String {
    match frontmatter.scope {
        Scope::Project => "project".to_string(),
        Scope::Agent | Scope::Subagent => "agent".to_string(),
        Scope::User => "me".to_string(),
        Scope::Org => "project".to_string(),
    }
}

fn entity_ids(frontmatter: &Frontmatter) -> Vec<String> {
    frontmatter.entities.iter().map(|entity| entity.id.clone()).collect()
}

enum ReviewDecision {
    Approve,
    Reject { reason: String },
}

impl ReviewDecision {
    fn apply(&self, memory: &mut Memory) -> &'static str {
        memory.frontmatter.updated_at = chrono::Utc::now();
        memory.frontmatter.requires_user_confirmation = false;
        memory.frontmatter.write_policy.human_review_required = false;
        match self {
            Self::Approve => {
                memory.frontmatter.status = MemoryStatus::Active;
                memory.frontmatter.trust_level = TrustLevel::Trusted;
                memory.frontmatter.review_state = None;
                "approved"
            }
            Self::Reject { reason } => {
                memory.frontmatter.status = MemoryStatus::Archived;
                memory.frontmatter.review_state = Some("rejected".to_string());
                memory.frontmatter.retrieval_policy.index_body = false;
                memory.frontmatter.retrieval_policy.index_embeddings = false;
                memory.frontmatter.extras.insert("review_rejection_reason".to_string(), serde_json::json!(reason));
                "rejected"
            }
        }
    }
}

fn candidate_memory(id: MemoryId, text: &str, storage_action: PrivacyStorageAction) -> Memory {
    let now = chrono::Utc::now();
    let sensitivity = Sensitivity::Internal;
    let encrypted = storage_action.requires_encryption();
    Memory {
        frontmatter: Frontmatter {
            schema_version: memory_substrate::SUBSTRATE_SCHEMA_VERSION,
            id: id.clone(),
            memory_type: MemoryType::Pattern,
            scope: Scope::Agent,
            summary: if encrypted { "encrypted note".to_string() } else { bounded(text, 120) },
            confidence: 0.5,
            original_confidence: None,
            trust_level: TrustLevel::Candidate,
            sensitivity,
            status: MemoryStatus::Candidate,
            created_at: now,
            updated_at: now,
            observed_at: None,
            author: Author {
                kind: AuthorKind::System,
                user_handle: None,
                harness: None,
                harness_version: None,
                session_id: None,
                subagent_id: None,
                phase: None,
                component: Some("memoryd".to_string()),
            },
            namespace: None,
            canonical_namespace_id: None,
            tags: if encrypted { Vec::new() } else { vec!["candidate".to_string(), "memoryd-note".to_string()] },
            entities: Vec::new(),
            aliases: Vec::new(),
            source: Source {
                kind: SourceKind::Import,
                reference: Some("memoryd.write_note".to_string()),
                harness: None,
                harness_version: None,
                session_id: None,
                subagent_id: None,
                device: None,
            },
            evidence: Vec::new(),
            requires_user_confirmation: true,
            review_state: Some("candidate".to_string()),
            supersedes: Vec::new(),
            superseded_by: Vec::new(),
            related: Vec::new(),
            tombstone_events: Vec::new(),
            retrieval_policy: RetrievalPolicy {
                passive_recall: true,
                max_scope: Scope::Agent,
                mask_personal_for_synthesis: encrypted,
                index_body: !encrypted,
                index_embeddings: !encrypted,
            },
            write_policy: WritePolicy {
                human_review_required: true,
                policy_applied: "memoryd-candidate-v1".to_string(),
                expected_base_hash: None,
            },
            merge_diagnostics: None,
            extras: BTreeMap::new(),
        },
        body: text.to_string(),
        path: Some(RepoPath::new(format!("agent/patterns/{}.md", id.as_str()))),
    }
}

fn insert_safe_descriptor(object: &mut serde_json::Map<String, Value>, key: &str, value: Option<&str>) {
    if let Some(value) = value.filter(|value| is_safe_plaintext_for_indexing(value)) {
        object.insert(key.to_string(), Value::String(value.to_string()));
    }
}

fn is_safe_plaintext_for_indexing(text: &str) -> bool {
    matches!(safe_plaintext_fragment(&DeterministicPrivacyClassifier::new(), text), SafeFragmentDecision::Allow)
}

fn safe_index_projection(memory: &Memory) -> Option<IndexProjection> {
    let mut fragments = Vec::new();
    if !memory.frontmatter.summary.starts_with("encrypted ") {
        fragments.push(memory.frontmatter.summary.clone());
    }
    fragments.extend(memory.frontmatter.tags.iter().cloned());
    if let Some(reference) = &memory.frontmatter.source.reference {
        if reference != "memoryd.governance" && reference != "memoryd.write_note" {
            fragments.push(reference.clone());
        }
    }
    if let Some(descriptors) = memory.frontmatter.extras.get("privacy_descriptors") {
        collect_descriptor_strings(descriptors, &mut fragments);
    }
    let safe_body = fragments
        .into_iter()
        .map(|fragment| fragment.trim().to_string())
        .filter(|fragment| !fragment.is_empty() && is_safe_plaintext_for_indexing(fragment))
        .collect::<Vec<_>>()
        .join("\n");
    (!safe_body.is_empty()).then_some(IndexProjection { safe_body: Some(safe_body) })
}

fn collect_descriptor_strings(value: &Value, output: &mut Vec<String>) {
    match value {
        Value::String(value) => output.push(value.clone()),
        Value::Array(values) => values.iter().for_each(|value| collect_descriptor_strings(value, output)),
        Value::Object(values) => values.values().for_each(|value| collect_descriptor_strings(value, output)),
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn bounded(text: &str, max_chars: usize) -> String {
    bounded_with_truncation(text, max_chars).0
}

fn audit_reveal_reason(reason: &str) -> String {
    let bounded_reason = bounded(reason, REVEAL_REASON_MAX_CHARS);
    let Ok(privacy) = classify_privacy(&bounded_reason, PrivacyNamespace::Project, None) else {
        return "privacy-redacted reveal reason".to_owned();
    };
    redact_sensitive_privacy_spans(&bounded_reason, &privacy)
}

fn redact_sensitive_privacy_spans(text: &str, privacy: &PrivacyDecision) -> String {
    let mut spans = privacy
        .spans
        .iter()
        .filter(|span| {
            let action = span.label.storage_action();
            action.requires_encryption() || action.refuses_storage()
        })
        .collect::<Vec<_>>();
    if spans.is_empty() {
        return text.to_owned();
    }

    spans.sort_by_key(|span| (span.start, span.end));
    let mut output = String::with_capacity(text.len());
    let mut cursor = 0usize;
    for span in spans {
        let start = span.start.min(text.len());
        let end = span.end.min(text.len());
        if start < cursor {
            continue;
        }
        if start > end || !text.is_char_boundary(start) || !text.is_char_boundary(end) {
            return "privacy-redacted reveal reason".to_owned();
        }
        output.push_str(&text[cursor..start]);
        output.push_str("[redacted]");
        cursor = end;
    }
    output.push_str(&text[cursor..]);
    if output.trim().is_empty() {
        "privacy-redacted reveal reason".to_owned()
    } else {
        output
    }
}

fn bounded_with_truncation(text: &str, max_chars: usize) -> (String, bool) {
    let mut chars = text.chars();
    let bounded: String = chars.by_ref().take(max_chars).collect();
    let truncated = chars.next().is_some();
    (bounded, truncated)
}

fn resolve_memoryd_web_binary() -> Result<PathBuf, String> {
    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(sibling) = current_exe.parent().map(|dir| dir.join("memoryd-web")).filter(|path| path.is_file()) {
            return Ok(sibling);
        }
    }
    let Some(path_env) = std::env::var_os("PATH") else {
        return Err("memoryd-web binary not found on PATH".to_owned());
    };
    std::env::split_paths(&path_env)
        .map(|dir| dir.join("memoryd-web"))
        .find(|path| path.is_file())
        .ok_or_else(|| "memoryd-web binary not found on PATH".to_owned())
}

#[derive(Debug)]
struct HandlerError {
    code: String,
    message: String,
    retryable: bool,
}

impl HandlerError {
    fn invalid_request(message: impl Into<String>) -> Self {
        Self { code: "invalid_request".to_string(), message: message.into(), retryable: false }
    }

    fn dream_unavailable(message: impl Into<String>) -> Self {
        Self { code: "dream_unavailable".to_string(), message: message.into(), retryable: true }
    }

    fn dream_disabled(message: impl Into<String>) -> Self {
        Self { code: "dream_disabled".to_string(), message: message.into(), retryable: false }
    }

    fn web_unavailable(message: impl Into<String>) -> Self {
        Self { code: "web_unavailable".to_string(), message: message.into(), retryable: false }
    }

    fn port_in_use(message: impl Into<String>) -> Self {
        Self { code: "port_in_use".to_string(), message: message.into(), retryable: false }
    }

    fn substrate(error: impl std::fmt::Display) -> Self {
        Self { code: "substrate_error".to_string(), message: error.to_string(), retryable: true }
    }

    fn privacy(error: impl std::fmt::Display) -> Self {
        Self { code: "privacy_error".to_string(), message: bounded(&error.to_string(), 240), retryable: false }
    }

    fn source_capture(error: SourceError) -> Self {
        let code = match &error {
            SourceError::InvalidId(_)
            | SourceError::InvalidSourceRef(_)
            | SourceError::Unsupported(_)
            | SourceError::UrlSafety(_)
            | SourceError::Privacy(_)
            | SourceError::ExcerptNotFound(_) => "invalid_request",
            SourceError::Io(_) | SourceError::Json(_) | SourceError::Integrity(_) | SourceError::CaptureFailed(_) => {
                "source_capture_failed"
            }
        };
        Self { code: code.to_string(), message: bounded(&error.to_string(), 240), retryable: false }
    }

    fn trust_artifact(error: crate::trust_artifact::TrustArtifactError) -> Self {
        match error {
            crate::trust_artifact::TrustArtifactError::MemoryNotFound(memory_id) => Self {
                code: "not_found".to_string(),
                message: format!("memory {} was not found", memory_id.as_str()),
                retryable: false,
            },
            crate::trust_artifact::TrustArtifactError::ReadMemory {
                id,
                source: memory_substrate::ReadError::NotFound(_),
            } => Self {
                code: "not_found".to_string(),
                message: format!("memory {} was not found", id.as_str()),
                retryable: false,
            },
            other => Self {
                code: "trust_artifact_error".to_string(),
                message: bounded(&other.to_string(), 240),
                retryable: true,
            },
        }
    }

    fn from_recall(error: RecallError) -> Self {
        Self {
            code: error.protocol_code().to_owned(),
            message: bounded(error.message(), 240),
            retryable: error.retryable(),
        }
    }

    fn from_dream(error: crate::dream::types::DreamError) -> Self {
        Self { code: error.code().to_string(), message: bounded(&error.to_string(), 240), retryable: false }
    }

    fn from_lease(error: crate::dream::lease::LeaseError) -> Self {
        let retryable = matches!(
            error,
            crate::dream::lease::LeaseError::Held { .. } | crate::dream::lease::LeaseError::Unavailable { .. }
        );
        Self { code: error.code().to_string(), message: bounded(&error.to_string(), 240), retryable }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct LaunchRecord {
        program: String,
        args: Vec<String>,
    }

    #[derive(Clone, Debug)]
    struct FakeChildHandle {
        state: Arc<Mutex<FakeChildState>>,
    }

    #[derive(Clone, Debug, Default)]
    struct FakeChildState {
        running: bool,
        killed: bool,
        waited: bool,
    }

    #[derive(Debug)]
    struct FakeWebDashboardLauncher {
        readiness: FakeReadiness,
        launches: Mutex<Vec<LaunchRecord>>,
        children: Mutex<Vec<FakeChildHandle>>,
    }

    #[derive(Debug)]
    enum FakeReadiness {
        Ready,
        ExitedBeforeBinding,
        Timeout,
    }

    impl FakeWebDashboardLauncher {
        fn ready() -> Self {
            Self::new(FakeReadiness::Ready)
        }

        fn exited_before_binding() -> Self {
            Self::new(FakeReadiness::ExitedBeforeBinding)
        }

        fn timeout() -> Self {
            Self::new(FakeReadiness::Timeout)
        }

        fn new(readiness: FakeReadiness) -> Self {
            Self { readiness, launches: Mutex::new(Vec::new()), children: Mutex::new(Vec::new()) }
        }

        fn launches(&self) -> Vec<LaunchRecord> {
            self.launches.lock().expect("launches lock poisoned").clone()
        }

        fn only_child(&self) -> FakeChildState {
            let children = self.children.lock().expect("children lock poisoned");
            let child = children.first().expect("launcher recorded child");
            let state = child.state.lock().expect("child state lock poisoned").clone();
            state
        }
    }

    impl WebDashboardLauncher for FakeWebDashboardLauncher {
        fn ensure_port_available(&self, _port: u16) -> Result<(), String> {
            Ok(())
        }

        fn spawn(&self, socket_path: &str, port: u16, repo: &Path) -> Result<Box<dyn WebDashboardChild>, String> {
            self.launches.lock().expect("launches lock poisoned").push(LaunchRecord {
                program: "memoryd-web".to_owned(),
                args: vec![
                    "--socket".to_owned(),
                    socket_path.to_owned(),
                    "--port".to_owned(),
                    port.to_string(),
                    "--repo".to_owned(),
                    repo.display().to_string(),
                ],
            });
            let state = Arc::new(Mutex::new(FakeChildState {
                running: matches!(self.readiness, FakeReadiness::Ready | FakeReadiness::Timeout),
                killed: false,
                waited: false,
            }));
            self.children.lock().expect("children lock poisoned").push(FakeChildHandle { state: Arc::clone(&state) });
            Ok(Box::new(FakeWebDashboardChild { state }))
        }

        fn wait_until_ready(&self, port: u16, child: &mut dyn WebDashboardChild) -> Result<(), String> {
            let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
            match self.readiness {
                FakeReadiness::Ready => Ok(()),
                FakeReadiness::ExitedBeforeBinding => {
                    let status = child.try_wait()?.expect("fake child exited before binding");
                    Err(format!("memoryd-web exited before binding {address}: {status}"))
                }
                FakeReadiness::Timeout => Err(format!("memoryd-web did not bind {address} before readiness timeout")),
            }
        }
    }

    #[derive(Debug)]
    struct FakeWebDashboardChild {
        state: Arc<Mutex<FakeChildState>>,
    }

    impl WebDashboardChild for FakeWebDashboardChild {
        fn try_wait(&mut self) -> Result<Option<String>, String> {
            let state = self.state.lock().expect("child state lock poisoned");
            Ok((!state.running).then(|| "exit status: 1".to_owned()))
        }

        fn kill(&mut self) -> Result<(), String> {
            let mut state = self.state.lock().expect("child state lock poisoned");
            state.running = false;
            state.killed = true;
            Ok(())
        }

        fn wait(&mut self) -> Result<(), String> {
            self.state.lock().expect("child state lock poisoned").waited = true;
            Ok(())
        }
    }

    fn unused_localhost_port() -> u16 {
        let listener =
            TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).expect("test listener binds");
        listener.local_addr().expect("test listener has local address").port()
    }

    fn web_launch_config<'a>(port: u16, socket_path: &'a str, repo: &'a Path) -> WebDashboardLaunchConfig<'a> {
        WebDashboardLaunchConfig { port, socket_path, repo }
    }

    #[test]
    fn web_dashboard_enable_success_records_running_status_and_spawn_argv() {
        let launcher = Arc::new(FakeWebDashboardLauncher::ready());
        let mut runtime = WebDashboardRuntime::with_launcher(launcher.clone());
        let port = unused_localhost_port();
        let socket_path = "/tmp/memoryd-test.sock";
        let repo = Path::new("/tmp/memoryd-test-repo");

        let status =
            runtime.enable(web_launch_config(port, socket_path, repo), chrono::Utc::now()).expect("dashboard starts");

        assert!(status.running);
        assert_eq!(status.port, Some(port));
        assert_eq!(status.url.as_deref(), Some(format!("http://localhost:{port}").as_str()));
        assert_eq!(
            launcher.launches(),
            vec![LaunchRecord {
                program: "memoryd-web".to_owned(),
                args: vec![
                    "--socket".to_owned(),
                    socket_path.to_owned(),
                    "--port".to_owned(),
                    port.to_string(),
                    "--repo".to_owned(),
                    repo.display().to_string(),
                ],
            }]
        );
    }

    #[test]
    fn web_dashboard_enable_child_exit_before_binding_cleans_up_and_stops_status() {
        let launcher = Arc::new(FakeWebDashboardLauncher::exited_before_binding());
        let mut runtime = WebDashboardRuntime::with_launcher(launcher.clone());

        let error = runtime
            .enable(
                web_launch_config(
                    unused_localhost_port(),
                    "/tmp/memoryd-test.sock",
                    Path::new("/tmp/memoryd-test-repo"),
                ),
                chrono::Utc::now(),
            )
            .expect_err("start fails");

        assert_eq!(error.code, "web_unavailable");
        assert!(error.message.contains("exited before binding"));
        assert!(!runtime.status(chrono::Utc::now()).running);
        let child = launcher.only_child();
        assert!(!child.killed);
        assert!(child.waited);
    }

    #[test]
    fn web_dashboard_enable_readiness_timeout_kills_child_and_stops_status() {
        let launcher = Arc::new(FakeWebDashboardLauncher::timeout());
        let mut runtime = WebDashboardRuntime::with_launcher(launcher.clone());

        let error = runtime
            .enable(
                web_launch_config(
                    unused_localhost_port(),
                    "/tmp/memoryd-test.sock",
                    Path::new("/tmp/memoryd-test-repo"),
                ),
                chrono::Utc::now(),
            )
            .expect_err("start fails");

        assert_eq!(error.code, "web_unavailable");
        assert!(error.message.contains("did not bind"));
        assert!(!runtime.status(chrono::Utc::now()).running);
        let child = launcher.only_child();
        assert!(child.killed);
        assert!(child.waited);
    }

    #[test]
    fn web_dashboard_enable_same_live_port_is_idempotent_without_second_spawn() {
        let launcher = Arc::new(FakeWebDashboardLauncher::ready());
        let mut runtime = WebDashboardRuntime::with_launcher(launcher.clone());
        let port = unused_localhost_port();
        let repo = Path::new("/tmp/memoryd-test-repo");

        let first = runtime
            .enable(web_launch_config(port, "/tmp/memoryd-test.sock", repo), chrono::Utc::now())
            .expect("dashboard starts");
        let second = runtime
            .enable(web_launch_config(port, "/tmp/memoryd-test.sock", repo), chrono::Utc::now())
            .expect("dashboard is reused");

        assert!(first.running);
        assert!(second.running);
        assert_eq!(second.port, Some(port));
        assert_eq!(launcher.launches().len(), 1);
    }

    #[test]
    fn web_dashboard_enable_rejects_preoccupied_port_before_spawn() {
        let listener =
            TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).expect("test listener binds");
        let port = listener.local_addr().expect("test listener has local address").port();
        let mut runtime = WebDashboardRuntime::default();

        let error = runtime
            .enable(
                web_launch_config(port, "/tmp/memoryd-test.sock", Path::new("/tmp/memoryd-test-repo")),
                chrono::Utc::now(),
            )
            .expect_err("port is rejected");

        assert_eq!(error.code, "port_in_use");
        assert!(error.message.contains("is unavailable before start"));
        assert!(!runtime.status(chrono::Utc::now()).running);
    }

    #[test]
    fn forget_reason_sanitizer_bounds_and_redacts_sensitive_text() {
        assert_eq!(sanitize_forget_reason("  stale memory  "), "stale memory");
        assert_eq!(sanitize_forget_reason(""), REDACTED_FORGET_REASON);
        assert_eq!(sanitize_forget_reason("SSN 123-45-6789"), REDACTED_FORGET_REASON);
        assert_eq!(sanitize_forget_reason(&"a".repeat(FORGET_REASON_MAX_CHARS + 10)).len(), FORGET_REASON_MAX_CHARS);
    }
}
