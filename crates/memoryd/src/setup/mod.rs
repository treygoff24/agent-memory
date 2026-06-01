//! Shared setup-engine scaffolding for `memoryd init`.
//!
//! The public setup engine keeps detection/decision collection in this module
//! and delegates executable setup steps to `steps`.

pub mod decide;
pub mod detect;
pub mod io;
pub mod mcp_wire;
pub mod report;
pub mod steps;

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

    /// Run setup using default harness discovery and this engine's runtime socket.
    pub async fn run(&self, io: &mut dyn SetupIo) -> SetupResult<SetupOutcome> {
        self.run_with_options(io, self.default_detection_options()).await
    }

    /// Run setup with explicit detection options.
    ///
    /// This keeps tests deterministic by letting them pin harness roots and the
    /// daemon socket while production callers use default discovery.
    pub async fn run_with_options(
        &self,
        io: &mut dyn SetupIo,
        mut options: SetupDetectionOptions,
    ) -> SetupResult<SetupOutcome> {
        if options.socket_path.is_none() {
            options.socket_path = Some(crate::socket::resolve_socket_path(&self.runtime));
        }

        let detection = SetupDetection::run_with_options(options)?;
        let decisions = collect_setup_decisions(io, &detection)?;
        let plan = SetupPlan { detection: detection.clone(), decisions: decisions.clone() };
        let mut report = SetupReport::new(detection, decisions);
        report.push_step(SetupStepReport::new(SetupStep::Detect, SetupStepStatus::Succeeded));

        steps::run_all(self, &plan, io, &mut report).await;
        Ok(report)
    }

    fn default_detection_options(&self) -> SetupDetectionOptions {
        SetupDetectionOptions {
            claude_root_override: None,
            codex_root_override: None,
            socket_path: Some(crate::socket::resolve_socket_path(&self.runtime)),
        }
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
