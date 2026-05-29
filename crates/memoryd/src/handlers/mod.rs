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
pub(crate) mod peer;
pub(crate) mod web_dashboard;

use doctor::doctor_response;
use peer::{
    peer_activity_response, peer_heartbeat_response, peer_release_lock_response, peer_status_response,
    PeerDeliveryAudit, PeerUpdateCooldowns,
};
use web_dashboard::{web_disable_response, web_enable_response, web_status_response, WebDashboardRuntime};

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
    payload: SourceCapturePayload,
) -> Result<ResponsePayload, HandlerError> {
    let SourceCapturePayload { source, mode, excerpts, note, local_path } = payload;
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
    validate_source_capture_location(mode, local_path.as_deref())?;
    let encryption_key = FileKeyProvider::runtime_default(&substrate.roots().runtime);
    let key_path = encryption_key.path().exists().then(|| encryption_key.path().to_path_buf());
    let response = capture_web_source(
        substrate.roots().repo.clone(),
        CaptureWebSourceRequest {
            url: source,
            excerpts,
            note,
            mode: source_mode_to_capture_mode(mode),
            local_path,
            key_path,
        },
    )
    .await
    .map_err(HandlerError::source_capture)?;
    Ok(ResponsePayload::CaptureSource(CaptureSourceResponse {
        artifact_id: response.artifact_id,
        source_refs: response.source_refs,
        mode,
        final_url: response.final_url,
        captured_at: response.captured_at,
        capture_status: response.capture_status,
        warnings: response.warnings,
    }))
}

fn validate_source_capture_location(mode: CaptureSourceMode, local_path: Option<&Path>) -> Result<(), HandlerError> {
    match mode {
        CaptureSourceMode::HttpStatic => Ok(()),
        CaptureSourceMode::LocalArtifact => {
            let path = local_path
                .ok_or_else(|| HandlerError::invalid_request("local_artifact source capture requires local_path"))?;
            if path.components().any(|component| matches!(component, Component::ParentDir)) {
                return Err(HandlerError::invalid_request("source capture local_path must not contain path traversal"));
            }
            Ok(())
        }
        CaptureSourceMode::PdfText
        | CaptureSourceMode::BrowserRendered
        | CaptureSourceMode::Screenshot
        | CaptureSourceMode::Authenticated
        | CaptureSourceMode::Unsupported => {
            if local_path
                .is_some_and(|path| path.components().any(|component| matches!(component, Component::ParentDir)))
            {
                return Err(HandlerError::invalid_request("source capture local_path must not contain path traversal"));
            }
            Ok(())
        }
    }
}

fn source_mode_to_capture_mode(mode: CaptureSourceMode) -> CaptureMode {
    match mode {
        CaptureSourceMode::HttpStatic => CaptureMode::HttpStatic,
        CaptureSourceMode::LocalArtifact => CaptureMode::LocalArtifact,
        CaptureSourceMode::PdfText => CaptureMode::PdfText,
        CaptureSourceMode::BrowserRendered => CaptureMode::BrowserRendered,
        CaptureSourceMode::Screenshot => CaptureMode::Screenshot,
        CaptureSourceMode::Authenticated => CaptureMode::Authenticated,
        CaptureSourceMode::Unsupported => CaptureMode::Unsupported,
    }
}

async fn status_response(substrate: &Substrate, state: &HandlerState) -> StatusResponse {
    let mut dashboard_warnings = Vec::new();
    let index_stats = match live_index_stats(substrate).await {
        Ok(stats) => Some(stats),
        Err(error) => {
            dashboard_warnings.push(format!("index_stats_unavailable: {}", bounded(&error.message, 160)));
            None
        }
    };
    let review_queue_counts = match live_review_queue_counts(substrate).await {
        Ok(counts) => Some(counts),
        Err(error) => {
            dashboard_warnings.push(format!("review_queue_counts_unavailable: {}", bounded(&error.message, 160)));
            None
        }
    };
    let conflicts_count = match live_conflicts_count(substrate) {
        Ok(count) => Some(count),
        Err(error) => {
            dashboard_warnings.push(format!("conflicts_count_unavailable: {}", bounded(&error.message, 160)));
            None
        }
    };
    let compact_dream_status = match live_compact_dream_status(substrate, chrono::Utc::now()) {
        Ok(status) => Some(status),
        Err(error) => {
            dashboard_warnings.push(format!("compact_dream_status_unavailable: {}", bounded(&error, 160)));
            None
        }
    };

    StatusResponse {
        state: if dashboard_warnings.is_empty() { "ready".to_string() } else { "degraded".to_string() },
        guidance: "memoryd handlers are backed by the Stream A substrate.".to_string(),
        daemon: Some(DaemonProcessStatus {
            version: env!("CARGO_PKG_VERSION").to_string(),
            pid: std::process::id(),
            uptime_seconds: None,
        }),
        dashboard_warnings,
        recall: state.recall.snapshot(),
        dreams: Default::default(),
        passive_notifications: state
            .passive_notifications
            .entries()
            .into_iter()
            .map(|entry| PassiveNotificationStatus { message: entry.message, created_at: entry.created_at })
            .collect(),
        index_stats,
        review_queue_counts,
        conflicts_count,
        peer_sessions: peer_status_response(state).active_sessions,
        peer_update_count: Some(state.peer_deliveries.snapshot().len() as u64),
        compact_dream_status,
    }
}

async fn live_index_stats(substrate: &Substrate) -> Result<IndexStats, HandlerError> {
    let active = count_memories_by_status(substrate, MemoryStatus::Active).await?;
    let pinned = count_memories_by_status(substrate, MemoryStatus::Pinned).await?;
    let last_reindex = substrate
        .events()
        .map_err(HandlerError::substrate)?
        .into_iter()
        .filter(|event| matches!(event.kind, EventKind::StartupReconciliationCompleted { .. }))
        .max_by_key(|event| event.at)
        .map(|event| event.at);
    Ok(IndexStats { active_memories: active + pinned, last_reindex })
}

async fn live_review_queue_counts(substrate: &Substrate) -> Result<ReviewQueueCounts, HandlerError> {
    let candidate = count_memories_by_status(substrate, MemoryStatus::Candidate).await?;
    let quarantined = count_memories_by_status(substrate, MemoryStatus::Quarantined).await?;
    Ok(ReviewQueueCounts { candidate, quarantined, dream_low_confidence: 0 })
}

async fn count_memories_by_status(substrate: &Substrate, status: MemoryStatus) -> Result<u64, HandlerError> {
    let rows = substrate
        .query_memory(MemoryQuery {
            id: None,
            tag: None,
            status: Some(status),
            include_metadata_only: true,
            namespace_prefix: None,
            passive_recall_only: false,
            updated_since: None,
        })
        .await
        .map_err(HandlerError::substrate)?;
    Ok(rows.len() as u64)
}

fn live_conflicts_count(substrate: &Substrate) -> Result<u32, HandlerError> {
    let count = substrate.startup_reconcile_report().blocking_conflicts.len();
    count.try_into().map_err(|_| HandlerError::substrate("conflict count exceeds u32"))
}

fn live_compact_dream_status(
    substrate: &Substrate,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<CompactDreamStatus, String> {
    let roots = substrate.roots();
    let enabled = crate::dream::status::dreaming_enabled(&roots.repo, &roots.runtime)?;
    let last_runs = crate::dream::status::collect_last_runs(&roots.repo)?;
    let active_leases = crate::dream::status::collect_active_leases(&roots.repo, now)?;
    let latest_run = last_runs.iter().filter(|run| run.last_run_at.is_some()).max_by_key(|run| run.last_run_at);
    Ok(CompactDreamStatus {
        enabled,
        last_run_at: latest_run.and_then(|run| run.last_run_at),
        last_run_outcome: latest_run.and_then(|run| run.last_run_outcome),
        next_scheduled_at: None,
        active_leases: active_leases.into_iter().map(|lease| lease.scope).collect(),
    })
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
        RealityCheckRequest::History { limit } => {
            let history = crate::state::RcHistoryStore::new(&substrate.roots().runtime)
                .load(chrono::Utc::now(), limit)
                .map_err(HandlerError::substrate)?;
            Ok(ResponsePayload::RealityCheck(RealityCheckResponse::History {
                sessions: history
                    .sessions
                    .into_iter()
                    .map(|entry| RealityCheckHistorySession {
                        session_id: entry.session_id,
                        started_at: entry.started_at,
                        completed_at: entry.completed_at,
                        items_total: entry.items_total,
                        reviewed: entry.reviewed,
                        confirmed: entry.confirmed,
                        corrected: entry.corrected,
                        forgotten: entry.forgotten,
                        not_relevant: entry.not_relevant,
                        deferred: entry.deferred,
                        remaining: entry.remaining,
                    })
                    .collect(),
            }))
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
        RealityCheckRequest::List { .. } | RealityCheckRequest::History { .. } => {
            unreachable!("read-only requests are handled without the mutation lock")
        }
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
    if let Some(response) = handler
        .try_finalize_completed_session_response(&session_id, &memory_id, now)
        .map_err(HandlerError::substrate)?
    {
        return Ok(response);
    }
    let session = match handler.load_session_for_response(&session_id, &memory_id, now) {
        Ok(session) => session,
        Err(response) => return Ok(*response),
    };

    let advance = match action {
        RealityCheckAction::Confirm => {
            confirm_reality_check_item(substrate, &session_id, &memory_id, now).await?;
            RcSessionAdvance::Confirmed
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
            RcSessionAdvance::Corrected
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
            RcSessionAdvance::Forgotten
        }
        RealityCheckAction::NotRelevant => {
            not_relevant_reality_check_item(substrate, &session_id, &memory_id).await?;
            RcSessionAdvance::NotRelevant
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
    let old = substrate.read_memory(memory_id).await.map_err(HandlerError::substrate)?;
    let response = match governance_supersede_response(
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
    .await
    {
        Ok(response) => response,
        Err(error) if error.code == "privacy_error" => {
            return Ok(Some(reality_check_refused(
                session_id,
                memory_id,
                format!("governance refused correction: {}", error.message),
                RespondRefusalKind::GovernanceRefused,
            )));
        }
        Err(error) => return Err(error),
    };
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
            substrate.read_memory_envelope(&chunk.memory_id).await.ok().and_then(|envelope| match envelope.content {
                MemoryContent::Plaintext(body) => Some(body),
                MemoryContent::Ciphertext { .. } | MemoryContent::MetadataOnly => None,
            })
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
    let json = serde_json::to_value(value)
        .expect("invariant: caller passes a unit-variant enum that serde always serializes infallibly");
    json.as_str().expect("invariant: callers pass unit-variant enums that serialize to JSON strings").to_string()
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
        .record_encrypted_content_revealed(memory_id, bounded(reason, REVEAL_REASON_MAX_CHARS))
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
        .map(|span| PrivacySpanRecord { label: serialized_enum_value(&span.label), start: span.start, end: span.end })
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
    for path in memory_substrate::tree::relative_memory_paths(substrate.roots().repo.as_path()) {
        let repo_path = RepoPath::new(path.to_string_lossy().replace('\\', "/"));
        let envelope = substrate.read_path_envelope(&repo_path).await.map_err(HandlerError::substrate)?;
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
        .map(|item| ReviewQueueItemResponse {
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

fn review_envelope_from_memory(memory: Memory) -> ReviewMemoryEnvelope {
    ReviewMemoryEnvelope {
        id: memory.frontmatter.id.as_str().to_string(),
        summary: memory.frontmatter.summary,
        status: serialized_enum_value(&memory.frontmatter.status),
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
    // Importer-provenance fields (additive per Stream A §6.2/§6.5; all Option-wrapped so
    // existing callers continue to work without supplying them). The daemon mints
    // `Entity`/`Evidence` ids and `quote_norm_hash` from the caller-supplied surface form.
    entities: Option<Vec<EntityMeta>>,
    aliases: Option<Vec<String>>,
    related: Option<Vec<String>>,
    evidence: Option<Vec<EvidenceMeta>>,
    supersedes: Option<Vec<String>>,
    canonical_namespace_id: Option<String>,
    requires_user_confirmation: Option<bool>,
}

/// Caller-supplied entity surface form. The substrate `Entity` struct adds nothing
/// the daemon needs to compute, so this is a direct field-for-field carry.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EntityMeta {
    id: String,
    label: String,
    #[serde(default)]
    aliases: Vec<String>,
}

/// Caller-supplied evidence surface form. The daemon mints `id = ev_<ulid>` and
/// computes `quote_norm_hash = sha256:<hex>` over the whitespace-normalized quote.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EvidenceMeta {
    #[serde(rename = "ref")]
    reference: String,
    #[serde(default)]
    quote: Option<String>,
    #[serde(default)]
    observed_at: Option<chrono::DateTime<chrono::Utc>>,
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
    /// Backfill from a prior harness's memory layer (Claude Code, Codex CLI).
    /// Wire JSON is `"import"`; daemon-side mapping in `author()` and
    /// `substrate_source()` records the import as an agent-authored file load
    /// with `harness = "memoryd-import"`.
    #[serde(rename = "import")]
    Import,
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
            entities: None,
            aliases: None,
            related: None,
            evidence: None,
            supersedes: None,
            canonical_namespace_id: None,
            requires_user_confirmation: None,
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

    /// Build a [`Memory`] from this write input, applying lifecycle, privacy, and any
    /// caller-supplied importer-provenance fields.
    ///
    /// Mapping notes for `GovernanceSourceKindMeta::Import`:
    /// - `author = Author { kind: Agent, harness: Some("memoryd-import"), .. }`
    ///   (recorded as agent-authored, not user-authored, even though the content
    ///   originated from the user's prior harness sessions).
    /// - `source.kind = SourceKind::File` (the source IS a local file on disk,
    ///   even though the upstream `source_kind` tag is `"import"`).
    /// - `source.harness = Some("memoryd-import")` so downstream consumers can
    ///   filter the backfill in dashboards and recall ranking.
    ///
    /// Evidence ids and `quote_norm_hash` are minted here from the caller-supplied
    /// `EvidenceMeta` surface form so the importer never has to invent identifiers.
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
        let entities = self.entities_for_persist();
        let aliases = self.aliases_for_persist();
        let related = self.related_for_persist();
        let supersedes = self.supersedes_for_persist();
        let evidence = self.evidence_for_persist();
        let canonical_namespace_id = self.meta.canonical_namespace_id.clone().or_else(|| self.substrate_namespace());
        // Importer writes carry already-vetted content from prior harness sessions and
        // should not flood the Reality Check review queue with low-confidence guesses.
        // Caller can suppress the review flag for non-candidate writes; lifecycle still
        // forces review for `Candidate`/`Quarantined` so the override never weakens
        // governance.
        let requires_user_confirmation =
            self.meta.requires_user_confirmation.map_or(requires_review, |caller| requires_review || caller);
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
                canonical_namespace_id,
                tags: self.persisted_tags(privacy.storage_action),
                entities,
                aliases,
                source: self.substrate_source(privacy.storage_action),
                evidence,
                requires_user_confirmation,
                review_state,
                supersedes,
                superseded_by: Vec::new(),
                related,
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

    fn entities_for_persist(&self) -> Vec<Entity> {
        self.meta
            .entities
            .as_ref()
            .map(|entries| {
                entries
                    .iter()
                    .map(|entry| Entity {
                        id: entry.id.clone(),
                        label: entry.label.clone(),
                        aliases: entry.aliases.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn aliases_for_persist(&self) -> Vec<String> {
        self.meta.aliases.clone().unwrap_or_default()
    }

    fn related_for_persist(&self) -> Vec<MemoryId> {
        self.meta.related.as_ref().map(|ids| ids.iter().cloned().map(MemoryId::new).collect()).unwrap_or_default()
    }

    fn supersedes_for_persist(&self) -> Vec<MemoryId> {
        self.meta.supersedes.as_ref().map(|ids| ids.iter().cloned().map(MemoryId::new).collect()).unwrap_or_default()
    }

    fn evidence_for_persist(&self) -> Vec<Evidence> {
        let Some(entries) = self.meta.evidence.as_ref() else {
            return Vec::new();
        };
        entries
            .iter()
            .map(|entry| {
                let quote = entry.quote.clone().unwrap_or_default();
                let quote_norm_hash = (!quote.is_empty()).then(|| compute_quote_norm_hash(&quote));
                Evidence {
                    id: format!("ev_{}", ulid::Ulid::new()),
                    quote,
                    quote_norm_hash,
                    reference: entry.reference.clone(),
                    weight: 1.0,
                    observed_at: entry.observed_at,
                    source: None,
                }
            })
            .collect()
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
            GovernanceSourceKindMeta::AgentPrimary
            | GovernanceSourceKindMeta::File
            | GovernanceSourceKindMeta::Import => GovernanceSourceKind::AgentPrimary,
        };
        vec![GovernanceSource::new(kind, self.meta.source_ref.clone())]
    }

    fn substrate_source(&self, storage_action: PrivacyStorageAction) -> Source {
        let kind = match self.meta.source_kind {
            GovernanceSourceKindMeta::User => SourceKind::User,
            GovernanceSourceKindMeta::Subagent => SourceKind::AgentSubagent,
            GovernanceSourceKindMeta::WebCapture => SourceKind::Web,
            // The importer reads files off disk, so the substrate source kind is `File`
            // regardless of the upstream `source_kind = "import"` tag. The `harness`
            // field below distinguishes import writes from generic file writes.
            GovernanceSourceKindMeta::File | GovernanceSourceKindMeta::Import => SourceKind::File,
            GovernanceSourceKindMeta::AgentPrimary => SourceKind::AgentPrimary,
        };
        let harness =
            matches!(self.meta.source_kind, GovernanceSourceKindMeta::Import).then(|| "memoryd-import".to_string());
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
            harness,
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
            GovernanceSourceKindMeta::Import => Author {
                kind: AuthorKind::Agent,
                user_handle: None,
                harness: Some("memoryd-import".to_string()),
                harness_version: Some(env!("CARGO_PKG_VERSION").to_string()),
                session_id: Some("memoryd-session".to_string()),
                subagent_id: None,
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

    // -----------------------------------------------------------------------------------
    // T00: importer-provenance fields on GovernanceMeta. The tests below lock the
    // additive-extension contract — new optional fields round-trip, defaults stay
    // None, `deny_unknown_fields` still rejects unknown keys, and `source_kind:
    // "import"` maps to a file-source agent-author with the `memoryd-import` harness.
    // -----------------------------------------------------------------------------------

    fn write_input(meta: Value) -> GovernanceWriteInput {
        GovernanceWriteInput::parse(GovernanceWriteInputParts {
            body: "Body text".to_string(),
            title: Some("Title".to_string()),
            tags: Vec::new(),
            meta,
            source: MetaSource::Default,
        })
        .expect("write input parses")
    }

    fn plaintext_privacy_decision() -> memory_privacy::PrivacyDecision {
        memory_privacy::PrivacyDecision::new(
            memory_privacy::PrivacyTier::Internal,
            memory_privacy::PrivacyStorageAction::Plaintext,
            Vec::new(),
            "test-classifier",
        )
    }

    fn promoted_lifecycle() -> GovernedLifecycle {
        GovernedLifecycle::new(MemoryStatus::Active, TrustLevel::Trusted, "test-policy".to_string())
    }

    #[test]
    fn governance_meta_empty_payload_preserves_existing_defaults() {
        let meta: GovernanceMeta = parse_governance_meta(Value::Null, MetaSource::Default).expect("null parses");
        assert!(meta.entities.is_none());
        assert!(meta.aliases.is_none());
        assert!(meta.related.is_none());
        assert!(meta.evidence.is_none());
        assert!(meta.supersedes.is_none());
        assert!(meta.canonical_namespace_id.is_none());
        assert!(meta.requires_user_confirmation.is_none());

        // Backward-compat: an empty payload should produce the exact same Memory shape
        // as before the additive extension — empty entities/aliases/related/evidence/supersedes
        // and canonical_namespace_id falling back to the default project namespace.
        let input = write_input(Value::Null);
        let memory = input.to_memory(
            MemoryId::new("mem_20260527_a1b2c3d4e5f60718_000001"),
            promoted_lifecycle(),
            &plaintext_privacy_decision(),
        );
        assert!(memory.frontmatter.entities.is_empty());
        assert!(memory.frontmatter.aliases.is_empty());
        assert!(memory.frontmatter.related.is_empty());
        assert!(memory.frontmatter.evidence.is_empty());
        assert!(memory.frontmatter.supersedes.is_empty());
        assert_eq!(memory.frontmatter.canonical_namespace_id.as_deref(), Some(DEFAULT_PROJECT_NAMESPACE));
        assert!(!memory.frontmatter.requires_user_confirmation);
    }

    #[test]
    fn governance_meta_accepts_importer_provenance_fields_and_round_trips_through_to_memory() {
        let payload = serde_json::json!({
            "namespace": "project",
            "source_kind": "import",
            "source_ref": "/Users/treygoff/.claude/projects/example/memory/topic.md",
            "confidence": 0.7,
            "requires_user_confirmation": false,
            "canonical_namespace_id": "proj_0123456789abcdef",
            "entities": [
                { "id": "ent_acme", "label": "Acme Corp", "aliases": ["Acme", "ACME"] }
            ],
            "aliases": ["topic.md"],
            "related": ["mem_20260527_a1b2c3d4e5f60718_000010"],
            "supersedes": ["mem_20260527_a1b2c3d4e5f60718_000003"],
            "evidence": [
                {
                    "ref": "file:///Users/treygoff/.codex/memories/rollouts/abc.md",
                    "quote": "  shipped\n  fix  ",
                    "observed_at": "2026-05-27T22:33:00Z"
                }
            ]
        });
        let input = write_input(payload);
        let memory = input.to_memory(
            MemoryId::new("mem_20260527_a1b2c3d4e5f60718_000042"),
            promoted_lifecycle(),
            &plaintext_privacy_decision(),
        );

        assert_eq!(memory.frontmatter.entities.len(), 1);
        assert_eq!(memory.frontmatter.entities[0].id, "ent_acme");
        assert_eq!(memory.frontmatter.entities[0].aliases, vec!["Acme".to_string(), "ACME".to_string()]);
        assert_eq!(memory.frontmatter.aliases, vec!["topic.md".to_string()]);
        assert_eq!(memory.frontmatter.related[0].as_str(), "mem_20260527_a1b2c3d4e5f60718_000010");
        assert_eq!(memory.frontmatter.supersedes[0].as_str(), "mem_20260527_a1b2c3d4e5f60718_000003");
        assert_eq!(memory.frontmatter.canonical_namespace_id.as_deref(), Some("proj_0123456789abcdef"));

        // Evidence id is minted as `ev_<ulid>`; quote_norm_hash is `sha256:<hex>` over
        // the whitespace-collapsed quote (so "  shipped\n  fix  " hashes the same as
        // "shipped fix").
        let evidence = &memory.frontmatter.evidence[0];
        assert!(evidence.id.starts_with("ev_"));
        assert_eq!(evidence.reference, "file:///Users/treygoff/.codex/memories/rollouts/abc.md");
        assert_eq!(evidence.quote, "  shipped\n  fix  ");
        let expected_hash = compute_quote_norm_hash("shipped fix");
        assert_eq!(evidence.quote_norm_hash.as_deref(), Some(expected_hash.as_str()));
        assert!(evidence.observed_at.is_some());
    }

    #[test]
    fn governance_meta_import_source_kind_maps_to_file_source_and_memoryd_import_harness() {
        let payload = serde_json::json!({
            "namespace": "project",
            "source_kind": "import",
            "source_ref": "/Users/treygoff/.claude/projects/x/memory/y.md"
        });
        let input = write_input(payload);
        assert!(matches!(input.meta.source_kind, GovernanceSourceKindMeta::Import));

        let memory = input.to_memory(
            MemoryId::new("mem_20260527_a1b2c3d4e5f60718_000007"),
            promoted_lifecycle(),
            &plaintext_privacy_decision(),
        );

        // Author records the agent-authored import with the dedicated harness tag so
        // dashboards and recall ranking can identify backfilled content.
        assert!(matches!(memory.frontmatter.author.kind, AuthorKind::Agent));
        assert_eq!(memory.frontmatter.author.harness.as_deref(), Some("memoryd-import"));

        // Substrate Source stays `File` (the source IS a local file) but the harness
        // tag differentiates it from generic file writes.
        assert!(matches!(memory.frontmatter.source.kind, SourceKind::File));
        assert_eq!(memory.frontmatter.source.harness.as_deref(), Some("memoryd-import"));
        assert_eq!(
            memory.frontmatter.source.reference.as_deref(),
            Some("/Users/treygoff/.claude/projects/x/memory/y.md")
        );
    }

    #[test]
    fn governance_meta_rejects_unknown_field() {
        let payload = serde_json::json!({
            "namespace": "project",
            "source_kind": "user",
            "zzz_unknown_field": 1
        });
        let err = parse_governance_meta(payload, MetaSource::Default).expect_err("unknown field is rejected");
        assert!(err.message.contains("zzz_unknown_field"), "error mentions the field: {}", err.message);
    }

    #[test]
    fn governance_meta_serializes_import_source_kind_as_lowercase_token() {
        // Lock the wire format: the import variant must serialize as the JSON token
        // `"import"` (matches Stream A spec §6 frontmatter source.kind) so MCP clients
        // can submit the same shape that the importer uses internally.
        let payload = serde_json::json!({ "source_kind": "import" });
        let meta: GovernanceMeta = parse_governance_meta(payload, MetaSource::Default).expect("import parses");
        assert!(matches!(meta.source_kind, GovernanceSourceKindMeta::Import));
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
