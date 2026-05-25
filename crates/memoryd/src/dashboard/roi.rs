use std::collections::{BTreeMap, BTreeSet};

use chrono::{Duration, Utc};
use memory_substrate::{events::EventKind, MemoryStatus, RecallIndexQuery, RecallIndexRow, Substrate};

use crate::protocol::{DashboardRoiResponse, DreamingRoiSummary, RealityCheckAdherenceSummary};

pub async fn dashboard_roi(substrate: &Substrate, window_days: u16) -> Result<DashboardRoiResponse, String> {
    let since = Utc::now() - Duration::days(i64::from(window_days));
    let rows = substrate
        .query_recall_index_including_metadata_only(RecallIndexQuery {
            updated_since: Some(since),
            ..RecallIndexQuery::default()
        })
        .await
        .map_err(|error| error.to_string())?;
    let events = substrate.events().map_err(|error| error.to_string())?;
    let events = events.into_iter().filter(|event| event.at >= since).collect::<Vec<_>>();

    let promoted_memories = count_rows_with_status(&rows, &[MemoryStatus::Active, MemoryStatus::Pinned]);
    let review_memories = count_rows_with_status(&rows, &[MemoryStatus::Candidate, MemoryStatus::Quarantined]);
    let total_memory_outcomes = promoted_memories + review_memories;
    let promotion_rate = ratio(promoted_memories, total_memory_outcomes);

    let current_status_by_id =
        rows.iter().map(|row| (row.id.as_str().to_owned(), row.status)).collect::<BTreeMap<_, _>>();
    let promoted_write_events = events
        .iter()
        .filter_map(|event| write_event_memory_id(&event.kind))
        .filter(|id| {
            current_status_by_id
                .get(id.as_str())
                .is_some_and(|status| matches!(status, MemoryStatus::Active | MemoryStatus::Pinned))
        })
        .count();
    let refusal_breakdown = refusal_breakdown(&events);
    let refused_writes = refusal_breakdown.values().copied().map(|count| count as usize).sum::<usize>();
    let promotion_precision = ratio(promoted_write_events, promoted_write_events + refused_writes);

    let dreaming = dreaming_roi(&rows);
    let reality_check_adherence = reality_check_adherence(&events);

    Ok(DashboardRoiResponse {
        window_days,
        promotion_rate,
        promotion_precision,
        refusal_breakdown,
        dreaming,
        reality_check_adherence,
    })
}

fn count_rows_with_status(rows: &[RecallIndexRow], statuses: &[MemoryStatus]) -> usize {
    rows.iter().filter(|row| statuses.contains(&row.status)).count()
}

fn write_event_memory_id(kind: &EventKind) -> Option<&memory_substrate::MemoryId> {
    match kind {
        EventKind::WriteCommitted { id, .. } | EventKind::EncryptedWriteCommitted { id, .. } => Some(id),
        _ => None,
    }
}

fn refusal_breakdown(events: &[memory_substrate::events::Event]) -> BTreeMap<String, u32> {
    let mut breakdown = BTreeMap::new();
    for event in events {
        if let EventKind::WriteRefused { reason, .. } = &event.kind {
            let count = breakdown.entry(reason.clone()).or_insert(0_u32);
            *count = count.saturating_add(1);
        }
    }
    breakdown
}

fn dreaming_roi(rows: &[RecallIndexRow]) -> DreamingRoiSummary {
    let dream_rows = rows.iter().filter(|row| is_dreaming_row(row)).collect::<Vec<_>>();
    let promoted_silent = dream_rows
        .iter()
        .filter(|row| matches!(row.status, MemoryStatus::Active | MemoryStatus::Pinned) && !row.human_review_required)
        .count();
    let entered_review_queue = dream_rows
        .iter()
        .filter(|row| matches!(row.status, MemoryStatus::Candidate | MemoryStatus::Quarantined))
        .count();
    DreamingRoiSummary {
        candidates_generated: usize_to_u32_saturating(dream_rows.len()),
        promoted_silent: usize_to_u32_saturating(promoted_silent),
        entered_review_queue: usize_to_u32_saturating(entered_review_queue),
        dropped: 0,
        review_queue_approval_rate: ratio(promoted_silent, promoted_silent + entered_review_queue),
    }
}

fn is_dreaming_row(row: &RecallIndexRow) -> bool {
    row.tags.iter().any(|tag| tag == "dreaming") || row.path.as_str().starts_with("dreams/")
}

fn reality_check_adherence(events: &[memory_substrate::events::Event]) -> RealityCheckAdherenceSummary {
    let mut completed_sessions = BTreeSet::new();
    for event in events {
        match &event.kind {
            EventKind::RealityCheckConfirmed { session_id, .. }
            | EventKind::RealityCheckForgotten { session_id, .. }
            | EventKind::RealityCheckNotRelevant { session_id, .. } => {
                completed_sessions.insert(session_id.clone());
            }
            _ => {}
        }
    }
    RealityCheckAdherenceSummary {
        weeks_completed: usize_to_u32_saturating(completed_sessions.len()),
        weeks_skipped: 0,
    }
}

fn ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

fn usize_to_u32_saturating(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}
