use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::recall::{DeltaRequest, DeltaResponse, RecallStatusCounters, StartupRequest, StartupResponse};

pub use memory_governance::GovernanceRefusalReason;

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
    Search { query: String, limit: Option<usize>, include_body: bool },
    Get { id: String, include_provenance: bool },
    Reveal { id: String, reason: String },
    WriteNote { text: String },
    WriteMemory { body: String, title: Option<String>, tags: Vec<String>, meta: Value },
    Supersede { old_id: String, content: String, reason: String, meta: Value },
    Forget { id: String, reason: String },
    ReviewQueue { limit: Option<usize> },
    ReviewApprove { id: String },
    ReviewReject { id: String, reason: String },
    Startup(StartupRequest),
    Delta(DeltaRequest),
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusResponse {
    pub state: String,
    pub guidance: String,
    #[serde(default)]
    pub recall: RecallStatusCounters,
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
