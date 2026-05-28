//! Import state file: `$MEMORUM_REPO/.memorum/import-state.json`.
//!
//! Per the plan (Goal section), the state file is a **performance optimization**,
//! not the load-bearing correctness mechanism for idempotency. The daemon's
//! duplicate-detection in `governance::contradiction` already prevents
//! double-writes; the state file just lets the importer skip the parse +
//! socket round-trip for sources it has already confirmed-imported.
//!
//! Durability strategy (plan-locked):
//!
//! - Per-record write: atomic tmp + rename. No parent-dir fsync per record
//!   (wasted I/O — daemon dedup is the safety net).
//! - End-of-import canonical save: tmp + rename plus a single parent-dir fsync.
//! - Concurrent invocations are gated by `flock` on a sibling lock file
//!   `<state-file>.lock` with a 5s timeout; the owning pid is written to
//!   `<dir>/import.pid` so the operator can see who holds the lock.

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use fs2::FileExt;
use serde::{Deserialize, Serialize};

use super::{ImportError, ImportResult};

/// State file schema version. Bumped on incompatible shape changes; older state
/// files trigger a `CorruptState` error and get rotated aside on load.
pub const SCHEMA_VERSION: u32 = 1;

/// Default lock-acquisition timeout. 5 seconds matches the plan's contract for
/// `AnotherImportInProgress` errors.
const LOCK_TIMEOUT: Duration = Duration::from_secs(5);

/// Per-source idempotency record. The `source_key` is harness-relative so the
/// state file stays portable across machines with different home directories.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportRecord {
    pub memory_id: String,
    pub content_hash: String,
    pub imported_at: DateTime<Utc>,
    pub harness: String,
    pub source_path_at_import: PathBuf,
    #[serde(default)]
    pub supersession_chain: Vec<SupersededRecord>,
}

/// Prior version of an imported memory, preserved when content-hash supersession
/// promotes a new memory in its place. Most-recent entry at the end of the chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupersededRecord {
    pub memory_id: String,
    pub content_hash: String,
    pub imported_at: DateTime<Utc>,
}

/// In-memory representation of the import state file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportState {
    pub schema_version: u32,
    #[serde(default)]
    pub imports: BTreeMap<String, ImportRecord>,
}

impl Default for ImportState {
    fn default() -> Self {
        Self { schema_version: SCHEMA_VERSION, imports: BTreeMap::new() }
    }
}

impl ImportState {
    /// Load the state file from disk. Missing file → empty state. Corrupt JSON →
    /// rotate aside to `<path>.corrupt-<unix-ts>` and return `CorruptState` so
    /// the operator can see what happened; the caller can rerun on the now-empty
    /// directory and rely on the daemon's duplicate-detection to re-establish
    /// idempotency.
    pub fn load(path: &Path) -> ImportResult<Self> {
        let raw = match std::fs::read_to_string(path) {
            Ok(value) => value,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => return Err(ImportError::io(path, error)),
        };
        match serde_json::from_str::<Self>(&raw) {
            Ok(state) if state.schema_version == SCHEMA_VERSION => Ok(state),
            Ok(state) => {
                let rotated = rotate_corrupt_state_file(path);
                Err(ImportError::CorruptState {
                    path: rotated,
                    reason: format!("schema_version {} unsupported (expected {SCHEMA_VERSION})", state.schema_version),
                })
            }
            Err(error) => {
                let rotated = rotate_corrupt_state_file(path);
                Err(ImportError::CorruptState { path: rotated, reason: error.to_string() })
            }
        }
    }

    /// Atomic per-record save: write to `<path>.tmp` then rename over `<path>`.
    /// Does not fsync the parent directory; that's reserved for `save_canonical`
    /// at end of import.
    pub fn save_atomic(&self, path: &Path) -> ImportResult<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| ImportError::io(parent, error))?;
        }
        let tmp_path = path.with_extension("json.tmp");
        let body = serde_json::to_vec_pretty(self)?;
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp_path)
            .map_err(|error| ImportError::io(&tmp_path, error))?;
        file.write_all(&body).map_err(|error| ImportError::io(&tmp_path, error))?;
        file.sync_data().map_err(|error| ImportError::io(&tmp_path, error))?;
        drop(file);
        std::fs::rename(&tmp_path, path).map_err(|error| ImportError::io(path, error))?;
        Ok(())
    }

    /// End-of-import canonical save. Same atomic rename as `save_atomic` plus a
    /// best-effort parent-dir fsync so the rename survives a power-loss after a
    /// long import. Best-effort because some filesystems don't support fsync on
    /// directories; the rename is already crash-consistent under POSIX.
    pub fn save_canonical(&self, path: &Path) -> ImportResult<()> {
        self.save_atomic(path)?;
        if let Some(parent) = path.parent() {
            if let Ok(dir) = File::open(parent) {
                // fsync on a directory may return ENOTSUP on some filesystems; we
                // tolerate that because the atomic rename already provides
                // crash-consistency in those cases.
                let _ = dir.sync_all();
            }
        }
        Ok(())
    }
}

fn rotate_corrupt_state_file(path: &Path) -> PathBuf {
    let now_ts = chrono::Utc::now().timestamp();
    let rotated = path.with_extension(format!("json.corrupt-{now_ts}"));
    let _ = std::fs::rename(path, &rotated);
    rotated
}

/// `flock`-based mutual exclusion across importer invocations. While the guard
/// is alive, `<path>.lock` holds an exclusive lock and `<dir>/import.pid` records
/// the holding pid. Dropping the guard releases the lock and cleans up the pid
/// file.
#[derive(Debug)]
pub struct ImportLockGuard {
    file: File,
    lock_path: PathBuf,
    pid_path: PathBuf,
}

impl ImportLockGuard {
    /// Acquire the lock with the default 5s timeout. Errors with
    /// `AnotherImportInProgress { pid }` when the lock can't be acquired.
    pub fn acquire(state_path: &Path) -> ImportResult<Self> {
        Self::acquire_with_timeout(state_path, LOCK_TIMEOUT)
    }

    /// Acquire with a custom timeout (used by tests).
    pub fn acquire_with_timeout(state_path: &Path, timeout: Duration) -> ImportResult<Self> {
        let parent = state_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        std::fs::create_dir_all(&parent).map_err(|error| ImportError::io(&parent, error))?;
        let lock_path = state_path.with_extension("json.lock");
        let pid_path = parent.join("import.pid");

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|error| ImportError::io(&lock_path, error))?;

        let deadline = Instant::now() + timeout;
        loop {
            match file.try_lock_exclusive() {
                Ok(()) => break,
                Err(_) => {
                    if Instant::now() >= deadline {
                        let pid = read_lock_holder_pid(&pid_path);
                        return Err(ImportError::AnotherImportInProgress { pid, lock_path });
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        }

        let pid = std::process::id();
        std::fs::write(&pid_path, pid.to_string()).map_err(|error| ImportError::io(&pid_path, error))?;

        Ok(Self { file, lock_path, pid_path })
    }
}

impl Drop for ImportLockGuard {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
        let _ = std::fs::remove_file(&self.pid_path);
        // The lock file itself stays — `flock` semantics tolerate a persistent
        // file and removing it would race against a concurrent acquirer.
        let _ = &self.lock_path;
    }
}

fn read_lock_holder_pid(pid_path: &Path) -> u32 {
    let mut raw = String::new();
    let Ok(mut file) = File::open(pid_path) else {
        return 0;
    };
    if file.read_to_string(&mut raw).is_err() {
        return 0;
    }
    raw.trim().parse().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn sample_record() -> ImportRecord {
        ImportRecord {
            memory_id: "mem_20260527_a1b2c3d4e5f60718_000001".to_string(),
            content_hash: "sha256:abc".to_string(),
            imported_at: Utc.with_ymd_and_hms(2026, 5, 27, 22, 33, 0).unwrap(),
            harness: "claude-code".to_string(),
            source_path_at_import: PathBuf::from("/Users/u/.claude/projects/x/memory/y.md"),
            supersession_chain: Vec::new(),
        }
    }

    #[test]
    fn missing_state_file_loads_as_empty() {
        let tmp = tempfile::tempdir().expect("tmp");
        let path = tmp.path().join("import-state.json");
        let state = ImportState::load(&path).expect("load ok");
        assert_eq!(state.schema_version, SCHEMA_VERSION);
        assert!(state.imports.is_empty());
    }

    #[test]
    fn save_atomic_round_trips_through_load() {
        let tmp = tempfile::tempdir().expect("tmp");
        let path = tmp.path().join("import-state.json");
        let mut state = ImportState::default();
        state.imports.insert("claude:projects/x/memory/y.md".to_string(), sample_record());
        state.save_atomic(&path).expect("save ok");
        let loaded = ImportState::load(&path).expect("load ok");
        assert_eq!(loaded, state);
    }

    #[test]
    fn save_canonical_writes_same_payload_as_save_atomic() {
        let tmp = tempfile::tempdir().expect("tmp");
        let path = tmp.path().join("import-state.json");
        let mut state = ImportState::default();
        state.imports.insert("k".to_string(), sample_record());
        state.save_canonical(&path).expect("canonical save ok");
        let loaded = ImportState::load(&path).expect("load ok");
        assert_eq!(loaded, state);
    }

    #[test]
    fn corrupt_state_file_is_rotated_aside_and_load_errors() {
        let tmp = tempfile::tempdir().expect("tmp");
        let path = tmp.path().join("import-state.json");
        std::fs::write(&path, "{not valid json").expect("seed corrupt");
        let error = ImportState::load(&path).expect_err("corrupt load fails");
        match error {
            ImportError::CorruptState { path: rotated, .. } => {
                assert!(rotated.to_string_lossy().contains(".json.corrupt-"));
                assert!(rotated.exists(), "corrupt file is preserved at the rotated path");
                assert!(!path.exists(), "original path no longer holds the corrupt file");
            }
            other => panic!("expected CorruptState, got {other:?}"),
        }
    }

    #[test]
    fn schema_version_mismatch_is_treated_as_corrupt_and_rotated_aside() {
        let tmp = tempfile::tempdir().expect("tmp");
        let path = tmp.path().join("import-state.json");
        std::fs::write(&path, r#"{"schema_version":999,"imports":{}}"#).expect("seed mismatched");
        let error = ImportState::load(&path).expect_err("mismatch load fails");
        assert!(matches!(error, ImportError::CorruptState { .. }));
    }

    #[test]
    fn lock_guard_blocks_second_acquirer_until_timeout_then_returns_pid() {
        let tmp = tempfile::tempdir().expect("tmp");
        let path = tmp.path().join("import-state.json");
        let _guard = ImportLockGuard::acquire(&path).expect("first acquires");
        let start = Instant::now();
        let result = ImportLockGuard::acquire_with_timeout(&path, Duration::from_millis(250));
        let elapsed = start.elapsed();
        assert!(elapsed >= Duration::from_millis(200), "second acquisition waits for the timeout");
        match result {
            Err(ImportError::AnotherImportInProgress { pid, .. }) => {
                assert_eq!(pid, std::process::id(), "lock file records the holding pid");
            }
            other => panic!("expected AnotherImportInProgress, got {other:?}"),
        }
    }

    #[test]
    fn lock_guard_releases_on_drop_so_subsequent_acquirer_succeeds() {
        let tmp = tempfile::tempdir().expect("tmp");
        let path = tmp.path().join("import-state.json");
        {
            let _guard = ImportLockGuard::acquire(&path).expect("first acquires");
        }
        let _again = ImportLockGuard::acquire(&path).expect("second acquires after drop");
    }
}
