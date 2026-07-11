//! Daemon transport for the execute phase: the `DaemonClient` trait, its
//! production `SocketDaemonClient` implementation, the request/outcome shapes,
//! and the error/payload-naming helpers. Moved verbatim from the former
//! single-file `pipeline.rs`.

use std::path::PathBuf;

use serde_json::Value;

use crate::import::{ImportError, ImportResult};
use crate::protocol::{
    GetResponse, GovernanceRefusalReason, GovernanceStatus, ProtocolError, ResponsePayload, ResponseResult,
};

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

    /// Return the memory ids that have superseded `id`, walking the chain.
    /// Used by the import supersede retry path to avoid re-writing a memory
    /// whose previous supersede was already issued before a crash.
    ///
    /// Error semantics (F20): a chain node that the daemon reports as
    /// `not_found` is provably gone — it is treated as a leaf (skipped, walk
    /// continues) so a dangling supersession link can never livelock the
    /// import. Any other daemon error fails closed and propagates.
    async fn get_superseded_by_chain(&mut self, id: &str) -> ImportResult<Vec<String>>;

    /// Fetch a memory by id. Used by the supersede retry path to check whether
    /// a candidate's content already exists in a supersession chain.
    /// `full_body` requests the unbounded body; the handler truncates at 4KiB
    /// when this is false.
    ///
    /// Returns `Ok(None)` when the daemon reports the id as `not_found` (F20:
    /// the node is provably gone — callers skip it rather than aborting). Any
    /// other daemon error fails closed and propagates.
    async fn get_memory(&mut self, id: &str, full_body: bool) -> ImportResult<Option<GetResponse>>;
}

/// Depth-bounded BFS frontier over a supersession chain, shared by the
/// production client and the test mock so the bound semantics (F22) cannot
/// drift between them: the walk is bounded by *depth* (hops from the root),
/// with an explicit total-node backstop against pathological fan-out — never
/// by collected-node count checked pre-hop, which stopped traversal to deeper
/// replacements once 16 siblings had been seen.
pub(super) struct ChainWalker {
    visited: std::collections::HashSet<String>,
    queue: std::collections::VecDeque<(String, usize)>,
    chain: Vec<String>,
}

impl ChainWalker {
    /// Maximum hops from the root memory. A real supersession chain is a short
    /// path; 16 hops is far beyond any legitimate history.
    pub(super) const MAX_DEPTH: usize = 16;
    /// Runaway backstop on total collected nodes (wide fan-out × depth).
    pub(super) const MAX_NODES: usize = 256;

    pub(super) fn new(root: &str) -> Self {
        let mut visited = std::collections::HashSet::new();
        visited.insert(root.to_string());
        let mut queue = std::collections::VecDeque::new();
        queue.push_back((root.to_string(), 0));
        Self { visited, queue, chain: Vec::new() }
    }

    /// Next node to expand, or `None` when the frontier is exhausted or the
    /// node backstop is hit. Nodes at `MAX_DEPTH` are yielded into the chain
    /// but never expanded (their links are not followed).
    pub(super) fn next_node(&mut self) -> Option<(String, usize)> {
        if self.chain.len() >= Self::MAX_NODES {
            return None;
        }
        self.queue.pop_front()
    }

    /// Record the supersession links discovered at `node_depth`, enqueueing
    /// unvisited children for expansion when within the depth bound.
    pub(super) fn push_links(&mut self, node_depth: usize, links: impl IntoIterator<Item = String>) {
        for next_id in links {
            if self.chain.len() >= Self::MAX_NODES {
                return;
            }
            if self.visited.insert(next_id.clone()) {
                self.chain.push(next_id.clone());
                if node_depth + 1 < Self::MAX_DEPTH {
                    self.queue.push_back((next_id, node_depth + 1));
                }
            }
        }
    }

    pub(super) fn into_chain(self) -> Vec<String> {
        self.chain
    }
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

    async fn get_superseded_by_chain(&mut self, id: &str) -> ImportResult<Vec<String>> {
        // F15: transitive client-side walk — the TrustArtifact endpoint returns
        // only immediate supersession links, so fetch one hop at a time.
        // F20/F22: bound semantics live in ChainWalker (depth-bounded, shared
        // with the test mock); a `not_found` node is a provably-gone leaf and
        // is skipped, any other daemon error fails closed.
        let mut walker = ChainWalker::new(id);
        while let Some((current_id, depth)) = walker.next_node() {
            let request_id = self.next_request_id("import-trust-artifact");
            let payload = crate::protocol::RequestPayload::TrustArtifact { id: current_id };
            let envelope = crate::client::request(&self.socket_path, request_id, payload)
                .await
                .map_err(|error| ImportError::io(self.socket_path.clone(), std::io::Error::other(error.to_string())))?;
            let links = match envelope.result {
                ResponseResult::Success(ResponsePayload::TrustArtifact(artifact)) => artifact.superseded_by,
                ResponseResult::Error(error) if error.code == "not_found" => continue,
                ResponseResult::Error(error) => return Err(daemon_protocol_error("TrustArtifact", error)),
                ResponseResult::Success(payload) => return Err(unexpected_daemon_payload("TrustArtifact", &payload)),
            };
            walker.push_links(depth, links.into_iter().map(|link| link.id.as_str().to_string()));
        }

        Ok(walker.into_chain())
    }

    async fn get_memory(&mut self, id: &str, full_body: bool) -> ImportResult<Option<GetResponse>> {
        let request_id = self.next_request_id("import-get");
        let payload = crate::protocol::RequestPayload::Get { id: id.to_string(), include_provenance: false, full_body };
        let envelope = crate::client::request(&self.socket_path, request_id, payload)
            .await
            .map_err(|error| ImportError::io(self.socket_path.clone(), std::io::Error::other(error.to_string())))?;
        match envelope.result {
            ResponseResult::Success(ResponsePayload::Get(response)) => Ok(Some(response)),
            ResponseResult::Error(error) if error.code == "not_found" => Ok(None),
            ResponseResult::Error(error) => Err(daemon_protocol_error("Get", error)),
            ResponseResult::Success(payload) => Err(unexpected_daemon_payload("Get", &payload)),
        }
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
        ResponsePayload::QuarantineResolve(_) => "QuarantineResolve",
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
