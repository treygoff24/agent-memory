use std::path::Path;

use chrono::{DateTime, Utc};
use memory_substrate::{MemoryId, Substrate};
use rusqlite::{params, Connection};

use crate::protocol::{RecallHitSummary, RecallHitsResponse};

const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 500;

#[derive(Debug, thiserror::Error)]
pub enum RecallHitsError {
    #[error("open events-log mirror: {0}")]
    OpenMirror(#[from] rusqlite::Error),
    #[error("query event mirror: {0}")]
    QueryMirror(rusqlite::Error),
    #[error("invalid memory id in recall-hit row: {0}")]
    InvalidMemoryId(String),
}

pub fn recent_recall_hits(
    substrate: &Substrate,
    since: Option<DateTime<Utc>>,
    limit: Option<usize>,
) -> Result<RecallHitsResponse, RecallHitsError> {
    let connection = Connection::open(index_path(substrate))?;
    query_recent_recall_hits(&connection, since, limit)
}

fn index_path(substrate: &Substrate) -> std::path::PathBuf {
    substrate.roots().runtime.join("index.sqlite")
}

fn query_recent_recall_hits(
    connection: &Connection,
    since: Option<DateTime<Utc>>,
    limit: Option<usize>,
) -> Result<RecallHitsResponse, RecallHitsError> {
    let limit = limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let since_text = since.map(|value| value.to_rfc3339());
    let mut statement = connection
        .prepare_cached(
            "SELECT e.event_id, e.device, e.seq, e.memory_id, e.ts, m.summary
             FROM events_log e
             LEFT JOIN memories m ON m.id = e.memory_id
             WHERE e.kind = 'recall_hit'
               AND (?1 IS NULL OR e.ts > ?1)
             ORDER BY e.ts DESC, e.event_id DESC
             LIMIT ?2",
        )
        .map_err(RecallHitsError::QueryMirror)?;
    let rows = statement
        .query_map(params![since_text.as_deref(), limit as i64], |row| {
            let event_id: String = row.get(0)?;
            let device: String = row.get(1)?;
            let seq: i64 = row.get(2)?;
            let memory_id: String = row.get(3)?;
            let recalled_at: String = row.get(4)?;
            let summary: Option<String> = row.get(5)?;
            Ok((event_id, device, seq, memory_id, recalled_at, summary))
        })
        .map_err(RecallHitsError::QueryMirror)?;

    let mut hits = Vec::new();
    for row in rows {
        let (event_id, device, seq, memory_id, recalled_at, summary) = row.map_err(RecallHitsError::QueryMirror)?;
        let memory_id =
            MemoryId::try_new(memory_id.clone()).map_err(|_| RecallHitsError::InvalidMemoryId(memory_id))?;
        hits.push(RecallHitSummary {
            event_id,
            device,
            seq: seq.max(0) as u64,
            memory_id,
            recalled_at: parse_time(&recalled_at),
            summary,
        });
    }

    Ok(RecallHitsResponse { since, limit, hits })
}

fn parse_time(value: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value).map(|value| value.with_timezone(&Utc)).unwrap_or(DateTime::<Utc>::UNIX_EPOCH)
}

#[allow(dead_code)]
fn _assert_index_path_is_path(_: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limit_defaults_and_clamps() {
        let connection = Connection::open_in_memory().expect("sqlite");
        connection
            .execute_batch(
                "CREATE TABLE events_log(
                   event_id TEXT PRIMARY KEY,
                   device TEXT NOT NULL,
                   seq INTEGER NOT NULL,
                   kind TEXT NOT NULL,
                   memory_id TEXT,
                   ts TEXT NOT NULL,
                   payload_json TEXT NOT NULL
                 );
                 CREATE TABLE memories(id TEXT PRIMARY KEY, summary TEXT);",
            )
            .expect("schema");

        let response = query_recent_recall_hits(&connection, None, Some(10_000)).expect("query");

        assert_eq!(response.limit, 500);
        assert!(response.hits.is_empty());
    }
}
