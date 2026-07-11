use std::collections::BTreeMap;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::recall::counters::RecallStatusCounters;
use crate::recall::types::{DeltaRequest, DeltaResponse, StartupRequest, StartupResponse};

pub use memorum_coordination::{ClaimLockInfo, PeerHeartbeat, PeerHeartbeatAck};
pub use memory_governance::{GovernanceRefusalReason, ReviewStatus};
pub use memory_source::CaptureStatus;
use memory_substrate::events::EventKind;
pub use memory_substrate::{AuthorKind, EventId, MemoryId, MemoryStatus, ObserveKind, Sensitivity, SourceKind};

/// Maximum byte length of a single newline-delimited request or response frame.
/// Defined here so both the server-side reader and client-side reader share the same limit.
pub const MAX_FRAME_BYTES: usize = 64 * 1024;

/// Capacity for the internal `memoryd` notification broadcast channel.
///
/// Stream G §6.3 defines the channel as process-internal. Dispatchers tolerate
/// lagged receivers; notifications are not persisted by this protocol layer.
pub const NOTIFICATION_CHANNEL_CAPACITY: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestEnvelope {
    pub id: String,
    pub request: RequestPayload,
}

impl RequestEnvelope {
    pub fn new(id: impl Into<String>, request: RequestPayload) -> Self {
        Self { id: id.into(), request }
    }

    pub fn to_json_line(&self) -> serde_json::Result<String> {
        encode_json_line(self)
    }

    pub fn from_json_line(line: &str) -> serde_json::Result<Self> {
        serde_json::from_str(line.trim_end_matches('\n'))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestPayload {
    Status,
    Doctor,
    Search {
        query: String,
        limit: Option<usize>,
        include_body: bool,
    },
    Get {
        id: String,
        include_provenance: bool,
        #[serde(default)]
        full_body: bool,
    },
    TrustArtifact {
        id: String,
    },
    CaptureSource(SourceCapturePayload),
    DashboardRoi {
        window_days: u16,
    },
    NotificationsRecent {
        limit: Option<usize>,
    },
    PolicyValidate {
        raw_yaml: String,
        file_name: Option<String>,
    },
    PolicyWrite {
        raw_yaml: String,
        file_name: Option<String>,
    },
    RecallHits {
        since: Option<DateTime<Utc>>,
        limit: Option<usize>,
    },
    Reveal {
        id: String,
        reason: String,
    },
    WriteNote {
        text: String,
        #[serde(default)]
        meta: Value,
    },
    WriteMemory {
        body: String,
        title: Option<String>,
        tags: Vec<String>,
        meta: Value,
    },
    Supersede {
        old_id: String,
        content: String,
        reason: String,
        meta: Value,
    },
    Forget {
        id: String,
        reason: String,
    },
    ReviewQueue {
        limit: Option<usize>,
    },
    ReviewApprove {
        id: String,
    },
    ReviewReject {
        id: String,
        reason: String,
    },
    ReviewMerges,
    ReviewMergeApprove {
        proposal_id: String,
        #[serde(default)]
        approve_pinned: Vec<String>,
    },
    ReviewMergeReject {
        proposal_id: String,
    },
    Startup(StartupRequest),
    Delta(DeltaRequest),
    PeerHeartbeat(PeerHeartbeat),
    PeerStatus,
    PeerActivity {
        session: Option<String>,
        since: Option<String>,
        limit: Option<usize>,
        format: PeerActivityFormat,
    },
    PeerReleaseLock {
        memory_id: String,
    },
    Observe {
        text: String,
        kind: ObserveKind,
        #[serde(default)]
        entities: Vec<String>,
        #[serde(default = "default_observe_cwd")]
        cwd: String,
        #[serde(default = "default_observe_session_id")]
        session_id: String,
        #[serde(default = "default_observe_harness")]
        harness: String,
        #[serde(default)]
        harness_version: Option<String>,
    },
    DreamNow {
        scope: String,
        force: bool,
        cli_override: Option<String>,
    },
    DreamStatus {},
    WebEnable {
        port: u16,
        socket_path: String,
    },
    WebDisable,
    WebStatus,
    RealityCheck(RealityCheckRequest),
    InspectEntities {
        limit: Option<usize>,
        prefix: Option<String>,
    },
    EventsLogPage {
        since: Option<EventId>,
        limit: usize,
        kind_filter: Option<Vec<EventKind>>,
    },
    NamespaceTree {
        root: Option<String>,
        depth: Option<usize>,
    },
    GovernancePolicyDump,
    ConflictsList {
        limit: Option<usize>,
    },
    QuarantineResolve {
        id: String,
        mode: QuarantineResolutionMode,
    },

    /// Inject a synthetic event-log entry with a controlled timestamp.
    ///
    /// Only meaningful when `memoryd` is built with the `test-utils` feature.
    /// Production builds receive this variant and return `method_not_allowed`.
    /// Stream H eval tests use this to seed `RecallHit` and `WriteCommitted`
    /// events so that drift-score derived metrics are deterministic. (H-R1)
    TestInjectEvent {
        kind: InjectableEventKind,
        memory_id: MemoryId,
        ts: DateTime<Utc>,
        /// Provenance fields for injected `WriteCommitted` events; ignored for `RecallHit`.
        harness: Option<String>,
        session_id: Option<String>,
    },
}

/// Kind of synthetic event injected by `TestInjectEvent`.
///
/// Part of the `test-utils` surface; see `RequestPayload::TestInjectEvent`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InjectableEventKind {
    /// Append `EventKind::RecallHit { id, recalled_at: ts }` to the events log.
    RecallHit,
    /// Append a `WriteCommitted`-style event with synthetic provenance.
    WriteCommitted,
}

/// Response to a successful `TestInjectEvent` request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestInjectEventResponse {
    pub event_id: String,
    pub injected_kind: InjectableEventKind,
    pub memory_id: MemoryId,
}

pub fn default_observe_cwd() -> String {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")).to_string_lossy().into_owned()
}

pub fn default_observe_session_id() -> String {
    "synthetic-memory-observe".to_owned()
}

pub fn default_observe_harness() -> String {
    "unknown".to_owned()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RealityCheckRequest {
    List { namespace: Option<String>, limit: Option<usize> },
    History { limit: Option<usize> },
    Run { session_id: Option<String>, namespace: Option<String>, limit: Option<usize> },
    Respond { session_id: String, memory_id: MemoryId, action: RealityCheckAction },
    Skip,
    Snooze { until: Option<DateTime<Utc>> },
    Reset,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RealityCheckAction {
    Confirm,
    Correct { new_body: String },
    Forget { reason: String },
    NotRelevant,
    SkipThisWeek,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponseEnvelope {
    pub id: String,
    pub result: ResponseResult,
}

impl ResponseEnvelope {
    pub fn success(id: impl Into<String>, payload: ResponsePayload) -> Self {
        Self { id: id.into(), result: ResponseResult::Success(payload) }
    }

    pub fn error(id: impl Into<String>, code: impl Into<String>, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            id: id.into(),
            result: ResponseResult::Error(ProtocolError { code: code.into(), message: message.into(), retryable }),
        }
    }

    pub fn to_json_line(&self) -> serde_json::Result<String> {
        encode_json_line(self)
    }

    pub fn from_json_line(line: &str) -> serde_json::Result<Self> {
        serde_json::from_str(line.trim_end_matches('\n'))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(clippy::large_enum_variant)]
pub enum ResponseResult {
    Success(ResponsePayload),
    Error(ProtocolError),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[expect(clippy::large_enum_variant, reason = "protocol enum intentionally carries typed response DTOs")]
pub enum ResponsePayload {
    Status(StatusResponse),
    Doctor(DoctorResponse),
    Search(SearchResponse),
    Get(GetResponse),
    TrustArtifact(Box<crate::trust_artifact::TrustArtifact>),
    CaptureSource(CaptureSourceResponse),
    DashboardRoi(DashboardRoiResponse),
    NotificationsRecent(NotificationsRecentResponse),
    PolicyValidate(PolicyEditorMutationResponse),
    PolicyWrite(PolicyEditorMutationResponse),
    RecallHits(RecallHitsResponse),
    Reveal(RevealResponse),
    WriteNote(WriteNoteResponse),
    GovernanceWrite(GovernanceWriteResponse),
    GovernanceSupersede(GovernanceSupersedeResponse),
    GovernanceForget(GovernanceForgetResponse),
    ReviewQueue(ReviewQueueResponse),
    ReviewApprove(ReviewDecisionResponse),
    ReviewReject(ReviewDecisionResponse),
    ReviewMerges(MergeReviewResponse),
    Startup(Box<StartupResponse>),
    Delta(DeltaResponse),
    PeerHeartbeat(PeerHeartbeatAck),
    PeerStatus(PeerStatusResponse),
    PeerActivity(PeerActivityResponse),
    PeerReleaseLock(PeerReleaseLockResponse),
    Observe(ObserveResponse),
    DreamNow(Box<DreamRunReport>),
    DreamStatus(Box<DreamStatusReport>),
    WebStatus(WebDashboardStatus),
    RealityCheck(RealityCheckResponse),
    InspectEntities(InspectEntitiesResponse),
    EventsLogPage(EventsLogPageResponse),
    NamespaceTree(NamespaceTreeResponse),
    GovernancePolicyDump(GovernancePolicySnapshot),
    ConflictsList(ConflictsListResponse),
    QuarantineResolve(QuarantineResolveResponse),
    TestInjectEvent(TestInjectEventResponse),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureSourceMode {
    #[default]
    HttpStatic,
    LocalArtifact,
    PdfText,
    BrowserRendered,
    Screenshot,
    Authenticated,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceCapturePayload {
    #[serde(alias = "url")]
    pub source: String,
    #[serde(default)]
    pub mode: CaptureSourceMode,
    pub excerpts: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_path: Option<PathBuf>,
}

impl Default for SourceCapturePayload {
    fn default() -> Self {
        Self {
            source: String::new(),
            mode: CaptureSourceMode::HttpStatic,
            excerpts: Vec::new(),
            note: None,
            local_path: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DashboardRoiResponse {
    pub window_days: u16,
    pub promotion_rate: f64,
    pub promotion_precision: f64,
    pub refusal_breakdown: BTreeMap<String, u32>,
    pub dreaming: DreamingRoiSummary,
    pub reality_check_adherence: RealityCheckAdherenceSummary,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DreamingRoiSummary {
    pub candidates_generated: u32,
    pub promoted_silent: u32,
    pub entered_review_queue: u32,
    pub dropped: u32,
    pub review_queue_approval_rate: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RealityCheckAdherenceSummary {
    pub weeks_completed: u32,
    pub weeks_skipped: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationsRecentResponse {
    pub notifications: Vec<NotificationSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationSnapshot {
    pub id: String,
    pub kind: String,
    pub message: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PolicyEditorMutationResponse {
    pub accepted: bool,
    pub file_name: String,
    pub policies: Vec<GovernancePolicySummary>,
}

/// Entity index summary for daemon-side TUI inspectors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntitySummary {
    pub entity_id: String,
    pub label: String,
    pub aliases: Vec<String>,
    pub memory_count: usize,
    pub recent_memory_ids: Vec<MemoryId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InspectEntitiesResponse {
    pub entities: Vec<EntitySummary>,
}

/// Bounded event-log row for timeline/inbox consumers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventLogEntry {
    pub event_id: EventId,
    pub ts: DateTime<Utc>,
    pub device: String,
    pub seq: u64,
    pub kind: EventKind,
    pub memory_id: Option<MemoryId>,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventsLogPageResponse {
    pub entries: Vec<EventLogEntry>,
    pub next_since: Option<EventId>,
}

/// Namespace tree node used by command-palette jumps and inspector relationship blocks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NamespaceNode {
    pub name: String,
    pub path: String,
    pub memory_count: usize,
    pub children: Vec<NamespaceNode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NamespaceTreeResponse {
    pub root: NamespaceNode,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GovernancePolicySummary {
    pub scope: String,
    pub selected_policy: String,
    pub policy_source: String,
    pub confidence_floor: f32,
    pub review_gates: Vec<String>,
    pub requires_grounding: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GovernancePolicySnapshot {
    pub source: String,
    pub raw_yaml: Option<String>,
    pub policies: Vec<GovernancePolicySummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_file: Option<String>,
    #[serde(default)]
    pub writable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConflictSummary {
    pub id: MemoryId,
    pub path: String,
    pub summary: String,
    pub reason: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConflictsListResponse {
    pub conflicts: Vec<ConflictSummary>,
}

// No `ValueEnum`/`#[clap]` derive: this is never used as a clap value arg — the
// CLI resolves via a `--edited` bool flag (see `cli::QuarantineResolveArgs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuarantineResolutionMode {
    /// The operator resolved the conflict by editing the canonical file by hand
    /// and is certifying the current on-disk body as the resolution.
    ///
    /// This is the only mode the daemon can honestly perform: it promotes the
    /// current file to Active/Trusted after a conflict-marker check. True
    /// "accept ours"/"accept theirs" side-selection needs a substrate side-swap
    /// API that does not exist yet, so those modes were removed rather than
    /// advertise flags that silently took this same path.
    Edited,
}

impl QuarantineResolutionMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Edited => "edited",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuarantineResolveResponse {
    pub id: String,
    pub path: String,
    pub mode: QuarantineResolutionMode,
    pub remaining_blocking_conflicts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecallHitsResponse {
    pub since: Option<DateTime<Utc>>,
    pub limit: usize,
    pub hits: Vec<RecallHitSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureSourceResponse {
    pub artifact_id: String,
    pub source_refs: Vec<String>,
    #[serde(default)]
    pub mode: CaptureSourceMode,
    pub final_url: String,
    pub captured_at: DateTime<Utc>,
    pub capture_status: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecallHitSummary {
    pub event_id: String,
    pub device: String,
    pub seq: u64,
    pub memory_id: MemoryId,
    pub recalled_at: DateTime<Utc>,
    pub summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebDashboardStatus {
    pub running: bool,
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launch_url: Option<String>,
    pub port: Option<u16>,
    pub uptime_seconds: Option<u64>,
    pub active_connections: u32,
}

impl WebDashboardStatus {
    pub fn stopped() -> Self {
        Self { running: false, url: None, launch_url: None, port: None, uptime_seconds: None, active_connections: 0 }
    }

    pub fn running(port: u16, uptime_seconds: u64) -> Self {
        Self {
            running: true,
            url: Some(format!("http://localhost:{port}")),
            launch_url: None,
            port: Some(port),
            uptime_seconds: Some(uptime_seconds),
            active_connections: 0,
        }
    }

    pub fn running_with_launch_url(port: u16, uptime_seconds: u64, auth_token: &str) -> Self {
        let mut status = Self::running(port, uptime_seconds);
        status.launch_url = Some(format!("http://localhost:{port}/?auth={auth_token}"));
        status
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RealityCheckResponse {
    Pending {
        session_id: Option<String>,
        items: Vec<RealityCheckItem>,
        total_scored: usize,
        last_completed_at: Option<DateTime<Utc>>,
    },
    RespondAccepted {
        session_id: String,
        memory_id: MemoryId,
        next_item: Option<RealityCheckItem>,
        completion: RealityCheckCompletion,
    },
    RespondRefused {
        session_id: String,
        memory_id: MemoryId,
        reason: String,
        kind: RespondRefusalKind,
    },
    Snoozed {
        snooze_until: DateTime<Utc>,
    },
    Skipped {
        skipped_until: DateTime<Utc>,
    },
    Reset {
        cleared_pending: usize,
        cleared_session: bool,
    },
    History {
        sessions: Vec<RealityCheckHistorySession>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealityCheckHistorySession {
    pub session_id: String,
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
    pub items_total: usize,
    pub reviewed: u32,
    pub confirmed: u32,
    pub corrected: u32,
    pub forgotten: u32,
    pub not_relevant: u32,
    pub deferred: u32,
    pub remaining: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealityCheckItem {
    pub memory_id: MemoryId,
    pub title: String,
    pub namespace: String,
    pub status: MemoryStatus,
    pub sensitivity: Option<Sensitivity>,
    pub score: f64,
    pub component_scores: ComponentScores,
    pub encrypted: bool,
    pub last_observed_at: DateTime<Utc>,
    pub recall_count_30d: u32,
    pub last_recalled_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComponentScores {
    pub days_since_observed_norm: f64,
    pub recall_frequency_norm: f64,
    pub cross_source_corroboration: f64,
    pub confidence_decay: f64,
    pub sensitivity_weight: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RealityCheckCompletion {
    Progress { remaining: usize, deferred: usize },
    Complete { reviewed: usize, deferred: usize, completed_at: DateTime<Utc> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RespondRefusalKind {
    GovernanceRefused,
    TombstoneMatch,
    InvalidAction,
    SessionExpired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotificationEvent {
    LeakedSecretDetected { memory_id: MemoryId },
    BlockingMergeConflict { path: String },
    OperatorActionRequired { message: String },
    ReviewQueueOverThreshold { count: usize, threshold: usize },
    DreamRunCompleted { scope: String, promoted: usize, queued: usize, dropped: usize },
    RealityCheckDue { due_at: DateTime<Utc> },
    RealityCheckOverdue { last_completed_at: Option<DateTime<Utc>>, weeks_skipped: u32 },
    DailySynthesisSummaryReady { scope: String },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusResponse {
    pub state: String,
    pub guidance: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub daemon: Option<DaemonProcessStatus>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dashboard_warnings: Vec<String>,
    #[serde(default)]
    pub recall: RecallStatusCounters,
    #[serde(default)]
    pub embedding: EmbeddingStatus,
    #[serde(default)]
    pub dreams: DreamStatusCounters,
    #[serde(default)]
    pub passive_notifications: Vec<PassiveNotificationStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index_stats: Option<IndexStats>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_queue_counts: Option<ReviewQueueCounts>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conflicts_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub peer_sessions: Vec<PeerSessionStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer_update_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compact_dream_status: Option<CompactDreamStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbeddingStatus {
    pub state: String,
    pub load_count: u64,
    pub unload_count: u64,
    pub idle_unload_secs: Option<u64>,
    pub idle_unload_source: String,
    pub in_flight: usize,
    #[serde(default)]
    pub held_local_jobs: u64,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub row_kind_counts: BTreeMap<String, u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

impl Default for EmbeddingStatus {
    fn default() -> Self {
        Self {
            state: "unknown".to_string(),
            load_count: 0,
            unload_count: 0,
            idle_unload_secs: None,
            idle_unload_source: "unknown".to_string(),
            in_flight: 0,
            held_local_jobs: 0,
            row_kind_counts: BTreeMap::new(),
            last_error: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PassiveNotificationStatus {
    pub message: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexStats {
    pub active_memories: u64,
    pub last_reindex: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewQueueCounts {
    pub candidate: u64,
    pub quarantined: u64,
    pub dream_low_confidence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonProcessStatus {
    pub version: String,
    pub pid: u32,
    pub uptime_seconds: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactDreamStatus {
    pub enabled: bool,
    pub last_run_at: Option<DateTime<Utc>>,
    pub last_run_outcome: Option<PassStatus>,
    pub next_scheduled_at: Option<DateTime<Utc>>,
    pub active_leases: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum PeerActivityFormat {
    Human,
    Json,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PeerStatusResponse {
    pub coordination_level: u8,
    pub active_sessions: Vec<PeerSessionStatus>,
    pub claim_locks: Vec<ClaimLockInfo>,
    pub recent_deliveries: Vec<PeerDeliveryAuditEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerSessionStatus {
    pub session_id: String,
    pub harness: String,
    pub namespace: String,
    pub salient_entities: Vec<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub last_heartbeat_age_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PeerActivityResponse {
    pub entries: Vec<PeerDeliveryAuditEntry>,
    pub limit: usize,
    pub total_recorded: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PeerDeliveryAuditEntry {
    pub delivered_at: DateTime<Utc>,
    pub from_harness: String,
    pub from_session_id: String,
    pub to_harness: String,
    pub to_session_id: String,
    pub memory_id: String,
    pub relevance: f64,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerReleaseLockResponse {
    pub memory_id: String,
    pub status: PeerReleaseLockStatus,
    pub released: Option<ClaimLockInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PeerReleaseLockStatus {
    Released,
    NoLockFound,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObserveResponse {
    pub fragment_id: String,
    pub target: ObserveTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObserveTarget {
    PlaintextSubstrate,
    EncryptedSubstrate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DreamRunReport {
    pub scope: String,
    pub cli_used: Option<String>,
    pub pass_1: PassOutcome,
    pub pass_2: PassOutcome,
    #[serde(default)]
    pub pass_2_refusal_counts_by_reason: std::collections::BTreeMap<String, usize>,
    pub pass_3: PassOutcome,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PassOutcome {
    pub status: PassStatus,
    pub output_path: Option<String>,
    pub candidate_results: Vec<CandidateWriteResult>,
    pub error_code: Option<String>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateWriteResult {
    pub id: Option<String>,
    pub accepted: bool,
    pub reason: Option<String>,
    pub source_ref_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PassStatus {
    Success,
    Skipped,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DreamStatusReport {
    pub enabled: bool,
    pub last_runs: Vec<ScopeRunSummary>,
    pub active_leases: Vec<LeaseRecord>,
    pub cli_inventory: Vec<HarnessCliStatus>,
    pub counters: DreamStatusCounters,
    pub privacy_disclosure: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopeRunSummary {
    pub scope: String,
    pub last_run_at: Option<DateTime<Utc>>,
    pub last_run_outcome: Option<PassStatus>,
    pub last_run_cli: Option<String>,
    pub consecutive_missed_runs: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessCliStatus {
    pub name: String,
    pub is_installed: bool,
    pub is_authenticated: Option<bool>,
    pub prompt_transport: PromptTransport,
    pub last_probe_at: Option<DateTime<Utc>>,
    pub last_probe_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptTransport {
    Stdin,
    Argv,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaseRecord {
    pub device: String,
    pub scope: String,
    pub acquired_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub run_id: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DreamStatusCounters {
    #[serde(default)]
    pub substrate_fragments_written_total: BTreeMap<String, u64>,
    pub dream_runs_invoked_total: u64,
    #[serde(default)]
    pub dream_runs_failed_total: BTreeMap<String, u64>,
    #[serde(default)]
    pub pass_failed_total: BTreeMap<String, u64>,
    #[serde(default)]
    pub harness_cli_calls_total: BTreeMap<String, u64>,
    #[serde(default)]
    pub harness_cli_auth_failures_total: BTreeMap<String, u64>,
    pub cleanup_runs_invoked_total: u64,
    #[serde(default)]
    pub cleanup_findings_total: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DoctorResponse {
    pub healthy: bool,
    pub findings: Vec<DoctorFinding>,
    pub guidance: String,
    /// Per-row-kind embedding lifecycle counts.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub embedding_counts: BTreeMap<String, u64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub merge_proposal_counts: BTreeMap<String, u64>,
}

/// Severity of a doctor finding (F4 / I-F4.2). `Fatal` findings flip `healthy`
/// false (the loop is broken); `Advisory` findings keep `healthy` true but still
/// surface in `findings`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DoctorSeverity {
    /// A cut seam: the runtime loop is broken.
    Fatal,
    /// Surfaced but non-blocking; the loop still runs.
    #[default]
    Advisory,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DoctorFinding {
    pub code: String,
    pub message: String,
    pub repair: Option<String>,
    #[serde(default)]
    pub severity: DoctorSeverity,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchResponse {
    pub hits: Vec<SearchHit>,
    pub total: usize,
    pub guidance: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchHit {
    pub id: String,
    pub summary: String,
    pub snippet: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    pub score: f64,
}

/// The body string the daemon returns in place of encrypted-at-rest content.
/// Part of the wire contract: consumers that compare body content (import
/// supersede adoption) must treat this sentinel as "content unavailable" even
/// when talking to an older daemon that predates [`GetResponse::encrypted`].
pub const ENCRYPTED_BODY_SENTINEL: &str = "[encrypted content omitted]";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GetResponse {
    pub id: String,
    pub summary: String,
    pub body: String,
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<GetProvenance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sensitivity: Option<Sensitivity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<MemoryStatus>,
    /// True when the memory's content is encrypted at rest: `body` carries a
    /// redaction sentinel, never plaintext. Additive (F23) so import adoption
    /// can refuse hash comparison against a redacted body instead of silently
    /// mismatching and minting a duplicate supersede.
    #[serde(default)]
    pub encrypted: bool,
    pub guidance: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GetProvenance {
    pub path: Option<String>,
    pub source_kind: String,
    pub source_ref: Option<String>,
    pub author_kind: String,
    pub harness: Option<String>,
    pub session_id: Option<String>,
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RevealResponse {
    pub id: String,
    pub summary: String,
    pub body: String,
    pub truncated: bool,
    pub guidance: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriteNoteResponse {
    pub id: String,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GovernanceStatus {
    Promoted,
    Candidate,
    Quarantined,
    Refused,
    Tombstoned,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GovernanceWriteResponse {
    pub status: GovernanceStatus,
    pub id: Option<String>,
    pub namespace: Option<String>,
    pub reason: Option<GovernanceRefusalReason>,
    pub next_actions: Vec<String>,
    pub policy_applied: Option<String>,
    pub policy_source: Option<String>,
    pub existing_id: Option<String>,
    /// Decision-trace marker recording that embedding-backed contradiction
    /// detection ran degraded for this write — the configured embedding triple
    /// had no provider loaded, the provider's triple disagreed with the active
    /// triple, the active triple had no vector table yet, or the KNN/embedding
    /// step failed. When set, the "no contradiction" portion of the decision was
    /// reached *without* a real similarity backend, so an operator must not read
    /// a `promoted` status as "checked against the corpus and found nothing
    /// similar" (invariant 3: visible here, never silent). `None` on the normal
    /// path; serde-skipped when absent so existing response shapes are unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub similarity_degraded: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GovernanceSupersedeResponse {
    pub status: GovernanceStatus,
    pub new_id: Option<String>,
    pub old_id: Option<String>,
    pub reason: Option<GovernanceRefusalReason>,
    pub chain: Option<Value>,
    pub policy_applied: Option<String>,
    pub policy_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<ClaimLockWarning>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimLockWarning {
    pub code: String,
    pub message: String,
    pub holder: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GovernanceForgetResponse {
    pub status: GovernanceStatus,
    pub id: String,
    pub tombstone_ref: Option<String>,
    pub reason: Option<GovernanceRefusalReason>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewQueueResponse {
    pub items: Vec<ReviewQueueItemResponse>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewQueueItemResponse {
    pub id: String,
    pub summary: String,
    pub status: String,
    pub policy_applied: String,
    pub reason: Option<String>,
    pub next_actions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewDecisionResponse {
    pub id: String,
    pub status: String,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeReviewResponse {
    pub proposals: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolErrorCode {
    /// Admin/UI protocol methods are reachable through the daemon socket, CLI,
    /// and dashboard only. Stream G §5.7 and system-v0.2 §19 require MCP to
    /// reject Reality Check requests with this stable protocol code.
    MethodNotAllowedOnMcp,
}

impl ProtocolErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MethodNotAllowedOnMcp => "method_not_allowed_on_mcp",
        }
    }
}

impl ProtocolError {
    pub fn method_not_allowed_on_mcp() -> Self {
        Self {
            code: ProtocolErrorCode::MethodNotAllowedOnMcp.as_str().to_owned(),
            message: "request payload is not allowed through the MCP forwarder".to_owned(),
            retryable: false,
        }
    }
}

fn encode_json_line<T>(value: &T) -> serde_json::Result<String>
where
    T: Serialize,
{
    let mut line = serde_json::to_string(value)?;
    line.push('\n');
    Ok(line)
}
