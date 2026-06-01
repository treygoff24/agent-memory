//! MCP wiring scaffold.
//!
//! T02 replaces this unsupported stub with per-harness config writers.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Harness whose MCP configuration should be wired.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HarnessTarget {
    Claude,
    Codex,
}

/// Desired MCP server command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpServerSpec {
    pub name: String,
    pub command: PathBuf,
    pub args: Vec<String>,
}

impl McpServerSpec {
    pub fn new(name: impl Into<String>, command: impl Into<PathBuf>, args: Vec<String>) -> Self {
        Self { name: name.into(), command: command.into(), args }
    }
}

/// Wiring mode for config writers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WireMode {
    Apply,
    PrintOnly,
}

/// MCP wiring outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireOutcome {
    pub target: HarnessTarget,
    pub status: WireStatus,
    pub message: Option<String>,
}

/// Status values produced by MCP wiring.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WireStatus {
    Wired,
    AlreadyCurrent,
    Updated,
    PrintedOnly,
    Skipped,
}

/// Errors returned by the current stub or future wiring implementations.
#[derive(Debug, Error)]
pub enum WireError {
    #[error("MCP wiring for {target:?} is not implemented yet")]
    Unsupported { target: HarnessTarget },
}

/// Compile-time stub for T00. T02 fills in Claude and Codex behavior.
pub fn wire(target: HarnessTarget, _spec: &McpServerSpec, _mode: WireMode) -> Result<WireOutcome, WireError> {
    Err(WireError::Unsupported { target })
}
