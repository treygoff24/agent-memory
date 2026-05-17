//! `memoryd export` subcommand implementation.
//!
//! Emits a portable JSON snapshot of a substrate's contents per
//! `feature-memoryd-export-v0.1.md`.  Semantically read-only against substrate
//! content; does not mutate memories, locks, event-log entries, or index rows
//! beyond what `Substrate::open`'s standard runtime initialization already does.

use std::io::Write as _;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use memory_substrate::config::load_local_device_config;
use memory_substrate::{MemoryContent, MemoryStatus, Roots, Scope, Substrate};
use serde::Serialize;

use crate::runtime_privacy::install_privacy_runtime_from_roots;

// ---------------------------------------------------------------------------
// CLI args
// ---------------------------------------------------------------------------

/// Arguments for `memoryd export`.
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
    #[arg(long, default_value = "json")]
    pub format: String,
    /// Include only memories whose `updated_at >= <ISO8601>`.
    #[arg(long)]
    pub since: Option<String>,
}

// ---------------------------------------------------------------------------
// Output schema types
// ---------------------------------------------------------------------------

/// Top-level export envelope (§4).
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

/// Per-memory row emitted in the export (§4).
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

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

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
    // Validate --format early (exit 2 on unknown).
    if args.format != "json" {
        return Err(ExportError::Argument(format!(
            "--format '{}' is not supported in v0.1; only 'json' is accepted",
            args.format
        )));
    }

    // Parse --since early (exit 2 on bad value).
    let since_dt: Option<DateTime<Utc>> = match args.since.as_deref() {
        None => None,
        Some(raw) => {
            // Reject bare dates (YYYY-MM-DD) and non-RFC3339 values.
            if raw.len() == 10 && raw.chars().nth(4) == Some('-') {
                return Err(ExportError::Argument(format!(
                    "--since '{raw}' is a bare date; use RFC3339 form, e.g. {raw}T00:00:00Z"
                )));
            }
            raw.parse::<DateTime<Utc>>().map_err(|_| {
                ExportError::Argument(format!(
                    "--since '{raw}': parse failed; use RFC3339 UTC, e.g. 2026-05-01T00:00:00Z"
                ))
            })?;
            // Re-parse as lenient chrono type (accepts offset-qualified too).
            Some(
                raw.parse::<DateTime<Utc>>()
                    .or_else(|_| {
                        raw.parse::<chrono::DateTime<chrono::FixedOffset>>()
                            .map(|dt| dt.with_timezone(&Utc))
                    })
                    .map_err(|_| {
                        ExportError::Argument(format!(
                            "--since '{raw}': parse failed; use RFC3339 UTC, e.g. 2026-05-01T00:00:00Z"
                        ))
                    })?,
            )
        }
    };

    // Install privacy runtime BEFORE opening substrate (mirrors Command::Serve).
    install_privacy_runtime_from_roots(&args.repo, &args.runtime)
        .map_err(|e| ExportError::Substrate(format!("privacy runtime install failed: {e}")))?;

    // Open substrate.
    let roots = Roots::new(args.repo.clone(), args.runtime.clone());
    let substrate = Substrate::open(roots).await.map_err(|e| ExportError::Substrate(e.to_string()))?;

    // Read device id from runtime local-device.yaml.
    let source_device_id = load_local_device_config(&args.runtime)
        .map_err(|e| ExportError::Substrate(format!("device config load failed: {e}")))?
        .map(|cfg| cfg.device.id)
        .unwrap_or_default();

    // exported_at: RFC3339 UTC millisecond precision.
    let exported_at = format_rfc3339_millis(Utc::now());

    // Collect all envelopes.
    let mut memories: Vec<ExportMemory> = substrate
        .iter_memory_envelopes()
        .filter_map(|result| {
            match result {
                Err(_) => None, // skip unreadable files (e.g. legacy raw ciphertext)
                Ok(envelope) => {
                    let fm = &envelope.metadata.frontmatter;

                    // Apply --since filter.
                    if let Some(since) = since_dt {
                        if fm.updated_at < since {
                            return None;
                        }
                    }

                    // Body-variant routing (§6).
                    let (body, body_marker) = match &envelope.content {
                        MemoryContent::Plaintext(text) => (Some(text.clone()), None),
                        MemoryContent::Ciphertext { .. } => (None, Some("encrypted".to_string())),
                        MemoryContent::MetadataOnly => (None, Some("metadata-only".to_string())),
                    };

                    // Timestamps — default to epoch when absent/zero (§4).
                    let epoch = "1970-01-01T00:00:00Z".to_string();
                    let created_at = format_rfc3339_secs(fm.created_at);
                    let updated_at = format_rfc3339_secs(fm.updated_at);
                    let created_at = if created_at == "1970-01-01T00:00:00Z" { epoch.clone() } else { created_at };
                    let updated_at = if updated_at == "1970-01-01T00:00:00Z" { epoch } else { updated_at };

                    // Frontmatter: serialize to serde_json::Value via canonical path.
                    let frontmatter_value = serde_json::to_value(fm).unwrap_or(serde_json::Value::Object(Default::default()));

                    Some(ExportMemory {
                        id: fm.id.as_str().to_string(),
                        scope: fm.scope,
                        status: fm.status,
                        frontmatter: frontmatter_value,
                        body,
                        body_marker,
                        created_at,
                        updated_at,
                    })
                }
            }
        })
        .collect();

    // Sort by (updated_at, id) ascending (§4).
    memories.sort_by(|a, b| {
        a.updated_at.cmp(&b.updated_at).then_with(|| a.id.cmp(&b.id))
    });

    let memory_count = memories.len();

    let envelope = ExportEnvelope {
        schema_version: 1,
        exported_at,
        source_device_id,
        filters: ExportFilters { since: args.since },
        memory_count,
        memories,
    };

    // Serialize to JSON (two-space indent, trailing newline).
    let json = serde_json::to_string_pretty(&envelope)
        .map_err(|e| ExportError::Io(format!("JSON serialization failed: {e}")))?;
    let json_with_newline = format!("{json}\n");
    let bytes_len = json_with_newline.len();

    // Emit to stdout or --out (§3).
    match args.out {
        None => {
            print!("{json_with_newline}");
        }
        Some(out_path) => {
            atomic_write_export(&out_path, json_with_newline.as_bytes())
                .map_err(|e| ExportError::Io(format!("atomic write failed: {e}")))?;
        }
    }

    // Success summary to stderr (§3).  Exactly one line; no trailing diagnostics.
    eprintln!("memory_count={memory_count} bytes={bytes_len}");

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Format a `DateTime<Utc>` as RFC3339 UTC with millisecond precision.
fn format_rfc3339_millis(dt: DateTime<Utc>) -> String {
    dt.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

/// Format a `DateTime<Utc>` as RFC3339 UTC with second precision.
fn format_rfc3339_secs(dt: DateTime<Utc>) -> String {
    dt.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Atomic write: write to temp → fsync → rename over target.
///
/// Mirrors the pattern in `crates/memory-merge-driver/src/main.rs::persist_merged_output`.
fn atomic_write_export(target: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = target
        .parent()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "output path has no parent directory"))?;
    // Surface the missing-parent case early with the path baked into
    // the error message so operators see WHICH parent is missing,
    // rather than a bare "No such file or directory" propagated from
    // the file creation below (spec §8.4).
    if !parent.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("parent directory does not exist: {}", parent.display()),
        ));
    }
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    let tmp_name = format!("{}.{pid}.{nanos}.tmp", target.display());
    let tmp_path = parent.join(
        std::path::Path::new(&tmp_name)
            .file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("export.tmp")),
    );
    // Write + fsync temp.
    let mut file = std::fs::File::create(&tmp_path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    // Rename over target.
    if let Err(err) = std::fs::rename(&tmp_path, target) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(err);
    }
    Ok(())
}
