//! `memoryd export` subcommand implementation.
//!
//! Emits a portable JSON snapshot of a substrate's contents per
//! `feature-memoryd-export-v0.1.md`.  Semantically read-only against substrate
//! content; does not mutate memories, locks, event-log entries, or index rows
//! beyond what `Substrate::open`'s standard runtime initialization already does.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use memory_substrate::config::load_local_device_config;
use memory_substrate::{MemoryContent, MemoryStatus, Roots, Scope, Substrate};
use serde::Serialize;

use crate::runtime_privacy::install_privacy_runtime_from_roots;

/// Arguments for `memoryd export`.
///
/// Note: opening the substrate triggers standard runtime-initialization side
/// effects even though the export does not write memory content.  These include
/// runtime-directory creation, index-repair replay, and event-log mirror
/// rebuild.  Stop any running `memoryd serve` daemon before exporting against
/// the same `--repo` / `--runtime` pair.
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
    fn exit_code(&self) -> i32 {
        match self {
            ExportError::Argument(_) => 2,
            ExportError::Substrate(_) | ExportError::Io(_) => 1,
        }
    }
}

/// Run the export subcommand.
///
/// Returns `Ok(())` on success.  On failure, prints to stderr and calls
/// `std::process::exit` with the appropriate exit code so that stdout is never
/// contaminated with diagnostic output.
pub async fn run_export(args: ExportArgs) -> anyhow::Result<()> {
    if let Err(err) = run_export_inner(args).await {
        eprintln!("error: {err}");
        std::process::exit(err.exit_code());
    }
    Ok(())
}

async fn run_export_inner(args: ExportArgs) -> Result<(), ExportError> {
    // --format is enforced at clap-parse time via ValueEnum; no runtime check needed.
    let since_dt = parse_since(args.since.as_deref())?;

    install_privacy_runtime_from_roots(&args.repo, &args.runtime)
        .map_err(|e| ExportError::Substrate(format!("privacy runtime install failed: {e}")))?;

    let roots = Roots::new(args.repo.clone(), args.runtime.clone());
    let substrate = Substrate::open(roots).await.map_err(|e| ExportError::Substrate(e.to_string()))?;

    let source_device_id = read_device_id(&args.runtime)?;
    let exported_at = format_rfc3339_millis(Utc::now());
    let mut memories = collect_memories(&substrate, since_dt)?;
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

    let json = serde_json::to_string_pretty(&envelope)
        .map_err(|e| ExportError::Io(format!("JSON serialization failed: {e}")))?;
    let output = format!("{json}\n");
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

    let dt = raw
        .parse::<DateTime<Utc>>()
        .or_else(|_| raw.parse::<chrono::DateTime<chrono::FixedOffset>>().map(|dt| dt.with_timezone(&Utc)))
        .map_err(|_| {
            ExportError::Argument(format!("--since '{raw}': parse failed; use RFC3339 UTC, e.g. 2026-05-01T00:00:00Z"))
        })?;

    Ok(Some(dt))
}

fn read_device_id(runtime: &Path) -> Result<String, ExportError> {
    load_local_device_config(runtime)
        .map_err(|e| ExportError::Substrate(format!("device config load failed: {e}")))?
        .ok_or_else(|| {
            ExportError::Substrate(
                "device config not found; run `memoryd serve --init` to initialize the runtime directory".to_string(),
            )
        })
        .map(|cfg| cfg.device.id)
}

fn collect_memories(substrate: &Substrate, since_dt: Option<DateTime<Utc>>) -> Result<Vec<ExportMemory>, ExportError> {
    substrate
        .iter_memory_envelopes()
        .map(|result| {
            let envelope =
                result.map_err(|e| ExportError::Substrate(format!("failed to read memory envelope: {e}")))?;
            let fm = &envelope.metadata.frontmatter;

            if let Some(since) = since_dt {
                if fm.updated_at < since {
                    return Ok(None);
                }
            }

            let (body, body_marker) = match &envelope.content {
                MemoryContent::Plaintext(text) => (Some(text.clone()), None),
                MemoryContent::Ciphertext { .. } => (None, Some("encrypted".to_string())),
                MemoryContent::MetadataOnly => (None, Some("metadata-only".to_string())),
            };

            let created_at = format_rfc3339_millis(fm.created_at);
            let updated_at = format_rfc3339_millis(fm.updated_at);
            let frontmatter_value = serde_json::to_value(fm).expect("Frontmatter must serialize to JSON");

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
        })
        .collect::<Result<Vec<Option<ExportMemory>>, ExportError>>()
        .map(|v| v.into_iter().flatten().collect())
}

fn emit_output(out: Option<&Path>, content: &str) -> Result<(), ExportError> {
    match out {
        None => {
            let stdout = std::io::stdout();
            let mut lock = stdout.lock();
            lock.write_all(content.as_bytes()).map_err(|e| ExportError::Io(format!("stdout write failed: {e}")))?;
            lock.flush().map_err(|e| ExportError::Io(format!("stdout flush failed: {e}")))?;
        }
        Some(path) => {
            atomic_write_export(path, content.as_bytes())
                .map_err(|e| ExportError::Io(format!("atomic write failed: {e}")))?;
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
    let parent = target
        .parent()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "output path has no parent directory"))?;
    if !parent.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("parent directory does not exist: {}", parent.display()),
        ));
    }
    let target_name = target
        .file_name()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "output path has no file name"))?
        .to_string_lossy();
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().subsec_nanos();
    // Leading-dot hides the temp file from `ls` between fsync and rename.
    let tmp_path = parent.join(format!(".{target_name}.{pid}.{nanos}.tmp"));
    // create_new fails atomically if a stale temp exists, instead of truncating it.
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp_path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    if let Err(err) = std::fs::rename(&tmp_path, target) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(err);
    }
    Ok(())
}
