//! Daemon transport for the execute phase: the `DaemonClient` trait, its
//! production `SocketDaemonClient` implementation, the request/outcome shapes,
//! and the error/payload-naming helpers. Moved verbatim from the former
//! single-file `pipeline.rs`.

use std::path::PathBuf;

use serde_json::Value;

use crate::import::{ImportError, ImportResult};
use crate::protocol::{GovernanceRefusalReason, GovernanceStatus, ProtocolError, ResponsePayload, ResponseResult};

/// Request shape for a `WriteMemory` daemon call. Bundling these into a struct
/// keeps the trait method's argument count manageable.
#[derive(Debug, Clone)]
pub struct WriteMemoryRequest {
    pub body: String,
    pub title: Option<String>,
    pub tags: Vec<String>,
    pub meta: Value,
}

/// Request shape for a `Supersede` daemon call.
#[derive(Debug, Clone)]
pub struct SupersedeRequest {
    pub old_id: String,
    pub content: String,
    pub reason: String,
    pub meta: Value,
}

/// A daemon client used by the execute phase. Production uses
/// [`SocketDaemonClient`] which forwards through `crate::client::request`.
/// Tests inject a `MockDaemonClient` over an in-memory script.
#[allow(async_fn_in_trait)]
pub trait DaemonClient {
    /// Issue a `RequestPayload::WriteMemory` with the given JSON-shaped meta.
    async fn write_memory(&mut self, request: WriteMemoryRequest) -> ImportResult<WriteMemoryOutcome>;

    /// Issue a `RequestPayload::Supersede` with the given prior id.
    async fn supersede(&mut self, request: SupersedeRequest) -> ImportResult<SupersedeOutcome>;
}

/// Production daemon client backed by the existing memoryd Unix socket.
pub struct SocketDaemonClient {
    socket_path: PathBuf,
    request_counter: usize,
}

impl SocketDaemonClient {
    /// Build a client pointed at the given memoryd socket.
    pub fn new(socket_path: PathBuf) -> Self {
        Self { socket_path, request_counter: 0 }
    }

    fn next_request_id(&mut self, prefix: &str) -> String {
        self.request_counter += 1;
        format!("{prefix}-{:06}", self.request_counter)
    }
}

impl DaemonClient for SocketDaemonClient {
    async fn write_memory(&mut self, request: WriteMemoryRequest) -> ImportResult<WriteMemoryOutcome> {
        let request_id = self.next_request_id("import-write");
        let payload = crate::protocol::RequestPayload::WriteMemory {
            body: request.body,
            title: request.title,
            tags: request.tags,
            meta: request.meta,
        };
        let envelope = crate::client::request(&self.socket_path, request_id, payload)
            .await
            .map_err(|error| ImportError::io(self.socket_path.clone(), std::io::Error::other(error.to_string())))?;
        let write = match envelope.result {
            ResponseResult::Success(ResponsePayload::GovernanceWrite(write)) => write,
            ResponseResult::Error(error) => return Err(daemon_protocol_error("WriteMemory", error)),
            ResponseResult::Success(payload) => return Err(unexpected_daemon_payload("WriteMemory", &payload)),
        };
        Ok(WriteMemoryOutcome {
            status: write.status,
            id: write.id,
            existing_id: write.existing_id,
            next_actions: write.next_actions,
            reason: write.reason,
        })
    }

    async fn supersede(&mut self, request: SupersedeRequest) -> ImportResult<SupersedeOutcome> {
        let request_id = self.next_request_id("import-supersede");
        let payload = crate::protocol::RequestPayload::Supersede {
            old_id: request.old_id,
            content: request.content,
            reason: request.reason,
            meta: request.meta,
        };
        let envelope = crate::client::request(&self.socket_path, request_id, payload)
            .await
            .map_err(|error| ImportError::io(self.socket_path.clone(), std::io::Error::other(error.to_string())))?;
        let supersede = match envelope.result {
            ResponseResult::Success(ResponsePayload::GovernanceSupersede(supersede)) => supersede,
            ResponseResult::Error(error) => return Err(daemon_protocol_error("Supersede", error)),
            ResponseResult::Success(payload) => return Err(unexpected_daemon_payload("Supersede", &payload)),
        };
        Ok(SupersedeOutcome { status: supersede.status, new_id: supersede.new_id, reason: supersede.reason })
    }
}

pub(super) fn daemon_protocol_error(operation: &str, error: ProtocolError) -> ImportError {
    ImportError::Parse {
        source_key: "<daemon>".to_string(),
        reason: format!(
            "{operation} failed with daemon error {}: {} (retryable={})",
            error.code, error.message, error.retryable
        ),
    }
}

pub(super) fn unexpected_daemon_payload(operation: &str, payload: &ResponsePayload) -> ImportError {
    ImportError::Parse {
        source_key: "<daemon>".to_string(),
        reason: format!("{operation} returned unexpected daemon payload {}", response_payload_kind(payload)),
    }
}

fn response_payload_kind(payload: &ResponsePayload) -> &'static str {
    match payload {
        ResponsePayload::Status(_) => "Status",
        ResponsePayload::Doctor(_) => "Doctor",
        ResponsePayload::Search(_) => "Search",
        ResponsePayload::Get(_) => "Get",
        ResponsePayload::TrustArtifact(_) => "TrustArtifact",
        ResponsePayload::CaptureSource(_) => "CaptureSource",
        ResponsePayload::DashboardRoi(_) => "DashboardRoi",
        ResponsePayload::NotificationsRecent(_) => "NotificationsRecent",
        ResponsePayload::PolicyValidate(_) => "PolicyValidate",
        ResponsePayload::PolicyWrite(_) => "PolicyWrite",
        ResponsePayload::RecallHits(_) => "RecallHits",
        ResponsePayload::Reveal(_) => "Reveal",
        ResponsePayload::WriteNote(_) => "WriteNote",
        ResponsePayload::GovernanceWrite(_) => "GovernanceWrite",
        ResponsePayload::GovernanceSupersede(_) => "GovernanceSupersede",
        ResponsePayload::GovernanceForget(_) => "GovernanceForget",
        ResponsePayload::ReviewQueue(_) => "ReviewQueue",
        ResponsePayload::ReviewApprove(_) => "ReviewApprove",
        ResponsePayload::ReviewReject(_) => "ReviewReject",
        ResponsePayload::Startup(_) => "Startup",
        ResponsePayload::Delta(_) => "Delta",
        ResponsePayload::PeerHeartbeat(_) => "PeerHeartbeat",
        ResponsePayload::PeerStatus(_) => "PeerStatus",
        ResponsePayload::PeerActivity(_) => "PeerActivity",
        ResponsePayload::PeerReleaseLock(_) => "PeerReleaseLock",
        ResponsePayload::Observe(_) => "Observe",
        ResponsePayload::DreamNow(_) => "DreamNow",
        ResponsePayload::DreamStatus(_) => "DreamStatus",
        ResponsePayload::WebStatus(_) => "WebStatus",
        ResponsePayload::RealityCheck(_) => "RealityCheck",
        ResponsePayload::InspectEntities(_) => "InspectEntities",
        ResponsePayload::EventsLogPage(_) => "EventsLogPage",
        ResponsePayload::NamespaceTree(_) => "NamespaceTree",
        ResponsePayload::GovernancePolicyDump(_) => "GovernancePolicyDump",
        ResponsePayload::ConflictsList(_) => "ConflictsList",
        ResponsePayload::TestInjectEvent(_) => "TestInjectEvent",
    }
}

/// Outcome of a `WriteMemory` daemon call, normalised so the execute loop can
/// branch on it without re-handling JSON.
#[derive(Debug, Clone)]
pub struct WriteMemoryOutcome {
    pub status: GovernanceStatus,
    pub id: Option<String>,
    pub existing_id: Option<String>,
    pub next_actions: Vec<String>,
    pub reason: Option<GovernanceRefusalReason>,
}

#[derive(Debug, Clone)]
pub struct SupersedeOutcome {
    pub status: GovernanceStatus,
    pub new_id: Option<String>,
    pub reason: Option<GovernanceRefusalReason>,
}
