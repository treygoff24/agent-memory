//! Event log append/read operations.

use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::WriteFailureKind;
use crate::events::framing::{decode_line, encode_event_line, EventFramingError, MAX_LINE_BYTES};
use crate::model::{ClassificationOutcome, DeviceId, EventId, MemoryId, OperationId, RepoPath};

/// Substrate schema version stamped on every event (mirrors spec §12.1).
///
/// Single source of truth: derives from `crate::SUBSTRATE_SCHEMA_VERSION`.
pub const EVENT_SCHEMA_VERSION: u32 = crate::SUBSTRATE_SCHEMA_VERSION;

/// Stream A event per spec §12.1.
///
/// All eight fields from spec §12.1 are present. `crc32c` is zero on
/// construction and is written to the JSONL line by `framing::encode_event_line`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Event {
    /// Substrate schema version stamped at emission (spec §12.1).
    pub schema: u32,
    /// Event id.
    pub id: EventId,
    /// Timestamp (`ts` on disk per spec §12.1).
    #[serde(rename = "ts")]
    pub at: DateTime<Utc>,
    /// Emitting device id (spec §12.1).
    pub device: DeviceId,
    /// Per-device monotonic sequence number (spec §12.1).
    pub seq: u64,
    /// Operation id (audit trail; not in spec §12.1 but load-bearing).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<OperationId>,
    /// Event kind. Serialized as `"kind": "write_committed", "data": { ... }`
    /// per spec §12.1 (adjacently tagged so the payload lives in `data`, not
    /// flattened, avoiding field-name collisions with `id`).
    pub kind: EventKind,
    /// CRC32C checksum (spec §12.1). Written by `framing::encode_event_line`.
    /// Callers set this to 0; the framing layer fills in the real value.
    #[serde(default)]
    pub crc32c: u32,
}

/// Event kinds with typed payloads.
///
/// Adjacently tagged: `"kind": "write_committed", "data": { ... }` per spec §12.1.
/// The `data` subobject avoids field-name collisions with `Event.id`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum EventKind {
    /// Write committed.
    WriteCommitted { id: MemoryId, path: RepoPath, classification: ClassificationOutcome },
    /// Encrypted write committed.
    EncryptedWriteCommitted { id: MemoryId, path: RepoPath, classification: ClassificationOutcome },
    /// Tombstone committed.
    TombstoneCommitted { id: MemoryId },
    /// Duplicate id repaired.
    DuplicateIdRepaired { old_id: MemoryId, new_id: MemoryId },
    /// Embedding model changed.
    EmbeddingModelChanged { chunks_requeued: usize },
    /// Startup reconciliation completed.
    StartupReconciliationCompleted { reindexed: usize, repaired_events: usize },
    /// Operator repair required.
    OperatorRepairRequired { reason: String },
    /// Git push failed.
    GitPushFailed { reason: String },
    /// A write was refused (spec §8.7 step 6 / §12.2 `WriteRefused`).
    ///
    /// Phase 5 surface: enables Stream D audit-trail confirmation that every
    /// write got a positive classification call. Refusal events are appended
    /// to the per-device event log; no canonical-disk side-effect occurs.
    WriteRefused {
        /// Optional memory id (some refusals fail before id is materialized).
        #[serde(skip_serializing_if = "Option::is_none", default)]
        id: Option<MemoryId>,
        /// Optional intended path (None for refusals before path validation).
        #[serde(skip_serializing_if = "Option::is_none", default)]
        path: Option<RepoPath>,
        /// Classification supplied by the caller.
        classification: ClassificationOutcome,
        /// Refusal reason; serializes through `WriteFailureKind`'s display.
        reason: String,
    },
    /// Encrypted content was explicitly revealed through a user-directed privacy path.
    EncryptedContentRevealed {
        /// Memory id revealed.
        id: MemoryId,
        /// Bounded, non-plaintext audit reason supplied by the caller.
        reason: String,
    },
    /// Stream F substrate fragment appended.
    SubstrateFragmentWritten {
        /// Fragment id (`sub_<ulid>`).
        id: String,
        /// JSONL path written.
        path: RepoPath,
        /// Classification that selected plaintext vs encrypted substrate.
        classification: ClassificationOutcome,
    },
    // Deferred §12.2 event kinds: WriteStarted, WriteIndexed, WriteEventAppendFailed,
    // Deleted, Superseded, IndexUpdated, IndexFailed, VectorReconciled,
    // EmbeddingJobEnqueued, EventLogRecovered, MergeQuarantined,
    // PendingIndexReplayed, PendingEventReplayed, GitCommitted, GitFetched,
    // WatcherSuppressed, ReconciliationRepaired.
}

impl EventKind {
    /// Build a `WriteRefused` event from a refusal-failure kind.
    ///
    /// Centralizes `WriteFailureKind` → `String` so all refusal sites encode
    /// the reason consistently for Stream D's audit consumers.
    pub fn write_refused(
        id: Option<MemoryId>,
        path: Option<RepoPath>,
        classification: ClassificationOutcome,
        kind: &WriteFailureKind,
    ) -> Self {
        Self::WriteRefused { id, path, classification, reason: kind.to_string() }
    }
}

/// Outcome returned by git commit helpers.
#[derive(Debug, Eq, PartialEq)]
pub enum CommitOutcome {
    /// Nothing to commit; index was clean.
    NoChanges,
    /// Commit succeeded with this SHA.
    Committed { sha: String },
}

/// Error variants for event-log operations.
#[derive(Debug, thiserror::Error)]
pub enum EventLogError {
    /// A line exceeded the 64-KiB limit (spec §12.3 step 1).
    #[error("event line too long: {byte_len} bytes")]
    LineTooLong {
        /// Byte length of the rejected line.
        byte_len: usize,
    },
    /// JSON parse error on a specific line.
    #[error("malformed event log line {line_no}: {source}")]
    Parse {
        /// 1-based line number.
        line_no: u64,
        /// Underlying JSON error.
        source: serde_json::Error,
    },
    /// IO error.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl From<EventFramingError> for std::io::Error {
    fn from(err: EventFramingError) -> Self {
        std::io::Error::other(err.to_string())
    }
}

/// Append an event and fsync the log.
///
/// Serializes the event through `framing::encode_event_line` which injects the
/// `crc32c` field per spec §12.1. The caller sets `event.crc32c = 0`; the
/// framing layer computes and embeds the real checksum in the JSONL line.
///
/// Returns `Err` if the line exceeds 64 KiB (spec §12.3 step 1).
pub fn append_event(path: &Path, event: &Event) -> std::io::Result<()> {
    append_event_inner(path, event, true)
}

/// Append an event without forcing it to stable storage.
///
/// This is only used when the substrate was explicitly opened in
/// `DurabilityTier::BestEffort` mode. Full-durability repositories keep using
/// `append_event`, which preserves the Stream A event-log fsync contract.
pub fn append_event_best_effort(path: &Path, event: &Event) -> std::io::Result<()> {
    append_event_inner(path, event, false)
}

fn append_event_inner(path: &Path, event: &Event, sync: bool) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let value = serde_json::to_value(event).map_err(std::io::Error::other)?;
    let line = encode_event_line(&value)?;
    if line.len() > MAX_LINE_BYTES {
        return Err(std::io::Error::other(format!("event line too long: {} bytes (max {MAX_LINE_BYTES})", line.len())));
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(line.as_bytes())?;
    if sync {
        file.sync_all()?;
    }
    Ok(())
}

/// Read valid events, failing on any malformed line.
///
/// Use `read_events` for a forgiving reader that falls back to recovery.
pub fn read_events_strict(path: &Path) -> std::io::Result<Vec<Event>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = OpenOptions::new().read(true).open(path)?;
    let mut events = Vec::new();
    let mut seen = HashSet::new();
    for (line_index, line) in BufReader::new(file).lines().enumerate() {
        let line = line?;
        let value = decode_line(&line).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, format!("malformed event log line {}", line_index + 1))
        })?;
        let event = serde_json::from_value::<Event>(value).map_err(|err| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("invalid event payload on line {}: {err}", line_index + 1),
            )
        })?;
        if !seen.insert(event.id.clone()) {
            continue;
        }
        events.push(event);
    }
    Ok(events)
}

/// Read valid events, falling back to recovery on the first malformed line.
///
/// If `read_events_strict` fails with `InvalidData`, `recover_event_log` is
/// called to truncate the malformed trailing line, then the log is re-read.
pub fn read_events(path: &Path) -> std::io::Result<Vec<Event>> {
    match read_events_strict(path) {
        Ok(events) => Ok(events),
        Err(io_err) if io_err.kind() == std::io::ErrorKind::InvalidData => {
            crate::events::recovery::recover_event_log(path)?;
            read_events_strict(path)
        }
        Err(other) => Err(other),
    }
}

/// Rewrite an event log with the provided event set.
///
/// Used by Stream F cleanup compaction after old events have been moved into
/// compressed monthly archives. The same framing path as [`append_event`] is
/// used so CRC semantics stay identical for the retained live tail.
pub fn rewrite_events(path: &Path, events: &[Event]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut bytes = Vec::new();
    for event in events {
        let value = serde_json::to_value(event).map_err(std::io::Error::other)?;
        let line = encode_event_line(&value)?;
        if line.len() > MAX_LINE_BYTES {
            return Err(std::io::Error::other(format!(
                "event line too long: {} bytes (max {MAX_LINE_BYTES})",
                line.len()
            )));
        }
        bytes.extend_from_slice(line.as_bytes());
    }
    fs::write(path, bytes)
}

/// Refuse copied same-device logs until adoption repair removes the copy.
///
/// A file whose stem is exactly `<local_device_id>` or starts with
/// `<local_device_id>` followed by a copy-tool separator (space, parenthesis)
/// is considered a duplicate. Files with a distinct device ID prefix are
/// legitimate peer logs and are never refused.
pub fn refuse_duplicate_device_logs(events_dir: &Path, local_device_id: &DeviceId) -> std::io::Result<()> {
    if !events_dir.exists() {
        return Ok(());
    }
    let prefix = local_device_id.as_str();
    let mut local_count: usize = 0;
    for entry in fs::read_dir(events_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "jsonl") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if is_local_device_log(stem, prefix) {
            local_count += 1;
        }
    }
    if local_count > 1 {
        return Err(std::io::Error::other(format!("duplicate event log for device {prefix}; run adopt_clone repair")));
    }
    Ok(())
}

/// Return true when `stem` is either exactly `device_prefix` or starts with it
/// followed by a copy-tool separator recognised by Finder/Windows Copy.
fn is_local_device_log(stem: &str, device_prefix: &str) -> bool {
    if stem == device_prefix {
        return true;
    }
    // Finder: "dev_abc copy", "dev_abc (2)"
    if let Some(rest) = stem.strip_prefix(device_prefix) {
        return rest.starts_with(' ') || rest.starts_with('(');
    }
    false
}
