//! `memoryd export` subcommand implementation.
//!
//! Emits a portable JSON snapshot of a substrate's contents per
//! `feature-memoryd-export-v0.1.md`.  Semantically read-only against substrate
//! content; does not mutate memories, locks, event-log entries, or index rows
//! beyond what `Substrate::open`'s standard runtime initialization already does.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use memory_privacy::install_runtime_enforcement;
use memory_substrate::config::{load_config, load_local_device_config};
use memory_substrate::{
    AuxScope, MemoryContent, MemoryEnvelope, MemoryStatus, RecallIndexQuery, Roots, Scope, Substrate,
};
use serde::Serialize;
use tokio::sync::Semaphore;

const EXPORT_ENVELOPE_READ_CONCURRENCY: usize = 16;

/// Arguments for `memoryd export`.
///
/// Note: opening the substrate triggers standard runtime-initialization side
/// effects even though the export does not write memory content.  These include
/// runtime-directory creation, index-repair replay, and event-log mirror
/// rebuild.  Stop any running `memoryd serve` daemon before exporting against
/// the same `--repo` / `--runtime` pair.
///
/// W3: export is a complete backup snapshot, so merge-staged candidate rows
/// and superseded rows are intentionally included (they are gated out of
/// live recall surfaces, not removed from canonical storage).
#[derive(Debug, clap::Args)]
pub struct ExportArgs {
    /// Canonical memory repository root.
    #[arg(long, default_value = ".")]
    pub repo: PathBuf,
    /// Local per-device runtime root.
    #[arg(long, default_value = ".memoryd")]
    pub runtime: PathBuf,
    /// Write the export to this path atomically instead of stdout.
    #[arg(long)]
    pub out: Option<PathBuf>,
    /// Output format.  Only `json` is accepted in v0.1.
    #[arg(long, value_enum, default_value_t = ExportFormat::Json)]
    pub format: ExportFormat,
    /// Include only memories whose `updated_at >= <ISO8601>`.
    ///
    /// Accepts RFC3339 UTC (`2026-05-01T00:00:00Z` or `2026-05-01T00:00:00+00:00`).
    /// Bare dates are rejected with exit code 2.
    #[arg(long)]
    pub since: Option<String>,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum ExportFormat {
    Json,
}

impl std::fmt::Display for ExportFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Json => write!(f, "json"),
        }
    }
}

#[derive(Serialize)]
struct ExportEnvelope {
    schema_version: u32,
    exported_at: String,
    source_device_id: String,
    filters: ExportFilters,
    memory_count: usize,
    memories: Vec<ExportMemory>,
}

#[derive(Serialize)]
struct ExportFilters {
    since: Option<String>,
}

#[derive(Serialize)]
struct ExportMemory {
    id: String,
    scope: Scope,
    status: MemoryStatus,
    frontmatter: serde_json::Value,
    body: Option<String>,
    body_marker: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("{0}")]
    Argument(String),
    #[error("{0}")]
    Substrate(String),
    #[error("{0}")]
    Io(String),
}

impl ExportError {
    /// Process exit code corresponding to this error variant.
    ///
    /// Argument failures map to 2 (clap convention for argparse errors);
    /// substrate and IO failures map to 1.  Callers in `main.rs` consult this
    /// to set the process exit code rather than the export module calling
    /// `process::exit` itself.
    pub fn exit_code(&self) -> i32 {
        match self {
            ExportError::Argument(_) => 2,
            ExportError::Substrate(_) | ExportError::Io(_) => 1,
        }
    }
}

/// Run the export subcommand.
///
/// Returns `Ok(())` on success and `Err(ExportError)` on any failure.  The
/// caller (typically `main.rs`) decides the process exit code by consulting
/// [`ExportError::exit_code`] — this module never calls `process::exit`
/// itself.
pub async fn run_export(args: ExportArgs) -> Result<(), ExportError> {
    // --format is enforced at clap-parse time via ValueEnum; no runtime check needed.
    let since_dt = parse_since(args.since.as_deref())?;

    let loaded_config = load_config(&args.repo, &args.runtime, None)
        .map_err(|e| ExportError::Substrate(format!("config load failed: {e}")))?;
    let enforcement = loaded_config.privacy_enforcement();
    let _ = install_runtime_enforcement(enforcement);

    let source_device_id = read_device_id(&args.runtime)?;
    let roots = Roots::new(args.repo.clone(), args.runtime.clone());
    let substrate = Substrate::open(roots).await.map_err(|e| ExportError::Substrate(e.to_string()))?;
    let exported_at = format_rfc3339_millis(Utc::now());
    let mut memories = collect_memories(&substrate, since_dt).await?;
    memories.sort_by(|a, b| a.updated_at.cmp(&b.updated_at).then_with(|| a.id.cmp(&b.id)));

    let memory_count = memories.len();
    let envelope = ExportEnvelope {
        schema_version: 1,
        exported_at,
        source_device_id,
        filters: ExportFilters { since: args.since },
        memory_count,
        memories,
    };

    let mut output = Vec::new();
    serde_json::to_writer_pretty(&mut output, &envelope)
        .map_err(|e| ExportError::Io(format!("JSON serialization failed: {e}")))?;
    output.push(b'\n');
    let bytes_len = output.len();

    emit_output(args.out.as_deref(), &output)?;
    eprintln!("memory_count={memory_count} bytes={bytes_len}");

    Ok(())
}

/// Parse the `--since` string into a UTC timestamp.
///
/// Accepts both `Z` and `+00:00` offset forms per spec §5.
/// Rejects bare dates (`YYYY-MM-DD`) with exit code 2.
fn parse_since(raw: Option<&str>) -> Result<Option<DateTime<Utc>>, ExportError> {
    let raw = match raw {
        None => return Ok(None),
        Some(s) => s,
    };

    if chrono::NaiveDate::parse_from_str(raw, "%Y-%m-%d").is_ok() {
        return Err(ExportError::Argument(format!(
            "--since '{raw}' is a bare date; use RFC3339 form, e.g. {raw}T00:00:00Z"
        )));
    }

    let fixed = chrono::DateTime::parse_from_rfc3339(raw).map_err(|_| {
        ExportError::Argument(format!("--since '{raw}': parse failed; use RFC3339 UTC, e.g. 2026-05-01T00:00:00Z"))
    })?;
    if !(raw.ends_with('Z') || raw.ends_with("+00:00")) {
        return Err(ExportError::Argument(format!(
            "--since '{raw}' must use UTC (`Z` or `+00:00`); non-UTC offsets are not accepted"
        )));
    }

    Ok(Some(fixed.with_timezone(&Utc)))
}

fn read_device_id(runtime: &Path) -> Result<String, ExportError> {
    let id = load_local_device_config(runtime)
        .map_err(|e| ExportError::Substrate(format!("device config load failed: {e}")))?
        .ok_or_else(|| {
            ExportError::Substrate(
                "device config not found; run `memoryd serve --init` to initialize the runtime directory".to_string(),
            )
        })
        .map(|cfg| cfg.device.id)?;
    if id.is_empty() || id.trim() != id {
        return Err(ExportError::Substrate(
            "source_device_id in local-device.yaml must be non-empty and have no surrounding whitespace".to_string(),
        ));
    }
    Ok(id)
}

async fn collect_memories(
    substrate: &Substrate,
    since_dt: Option<DateTime<Utc>>,
) -> Result<Vec<ExportMemory>, ExportError> {
    let rows = substrate
        .query_recall_index_including_metadata_only(RecallIndexQuery {
            updated_since: since_dt,
            hydrate: AuxScope::None,
            source_identity: false,
            ..RecallIndexQuery::default()
        })
        .await
        .map_err(|e| ExportError::Substrate(format!("failed to query recall index: {e}")))?;

    let semaphore = Arc::new(Semaphore::new(EXPORT_ENVELOPE_READ_CONCURRENCY));
    let mut reads = tokio::task::JoinSet::new();
    for row in rows {
        let substrate = substrate.clone();
        let id = row.id.clone();
        let path = row.path.clone();
        let path_label = path.as_str().to_string();
        let semaphore = Arc::clone(&semaphore);
        reads.spawn(async move {
            let _permit = semaphore.acquire_owned().await.expect("export read semaphore is open");
            let envelope = tokio::task::spawn_blocking(move || substrate.read_path_envelope_blocking(&path)).await;
            (id, path_label, envelope)
        });
    }

    let mut memories = Vec::new();
    while let Some(joined) = reads.join_next().await {
        let (id, path, blocking_result) =
            joined.map_err(|e| ExportError::Substrate(format!("export read task failed: {e}")))?;
        let envelope = blocking_result
            .map_err(|e| ExportError::Substrate(format!("export read task failed for {id} at {path}: {e}")))?
            .map_err(|e| ExportError::Substrate(format!("failed to read memory envelope {id} at {path}: {e}")))?;
        if envelope.metadata.frontmatter.id != id {
            return Err(ExportError::Substrate(format!(
                "recall index row {id} at {path} resolved to envelope {}",
                envelope.metadata.frontmatter.id
            )));
        }
        if let Some(memory) = export_memory_from_envelope(envelope, since_dt)? {
            memories.push(memory);
        }
    }
    Ok(memories)
}

fn export_memory_from_envelope(
    envelope: MemoryEnvelope,
    since_dt: Option<DateTime<Utc>>,
) -> Result<Option<ExportMemory>, ExportError> {
    let MemoryEnvelope { metadata, content } = envelope;
    let fm = metadata.frontmatter;

    if let Some(since) = since_dt {
        if fm.updated_at < since {
            return Ok(None);
        }
    }

    let (body, body_marker) = export_body_fields(fm.status, content);
    let created_at = format_rfc3339_millis(fm.created_at);
    let updated_at = format_rfc3339_millis(fm.updated_at);
    let frontmatter_value =
        serde_json::to_value(&fm).map_err(|e| ExportError::Io(format!("frontmatter serialization failed: {e}")))?;

    Ok(Some(ExportMemory {
        id: fm.id.as_str().to_string(),
        scope: fm.scope,
        status: fm.status,
        frontmatter: frontmatter_value,
        body,
        body_marker,
        created_at,
        updated_at,
    }))
}

fn export_body_fields(status: MemoryStatus, content: MemoryContent) -> (Option<String>, Option<String>) {
    if status == MemoryStatus::Tombstoned {
        return (None, Some("tombstoned".to_string()));
    }

    match content {
        MemoryContent::Plaintext(text) => (Some(text), None),
        MemoryContent::Ciphertext { .. } => (None, Some("encrypted".to_string())),
        MemoryContent::MetadataOnly => (None, Some("metadata-only".to_string())),
    }
}

fn emit_output(out: Option<&Path>, content: &[u8]) -> Result<(), ExportError> {
    match out {
        None => {
            let stdout = std::io::stdout();
            let mut lock = stdout.lock();
            lock.write_all(content).map_err(|e| ExportError::Io(format!("stdout write failed: {e}")))?;
            lock.flush().map_err(|e| ExportError::Io(format!("stdout flush failed: {e}")))?;
        }
        Some(path) => {
            atomic_write_export(path, content).map_err(|e| ExportError::Io(format!("atomic write failed: {e}")))?;
        }
    }
    Ok(())
}

fn format_rfc3339_millis(dt: DateTime<Utc>) -> String {
    dt.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

/// Atomic write: write to temp → fsync → rename over target.
///
/// Mirrors the pattern in `crates/memory-merge-driver/src/main.rs::persist_merged_output`.
fn atomic_write_export(target: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = target.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    if !parent.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("parent directory does not exist: {}", parent.display()),
        ));
    }
    if !parent.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("output parent is not a directory: {}", parent.display()),
        ));
    }
    let target_name = target
        .file_name()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "output path has no file name"))?
        .to_string_lossy();
    refuse_symlink_target(target)?;
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock must be after UNIX_EPOCH")
        .as_nanos();
    // Leading-dot hides the temp file from `ls` between fsync and rename.
    let tmp_path = parent.join(format!(".{target_name}.{pid}.{nanos}.tmp"));
    // create_new fails atomically if a stale temp exists, instead of truncating it.
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options.open(&tmp_path)?;
    let write_result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&tmp_path, target)?;
        fsync_parent_dir_best_effort(parent);
        Ok(())
    })();
    if let Err(err) = write_result {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(err);
    }
    Ok(())
}

fn refuse_symlink_target(target: &Path) -> std::io::Result<()> {
    match std::fs::symlink_metadata(target) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("refusing to write export through symlink target: {}", target.display()),
        )),
        Ok(_) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

fn fsync_parent_dir_best_effort(parent: &Path) {
    if let Ok(dir) = std::fs::File::open(parent) {
        let _ = dir.sync_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use memory_substrate::{
        Author, AuthorKind, ClassificationOutcome, EventContext, Frontmatter, InitOptions, Memory, MemoryId,
        MemoryType, RepoPath, RetrievalPolicy, Sensitivity, Source, SourceKind, TrustLevel, WriteMode, WritePolicy,
        WriteRequest,
    };

    #[tokio::test]
    async fn collect_memories_fails_when_indexed_path_becomes_unreadable() {
        let temp = tempfile::tempdir().expect("tempdir");
        let roots = Roots::new(temp.path().join("repo"), temp.path().join("runtime"));
        let substrate = Substrate::init(
            roots.clone(),
            InitOptions { force_unsafe_durability: true, device_id: Some("dev_exportunit01".to_string()) },
        )
        .await
        .expect("init substrate");

        let id = "mem_20260501_deadbeef00000000_000001";
        write_test_memory(&substrate, id).await;
        let corrupt_path = roots.repo.join("agent").join("claims").join(format!("{id}.md"));
        std::fs::write(&corrupt_path, b"not valid frontmatter").expect("corrupt indexed file");

        let error = match collect_memories(&substrate, None).await {
            Ok(_) => panic!("corrupt indexed envelope must fail"),
            Err(error) => error,
        };
        let message = error.to_string();
        assert!(
            message.contains("failed to read memory envelope")
                && message.contains(id)
                && message.contains("agent/claims"),
            "unexpected error: {message}"
        );
    }

    async fn write_test_memory(substrate: &Substrate, id: &str) {
        let ts = DateTime::parse_from_rfc3339("2026-05-01T10:00:00Z").expect("fixed ts").with_timezone(&Utc);
        let memory_id = MemoryId::new(id);
        let memory = Memory {
            frontmatter: Frontmatter {
                schema_version: memory_substrate::SUBSTRATE_SCHEMA_VERSION,
                id: memory_id.clone(),
                memory_type: MemoryType::Claim,
                scope: Scope::Agent,
                summary: "export unit test".to_string(),
                confidence: 0.9,
                original_confidence: None,
                trust_level: TrustLevel::Trusted,
                sensitivity: Sensitivity::Internal,
                status: MemoryStatus::Active,
                created_at: ts,
                updated_at: ts,
                observed_at: None,
                author: Author {
                    kind: AuthorKind::System,
                    user_handle: None,
                    harness: None,
                    harness_version: None,
                    session_id: None,
                    subagent_id: None,
                    phase: None,
                    component: Some("export-unit-test".to_string()),
                },
                namespace: None,
                canonical_namespace_id: None,
                tags: vec!["export-test".to_string()],
                entities: Vec::new(),
                aliases: Vec::new(),
                source: Source {
                    kind: SourceKind::System,
                    reference: None,
                    harness: None,
                    harness_version: None,
                    session_id: None,
                    subagent_id: None,
                    device: None,
                },
                evidence: Vec::new(),
                requires_user_confirmation: false,
                review_state: None,
                supersedes: Vec::new(),
                superseded_by: Vec::new(),
                related: Vec::new(),
                tombstone_events: Vec::new(),
                retrieval_policy: RetrievalPolicy {
                    passive_recall: true,
                    max_scope: Scope::Agent,
                    mask_personal_for_synthesis: false,
                    index_body: true,
                    index_embeddings: false,
                },
                write_policy: WritePolicy {
                    human_review_required: false,
                    policy_applied: "trusted-v1".to_string(),
                    expected_base_hash: None,
                },
                merge_diagnostics: None,
                abstraction: None,
                cues: Vec::new(),
                extras: Default::default(),
            },
            body: "body".to_string(),
            path: Some(RepoPath::new(format!("agent/claims/{id}.md"))),
        };

        substrate
            .write_memory(WriteRequest {
                operation_id: None,
                memory,
                expected_base_hash: None,
                write_mode: WriteMode::CreateNew,
                index_projection: None,
                event_context: EventContext::default(),
                allow_best_effort_durability: true,
                classification: ClassificationOutcome::Trusted,
            })
            .await
            .expect("write test memory");
    }
}
