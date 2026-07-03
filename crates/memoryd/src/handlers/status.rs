//! Status request handler: assembles the daemon StatusResponse from live index,
//! review-queue, conflict, and compact-dream readings.

use super::*;

pub(crate) async fn status_response(substrate: &Substrate, state: &HandlerState) -> StatusResponse {
    let mut dashboard_warnings = Vec::new();
    let status_counts = substrate.count_memories_by_status().await;
    let (index_stats, review_queue_counts, conflicts_count) = match status_counts {
        Ok(counts) => {
            let index_stats = match live_index_stats(substrate, &counts) {
                Ok(stats) => Some(stats),
                Err(error) => {
                    dashboard_warnings.push(format!("index_stats_unavailable: {}", bounded(&error.message, 160)));
                    None
                }
            };
            // Blocking conflicts are memories quarantined under the authoritative OR
            // predicate (status == Quarantined OR trust_level == Quarantined). Use the
            // same live source as the post-resolve rescan so the count and the
            // notification queue never disagree: a status-only count under-reports
            // trust-level-only quarantines (and the count stays live — in-daemon
            // resolves are reflected — because the helper re-verifies on-disk state).
            let conflicts_count = match super::quarantine::blocking_conflict_paths(substrate).await {
                Ok(paths) => u32::try_from(paths.len()).ok(),
                Err(error) => {
                    dashboard_warnings.push(format!("conflicts_count_unavailable: {}", bounded(&error.message, 160)));
                    None
                }
            };
            (index_stats, Some(live_review_queue_counts(&counts)), conflicts_count)
        }
        Err(error) => {
            let error = HandlerError::substrate(error);
            dashboard_warnings.push(format!("status_counts_unavailable: {}", bounded(&error.message, 160)));
            (None, None, None)
        }
    };
    let compact_dream_status = match live_compact_dream_status(substrate, chrono::Utc::now()) {
        Ok(status) => Some(status),
        Err(error) => {
            dashboard_warnings.push(format!("compact_dream_status_unavailable: {}", bounded(&error, 160)));
            None
        }
    };

    StatusResponse {
        state: if dashboard_warnings.is_empty() { "ready".to_string() } else { "degraded".to_string() },
        guidance: "memoryd handlers are backed by the local Memorum substrate.".to_string(),
        daemon: Some(DaemonProcessStatus {
            version: env!("CARGO_PKG_VERSION").to_string(),
            pid: std::process::id(),
            uptime_seconds: None,
        }),
        dashboard_warnings,
        recall: state.recall.snapshot(),
        embedding: embedding_status(state),
        dreams: Default::default(),
        passive_notifications: state
            .passive_notifications
            .entries()
            .into_iter()
            .map(|entry| PassiveNotificationStatus { message: entry.message, created_at: entry.created_at })
            .collect(),
        index_stats,
        review_queue_counts,
        conflicts_count,
        peer_sessions: peer_status_response(state).active_sessions,
        peer_update_count: Some(state.peer_deliveries.snapshot().len() as u64),
        compact_dream_status,
    }
}

fn embedding_status(state: &HandlerState) -> EmbeddingStatus {
    let snapshot = state.embedding_provider_slot().snapshot();
    EmbeddingStatus {
        state: snapshot.state,
        load_count: snapshot.load_count,
        unload_count: snapshot.unload_count,
        idle_unload_secs: snapshot.idle_unload_secs,
        idle_unload_source: snapshot.idle_unload_source.to_string(),
        in_flight: snapshot.in_flight,
        last_error: snapshot.last_error,
    }
}

fn live_index_stats(substrate: &Substrate, counts: &[(MemoryStatus, u64)]) -> Result<IndexStats, HandlerError> {
    let active = status_count(counts, MemoryStatus::Active);
    let pinned = status_count(counts, MemoryStatus::Pinned);
    // Seek the latest reindex event in the SQLite mirror (kind-indexed MAX(ts))
    // instead of parsing the entire canonical JSONL log and scanning in Rust.
    let last_reindex = substrate
        .latest_event_ts_for_kind(event_kind_label(&EventKind::StartupReconciliationCompleted {
            reindexed: 0,
            repaired_events: 0,
        }))
        .map_err(HandlerError::substrate)?;
    Ok(IndexStats { active_memories: active + pinned, last_reindex })
}

fn live_review_queue_counts(counts: &[(MemoryStatus, u64)]) -> ReviewQueueCounts {
    let candidate = status_count(counts, MemoryStatus::Candidate);
    let quarantined = status_count(counts, MemoryStatus::Quarantined);
    ReviewQueueCounts { candidate, quarantined, dream_low_confidence: 0 }
}

fn status_count(counts: &[(MemoryStatus, u64)], status: MemoryStatus) -> u64 {
    counts.iter().find(|(candidate, _)| *candidate == status).map(|(_, count)| *count).unwrap_or(0)
}

fn live_compact_dream_status(
    substrate: &Substrate,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<CompactDreamStatus, String> {
    let roots = substrate.roots();
    let enabled = crate::dream::status::dreaming_enabled(&roots.repo, &roots.runtime)?;
    let last_runs = crate::dream::status::collect_last_runs(&roots.repo)?;
    let active_leases = crate::dream::status::collect_active_leases(&roots.repo, now)?;
    let latest_run = last_runs.iter().filter(|run| run.last_run_at.is_some()).max_by_key(|run| run.last_run_at);
    Ok(CompactDreamStatus {
        enabled,
        last_run_at: latest_run.and_then(|run| run.last_run_at),
        last_run_outcome: latest_run.and_then(|run| run.last_run_outcome),
        next_scheduled_at: None,
        active_leases: active_leases.into_iter().map(|lease| lease.scope).collect(),
    })
}
