//! Persistent per-device event sequence allocation.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;

use fs2::FileExt;
use serde::{Deserialize, Serialize};

use crate::events::read_events;
use crate::events::Event;
use crate::model::DeviceId;

/// Persisted event-sequence state.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct EventSeqState {
    /// Device this sequence file belongs to.
    device_id: String,
    /// Next sequence number to allocate.
    next: u64,
}

/// Reconcile the persisted sequence state against the canonical log's high-water
/// mark. Used at substrate open, where a persisted `event-seq.json` may be stale
/// relative to the JSONL log (fresh clone, post-merge log, post-compaction log),
/// so the full-log high-water read is the authoritative recovery step.
pub(crate) fn sync_event_sequence_state(runtime: &Path, event_log: &Path, device_id: &DeviceId) -> std::io::Result<()> {
    let _lock = lock_sequence_file(runtime)?;
    let path = runtime.join("event-seq.json");
    let mut state = read_state(&path, device_id).unwrap_or_else(|_| fresh_state(device_id));
    state.next = state.next.max(event_seq_high_water(event_log, device_id)?);
    write_state_atomic(runtime, &path, &state)
}

/// Ensure the persisted sequence state exists, trusting its `next` on the
/// steady-state path and only re-deriving the high-water mark from the canonical
/// log when the persisted state is missing or unreadable.
///
/// This is the hot-path counterpart to [`sync_event_sequence_state`]. The
/// persisted `event-seq.json` is maintained atomically under a file lock by
/// [`reserve_event_sequence`], so once a substrate has reconciled at open its
/// `next` is already correct for every subsequent append. Re-reading and
/// serde-parsing the entire JSONL event log on each append (the
/// `max(event_seq_high_water(...))` form) is a full-log scan on the hottest path
/// in the system; here the high-water read is taken only when the state file is
/// missing or could not be read.
///
/// `event-seq.json` is a derived cache — the canonical JSONL log is the source of
/// truth — so a corrupt or transiently-unreadable file is rebuilt from the log
/// rather than failing the write path. A genuinely unrecoverable condition (the
/// log is unreadable, or the rebuilt state cannot be written) still surfaces from
/// `load_or_recover_state`/`write_state_atomic` below. The read error is logged
/// rather than silently swallowed so a recurring cause is visible.
pub(crate) fn ensure_event_sequence_state(
    runtime: &Path,
    event_log: &Path,
    device_id: &DeviceId,
) -> std::io::Result<()> {
    let _lock = lock_sequence_file(runtime)?;
    let path = runtime.join("event-seq.json");
    match read_state(&path, device_id) {
        // Persisted state is present and valid; the file lock + atomic writes in
        // `reserve_event_sequence` keep its `next` authoritative. No full-log
        // scan needed.
        Ok(_) => return Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            tracing::warn!("event-seq.json unreadable ({err}); rebuilding sequence state from the canonical log");
        }
    }
    let state = load_or_recover_state(event_log, &path, device_id)?;
    write_state_atomic(runtime, &path, &state)
}

/// Reserve the next per-device event sequence number.
pub(crate) fn reserve_event_sequence(runtime: &Path, event_log: &Path, device_id: &DeviceId) -> std::io::Result<u64> {
    let _lock = lock_sequence_file(runtime)?;
    let path = runtime.join("event-seq.json");
    let mut state = load_or_recover_state(event_log, &path, device_id)?;
    let seq = state.next;
    state.next = state.next.saturating_add(1);
    write_state_atomic(runtime, &path, &state)?;
    Ok(seq)
}

/// Reserve `count` per-device event sequence numbers with one persisted state update.
///
/// The returned values match `count` consecutive calls to [`reserve_event_sequence`],
/// including saturating behavior at `u64::MAX`.
pub(crate) fn reserve_event_sequences(
    runtime: &Path,
    event_log: &Path,
    device_id: &DeviceId,
    count: usize,
) -> std::io::Result<Vec<u64>> {
    if count == 0 {
        return Ok(Vec::new());
    }
    let _lock = lock_sequence_file(runtime)?;
    let path = runtime.join("event-seq.json");
    let mut state = load_or_recover_state(event_log, &path, device_id)?;
    let start = state.next;
    state.next = state.next.saturating_add(count as u64);
    write_state_atomic(runtime, &path, &state)?;
    Ok((0..count).map(|offset| start.saturating_add(offset as u64)).collect())
}

/// Ensure an event has a non-zero sequence number before appending it.
pub(crate) fn stamp_event_sequence(runtime: &Path, event_log: &Path, event: &mut Event) -> std::io::Result<()> {
    if event.seq == 0 {
        event.seq = reserve_event_sequence(runtime, event_log, &event.device)?;
    }
    Ok(())
}

fn load_or_recover_state(event_log: &Path, path: &Path, device_id: &DeviceId) -> std::io::Result<EventSeqState> {
    match read_state(path, device_id) {
        Ok(state) => Ok(state),
        Err(_) => Ok(EventSeqState {
            device_id: device_id.as_str().to_string(),
            next: event_seq_high_water(event_log, device_id)?,
        }),
    }
}

fn event_seq_high_water(event_log: &Path, device_id: &DeviceId) -> std::io::Result<u64> {
    let events = read_events(event_log)?;
    Ok(events
        .into_iter()
        .filter(|event| &event.device == device_id)
        .map(|event| event.seq)
        .max()
        .map_or(1, |seq| seq.saturating_add(1)))
}

fn fresh_state(device_id: &DeviceId) -> EventSeqState {
    EventSeqState { device_id: device_id.as_str().to_string(), next: 1 }
}

fn read_state(path: &Path, device_id: &DeviceId) -> std::io::Result<EventSeqState> {
    let text = fs::read_to_string(path)?;
    let mut state: EventSeqState = serde_json::from_str(&text).map_err(std::io::Error::other)?;
    if state.device_id != device_id.as_str() {
        state = fresh_state(device_id);
    }
    if state.next == 0 {
        state.next = 1;
    }
    Ok(state)
}

fn write_state_atomic(runtime: &Path, path: &Path, state: &EventSeqState) -> std::io::Result<()> {
    let temp_path = runtime.join("event-seq.json.tmp");
    let bytes = serde_json::to_vec_pretty(state).map_err(std::io::Error::other)?;
    let mut file = OpenOptions::new().create_new(true).write(true).open(&temp_path)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    fs::rename(&temp_path, path)?;
    File::open(runtime)?.sync_all()?;
    Ok(())
}

fn lock_sequence_file(runtime: &Path) -> std::io::Result<File> {
    fs::create_dir_all(runtime)?;
    let lock =
        OpenOptions::new().create(true).read(true).truncate(false).write(true).open(runtime.join("event-seq.lock"))?;
    lock.lock_exclusive()?;
    Ok(lock)
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;
    use crate::events::{append_event, EventKind};
    use crate::model::EventId;

    fn event(device: &DeviceId, seq: u64) -> Event {
        Event {
            schema: crate::SUBSTRATE_SCHEMA_VERSION,
            id: EventId::new(format!("evt_{seq}")),
            at: Utc::now(),
            device: device.clone(),
            seq,
            operation_id: None,
            kind: EventKind::OperatorRepairRequired { reason: "test".to_string() },
            crc32c: 0,
        }
    }

    fn must<T, E: std::fmt::Display>(result: Result<T, E>, context: &str) -> T {
        match result {
            Ok(value) => value,
            Err(err) => panic!("{context}: {err}"),
        }
    }

    fn write_stale_state(runtime: &Path, device: &DeviceId, next: u64) {
        let state = EventSeqState { device_id: device.as_str().to_string(), next };
        let bytes = must(serde_json::to_vec_pretty(&state), "serialize state");
        must(std::fs::write(runtime.join("event-seq.json"), bytes), "write stale state");
    }

    // Regression guard for the index-first refactor's BestEffort seq-reuse hazard.
    // When the canonical per-device log has advanced past a persisted
    // `event-seq.json` (e.g. BestEffort-tier appends allocated from the in-memory
    // counter, which `event-seq.json` does not track), `ensure_*` trusts the
    // stale state, so a later `reserve` would hand back a seq already in the log.
    // `sync_*` reconciles against the high-water and prevents the reuse — which is
    // exactly why `Substrate::guard_event_sequence_state` must use `sync_*` in the
    // BestEffort tier rather than `ensure_*`.
    #[test]
    fn sync_reconciles_stale_state_but_ensure_trusts_it() {
        let temp = must(tempfile::tempdir(), "tempdir");
        let runtime = temp.path().join("runtime");
        must(std::fs::create_dir_all(&runtime), "mkdir runtime");
        let event_log = temp.path().join("events").join("dev_test.jsonl");
        let device = must(DeviceId::try_new("dev_test"), "device id");
        let state_path = runtime.join("event-seq.json");

        // Log high-water is 10; persisted state is stale at next=5.
        for seq in 1..=10 {
            must(append_event(&event_log, &event(&device, seq)), "append event");
        }
        write_stale_state(&runtime, &device, 5);

        // ensure trusts the stale state: next stays 5 (the reuse hazard).
        must(ensure_event_sequence_state(&runtime, &event_log, &device), "ensure");
        assert_eq!(must(read_state(&state_path, &device), "read state").next, 5);

        // sync reconciles to high_water + 1 = 11, so the next reserved seq is past
        // every seq already in the log — no reuse.
        must(sync_event_sequence_state(&runtime, &event_log, &device), "sync");
        assert_eq!(must(read_state(&state_path, &device), "read state").next, 11);
        assert_eq!(must(reserve_event_sequence(&runtime, &event_log, &device), "reserve"), 11);
    }
}
