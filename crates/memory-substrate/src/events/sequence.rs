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

/// Sync the persisted sequence state to the current log high-water mark.
pub(crate) fn sync_event_sequence_state(runtime: &Path, event_log: &Path, device_id: &DeviceId) -> std::io::Result<()> {
    let _lock = lock_sequence_file(runtime)?;
    let path = runtime.join("event-seq.json");
    let mut state = read_state(&path, device_id).unwrap_or_else(|_| fresh_state(device_id));
    state.next = state.next.max(event_seq_high_water(event_log, device_id)?);
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
