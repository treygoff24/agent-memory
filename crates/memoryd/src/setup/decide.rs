//! Owned setup decisions gathered by interactive or flag-driven frontends.

use std::fmt;

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
            // MCP wiring is opt-in under the CLI-first surface; Tier 1 is hooks +
            // skill/CLI. Every other default stays `current`/on.
            wire_mcp: WireMcpSelection::None,
            wire_hooks: WireHooksSelection::Current,
            daemon: DaemonStrategy::OnDemand,
            print_only: false,
        }
    }
}

/// One conceptual "which harness(es) does this decision target" type, shared by
/// import-harness selection, MCP wiring, and passive-recall hook wiring. The
/// frontends and the engine treat all three identically; the [`WireMcpSelection`]
/// and [`WireHooksSelection`] aliases name the same type at each call site for
/// readability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HarnessSelection {
    Current,
    Claude,
    Codex,
    All,
    None,
}

impl HarnessSelection {
    /// Human label used in wizard echoes (`--harness`, `--wire-mcp`,
    /// `--wire-hooks`). Distinct from the kebab-case serde representation.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Current => "current harness",
            Self::Claude => "Claude Code",
            Self::Codex => "Codex CLI",
            Self::All => "all harnesses",
            Self::None => "none",
        }
    }
}

impl fmt::Display for HarnessSelection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
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

/// Harness configs selected for MCP wiring. Same conceptual type as
/// [`HarnessSelection`] (same variants, same kebab-case serde); aliased for
/// call-site readability.
pub type WireMcpSelection = HarnessSelection;

/// Harness configs selected for passive-recall hook installation.
///
/// Installs the `memoryd recall hook` lifecycle hooks (SessionStart base block +
/// UserPromptSubmit delta + SubagentStart) into the selected harness config(s).
/// `Current` targets the single detected harness; `None` skips hook wiring
/// entirely. Same conceptual type as [`HarnessSelection`]; aliased for
/// call-site readability.
pub type WireHooksSelection = HarnessSelection;

/// Daemon arrangement selected for setup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DaemonStrategy {
    OnDemand,
    Background,
    Launchd,
    None,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// CLI-first surface: MCP wiring is opt-in, everything else stays on by
    /// default. Pins both halves of the Task 6 flip so a regression is loud.
    #[test]
    fn default_decisions_skip_mcp_but_keep_hooks() {
        let decisions = SetupDecisions::default();
        assert_eq!(decisions.wire_mcp, WireMcpSelection::None, "MCP wiring must be opt-in (default none)");
        assert_eq!(decisions.wire_hooks, WireHooksSelection::Current, "hook wiring stays default-current (Tier 1)");
        assert_eq!(decisions.harnesses, HarnessSelection::Current);
        assert_eq!(decisions.daemon, DaemonStrategy::OnDemand);
    }
}
