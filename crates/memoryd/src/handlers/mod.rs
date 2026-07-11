use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use memorum_coordination::claim_lock::{ClaimLockClock, ClaimLockRegistry};
use memorum_coordination::presence::ClaimLockHeartbeatRenewal;
use memorum_coordination::{
    handle_peer_heartbeat as coordination_handle_peer_heartbeat, ClaimLockInfo, CoordinationConfig, PeerHeartbeatError,
    PeerHeartbeatOptions, PresenceConfig, PresenceRegistry,
};
use memory_governance::review::REVIEW_QUEUE_DOGFOOD_THRESHOLD;
use memory_governance::{GovernanceRefusalReason, PolicySource, ReviewMemoryEnvelope, ReviewQueue};
use memory_privacy::{
    safe_descriptor_projection, DeterministicPrivacyClassifier, EncryptedPayload, FileKeyProvider, PrivacyDecision,
    PrivacyEncryptor, PrivacyNamespace, PrivacyStorageAction,
};
use memory_source::{capture_web_source, CaptureMode, CaptureWebSourceRequest};
use memory_substrate::{
    events::EventKind, Author, AuthorKind, AuxScope, ClassificationOutcome, EncryptedSubstrateDescriptor, EventContext,
    Frontmatter, Memory, MemoryContent, MemoryId, MemoryStatus, MemoryType, ObserveKind, PrivacySpanRecord,
    RecallIndexQuery, RepoPath, RetrievalPolicy, Scope, Sensitivity, Source, SourceKind, Substrate,
    SubstrateFragmentAppendRequest, SubstrateFragmentEncryption, SubstrateFragmentPayload, TrustLevel, WriteMode,
    WritePolicy, WriteRequest as SubstrateWriteRequest,
};
use serde_json::Value;
use tokio::sync::{broadcast, Mutex};

use crate::dream::rehydration;
use crate::protocol::{
    CaptureSourceMode, CaptureSourceResponse, CompactDreamStatus, DaemonProcessStatus, EmbeddingStatus, GetProvenance,
    GetResponse, GovernanceStatus, IndexStats, NotificationEvent, ObserveResponse, ObserveTarget,
    PassiveNotificationStatus, PeerActivityResponse, PeerDeliveryAuditEntry, PeerReleaseLockResponse,
    PeerReleaseLockStatus, PeerSessionStatus, PeerStatusResponse, QuarantineResolutionMode, QuarantineResolveResponse,
    RealityCheckAction, RealityCheckHistorySession, RealityCheckRequest, RealityCheckResponse, RequestEnvelope,
    RequestPayload, RespondRefusalKind, ResponseEnvelope, ResponsePayload, RevealResponse, ReviewDecisionResponse,
    ReviewQueueCounts, ReviewQueueItemResponse, ReviewQueueResponse, SearchHit, SearchResponse, SourceCapturePayload,
    StatusResponse, WebDashboardStatus, WriteNoteResponse, MAX_FRAME_BYTES, NOTIFICATION_CHANNEL_CAPACITY,
};
use crate::reality_check::{RcAdvanceRequest, RcRunRequest, RcSessionAdvance, RcSessionHandler};
use crate::recall::{
    build_startup_response_with_coordination_config, ConcurrentSessionMode, DeltaCoordinationContext,
    DeltaPeerCooldownStore, DeltaPeerDelivery, DeltaPeerDeliveryRecorder, OmissionReason, RecallDedupState,
    SessionBinding, SharedRecallCounters, StartupResponse,
};

mod doctor;
pub(crate) mod dream;
pub(crate) mod error;
pub(crate) mod governance;
mod inspect;
pub(crate) mod memory_ops;
pub(crate) mod peer;
mod privacy_text;
pub(crate) mod quarantine;
pub(crate) mod reality_check;
pub(crate) mod review;
pub(crate) mod source;
pub(crate) mod status;
pub(crate) mod web_dashboard;

// Re-export the moved items back into the `handlers` module namespace so the
// historical paths stay valid: the sibling handler modules pull them in through
// `use super::*`, and `governance::*` reaches them via `crate::handlers::…`.
pub(crate) use error::HandlerError;
pub(crate) use inspect::event_kind_label;
pub(crate) use privacy_text::{
    contains_secret_or_pii_marker, insert_safe_descriptor, is_safe_plaintext_for_indexing, safe_index_projection,
    sanitize_reason,
};

use doctor::doctor_response;
use dream::{dream_now_response, dream_status_response, DreamNowRequest};
use governance::{
    governance_forget_response, governance_supersede_response, governance_write_response, load_policy_set,
    GovernanceMeta, GovernanceSupersedeRequest, GovernanceWriteRequest,
};
use memory_ops::{
    delta_response, get_response, observe_response, reveal_response, search_response, startup_response,
    write_note_response, ObserveRequestFields, SearchResponseRequest,
};
use peer::{
    peer_activity_response, peer_heartbeat_response, peer_release_lock_response, peer_status_response,
    PeerDeliveryAudit, PeerUpdateCooldowns,
};
use quarantine::quarantine_resolve_response;
use reality_check::reality_check_response;
use review::{
    review_decision_response, review_merges_response, review_queue_response, MergeReviewAction, ReviewDecision,
};
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
const REVEAL_REASON_MAX_CHARS: usize = 512;
const REDACTED_REASON: &str = "[redacted]";
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
    recall_dedup: RecallDedupState,
    /// Late-initialized embedding provider, shared with the background embedding
    /// worker. Governance contradiction detection reads it to embed a write
    /// candidate for KNN similarity; an empty slot degrades to "no similarity
    /// candidates" (visible in the decision trace).
    embedding_provider: crate::embedding::EmbeddingProviderSlot,
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
            recall_dedup: RecallDedupState::default(),
            embedding_provider: crate::embedding::EmbeddingProviderSlot::empty(),
        }
    }

    /// Shared embedding-provider slot, cloned for the background worker so it can
    /// publish the loaded provider and for governance to embed write candidates.
    pub fn embedding_provider_slot(&self) -> crate::embedding::EmbeddingProviderSlot {
        self.embedding_provider.clone()
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

    /// Per-daemon dedup state for startup recall (reality-check + dream-question
    /// surfacing windows). Replaces the former process-global statics.
    pub(crate) fn recall_dedup(&self) -> &RecallDedupState {
        &self.recall_dedup
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
        RequestPayload::Doctor => Ok(ResponsePayload::Doctor(doctor_response(substrate, state).await)),
        RequestPayload::Search { query, limit, include_body } => {
            search_response(substrate, state, SearchResponseRequest { query: &query, limit, include_body }).await
        }
        RequestPayload::Get { id, include_provenance, full_body } => {
            get_response(substrate, &id, include_provenance, full_body).await
        }
        RequestPayload::TrustArtifact { id } => trust_artifact_response(substrate, state, &id).await,
        RequestPayload::CaptureSource(payload) => capture_source_response(substrate, payload).await,
        RequestPayload::DashboardRoi { window_days } => crate::dashboard::roi::dashboard_roi(substrate, window_days)
            .await
            .map(ResponsePayload::DashboardRoi)
            .map_err(HandlerError::substrate),
        RequestPayload::NotificationsRecent { limit } => {
            Ok(ResponsePayload::NotificationsRecent(inspect::notifications_recent_response(state, limit)))
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
        RequestPayload::RecallHits { since, limit } => inspect::recall_hits_response(substrate, since, limit).await,
        RequestPayload::Reveal { id, reason } => reveal_response(substrate, &id, &reason).await,
        RequestPayload::WriteNote { text, meta } => write_note_response(substrate, &text, &meta).await,
        RequestPayload::WriteMemory { body, title, tags, meta } => {
            governance_write_response(substrate, Some(state), GovernanceWriteRequest { body, title, tags, meta }).await
        }
        RequestPayload::Supersede { old_id, content, reason, meta } => {
            governance_supersede_response(
                substrate,
                Some(state),
                GovernanceSupersedeRequest { old_id, content, reason, meta, preserve_frontmatter: false },
            )
            .await
        }
        RequestPayload::Forget { id, reason } => governance_forget_response(substrate, id, reason).await,
        RequestPayload::ReviewQueue { limit } => review_queue_response(substrate, state, limit).await,
        RequestPayload::ReviewApprove { id } => review_decision_response(substrate, &id, ReviewDecision::Approve).await,
        RequestPayload::ReviewReject { id, reason } => {
            review_decision_response(substrate, &id, ReviewDecision::Reject { reason }).await
        }
        RequestPayload::ReviewMerges => review_merges_response(substrate, state, MergeReviewAction::List).await,
        RequestPayload::ReviewMergeApprove { proposal_id, approve_pinned } => {
            review_merges_response(substrate, state, MergeReviewAction::Approve { proposal_id, approve_pinned }).await
        }
        RequestPayload::ReviewMergeReject { proposal_id } => {
            review_merges_response(substrate, state, MergeReviewAction::Reject { proposal_id }).await
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
        RequestPayload::InspectEntities { limit, prefix } => {
            inspect::inspect_entities_response(substrate, limit, prefix).await
        }
        RequestPayload::EventsLogPage { since, limit, kind_filter } => {
            inspect::events_log_page_response(substrate, since, limit, kind_filter)
        }
        RequestPayload::NamespaceTree { root, depth } => inspect::namespace_tree_response(substrate, root, depth).await,
        RequestPayload::GovernancePolicyDump => inspect::governance_policy_dump_response(substrate),
        RequestPayload::ConflictsList { limit } => inspect::conflicts_list_response(substrate, limit).await,
        RequestPayload::QuarantineResolve { id, mode } => quarantine_resolve_response(substrate, state, id, mode).await,
        RequestPayload::TestInjectEvent { kind, memory_id, ts, harness, session_id } => {
            inspect::test_inject_event_response(
                substrate,
                inspect::TestInjectEventRequest { kind, memory_id, ts, harness, session_id },
            )
            .await
        }
    }
}

/// Coarse governance bucket for a substrate `Scope`: the three-way
/// `me` / `project` / `agent` grouping that collapses `Org` into `project` and
/// `Subagent` into `agent`. Single source of truth for the inverse of
/// `policy::scopes_for_namespace`. Distinct from `namespace_label`, which is the
/// per-memory *display* path and keeps `Org`/`Project` separate as `org:{id}` /
/// `project:{id}`.
fn namespace_bucket_for_scope(scope: Scope) -> &'static str {
    match scope {
        Scope::User => "me",
        Scope::Project | Scope::Org => "project",
        Scope::Agent | Scope::Subagent => "agent",
    }
}

fn governance_namespace_meta(frontmatter: &Frontmatter) -> &'static str {
    namespace_bucket_for_scope(frontmatter.scope)
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

fn sanitize_forget_reason(reason: &str) -> String {
    sanitize_reason(reason, FORGET_REASON_MAX_CHARS)
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
    namespace_bucket_for_scope(frontmatter.scope).to_string()
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
            abstraction: None,
            cues: Vec::new(),
            extras: BTreeMap::new(),
        },
        body: text.to_string(),
        path: Some(RepoPath::new(format!("agent/patterns/{}.md", id.as_str()))),
    }
}

/// Compute the spec §6.5 `quote_norm_hash` over an evidence quote: whitespace-collapse
/// to single spaces then SHA-256 the result, formatted as `sha256:<hex>`.
fn compute_quote_norm_hash(quote: &str) -> String {
    use sha2::{Digest, Sha256};
    let normalized = quote.split_whitespace().collect::<Vec<_>>().join(" ");
    let digest = Sha256::digest(normalized.as_bytes());
    format!("sha256:{}", hex::encode(digest))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forget_reason_sanitizer_bounds_and_redacts_sensitive_text() {
        assert_eq!(sanitize_forget_reason("  stale memory  "), "stale memory");
        assert_eq!(sanitize_forget_reason(""), REDACTED_REASON);
        assert_eq!(sanitize_forget_reason("SSN 123-45-6789"), REDACTED_REASON);
        assert_eq!(sanitize_forget_reason(&"a".repeat(FORGET_REASON_MAX_CHARS + 10)).len(), FORGET_REASON_MAX_CHARS);
    }

    #[test]
    fn forget_reason_redacts_bare_credentials_without_marker_words() {
        // The structural/entropy classifier — not the keyword denylist — must be the
        // primary gate. These reasons carry none of the denylisted marker words
        // ("secret", "token", "api key", "sk-"), so they exercise the classifier path.

        // AWS access key ID shape, no marker words.
        assert_eq!(sanitize_forget_reason("rotating AKIAIOSFODNN7EXAMPLE per policy"), REDACTED_REASON);

        // Bare high-entropy credential (>=32 mixed alnum chars), no marker words.
        assert_eq!(sanitize_forget_reason("replacing xQ9fLp2Zr7Wk4Nb8Vt3Hy6Mc1Js5Dg0Ae after audit"), REDACTED_REASON);

        // Sanity: an ordinary operator reason with no secret structure survives.
        assert_eq!(sanitize_forget_reason("duplicate of the onboarding note"), "duplicate of the onboarding note");
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
