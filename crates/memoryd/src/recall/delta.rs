use std::collections::HashSet;
use std::time::{Duration as StdDuration, Instant};

use chrono::{DateTime, Utc};
use memorum_coordination::presence::ActivePeerQuery;
use memorum_coordination::{
    ClaimLockRegistry, CoordinationConfig, CoordinationInsertion, PeerPresenceEntry, PeerUpdateEntry,
    PeerWriteCandidate, PresenceRegistry, RelevanceGate, SessionContext,
};
use memory_privacy::{safe_plaintext_fragment, DeterministicPrivacyClassifier, SafeFragmentDecision};
use memory_substrate::config::load_local_device_config;
use memory_substrate::{ChunkQuery, MemoryStatus, RecallIndexQuery, RecallIndexRow, Scope, SourceKind, Substrate};

use crate::recall::error::RecallError;
use crate::recall::render::escape_xml_text;
use crate::recall::render::{emit_recall_hits, render_delta_frame, DeltaRecallItem};
use crate::recall::source_identity::peer_source_identity;
use crate::recall::types::{
    ConcurrentSessionMode, DeltaPeerDelivery, DeltaRequest, DeltaResponse, SessionBinding, DEFAULT_DELTA_BUDGET_TOKENS,
};

const DELTA_PEER_PRESENCE_CAP: usize = 4;
const AUDIT_SUMMARY_MAX_BYTES: usize = 240;

#[derive(Clone, Copy)]
pub struct DeltaCoordinationContext<'a> {
    pub config: &'a CoordinationConfig,
    pub presence: &'a PresenceRegistry,
    pub claim_locks: &'a ClaimLockRegistry,
    pub delivery_recorder: Option<&'a dyn DeltaPeerDeliveryRecorder>,
    pub peer_cooldown: Option<&'a dyn DeltaPeerCooldownStore>,
}

pub trait DeltaPeerDeliveryRecorder: Sync {
    fn record_delta_peer_delivery(&self, delivery: DeltaPeerDelivery);
}

pub trait DeltaPeerCooldownStore: Sync {
    fn surfaced_peer_writes(&self, session_binding: &SessionBinding) -> HashSet<String>;

    fn record_surfaced_peer_writes(&self, session_binding: &SessionBinding, memory_ids: &[String]);
}

pub async fn build_delta_response(substrate: &Substrate, request: DeltaRequest) -> Result<DeltaResponse, RecallError> {
    build_delta_response_inner(substrate, request, None).await
}

pub async fn build_delta_response_with_coordination(
    substrate: &Substrate,
    request: DeltaRequest,
    coordination: DeltaCoordinationContext<'_>,
) -> Result<DeltaResponse, RecallError> {
    build_delta_response_inner(substrate, request, Some(coordination)).await
}

async fn build_delta_response_inner(
    substrate: &Substrate,
    request: DeltaRequest,
    coordination: Option<DeltaCoordinationContext<'_>>,
) -> Result<DeltaResponse, RecallError> {
    let session_binding = validate_delta_request(&request).await?;
    let budget_tokens = request.budget_tokens.unwrap_or(DEFAULT_DELTA_BUDGET_TOKENS);
    let message = request.message.trim();
    let chunks = substrate
        .query_chunks(ChunkQuery { text: Some(message.to_owned()), triple: None, vector: None })
        .await
        .map_err(|error| RecallError::substrate_error(error.to_string()))?;

    let items = chunks
        .into_iter()
        .map(|chunk| DeltaRecallItem { id: chunk.memory_id.to_string(), text: chunk.text })
        .collect::<Vec<_>>();
    let delta_coordination = match coordination {
        Some(context) => build_delta_coordination(substrate, &session_binding, message, context).await?,
        None => DeltaCoordination::default(),
    };
    let rendered = render_delta_frame(&items, budget_tokens, delta_coordination.insertion.as_ref());
    emit_recall_hits(substrate, rendered.included_item_ids.iter().map(String::as_str));

    let peer_deliveries = delta_coordination
        .insertion
        .as_ref()
        .map(|insertion| rendered_peer_deliveries(insertion, &rendered.block, &session_binding, Utc::now()))
        .unwrap_or_default();
    if let Some(context) = coordination {
        record_surfaced_peer_writes(context.peer_cooldown, &session_binding, &peer_deliveries);
        record_peer_deliveries(context.delivery_recorder, peer_deliveries);
    }

    Ok(DeltaResponse {
        delta_block: rendered.block,
        budget_used_tokens: rendered.budget_used_tokens,
        guidance: delta_guidance(rendered.included_item_ids.is_empty()),
    })
}

#[derive(Default)]
struct DeltaCoordination {
    insertion: Option<CoordinationInsertion>,
}

async fn build_delta_coordination(
    substrate: &Substrate,
    session_binding: &SessionBinding,
    message: &str,
    context: DeltaCoordinationContext<'_>,
) -> Result<DeltaCoordination, RecallError> {
    let level = effective_coordination_level(session_binding, context.config.level);
    if level < 2 {
        return Ok(DeltaCoordination::default());
    }

    let now = Utc::now();
    let mut session = delta_session_context(session_binding, message);
    if session.is_observe_only_harness() {
        return Ok(DeltaCoordination::default());
    }
    let recency_cutoff = now
        - chrono::Duration::try_seconds(context.config.relevance_gate.recency_window_seconds as i64)
            .unwrap_or(chrono::Duration::MAX);
    let rows = delta_peer_candidate_rows(substrate, session_binding)
        .await?
        .into_iter()
        .filter(|row| row.indexed_at >= recency_cutoff)
        .collect::<Vec<_>>();
    if let Some(cooldown) = context.peer_cooldown {
        session.surfaced_peer_writes = cooldown.surfaced_peer_writes(session_binding);
    }
    let mut insertion = RelevanceGate::new(context.config.clone()).evaluate(
        &mut session,
        &peer_write_candidates(substrate, session_binding, &rows).await,
        now,
    );
    attach_claim_locks(&mut insertion.peer_updates, context.claim_locks);

    if level >= 3 {
        let presence =
            active_peer_presence(&session, session_binding, context.presence, context.config.presence.stale_after());
        insertion.capped_peer_presence = presence.capped_count;
        insertion.peer_presence = presence.entries;
    }

    Ok(DeltaCoordination { insertion: insertion.has_entries().then_some(insertion) })
}

fn effective_coordination_level(session_binding: &SessionBinding, default_coordination_level: u8) -> u8 {
    match session_binding.project.as_ref().and_then(|project| project.concurrent_session_mode) {
        Some(ConcurrentSessionMode::Minimal) => 1,
        Some(ConcurrentSessionMode::Default) => 2,
        Some(ConcurrentSessionMode::Collaborative) => 3,
        None => default_coordination_level,
    }
}

async fn delta_peer_candidate_rows(
    substrate: &Substrate,
    session_binding: &SessionBinding,
) -> Result<Vec<RecallIndexRow>, RecallError> {
    let local_device_id = local_device_id(substrate)?;
    let mut rows = Vec::new();
    for namespace_prefix in &session_binding.namespaces_in_scope {
        rows.extend(
            substrate
                .query_recall_index(RecallIndexQuery {
                    namespace_prefix: Some(namespace_prefix.clone()),
                    statuses: vec![MemoryStatus::Active, MemoryStatus::Pinned, MemoryStatus::Candidate],
                    passive_recall_only: true,
                    updated_since: None,
                    match_terms: Vec::new(),
                })
                .await
                .map_err(|error| RecallError::substrate_error(error.to_string()))?
                .into_iter()
                .filter(|row| row.source_device.as_deref().is_none_or(|device_id| device_id == local_device_id))
                .filter(is_peer_write_row),
        );
    }
    rows.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    rows.dedup_by(|left, right| left.id == right.id);
    Ok(rows)
}

fn local_device_id(substrate: &Substrate) -> Result<String, RecallError> {
    load_local_device_config(&substrate.roots().runtime)
        .map_err(RecallError::substrate_error)?
        .map(|config| config.device.id)
        .ok_or_else(|| RecallError::substrate_error("local device identity missing"))
}

fn is_peer_write_row(row: &RecallIndexRow) -> bool {
    matches!(row.source_kind, SourceKind::AgentPrimary | SourceKind::AgentSubagent)
}

fn delta_session_context(session_binding: &SessionBinding, message: &str) -> SessionContext {
    let mut session = SessionContext {
        session_id: session_binding.session_id.clone(),
        harness: session_binding.harness.clone(),
        namespaces_in_scope: session_binding.namespaces_in_scope.clone(),
        ..SessionContext::default()
    };
    session.salient_entities = message_entity_ids(message);
    session.salient_paths = message_paths(message);
    if let Some(project) = &session_binding.project {
        insert_normalized(&mut session.salient_entities, &project.canonical_id);
        if let Some(alias) = &project.alias {
            insert_normalized(&mut session.salient_entities, alias);
        }
    }
    session
}

fn message_entity_ids(message: &str) -> HashSet<String> {
    message
        .split(|character: char| {
            character.is_whitespace() || matches!(character, ',' | ';' | ':' | '"' | '\'' | '<' | '>')
        })
        .filter(|token| token.contains("ent_") || token.contains("proj_"))
        .filter_map(normalized_non_empty)
        .collect()
}

fn message_paths(message: &str) -> HashSet<String> {
    message
        .split_whitespace()
        .map(|token| {
            token.trim_matches(|character: char| {
                matches!(character, ',' | ';' | ':' | '"' | '\'' | '<' | '>' | '(' | ')' | '[' | ']')
            })
        })
        .filter(|token| token.contains('/'))
        .map(ToOwned::to_owned)
        .collect()
}

fn insert_normalized(values: &mut HashSet<String>, value: &str) {
    if let Some(value) = normalized_non_empty(value) {
        values.insert(value);
    }
}

fn normalized_non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim().to_ascii_lowercase();
    (!trimmed.is_empty()).then_some(trimmed)
}

async fn peer_write_candidates(
    substrate: &Substrate,
    session_binding: &SessionBinding,
    rows: &[RecallIndexRow],
) -> Vec<PeerWriteCandidate> {
    let mut candidates = Vec::new();
    for row in rows {
        let identity = peer_source_identity(substrate, row).await;
        if identity.matches_session(session_binding) {
            continue;
        }
        candidates.push(PeerWriteCandidate {
            memory_id: row.id.clone(),
            row: row.clone(),
            paths: vec![row.path.as_str().to_owned()],
            harness: identity.harness,
            session_id: identity.session_id,
            namespace: namespace_for_row(row),
            embedding: None,
        });
    }
    candidates
}

fn namespace_for_row(row: &RecallIndexRow) -> String {
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

fn attach_claim_locks(peer_updates: &mut [PeerUpdateEntry], claim_locks: &ClaimLockRegistry) {
    for update in peer_updates {
        update.claim_locked = claim_locks.get(&update.reference);
    }
}

struct DeltaPresence {
    entries: Vec<PeerPresenceEntry>,
    capped_count: u32,
}

fn active_peer_presence(
    session: &SessionContext,
    session_binding: &SessionBinding,
    presence: &PresenceRegistry,
    stale_after: StdDuration,
) -> DeltaPresence {
    let mut records = presence.active_peers(ActivePeerQuery {
        namespace: &session_namespace(session_binding),
        own_session_id: Some(&session_binding.session_id),
        now: Instant::now(),
        stale_threshold: stale_after,
    });
    records.retain(|record| presence_record_overlaps_session(record, session));
    records.sort_by(|left, right| {
        presence_entity_overlap_score(right, session)
            .cmp(&presence_entity_overlap_score(left, session))
            .then_with(|| left.harness.cmp(&right.harness))
            .then_with(|| left.session_id.cmp(&right.session_id))
    });

    let capped_count = records.len().saturating_sub(DELTA_PEER_PRESENCE_CAP).try_into().unwrap_or(u32::MAX);
    let entries = records
        .into_iter()
        .take(DELTA_PEER_PRESENCE_CAP)
        .map(|record| PeerPresenceEntry {
            harness: record.harness,
            session_id: record.session_id,
            salient_entities: record.salient_entities,
            started_at: record.started_at.unwrap_or_else(Utc::now),
        })
        .collect();

    DeltaPresence { entries, capped_count }
}

fn presence_entity_overlap_score(record: &memorum_coordination::PresenceRecord, session: &SessionContext) -> usize {
    record
        .salient_entities
        .iter()
        .filter(|entity| {
            normalized_non_empty(entity).is_some_and(|entity| session.salient_entities.contains(entity.as_str()))
        })
        .count()
}

fn presence_record_overlaps_session(record: &memorum_coordination::PresenceRecord, session: &SessionContext) -> bool {
    record.salient_entities.iter().any(|entity| {
        normalized_non_empty(entity).is_some_and(|entity| session.salient_entities.contains(entity.as_str()))
    }) || record
        .salient_paths
        .iter()
        .map(|path| path.trim())
        .any(|path| !path.is_empty() && session.salient_paths.contains(path))
}

fn session_namespace(session_binding: &SessionBinding) -> String {
    session_binding
        .project
        .as_ref()
        .map(|project| format!("project:{}", project.canonical_id))
        .unwrap_or_else(|| "agent".to_owned())
}

fn rendered_peer_deliveries(
    insertion: &CoordinationInsertion,
    rendered_block: &str,
    session_binding: &SessionBinding,
    delivered_at: DateTime<Utc>,
) -> Vec<DeltaPeerDelivery> {
    insertion
        .peer_updates
        .iter()
        .filter(|update| peer_update_was_rendered(rendered_block, &update.reference))
        .map(|update| DeltaPeerDelivery {
            delivered_at,
            from_harness: update.harness.clone(),
            from_session_id: update.session_id.clone(),
            to_harness: session_binding.harness.clone(),
            to_session_id: session_binding.session_id.clone(),
            memory_id: update.reference.clone(),
            relevance: update.relevance,
            summary: safe_audit_summary(&update.summary),
        })
        .collect()
}

fn record_peer_deliveries(recorder: Option<&dyn DeltaPeerDeliveryRecorder>, deliveries: Vec<DeltaPeerDelivery>) {
    let Some(recorder) = recorder else {
        return;
    };
    for delivery in deliveries {
        recorder.record_delta_peer_delivery(delivery);
    }
}

fn record_surfaced_peer_writes(
    cooldown: Option<&dyn DeltaPeerCooldownStore>,
    session_binding: &SessionBinding,
    deliveries: &[DeltaPeerDelivery],
) {
    let Some(cooldown) = cooldown else {
        return;
    };
    let memory_ids = deliveries.iter().map(|delivery| delivery.memory_id.clone()).collect::<Vec<_>>();
    cooldown.record_surfaced_peer_writes(session_binding, &memory_ids);
}

fn peer_update_was_rendered(rendered_block: &str, reference: &str) -> bool {
    let escaped_ref = escape_xml_text(reference);
    rendered_block.contains(&format!("<ref>{escaped_ref}</ref>"))
}

fn safe_audit_summary(summary: &str) -> String {
    let classifier = DeterministicPrivacyClassifier::new();
    match safe_plaintext_fragment(&classifier, summary) {
        SafeFragmentDecision::Allow => crate::recall::truncate_utf8_bytes(summary, AUDIT_SUMMARY_MAX_BYTES).value,
        SafeFragmentDecision::OmitEncryptedBodyHidden | SafeFragmentDecision::OmitReviewPending => {
            "[content not available — privacy classification pending]".to_owned()
        }
    }
}

fn delta_guidance(empty: bool) -> String {
    if empty {
        "No passive recall delta matched this turn.".to_owned()
    } else {
        "Stream E passive recall delta assembled through daemon protocol.".to_owned()
    }
}

async fn validate_delta_request(request: &DeltaRequest) -> Result<SessionBinding, RecallError> {
    if request.message.trim().is_empty() {
        return Err(RecallError::invalid_request("message must be non-empty"));
    }
    let budget = request.budget_tokens.unwrap_or(DEFAULT_DELTA_BUDGET_TOKENS);
    if !(128..=8_000).contains(&budget) {
        return Err(RecallError::invalid_request("budget_tokens must be in 128..=8000"));
    }
    crate::recall::binding::validate_session_fields(&request.cwd, &request.session_id, &request.harness).await
}

#[cfg(test)]
mod tests {
    use crate::recall::render::render_delta_item;

    #[test]
    fn delta_item_escapes_id_as_xml_attribute() {
        let rendered = render_delta_item("mem\" onclick=\"evil", "safe <text>");

        assert!(rendered.contains("id=\"mem&quot; onclick=&quot;evil\""));
        assert!(rendered.contains("safe &lt;text&gt;"));
        assert!(!rendered.contains("onclick=\"evil"));
    }
}
