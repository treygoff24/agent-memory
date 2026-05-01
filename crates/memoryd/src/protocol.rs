use std::collections::BTreeMap;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::recall::{DeltaRequest, DeltaResponse, RecallStatusCounters, StartupRequest, StartupResponse};

pub use memory_governance::GovernanceRefusalReason;
pub use memory_substrate::ObserveKind;

/// Maximum byte length of a single newline-delimited request or response frame.
/// Defined here so both the server-side reader and client-side reader share the same limit.
pub const MAX_FRAME_BYTES: usize = 64 * 1024;

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
    Observe(ObserveResponse),
    DreamNow(Box<DreamRunReport>),
    DreamStatus(Box<DreamStatusReport>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusResponse {
    pub state: String,
    pub guidance: String,
    #[serde(default)]
    pub recall: RecallStatusCounters,
    #[serde(default)]
    pub dreams: DreamStatusCounters,
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

fn encode_json_line<T>(value: &T) -> serde_json::Result<String>
where
    T: Serialize,
{
    let mut line = serde_json::to_string(value)?;
    line.push('\n');
    Ok(line)
}
