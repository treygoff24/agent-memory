use chrono::{DateTime, Utc};
use memorum_coordination::PeerWriteCandidate;
use memory_substrate::config::load_local_device_config;
use memory_substrate::{RecallIndexRow, Scope, SourceKind, Substrate};

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

/// The recency cutoff for peer-write candidates: `now` minus a window in
/// seconds, saturating to `Duration::MAX` when the window overflows an `i64`.
/// Shared by the startup and delta peer-coordination passes so the cutoff math
/// can never silently drift between the two entry points.
pub(crate) fn recency_cutoff(now: DateTime<Utc>, seconds: u64) -> DateTime<Utc> {
    now - chrono::Duration::try_seconds(seconds as i64).unwrap_or(chrono::Duration::MAX)
}

/// The coordination namespace a peer-write row belongs to, derived from its
/// scope (and canonical namespace id for Project/Org). Shared by both recall
/// entry points so the namespace rules stay identical.
pub(crate) fn namespace_for_row(row: &RecallIndexRow) -> String {
    match row.scope {
        Scope::User => "me".to_owned(),
        Scope::Agent | Scope::Subagent => "agent".to_owned(),
        Scope::Project => row
            .canonical_namespace_id
            .as_ref()
            .map(|namespace| format!("project:{namespace}"))
            .unwrap_or_else(|| "project".to_owned()),
        Scope::Org => row
            .canonical_namespace_id
            .as_ref()
            .map(|namespace| format!("org:{namespace}"))
            .unwrap_or_else(|| "org".to_owned()),
    }
}

/// Build the peer-write candidates from already-filtered index rows, dropping
/// any row whose source identity matches the current session.
///
/// `paths_for_row` selects each candidate's `paths` field: the startup path
/// special-cases Project scope into `project:<canonical_id>` (see
/// [`project_scoped_candidate_paths`]), while the delta path passes the row's
/// plain path. The selector keeps that one divergence caller-owned so the rest
/// of the candidate-assembly logic (identity match, namespace, row move) stays
/// shared.
///
/// Rows are consumed by value: each surviving row moves into its
/// `PeerWriteCandidate` rather than being deep-cloned (including its
/// `Vec<Entity>`) a second time.
pub(crate) fn peer_write_candidates(
    session_binding: &SessionBinding,
    rows: Vec<RecallIndexRow>,
    paths_for_row: fn(&RecallIndexRow) -> Vec<String>,
) -> Vec<PeerWriteCandidate> {
    let mut candidates = Vec::new();
    for row in rows {
        let identity = peer_source_identity(&row);
        if identity.matches_session(session_binding) {
            continue;
        }
        candidates.push(PeerWriteCandidate {
            memory_id: row.id.clone(),
            paths: paths_for_row(&row),
            harness: identity.harness,
            session_id: identity.session_id,
            namespace: namespace_for_row(&row),
            row,
            embedding: None,
        });
    }
    candidates
}

/// Startup `paths_for_row` selector: Project-scope rows resolve to their
/// canonical `project:<id>` path, everything else to the row's plain path.
pub(crate) fn project_scoped_candidate_paths(row: &RecallIndexRow) -> Vec<String> {
    if row.scope == Scope::Project {
        if let Some(canonical_id) = &row.canonical_namespace_id {
            return vec![format!("project:{canonical_id}")];
        }
    }
    vec![row.path.as_str().to_owned()]
}

/// Delta `paths_for_row` selector: always the row's plain path.
pub(crate) fn plain_candidate_paths(row: &RecallIndexRow) -> Vec<String> {
    vec![row.path.as_str().to_owned()]
}
