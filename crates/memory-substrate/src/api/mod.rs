#![allow(unknown_lints, file_too_long)]
//! Public Stream A API.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex, MutexGuard,
};

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use chrono::{DateTime, Duration, Utc};
use serde::Serialize;

use crate::error::{
    OpenError, ReadError, SubstrateError, SubstrateResult, ValidationError, VectorError, WriteFailure, WriteFailureKind,
};
use crate::events::{
    append_event, append_event_best_effort, append_events, append_events_best_effort, decode_line,
    ensure_event_sequence_state, read_events, reserve_event_sequence, reserve_event_sequences,
    sync_event_sequence_state, Event, EventKind,
};
use crate::frontmatter::{validate_frontmatter, validate_lifecycle_transition};
use crate::git;
use crate::ids::next_memory_id;
use crate::index::{open_index, Index};
use crate::markdown::{atomic_write, probe_durability, read_memory_file};
use crate::model::*;
use crate::path_validation::is_noncanonical_stream_f_repo_path;
use crate::runtime::reconcile::{
    enqueue_pending_event, reconcile_startup_pre_index_report, replay_pending_repairs_into_report,
    write_startup_marker, PendingEncryptedIndexOp, PendingEventOp, PendingIndexKind, PendingIndexOp, ReconcileReport,
};
use crate::runtime::repair_cascade::{CascadeFailureKinds, IndexRepairOp, RepairCascade};
use crate::tree::{has_substrate_marker, validate_tree, TreeValidationMode};
use crate::watcher::{watch_root_with_suppression, SuppressionLedger, WatchSubscription};

mod events;
mod fragments;
mod lifecycle;
mod query;
mod read;
mod write;

/// Stream A substrate handle.
#[derive(Clone)]
pub struct Substrate {
    roots: Roots,
    device_id: String,
    durability: DurabilityTier,
    index: Arc<Mutex<Index>>,
    event_log: PathBuf,
    best_effort_event_seq: Arc<AtomicU64>,
    suppression: Arc<Mutex<SuppressionLedger>>,
    startup_reconcile_report: Arc<ReconcileReport>,
}

impl Substrate {
    /// Roots backing this substrate handle.
    pub fn roots(&self) -> &Roots {
        &self.roots
    }

    /// Full startup reconciliation report captured when this handle was opened.
    pub fn startup_reconcile_report(&self) -> &ReconcileReport {
        &self.startup_reconcile_report
    }

    /// Run a read-only operation against the substrate's live, already-initialized
    /// derived index.
    ///
    /// Lets read-only recall consumers (e.g. strength hydration on the blocking
    /// pool) reuse this connection instead of calling `open_index` per request —
    /// which would re-run the full `SCHEMA_SQL` DDL batch, the migration version
    /// probes, and the WAL pragmas on every recall. SQLite `PRAGMA query_only` is
    /// enabled around the closure and restored afterward, so accidental writes
    /// through the live connection fail instead of mutating derived state outside
    /// the substrate write/reconcile paths.
    pub fn with_index<T>(&self, operation: impl FnOnce(&Index) -> SubstrateResult<T>) -> SubstrateResult<T> {
        // Catch a panic from `operation` so the mutex guard is released by normal
        // scope exit (with `std::thread::panicking()` false) rather than during the
        // unwind. A guard dropped mid-unwind poisons the shared index mutex, turning
        // one panicked recall/reality-check request into a daemon-wide outage — every
        // later `lock_index` would observe the poison. We resume the unwind only after
        // the guard is cleanly released, preserving the recall hot path's own
        // `catch_unwind` soft-failure contract (spec §3) while keeping the mutex sound.
        let outcome = {
            let index = lock_index(&self.index);
            let _query_only = QueryOnlyGuard::enable(index.connection())?;
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| operation(&index)))
        };
        match outcome {
            Ok(result) => result,
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }

    /// Git preflight.
    pub async fn git_preflight(&self, merge_driver_binary: PathBuf) -> Result<(), crate::error::GitError> {
        git::git_preflight(&self.roots.repo, &merge_driver_binary)
    }

    /// Inspect fetch without merge.
    pub async fn fetch_inspect(&self) -> Result<String, crate::error::GitError> {
        git::fetch_inspect(&self.roots.repo)
    }

    /// Auto commit.
    ///
    /// Deferred: return `CommitOutcome` so callers know whether a commit was made.
    pub async fn auto_commit(&self) -> Result<(), crate::error::GitError> {
        git::auto_commit(&self.roots.repo, "Stream A auto-commit\n\nStream-A: true").map(|_| ())
    }

    /// Push.
    pub async fn push(&self) -> Result<(), crate::error::GitError> {
        git::push(&self.roots.repo)
    }

    /// Resolved durability tier.
    pub fn durability_tier(&self) -> DurabilityTier {
        self.durability
    }

    /// Synchronous watch subscription setup.
    pub fn watch(&self) -> Result<WatchSubscription, crate::error::WatchError> {
        watch_root_with_suppression(&self.roots.repo, Some(Arc::clone(&self.suppression)))
    }
}

/// Lock the derived index, recovering a poisoned mutex instead of failing.
///
/// The guarded SQLite connection holds no Rust-side invariant a panic can tear:
/// rusqlite `Statement`/`Transaction` are RAII types that finalize and roll back
/// on unwind, so a connection observed after a panicked holder is still usable.
/// Recovering from poison therefore keeps a single panicked index operation from
/// bricking every later read/write daemon-wide. `with_index` additionally catches
/// closure panics so the read path never poisons in the first place; this recovery
/// is the backstop for any future holder that does not.
fn lock_index(index: &Mutex<Index>) -> MutexGuard<'_, Index> {
    index.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

struct QueryOnlyGuard<'connection> {
    connection: &'connection rusqlite::Connection,
    previous: bool,
}

impl<'connection> QueryOnlyGuard<'connection> {
    fn enable(connection: &'connection rusqlite::Connection) -> SubstrateResult<Self> {
        let previous = query_only_enabled(connection)?;
        connection.pragma_update(None, "query_only", true)?;
        Ok(Self { connection, previous })
    }
}

impl Drop for QueryOnlyGuard<'_> {
    fn drop(&mut self) {
        let _ = self.connection.pragma_update(None, "query_only", self.previous);
    }
}

fn query_only_enabled(connection: &rusqlite::Connection) -> rusqlite::Result<bool> {
    connection.query_row("PRAGMA query_only", [], |row| row.get::<_, i64>(0)).map(|value| value != 0)
}

fn read_all_event_logs_from_repo(repo: &std::path::Path) -> std::io::Result<Vec<Event>> {
    let events_dir = repo.join("events");
    if !events_dir.exists() {
        return Ok(Vec::new());
    }
    let mut paths = std::fs::read_dir(&events_dir)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    paths.sort();

    let mut events = Vec::new();
    for path in paths {
        if path.extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
            events.extend(read_events(&path)?);
        }
    }
    events.sort_by(|left, right| {
        left.device
            .as_str()
            .cmp(right.device.as_str())
            .then_with(|| left.seq.cmp(&right.seq))
            .then_with(|| left.id.as_str().cmp(right.id.as_str()))
    });
    Ok(events)
}

fn copy_io_error(err: &std::io::Error) -> std::io::Error {
    std::io::Error::new(err.kind(), err.to_string())
}

fn best_effort_event_seq_start(event_log: &std::path::Path, device: &DeviceId) -> u64 {
    latest_event_seq_for_device(event_log, device).ok().flatten().map_or(1, |seq| seq.saturating_add(1))
}

fn latest_event_seq_for_device(event_log: &std::path::Path, device: &DeviceId) -> std::io::Result<Option<u64>> {
    if !event_log.exists() {
        return Ok(None);
    }

    const TAIL_CHUNK_SIZE: u64 = 8192;
    let mut file = std::fs::File::open(event_log)?;
    let mut position = file.seek(SeekFrom::End(0))?;
    let mut suffix = Vec::new();

    while position > 0 {
        let read_len = position.min(TAIL_CHUNK_SIZE);
        position -= read_len;
        file.seek(SeekFrom::Start(position))?;

        let mut chunk = vec![0; read_len as usize];
        file.read_exact(&mut chunk)?;
        chunk.extend_from_slice(&suffix);

        let mut search_end = chunk.len();
        while let Some(newline_index) = chunk[..search_end].iter().rposition(|byte| *byte == b'\n') {
            let line = &chunk[newline_index + 1..search_end];
            if let Some(seq) = event_seq_from_line_for_device(line, device) {
                return Ok(Some(seq));
            }
            search_end = newline_index;
        }
        suffix = chunk[..search_end].to_vec();
    }

    Ok(event_seq_from_line_for_device(&suffix, device))
}

fn event_seq_from_line_for_device(line: &[u8], device: &DeviceId) -> Option<u64> {
    if line.is_empty() {
        return None;
    }
    let line = std::str::from_utf8(line).ok()?.trim_end_matches('\r');
    let value = decode_line(line)?;
    let event = serde_json::from_value::<Event>(value).ok()?;
    (&event.device == device).then_some(event.seq)
}

fn committed_lifecycle_failure(failure: WriteFailure, committed_outcome: &WriteOutcome) -> WriteFailure {
    if failure.outcome.committed {
        failure
    } else {
        let mut outcome = committed_outcome.clone();
        outcome.repair_required.get_or_insert(RepairRequired::FullStartupScan);
        WriteFailure { outcome, kind: failure.kind }
    }
}

fn lifecycle_updated_at(frontmatter: &Frontmatter) -> chrono::DateTime<Utc> {
    Utc::now().max(frontmatter.created_at)
}

fn validate_substrate_fragment_append(request: &SubstrateFragmentAppendRequest) -> Result<(), String> {
    if request.scope.trim().is_empty() {
        return Err("substrate fragment scope is required".to_string());
    }
    if request.entities.len() > 32 {
        return Err("substrate fragment entities exceeds 32 entries".to_string());
    }
    for entity in &request.entities {
        if entity.len() > 128 {
            return Err(format!("substrate fragment entity exceeds 128 bytes: {entity}"));
        }
    }
    match (&request.payload, request.classification) {
        (SubstrateFragmentPayload::Plaintext { text }, ClassificationOutcome::Trusted) if text.trim().is_empty() => {
            Err("plaintext substrate fragment text is required".to_string())
        }
        (SubstrateFragmentPayload::Plaintext { .. }, ClassificationOutcome::Trusted) => Ok(()),
        (SubstrateFragmentPayload::Encrypted { encryption, descriptor }, ClassificationOutcome::RequiresEncryption) => {
            if encryption.recipient.trim().is_empty() || encryption.ciphertext_b64.trim().is_empty() {
                return Err("encrypted substrate fragment requires recipient and ciphertext_b64".to_string());
            }
            if descriptor.summary_safe.trim().is_empty() {
                return Err("encrypted substrate fragment requires descriptor.summary_safe".to_string());
            }
            Ok(())
        }
        (_, ClassificationOutcome::Secret) => Err("secret substrate fragments are refused".to_string()),
        (SubstrateFragmentPayload::Plaintext { .. }, ClassificationOutcome::RequiresEncryption) => {
            Err("requires_encryption classification must use encrypted substrate payload".to_string())
        }
        (SubstrateFragmentPayload::Encrypted { .. }, ClassificationOutcome::Trusted) => {
            Err("trusted classification must use plaintext substrate payload".to_string())
        }
    }
}

struct JsonlWriteTarget<'a> {
    repo: &'a std::path::Path,
    path: &'a RepoPath,
    operation_id: &'a OperationId,
    durability: DurabilityTier,
}

impl<'a> JsonlWriteTarget<'a> {
    fn new(
        repo: &'a std::path::Path,
        path: &'a RepoPath,
        operation_id: &'a OperationId,
        durability: DurabilityTier,
    ) -> Self {
        Self { repo, path, operation_id, durability }
    }
}

fn substrate_fragment_path(request: &SubstrateFragmentAppendRequest, device_id: &str) -> Result<RepoPath, String> {
    let prefix = match &request.payload {
        SubstrateFragmentPayload::Plaintext { .. } => "substrate",
        SubstrateFragmentPayload::Encrypted { .. } => "encrypted/substrate",
    };
    RepoPath::try_new(format!("{prefix}/{}/{}.jsonl", device_id, request.at.format("%Y-%m-%d")))
}

fn append_jsonl_record<T: Serialize>(target: JsonlWriteTarget<'_>, record: &T) -> std::io::Result<()> {
    ensure_write_parent_contained(target.repo, target.path).map_err(std::io::Error::other)?;
    let final_path = target.repo.join(target.path.as_path());
    let parent = final_path.parent().ok_or_else(|| std::io::Error::other("missing parent"))?;
    std::fs::create_dir_all(parent)?;
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(&final_path)?;
    serde_json::to_writer(&mut file, record).map_err(std::io::Error::other)?;
    file.write_all(b"\n")?;
    if matches!(target.durability, DurabilityTier::Full) {
        file.sync_all()?;
        std::fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

fn write_jsonl_records<T: Serialize>(target: JsonlWriteTarget<'_>, records: &[T]) -> std::io::Result<()> {
    ensure_write_parent_contained(target.repo, target.path).map_err(std::io::Error::other)?;
    let final_path = target.repo.join(target.path.as_path());
    let parent = final_path.parent().ok_or_else(|| std::io::Error::other("missing parent"))?;
    std::fs::create_dir_all(parent)?;
    let file_name = final_path.file_name().and_then(|name| name.to_str()).unwrap_or("substrate.jsonl");
    let temp_path = parent.join(format!(".{file_name}.{}.tmp", target.operation_id.as_str()));
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&temp_path)?;
    for record in records {
        serde_json::to_writer(&mut file, record).map_err(std::io::Error::other)?;
        file.write_all(b"\n")?;
    }
    if matches!(target.durability, DurabilityTier::Full) {
        file.sync_all()?;
    }
    std::fs::rename(&temp_path, &final_path)?;
    if matches!(target.durability, DurabilityTier::Full) {
        std::fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

fn read_substrate_records(path: &std::path::Path) -> std::io::Result<Vec<SubstrateFragmentRecord>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(path)?;
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).map_err(std::io::Error::other))
        .collect()
}

fn enforce_no_dream_prose_sources(memory: &Memory, outcome: WriteOutcome) -> Result<(), WriteFailure> {
    let source_ref = memory.frontmatter.source.reference.as_deref();
    let evidence_refs = memory.frontmatter.evidence.iter().map(|evidence| evidence.reference.as_str());

    if source_ref.into_iter().chain(evidence_refs).any(is_dream_prose_ref) {
        Err(WriteFailure { outcome, kind: WriteFailureKind::DreamProseAsSource })
    } else {
        Ok(())
    }
}

fn is_dream_prose_ref(reference: &str) -> bool {
    let without_file_prefix = reference.strip_prefix("file:").unwrap_or(reference);
    without_file_prefix
        .split_once('#')
        .map_or(without_file_prefix, |(path, _fragment)| path)
        .split('/')
        .collect::<Vec<_>>()
        .windows(3)
        .any(|window| window[0] == "dreams" && matches!(window[1], "journal" | "questions" | "cleanup"))
}

fn absolute_to_repo_path(repo: &std::path::Path, absolute: &std::path::Path) -> Result<RepoPath, String> {
    let relative = absolute.strip_prefix(repo).map_err(|err| err.to_string())?;
    RepoPath::try_new(relative.to_string_lossy().replace('\\', "/"))
}

fn new_substrate_fragment_id() -> String {
    format!("sub_{}", ulid::Ulid::new())
}

struct BinaryWrite<'a> {
    repo: &'a std::path::Path,
    path: &'a RepoPath,
    bytes: &'a [u8],
    operation_id: &'a OperationId,
    durability: DurabilityTier,
    suppression: Option<&'a Arc<Mutex<SuppressionLedger>>>,
}

fn atomic_write_bytes(args: BinaryWrite<'_>) -> std::io::Result<()> {
    let final_path = args.repo.join(args.path.as_path());
    ensure_write_parent_contained(args.repo, args.path).map_err(std::io::Error::other)?;
    if final_path.exists() {
        return Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, "encrypted target already exists"));
    }
    let parent = final_path.parent().ok_or_else(|| std::io::Error::other("missing parent"))?;
    std::fs::create_dir_all(parent)?;
    let file_name = final_path.file_name().and_then(|name| name.to_str()).unwrap_or("encrypted.bin");
    let temp_path = parent.join(format!(".{file_name}.{}.tmp", args.operation_id.as_str()));
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&temp_path)?;
    use std::io::Write;
    file.write_all(args.bytes)?;
    file.sync_all()?;
    let final_hash = crate::markdown::hash_bytes(args.bytes);
    if let Some(suppression) = args.suppression {
        if let Ok(mut ledger) = suppression.lock() {
            ledger.insert_in_flight(args.path.clone(), args.operation_id.clone(), final_hash.clone());
        }
    }
    let write_result = (|| {
        std::fs::hard_link(&temp_path, &final_path)?;
        std::fs::remove_file(&temp_path)?;
        if matches!(args.durability, DurabilityTier::Full) {
            std::fs::File::open(parent)?.sync_all()?;
        }
        Ok(())
    })();
    match write_result {
        Ok(()) => {
            if let Some(suppression) = args.suppression {
                if let Ok(mut ledger) = suppression.lock() {
                    ledger.promote_committed(args.path.clone(), final_hash);
                }
            }
            Ok(())
        }
        Err(err) => {
            let _ = std::fs::remove_file(&temp_path);
            if let Some(suppression) = args.suppression {
                if let Ok(mut ledger) = suppression.lock() {
                    ledger.remove(args.path);
                }
            }
            Err(err)
        }
    }
}

/// Invariant audit context shared by every pre-disk refusal gate of a single write.
///
/// Built once per write so each gate stops re-spelling the id/path/classification/
/// operation-id quadruple (spec §8.7 step 6 `WriteRefused` audit trail).
struct RefusalAuditContext {
    id: MemoryId,
    path: Option<RepoPath>,
    classification: ClassificationOutcome,
    operation_id: OperationId,
}

/// Fully-committed write outcome: canonical file, index, and audit event all durable.
fn committed_indexed_recorded(operation_id: OperationId, durability: DurabilityTier) -> WriteOutcome {
    WriteOutcome {
        committed: true,
        indexed: true,
        event_recorded: true,
        durability,
        repair_required: None,
        operation_id,
    }
}

/// Committed + indexed, audit event deferred to the `PendingEvent` repair queue.
fn committed_pending_event(operation_id: OperationId, durability: DurabilityTier) -> WriteOutcome {
    WriteOutcome {
        committed: true,
        indexed: true,
        event_recorded: false,
        durability,
        repair_required: Some(RepairRequired::PendingEvent),
        operation_id,
    }
}

/// Committed + indexed, audit event unrecorded, carrying an explicit repair requirement.
fn committed_event_repair(
    operation_id: OperationId,
    durability: DurabilityTier,
    repair: RepairRequired,
) -> WriteOutcome {
    WriteOutcome {
        committed: true,
        indexed: true,
        event_recorded: false,
        durability,
        repair_required: Some(repair),
        operation_id,
    }
}

/// Default canonical repo path for a memory that carries no explicit `path`.
///
/// Single source for the `agent/patterns/<id>.md` layout fallback so the default
/// namespace lives in one place rather than smeared across the write paths.
fn default_memory_path(memory: &Memory) -> RepoPath {
    RepoPath::new(format!("agent/patterns/{}.md", memory.frontmatter.id.as_str()))
}

fn encrypted_ciphertext_path(memory: &Memory) -> Result<RepoPath, String> {
    let original = memory.path.clone().unwrap_or_else(|| default_memory_path(memory));
    if !original.is_safe_relative() {
        return Err(format!("invalid repo path: {}", original.as_str()));
    }
    let memory_prefix = ["me/", "projects/", "agent/", "dreams/"];
    if !memory_prefix.iter().any(|prefix| original.as_str().starts_with(prefix))
        || original.as_str().starts_with("encrypted/")
        || !crate::watcher::is_memory_path(original.as_path())
    {
        return Err(format!("encrypted writes require an original memory markdown path: {}", original.as_str()));
    }
    // Spec §5.1 / §8.4: ciphertext is stored under `encrypted/<original-relative-path>`,
    // preserving the `.md` extension. The body inside the file is base64/armor; the
    // file itself is still a Markdown file from the tree allow-list's perspective.
    let encrypted = PathBuf::from("encrypted").join(original.as_path());
    RepoPath::try_new(encrypted.to_string_lossy().replace('\\', "/"))
}

fn encrypted_metadata_path(memory: &Memory) -> Result<RepoPath, String> {
    let Some(path) = memory.path.clone() else {
        return Err(format!("encrypted memory {} is missing a repo path", memory.frontmatter.id.as_str()));
    };
    if !path.is_safe_relative() {
        return Err(format!("invalid repo path: {}", path.as_str()));
    }
    if !path.as_str().starts_with("encrypted/") {
        return Err(format!("encrypted metadata update cannot target plaintext path: {}", path.as_str()));
    }
    Ok(path)
}

fn encrypted_index_projection(stored_memory: &Memory) -> (Memory, bool) {
    let mut indexed_memory = stored_memory.clone();
    match stored_memory
        .frontmatter
        .extras
        .get("index_projection")
        .and_then(|projection| projection.get("safe_body"))
        .and_then(serde_json::Value::as_str)
    {
        Some(safe_body) => {
            indexed_memory.body = safe_body.to_owned();
            indexed_memory.frontmatter.retrieval_policy.index_body = true;
            (indexed_memory, false)
        }
        None => {
            indexed_memory.body.clear();
            (indexed_memory, true)
        }
    }
}

/// Load the device id from `local-device.yaml`.
///
/// Per Q4, `git::adopt_clone` is the sole authority for minting
/// `local-device.yaml`. Returns `DeviceIdentityMissing` when absent.
fn load_device_id(runtime: &std::path::Path) -> Result<String, OpenError> {
    let local = crate::config::load_local_device_config(runtime).map_err(OpenError::InvalidRoots)?;
    match local {
        Some(cfg) => Ok(cfg.device.id),
        None => Err(OpenError::DeviceIdentityMissing { repair: crate::error::RepairAction::AdoptClone }),
    }
}

fn new_operation_id() -> OperationId {
    OperationId::new(format!("op_{}", uuid::Uuid::new_v4()))
}

#[cfg(test)]
mod event_seq_tests {
    use chrono::Utc;
    use std::io::Write as _;

    use super::*;
    use crate::events::append_event;

    #[test]
    fn best_effort_event_seq_start_reads_valid_tail_without_full_log_recovery() {
        let temp = must(tempfile::tempdir(), "tempdir");
        let event_log = temp.path().join("events").join("dev_test.jsonl");
        let device = must(DeviceId::try_new("dev_test"), "device id");
        must(append_event(&event_log, &test_event(&device, 41)), "append first event");
        let mut file = must(std::fs::OpenOptions::new().append(true).open(&event_log), "open event log");
        must(file.write_all(b"{not-json}\n"), "append malformed middle line");
        must(append_event(&event_log, &test_event(&device, 42)), "append tail event");

        assert_eq!(best_effort_event_seq_start(&event_log, &device), 43);
    }

    fn must<T, E: std::fmt::Display>(result: Result<T, E>, context: &str) -> T {
        match result {
            Ok(value) => value,
            Err(err) => panic!("{context}: {err}"),
        }
    }

    fn test_event(device: &DeviceId, seq: u64) -> Event {
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
}

/// Full reindex backing the public `memoryd reindex` command: clear all
/// plaintext rows and rebuild from every Markdown file on disk.
///
/// Startup (`open_with_options`) does **not** call this; phase 6 still
/// reads+hashes plaintext files for stale detection, then
/// [`incremental_reindex_at_open`] runs only the remaining sweeps. Kept because
/// `reindex()` and the CLI need an unconditional clear+rebuild.
fn full_reindex_from_repo(repo: &std::path::Path, index: &mut Index) -> std::io::Result<usize> {
    let entries = collect_reindex_paths(repo, ReindexScope::All).map_err(std::io::Error::other)?;
    index.clear_plaintext_memory_index().map_err(|err| std::io::Error::other(err.to_string()))?;
    let count = entries.len();
    let has_supersession_edges = entries.iter().any(|entry| !entry.memory.frontmatter.supersedes.is_empty());
    // Per-row upsert (each its own transaction). A single bulk transaction here
    // is ~40% faster on cold reindex, but it measurably shifts the page-cache
    // state a subsequent point lookup sees (the `query_by_id` perf gate caught a
    // ~5µs p50 regression that survived a post-batch WAL checkpoint). Reindex is a
    // rare bulk op; the steady-state read path is the one with a latency gate, so
    // the per-row write cost is the right trade. The deferred supersession pass
    // below still re-adds any FK-guarded edge whose target landed later in the walk.
    for entry in &entries {
        index
            .upsert_memory_with_file_hash(&entry.memory, entry.metadata_only, Some(&entry.file_hash))
            .map_err(|err| std::io::Error::other(err.to_string()))?;
    }
    if has_supersession_edges {
        index.resync_supersession_edges().map_err(|err| std::io::Error::other(err.to_string()))?;
    }
    index
        .reconcile_active_embedding_jobs(crate::model::EmbeddingLaneEligibility::AllTiers)
        .map_err(|err| std::io::Error::other(err.to_string()))?;
    Ok(count)
}

/// Incremental open-time index reconciliation (spec §13.5.1 phase-6 companion).
///
/// Replaces the duplicate unconditional clear+rebuild pass at startup.
/// Plaintext freshness is left to phase 6 (`reindex_stale_memories`), which
/// still reads+hashes every plaintext `.md` and has already run by the time
/// `open` reaches here. This sweep covers the three things phase 6 does not:
///
/// 1. **Orphan-row cleanup** — drop index rows whose plaintext file no longer
///    stats (memory deleted/moved on disk). Phase 6 only visits files that
///    exist. Derived rows only; never canonical files.
/// 2. **Encrypted-tier indexing** — phase 6 skips `encrypted/`. Re-index only
///    encrypted files whose ciphertext hash drifted from the index row, using
///    the same `metadata_only` + `safe_body` projection as a full reindex.
/// 3. **Embedding-job reconciliation** — `reconcile_active_embedding_jobs` runs
///    last, exactly as the full reindex did.
fn incremental_reindex_at_open(repo: &std::path::Path, index: &mut Index) -> std::io::Result<usize> {
    // (1) Orphan sweep — O(n_index_rows) stat calls, not O(n) file reads.
    index.prune_orphaned_plaintext_rows(repo).map_err(|err| std::io::Error::other(err.to_string()))?;

    // (2) Encrypted-tier incremental reindex — hash-compare like phase 6.
    //
    // Two-pass to amortize: first filter the stale entries (cheap hash compare),
    // then upsert the whole stale set under a single transaction via
    // `batch_upsert_memories_with_file_hash` (one WAL commit cycle instead of N).
    // The deferred `resync_supersession_edges` pass below covers the FK-guarded
    // per-row supersession behavior, exactly as the per-row path required.
    let entries = collect_reindex_paths(repo, ReindexScope::EncryptedOnly).map_err(std::io::Error::other)?;
    let stale_entries: Vec<ReindexEntry> = entries
        .into_iter()
        .filter(|entry| {
            entry
                .memory
                .path
                .as_ref()
                .and_then(|path| index.file_hash_for(path))
                .map(|indexed| indexed != entry.file_hash)
                .unwrap_or(true)
        })
        .collect();
    let count = stale_entries.len();
    index
        .batch_upsert_memories_with_file_hash(
            stale_entries.iter().map(|entry| (&entry.memory, entry.metadata_only, Some(&entry.file_hash))),
        )
        .map_err(|err| std::io::Error::other(err.to_string()))?;

    // (3) Deferred supersession pass. Phase 6 (`reindex_stale_memories`) and the
    // encrypted-tier sweep above both FK-guard each per-memory supersession edge,
    // so a memory indexed before its `supersedes` target dropped that edge. By
    // the time this runs at open, every plaintext + encrypted `memories` row is
    // present, so re-derive and re-add any edge whose target is now indexed.
    index.resync_supersession_edges().map_err(|err| std::io::Error::other(err.to_string()))?;

    // (4) Embedding-job reconciliation — must keep running at open.
    index
        .reconcile_active_embedding_jobs(crate::model::EmbeddingLaneEligibility::AllTiers)
        .map_err(|err| std::io::Error::other(err.to_string()))?;
    Ok(count)
}

struct ReindexEntry {
    memory: Memory,
    metadata_only: bool,
    file_hash: Sha256,
}

/// Which repo tier `collect_reindex_paths` should gather.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ReindexScope {
    /// Every Markdown file (plaintext + encrypted projections).
    All,
    /// Only the `encrypted/` tier.
    EncryptedOnly,
}

fn collect_reindex_paths(repo: &std::path::Path, scope: ReindexScope) -> Result<Vec<ReindexEntry>, String> {
    let mut acc = Vec::new();
    for raw in crate::tree::relative_memory_paths(repo) {
        let rel = raw.to_string_lossy().replace('\\', "/");
        // Non-Stream-A `.md` files (runtime-dir artifacts when the runtime nests
        // inside the repo, stray docs) are not memories; skip rather than panic
        // via the validating constructor.
        let Ok(path) = RepoPath::try_new(rel.clone()) else { continue };
        if rel.starts_with("encrypted/") {
            match read_memory_file(repo, &path) {
                Ok((memory, hash)) => {
                    if memory.frontmatter.extras.contains_key("encryption") {
                        let mut indexed_memory = memory;
                        let metadata_only = if let Some(safe_body) = indexed_memory
                            .frontmatter
                            .extras
                            .get("index_projection")
                            .and_then(|projection| projection.get("safe_body"))
                            .and_then(|value| value.as_str())
                        {
                            indexed_memory.body = safe_body.to_string();
                            indexed_memory.frontmatter.retrieval_policy.index_body = true;
                            false
                        } else {
                            true
                        };
                        acc.push(ReindexEntry { memory: indexed_memory, metadata_only, file_hash: hash });
                    } else {
                        return Err(format!(
                            "plaintext markdown under encrypted namespace requires operator repair: {}",
                            path.as_str()
                        ));
                    }
                }
                Err(_) => continue, // legacy raw ciphertext: not a Markdown file; skip from plaintext reindex
            }
        } else if scope == ReindexScope::All {
            acc.push(
                read_memory_file(repo, &path)
                    .map(|(memory, hash)| ReindexEntry { memory, metadata_only: false, file_hash: hash })
                    .map_err(|err| err.to_string())?,
            );
        }
    }
    Ok(acc)
}

/// Write a minimal `config.yaml` if none exists yet.
///
/// Seeds the synthetic triple for development/test environments. Production
/// operators replace `config.yaml` with an operator-authored file.
/// Deferred: `InitOptions` should carry an explicit `active_embedding` field.
fn write_initial_config_if_absent(repo: &std::path::Path) -> Result<(), OpenError> {
    let config_path = repo.join("config.yaml");
    if config_path.exists() {
        return Ok(());
    }
    let content =
        "schema_version: 1\nactive_embedding:\n  provider: synthetic\n  model_ref: stream-a-test\n  dimension: 32\n";
    std::fs::write(&config_path, content)?;
    Ok(())
}

fn ensure_write_parent_contained(repo: &std::path::Path, path: &RepoPath) -> Result<(), String> {
    let canonical_repo = repo.canonicalize().map_err(|err| err.to_string())?;
    let mut current = repo.to_path_buf();
    let mut components = path.as_path().components().peekable();
    while let Some(component) = components.next() {
        if components.peek().is_none() {
            break;
        }
        current.push(component.as_os_str());
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(format!("write parent contains symlink: {}", path.as_str()));
            }
            Ok(metadata) if metadata.is_dir() => {
                let canonical = current.canonicalize().map_err(|err| err.to_string())?;
                if !canonical.starts_with(&canonical_repo) {
                    return Err(format!("write parent resolves outside repository: {}", path.as_str()));
                }
            }
            Ok(_) => return Err(format!("write parent is not a directory: {}", path.as_str())),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => break,
            Err(err) => return Err(err.to_string()),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn committed_lifecycle_failure_marks_stale_old_mutation_as_repair_required() {
        let operation_id = OperationId::new("op_supersede_replacement_committed");
        let replacement_outcome = WriteOutcome {
            committed: true,
            indexed: true,
            event_recorded: true,
            durability: DurabilityTier::BestEffort,
            repair_required: None,
            operation_id: operation_id.clone(),
        };
        let stale_old_mutation = WriteFailure {
            outcome: WriteOutcome::not_committed(operation_id.clone(), DurabilityTier::BestEffort),
            kind: WriteFailureKind::StaleBase,
        };

        let failure = committed_lifecycle_failure(stale_old_mutation, &replacement_outcome);

        assert_eq!(failure.kind, WriteFailureKind::StaleBase);
        assert!(failure.outcome.committed);
        assert_eq!(failure.outcome.repair_required, Some(RepairRequired::FullStartupScan));
        assert_eq!(failure.outcome.operation_id, operation_id);
    }
}
