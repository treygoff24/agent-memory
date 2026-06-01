//! Shared setup-engine scaffolding for `memoryd init`.
//!
//! This module intentionally contains only the type surface and detection
//! scaffold. Step execution is wired in later onboarding tasks.

pub mod decide;
pub mod detect;
pub mod io;
pub mod mcp_wire;
pub mod report;

use std::path::{Path, PathBuf};

use thiserror::Error;

pub use decide::{DaemonStrategy, HarnessSelection, NonGitCwdDecision, SetupDecisions, WireMcpSelection};
pub use detect::{
    DaemonDetection, HarnessDetection, SetupDetection, SetupDetectionOptions, SetupDiscoverySource, SetupSocketState,
};
pub use io::{collect_setup_decisions, FlagDrivenIo, InteractiveIo, SetupIo};
pub use mcp_wire::{wire, HarnessTarget, McpServerSpec, WireError, WireMode, WireOutcome, WireStatus};
pub use report::{SetupReport, SetupStep, SetupStepReport, SetupStepStatus};

/// Result alias for setup orchestration.
pub type SetupResult<T> = Result<T, SetupError>;

/// Shared setup error type for detection, decisions, and future steps.
#[derive(Debug, Error)]
pub enum SetupError {
    /// Import discovery or parsing failed while detecting existing memories.
    #[error(transparent)]
    Import(#[from] crate::import::ImportError),

    /// MCP wiring failed.
    #[error(transparent)]
    Wire(#[from] WireError),

    /// A setup surface exists for downstream tasks but has no behavior yet.
    #[error("setup behavior is not implemented yet: {0}")]
    Unsupported(&'static str),
}

/// Minimal engine handle. Downstream tasks add step execution here.
#[derive(Debug, Clone)]
pub struct SetupEngine {
    repo: PathBuf,
    runtime: PathBuf,
}

impl SetupEngine {
    /// Create an engine scoped to a Memorum repo and runtime directory.
    pub fn new(repo: impl Into<PathBuf>, runtime: impl Into<PathBuf>) -> Self {
        Self { repo: repo.into(), runtime: runtime.into() }
    }

    /// Memorum repository root targeted by setup.
    pub fn repo(&self) -> &Path {
        &self.repo
    }

    /// Runtime directory targeted by setup.
    pub fn runtime(&self) -> &Path {
        &self.runtime
    }
}

/// Owned setup plan produced after detection and decision collection.
#[derive(Debug, Clone)]
pub struct SetupPlan {
    pub detection: SetupDetection,
    pub decisions: SetupDecisions,
}

/// Setup run outcome type. Future step execution fills this report.
pub type SetupOutcome = SetupReport;

/// Per-step result type used by reports.
pub type SetupStepResult = SetupStepReport;
