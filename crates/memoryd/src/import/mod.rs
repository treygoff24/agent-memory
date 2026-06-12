//! Memorum importer: non-destructive, idempotent backfill from prior agent
//! harnesses (Claude Code, OpenAI Codex CLI) into the Memorum substrate.
//!
//! Architecture (per `docs/plans/2026-05-27-memorum-importer-and-predogfood-ux.md`):
//!
//! - `discovery` locates Claude / Codex memory roots from CLI flags, env vars,
//!   settings files, and defaults — in that precedence order.
//! - `state` tracks per-source-key idempotency in `$MEMORUM_REPO/.memorum/import-state.json`,
//!   guarded by `flock` against concurrent invocations.
//! - `project_map` (T04) maps cwds to project namespaces using `recall::project`.
//! - `sources::claude` / `sources::codex` (T02/T03) parse harness-specific shapes
//!   into a uniform `ParsedMemory` candidate.
//! - `pipeline` (T05/T06) plans then executes the import, going through the
//!   daemon socket so privacy/governance/event-log machinery is reused intact.
//! - `report` (T06) summarises per-harness counts, refusals, and dedup hits.
//!
//! **Invariants the importer must not violate** (full list in the plan):
//! 1. Source files are read-only; the importer never modifies harness memory.
//! 2. Every memory goes through the daemon socket — no direct `Substrate::write_memory`.
//! 3. `source.kind = import`, `source.harness = "claude-code" | "codex"`,
//!    `source.ref = <absolute source path>` is the fixed provenance shape.
//! 4. Idempotency is durable through the state file (performance optimization)
//!    and re-established via daemon duplicate-detection (correctness).

pub mod candidate;
pub mod discovery;
pub mod pipeline;
pub mod project_map;
pub mod report;
pub mod state;

pub mod sources;

use std::path::PathBuf;

use thiserror::Error;

/// Error type for the importer. Carries enough context that each variant maps to
/// a single line in the import report.
#[derive(Debug, Error)]
pub enum ImportError {
    /// IO error reading a source file, the state file, or the lock file.
    #[error("import I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// Parse error: malformed YAML frontmatter on a Claude topic, missing
    /// `scope:` on a Codex Task Group, etc. Carries the source-key for the
    /// import report.
    #[error("parse error for {source_key}: {reason}")]
    Parse { source_key: String, reason: String },

    /// Encoding error: source file is not valid UTF-8.
    #[error("encoding error for {source_key}: {reason}")]
    Encoding { source_key: String, reason: String },

    /// State-file lock could not be acquired within the timeout window — another
    /// `memoryd import` is in progress. The lock file's pid identifies the
    /// owning process.
    #[error("another import is in progress (pid {pid}); release the lock at {lock_path:?} before retrying")]
    AnotherImportInProgress { pid: u32, lock_path: PathBuf },

    /// State file on disk is malformed JSON. The file is renamed to
    /// `<path>.corrupt-<ts>` for diagnosis; the importer re-creates an empty
    /// state file on next run.
    #[error("import state file at {path} is corrupt: {reason}")]
    CorruptState { path: PathBuf, reason: String },

    /// Wraps a JSON serialization or deserialization failure.
    #[error("import json error: {0}")]
    Json(#[from] serde_json::Error),

    /// Execution aborted after one or more daemon-mediated writes had already
    /// committed. Import is intentionally not fully transactional, so the
    /// operator needs an honest count before rerunning.
    #[error("import aborted while processing {source_key} after {completed_writes} memories had already been written: {source}")]
    PartialExecute {
        source_key: String,
        completed_writes: usize,
        #[source]
        source: Box<ImportError>,
    },
}

impl ImportError {
    /// Convert a `std::io::Error` into a path-tagged `ImportError::Io`.
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io { path: path.into(), source }
    }
}

/// Result alias used throughout the importer.
pub type ImportResult<T> = Result<T, ImportError>;
