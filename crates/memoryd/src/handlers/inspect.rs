//! Read-only inspection / introspection request handlers.
//!
//! Owns the daemon's "look but don't mutate" surface: entity inspection, the
//! events-log page, the namespace tree, the conflicts list, the governance
//! policy dump, recent recall hits, recent passive notifications, and the
//! test-only synthetic event injector. The `*_response` handlers are `pub(super)`
//! so `handlers::mod`'s dispatch can call them via `inspect::…`; `event_kind_label`
//! is `pub(crate)` (re-exported by `handlers::mod`) because `handlers::status`
//! reads it through its `use super::*` glob.
//!
//! The `EventKindView` mapping and its `label` strings feed the SQL kind filter
//! and the events-log rendering; they must stay byte-identical to the historical
//! mapping.

use std::collections::BTreeMap;
use std::path::Path;

use memory_governance::{CandidateContext, PolicySet, Scope as GovernanceScope};
use memory_substrate::{events::EventKind, AuxScope, MemoryId, MemoryStatus, RecallIndexQuery, Scope};

use super::{bounded, load_policy_set, policy_source_string, HandlerError, HandlerState, REVIEW_QUEUE_SUMMARY_MAX};
use crate::protocol::{
    ConflictSummary, ConflictsListResponse, EntitySummary, EventLogEntry, EventsLogPageResponse,
    GovernancePolicySnapshot, GovernancePolicySummary, InjectableEventKind, InspectEntitiesResponse, NamespaceNode,
    NamespaceTreeResponse, NotificationsRecentResponse, ResponsePayload,
};

pub(super) fn notifications_recent_response(state: &HandlerState, limit: Option<usize>) -> NotificationsRecentResponse {
    NotificationsRecentResponse { notifications: state.passive_notifications.recent_snapshots(limit) }
}

pub(super) async fn recall_hits_response(
    substrate: &memory_substrate::Substrate,
    since: Option<chrono::DateTime<chrono::Utc>>,
    limit: Option<usize>,
) -> Result<ResponsePayload, HandlerError> {
    crate::recall_hits::recent_recall_hits(substrate, since, limit)
        .map(ResponsePayload::RecallHits)
        .map_err(HandlerError::substrate)
}

pub(super) async fn inspect_entities_response(
    substrate: &memory_substrate::Substrate,
    limit: Option<usize>,
    prefix: Option<String>,
) -> Result<ResponsePayload, HandlerError> {
    // Read only the entity tables (id-ordered), instead of loading and fully
    // hydrating every recall-index row. Same iteration order as the prior
    // `for row { for entity in row.entities }`, so aggregation is identical.
    let entity_rows = substrate.entity_index_rows().await.map_err(HandlerError::substrate)?;
    let prefix = prefix.map(|value| value.to_ascii_lowercase());
    let mut by_id: BTreeMap<String, EntitySummary> = BTreeMap::new();
    for (memory_id, entity) in entity_rows {
        if prefix.as_ref().is_some_and(|prefix| !entity_matches_prefix(&entity, prefix)) {
            continue;
        }
        let entry = by_id.entry(entity.id.clone()).or_insert_with(|| EntitySummary {
            entity_id: entity.id.clone(),
            label: entity.label.clone(),
            aliases: Vec::new(),
            memory_count: 0,
            recent_memory_ids: Vec::new(),
        });
        entry.memory_count += 1;
        entry.recent_memory_ids.push(memory_id);
        for alias in entity.aliases {
            if !entry.aliases.contains(&alias) {
                entry.aliases.push(alias);
            }
        }
    }
    let mut entities = by_id.into_values().collect::<Vec<_>>();
    entities.sort_by(|left, right| {
        right.memory_count.cmp(&left.memory_count).then_with(|| left.entity_id.cmp(&right.entity_id))
    });
    entities.truncate(limit.unwrap_or(50).min(200));
    Ok(ResponsePayload::InspectEntities(InspectEntitiesResponse { entities }))
}

fn entity_matches_prefix(entity: &memory_substrate::Entity, prefix: &str) -> bool {
    entity.id.to_ascii_lowercase().starts_with(prefix)
        || entity.label.to_ascii_lowercase().starts_with(prefix)
        || entity.aliases.iter().any(|alias| alias.to_ascii_lowercase().starts_with(prefix))
}

pub(super) fn events_log_page_response(
    substrate: &memory_substrate::Substrate,
    since: Option<crate::protocol::EventId>,
    limit: usize,
    kind_filter: Option<Vec<EventKind>>,
) -> Result<ResponsePayload, HandlerError> {
    // Push the kind filter, cursor, ORDER BY, and LIMIT into the events_log
    // mirror so a page costs an index seek + <=200 rows, not a full JSONL parse.
    let limit = limit.min(200);
    let filter_labels = kind_filter.map(|kinds| kinds.iter().map(|kind| event_kind_label(kind)).collect::<Vec<_>>());
    let entries = substrate
        .events_log_page(filter_labels.as_deref(), since.as_ref().map(|cursor| cursor.as_str()), limit)
        .map_err(HandlerError::substrate)?
        .into_iter()
        .map(|event| {
            let view = event_kind_view(&event.kind);
            EventLogEntry {
                event_id: event.event_id,
                ts: event.at,
                device: event.device,
                seq: event.seq,
                memory_id: view.memory_id,
                summary: view.summary,
                kind: event.kind,
            }
        })
        .collect::<Vec<_>>();
    let next_since = entries.last().map(|entry| entry.event_id.clone());
    Ok(ResponsePayload::EventsLogPage(EventsLogPageResponse { entries, next_since }))
}

pub(super) async fn namespace_tree_response(
    substrate: &memory_substrate::Substrate,
    root: Option<String>,
    depth: Option<usize>,
) -> Result<ResponsePayload, HandlerError> {
    let root = root.unwrap_or_else(|| "all".to_string());
    let include_children = depth.unwrap_or(1) > 0;
    // Aggregate scope/namespace counts in SQL (GROUP BY) instead of loading and
    // fully hydrating every recall-index row only to count namespaces in Rust.
    let namespace_counts = substrate.namespace_counts().await.map_err(HandlerError::substrate)?;
    let mut counts = BTreeMap::<String, usize>::new();
    for (scope, canonical_namespace_id, count) in namespace_counts {
        let namespace = namespace_label(scope, canonical_namespace_id.as_deref());
        if root != "all" && !namespace.starts_with(&root) {
            continue;
        }
        *counts.entry(namespace).or_default() += count as usize;
    }
    let children = if include_children {
        counts
            .into_iter()
            .map(|(path, memory_count)| NamespaceNode {
                name: leaf_name(&path),
                path,
                memory_count,
                children: Vec::new(),
            })
            .collect()
    } else {
        Vec::new()
    };
    let memory_count = children.iter().map(|child: &NamespaceNode| child.memory_count).sum();
    Ok(ResponsePayload::NamespaceTree(NamespaceTreeResponse {
        root: NamespaceNode { name: leaf_name(&root), path: root, memory_count, children },
    }))
}

pub(super) fn governance_policy_dump_response(
    substrate: &memory_substrate::Substrate,
) -> Result<ResponsePayload, HandlerError> {
    match crate::policy_editor::snapshot(substrate.roots().repo.as_path()) {
        Ok(snapshot) => Ok(ResponsePayload::GovernancePolicyDump(snapshot)),
        Err(_) => {
            let (policies, source) = load_policy_set(substrate.roots().repo.as_path())?;
            Ok(ResponsePayload::GovernancePolicyDump(GovernancePolicySnapshot {
                source: policy_source_string(source),
                raw_yaml: first_policy_yaml(substrate.roots().repo.as_path()),
                policies: summarize_governance_policy_set(&policies)?,
                current_file: None,
                files: Vec::new(),
                writable: false,
            }))
        }
    }
}

fn summarize_governance_policy_set(policies: &PolicySet) -> Result<Vec<GovernancePolicySummary>, HandlerError> {
    let scopes = [GovernanceScope::Me, GovernanceScope::Project, GovernanceScope::Agent, GovernanceScope::Dreaming];
    scopes
        .into_iter()
        .map(|scope| {
            let policy =
                policies.policy_for_scope(scope).map_err(|error| HandlerError::invalid_request(error.to_string()))?;
            let preview = policy.dry_run(&CandidateContext::new(scope).with_confidence(0.0).with_grounding(false));
            Ok(GovernancePolicySummary {
                scope: format!("{scope:?}").to_ascii_lowercase(),
                selected_policy: preview.selected_policy,
                policy_source: format!("{:?}", preview.policy_source).to_ascii_lowercase(),
                confidence_floor: preview.confidence_floor,
                review_gates: preview.triggered_review_gates,
                requires_grounding: preview.requires_grounding,
            })
        })
        .collect()
}

pub(super) async fn conflicts_list_response(
    substrate: &memory_substrate::Substrate,
    limit: Option<usize>,
) -> Result<ResponsePayload, HandlerError> {
    // Served entirely from the SQLite recall index: `summary`, `updated_at`, and
    // `_merge_diagnostics` are all projected from indexed columns / frontmatter
    // JSON, so no per-row canonical-file read+parse is needed (avoids the prior
    // N+1 over up to 200 quarantined rows). Metadata-only/encrypted quarantined
    // rows are included to match the historical `include_metadata_only: true`.
    let rows = substrate
        .query_recall_index_including_metadata_only(RecallIndexQuery {
            statuses: vec![MemoryStatus::Quarantined],
            hydrate: AuxScope::None,
            // Reads `merge_diagnostics_json` per row to render the conflict reason.
            source_identity: true,
            exclude_merge_non_servable: true,
            ..RecallIndexQuery::default()
        })
        .await
        .map_err(HandlerError::substrate)?;
    let conflicts = rows
        .into_iter()
        .take(limit.unwrap_or(50).min(200))
        .map(|row| ConflictSummary {
            id: row.id,
            path: row.path.to_string(),
            summary: bounded(&row.summary, REVIEW_QUEUE_SUMMARY_MAX),
            // `merge_diagnostics_json` is the raw stored JSON for `_merge_diagnostics`;
            // bounding it matches the prior `Value::to_string()` rendering.
            reason: row.merge_diagnostics_json.map(|value| bounded(&value, 240)),
            updated_at: row.updated_at,
        })
        .collect();
    Ok(ResponsePayload::ConflictsList(ConflictsListResponse { conflicts }))
}

fn namespace_label(scope: Scope, canonical_namespace_id: Option<&str>) -> String {
    match scope {
        Scope::User => "me".to_string(),
        Scope::Agent => "agent".to_string(),
        Scope::Subagent => "subagent".to_string(),
        Scope::Project => format!("project:{}", canonical_namespace_id.unwrap_or("unknown")),
        Scope::Org => format!("org:{}", canonical_namespace_id.unwrap_or("unknown")),
    }
}

fn leaf_name(path: &str) -> String {
    path.rsplit([':', '/']).next().filter(|name| !name.is_empty()).unwrap_or(path).to_string()
}

fn first_policy_yaml(repo: &Path) -> Option<String> {
    let policy_dir = repo.join("policies");
    let mut paths = std::fs::read_dir(policy_dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "yaml"))
        .collect::<Vec<_>>();
    paths.sort();
    paths.into_iter().next().and_then(|path| std::fs::read_to_string(path).ok())
}

/// All three presentation facets of an `EventKind`, derived from a single match
/// so a new or renamed variant can't pick up a `label` without a `summary`, or a
/// `summary` that disagrees with the extracted `memory_id`. `label` feeds the
/// SQL kind filter and must stay byte-identical to the historical mapping.
struct EventKindView {
    memory_id: Option<MemoryId>,
    summary: String,
    label: &'static str,
}

fn event_kind_view(kind: &EventKind) -> EventKindView {
    match kind {
        EventKind::WriteCommitted { id, .. } => EventKindView {
            memory_id: Some(id.clone()),
            summary: format!("memory write committed: {id}"),
            label: "write_committed",
        },
        EventKind::EncryptedWriteCommitted { id, .. } => EventKindView {
            memory_id: Some(id.clone()),
            summary: format!("encrypted memory write committed: {id}"),
            label: "encrypted_write_committed",
        },
        EventKind::MetadataAmended { id, changed_fields, .. } => EventKindView {
            memory_id: Some(id.clone()),
            summary: format!("memory metadata amended: {id} ({})", changed_fields.join(", ")),
            label: "metadata_amended",
        },
        EventKind::TombstoneCommitted { id } => EventKindView {
            memory_id: Some(id.clone()),
            summary: format!("memory tombstoned: {id}"),
            label: "tombstone_committed",
        },
        EventKind::DuplicateIdRepaired { old_id, new_id } => EventKindView {
            memory_id: Some(new_id.clone()),
            summary: format!("duplicate id repaired: {old_id} -> {new_id}"),
            label: "duplicate_id_repaired",
        },
        EventKind::EmbeddingModelChanged { chunks_requeued } => EventKindView {
            memory_id: None,
            summary: format!("embedding model changed; {chunks_requeued} chunks requeued"),
            label: "embedding_model_changed",
        },
        EventKind::StartupReconciliationCompleted { reindexed, repaired_events } => EventKindView {
            memory_id: None,
            summary: format!(
                "startup reconciliation completed; reindexed={reindexed}, repaired_events={repaired_events}"
            ),
            label: "startup_reconciliation_completed",
        },
        EventKind::OperatorRepairRequired { reason } => {
            EventKindView { memory_id: None, summary: reason.clone(), label: "operator_repair_required" }
        }
        EventKind::GitPushFailed { reason } => {
            EventKindView { memory_id: None, summary: reason.clone(), label: "git_push_failed" }
        }
        EventKind::WriteRefused { reason, .. } => {
            EventKindView { memory_id: None, summary: format!("write refused: {reason}"), label: "write_refused" }
        }
        EventKind::EncryptedContentRevealed { reason, .. } => {
            EventKindView { memory_id: None, summary: reason.clone(), label: "encrypted_content_revealed" }
        }
        EventKind::SubstrateFragmentWritten { id, path, .. } => EventKindView {
            memory_id: None,
            summary: format!("substrate fragment written: {id} at {path}"),
            label: "substrate_fragment_written",
        },
        EventKind::RecallHit { id, .. } => EventKindView {
            memory_id: Some(id.clone()),
            summary: format!("memory recalled: {id}"),
            label: "recall_hit",
        },
        EventKind::RealityCheckConfirmed { id, .. } => EventKindView {
            memory_id: Some(id.clone()),
            summary: format!("reality check confirmed: {id}"),
            label: "reality_check_confirmed",
        },
        EventKind::RealityCheckForgotten { id, .. } => EventKindView {
            memory_id: Some(id.clone()),
            summary: format!("reality check forgot: {id}"),
            label: "reality_check_forgotten",
        },
        EventKind::RealityCheckNotRelevant { id, .. } => EventKindView {
            memory_id: Some(id.clone()),
            summary: format!("reality check not relevant: {id}"),
            label: "reality_check_not_relevant",
        },
        EventKind::ClaimLockContention { memory_id, .. } => EventKindView {
            memory_id: Some(memory_id.clone()),
            summary: format!("claim-lock contention: {memory_id}"),
            label: "claim_lock_contention",
        },
        EventKind::DeviceKeysRotated { active_recipient, .. } => EventKindView {
            memory_id: None,
            summary: format!("device keys rotated: active recipient {active_recipient}"),
            label: "device_keys_rotated",
        },
        EventKind::PolicyChanged { file_name } => {
            EventKindView { memory_id: None, summary: format!("policy changed: {file_name}"), label: "policy_changed" }
        }
        EventKind::MergeApplied { proposal_id, replacement_id, .. } => EventKindView {
            memory_id: Some(replacement_id.clone()),
            summary: format!("merge applied: {proposal_id}"),
            label: "merge_applied",
        },
        EventKind::MergeRolledBack { proposal_id, replacement_id, .. } => EventKindView {
            memory_id: Some(replacement_id.clone()),
            summary: format!("merge rolled back: {proposal_id}"),
            label: "merge_rolled_back",
        },
    }
}

pub(crate) fn event_kind_label(kind: &EventKind) -> &'static str {
    event_kind_view(kind).label
}

/// Inject a synthetic event-log entry with a controlled timestamp.
///
/// This handler is only functional when `memoryd` is compiled with the
/// `test-utils` feature flag; without it, the protocol variant still exists
/// (so the crate compiles) but the handler returns `method_not_allowed`. This
/// keeps the test-only surface invisible in production daemon builds while
/// letting Stream H eval tests exercise events-log-derived metrics
/// deterministically. (H-R1)
#[cfg_attr(not(feature = "test-utils"), allow(dead_code))]
pub(super) struct TestInjectEventRequest {
    pub(super) kind: InjectableEventKind,
    pub(super) memory_id: MemoryId,
    pub(super) ts: chrono::DateTime<chrono::Utc>,
    pub(super) harness: Option<String>,
    pub(super) session_id: Option<String>,
}

pub(super) async fn test_inject_event_response(
    substrate: &memory_substrate::Substrate,
    request: TestInjectEventRequest,
) -> Result<ResponsePayload, HandlerError> {
    #[cfg(not(feature = "test-utils"))]
    {
        let _ = (substrate, request);
        Err(HandlerError::invalid_request(
            "TestInjectEvent requires the memoryd `test-utils` feature; \
             this daemon was compiled without it",
        ))
    }

    #[cfg(feature = "test-utils")]
    {
        let event_kind = match request.kind {
            InjectableEventKind::RecallHit => {
                EventKind::RecallHit { id: request.memory_id.clone(), recalled_at: request.ts }
            }
            InjectableEventKind::WriteCommitted => {
                // Synthetic WriteCommitted: we use a placeholder path derived from the
                // memory_id since we don't re-query the substrate for the actual file path.
                // The cross_source_corroboration metric only counts distinct devices/harnesses
                // that produced WriteCommitted events for a given memory_id; the path field
                // is not used for scoring. Source attribution (harness) is available in the
                // harness parameter but WriteCommitted's schema does not carry it (§12.1).
                let synthetic_path =
                    memory_substrate::RepoPath::new(format!("synthetic-test-inject/{}.md", request.memory_id.as_str()));
                EventKind::WriteCommitted {
                    id: request.memory_id.clone(),
                    path: synthetic_path,
                    classification: memory_substrate::ClassificationOutcome::Trusted,
                }
            }
        };
        let _ = (request.harness, request.session_id); // reserved for future provenance embedding
        substrate.record_event_best_effort(event_kind).map_err(HandlerError::substrate)?;
        let event_id = format!("injected-{}-{}", kind_label(request.kind), request.memory_id.as_str());
        Ok(ResponsePayload::TestInjectEvent(crate::protocol::TestInjectEventResponse {
            event_id,
            injected_kind: request.kind,
            memory_id: request.memory_id,
        }))
    }
}

#[cfg(feature = "test-utils")]
fn kind_label(kind: InjectableEventKind) -> &'static str {
    match kind {
        InjectableEventKind::RecallHit => "recall-hit",
        InjectableEventKind::WriteCommitted => "write-committed",
    }
}
