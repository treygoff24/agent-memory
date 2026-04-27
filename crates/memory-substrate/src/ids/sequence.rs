//! Sequence-backed memory ID allocation.

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;

use chrono::Utc;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::IdError;
use crate::model::MemoryId;

/// Local sequence state.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SeqState {
    /// UTC date YYYY-MM-DD.
    pub date: String,
    /// Next sequence number.
    pub next: u32,
    /// Device id.
    pub device_id: String,
}

/// Stable 16-hex shard for a device id.
pub fn shard_for_device(device_id: &str) -> String {
    let digest = Sha256::digest(device_id.as_bytes());
    hex::encode(&digest[..8])
}

/// Allocate the next memory id.
pub fn next_memory_id(runtime: &Path, device_id: &str, reserved: &HashSet<MemoryId>) -> Result<MemoryId, IdError> {
    let mut ids = next_memory_ids(runtime, device_id, reserved, 1)?;
    ids.pop().ok_or_else(|| IdError::InvalidState("batch allocator returned no ids".to_string()))
}

/// Allocate a batch of memory ids under one sequence-file lock.
pub fn next_memory_ids(
    runtime: &Path,
    device_id: &str,
    reserved: &HashSet<MemoryId>,
    count: usize,
) -> Result<Vec<MemoryId>, IdError> {
    fs::create_dir_all(runtime).map_err(|err| IdError::InvalidState(err.to_string()))?;
    let lock = OpenOptions::new()
        .create(true)
        .read(true)
        .truncate(false)
        .write(true)
        .open(runtime.join("seq.lock"))
        .map_err(|err| IdError::InvalidState(err.to_string()))?;
    lock.lock_exclusive().map_err(|err| IdError::InvalidState(err.to_string()))?;
    let path = runtime.join("seq.json");
    let today = Utc::now().date_naive().format("%Y-%m-%d").to_string();
    let mut state = read_state(&path, device_id, &today)?;
    if state.device_id != device_id {
        return Err(IdError::DeviceMismatch);
    }
    let shard = shard_for_device(device_id);
    let high_water_next = reserved_high_water_next(reserved, &today, &shard);
    state.next = state.next.max(high_water_next);
    let mut ids = Vec::with_capacity(count);
    let mut allocated = HashSet::with_capacity(count);
    while ids.len() < count {
        // Guard: check exhaustion before calling format_id (which panics on
        // seq > 999_999 because MemoryId::new validates the 6-digit format).
        if state.next > 999_999 {
            return Err(IdError::SequenceExhausted { date: today });
        }
        let candidate = format_id(&today, &shard, state.next);
        if reserved.contains(&candidate) || allocated.contains(&candidate) {
            state.next += 1;
            continue;
        }
        allocated.insert(candidate.clone());
        ids.push(candidate);
        state.next += 1;
    }
    write_state_atomic(runtime, &path, &state)?;
    Ok(ids)
}

fn read_state(path: &Path, device_id: &str, today: &str) -> Result<SeqState, IdError> {
    if path.exists() {
        let text = fs::read_to_string(path).map_err(|err| IdError::InvalidState(err.to_string()))?;
        let mut state: SeqState = serde_json::from_str(&text).map_err(|err| IdError::InvalidState(err.to_string()))?;
        if state.date != today {
            // R-FT-4: clock regression check. If the stored date is *ahead* of
            // today the local clock has moved backwards (NTP correction, timezone
            // change, VM resume, etc.). Refuse to allocate: issuing IDs with a
            // stale future date would corrupt the global sort order and silently
            // produce duplicates when the clock catches up.
            if state.date.as_str() > today {
                return Err(IdError::ClockRegression { last_allocated: state.date, now: today.to_string() });
            }
            // Normal calendar rollover: reset the sequence for the new day.
            state.date = today.to_string();
            state.next = 1;
        }
        Ok(state)
    } else {
        Ok(SeqState { date: today.to_string(), next: 1, device_id: device_id.to_string() })
    }
}

fn format_id(date: &str, shard: &str, seq: u32) -> MemoryId {
    MemoryId::new(format!("mem_{}_{}_{seq:06}", date.replace('-', ""), shard))
}

fn reserved_high_water_next(reserved: &HashSet<MemoryId>, today: &str, shard: &str) -> u32 {
    let prefix = format!("mem_{}_{}_", today.replace('-', ""), shard);
    reserved
        .iter()
        .filter_map(|id| id.as_str().strip_prefix(&prefix))
        .filter_map(|seq| seq.parse::<u32>().ok())
        .max()
        .map_or(1, |seq| seq.saturating_add(1))
}

fn write_state_atomic(runtime: &Path, path: &Path, state: &SeqState) -> Result<(), IdError> {
    let temp_path = runtime.join("seq.json.tmp");
    let bytes = serde_json::to_vec_pretty(state).map_err(|err| IdError::InvalidState(err.to_string()))?;
    if temp_path.exists() {
        fs::remove_file(&temp_path).map_err(|err| IdError::InvalidState(err.to_string()))?;
    }
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp_path)
        .map_err(|err| IdError::InvalidState(err.to_string()))?;
    file.write_all(&bytes).map_err(|err| IdError::InvalidState(err.to_string()))?;
    file.sync_all().map_err(|err| IdError::InvalidState(err.to_string()))?;
    fs::rename(&temp_path, path).map_err(|err| IdError::InvalidState(err.to_string()))?;
    File::open(runtime).and_then(|dir| dir.sync_all()).map_err(|err| IdError::InvalidState(err.to_string()))?;
    Ok(())
}
