//! Owned setup decisions gathered by interactive or flag-driven frontends.

use serde::{Deserialize, Serialize};

/// Complete decision bundle for a setup run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetupDecisions {
    pub import_memories: bool,
    pub harnesses: HarnessSelection,
    pub non_git_cwd_default: NonGitCwdDecision,
    pub wire_mcp: WireMcpSelection,
    pub wire_hooks: WireHooksSelection,
    pub daemon: DaemonStrategy,
    pub print_only: bool,
}

impl Default for SetupDecisions {
    fn default() -> Self {
        Self {
            import_memories: false,
            harnesses: HarnessSelection::Current,
            non_git_cwd_default: NonGitCwdDecision::DeriveProject,
            wire_mcp: WireMcpSelection::Current,
            wire_hooks: WireHooksSelection::Current,
            daemon: DaemonStrategy::OnDemand,
            print_only: false,
        }
    }
}

/// Harness set selected for import.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HarnessSelection {
    Current,
    Claude,
    Codex,
    All,
    None,
}

/// Default disposition for imported memories with non-git working directories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NonGitCwdDecision {
    /// Derive a project namespace from the cwd path (no `.memory-project.yaml`
    /// written) so the memories are saved and land active. The default — it
    /// never loses memories and keeps them recall-visible.
    DeriveProject,
    Skip,
    Me,
    Generate,
}

/// Harness configs selected for MCP wiring.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WireMcpSelection {
    Current,
    Claude,
    Codex,
    All,
    None,
}

/// Harness configs selected for passive-recall hook installation.
///
/// Mirrors [`WireMcpSelection`]: installs the `memoryd recall hook` lifecycle
/// hooks (SessionStart base block + UserPromptSubmit delta + SubagentStart) into
/// the selected harness config(s). `Current` targets the single detected harness;
/// `None` skips hook wiring entirely.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WireHooksSelection {
    Current,
    Claude,
    Codex,
    All,
    None,
}

/// Daemon arrangement selected for setup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DaemonStrategy {
    OnDemand,
    Background,
    Launchd,
    None,
}
