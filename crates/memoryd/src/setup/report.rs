//! Serializable setup reports.

use serde::{Deserialize, Serialize};

use crate::import::report::ImportReport;

use super::decide::SetupDecisions;
use super::detect::SetupDetection;

/// Machine-readable outcome for `memoryd init`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetupReport {
    pub schema_version: u32,
    pub detection: SetupDetection,
    pub decisions: SetupDecisions,
    pub steps: Vec<SetupStepReport>,
    pub import_report: Option<ImportReport>,
    pub restart_required: bool,
}

impl SetupReport {
    pub fn new(detection: SetupDetection, decisions: SetupDecisions) -> Self {
        Self {
            schema_version: 1,
            detection,
            decisions,
            steps: Vec::new(),
            import_report: None,
            restart_required: false,
        }
    }

    pub fn with_restart_required(mut self, restart_required: bool) -> Self {
        self.restart_required = restart_required;
        self
    }

    pub fn with_import_report(mut self, import_report: ImportReport) -> Self {
        self.import_report = Some(import_report);
        self
    }

    pub fn push_step(&mut self, step: SetupStepReport) {
        self.steps.push(step);
    }
}

/// Individual setup step report entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetupStepReport {
    pub step: SetupStep,
    pub status: SetupStepStatus,
    pub message: Option<String>,
}

impl SetupStepReport {
    pub fn new(step: SetupStep, status: SetupStepStatus) -> Self {
        Self { step, status, message: None }
    }

    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }
}

/// Known setup steps. Future tasks attach behavior to these names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupStep {
    Detect,
    EnsureRepo,
    EnsureDaemon,
    Import,
    WireMcp,
    Verify,
}

/// Step status used in setup reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupStepStatus {
    Succeeded,
    Failed,
    Skipped,
    Expected,
}
