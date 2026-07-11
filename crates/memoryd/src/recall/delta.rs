use std::collections::HashSet;
use std::time::{Duration as StdDuration, Instant};

use chrono::{DateTime, Utc};
use memorum_coordination::presence::ActivePeerQuery;
use memorum_coordination::{
    ClaimLockRegistry, CoordinationConfig, CoordinationInsertion, PeerPresenceEntry, PeerUpdateEntry, PresenceRegistry,
    RelevanceGate, SessionContext,
};
use memory_privacy::{safe_plaintext_fragment, DeterministicPrivacyClassifier, SafeFragmentDecision};
use memory_substrate::{AuxScope, ChunkQuery, MemoryStatus, RecallIndexQuery, RecallIndexRow, Substrate};

use crate::recall::error::RecallError;
use crate::recall::hybrid::{collect_hybrid_recall, HybridRecallDecision, VectorRecallContext};
use crate::recall::render::escape_xml_text;
use crate::recall::render::{
    cap_passive_block, emit_recall_hits, render_delta_frame, render_delta_frame_passive, DeltaRecallItem,
    RenderedDeltaFrame,
};
use crate::recall::source_identity::{
    effective_coordination_level, hydrate_peer_candidate_entities, is_peer_write_row, local_device_id,
    peer_write_candidates, plain_candidate_paths, recency_cutoff,
};
use crate::recall::types::{
    DeltaPeerDelivery, DeltaRequest, DeltaResponse, SessionBinding, DEFAULT_DELTA_BUDGET_TOKENS,
    HOOK_DELTA_BUDGET_TOKENS,
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
    build_delta_response_inner(substrate, request, None, None).await
}

pub async fn build_delta_response_with_coordination(
    substrate: &Substrate,
    request: DeltaRequest,
    coordination: DeltaCoordinationContext<'_>,
) -> Result<DeltaResponse, RecallError> {
    build_delta_response_inner(substrate, request, Some(coordination), None).await
}

pub async fn build_delta_response_with_vector_recall(
    substrate: &Substrate,
    request: DeltaRequest,
    vector_recall: VectorRecallContext,
) -> Result<DeltaResponse, RecallError> {
    build_delta_response_inner(substrate, request, None, Some(vector_recall)).await
}

pub async fn build_delta_response_with_vector_recall_and_coordination(
    substrate: &Substrate,
    request: DeltaRequest,
    coordination: DeltaCoordinationContext<'_>,
    vector_recall: VectorRecallContext,
) -> Result<DeltaResponse, RecallError> {
    build_delta_response_inner(substrate, request, Some(coordination), Some(vector_recall)).await
}

async fn build_delta_response_inner(
    substrate: &Substrate,
    request: DeltaRequest,
    coordination: Option<DeltaCoordinationContext<'_>>,
    vector_recall: Option<VectorRecallContext>,
) -> Result<DeltaResponse, RecallError> {
    let session_binding = validate_delta_request(&request).await?;
    let passive = request.passive;
    // Hook mode with no explicit budget uses the reduced delta budget so the
    // per-turn injection stays small and under the char cap (plan Decision 8).
    let budget_tokens =
        request.budget_tokens.unwrap_or(if passive { HOOK_DELTA_BUDGET_TOKENS } else { DEFAULT_DELTA_BUDGET_TOKENS });
    let message = request.message.trim();
    let (items, vector_recall_degraded) = collect_delta_items(substrate, message, vector_recall.as_ref()).await?;
    let delta_coordination = match coordination {
        Some(context) => build_delta_coordination(substrate, &session_binding, message, context).await?,
        None => DeltaCoordination::default(),
    };
    let rendered = if passive {
        let frame = render_delta_frame_passive(&items, budget_tokens, delta_coordination.insertion.as_ref());
        // Enforce the < 10k-char injection guard on the passive delta tail too.
        RenderedDeltaFrame { block: cap_passive_block(frame.block), ..frame }
    } else {
        render_delta_frame(&items, budget_tokens, delta_coordination.insertion.as_ref())
    };
    // Read-only recall (plan Decision 10): a passive hook must not append RecallHit
    // events, record peer-write cooldowns, or write the delivery audit — every one
    // is recall-path feedback that mutates ranking-adjacent or audit state.
    if !passive {
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
    }

    Ok(DeltaResponse {
        delta_block: rendered.block,
        budget_used_tokens: rendered.budget_used_tokens,
        guidance: delta_guidance(rendered.included_item_ids.is_empty()),
        vector_recall_degraded,
    })
}

async fn collect_delta_items(
    substrate: &Substrate,
    message: &str,
    vector_recall: Option<&VectorRecallContext>,
) -> Result<(Vec<DeltaRecallItem>, Option<String>), RecallError> {
    match collect_hybrid_recall(substrate, message, vector_recall).await {
        HybridRecallDecision::Fused { candidates, degraded } => Ok((
            candidates
                .into_iter()
                .map(|candidate| DeltaRecallItem { id: candidate.id, text: candidate.text })
                .collect(),
            degraded.map(str::to_owned),
        )),
        HybridRecallDecision::FtsOnly { degraded } => {
            let items = fts_delta_items(substrate, message).await?;
            Ok((items, degraded.map(ToOwned::to_owned)))
        }
    }
}

async fn fts_delta_items(substrate: &Substrate, message: &str) -> Result<Vec<DeltaRecallItem>, RecallError> {
    let chunks = substrate
        .query_chunks(ChunkQuery { text: Some(message.to_owned()), triple: None, vector: None })
        .await
        .map_err(|error| RecallError::substrate_error(error.to_string()))?;

    Ok(chunks.into_iter().map(|chunk| DeltaRecallItem { id: chunk.memory_id.to_string(), text: chunk.text }).collect())
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
    session.set_harness_registry(context.config.harness_registry());
    if session.is_observe_only_harness() {
        return Ok(DeltaCoordination::default());
    }
    let recency_cutoff = recency_cutoff(now, context.config.relevance_gate.recency_window_seconds);
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
        &peer_write_candidates(session_binding, rows, plain_candidate_paths),
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

async fn delta_peer_candidate_rows(
    substrate: &Substrate,
    session_binding: &SessionBinding,
) -> Result<Vec<RecallIndexRow>, RecallError> {
    let local_device_id = local_device_id(substrate)?;
    let mut rows = Vec::new();
    for namespace_prefix in &session_binding.namespaces_in_scope {
        // Fetch scalar-only: the device + `is_peer_write_row` filters key on
        // base-row fields, and only the surviving peer-write subset feeds the
        // relevance gate that reads `row.entities`. Entities are hydrated below
        // over just the survivors instead of every fetched row.
        rows.extend(
            substrate
                .query_recall_index(RecallIndexQuery {
                    namespace_prefix: Some(namespace_prefix.clone()),
                    statuses: vec![MemoryStatus::Active, MemoryStatus::Pinned, MemoryStatus::Candidate],
                    passive_recall_only: true,
                    updated_since: None,
                    match_terms: Vec::new(),
                    hydrate: AuxScope::None,
                    // Peer-write attribution reads source/author harness+session
                    // identity off each surviving row, so project those fields.
                    source_identity: true,
                    exclude_merge_non_servable: true,
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
    hydrate_peer_candidate_entities(substrate, &mut rows).await?;
    Ok(rows)
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
        "Memorum passive recall delta assembled through daemon protocol.".to_owned()
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
    use crate::recall::{build_delta_response, DeltaRequest};
    use memory_substrate::{
        Author, AuthorKind, ClassificationOutcome, EventContext, Frontmatter, Memory, MemoryId, MemoryStatus,
        MemoryType, RepoPath, RetrievalPolicy, Roots, Scope, Sensitivity, Source, SourceKind, Substrate, TrustLevel,
        WriteMode, WritePolicy, WriteRequest,
    };

    #[test]
    fn delta_item_escapes_id_as_xml_attribute() {
        let rendered = render_delta_item("mem\" onclick=\"evil", "safe <text>");

        assert!(rendered.contains("id=\"mem&quot; onclick=&quot;evil\""));
        assert!(rendered.contains("safe &lt;text&gt;"));
        assert!(!rendered.contains("onclick=\"evil"));
    }

    #[tokio::test]
    async fn passive_delta_leaves_store_byte_unchanged_while_active_delta_writes() {
        let fixture = DeltaFixture::new("dev_passivedelta").await;
        // A distinctive token in the body so the delta message matches via FTS.
        fixture.write_memory("mem_20260619_aaaaaaaaaaaaaaaa_000001", "passivedeltaneedle recall body fact").await;

        let before = fixture.snapshot();
        let passive = fixture.delta("passivedeltaneedle", true).await;
        assert!(!passive.delta_block.is_empty());
        let after_passive = fixture.snapshot();
        assert_eq!(before, after_passive, "passive delta must not mutate any on-disk store state");

        // Teeth: the active path records RecallHit events for matched items.
        let active = fixture.delta("passivedeltaneedle", false).await;
        assert!(active.delta_block.contains("mem_20260619_aaaaaaaaaaaaaaaa_000001"));
        let after_active = fixture.snapshot();
        assert_ne!(before, after_active, "active delta recall is expected to append RecallHit events");
    }

    struct DeltaFixture {
        _temp: tempfile::TempDir,
        roots: Roots,
        substrate: Substrate,
    }

    impl DeltaFixture {
        async fn new(device_id: &str) -> Self {
            let temp = tempfile::tempdir().expect("tempdir");
            let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
            let substrate = Substrate::init(
                roots.clone(),
                memory_substrate::InitOptions { force_unsafe_durability: true, device_id: Some(device_id.to_owned()) },
            )
            .await
            .expect("substrate init");
            Self { _temp: temp, roots, substrate }
        }

        async fn delta(&self, message: &str, passive: bool) -> crate::recall::DeltaResponse {
            build_delta_response(
                &self.substrate,
                DeltaRequest {
                    cwd: self.roots.repo.to_string_lossy().into_owned(),
                    session_id: "sess_delta".to_owned(),
                    harness: "claude-code".to_owned(),
                    message: message.to_owned(),
                    budget_tokens: None,
                    passive,
                },
            )
            .await
            .expect("delta recall")
        }

        async fn write_memory(&self, id: &str, body: &str) {
            let updated_at = chrono::Utc::now();
            let memory = Memory {
                frontmatter: Frontmatter {
                    schema_version: 1,
                    id: MemoryId::new(id),
                    memory_type: MemoryType::Pattern,
                    scope: Scope::User,
                    summary: body.to_owned(),
                    confidence: 0.8,
                    original_confidence: None,
                    trust_level: TrustLevel::Trusted,
                    sensitivity: Sensitivity::Internal,
                    status: MemoryStatus::Active,
                    created_at: updated_at,
                    updated_at,
                    observed_at: None,
                    author: Author {
                        kind: AuthorKind::Agent,
                        user_handle: None,
                        harness: Some("claude-code".to_owned()),
                        harness_version: None,
                        session_id: Some("sess_seed".to_owned()),
                        subagent_id: None,
                        phase: None,
                        component: Some("passive-delta-test".to_owned()),
                    },
                    namespace: None,
                    canonical_namespace_id: None,
                    tags: Vec::new(),
                    entities: Vec::new(),
                    aliases: Vec::new(),
                    source: Source {
                        kind: SourceKind::AgentPrimary,
                        reference: None,
                        harness: Some("claude-code".to_owned()),
                        harness_version: None,
                        session_id: Some("sess_seed".to_owned()),
                        subagent_id: None,
                        device: None,
                    },
                    evidence: Vec::new(),
                    requires_user_confirmation: false,
                    review_state: None,
                    supersedes: Vec::new(),
                    superseded_by: Vec::new(),
                    related: Vec::new(),
                    tombstone_events: Vec::new(),
                    retrieval_policy: RetrievalPolicy {
                        passive_recall: true,
                        max_scope: Scope::User,
                        mask_personal_for_synthesis: false,
                        index_body: true,
                        index_embeddings: false,
                    },
                    write_policy: WritePolicy {
                        human_review_required: false,
                        policy_applied: "passive-delta-test".to_owned(),
                        expected_base_hash: None,
                    },
                    merge_diagnostics: None,
                    abstraction: None,
                    cues: Vec::new(),
                    extras: Default::default(),
                },
                body: body.to_owned(),
                path: Some(RepoPath::new(format!("me/{id}.md"))),
            };
            self.substrate
                .write_memory(WriteRequest {
                    operation_id: None,
                    memory,
                    expected_base_hash: None,
                    write_mode: WriteMode::CreateNew,
                    index_projection: None,
                    event_context: EventContext::default(),
                    allow_best_effort_durability: true,
                    classification: ClassificationOutcome::Trusted,
                })
                .await
                .expect("write memory");
        }

        /// Digest of the canonical store state (event JSONL log, memory markdown,
        /// runtime `state/` markers) that read-only recall must not touch. Git
        /// plumbing and the derived SQLite index sidecars are volatile and excluded.
        fn snapshot(&self) -> Vec<(String, Vec<u8>)> {
            let mut entries = Vec::new();
            for root in [&self.roots.repo, &self.roots.runtime] {
                for entry in walkdir::WalkDir::new(root).sort_by_file_name() {
                    let entry = entry.expect("walk store");
                    if !entry.file_type().is_file() {
                        continue;
                    }
                    let rel = entry.path().strip_prefix(root).unwrap_or(entry.path()).to_string_lossy().into_owned();
                    if rel.starts_with(".git/") {
                        continue;
                    }
                    let is_canonical = rel.starts_with("events/")
                        || rel.ends_with(".md")
                        || rel.contains("/state/")
                        || rel.starts_with("state/");
                    if !is_canonical {
                        continue;
                    }
                    let bytes = std::fs::read(entry.path()).expect("read store file");
                    entries.push((format!("{}:{rel}", root.display()), bytes));
                }
            }
            entries.sort();
            entries
        }
    }
}
