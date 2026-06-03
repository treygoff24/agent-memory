use memory_substrate::config::load_local_device_config;
use memory_substrate::{Memory, RecallIndexRow, SourceKind, Substrate};

use crate::recall::error::RecallError;
use crate::recall::types::{ConcurrentSessionMode, SessionBinding};

#[derive(Debug)]
pub(crate) struct PeerSourceIdentity {
    pub(crate) harness: String,
    pub(crate) session_id: String,
}

impl PeerSourceIdentity {
    fn from_memory(memory: &Memory) -> Self {
        Self {
            harness: first_present([
                memory.frontmatter.source.harness.as_deref(),
                memory.frontmatter.author.harness.as_deref(),
            ]),
            session_id: first_present([
                memory.frontmatter.source.session_id.as_deref(),
                memory.frontmatter.author.session_id.as_deref(),
            ]),
        }
    }

    fn unknown() -> Self {
        Self { harness: "unknown".to_owned(), session_id: "unknown".to_owned() }
    }

    pub(crate) fn matches_session(&self, session_binding: &SessionBinding) -> bool {
        self.harness == session_binding.harness && self.session_id == session_binding.session_id
    }
}

pub(crate) async fn peer_source_identity(substrate: &Substrate, row: &RecallIndexRow) -> PeerSourceIdentity {
    match substrate.read_memory(&row.id).await {
        Ok(memory) => PeerSourceIdentity::from_memory(&memory),
        Err(_) => PeerSourceIdentity::unknown(),
    }
}

fn first_present<const N: usize>(values: [Option<&str>; N]) -> String {
    values.into_iter().flatten().map(str::trim).find(|value| !value.is_empty()).unwrap_or("unknown").to_owned()
}

/// Resolve the effective coordination level for a session: the project's
/// concurrent-session mode wins when set, otherwise the daemon default. Shared
/// by the startup and delta recall paths, which gate peer coordination on it.
pub(crate) fn effective_coordination_level(session_binding: &SessionBinding, default_coordination_level: u8) -> u8 {
    match session_binding.project.as_ref().and_then(|project| project.concurrent_session_mode) {
        Some(ConcurrentSessionMode::Minimal) => 1,
        Some(ConcurrentSessionMode::Default) => 2,
        Some(ConcurrentSessionMode::Collaborative) => 3,
        None => default_coordination_level,
    }
}

/// Whether a recall-index row was written by an agent (primary or subagent),
/// i.e. a peer write rather than a user/source write.
pub(crate) fn is_peer_write_row(row: &RecallIndexRow) -> bool {
    matches!(row.source_kind, SourceKind::AgentPrimary | SourceKind::AgentSubagent)
}

/// Read this device's identity id from local runtime state. Errors if the
/// identity is missing (a fresh clone must adopt before recall runs).
pub(crate) fn local_device_id(substrate: &Substrate) -> Result<String, RecallError> {
    load_local_device_config(&substrate.roots().runtime)
        .map_err(RecallError::substrate_error)?
        .map(|config| config.device.id)
        .ok_or_else(|| RecallError::substrate_error("local device identity missing"))
}
