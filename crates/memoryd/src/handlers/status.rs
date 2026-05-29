//! Status request handler: assembles the daemon StatusResponse from live index,
//! review-queue, conflict, and compact-dream readings.

use super::*;

pub(crate) async fn status_response(substrate: &Substrate, state: &HandlerState) -> StatusResponse {
    let mut dashboard_warnings = Vec::new();
    let index_stats = match live_index_stats(substrate).await {
        Ok(stats) => Some(stats),
        Err(error) => {
            dashboard_warnings.push(format!("index_stats_unavailable: {}", bounded(&error.message, 160)));
            None
        }
    };
    let review_queue_counts = match live_review_queue_counts(substrate).await {
        Ok(counts) => Some(counts),
        Err(error) => {
            dashboard_warnings.push(format!("review_queue_counts_unavailable: {}", bounded(&error.message, 160)));
            None
        }
    };
    let conflicts_count = match live_conflicts_count(substrate) {
        Ok(count) => Some(count),
        Err(error) => {
            dashboard_warnings.push(format!("conflicts_count_unavailable: {}", bounded(&error.message, 160)));
            None
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
        guidance: "memoryd handlers are backed by the Stream A substrate.".to_string(),
        daemon: Some(DaemonProcessStatus {
            version: env!("CARGO_PKG_VERSION").to_string(),
            pid: std::process::id(),
            uptime_seconds: None,
        }),
        dashboard_warnings,
        recall: state.recall.snapshot(),
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

async fn live_index_stats(substrate: &Substrate) -> Result<IndexStats, HandlerError> {
    let active = count_memories_by_status(substrate, MemoryStatus::Active).await?;
    let pinned = count_memories_by_status(substrate, MemoryStatus::Pinned).await?;
    let last_reindex = substrate
        .events()
        .map_err(HandlerError::substrate)?
        .into_iter()
        .filter(|event| matches!(event.kind, EventKind::StartupReconciliationCompleted { .. }))
        .max_by_key(|event| event.at)
        .map(|event| event.at);
    Ok(IndexStats { active_memories: active + pinned, last_reindex })
}

async fn live_review_queue_counts(substrate: &Substrate) -> Result<ReviewQueueCounts, HandlerError> {
    let candidate = count_memories_by_status(substrate, MemoryStatus::Candidate).await?;
    let quarantined = count_memories_by_status(substrate, MemoryStatus::Quarantined).await?;
    Ok(ReviewQueueCounts { candidate, quarantined, dream_low_confidence: 0 })
}

async fn count_memories_by_status(substrate: &Substrate, status: MemoryStatus) -> Result<u64, HandlerError> {
    let rows = substrate
        .query_memory(MemoryQuery {
            id: None,
            tag: None,
            status: Some(status),
            include_metadata_only: true,
            namespace_prefix: None,
            passive_recall_only: false,
            updated_since: None,
        })
        .await
        .map_err(HandlerError::substrate)?;
    Ok(rows.len() as u64)
}

fn live_conflicts_count(substrate: &Substrate) -> Result<u32, HandlerError> {
    let count = substrate.startup_reconcile_report().blocking_conflicts.len();
    count.try_into().map_err(|_| HandlerError::substrate("conflict count exceeds u32"))
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
