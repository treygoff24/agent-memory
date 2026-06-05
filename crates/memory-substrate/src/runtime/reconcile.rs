//! Startup reconciliation and durable repair queues.
//!
//! Newspaper layout: `reconcile_all_phases` orchestrator at top, nine phase
//! helpers in spec §13.5.1 order below, private helpers at the bottom.

use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::events::{
    append_event, decode_line, encode_event_line, read_events, recover_event_log, stamp_event_sequence, Event,
    EventKind,
};
use crate::index::Index;
use crate::markdown::read_memory_file;
use crate::model::{EventId, Memory, MemoryId, MemoryStatus, OperationId, RepoPath, Sha256, TrustLevel};

/// Durable pending index operation kind.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PendingIndexKind {
    /// Upsert a repo path into the derived index.
    UpsertPath,
    /// Delete a repo path from the derived index.
    DeletePath,
}

/// Durable pending index operation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PendingIndexOp {
    /// Operation id.
    pub op_id: OperationId,
    /// Operation kind.
    pub kind: PendingIndexKind,
    /// Repo-relative path.
    pub path: RepoPath,
    /// Optional memory id.
    pub memory_id: Option<MemoryId>,
    /// Expected file hash.
    pub expected_file_hash: Option<Sha256>,
    /// Enqueue timestamp.
    pub enqueued_at: DateTime<Utc>,
    /// Replay attempts.
    pub attempts: u32,
    /// Last replay error.
    pub last_error: Option<String>,
}

/// Durable encrypted index repair operation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PendingEncryptedIndexOp {
    /// Operation id.
    pub op_id: OperationId,
    /// Safe metadata/projection to index.
    pub indexed_memory: Memory,
    /// Whether this record must remain metadata-only.
    pub metadata_only: bool,
    /// Expected ciphertext hash.
    pub expected_ciphertext_hash: Sha256,
    /// Enqueue timestamp.
    pub enqueued_at: DateTime<Utc>,
    /// Replay attempts.
    pub attempts: u32,
    /// Last replay error.
    pub last_error: Option<String>,
}

/// Durable pending event operation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PendingEventOp {
    /// Operation id.
    pub op_id: OperationId,
    /// Event id.
    pub event_id: EventId,
    /// Event payload.
    pub event: Event,
    /// Enqueue timestamp.
    pub enqueued_at: DateTime<Utc>,
    /// Replay attempts.
    pub attempts: u32,
    /// Last replay error.
    pub last_error: Option<String>,
}

/// Outcomes of the full nine-phase startup reconciliation (spec §13.5.1).
#[derive(Clone, Debug, Default)]
pub struct ReconcileReport {
    /// Phase names that ran to completion.
    pub phases_run: Vec<&'static str>,
    /// Pending vector jobs successfully replayed.
    pub vector_repairs: u32,
    /// Pending event ops successfully replayed.
    pub event_repairs: u32,
    /// Pending index ops successfully replayed.
    pub pending_index_replays: u32,
    /// Disk files whose index row was stale and required reindexing (phase 7).
    pub reindexed_memories: u32,
    /// Whether the operator must take action before writes resume.
    pub operator_action_required: bool,
    /// Whether a crash-recovery marker or mid-merge MERGE_HEAD was found.
    pub recovery_required: bool,
    /// Whether an auto-commit was performed during reconciliation.
    pub auto_committed: bool,
    /// Repository-relative memory paths whose merge was quarantined and require
    /// operator attention. Consumed by the daemon to emit
    /// NotificationEvent::BlockingMergeConflict per entry.
    pub blocking_conflicts: Vec<String>,
}

/// Append a durable pending index operation.
pub fn enqueue_pending_index(runtime: &Path, op: &PendingIndexOp) -> std::io::Result<()> {
    append_framed_jsonl(&runtime.join("pending/index-ops.jsonl"), op)
}

/// Append a durable pending event operation.
pub fn enqueue_pending_event(runtime: &Path, op: &PendingEventOp) -> std::io::Result<()> {
    append_framed_jsonl(&runtime.join("pending/events.jsonl"), op)
}

/// Append a durable pending encrypted-index operation.
pub fn enqueue_pending_encrypted_index(runtime: &Path, op: &PendingEncryptedIndexOp) -> std::io::Result<()> {
    append_framed_jsonl(&runtime.join("pending/encrypted-index-ops.jsonl"), op)
}

/// Write the startup reconciliation marker.
pub fn write_startup_marker(runtime: &Path, reason: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(runtime)?;
    let path = runtime.join("startup-reconcile.required");
    std::fs::write(&path, reason.as_bytes())?;
    fsync_parent(&path)
}

/// Run the full nine-phase startup reconciliation (spec §13.5.1).
///
/// Phases run in order; each appends its name to `report.phases_run` on
/// success. A phase may return `Err` only for conditions that require operator
/// repair before writes may proceed — all other errors are tolerated and
/// surfaced via events.
///
/// The single `StartupReconciliationCompleted` event is emitted in phase 9,
/// after all other phases, so its counts reflect reality.
/// The startup-reconcile marker is cleared only on full success.
#[allow(clippy::too_many_arguments)]
pub fn reconcile_all_phases(
    repo: &Path,
    runtime: &Path,
    event_log: &Path,
    device_id: &crate::model::DeviceId,
    index: &mut Index,
) -> std::io::Result<ReconcileReport> {
    std::fs::create_dir_all(runtime.join("pending"))?;
    let mut report = ReconcileReport::default();

    phase_1_crash_recovery_scan(repo, runtime, &mut report)?;
    phase_2_event_log_recovery(event_log, &mut report)?;
    phase_3_replay_pending_index(repo, runtime, index, &mut report)?;
    phase_4_replay_pending_encrypted_index(repo, runtime, index, &mut report)?;
    phase_5_replay_pending_events(runtime, event_log, &mut report)?;
    phase_6_index_consistency(repo, index, &mut report)?;
    phase_9_emit_completion(runtime, event_log, device_id, &mut report)?;

    Ok(report)
}

/// Phase 1 — Crash-recovery scan.
///
/// Reads `<runtime>/startup-reconcile.required` and `<repo>/.git/MERGE_HEAD`.
/// Sets `report.recovery_required` when either is present.
fn phase_1_crash_recovery_scan(repo: &Path, runtime: &Path, report: &mut ReconcileReport) -> std::io::Result<()> {
    let marker = runtime.join("startup-reconcile.required");
    let merge_head = repo.join(".git/MERGE_HEAD");
    report.recovery_required = marker.exists() || merge_head.exists();
    report.phases_run.push("crash_recovery_scan");
    Ok(())
}

/// Phase 2 — Event-log recovery.
///
/// Truncates a trailing-malformed line from the event log per spec §12.3 step
/// 5. Returns `Err(OperatorRepairRequired)` for non-final malformed lines.
fn phase_2_event_log_recovery(event_log: &Path, report: &mut ReconcileReport) -> std::io::Result<()> {
    recover_event_log(event_log)?;
    report.phases_run.push("event_log_recovery");
    Ok(())
}

/// Phase 3 — Replay pending plaintext index ops.
///
/// For each op: on hash match → index + track count.
/// On hash mismatch → keep for next startup (hash-mismatch deferred).
/// After all ops: if all succeeded → delete the queue file.
/// If some remain → rewrite the queue with only the remaining ops.
fn phase_3_replay_pending_index(
    repo: &Path,
    runtime: &Path,
    index: &mut Index,
    report: &mut ReconcileReport,
) -> std::io::Result<()> {
    let path = runtime.join("pending/index-ops.jsonl");
    let ops = read_framed_jsonl_strict::<PendingIndexOp>(&path)?;
    let mut remaining = Vec::new();

    for op in ops {
        match op.kind {
            PendingIndexKind::UpsertPath => {
                let replay_result = upsert_with_hash_check(repo, index, &op);
                match replay_result {
                    ReplayOutcome::Replayed => report.pending_index_replays += 1,
                    ReplayOutcome::Deferred => remaining.push(op),
                    ReplayOutcome::Failed => remaining.push(op),
                }
            }
            PendingIndexKind::DeletePath => remaining.push(op),
        }
    }

    finalize_pending_queue(&path, &remaining)?;
    report.phases_run.push("replay_pending_index");
    Ok(())
}

/// Phase 4 — Replay pending encrypted index ops.
///
/// Hash mismatch: defer (keep in remaining, don't abort reconciliation).
/// Corruption (missing path): quarantine and emit a warning.
fn phase_4_replay_pending_encrypted_index(
    repo: &Path,
    runtime: &Path,
    index: &mut Index,
    report: &mut ReconcileReport,
) -> std::io::Result<()> {
    let path = runtime.join("pending/encrypted-index-ops.jsonl");
    let ops = read_framed_jsonl_strict::<PendingEncryptedIndexOp>(&path)?;
    let mut remaining = Vec::new();

    for op in ops {
        let outcome = replay_encrypted_op(repo, index, &op);
        match outcome {
            ReplayOutcome::Replayed => report.pending_index_replays += 1,
            ReplayOutcome::Deferred => remaining.push(op),
            ReplayOutcome::Failed => {
                // Quarantine: log and discard this op rather than aborting
                // reconciliation for all remaining ops (B-RT-2).
                tracing::warn!(
                    op_id = op.op_id.as_str(),
                    "encrypted pending index op quarantined due to hash mismatch or missing path"
                );
                report.operator_action_required = true;
            }
        }
    }

    finalize_pending_queue(&path, &remaining)?;
    report.phases_run.push("replay_pending_encrypted_index");
    Ok(())
}

/// Phase 5 — Replay pending event ops.
///
/// Idempotent: skips events already present in the log by id.
/// After replay: deletes the queue file (Q11 — no `.compacted.jsonl` rename).
fn phase_5_replay_pending_events(
    runtime: &Path,
    event_log: &Path,
    report: &mut ReconcileReport,
) -> std::io::Result<()> {
    let path = runtime.join("pending/events.jsonl");
    let ops = read_framed_jsonl_strict::<PendingEventOp>(&path)?;
    if ops.is_empty() {
        report.phases_run.push("replay_pending_events");
        return Ok(());
    }

    let existing_ids: std::collections::HashSet<_> = read_events(event_log)?.into_iter().map(|e| e.id).collect();

    for op in ops {
        if existing_ids.contains(&op.event.id) {
            continue;
        }
        let mut event = op.event;
        stamp_event_sequence(runtime, event_log, &mut event)?;
        append_event(event_log, &event)?;
        report.event_repairs += 1;
    }

    // Delete after successful replay (Q11: no rename to .compacted.jsonl).
    if path.exists() {
        std::fs::remove_file(&path)?;
        fsync_parent(&path)?;
    }

    report.phases_run.push("replay_pending_events");
    Ok(())
}

/// Phase 6 — Index/file consistency.
///
/// For each `.md` in the repo: compare the indexed file hash against what's on
/// disk. Only reindex files whose bytes have drifted.
fn phase_6_index_consistency(repo: &Path, index: &mut Index, report: &mut ReconcileReport) -> std::io::Result<()> {
    let reindexed = reindex_stale_memories(repo, index)
        .map_err(|err| std::io::Error::other(format!("index consistency: {err}")))?;
    report.reindexed_memories = reindexed;
    report.blocking_conflicts =
        scan_blocking_conflicts(repo).map_err(|err| std::io::Error::other(format!("blocking conflict scan: {err}")))?;
    if !report.blocking_conflicts.is_empty() {
        report.operator_action_required = true;
    }
    report.phases_run.push("index_consistency");
    Ok(())
}

/// Phase 9 — Emit single completion event and clear the startup marker.
///
/// Only emitted after all prior phases succeed. Clears the startup-reconcile
/// marker on success.
fn phase_9_emit_completion(
    runtime: &Path,
    event_log: &Path,
    device_id: &crate::model::DeviceId,
    report: &mut ReconcileReport,
) -> std::io::Result<()> {
    let mut event = Event {
        schema: crate::SUBSTRATE_SCHEMA_VERSION,
        id: EventId::new(format!("evt_{}", uuid::Uuid::new_v4())),
        at: Utc::now(),
        device: device_id.clone(),
        seq: 0,
        operation_id: Some(OperationId::new("startup")),
        kind: EventKind::StartupReconciliationCompleted {
            reindexed: report.reindexed_memories as usize,
            repaired_events: report.event_repairs as usize,
        },
        crc32c: 0,
    };
    stamp_event_sequence(runtime, event_log, &mut event)?;
    append_event(event_log, &event)?;
    report.phases_run.push("emit_completion");
    Ok(())
}

/// Extended pre-index startup reconciliation with a full report.
pub fn reconcile_startup_pre_index_report(
    runtime: &Path,
    event_log: &Path,
    repo: &Path,
) -> std::io::Result<ReconcileReport> {
    std::fs::create_dir_all(runtime.join("pending"))?;
    let mut report = ReconcileReport::default();
    phase_1_crash_recovery_scan(repo, runtime, &mut report)?;
    report.event_repairs = recover_event_log(event_log)? as u32;
    report.phases_run.push("event_log_recovery");
    Ok(report)
}

/// Replay durable pending repairs, appending phase results into an existing report.
#[allow(clippy::too_many_arguments)]
pub fn replay_pending_repairs_into_report(
    repo: &Path,
    runtime: &Path,
    event_log: &Path,
    device_id: &crate::model::DeviceId,
    index: &mut Index,
    mut report: ReconcileReport,
) -> std::io::Result<ReconcileReport> {
    phase_3_replay_pending_index(repo, runtime, index, &mut report)?;
    phase_4_replay_pending_encrypted_index(repo, runtime, index, &mut report)?;
    phase_5_replay_pending_events(runtime, event_log, &mut report)?;
    phase_6_index_consistency(repo, index, &mut report)?;
    phase_9_emit_completion(runtime, event_log, device_id, &mut report)?;
    Ok(report)
}

enum ReplayOutcome {
    Replayed,
    Deferred,
    Failed,
}

fn upsert_with_hash_check(repo: &Path, index: &mut Index, op: &PendingIndexOp) -> ReplayOutcome {
    let Ok((memory, hash)) = read_memory_file(repo, &op.path)
        .map_err(|err| tracing::warn!(op_id = op.op_id.as_str(), "pending index read failed: {err}"))
    else {
        return ReplayOutcome::Deferred;
    };

    if op.expected_file_hash.as_ref().is_some_and(|expected| expected != &hash) {
        return ReplayOutcome::Deferred;
    }

    match index.upsert_memory_with_file_hash(&memory, false, Some(&hash)) {
        Ok(_) => ReplayOutcome::Replayed,
        Err(err) => {
            tracing::warn!(op_id = op.op_id.as_str(), "pending index upsert failed: {err}");
            ReplayOutcome::Failed
        }
    }
}

fn replay_encrypted_op(repo: &Path, index: &mut Index, op: &PendingEncryptedIndexOp) -> ReplayOutcome {
    let Some(path) = op.indexed_memory.path.as_ref() else {
        return ReplayOutcome::Failed;
    };
    let ciphertext_path = repo.join(path.as_path());
    let Ok(ciphertext) = std::fs::read(&ciphertext_path) else {
        return ReplayOutcome::Deferred;
    };
    let actual_hash = crate::markdown::hash_bytes(&ciphertext);
    if actual_hash != op.expected_ciphertext_hash {
        return ReplayOutcome::Deferred;
    }
    match index.upsert_memory_with_file_hash(&op.indexed_memory, op.metadata_only, Some(&actual_hash)) {
        Ok(_) => ReplayOutcome::Replayed,
        Err(err) => {
            tracing::warn!(op_id = op.op_id.as_str(), "encrypted index upsert failed: {err}");
            ReplayOutcome::Failed
        }
    }
}

/// Reindex only memories whose disk hash has drifted from the index row.
///
/// Returns the count of memories reindexed. Falls back to full reindex when
/// the index cannot answer hash queries (empty index, schema mismatch).
fn reindex_stale_memories(repo: &Path, index: &mut Index) -> Result<u32, Box<dyn std::error::Error>> {
    let mut count = 0u32;
    for entry in walkdir::WalkDir::new(repo).follow_links(false) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "md") {
            continue;
        }
        // Skip paths inside .git/
        if path.components().any(|c| c.as_os_str() == ".git") {
            continue;
        }
        let Ok(repo_relative) = path.strip_prefix(repo) else { continue };
        // Skip encrypted-tier ciphertext (raw bytes, not Markdown) to avoid blowing
        // up index consistency reindex. Deferred: confirm guard placement vs. walker.
        if repo_relative.components().next().is_some_and(|c| c.as_os_str() == "encrypted") {
            continue;
        }
        let Ok(relative_str) = repo_relative.to_str().ok_or("non-utf8 path") else { continue };
        let repo_path = RepoPath::new(relative_str);

        let (memory, disk_hash) = read_memory_file(repo, &repo_path)?;

        let needs_reindex = index.file_hash_for(&repo_path).map(|idx_hash| idx_hash != disk_hash).unwrap_or(true);

        if needs_reindex {
            index.upsert_memory_with_file_hash(&memory, false, Some(&disk_hash))?;
            count += 1;
        }
    }
    Ok(count)
}

fn scan_blocking_conflicts(repo: &Path) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut conflicts = Vec::new();
    for entry in walkdir::WalkDir::new(repo).follow_links(false) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "md") {
            continue;
        }
        if path.components().any(|c| c.as_os_str() == ".git") {
            continue;
        }
        let Ok(repo_relative) = path.strip_prefix(repo) else { continue };
        if repo_relative.components().next().is_some_and(|c| c.as_os_str() == "encrypted") {
            continue;
        }
        let Ok(relative_str) = repo_relative.to_str().ok_or("non-utf8 path") else { continue };
        let repo_path = RepoPath::new(relative_str);
        let (memory, _) = read_memory_file(repo, &repo_path)?;
        if memory.frontmatter.status == MemoryStatus::Quarantined
            || memory.frontmatter.trust_level == TrustLevel::Quarantined
        {
            conflicts.push(repo_path.as_str().to_string());
        }
    }
    conflicts.sort();
    conflicts.dedup();
    Ok(conflicts)
}

/// Delete or rewrite a pending queue file after replay.
///
/// - Empty remaining → delete the file (Q11: no `.compacted.jsonl` rename).
/// - Non-empty remaining → rewrite atomically with only the leftover ops.
/// - File doesn't exist → no-op.
fn finalize_pending_queue<T: Serialize>(path: &Path, remaining: &[T]) -> std::io::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    if remaining.is_empty() {
        std::fs::remove_file(path)?;
        fsync_parent(path)?;
        return Ok(());
    }
    rewrite_pending_queue(path, remaining)
}

/// Atomically rewrite a pending queue file with only the remaining ops.
fn rewrite_pending_queue<T: Serialize>(path: &Path, remaining: &[T]) -> std::io::Result<()> {
    let temp = path.with_extension("jsonl.tmp");
    if let Some(parent) = temp.parent() {
        std::fs::create_dir_all(parent)?;
    }
    {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&temp)?;
        for value in remaining {
            let value = serde_json::to_value(value).map_err(std::io::Error::other)?;
            file.write_all(encode_event_line(&value).map_err(std::io::Error::other)?.as_bytes())?;
        }
        file.sync_all()?;
    }
    std::fs::rename(&temp, path)?;
    fsync_parent(path)
}

fn append_framed_jsonl<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let value = serde_json::to_value(value).map_err(std::io::Error::other)?;
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    use std::io::Write;
    file.write_all(encode_event_line(&value).map_err(std::io::Error::other)?.as_bytes())?;
    file.sync_all()?;
    fsync_parent(path)
}

/// Read a pending-queue JSONL file strictly (no trailing-truncation grant).
///
/// Pending queues are durable repair markers. A malformed non-final frame
/// means a corrupt write that requires operator repair — not silent truncation.
/// (Trailing-truncation is only granted to the event log per spec §12.3 — R-RT-2.)
fn read_framed_jsonl_strict<T: serde::de::DeserializeOwned>(path: &Path) -> std::io::Result<Vec<T>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(path)?;
    let mut records = Vec::new();
    let mut valid_end = 0usize;
    let mut malformed_count = 0usize;
    let mut first_malformed_start: Option<usize> = None;

    for line in text.split_inclusive('\n') {
        if let Some(value) = decode_line(line) {
            records.push(value_to_record(value)?);
            valid_end += line.len();
        } else {
            malformed_count += 1;
            if first_malformed_start.is_none() {
                first_malformed_start = Some(valid_end);
            }
        }
    }

    if malformed_count == 0 {
        return Ok(records);
    }

    // Single malformed trailing line (unterminated crash write) → truncate and
    // continue. This is the only grace granted per R-RT-2 (pending queues).
    let is_single_trailing = malformed_count == 1
        && first_malformed_start == Some(valid_end)
        && valid_end < text.len()
        && !text[valid_end..].ends_with('\n');

    if is_single_trailing {
        let file = std::fs::OpenOptions::new().write(true).open(path)?;
        file.set_len(valid_end as u64)?;
        return Ok(records);
    }

    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!("non-final malformed pending repair frame in {}", path.display()),
    ))
}

fn value_to_record<T: serde::de::DeserializeOwned>(value: Value) -> std::io::Result<T> {
    serde_json::from_value(value).map_err(std::io::Error::other)
}

fn fsync_parent(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}
