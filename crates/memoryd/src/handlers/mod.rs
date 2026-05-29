use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};
use std::fs::OpenOptions;
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
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
use memory_source::{capture_web_source, ArtifactStore, CaptureMode, CaptureWebSourceRequest, SourceError};
use memory_substrate::{
    events::EventKind, Author, AuthorKind, ChunkQuery, ClassificationOutcome, EncryptedSubstrateDescriptor,
    EncryptedWriteRequest, Entity, EventContext, Evidence, Frontmatter, IndexProjection, Memory, MemoryContent,
    MemoryId, MemoryQuery, MemoryStatus, MemoryType, ObserveKind, PrivacySpanRecord, RecallIndexQuery, RepoPath,
    RetrievalPolicy, Scope, Sensitivity, Source, SourceKind, Substrate, SubstrateFragmentAppendRequest,
    SubstrateFragmentEncryption, SubstrateFragmentPayload, SupersedeRequest as SubstrateSupersedeRequest,
    TombstoneRequest, TrustLevel, WriteMode, WritePolicy, WriteRequest as SubstrateWriteRequest,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{broadcast, Mutex};

use crate::dream::rehydration;
use crate::protocol::{
    CaptureSourceMode, CaptureSourceResponse, ClaimLockWarning, CompactDreamStatus, ConflictSummary,
    ConflictsListResponse, DaemonProcessStatus, EntitySummary, EventLogEntry, EventsLogPageResponse, GetProvenance,
    GetResponse, GovernanceForgetResponse, GovernancePolicySnapshot, GovernancePolicySummary, GovernanceStatus,
    GovernanceSupersedeResponse, GovernanceWriteResponse, IndexStats, InjectableEventKind, InspectEntitiesResponse,
    NamespaceNode, NamespaceTreeResponse, NotificationEvent, NotificationsRecentResponse, ObserveResponse,
    ObserveTarget, PassiveNotificationStatus, PeerActivityResponse, PeerDeliveryAuditEntry, PeerReleaseLockResponse,
    PeerReleaseLockStatus, PeerSessionStatus, PeerStatusResponse, RealityCheckAction, RealityCheckHistorySession,
    RealityCheckRequest, RealityCheckResponse, RequestEnvelope, RequestPayload, RespondRefusalKind, ResponseEnvelope,
    ResponsePayload, RevealResponse, ReviewDecisionResponse, ReviewQueueCounts, ReviewQueueItemResponse,
    ReviewQueueResponse, SearchHit, SearchResponse, SourceCapturePayload, StatusResponse, WebDashboardStatus,
    WriteNoteResponse, MAX_FRAME_BYTES, NOTIFICATION_CHANNEL_CAPACITY,
};
use crate::reality_check::{RcAdvanceRequest, RcRunRequest, RcSessionAdvance, RcSessionHandler};
use crate::recall::{
    build_delta_response_with_coordination, build_startup_response_with_coordination_config, ConcurrentSessionMode,
    DeltaCoordinationContext, DeltaPeerCooldownStore, DeltaPeerDelivery, DeltaPeerDeliveryRecorder, OmissionReason,
    RecallError, SessionBinding, SharedRecallCounters, StartupResponse,
};

mod doctor;
pub(crate) mod dream;
pub(crate) mod governance;
pub(crate) mod memory_ops;
pub(crate) mod peer;
pub(crate) mod reality_check;
pub(crate) mod review;
pub(crate) mod source;
pub(crate) mod status;
pub(crate) mod web_dashboard;

use doctor::doctor_response;
use dream::{dream_now_response, dream_status_response, DreamNowRequest};
use governance::{
    governance_forget_response, governance_supersede_response, governance_write_response, load_policy_set,
    GovernanceMeta, GovernanceSupersedeRequest, GovernanceWriteRequest,
};
use memory_ops::{
    delta_response, get_response, observe_response, reveal_response, search_response, startup_response,
    validated_claim_lock_identity_field, write_note_response, ObserveRequestFields,
};
use peer::{
    peer_activity_response, peer_heartbeat_response, peer_release_lock_response, peer_status_response,
    PeerDeliveryAudit, PeerUpdateCooldowns,
};
use reality_check::reality_check_response;
use review::{review_decision_response, review_queue_response, ReviewDecision};
use source::{capture_source_response, trust_artifact_response};
use status::status_response;
use web_dashboard::{web_disable_response, web_enable_response, web_status_response, WebDashboardRuntime};

const REVIEW_QUEUE_LIMIT_DEFAULT: usize = 50;
const REVIEW_QUEUE_LIMIT_MAX: usize = 100;
const REVIEW_QUEUE_SUMMARY_MAX: usize = 512;
const REVIEW_QUEUE_POLICY_MAX: usize = 128;
const REVIEW_QUEUE_REASON_MAX: usize = 512;
const REVIEW_QUEUE_ACTION_MAX: usize = 96;
const REVIEW_DECISION_SUMMARY_MAX: usize = 512;
const REVIEW_RESPONSE_FRAME_BUDGET: usize = MAX_FRAME_BYTES - 1024;
const DEFAULT_PROJECT_NAMESPACE: &str = "agent-memory";
const REVEAL_REASON_MAX_CHARS: usize = 512;
const REDACTED_FORGET_REASON: &str = "[redacted]";
const FORGET_REASON_MAX_CHARS: usize = 160;
const DEFAULT_SUPERSEDE_SESSION_ID: &str = "synthetic-memory-supersede";
const DEFAULT_SUPERSEDE_HARNESS: &str = "unknown";

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
        RequestPayload::Status => Ok(ResponsePayload::Status(status_response(substrate, state).await)),
        RequestPayload::Doctor => Ok(ResponsePayload::Doctor(doctor_response(substrate).await)),
        RequestPayload::Search { query, limit, include_body } => {
            search_response(substrate, &query, limit, include_body).await
        }
        RequestPayload::Get { id, include_provenance } => get_response(substrate, &id, include_provenance).await,
        RequestPayload::TrustArtifact { id } => trust_artifact_response(substrate, state, &id).await,
        RequestPayload::CaptureSource(payload) => capture_source_response(substrate, payload).await,
        RequestPayload::DashboardRoi { window_days } => crate::dashboard::roi::dashboard_roi(substrate, window_days)
            .await
            .map(ResponsePayload::DashboardRoi)
            .map_err(HandlerError::substrate),
        RequestPayload::NotificationsRecent { limit } => {
            Ok(ResponsePayload::NotificationsRecent(notifications_recent_response(state, limit)))
        }
        RequestPayload::PolicyValidate { raw_yaml, file_name } => {
            crate::policy_editor::validate(substrate.roots().repo.as_path(), &raw_yaml, file_name.as_deref())
                .map(ResponsePayload::PolicyValidate)
                .map_err(|error| HandlerError::invalid_request(format!("invalid governance policy: {error}")))
        }
        RequestPayload::PolicyWrite { raw_yaml, file_name } => {
            crate::policy_editor::write(substrate, &raw_yaml, file_name.as_deref())
                .map(ResponsePayload::PolicyWrite)
                .map_err(|error| HandlerError::invalid_request(format!("invalid governance policy: {error}")))
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
        RequestPayload::PeerReleaseLock { memory_id } => {
            Ok(ResponsePayload::PeerReleaseLock(peer_release_lock_response(state, &memory_id)?))
        }
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

fn notifications_recent_response(state: &HandlerState, limit: Option<usize>) -> NotificationsRecentResponse {
    NotificationsRecentResponse { notifications: state.passive_notifications.recent_snapshots(limit) }
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
    match crate::policy_editor::snapshot(substrate.roots().repo.as_path()) {
        Ok(snapshot) => Ok(ResponsePayload::GovernancePolicyDump(snapshot)),
        Err(_) => {
            let (policies, source) = load_policy_set(substrate.roots().repo.as_path())?;
            Ok(ResponsePayload::GovernancePolicyDump(GovernancePolicySnapshot {
                source: policy_source_string(source),
                raw_yaml: first_policy_yaml(substrate.roots().repo.as_path()),
                policies: summarize_governance_policy_set(&policies)?,
                current_file: None,
                files: Vec::new(),
                writable: false,
            }))
        }
    }
}

fn summarize_governance_policy_set(policies: &PolicySet) -> Result<Vec<GovernancePolicySummary>, HandlerError> {
    let scopes = [GovernanceScope::Me, GovernanceScope::Project, GovernanceScope::Agent, GovernanceScope::Dreaming];
    scopes
        .into_iter()
        .map(|scope| {
            let policy =
                policies.policy_for_scope(scope).map_err(|error| HandlerError::invalid_request(error.to_string()))?;
            let preview = policy.dry_run(&CandidateContext::new(scope).with_confidence(0.0).with_grounding(false));
            Ok(GovernancePolicySummary {
                scope: format!("{scope:?}").to_ascii_lowercase(),
                selected_policy: preview.selected_policy,
                policy_source: format!("{:?}", preview.policy_source).to_ascii_lowercase(),
                confidence_floor: preview.confidence_floor,
                review_gates: preview.triggered_review_gates,
                requires_grounding: preview.requires_grounding,
            })
        })
        .collect()
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
        | EventKind::SubstrateFragmentWritten { .. }
        | EventKind::DeviceKeysRotated { .. }
        | EventKind::PolicyChanged { .. } => None,
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
        EventKind::DeviceKeysRotated { active_recipient, .. } => {
            format!("device keys rotated: active recipient {active_recipient}")
        }
        EventKind::PolicyChanged { file_name } => format!("policy changed: {file_name}"),
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
        EventKind::DeviceKeysRotated { .. } => "device_keys_rotated",
        EventKind::PolicyChanged { .. } => "policy_changed",
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

fn serialized_payload_len(payload: &ResponsePayload) -> usize {
    serde_json::to_vec(payload).map_or(MAX_FRAME_BYTES, |bytes| bytes.len())
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

/// Compute the spec §6.5 `quote_norm_hash` over an evidence quote: whitespace-collapse
/// to single spaces then SHA-256 the result, formatted as `sha256:<hex>`.
fn compute_quote_norm_hash(quote: &str) -> String {
    use sha2::{Digest, Sha256};
    let normalized = quote.split_whitespace().collect::<Vec<_>>().join(" ");
    let digest = Sha256::digest(normalized.as_bytes());
    format!("sha256:{}", hex::encode(digest))
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

fn bounded_with_truncation(text: &str, max_chars: usize) -> (String, bool) {
    let mut chars = text.chars();
    let bounded: String = chars.by_ref().take(max_chars).collect();
    let truncated = chars.next().is_some();
    (bounded, truncated)
}

#[derive(Debug)]
pub(crate) struct HandlerError {
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
            | SourceError::UrlSafety(_)
            | SourceError::Privacy(_)
            | SourceError::ExcerptNotFound(_) => "invalid_request",
            SourceError::Unsupported(_) => "unsupported",
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

    #[test]
    fn forget_reason_sanitizer_bounds_and_redacts_sensitive_text() {
        assert_eq!(sanitize_forget_reason("  stale memory  "), "stale memory");
        assert_eq!(sanitize_forget_reason(""), REDACTED_FORGET_REASON);
        assert_eq!(sanitize_forget_reason("SSN 123-45-6789"), REDACTED_FORGET_REASON);
        assert_eq!(sanitize_forget_reason(&"a".repeat(FORGET_REASON_MAX_CHARS + 10)).len(), FORGET_REASON_MAX_CHARS);
    }

    #[test]
    fn compute_quote_norm_hash_collapses_whitespace_and_produces_stable_hex() {
        // Whitespace collapse is the invariant; two superficially different quotes
        // that normalize to the same token sequence must hash identically.
        let h1 = compute_quote_norm_hash("hello\tworld");
        let h2 = compute_quote_norm_hash("hello   world\n");
        assert_eq!(h1, h2);
        assert!(h1.starts_with("sha256:"));
        assert_eq!(h1.len(), "sha256:".len() + 64);
    }
}
