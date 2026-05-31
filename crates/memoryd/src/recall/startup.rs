use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use chrono::{DateTime, Duration, Utc};
use memorum_coordination::{
    CoordinationConfig, CoordinationInsertion, PeerUpdateEntry, PeerWriteCandidate, RelevanceGate, SessionContext,
};
use memory_substrate::config::load_local_device_config;
use memory_substrate::{MemoryStatus, RecallIndexQuery, RecallIndexRow, Scope, SourceKind, Substrate, SubstrateError};

use crate::reality_check::RcScheduler;
use crate::recall::budget::estimated_tokens;
use crate::recall::candidates::{collect_recall_candidates_from_index, RecallCollectionRequest};
use crate::recall::dream_questions::{select_pending_attention_questions, CAP_TOTAL};
use crate::recall::error::RecallError;
use crate::recall::rank::{select_ranked_candidates, RankingContext};
use crate::recall::render::{
    emit_recall_hits, escape_xml_text, render_memory_entry, render_pending_attention_body,
    render_startup_frame_with_cross_device_updates, CrossDeviceStartupUpdates, RecallEntry, RenderedRecallSection,
    StartupCoordinationRender,
};
use crate::recall::source_identity::peer_source_identity;
use crate::recall::types::{
    bounded_omissions, ConcurrentSessionMode, RecallExplanation, RecallSectionExplanation, RecallSectionName,
    SessionBinding, StartupRequest, StartupResponse, DEFAULT_STARTUP_BUDGET_TOKENS,
};
use crate::recall::validate_startup_request;
use crate::state::DaemonState;

const REALITY_CHECK_SURFACE_WINDOW: Duration = Duration::days(7);
const REALITY_CHECK_SURFACE_MARKER: &str = "reality-check-pending-attention.last";
const STARTUP_PEER_UPDATE_CAP: usize = 2;
const DEFAULT_COORDINATION_LEVEL: u8 = 2;

static REALITY_CHECK_SURFACED_AT: OnceLock<Mutex<BTreeMap<PathBuf, DateTime<Utc>>>> = OnceLock::new();

pub async fn build_startup_response(
    substrate: &Substrate,
    request: StartupRequest,
) -> Result<StartupResponse, RecallError> {
    let config = CoordinationConfig { level: DEFAULT_COORDINATION_LEVEL, ..CoordinationConfig::default() };
    build_startup_response_with_coordination_config(substrate, request, config).await
}

pub async fn build_startup_response_with_coordination_level(
    substrate: &Substrate,
    request: StartupRequest,
    default_coordination_level: u8,
) -> Result<StartupResponse, RecallError> {
    let config = CoordinationConfig { level: default_coordination_level, ..CoordinationConfig::default() };
    build_startup_response_with_coordination_config(substrate, request, config).await
}

pub async fn build_startup_response_with_coordination_config(
    substrate: &Substrate,
    request: StartupRequest,
    coordination_config: CoordinationConfig,
) -> Result<StartupResponse, RecallError> {
    let budget_tokens = request.budget_tokens.unwrap_or(DEFAULT_STARTUP_BUDGET_TOKENS);
    let include_recent = request.include_recent;
    let since_event_id = request.since_event_id.clone();
    let session_binding = validate_startup_request(request).await?;

    let updated_since = startup_updated_since_from_event(substrate, since_event_id.as_deref());

    let collection = collect_recall_candidates_from_index(
        substrate,
        RecallCollectionRequest {
            section: RecallSectionName::RecentMemory,
            namespace_prefixes: session_binding.namespaces_in_scope.clone(),
            updated_since,
        },
    )
    .await
    .map_err(map_substrate_error)?;
    let candidate_attention_count = count_candidate_attention(substrate, &session_binding.namespaces_in_scope).await?;

    let project_namespace = session_binding.project.as_ref().map(|project| project.canonical_id.clone());
    let ranking_now = collection.facts.iter().map(|candidate| candidate.row.updated_at).max().unwrap_or_default();
    let selected = select_ranked_candidates(
        RecallSectionName::RecentMemory,
        collection.facts,
        RankingContext { now: ranking_now, exact_project_namespace: project_namespace },
        budget_tokens.saturating_sub(128).max(1),
    );

    let included_memory_ids = if include_recent {
        selected.selected.iter().map(|candidate| candidate.id.clone()).collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    let recent_body = if include_recent {
        selected
            .selected
            .iter()
            .map(|candidate| {
                render_memory_entry(&RecallEntry {
                    id: candidate.id.clone(),
                    summary: candidate.candidate.row.summary.clone(),
                    snippet: None,
                    updated: candidate.candidate.row.updated_at.to_rfc3339(),
                    source_kind: candidate.candidate.row.source_kind.to_string(),
                    confidence: format!("{:.2}", candidate.candidate.row.confidence),
                })
            })
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        String::new()
    };

    let pending_attention_count = collection.pending_attention_count + candidate_attention_count;
    let review_attention_line = (pending_attention_count > 0)
        .then(|| format!("- {pending_attention_count} memory item(s) require review before factual recall."));
    let active_entity_ids = active_entity_ids(&selected);
    let dream_questions = select_pending_attention_questions(
        &substrate.roots().repo,
        &session_binding.namespaces_in_scope,
        &active_entity_ids,
    );
    let pending_attention_items = review_attention_line.into_iter().chain(dream_questions.lines).collect::<Vec<_>>();
    let include_reality_check_due = should_offer_reality_check(substrate, Utc::now());
    let rendered_pending_attention = render_pending_attention_body(pending_attention_items, include_reality_check_due);
    if rendered_pending_attention.reality_check_due_emitted {
        record_reality_check_surface(&substrate.roots().runtime, Utc::now());
    }
    let mut pending_attention_omissions = dream_questions.omitted_total;
    if rendered_pending_attention.omitted_count > 0 {
        *pending_attention_omissions.entry(CAP_TOTAL.to_owned()).or_default() +=
            rendered_pending_attention.omitted_count;
    }

    let sections = vec![
        RenderedRecallSection { name: RecallSectionName::Identity, body: identity_body(&session_binding) },
        RenderedRecallSection { name: RecallSectionName::ProjectState, body: project_body(&session_binding) },
        RenderedRecallSection { name: RecallSectionName::EntityRecall, body: String::new() },
        RenderedRecallSection { name: RecallSectionName::RecentMemory, body: recent_body },
        RenderedRecallSection { name: RecallSectionName::PendingAttention, body: rendered_pending_attention.body },
        RenderedRecallSection {
            name: RecallSectionName::RecallExplanation,
            body: "Deterministic passive recall from Memorum index rows.".to_owned(),
        },
    ];
    let startup_context = startup_context_from_selection(&session_binding, &selected);
    let startup_peer_updates =
        startup_peer_updates(substrate, &session_binding, &coordination_config, startup_context.clone()).await?;

    let section_token_estimates = section_token_estimates(&sections);
    let mut omissions = collection.omitted;
    omissions.extend(selected.omitted);
    let bounded = bounded_omissions(omissions);
    let mut explanation = RecallExplanation {
        budget_tokens,
        budget_used_tokens: 0,
        policy: crate::recall::STREAM_E_POLICY.to_owned(),
        sections: section_explanations(
            &section_token_estimates,
            selected.selected.iter().map(|candidate| candidate.id.clone()).collect(),
            bounded.omitted.len() as u32 + bounded.omitted_truncated_count,
        ),
        omitted: bounded.omitted,
        omitted_truncated_count: bounded.omitted_truncated_count,
    };
    let recall_block = render_startup_frame_with_stable_budget(
        &session_binding,
        &mut explanation,
        &sections,
        StartupCoordinationRender {
            same_device: startup_peer_updates.same_device.as_ref(),
            cross_device: startup_peer_updates.cross_device.as_ref(),
            salient_entities: Some(&startup_context.salient_entities),
        },
    );
    emit_recall_hits(substrate, included_memory_ids.iter().map(String::as_str));

    Ok(StartupResponse {
        session_binding,
        recall_block,
        budget_used_tokens: explanation.budget_used_tokens,
        recall_explanation: explanation,
        guidance: "Memorum passive recall assembled from read-only index projections.".to_owned(),
        dream_question_omissions: pending_attention_omissions,
    })
}

fn startup_updated_since_from_event(substrate: &Substrate, since_event_id: Option<&str>) -> Option<DateTime<Utc>> {
    let since_event_id = since_event_id?.trim();
    if since_event_id.is_empty() {
        return None;
    }
    match substrate.events() {
        Ok(events) => match events.into_iter().find(|event| event.id.as_str() == since_event_id) {
            Some(event) => Some(event.at),
            None => {
                tracing::warn!(since_event_id, "startup since_event_id not found; falling back to full startup recall");
                None
            }
        },
        Err(error) => {
            tracing::warn!(since_event_id, %error, "failed to read startup event log; falling back to full startup recall");
            None
        }
    }
}

#[derive(Debug, Default)]
struct StartupPeerUpdates {
    same_device: Option<CoordinationInsertion>,
    cross_device: Option<CrossDeviceStartupUpdates>,
}

async fn startup_peer_updates(
    substrate: &Substrate,
    session_binding: &SessionBinding,
    config: &CoordinationConfig,
    startup_context: SessionContext,
) -> Result<StartupPeerUpdates, RecallError> {
    if effective_coordination_level(session_binding, config.level) < 2 || startup_context.is_observe_only_harness() {
        return Ok(StartupPeerUpdates::default());
    }

    let rows = startup_peer_candidate_rows(substrate, session_binding).await?;
    if rows.is_empty() {
        return Ok(StartupPeerUpdates::default());
    }

    let local_device_id = local_device_id(substrate)?;
    let (same_device_rows, cross_device_rows): (Vec<_>, Vec<_>) = rows
        .into_iter()
        .partition(|row| row.source_device.as_deref().is_none_or(|device_id| device_id == local_device_id));

    let now = Utc::now();
    let evaluation = PeerUpdateEvaluation {
        substrate,
        session_binding,
        now,
        base_config: config,
        startup_context: &startup_context,
    };
    let same_device = same_device_updates(&evaluation, &same_device_rows).await;

    // I-R5: share the cool-down set across both passes. Peer-write ids surfaced
    // in the same-device pass must not be surfaced again in the cross-device
    // pass during the same startup (spec §4.2 single-session cool-down).
    // We extract the surfaced ids from the same-device result and seed the
    // startup_context clone used by the cross-device pass with them, so the
    // relevance gate's cool-down check suppresses duplicates.
    let same_device_surfaced = surfaced_peer_update_references(same_device.as_ref());
    let cross_device = cross_device_updates(&evaluation, &cross_device_rows, same_device_surfaced).await;

    Ok(StartupPeerUpdates { same_device, cross_device })
}

fn surfaced_peer_update_references(insertion: Option<&CoordinationInsertion>) -> HashSet<String> {
    insertion
        .map(|insertion| insertion.peer_updates.iter().map(|update| update.reference.clone()).collect())
        .unwrap_or_default()
}

fn effective_coordination_level(session_binding: &SessionBinding, default_coordination_level: u8) -> u8 {
    match session_binding.project.as_ref().and_then(|project| project.concurrent_session_mode) {
        Some(ConcurrentSessionMode::Minimal) => 1,
        Some(ConcurrentSessionMode::Default) => 2,
        Some(ConcurrentSessionMode::Collaborative) => 3,
        None => default_coordination_level,
    }
}

async fn startup_peer_candidate_rows(
    substrate: &Substrate,
    session_binding: &SessionBinding,
) -> Result<Vec<RecallIndexRow>, RecallError> {
    let mut rows = substrate
        .query_recall_index(RecallIndexQuery {
            namespace_prefix: None,
            statuses: vec![MemoryStatus::Active, MemoryStatus::Pinned],
            passive_recall_only: false,
            updated_since: None,
            match_terms: Vec::new(),
        })
        .await
        .map_err(map_substrate_error)?
        .into_iter()
        .filter(|row| row_is_in_startup_scope(row, session_binding))
        .filter(is_peer_write_row)
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    rows.dedup_by(|left, right| left.id == right.id);
    Ok(rows)
}

fn row_is_in_startup_scope(row: &RecallIndexRow, session_binding: &SessionBinding) -> bool {
    session_binding.namespaces_in_scope.iter().any(|namespace| match row.scope {
        Scope::User => namespace == "me",
        Scope::Agent | Scope::Subagent => namespace == "agent",
        Scope::Project => namespace
            .strip_prefix("project:")
            .is_some_and(|canonical_id| row.canonical_namespace_id.as_deref() == Some(canonical_id)),
        Scope::Org => namespace
            .strip_prefix("org:")
            .is_some_and(|canonical_id| row.canonical_namespace_id.as_deref() == Some(canonical_id)),
    })
}

struct PeerUpdateEvaluation<'a> {
    substrate: &'a Substrate,
    session_binding: &'a SessionBinding,
    now: DateTime<Utc>,
    base_config: &'a CoordinationConfig,
    startup_context: &'a SessionContext,
}

async fn same_device_updates(
    evaluation: &PeerUpdateEvaluation<'_>,
    rows: &[RecallIndexRow],
) -> Option<CoordinationInsertion> {
    let mut config = evaluation.base_config.clone();
    config.relevance_gate.per_turn_cap = STARTUP_PEER_UPDATE_CAP;
    let recency_cutoff = recency_cutoff(evaluation.now, config.relevance_gate.recency_window_seconds);
    let rows = rows.iter().filter(|row| row.indexed_at >= recency_cutoff).cloned().collect::<Vec<_>>();
    let mut session = evaluation.startup_context.clone();
    let candidates = peer_write_candidates(evaluation.substrate, evaluation.session_binding, &rows).await;
    let insertion = RelevanceGate::new(config).evaluate(&mut session, &candidates, evaluation.now);
    non_empty_insertion(insertion)
}

async fn cross_device_updates(
    evaluation: &PeerUpdateEvaluation<'_>,
    rows: &[RecallIndexRow],
    // I-R5: peer-write ids already surfaced in the same-device pass. These are
    // pre-seeded into the session clone's `surfaced_peer_writes` set so the
    // relevance gate's cool-down check suppresses any id that already appeared
    // in pass 1 (spec §4.2 single-session cool-down).
    already_surfaced: HashSet<String>,
) -> Option<CrossDeviceStartupUpdates> {
    if rows.is_empty() {
        return None;
    }

    let mut config = evaluation.base_config.clone();
    config.relevance_gate.threshold = config.relevance_gate.cross_device_startup_threshold;
    config.relevance_gate.recency_window_seconds = config.relevance_gate.cross_device_startup_window_seconds;
    config.relevance_gate.per_turn_cap = STARTUP_PEER_UPDATE_CAP;

    let recency_cutoff = recency_cutoff(evaluation.now, config.relevance_gate.recency_window_seconds);
    let rows = rows.iter().filter(|row| row.indexed_at >= recency_cutoff).cloned().collect::<Vec<_>>();
    let mut session = evaluation.startup_context.clone();
    // Seed the cool-down set with ids surfaced in the same-device pass.
    for id in already_surfaced {
        session.record_surfaced_peer_write(id);
    }
    let candidates = peer_write_candidates(evaluation.substrate, evaluation.session_binding, &rows).await;
    let insertion = RelevanceGate::new(config).evaluate(&mut session, &candidates, evaluation.now);
    let mut peer_updates = insertion.peer_updates;
    if peer_updates.is_empty() {
        return None;
    }
    for update in &mut peer_updates {
        update.device = Some("other".to_owned());
    }

    Some(CrossDeviceStartupUpdates { from_sync_date: from_sync_date(&rows, &peer_updates), peer_updates })
}

fn recency_cutoff(now: DateTime<Utc>, seconds: u64) -> DateTime<Utc> {
    now - chrono::Duration::try_seconds(seconds as i64).unwrap_or(chrono::Duration::MAX)
}

fn non_empty_insertion(insertion: CoordinationInsertion) -> Option<CoordinationInsertion> {
    (!insertion.peer_updates.is_empty()).then_some(insertion)
}

fn startup_context_from_selection(
    session_binding: &SessionBinding,
    selected: &crate::recall::rank::RankedSelection,
) -> SessionContext {
    let mut session = SessionContext {
        session_id: session_binding.session_id.clone(),
        harness: session_binding.harness.clone(),
        namespaces_in_scope: session_binding.namespaces_in_scope.clone(),
        ..SessionContext::default()
    };
    session.salient_entities = startup_salient_entities(session_binding, selected);
    session.salient_paths = selected
        .selected
        .iter()
        .filter(|candidate| !is_peer_write_row(&candidate.candidate.row))
        .map(|candidate| candidate.candidate.row.path.as_str().to_owned())
        .collect();
    if let Some(project) = &session_binding.project {
        session.salient_paths.insert(format!("project:{}", project.canonical_id));
    }
    session
}

fn startup_salient_entities(
    session_binding: &SessionBinding,
    selected: &crate::recall::rank::RankedSelection,
) -> HashSet<String> {
    let mut entities = selected
        .selected
        .iter()
        .filter(|candidate| !is_peer_write_row(&candidate.candidate.row))
        .flat_map(|candidate| {
            candidate.candidate.row.entities.iter().map(|entity| entity.id.trim().to_ascii_lowercase())
        })
        .filter(|entity| !entity.is_empty())
        .collect::<HashSet<_>>();

    if let Some(project) = &session_binding.project {
        entities.insert(project.canonical_id.trim().to_ascii_lowercase());
        if let Some(alias) = &project.alias {
            entities.insert(alias.trim().to_ascii_lowercase());
        }
    }

    entities
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
        let row = row.clone();
        candidates.push(PeerWriteCandidate {
            memory_id: row.id.clone(),
            paths: candidate_paths(&row),
            harness: identity.harness,
            session_id: identity.session_id,
            namespace: namespace_for_row(&row),
            row,
            embedding: None,
        });
    }
    candidates
}

fn candidate_paths(row: &RecallIndexRow) -> Vec<String> {
    if row.scope == Scope::Project {
        if let Some(canonical_id) = &row.canonical_namespace_id {
            return vec![format!("project:{canonical_id}")];
        }
    }
    vec![row.path.as_str().to_owned()]
}

fn namespace_for_row(row: &RecallIndexRow) -> String {
    match row.scope {
        Scope::User => "me".to_owned(),
        Scope::Agent => "agent".to_owned(),
        Scope::Subagent => "agent".to_owned(),
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

fn from_sync_date(rows: &[RecallIndexRow], peer_updates: &[PeerUpdateEntry]) -> String {
    let selected_ids = peer_updates.iter().map(|update| update.reference.as_str()).collect::<BTreeSet<_>>();
    rows.iter()
        .filter(|row| selected_ids.contains(row.id.as_str()))
        .map(|row| row.indexed_at)
        .max()
        .unwrap_or_else(Utc::now)
        .date_naive()
        .to_string()
}

fn is_peer_write_row(row: &RecallIndexRow) -> bool {
    matches!(row.source_kind, SourceKind::AgentPrimary | SourceKind::AgentSubagent)
}

fn local_device_id(substrate: &Substrate) -> Result<String, RecallError> {
    load_local_device_config(&substrate.roots().runtime)
        .map_err(RecallError::substrate_error)?
        .map(|config| config.device.id)
        .ok_or_else(|| RecallError::substrate_error("local device identity missing"))
}

fn should_offer_reality_check(substrate: &Substrate, now: DateTime<Utc>) -> bool {
    let state = DaemonState::load(&substrate.roots().runtime);
    state.reality_check.last_completed_at.is_some()
        && RcScheduler::default().is_due(&state.reality_check, now)
        && !recently_surfaced_reality_check(&substrate.roots().runtime, now)
}

fn recently_surfaced_reality_check(runtime_root: &Path, now: DateTime<Utc>) -> bool {
    let in_process = reality_check_surfaced_at()
        .lock()
        .expect("reality check pending-attention surface lock not poisoned")
        .get(runtime_root)
        .is_some_and(|surfaced_at| is_inside_reality_check_surface_window(surfaced_at, now));
    in_process || persisted_reality_check_surface_is_recent(runtime_root, now)
}

fn record_reality_check_surface(runtime_root: &Path, now: DateTime<Utc>) {
    reality_check_surfaced_at()
        .lock()
        .expect("reality check pending-attention surface lock not poisoned")
        .insert(runtime_root.to_path_buf(), now);
    if let Err(error) = write_reality_check_surface_marker(runtime_root, now) {
        eprintln!("WARN failed to persist reality_check_due pending-attention marker: {error}");
    }
}

fn reality_check_surfaced_at() -> &'static Mutex<BTreeMap<PathBuf, DateTime<Utc>>> {
    REALITY_CHECK_SURFACED_AT.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn persisted_reality_check_surface_is_recent(runtime_root: &Path, now: DateTime<Utc>) -> bool {
    fs::read_to_string(reality_check_surface_marker_path(runtime_root))
        .ok()
        .and_then(|value| DateTime::parse_from_rfc3339(value.trim()).ok())
        .map(|surfaced_at| is_inside_reality_check_surface_window(&surfaced_at.with_timezone(&Utc), now))
        .unwrap_or(false)
}

fn write_reality_check_surface_marker(runtime_root: &Path, now: DateTime<Utc>) -> std::io::Result<()> {
    let path = reality_check_surface_marker_path(runtime_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp_path = path.with_extension("tmp");
    fs::write(&temp_path, now.to_rfc3339())?;
    fs::rename(temp_path, path)
}

fn reality_check_surface_marker_path(runtime_root: &Path) -> PathBuf {
    runtime_root.join("state").join(REALITY_CHECK_SURFACE_MARKER)
}

fn is_inside_reality_check_surface_window(surfaced_at: &DateTime<Utc>, now: DateTime<Utc>) -> bool {
    now.signed_duration_since(*surfaced_at) < REALITY_CHECK_SURFACE_WINDOW
}

fn active_entity_ids(selected: &crate::recall::rank::RankedSelection) -> BTreeSet<String> {
    selected
        .selected
        .iter()
        .flat_map(|candidate| candidate.candidate.row.entities.iter().map(|entity| entity.id.clone()))
        .collect()
}

async fn count_candidate_attention(substrate: &Substrate, namespace_prefixes: &[String]) -> Result<usize, RecallError> {
    let mut total = 0usize;
    for namespace_prefix in namespace_prefixes {
        let rows = substrate
            .query_recall_index(RecallIndexQuery {
                namespace_prefix: Some(namespace_prefix.clone()),
                statuses: vec![MemoryStatus::Candidate, MemoryStatus::Quarantined],
                passive_recall_only: true,
                updated_since: None,
                match_terms: Vec::new(),
            })
            .await
            .map_err(map_substrate_error)?;
        total += rows.len();
    }
    Ok(total)
}

fn identity_body(session_binding: &SessionBinding) -> String {
    format!(
        "- harness: {}\n- session: {}\n- cwd: {}",
        escape_xml_text(&session_binding.harness),
        escape_xml_text(&session_binding.session_id),
        escape_xml_text(&session_binding.cwd)
    )
}

fn project_body(session_binding: &SessionBinding) -> String {
    match &session_binding.project {
        Some(project) => {
            let display = project.alias.as_deref().unwrap_or(&project.canonical_id);
            format!(
                "- project: {}\n- namespace: project:{}",
                escape_xml_text(display),
                escape_xml_text(&project.canonical_id)
            )
        }
        None => "- project: none".to_owned(),
    }
}

fn render_startup_frame_with_stable_budget(
    session_binding: &SessionBinding,
    explanation: &mut RecallExplanation,
    sections: &[RenderedRecallSection],
    startup_coordination: StartupCoordinationRender<'_>,
) -> String {
    for _ in 0..4 {
        let recall_block = render_startup_frame_with_cross_device_updates(
            session_binding,
            explanation,
            sections,
            startup_coordination,
        );
        let measured = estimated_tokens(&recall_block);
        if explanation.budget_used_tokens == measured {
            return recall_block;
        }
        explanation.budget_used_tokens = measured;
    }
    let recall_block =
        render_startup_frame_with_cross_device_updates(session_binding, explanation, sections, startup_coordination);
    explanation.budget_used_tokens = estimated_tokens(&recall_block);
    render_startup_frame_with_cross_device_updates(session_binding, explanation, sections, startup_coordination)
}

fn section_token_estimates(sections: &[RenderedRecallSection]) -> Vec<(RecallSectionName, usize)> {
    sections.iter().map(|section| (section.name, estimated_tokens(&section.body))).collect()
}

fn section_explanations(
    section_token_estimates: &[(RecallSectionName, usize)],
    recent_selected_ids: Vec<String>,
    recent_omitted_count: u32,
) -> Vec<RecallSectionExplanation> {
    RecallSectionName::STARTUP_ORDER
        .into_iter()
        .map(|name| {
            let selected_ids =
                if name == RecallSectionName::RecentMemory { recent_selected_ids.clone() } else { Vec::new() };
            let omitted_count = if name == RecallSectionName::RecentMemory { recent_omitted_count } else { 0 };
            RecallSectionExplanation {
                name,
                selected_ids,
                matched_entities: Vec::new(),
                budget_used_tokens: section_token_estimates
                    .iter()
                    .find_map(|(section, tokens)| (*section == name).then_some(*tokens))
                    .unwrap_or(0),
                omitted_count,
            }
        })
        .collect()
}

fn map_substrate_error(error: SubstrateError) -> RecallError {
    match error {
        SubstrateError::InvalidQuery { message, .. } => RecallError::invalid_request(message),
        other => RecallError::substrate_error(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recall::types::{ProjectBinding, ProjectBindingSource};

    #[test]
    fn startup_frame_budget_tokens_converge_at_digit_boundary() {
        let session_binding = SessionBinding {
            session_id: "sess".to_owned(),
            harness: "codex".to_owned(),
            harness_version: None,
            cwd: "/tmp".to_owned(),
            project: None,
            namespaces_in_scope: vec!["me".to_owned()],
        };
        let sections = RecallSectionName::STARTUP_ORDER
            .into_iter()
            .map(|name| RenderedRecallSection {
                name,
                body: if name == RecallSectionName::RecentMemory { "x".repeat(3_850) } else { String::new() },
            })
            .collect::<Vec<_>>();
        let mut explanation = RecallExplanation {
            budget_tokens: 3_600,
            budget_used_tokens: 0,
            policy: crate::recall::STREAM_E_POLICY.to_owned(),
            sections: Vec::new(),
            omitted: Vec::new(),
            omitted_truncated_count: 0,
        };

        let recall_block = render_startup_frame_with_stable_budget(
            &session_binding,
            &mut explanation,
            &sections,
            StartupCoordinationRender::default(),
        );

        assert_eq!(explanation.budget_used_tokens, estimated_tokens(&recall_block));
        assert!(recall_block.contains(&format!("used-tokens=\"{}\"", explanation.budget_used_tokens)));
    }

    #[test]
    fn identity_and_project_bodies_escape_xml_element_content() {
        let binding = SessionBinding {
            session_id: "sess</memory-recall><script>".to_owned(),
            harness: "codex&evil".to_owned(),
            harness_version: None,
            cwd: "/tmp/<cwd>".to_owned(),
            project: Some(ProjectBinding {
                canonical_id: "proj&agent".to_owned(),
                alias: Some("alias</project-state>".to_owned()),
                concurrent_session_mode: None,
                resolved_via: ProjectBindingSource::YamlOverride,
            }),
            namespaces_in_scope: Vec::new(),
        };

        let rendered = format!("{}\n{}", identity_body(&binding), project_body(&binding));

        assert!(rendered.contains("codex&amp;evil"));
        assert!(rendered.contains("sess&lt;/memory-recall&gt;&lt;script&gt;"));
        assert!(rendered.contains("/tmp/&lt;cwd&gt;"));
        assert!(rendered.contains("alias&lt;/project-state&gt;"));
        assert!(rendered.contains("proj&amp;agent"));
        assert!(!rendered.contains("</memory-recall><script>"));
    }

    #[test]
    fn surfaced_peer_update_references_extracts_memory_reference_keys_for_cross_device_cooldown() {
        let insertion = CoordinationInsertion {
            peer_updates: vec![PeerUpdateEntry {
                harness: "codex".to_owned(),
                session_id: "sess_same_device".to_owned(),
                timestamp: Utc::now(),
                relevance: 0.9,
                summary: "same-device summary must not become the cooldown key".to_owned(),
                reference: "mem_20260501_a1b2c3d4e5f60718_000777".to_owned(),
                namespace: "project:proj_stream_i".to_owned(),
                claim_locked: None,
                device: None,
            }],
            peer_presence: Vec::new(),
            capped_peer_updates: 0,
            capped_peer_presence: 0,
        };

        let surfaced = surfaced_peer_update_references(Some(&insertion));

        assert_eq!(surfaced, HashSet::from(["mem_20260501_a1b2c3d4e5f60718_000777".to_owned()]));
        assert!(!surfaced.contains("sess_same_device"));
        assert!(!surfaced.contains("same-device summary must not become the cooldown key"));
    }
}
