//! Same-directory atomic writes.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::error::{WriteFailure, WriteFailureKind};
use crate::frontmatter::{parse_document, serialize_document};
use crate::markdown::cas::hash_bytes;
use crate::model::{DurabilityTier, Memory, OperationId, RepoPath, Sha256, WriteMode, WriteOutcome};
use crate::watcher::SuppressionLedger;

/// Read a Markdown memory file.
pub fn read_memory_file(repo: &Path, path: &RepoPath) -> Result<(Memory, Sha256), crate::error::ReadError> {
    if !path.is_safe_relative() {
        return Err(crate::error::ReadError::Parse {
            path: path.clone(),
            message: "invalid repo-relative memory path".to_string(),
        });
    }
    let absolute = repo.join(path.as_path());
    let canonical_repo = repo.canonicalize()?;
    let canonical_path = absolute.canonicalize()?;
    if !canonical_path.starts_with(&canonical_repo) {
        return Err(crate::error::ReadError::Parse {
            path: path.clone(),
            message: "memory path resolves outside repository".to_string(),
        });
    }
    let bytes = fs::read(&absolute)?;
    let text = String::from_utf8(bytes.clone())
        .map_err(|err| crate::error::ReadError::Parse { path: path.clone(), message: err.to_string() })?;
    let parsed = parse_document(&text, Some(path.clone())).map_err(crate::error::ReadError::Validation)?;
    Ok((parsed.memory, hash_bytes(&bytes)))
}

/// Arguments for an atomic write.
pub struct AtomicWrite<'a> {
    /// Repository root.
    pub repo: &'a Path,
    /// Memory to write.
    pub memory: &'a Memory,
    /// Optional expected hash.
    pub expected_base_hash: Option<&'a Sha256>,
    /// Write mode.
    pub mode: WriteMode,
    /// Operation id.
    pub operation_id: &'a OperationId,
    /// Durability tier.
    pub durability: DurabilityTier,
    /// Optional watcher self-event suppression ledger.
    pub suppression: Option<&'a Arc<Mutex<SuppressionLedger>>>,
}

/// Atomically serialize and write a memory file.
pub fn atomic_write(args: AtomicWrite<'_>) -> Result<Sha256, WriteFailure> {
    let relative = args.memory.path.clone().unwrap_or_else(|| default_path(args.memory));
    let final_path = args.repo.join(relative.as_path());
    let outcome = WriteOutcome::not_committed(args.operation_id.clone(), args.durability);
    if !relative.is_safe_relative() {
        return Err(WriteFailure {
            outcome,
            kind: WriteFailureKind::Validation(format!("invalid repo path: {}", relative.as_str())),
        });
    }
    if relative.as_str().starts_with("encrypted/") {
        return Err(WriteFailure {
            outcome,
            kind: WriteFailureKind::Validation(format!(
                "plaintext writes cannot target encrypted namespace: {}",
                relative.as_str()
            )),
        });
    }
    ensure_write_parent_contained(args.repo, &relative)
        .map_err(|err| WriteFailure { outcome: outcome.clone(), kind: WriteFailureKind::Validation(err) })?;
    enforce_preconditions(&final_path, args.expected_base_hash, args.mode, outcome.clone())?;
    let contents = serialize_document(args.memory).map_err(|err| WriteFailure {
        outcome: outcome.clone(),
        kind: WriteFailureKind::Validation(err.to_string()),
    })?;
    let final_hash = hash_bytes(contents.as_bytes());
    if let Some(suppression) = args.suppression {
        let Ok(mut ledger) = suppression.lock() else {
            panic!("suppression ledger poisoned");
        };
        ledger.insert_in_flight(relative.clone(), args.operation_id.clone(), final_hash.clone());
    }
    let parent = final_path.parent().ok_or_else(|| WriteFailure {
        outcome: outcome.clone(),
        kind: WriteFailureKind::Io("missing parent".to_string()),
    })?;
    fs::create_dir_all(parent)
        .map_err(|err| WriteFailure { outcome: outcome.clone(), kind: WriteFailureKind::Io(err.to_string()) })?;
    let file_name = final_path.file_name().and_then(|name| name.to_str()).unwrap_or("memory.md");
    let temp_path = parent.join(format!(".{file_name}.{}.tmp", args.operation_id.as_str()));
    let write_result = (|| {
        write_temp_file(&temp_path, contents.as_bytes(), outcome.clone())?;
        fs::rename(&temp_path, &final_path)
            .map_err(|err| WriteFailure { outcome: outcome.clone(), kind: WriteFailureKind::Io(err.to_string()) })?;
        if matches!(args.durability, DurabilityTier::Full) {
            fsync_dir(parent).map_err(|err| WriteFailure { outcome, kind: WriteFailureKind::Io(err.to_string()) })?;
        }
        Ok(())
    })();
    match write_result {
        Ok(()) => {
            if let Some(suppression) = args.suppression {
                let Ok(mut ledger) = suppression.lock() else {
                    panic!("suppression ledger poisoned");
                };
                ledger.promote_committed(relative, final_hash.clone());
            }
        }
        Err(err) => {
            // On rename-then-fsync-dir failure the file is already at its final
            // path. Leave the in-flight suppression entry to expire via TTL
            // rather than removing it — removing it would cause the watcher to
            // re-ingest a real on-disk file with no suppression guard.
            // (spec §8.3 footnote / B-IO-8 fix)
            if let Some(suppression) = args.suppression {
                let _ = suppression.lock().ok(); // do not re-panic; original error takes precedence
            }
            return Err(err);
        }
    }
    Ok(final_hash)
}

/// Remove a file if present.
pub fn remove_file_if_exists(path: &Path) -> std::io::Result<()> {
    if path.exists() {
        fs::remove_file(path)
    } else {
        Ok(())
    }
}

fn default_path(memory: &Memory) -> RepoPath {
    RepoPath::new(format!("agent/patterns/{}.md", memory.frontmatter.id.as_str()))
}

fn ensure_write_parent_contained(repo: &Path, path: &RepoPath) -> Result<(), String> {
    let canonical_repo = repo.canonicalize().map_err(|err| err.to_string())?;
    let mut current = repo.to_path_buf();
    let mut components = path.as_path().components().peekable();
    while let Some(component) = components.next() {
        if components.peek().is_none() {
            break;
        }
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
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

fn enforce_preconditions(
    final_path: &Path,
    expected_base_hash: Option<&Sha256>,
    mode: WriteMode,
    outcome: WriteOutcome,
) -> Result<(), WriteFailure> {
    match mode {
        WriteMode::CreateNew if final_path.exists() => {
            Err(WriteFailure { outcome, kind: WriteFailureKind::AlreadyExists })
        }
        WriteMode::ReplaceExisting => {
            let bytes = fs::read(final_path).map_err(|err| WriteFailure {
                outcome: outcome.clone(),
                kind: WriteFailureKind::Io(err.to_string()),
            })?;
            let current = hash_bytes(&bytes);
            if expected_base_hash.is_some_and(|expected| expected != &current) {
                Err(WriteFailure { outcome, kind: WriteFailureKind::StaleBase })
            } else {
                Ok(())
            }
        }
        _ => Ok(()),
    }
}

fn write_temp_file(path: &Path, contents: &[u8], outcome: WriteOutcome) -> Result<(), WriteFailure> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|err| WriteFailure { outcome: outcome.clone(), kind: WriteFailureKind::Io(err.to_string()) })?;
    file.write_all(contents)
        .map_err(|err| WriteFailure { outcome: outcome.clone(), kind: WriteFailureKind::Io(err.to_string()) })?;
    file.sync_all().map_err(|err| WriteFailure { outcome, kind: WriteFailureKind::Io(err.to_string()) })
}

/// Fsync a directory entry so its parent journal captures the rename/truncation.
pub fn fsync_dir(path: &Path) -> std::io::Result<()> {
    File::open(path)?.sync_all()
}
