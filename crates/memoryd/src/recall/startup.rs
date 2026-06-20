use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, Utc};
use memorum_coordination::{
    CoordinationConfig, CoordinationInsertion, PeerUpdateEntry, PeerWriteCandidate, RelevanceGate, SessionContext,
};
use memory_substrate::{AuxScope, MemoryStatus, RecallIndexQuery, RecallIndexRow, Scope, Substrate, SubstrateError};

use crate::dynamics::{load_dynamics_config, DynamicsConfig};
use crate::reality_check::RcScheduler;
use crate::recall::budget::estimated_tokens;
use crate::recall::candidates::{
    collect_recall_candidates_from_index, hydrate_candidate_strength, RecallCandidate, RecallCollectionRequest,
    StrengthHydration,
};
use crate::recall::dedup_state::RecallDedupState;
use crate::recall::dream_questions::{select_pending_attention_questions, DreamQuestionSelection, CAP_TOTAL};
use crate::recall::error::RecallError;
use crate::recall::rank::{select_ranked_candidates, RankingContext};
use crate::recall::render::{
    cap_passive_block, emit_recall_hits, escape_xml_text, render_memory_entry, render_memory_entry_passive,
    render_pending_attention_body, render_startup_frame_passive, render_startup_frame_with_cross_device_updates,
    CrossDeviceStartupUpdates, RecallEntry, RenderedRecallSection, StartupCoordinationRender,
};
use crate::recall::source_identity::{
    effective_coordination_level, hydrate_peer_candidate_entities, is_peer_write_row, local_device_id,
    peer_write_candidates, project_scoped_candidate_paths, recency_cutoff,
};
use crate::recall::types::{
    bounded_omissions, RecallExplanation, RecallSectionExplanation, RecallSectionName, RecallStrength, SessionBinding,
    StartupRequest, StartupResponse, DEFAULT_STARTUP_BUDGET_TOKENS, HOOK_STARTUP_BUDGET_TOKENS,
};
use crate::recall::validate_startup_request;
use crate::state::DaemonState;

const REALITY_CHECK_SURFACE_WINDOW: Duration = Duration::days(7);
const REALITY_CHECK_SURFACE_MARKER: &str = "reality-check-pending-attention.last";
const STARTUP_PEER_UPDATE_CAP: usize = 2;
const DEFAULT_COORDINATION_LEVEL: u8 = 2;

pub async fn build_startup_response(
    substrate: &Substrate,
    request: StartupRequest,
) -> Result<StartupResponse, RecallError> {
    let config = CoordinationConfig { level: DEFAULT_COORDINATION_LEVEL, ..CoordinationConfig::default() };
    build_startup_response_with_coordination_config(substrate, request, config, &RecallDedupState::default()).await
}

pub async fn build_startup_response_with_coordination_config(
    substrate: &Substrate,
    request: StartupRequest,
    coordination_config: CoordinationConfig,
    dedup_state: &RecallDedupState,
) -> Result<StartupResponse, RecallError> {
    let passive = request.passive;
    // Hook mode with no explicit budget uses the reduced startup budget so the
    // rendered block stays under the Claude Code char cap (plan Decision 8).
    let budget_tokens = request.budget_tokens.unwrap_or(if passive {
        HOOK_STARTUP_BUDGET_TOKENS
    } else {
        DEFAULT_STARTUP_BUDGET_TOKENS
    });
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

    // Hydrate use-driven strength (memory-dynamics-v0.1 §3) before ranking, gated
    // by `dynamics.enabled`. The hydration is a synchronous SQLite read, so it runs
    // on the blocking pool to keep tokio workers free on the per-prompt hot path.
    // Soft failure: on a query error the candidates keep `strength = None` and
    // `dynamics_degraded` is flagged — never a hard recall failure.
    let dynamics = load_dynamics_config(substrate.roots().repo.as_path()).unwrap_or_else(|error| {
        tracing::warn!(%error, "dynamics: failed to load config; ranking structural-only");
        DynamicsConfig::default()
    });
    let StrengthHydrationResult { facts, alpha_points, dynamics_degraded } =
        hydrate_strength_for_ranking(substrate, &dynamics, collection.facts, ranking_now).await;

    let selected = select_ranked_candidates(
        RecallSectionName::RecentMemory,
        facts,
        RankingContext { now: ranking_now, exact_project_namespace: project_namespace, alpha_points },
        budget_tokens.saturating_sub(128).max(1),
    );
    let strengths = strength_metadata(&selected);

    // Native auto-memory dedup (plan Decision 8): Claude auto-loads the head of the
    // active project's `MEMORY.md` every session start, so a passive base block
    // suppresses recent-memory entries already present there to avoid double
    // injection. Read once per request and frozen, so the block stays
    // byte-deterministic for the identity tuple (reconciles with Decision 4).
    // Best-effort / Claude-only: an absent or unreadable file skips dedup.
    let native_memory_head = if passive { read_native_memory_head(&session_binding.cwd).await } else { None };

    let recent_candidates = selected
        .selected
        .iter()
        .filter(|candidate| {
            native_memory_head
                .as_ref()
                .is_none_or(|head| !native_head_contains_summary(head, &candidate.candidate.row.summary))
        })
        .collect::<Vec<_>>();

    let included_memory_ids = if include_recent {
        recent_candidates.iter().map(|candidate| candidate.id().to_owned()).collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    let recent_body = if include_recent {
        recent_candidates
            .iter()
            .map(|candidate| {
                let entry = RecallEntry {
                    id: candidate.id().to_owned(),
                    summary: candidate.candidate.row.summary.clone(),
                    snippet: None,
                    updated: candidate.candidate.row.updated_at.to_rfc3339(),
                    source_kind: candidate.candidate.row.source_kind.to_string(),
                    confidence: format!("{:.2}", candidate.candidate.row.confidence),
                };
                if passive {
                    render_memory_entry_passive(&entry)
                } else {
                    render_memory_entry(&entry)
                }
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
    // Read-only + cache safety (plan Decisions 4 & 10): dream-question selection
    // reads today's wall-clock date and *records* surfaced novelty hashes into the
    // daemon's dedup ring, so it both varies across sessions and mutates ranking-
    // adjacent state. A passive hook skips it entirely and surfaces only the
    // deterministic review-attention count.
    let dream_questions = if passive {
        DreamQuestionSelection { lines: Vec::new(), omitted_total: Default::default() }
    } else {
        // `select_pending_attention_questions` reads `dreams/questions/**` from disk
        // (read_dir + per-file read_to_string) and locks this daemon's dedup ring.
        // Run the whole sync routine on the blocking pool so it never stalls a tokio
        // worker on the per-prompt recall hot path. Inputs are cloned to satisfy the
        // 'static bound; the original `namespaces_in_scope` stays borrowable below.
        let repo = substrate.roots().repo.clone();
        let namespaces_in_scope = session_binding.namespaces_in_scope.clone();
        // Clone the `Arc` (not the store) into the blocking closure so the scan
        // shares this daemon's dedup ring without holding a guard across `.await`.
        let surfaced_store = dedup_state.recent_surfaced_questions().clone();
        tokio::task::spawn_blocking(move || {
            select_pending_attention_questions(&repo, &namespaces_in_scope, &active_entity_ids, &surfaced_store)
        })
        .await
        .expect("select_pending_attention_questions blocking task panicked")
    };
    let pending_attention_items = review_attention_line.into_iter().chain(dream_questions.lines).collect::<Vec<_>>();
    // Read-only + cache safety (plan Decisions 4 & 10): the reality-check-due item
    // reads the wall clock and mutable daemon state, and surfacing it records a
    // marker. A passive hook must neither vary across sessions nor write, so it is
    // omitted entirely (and the marker is never recorded).
    let include_reality_check_due = !passive && should_offer_reality_check(substrate, dedup_state, Utc::now()).await;
    let rendered_pending_attention = render_pending_attention_body(pending_attention_items, include_reality_check_due);
    if rendered_pending_attention.reality_check_due_emitted {
        debug_assert!(!passive, "passive recall must never surface the reality-check-due marker");
        record_reality_check_surface(dedup_state, &substrate.roots().runtime, Utc::now()).await;
    }
    let mut pending_attention_omissions = dream_questions.omitted_total;
    if rendered_pending_attention.omitted_count > 0 {
        *pending_attention_omissions.entry(CAP_TOTAL.to_owned()).or_default() +=
            rendered_pending_attention.omitted_count;
    }

    let sections = vec![
        RenderedRecallSection {
            name: RecallSectionName::Identity,
            // The session id is per-session and must not enter the cached prefix
            // (plan Decision 4); the passive identity body keys on harness + cwd
            // only, both of which are part of the identity tuple.
            body: if passive { passive_identity_body(&session_binding) } else { identity_body(&session_binding) },
        },
        RenderedRecallSection { name: RecallSectionName::ProjectState, body: project_body(&session_binding) },
        RenderedRecallSection { name: RecallSectionName::EntityRecall, body: String::new() },
        RenderedRecallSection { name: RecallSectionName::RecentMemory, body: recent_body },
        RenderedRecallSection { name: RecallSectionName::PendingAttention, body: rendered_pending_attention.body },
        RenderedRecallSection {
            name: RecallSectionName::RecallExplanation,
            body: "Deterministic passive recall from Memorum index rows.".to_owned(),
        },
    ];
    let startup_context = startup_context_from_selection(&session_binding, &selected)
        .with_harness_registry(coordination_config.harness_registry());
    // Cache safety (plan Decision 4): peer-update assembly reads the wall clock and
    // live peer presence (clock-stamped, session-varying), neither of which is part
    // of the passive identity tuple — so a passive base block omits coordination
    // entirely to stay byte-deterministic across sessions.
    let startup_peer_updates = if passive {
        StartupPeerUpdates::default()
    } else {
        startup_peer_updates(substrate, &session_binding, &coordination_config, startup_context.clone()).await?
    };

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
            // Mirror the rendered recent-memory set: for non-passive recall this is
            // every ranked candidate (dedup is inert), and for passive recall it
            // drops entries suppressed by the native MEMORY.md dedup.
            recent_candidates.iter().map(|candidate| candidate.id().to_owned()).collect(),
            bounded.omitted.len() as u32 + bounded.omitted_truncated_count,
        ),
        omitted: bounded.omitted,
        omitted_truncated_count: bounded.omitted_truncated_count,
        strengths,
        dynamics_degraded,
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
        passive,
    );
    // Read-only recall (plan Decision 10): recording recall hits appends RecallHit
    // events that feed memory-dynamics ranking. A passive hook fires on every
    // session/subagent, so it must never write — skip the feedback when passive.
    if !passive {
        emit_recall_hits(substrate, included_memory_ids.iter().map(String::as_str));
    }

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
    // Look the cursor up by primary key in the SQLite events_log mirror instead
    // of parsing the entire canonical JSONL log to recover one timestamp. Misses
    // fall back to a full startup recall exactly as before.
    match substrate.event_ts_by_id(since_event_id) {
        Ok(Some(at)) => Some(at),
        Ok(None) => {
            tracing::warn!(since_event_id, "startup since_event_id not found; falling back to full startup recall");
            None
        }
        Err(error) => {
            tracing::warn!(since_event_id, %error, "failed to read startup event mirror; falling back to full startup recall");
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
    let evaluation =
        PeerUpdateEvaluation { session_binding, now, base_config: config, startup_context: &startup_context };
    let same_device = same_device_updates(&evaluation, same_device_rows).await;

    // I-R5: share the cool-down set across both passes. Peer-write ids surfaced
    // in the same-device pass must not be surfaced again in the cross-device
    // pass during the same startup (spec §4.2 single-session cool-down).
    // We extract the surfaced ids from the same-device result and seed the
    // startup_context clone used by the cross-device pass with them, so the
    // relevance gate's cool-down check suppresses duplicates.
    let same_device_surfaced = surfaced_peer_update_references(same_device.as_ref());
    let cross_device = cross_device_updates(&evaluation, cross_device_rows, same_device_surfaced).await;

    Ok(StartupPeerUpdates { same_device, cross_device })
}

fn surfaced_peer_update_references(insertion: Option<&CoordinationInsertion>) -> HashSet<String> {
    insertion
        .map(|insertion| insertion.peer_updates.iter().map(|update| update.reference.clone()).collect())
        .unwrap_or_default()
}

async fn startup_peer_candidate_rows(
    substrate: &Substrate,
    session_binding: &SessionBinding,
) -> Result<Vec<RecallIndexRow>, RecallError> {
    // Fetch scalar-only: `is_peer_write_row`/scope filters key on base-row
    // fields, and only the surviving peer-write subset feeds the relevance gate
    // that reads `row.entities`. Hydrate entities afterward over just the
    // survivors instead of every Active row.
    let mut rows = substrate
        .query_recall_index(RecallIndexQuery {
            namespace_prefix: None,
            statuses: vec![MemoryStatus::Active, MemoryStatus::Pinned],
            passive_recall_only: false,
            updated_since: None,
            match_terms: Vec::new(),
            hydrate: AuxScope::None,
            // Peer-write attribution reads source/author harness+session identity
            // off each surviving row, so project those fields.
            source_identity: true,
        })
        .await
        .map_err(map_substrate_error)?
        .into_iter()
        .filter(|row| row_is_in_startup_scope(row, session_binding))
        .filter(is_peer_write_row)
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    rows.dedup_by(|left, right| left.id == right.id);
    hydrate_peer_candidate_entities(substrate, &mut rows).await?;
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
    session_binding: &'a SessionBinding,
    now: DateTime<Utc>,
    base_config: &'a CoordinationConfig,
    startup_context: &'a SessionContext,
}

async fn same_device_updates(
    evaluation: &PeerUpdateEvaluation<'_>,
    rows: Vec<RecallIndexRow>,
) -> Option<CoordinationInsertion> {
    let mut config = evaluation.base_config.clone();
    config.relevance_gate.per_turn_cap = STARTUP_PEER_UPDATE_CAP;
    let recency_cutoff = recency_cutoff(evaluation.now, config.relevance_gate.recency_window_seconds);
    let rows = rows.into_iter().filter(|row| row.indexed_at >= recency_cutoff).collect::<Vec<_>>();
    let mut session = evaluation.startup_context.clone();
    let candidates = peer_write_candidates(evaluation.session_binding, rows, project_scoped_candidate_paths);
    let insertion = RelevanceGate::new(config).evaluate(&mut session, &candidates, evaluation.now);
    non_empty_insertion(insertion)
}

async fn cross_device_updates(
    evaluation: &PeerUpdateEvaluation<'_>,
    rows: Vec<RecallIndexRow>,
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
    let rows = rows.into_iter().filter(|row| row.indexed_at >= recency_cutoff).collect::<Vec<_>>();
    let mut session = evaluation.startup_context.clone();
    // Seed the cool-down set with ids surfaced in the same-device pass.
    for id in already_surfaced {
        session.record_surfaced_peer_write(id);
    }
    let candidates = peer_write_candidates(evaluation.session_binding, rows, project_scoped_candidate_paths);
    let insertion = RelevanceGate::new(config).evaluate(&mut session, &candidates, evaluation.now);
    let mut peer_updates = insertion.peer_updates;
    if peer_updates.is_empty() {
        return None;
    }
    for update in &mut peer_updates {
        update.device = Some("other".to_owned());
    }

    Some(CrossDeviceStartupUpdates { from_sync_date: from_sync_date(&candidates, &peer_updates), peer_updates })
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

fn from_sync_date(candidates: &[PeerWriteCandidate], peer_updates: &[PeerUpdateEntry]) -> String {
    let selected_ids = peer_updates.iter().map(|update| update.reference.as_str()).collect::<BTreeSet<_>>();
    candidates
        .iter()
        .filter(|candidate| selected_ids.contains(candidate.memory_id.as_str()))
        .map(|candidate| candidate.row.indexed_at)
        .max()
        .unwrap_or_else(Utc::now)
        .date_naive()
        .to_string()
}

async fn should_offer_reality_check(substrate: &Substrate, dedup_state: &RecallDedupState, now: DateTime<Utc>) -> bool {
    // DaemonState::load is sync file IO with many sync callers; hop to the
    // blocking pool here rather than async-ifying it crate-wide.
    let runtime_root = substrate.roots().runtime.clone();
    let state = tokio::task::spawn_blocking(move || DaemonState::load(&runtime_root))
        .await
        .expect("DaemonState::load blocking task panicked");
    state.reality_check.last_completed_at.is_some()
        && RcScheduler::default().is_due(&state.reality_check, now)
        && !recently_surfaced_reality_check(dedup_state, &substrate.roots().runtime, now).await
}

async fn recently_surfaced_reality_check(
    dedup_state: &RecallDedupState,
    runtime_root: &Path,
    now: DateTime<Utc>,
) -> bool {
    let in_process = dedup_state
        .reality_check_surfaced_at()
        .lock()
        .expect("reality check pending-attention surface lock not poisoned")
        .get(runtime_root)
        .is_some_and(|surfaced_at| is_inside_reality_check_surface_window(surfaced_at, now));
    in_process || persisted_reality_check_surface_is_recent(runtime_root, now).await
}

async fn record_reality_check_surface(dedup_state: &RecallDedupState, runtime_root: &Path, now: DateTime<Utc>) {
    dedup_state
        .reality_check_surfaced_at()
        .lock()
        .expect("reality check pending-attention surface lock not poisoned")
        .insert(runtime_root.to_path_buf(), now);
    if let Err(error) = write_reality_check_surface_marker(runtime_root, now).await {
        eprintln!("WARN failed to persist reality_check_due pending-attention marker: {error}");
    }
}

async fn persisted_reality_check_surface_is_recent(runtime_root: &Path, now: DateTime<Utc>) -> bool {
    tokio::fs::read_to_string(reality_check_surface_marker_path(runtime_root))
        .await
        .ok()
        .and_then(|value| DateTime::parse_from_rfc3339(value.trim()).ok())
        .map(|surfaced_at| is_inside_reality_check_surface_window(&surfaced_at.with_timezone(&Utc), now))
        .unwrap_or(false)
}

async fn write_reality_check_surface_marker(runtime_root: &Path, now: DateTime<Utc>) -> std::io::Result<()> {
    let path = reality_check_surface_marker_path(runtime_root);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let temp_path = path.with_extension("tmp");
    tokio::fs::write(&temp_path, now.to_rfc3339()).await?;
    tokio::fs::rename(temp_path, path).await
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
        // This caller needs only the count, not the rows. Use the index-only
        // `COUNT(*)` entrypoint instead of materializing + aux-hydrating every
        // matching row just to read `rows.len()`.
        total += substrate
            .count_recall_index(RecallIndexQuery {
                namespace_prefix: Some(namespace_prefix.clone()),
                statuses: vec![MemoryStatus::Candidate, MemoryStatus::Quarantined],
                passive_recall_only: true,
                updated_since: None,
                match_terms: Vec::new(),
                hydrate: AuxScope::None,
                source_identity: false,
            })
            .await
            .map_err(map_substrate_error)?;
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

/// Session-less identity body for passive (hook-mode) recall. Omits the
/// per-session id so the cached SessionStart prefix is byte-identical across
/// sessions for a given `(harness, cwd)` (plan Decision 4).
fn passive_identity_body(session_binding: &SessionBinding) -> String {
    format!(
        "- harness: {}\n- cwd: {}",
        escape_xml_text(&session_binding.harness),
        escape_xml_text(&session_binding.cwd)
    )
}

/// Bytes of `cwd/MEMORY.md` that Claude auto-loads at session start. The native
/// auto-load reads the first 200 lines / 25 KB, whichever is smaller; matching
/// that bound keeps dedup aligned with what is actually double-injected.
const NATIVE_MEMORY_HEAD_MAX_LINES: usize = 200;
const NATIVE_MEMORY_HEAD_MAX_BYTES: usize = 25 * 1024;

/// Read and normalize the head of the active project's `MEMORY.md` for passive
/// dedup (plan Decision 8). Best-effort and Claude-only: an absent or unreadable
/// file returns `None` so dedup is simply skipped. The head is read once per
/// request and frozen by the caller, preserving the passive block's byte
/// determinism (reconciles with Decision 4).
async fn read_native_memory_head(cwd: &str) -> Option<String> {
    let path = std::path::Path::new(cwd).join("MEMORY.md");
    let contents = tokio::fs::read_to_string(&path).await.ok()?;
    let mut head = String::new();
    for line in contents.lines().take(NATIVE_MEMORY_HEAD_MAX_LINES) {
        if head.len() + line.len() + 1 > NATIVE_MEMORY_HEAD_MAX_BYTES {
            break;
        }
        head.push_str(line);
        head.push('\n');
    }
    Some(normalize_for_dedup(&head))
}

/// `true` when the (normalized) native MEMORY.md head already contains a memory's
/// summary, meaning Claude will auto-load it and the passive block should suppress
/// the duplicate. Empty summaries never match.
fn native_head_contains_summary(normalized_head: &str, summary: &str) -> bool {
    let needle = normalize_for_dedup(summary);
    !needle.is_empty() && normalized_head.contains(&needle)
}

/// Lowercase and collapse interior whitespace so dedup matching is robust to
/// Markdown list markers, wrapping, and incidental spacing differences while
/// staying a pure (deterministic) function of its input.
fn normalize_for_dedup(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ").to_ascii_lowercase()
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

#[allow(clippy::too_many_arguments)]
fn render_startup_frame_with_stable_budget(
    session_binding: &SessionBinding,
    explanation: &mut RecallExplanation,
    sections: &[RenderedRecallSection],
    startup_coordination: StartupCoordinationRender<'_>,
    passive: bool,
) -> String {
    let render = |explanation: &RecallExplanation| {
        let frame = if passive {
            render_startup_frame_passive(session_binding, explanation, sections, startup_coordination)
        } else {
            render_startup_frame_with_cross_device_updates(session_binding, explanation, sections, startup_coordination)
        };
        // Enforce the Claude Code char cap on passive blocks only (plan Decision 8).
        // Truncation is a deterministic function of the rendered bytes, so it never
        // destabilizes the budget-convergence loop below.
        if passive {
            cap_passive_block(frame)
        } else {
            frame
        }
    };

    for _ in 0..4 {
        let recall_block = render(explanation);
        let measured = estimated_tokens(&recall_block);
        if explanation.budget_used_tokens == measured {
            return recall_block;
        }
        explanation.budget_used_tokens = measured;
    }
    let recall_block = render(explanation);
    explanation.budget_used_tokens = estimated_tokens(&recall_block);
    render(explanation)
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

struct StrengthHydrationResult {
    facts: Vec<RecallCandidate>,
    alpha_points: u32,
    dynamics_degraded: bool,
}

/// Hydrate strength onto the ranking candidates per the dynamics config
/// (memory-dynamics-v0.1 §3), on the blocking pool.
///
/// Dynamics off (`enabled = false`) → `alpha_points = 0`, no hydration, candidates
/// untouched: ranking is structural-only and the block is byte-identical to
/// pre-dynamics except the policy version string. On a usage-query soft failure
/// the candidates keep `strength = None` and `dynamics_degraded` is set.
async fn hydrate_strength_for_ranking(
    substrate: &Substrate,
    dynamics: &DynamicsConfig,
    facts: Vec<RecallCandidate>,
    now: DateTime<Utc>,
) -> StrengthHydrationResult {
    if !dynamics.enabled || facts.is_empty() {
        return StrengthHydrationResult { facts, alpha_points: 0, dynamics_degraded: false };
    }

    let substrate = substrate.clone();
    let hydration = StrengthHydration { weights: dynamics.weights, tau_days: dynamics.tau_days };
    let (facts, ok) = tokio::task::spawn_blocking(move || {
        let mut facts = facts;
        // Catch a hydration panic so a dynamics bug degrades to structural-only ranking
        // (spec §3 soft-failure) instead of aborting the recall hot path. `facts` is owned
        // here, so a caught panic still returns the (possibly partially hydrated) candidates.
        let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            substrate
                .with_index(|index| Ok(hydrate_candidate_strength(index, &mut facts, &hydration, now)))
                .unwrap_or(false)
        }))
        .unwrap_or(false);
        (facts, ok)
    })
    .await
    .expect("strength hydration join (hydration panics are caught inside the task)");

    StrengthHydrationResult { facts, alpha_points: dynamics.alpha_points, dynamics_degraded: !ok }
}

/// Per-memory strength metadata for the recall explanation (spec §3
/// observability), over the selected candidates that carry a hydrated value.
fn strength_metadata(selected: &crate::recall::rank::RankedSelection) -> Vec<RecallStrength> {
    selected
        .selected
        .iter()
        .filter_map(|candidate| {
            candidate.candidate.strength.map(|strength| RecallStrength { id: candidate.id().to_owned(), strength })
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
    use crate::recall::types::HOOK_BLOCK_CHAR_CAP;
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
            strengths: Vec::new(),
            dynamics_degraded: false,
        };

        let recall_block = render_startup_frame_with_stable_budget(
            &session_binding,
            &mut explanation,
            &sections,
            StartupCoordinationRender::default(),
            false,
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

    #[test]
    fn passive_identity_body_omits_session_id() {
        let mut binding = binding_with_session("sess_secret_123");
        binding.harness = "claude-code".to_owned();
        binding.cwd = "/Users/treygoff/Code/agent-memory".to_owned();

        let body = passive_identity_body(&binding);

        assert!(body.contains("- harness: claude-code"));
        assert!(body.contains("- cwd: /Users/treygoff/Code/agent-memory"));
        assert!(!body.contains("sess_secret_123"), "passive identity body must not leak the session id");
        assert!(!body.contains("session"), "passive identity body must not render a session line");
    }

    #[test]
    fn native_head_dedup_matches_normalized_summaries() {
        let head =
            normalize_for_dedup("# Project\n- Eval-gated merge order: A/B in the worktree first\n- Other note\n");

        assert!(native_head_contains_summary(&head, "Eval-gated merge order: A/B in the worktree first"));
        // Whitespace/case differences must still match (normalization is robust).
        assert!(native_head_contains_summary(&head, "EVAL-GATED   merge order:  A/B in the worktree first"));
        assert!(!native_head_contains_summary(&head, "A summary that is absent from the head"));
        // An empty summary never matches, so dedup never drops blank entries.
        assert!(!native_head_contains_summary(&head, ""));
    }

    #[tokio::test]
    async fn passive_startup_leaves_store_byte_unchanged_while_active_recall_writes() {
        let fixture = PassiveFixture::new("dev_passivero").await;
        fixture.write_memory("mem_20260501_aaaaaaaaaaaaaaaa_000001", "Operational invariant worth recalling.").await;
        fixture.write_memory("mem_20260501_bbbbbbbbbbbbbbbb_000002", "Second active note for recent memory.").await;

        let before = fixture.snapshot();
        let passive = fixture.startup(true, None).await;
        assert!(!passive.recall_block.is_empty());
        let after_passive = fixture.snapshot();
        assert_eq!(before, after_passive, "passive startup must not mutate any on-disk store state");

        // Teeth: the active (non-passive) path records RecallHit events, so the
        // store DOES change — proving the snapshot diff is sensitive.
        let _active = fixture.startup(false, None).await;
        let after_active = fixture.snapshot();
        assert_ne!(before, after_active, "active recall is expected to append RecallHit events");
    }

    #[tokio::test]
    async fn passive_startup_block_is_deterministic_across_sessions_and_clock() {
        let fixture = PassiveFixture::new("dev_passivedet").await;
        fixture.write_memory("mem_20260501_cccccccccccccccc_000003", "Deterministic recall content under test.").await;

        let first = fixture.startup(true, Some("sess_one")).await;
        let second = fixture.startup(true, Some("sess_two_different")).await;

        assert_eq!(
            first.recall_block.as_bytes(),
            second.recall_block.as_bytes(),
            "passive base block must be byte-identical across sessions on the same identity tuple"
        );
        assert!(!first.recall_block.contains("sess_one"));
        assert!(!first.recall_block.contains("sess_two_different"));
        assert!(!first.recall_block.contains("session=\""), "passive frame must omit the session attribute");

        // Changing the budget (part of the identity tuple) changes the block.
        let smaller_budget = fixture.startup(true, Some("sess_one")).await;
        assert_eq!(first.recall_block, smaller_budget.recall_block, "same tuple still byte-identical");
        let rebudgeted = fixture.startup_with_budget(true, "sess_one", 1_024).await;
        assert_ne!(first.recall_block, rebudgeted.recall_block, "a different budget must change the block");
    }

    #[tokio::test]
    async fn passive_startup_block_stays_under_char_cap() {
        let fixture = PassiveFixture::new("dev_passivecap").await;
        // Many max-length summaries push toward the cap; the reduced budget plus the
        // deterministic backstop must keep the block under HOOK_BLOCK_CHAR_CAP.
        for index in 0..40 {
            let id = format!("mem_20260501_dddddddddddddddd_{index:06}");
            fixture.write_memory(&id, &format!("{} {}", "x".repeat(220), index)).await;
        }

        let passive = fixture.startup(true, Some("sess_cap")).await;

        assert!(
            passive.recall_block.chars().count() < HOOK_BLOCK_CHAR_CAP,
            "passive block was {} chars, must stay under {HOOK_BLOCK_CHAR_CAP}",
            passive.recall_block.chars().count()
        );
    }

    #[tokio::test]
    async fn passive_startup_dedup_suppresses_native_memory_entries_and_stays_deterministic() {
        let fixture = PassiveFixture::new("dev_passivededup").await;
        let dup_summary = "This summary already lives in the native MEMORY.md head.";
        let kept_summary = "This summary is unique to Memorum recall.";
        fixture.write_memory("mem_20260501_eeeeeeeeeeeeeeee_000004", dup_summary).await;
        fixture.write_memory("mem_20260501_ffffffffffffffff_000005", kept_summary).await;

        // No native MEMORY.md yet: both entries should render.
        let without_native = fixture.startup(true, Some("sess_dedup")).await;
        assert!(without_native.recall_block.contains(dup_summary));
        assert!(without_native.recall_block.contains(kept_summary));

        // Drop a native MEMORY.md whose head contains the duplicate summary.
        fixture.write_native_memory_md(&format!("# Project memory\n- {dup_summary}\n"));
        let with_native_a = fixture.startup(true, Some("sess_dedup")).await;
        let with_native_b = fixture.startup(true, Some("sess_dedup_other")).await;

        assert!(
            !with_native_a.recall_block.contains(dup_summary),
            "entry present in MEMORY.md head must be suppressed"
        );
        assert!(with_native_a.recall_block.contains(kept_summary), "unique entry must survive dedup");
        assert_eq!(
            with_native_a.recall_block.as_bytes(),
            with_native_b.recall_block.as_bytes(),
            "dedup is frozen per request, so the deduped block stays byte-deterministic"
        );
    }

    fn binding_with_session(session_id: &str) -> SessionBinding {
        SessionBinding {
            session_id: session_id.to_owned(),
            harness: "claude-code".to_owned(),
            harness_version: None,
            cwd: "/tmp".to_owned(),
            project: None,
            namespaces_in_scope: vec!["me".to_owned()],
        }
    }

    struct PassiveFixture {
        _temp: tempfile::TempDir,
        roots: memory_substrate::Roots,
        substrate: Substrate,
    }

    impl PassiveFixture {
        async fn new(device_id: &str) -> Self {
            let temp = tempfile::tempdir().expect("tempdir");
            let roots = memory_substrate::Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
            let substrate = Substrate::init(
                roots.clone(),
                memory_substrate::InitOptions { force_unsafe_durability: true, device_id: Some(device_id.to_owned()) },
            )
            .await
            .expect("substrate init");
            Self { _temp: temp, roots, substrate }
        }

        async fn write_memory(&self, id: &str, summary: &str) {
            use memory_substrate::{
                Author, AuthorKind, ClassificationOutcome, EventContext, Frontmatter, Memory, MemoryId, MemoryType,
                RepoPath, RetrievalPolicy, Scope, Sensitivity, Source, SourceKind, TrustLevel, WriteMode, WritePolicy,
                WriteRequest,
            };
            let updated_at = Utc::now();
            let memory = Memory {
                frontmatter: Frontmatter {
                    schema_version: 1,
                    id: MemoryId::new(id),
                    memory_type: MemoryType::Pattern,
                    scope: Scope::User,
                    summary: summary.to_owned(),
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
                        component: Some("passive-recall-test".to_owned()),
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
                        policy_applied: "passive-recall-test".to_owned(),
                        expected_base_hash: None,
                    },
                    merge_diagnostics: None,
                    extras: Default::default(),
                },
                body: summary.to_owned(),
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

        fn write_native_memory_md(&self, contents: &str) {
            std::fs::write(self.roots.repo.join("MEMORY.md"), contents).expect("write native MEMORY.md");
        }

        async fn startup(&self, passive: bool, session_id: Option<&str>) -> StartupResponse {
            self.startup_request(passive, session_id.unwrap_or("sess_default"), None).await
        }

        async fn startup_with_budget(&self, passive: bool, session_id: &str, budget: usize) -> StartupResponse {
            self.startup_request(passive, session_id, Some(budget)).await
        }

        async fn startup_request(&self, passive: bool, session_id: &str, budget: Option<usize>) -> StartupResponse {
            build_startup_response(
                &self.substrate,
                StartupRequest {
                    cwd: self.roots.repo.to_string_lossy().into_owned(),
                    session_id: session_id.to_owned(),
                    harness: "claude-code".to_owned(),
                    harness_version: None,
                    include_recent: true,
                    since_event_id: None,
                    budget_tokens: budget,
                    passive,
                },
            )
            .await
            .expect("startup recall")
        }

        fn snapshot(&self) -> Vec<(String, Vec<u8>)> {
            snapshot_canonical_store(&self.roots)
        }
    }

    /// A content+path digest of the canonical store state that read-only recall
    /// must not touch: the event JSONL log (where `RecallHit` events land), the
    /// canonical memory markdown files, and the runtime `state/` markers (where the
    /// reality-check surface marker lands). Git plumbing and the derived SQLite
    /// index sidecars are excluded — they are volatile and not the substrate or
    /// ranking state the invariant protects (the JSONL log is the source of truth
    /// the index mirrors).
    pub(super) fn snapshot_canonical_store(roots: &memory_substrate::Roots) -> Vec<(String, Vec<u8>)> {
        let mut entries = Vec::new();
        for root in [&roots.repo, &roots.runtime] {
            for entry in walkdir::WalkDir::new(root).sort_by_file_name() {
                let entry = entry.expect("walk store");
                if !entry.file_type().is_file() {
                    continue;
                }
                let rel = entry.path().strip_prefix(root).unwrap_or(entry.path()).to_string_lossy().into_owned();
                if !is_canonical_store_path(&rel) {
                    continue;
                }
                let bytes = std::fs::read(entry.path()).expect("read store file");
                entries.push((format!("{}:{rel}", root.display()), bytes));
            }
        }
        entries.sort();
        entries
    }

    fn is_canonical_store_path(rel: &str) -> bool {
        if rel.starts_with(".git/") || rel == ".git" {
            return false;
        }
        rel.starts_with("events/") || rel.ends_with(".md") || rel.contains("/state/") || rel.starts_with("state/")
    }
}
