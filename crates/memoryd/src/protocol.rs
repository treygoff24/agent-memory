use std::collections::BTreeMap;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::recall::{DeltaRequest, DeltaResponse, RecallStatusCounters, StartupRequest, StartupResponse};

pub use memorum_coordination::{ClaimLockInfo, PeerHeartbeat, PeerHeartbeatAck};
pub use memory_governance::GovernanceRefusalReason;
pub use memory_substrate::{MemoryId, MemoryStatus, ObserveKind, Sensitivity};

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
    },
    TrustArtifact {
        id: String,
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
pub enum ResponsePayload {
    Status(StatusResponse),
    Doctor(DoctorResponse),
    Search(SearchResponse),
    Get(GetResponse),
    TrustArtifact(Box<crate::trust_artifact::TrustArtifact>),
    RecallHits(RecallHitsResponse),
    Reveal(RevealResponse),
    WriteNote(WriteNoteResponse),
    GovernanceWrite(GovernanceWriteResponse),
    GovernanceSupersede(GovernanceSupersedeResponse),
    GovernanceForget(GovernanceForgetResponse),
    ReviewQueue(ReviewQueueResponse),
    ReviewApprove(ReviewDecisionResponse),
    ReviewReject(ReviewDecisionResponse),
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
    TestInjectEvent(TestInjectEventResponse),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecallHitsResponse {
    pub since: Option<DateTime<Utc>>,
    pub limit: usize,
    pub hits: Vec<RecallHitSummary>,
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
    pub port: Option<u16>,
    pub uptime_seconds: Option<u64>,
    pub active_connections: u32,
}

impl WebDashboardStatus {
    pub fn stopped() -> Self {
        Self { running: false, url: None, port: None, uptime_seconds: None, active_connections: 0 }
    }

    pub fn running(port: u16, uptime_seconds: u64) -> Self {
        Self {
            running: true,
            url: Some(format!("http://localhost:{port}")),
            port: Some(port),
            uptime_seconds: Some(uptime_seconds),
            active_connections: 0,
        }
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
    ReviewQueueOverThreshold { count: usize, threshold: usize },
    DreamRunCompleted { scope: String, promoted: usize, queued: usize, dropped: usize },
    RealityCheckDue { due_at: DateTime<Utc> },
    RealityCheckOverdue { last_completed_at: Option<DateTime<Utc>>, weeks_skipped: u32 },
    DailySynthesisSummaryReady { scope: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusResponse {
    pub state: String,
    pub guidance: String,
    #[serde(default)]
    pub recall: RecallStatusCounters,
    #[serde(default)]
    pub dreams: DreamStatusCounters,
    #[serde(default)]
    pub passive_notifications: Vec<PassiveNotificationStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PassiveNotificationStatus {
    pub message: String,
    pub created_at: DateTime<Utc>,
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

pub fn render_peer_status_human(status: &PeerStatusResponse) -> String {
    let mut output = String::new();
    output.push_str(&format!(
        "Coordination level: {} ({})\n\n",
        status.coordination_level,
        coordination_level_label(status.coordination_level)
    ));

    output.push_str("Active peer sessions (same device):\n");
    if status.active_sessions.is_empty() {
        output.push_str("  [none]\n");
    } else {
        for session in &status.active_sessions {
            let entities = if session.salient_entities.is_empty() {
                "[none]".to_owned()
            } else {
                session.salient_entities.join(", ")
            };
            output.push_str(&format!(
                "  {}:{}   project:{}   entities: {}\n",
                session.harness,
                truncated_session_id(&session.session_id),
                session.namespace,
                entities
            ));
            output.push_str(&format!(
                "  started {}, last heartbeat {} ago\n",
                session
                    .started_at
                    .map_or_else(|| "unknown".to_owned(), |started_at| { started_at.format("%H:%M").to_string() }),
                human_duration_seconds(session.last_heartbeat_age_seconds)
            ));
        }
    }

    output.push_str("\nActive claim locks:\n");
    if status.claim_locks.is_empty() {
        output.push_str("  [none]\n");
    } else {
        let now = Utc::now();
        for lock in &status.claim_locks {
            let ttl_seconds =
                lock.expires_at.signed_duration_since(now).to_std().map_or(0, |duration| duration.as_secs());
            output.push_str(&format!(
                "  {}   held by {}:{}   expires in {}\n",
                lock.memory_id,
                lock.holder_harness,
                lock.holder_session_id,
                human_duration_seconds(ttl_seconds)
            ));
        }
    }

    output.push_str("\nRecent peer-update deliveries (this session):\n");
    if status.recent_deliveries.is_empty() {
        output.push_str("  [none - run memoryd peer activity for session history]\n");
    } else {
        for delivery in &status.recent_deliveries {
            output.push_str(&format!(
                "  {}:{} -> {}:{}   {}   relevance={:.2}\n",
                delivery.from_harness,
                truncated_session_id(&delivery.from_session_id),
                delivery.to_harness,
                truncated_session_id(&delivery.to_session_id),
                delivery.memory_id,
                delivery.relevance
            ));
        }
    }

    output
}

pub fn render_peer_activity_human(activity: &PeerActivityResponse) -> String {
    let mut output = format!("Peer-update audit (last {} deliveries, this device):\n\n", activity.limit);
    if activity.entries.is_empty() {
        output.push_str("[none]\n");
        return output;
    }

    for entry in &activity.entries {
        output.push_str(&format!(
            "{}  {}:{} -> {}:{}   {}   relevance={:.2}\n",
            entry.delivered_at.format("%Y-%m-%d %H:%M"),
            entry.from_harness,
            truncated_session_id(&entry.from_session_id),
            entry.to_harness,
            truncated_session_id(&entry.to_session_id),
            entry.memory_id,
            entry.relevance
        ));
        output.push_str(&format!("  summary: \"{}\"\n\n", entry.summary));
    }
    output
}

fn coordination_level_label(level: u8) -> &'static str {
    match level {
        1 => "minimal",
        2 => "default - writes + candidates + notes",
        3 => "collaborative",
        _ => "unknown",
    }
}

fn truncated_session_id(session_id: &str) -> String {
    session_id.chars().take(6).collect()
}

fn human_duration_seconds(seconds: u64) -> String {
    if seconds < 60 {
        return format!("{seconds}s");
    }
    let minutes = seconds / 60;
    if minutes < 60 {
        return format!("{minutes}m {}s", seconds % 60);
    }
    format!("{}h {}m", minutes / 60, minutes % 60)
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DoctorFinding {
    pub code: String,
    pub message: String,
    pub repair: Option<String>,
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
    pub score: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GetResponse {
    pub id: String,
    pub summary: String,
    pub body: String,
    pub truncated: bool,
    pub guidance: String,
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
