use memory_substrate::config::load_local_device_config;
use memory_substrate::{RecallIndexRow, SourceKind, Substrate};

use crate::recall::error::RecallError;
use crate::recall::types::{ConcurrentSessionMode, SessionBinding};

#[derive(Debug)]
pub(crate) struct PeerSourceIdentity {
    pub(crate) harness: String,
    pub(crate) session_id: String,
}

impl PeerSourceIdentity {
    /// Resolve peer harness/session identity directly from the recall-index
    /// row. The `source.*`/`author.*` identity fields are projected into the
    /// index, so this needs zero file reads on the per-turn delta-recall path.
    fn from_row(row: &RecallIndexRow) -> Self {
        Self {
            harness: first_present([row.source_harness.as_deref(), row.author_harness.as_deref()]),
            session_id: first_present([row.source_session_id.as_deref(), row.author_session_id.as_deref()]),
        }
    }

    pub(crate) fn matches_session(&self, session_binding: &SessionBinding) -> bool {
        self.harness == session_binding.harness && self.session_id == session_binding.session_id
    }
}

pub(crate) fn peer_source_identity(row: &RecallIndexRow) -> PeerSourceIdentity {
    PeerSourceIdentity::from_row(row)
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

/// Hydrate `row.entities` for an already-filtered set of peer-candidate rows in
/// one batched index query.
///
/// Peer candidates are fetched scalar-only (`AuxScope::None`) and narrowed by
/// `is_peer_write_row`/scope/device before the relevance gate, which is the
/// only consumer of `row.entities`. Hydrating entities here — over just the
/// surviving ids — avoids the per-Active-row entity fan-out that the gate would
/// otherwise pay for rows it immediately discards.
pub(crate) async fn hydrate_peer_candidate_entities(
    substrate: &Substrate,
    rows: &mut [RecallIndexRow],
) -> Result<(), RecallError> {
    if rows.is_empty() {
        return Ok(());
    }
    let ids = rows.iter().map(|row| row.id.as_str().to_owned()).collect::<Vec<_>>();
    let mut entities_by_memory =
        substrate.entities_for_memories(&ids).await.map_err(|error| RecallError::substrate_error(error.to_string()))?;
    for row in rows {
        row.entities = entities_by_memory.remove(row.id.as_str()).unwrap_or_default();
    }
    Ok(())
}

/// Read this device's identity id from local runtime state. Errors if the
/// identity is missing (a fresh clone must adopt before recall runs).
pub(crate) fn local_device_id(substrate: &Substrate) -> Result<String, RecallError> {
    load_local_device_config(&substrate.roots().runtime)
        .map_err(RecallError::substrate_error)?
        .map(|config| config.device.id)
        .ok_or_else(|| RecallError::substrate_error("local device identity missing"))
}
