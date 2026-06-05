//! Read-side queries against the derived `events_log` SQLite mirror.
//!
//! The canonical source of truth for events is the per-device JSONL log; this
//! module never replaces that. It exists so polled dashboard/status/recall read
//! paths can seek a bounded slice of events (one kind partition, one time
//! window, one event id) without a full `read_events` parse of the whole log.
//!
//! Each row reconstructs a [`MirrorEvent`] by deserializing the stored
//! `payload_json` back into an [`EventKind`], so callers get the same data they
//! would from the canonical log for the rows the query selected.

use chrono::{DateTime, Utc};
use rusqlite::{params_from_iter, types::Value, Connection, OptionalExtension as _};

use crate::events::EventKind;
use crate::model::EventId;

/// One event projected out of the `events_log` mirror.
///
/// Mirrors the canonical [`crate::events::Event`] fields that dashboard/recall
/// read paths consume; `operation_id`/`crc32c`/`schema` are intentionally
/// omitted because no read consumer of the mirror uses them. `device` is the
/// raw stored string (consumers only render it), avoiding a `DeviceId`
/// re-validation that the canonical write path already performed.
#[derive(Clone, Debug)]
pub struct MirrorEvent {
    /// Canonical event id (the mirror primary key).
    pub event_id: EventId,
    /// Authoring device, as stored.
    pub device: String,
    /// Per-device sequence number.
    pub seq: u64,
    /// Event timestamp.
    pub at: DateTime<Utc>,
    /// Reconstructed event payload.
    pub kind: EventKind,
}

/// Parameters for one bounded page of mirror events from the `events_log` mirror.
#[derive(Clone, Copy, Debug, Default)]
pub struct EventsLogPage<'a> {
    /// Filter on the stored `kind` column: `None` = all kinds, `Some(&[])` = none.
    pub kind_labels: Option<&'a [&'a str]>,
    /// Restrict to one authoring device (`None` = all devices).
    pub device: Option<&'a str>,
    /// Chronological cursor: return rows strictly OLDER than this event id.
    pub since_event_id: Option<&'a str>,
    /// Maximum rows to return.
    pub limit: usize,
}

/// Query a bounded, kind-filtered page of mirror events ordered newest-first.
///
/// `page.kind_labels` filters on the stored `kind` column (the same labels written
/// by the mirror writer). `page.device`, when `Some`, restricts the page to one
/// authoring device (the local device for the dashboard/ROI views).
/// `page.since_event_id` is the dashboard cursor: the page returns rows strictly
/// OLDER than that event in canonical `(ts, seq, event_id)` order, so successive
/// pages walk history without gaps or repeats. (A bare `event_id` comparison cannot
/// do this — event ids are random UUIDs, so lexical order is unrelated to time.)
/// Ordering is `ts DESC, seq DESC, event_id DESC`, then truncated to `page.limit`.
pub fn query_events_log_page(connection: &Connection, page: &EventsLogPage) -> rusqlite::Result<Vec<MirrorEvent>> {
    let mut sql = String::from("SELECT event_id, device, seq, kind, ts, payload_json FROM events_log");
    let mut filters: Vec<String> = Vec::new();
    let mut bindings: Vec<Value> = Vec::new();

    if let Some(labels) = page.kind_labels {
        if labels.is_empty() {
            // An explicit empty kind filter matches nothing, mirroring the prior
            // `HashSet::contains` semantics on an empty set.
            return Ok(Vec::new());
        }
        let placeholders = labels.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        filters.push(format!("kind IN ({placeholders})"));
        for label in labels {
            bindings.push(Value::Text((*label).to_string()));
        }
    }
    if let Some(device) = page.device {
        filters.push("device = ?".to_string());
        bindings.push(Value::Text(device.to_string()));
    }
    if let Some(cursor) = page.since_event_id {
        // Resolve the cursor to its canonical (ts, seq) key and page strictly older
        // via a row-value keyset, so pagination follows time, not random UUID order.
        // A cursor that is not in the mirror (evicted / never mirrored) degrades to
        // the newest page rather than erroring.
        match key_for_event_id(connection, cursor)? {
            Some((cursor_ts, cursor_seq)) => {
                filters.push("(ts, seq, event_id) < (?, ?, ?)".to_string());
                bindings.push(Value::Text(cursor_ts));
                bindings.push(Value::Integer(cursor_seq));
                bindings.push(Value::Text(cursor.to_string()));
            }
            None => {
                tracing::warn!(cursor, "events_log page cursor not found in mirror; returning newest page");
            }
        }
    }
    if !filters.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&filters.join(" AND "));
    }
    // `event_id` tiebreaker keeps ordering deterministic when the mirror holds
    // multiple devices with identical (ts, seq).
    sql.push_str(" ORDER BY ts DESC, seq DESC, event_id DESC LIMIT ?");
    bindings.push(Value::Integer(page.limit as i64));

    let mut stmt = connection.prepare_cached(&sql)?;
    let mut rows = stmt.query(params_from_iter(bindings.iter()))?;
    collect_mirror_events(&mut rows)
}

/// Query mirror events within a time window, optionally restricted to a kind
/// set and/or a single authoring `device`, ordered newest-first. Used by ROI
/// windowed aggregates (which scope to the local device).
pub fn query_events_log_window(
    connection: &Connection,
    kind_labels: Option<&[&str]>,
    device: Option<&str>,
    since: DateTime<Utc>,
) -> rusqlite::Result<Vec<MirrorEvent>> {
    let mut sql = String::from("SELECT event_id, device, seq, kind, ts, payload_json FROM events_log WHERE ts >= ?");
    let mut bindings: Vec<Value> = vec![Value::Text(since.to_rfc3339())];

    if let Some(device) = device {
        sql.push_str(" AND device = ?");
        bindings.push(Value::Text(device.to_string()));
    }
    if let Some(labels) = kind_labels {
        if labels.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = labels.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        sql.push_str(&format!(" AND kind IN ({placeholders})"));
        for label in labels {
            bindings.push(Value::Text((*label).to_string()));
        }
    }
    sql.push_str(" ORDER BY ts DESC, seq DESC, event_id DESC");

    let mut stmt = connection.prepare_cached(&sql)?;
    let mut rows = stmt.query(params_from_iter(bindings.iter()))?;
    collect_mirror_events(&mut rows)
}

/// Most recent timestamp for events of the given kind, or `None` if absent.
///
/// Seeks `idx_events_log_kind_ts` (`MAX(ts)` for one `kind`) instead of scanning.
pub fn latest_ts_for_kind(connection: &Connection, kind_label: &str) -> rusqlite::Result<Option<DateTime<Utc>>> {
    let value: Option<String> =
        connection.query_row("SELECT MAX(ts) FROM events_log WHERE kind = ?1", [kind_label], |row| {
            row.get::<_, Option<String>>(0)
        })?;
    Ok(value.and_then(|text| parse_ts(&text)))
}

/// Timestamp of a single event looked up by its canonical event id.
///
/// Seeks the `event_id` primary key. Returns `None` on miss, matching the prior
/// linear `.find()` fallback semantics.
pub fn ts_for_event_id(connection: &Connection, event_id: &str) -> rusqlite::Result<Option<DateTime<Utc>>> {
    let value: Option<String> = connection
        .query_row("SELECT ts FROM events_log WHERE event_id = ?1", [event_id], |row| row.get::<_, String>(0))
        .optional()?;
    Ok(value.and_then(|text| parse_ts(&text)))
}

/// Resolve an event id to its canonical `(ts, seq)` paging key, or `None` if the
/// mirror has no such row. Used to turn the dashboard's `event_id` cursor into a
/// chronological keyset bound.
fn key_for_event_id(connection: &Connection, event_id: &str) -> rusqlite::Result<Option<(String, i64)>> {
    connection
        .query_row("SELECT ts, seq FROM events_log WHERE event_id = ?1", [event_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .optional()
}

/// Collect mirror rows into `MirrorEvent`s, skipping (with a warning) any single
/// row that fails to parse rather than aborting the whole query.
///
/// The `events_log` mirror is a derived, rebuildable read optimization (the
/// canonical per-device JSONL log is the source of truth). A lone corrupt or
/// forward-schema-skew row — an `EventKind` variant this build doesn't know, a
/// malformed `ts`, a negative `seq` — must not turn an entire dashboard/recall
/// page into an error; `doctor --reindex` rebuilds such rows from the canonical
/// log. A genuine SQLite cursor error (from `rows.next()`) still propagates.
fn collect_mirror_events(rows: &mut rusqlite::Rows<'_>) -> rusqlite::Result<Vec<MirrorEvent>> {
    let mut events = Vec::new();
    while let Some(row) = rows.next()? {
        match mirror_event_from_row(row) {
            Ok(event) => events.push(event),
            Err(err) => {
                let event_id: Option<String> = row.get(0).ok();
                tracing::warn!(event_id = ?event_id, error = %err, "skipping unparseable events_log mirror row");
            }
        }
    }
    Ok(events)
}

fn mirror_event_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MirrorEvent> {
    let event_id: String = row.get(0)?;
    let device: String = row.get(1)?;
    let seq: i64 = row.get(2)?;
    // column 3 (`kind`) is the filter/index column; the full payload lives in
    // `payload_json`, which round-trips the original `EventKind`.
    let payload_json: String = row.get(5)?;
    let ts: String = row.get(4)?;
    let kind: EventKind = serde_json::from_str(&payload_json)
        .map_err(|err| rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(err)))?;
    // A negative seq or unparseable ts is mirror corruption, not a value to
    // silently coerce to 0/epoch — surface it so `collect_mirror_events` skips
    // the row and warns rather than serving fabricated data.
    let seq: u64 = seq.try_into().map_err(|_| {
        let msg = format!("negative event seq {seq} in mirror row");
        rusqlite::Error::FromSqlConversionFailure(
            2,
            rusqlite::types::Type::Integer,
            Box::new(std::io::Error::other(msg)),
        )
    })?;
    let at = parse_ts(&ts).ok_or_else(|| {
        let msg = format!("unparseable event ts {ts:?} in mirror row");
        rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(std::io::Error::other(msg)))
    })?;
    Ok(MirrorEvent { event_id: EventId::new(event_id), device, seq, at, kind })
}

fn parse_ts(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value).ok().map(|value| value.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn must<T, E: std::fmt::Display>(result: Result<T, E>, context: &str) -> T {
        match result {
            Ok(value) => value,
            Err(err) => panic!("{context}: {err}"),
        }
    }

    fn test_db() -> Connection {
        let conn = must(Connection::open_in_memory(), "open in-memory db");
        must(
            conn.execute_batch(
                "CREATE TABLE events_log(
                event_id     TEXT PRIMARY KEY,
                device       TEXT NOT NULL,
                seq          INTEGER NOT NULL,
                kind         TEXT NOT NULL,
                memory_id    TEXT,
                ts           TEXT NOT NULL,
                payload_json TEXT NOT NULL CHECK (json_valid(payload_json))
            );",
            ),
            "create events_log",
        );
        conn
    }

    /// Insert one mirror row. `row` is `(event_id, seq, ts, payload_json)`.
    fn insert_row(conn: &Connection, row: (&str, i64, &str, &str)) {
        let (event_id, seq, ts, payload_json) = row;
        must(
            conn.execute(
                "INSERT INTO events_log(event_id, device, seq, kind, memory_id, ts, payload_json)
                 VALUES (?1, 'dev_test', ?2, 'operator_repair_required', NULL, ?3, ?4)",
                rusqlite::params![event_id, seq, ts, payload_json],
            ),
            "insert row",
        );
    }

    fn good_payload() -> String {
        must(serde_json::to_string(&EventKind::OperatorRepairRequired { reason: "x".to_string() }), "serialize kind")
    }

    // A derived, rebuildable mirror must not let one corrupt/forward-skew row
    // abort the whole page: unknown EventKind variant, unparseable ts, and
    // negative seq are each skipped, the well-formed rows still returned.
    #[test]
    fn collect_skips_unparseable_rows_without_aborting_the_page() {
        let conn = test_db();
        let good = good_payload();
        insert_row(&conn, ("evt_a", 1, "2026-01-01T00:00:00Z", good.as_str()));
        insert_row(&conn, ("evt_unknown_kind", 2, "2026-01-02T00:00:00Z", r#"{"not_a_real_variant":true}"#));
        insert_row(&conn, ("evt_bad_ts", 3, "not-a-timestamp", good.as_str()));
        insert_row(&conn, ("evt_neg_seq", -1, "2026-01-03T00:00:00Z", good.as_str()));
        insert_row(&conn, ("evt_b", 4, "2026-01-04T00:00:00Z", good.as_str()));

        let events =
            must(query_events_log_page(&conn, &EventsLogPage { limit: 100, ..Default::default() }), "query page");
        let ids: Vec<&str> = events.iter().map(|event| event.event_id.as_str()).collect();
        // Newest-first by ts; only the two well-formed rows survive.
        assert_eq!(ids, vec!["evt_b", "evt_a"]);
    }

    #[test]
    fn collect_returns_every_well_formed_row() {
        let conn = test_db();
        let good = good_payload();
        insert_row(&conn, ("evt_a", 1, "2026-01-01T00:00:00Z", good.as_str()));
        insert_row(&conn, ("evt_b", 2, "2026-01-02T00:00:00Z", good.as_str()));

        let events =
            must(query_events_log_page(&conn, &EventsLogPage { limit: 100, ..Default::default() }), "query page");
        assert_eq!(events.len(), 2);
    }

    /// Insert one mirror row for an explicit device. `row` is `(event_id, seq, ts, payload_json)`.
    fn insert_row_dev(conn: &Connection, device: &str, row: (&str, i64, &str, &str)) {
        let (event_id, seq, ts, payload_json) = row;
        must(
            conn.execute(
                "INSERT INTO events_log(event_id, device, seq, kind, memory_id, ts, payload_json)
                 VALUES (?1, ?2, ?3, 'operator_repair_required', NULL, ?4, ?5)",
                rusqlite::params![event_id, device, seq, ts, payload_json],
            ),
            "insert row dev",
        );
    }

    // The cursor must page through history in canonical (ts, seq, event_id) order,
    // NOT by lexical event_id. Here event-id lexical order deliberately disagrees
    // with ts order, so a `event_id > cursor` cursor would duplicate the newest row
    // and skip the oldest across pages.
    #[test]
    fn page_cursor_walks_history_chronologically_not_by_uuid() {
        let conn = test_db();
        let good = good_payload();
        insert_row(&conn, ("evt_bbb", 1, "2026-01-01T00:00:00Z", good.as_str())); // oldest
        insert_row(&conn, ("evt_yyy", 2, "2026-01-02T00:00:00Z", good.as_str()));
        insert_row(&conn, ("evt_aaa", 3, "2026-01-03T00:00:00Z", good.as_str()));
        insert_row(&conn, ("evt_zzz", 4, "2026-01-04T00:00:00Z", good.as_str())); // newest

        let page1 = must(query_events_log_page(&conn, &EventsLogPage { limit: 2, ..Default::default() }), "page1");
        let ids1: Vec<&str> = page1.iter().map(|event| event.event_id.as_str()).collect();
        assert_eq!(ids1, vec!["evt_zzz", "evt_aaa"], "newest-first page");

        let cursor = ids1[1]; // "evt_aaa", the oldest row on page 1
        let page2 = must(
            query_events_log_page(
                &conn,
                &EventsLogPage { since_event_id: Some(cursor), limit: 2, ..Default::default() },
            ),
            "page2",
        );
        let ids2: Vec<&str> = page2.iter().map(|event| event.event_id.as_str()).collect();
        assert_eq!(ids2, vec!["evt_yyy", "evt_bbb"], "older page in chronological keyset order");

        // Across both pages: every row exactly once, none duplicated or skipped.
        let mut all: Vec<&str> = ids1.iter().chain(ids2.iter()).copied().collect();
        all.sort_unstable();
        assert_eq!(all, vec!["evt_aaa", "evt_bbb", "evt_yyy", "evt_zzz"]);
    }

    // Page and window scope to a single device when one is given; `None` returns all.
    #[test]
    fn page_and_window_filter_by_device() {
        let conn = test_db();
        let good = good_payload();
        insert_row_dev(&conn, "dev_local", ("evt_l1", 1, "2026-02-01T00:00:00Z", good.as_str()));
        insert_row_dev(&conn, "dev_peer", ("evt_p1", 1, "2026-02-02T00:00:00Z", good.as_str()));
        insert_row_dev(&conn, "dev_local", ("evt_l2", 2, "2026-02-03T00:00:00Z", good.as_str()));

        let local_page = must(
            query_events_log_page(
                &conn,
                &EventsLogPage { device: Some("dev_local"), limit: 100, ..Default::default() },
            ),
            "local page",
        );
        let ids: Vec<&str> = local_page.iter().map(|event| event.event_id.as_str()).collect();
        assert_eq!(ids, vec!["evt_l2", "evt_l1"], "device-scoped page omits the peer row");

        let all_page =
            must(query_events_log_page(&conn, &EventsLogPage { limit: 100, ..Default::default() }), "all page");
        assert_eq!(all_page.len(), 3, "None device scope returns every device");

        let since = must(DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z"), "since ts").with_timezone(&Utc);
        let local_window = must(query_events_log_window(&conn, None, Some("dev_local"), since), "local window");
        assert_eq!(local_window.len(), 2, "device-scoped window omits the peer row");
        let all_window = must(query_events_log_window(&conn, None, None, since), "all window");
        assert_eq!(all_window.len(), 3, "None device scope window returns every device");
    }
}
