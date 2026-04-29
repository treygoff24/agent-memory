use serde::{Deserialize, Serialize};

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
    WriteNote { text: String },
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
    WriteNote(WriteNoteResponse),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusResponse {
    pub state: String,
    pub guidance: String,
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
pub struct WriteNoteResponse {
    pub id: String,
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
