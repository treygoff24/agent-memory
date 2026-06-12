use chrono::{DateTime, Utc};
use memory_substrate::{MemoryId, Substrate};

use crate::protocol::{RecallHitSummary, RecallHitsResponse};

const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 500;

#[derive(Debug, thiserror::Error)]
pub enum RecallHitsError {
    #[error("query event mirror: {0}")]
    QueryMirror(String),
    #[error("invalid memory id in recall-hit row: {0}")]
    InvalidMemoryId(String),
}

pub fn recent_recall_hits(
    substrate: &Substrate,
    since: Option<DateTime<Utc>>,
    limit: Option<usize>,
) -> Result<RecallHitsResponse, RecallHitsError> {
    // Served from the substrate's long-lived WAL index connection instead of a
    // fresh `Connection::open` per request; the substrate builds the `since`
    // predicate dynamically so the query stays on the kind/ts index.
    let limit = clamp_limit(limit);
    let rows =
        substrate.recent_recall_hits(since, limit).map_err(|err| RecallHitsError::QueryMirror(err.to_string()))?;

    let mut hits = Vec::new();
    for (event_id, device, seq, memory_id, recalled_at, summary) in rows {
        let memory_id =
            MemoryId::try_new(memory_id.clone()).map_err(|_| RecallHitsError::InvalidMemoryId(memory_id))?;
        hits.push(RecallHitSummary {
            event_id,
            device,
            seq: seq.max(0) as u64,
            memory_id,
            recalled_at: crate::util::parse_rfc3339_utc(&recalled_at).unwrap_or(DateTime::<Utc>::UNIX_EPOCH),
            summary,
        });
    }

    Ok(RecallHitsResponse { since, limit, hits })
}

fn clamp_limit(limit: Option<usize>) -> usize {
    limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limit_defaults_and_clamps() {
        assert_eq!(clamp_limit(None), DEFAULT_LIMIT);
        assert_eq!(clamp_limit(Some(0)), 1);
        assert_eq!(clamp_limit(Some(10_000)), MAX_LIMIT);
        assert_eq!(clamp_limit(Some(120)), 120);
    }
}
