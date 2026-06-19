//! Synchronous setup decision I/O.

use super::decide::{
    DaemonStrategy, HarnessSelection, NonGitCwdDecision, SetupDecisions, WireHooksSelection, WireMcpSelection,
};
use super::detect::SetupDetection;
use super::SetupResult;

/// Decision I/O used by both interactive and flag-driven setup frontends.
pub trait SetupIo {
    fn confirm_import(&mut self, detection: &SetupDetection) -> SetupResult<bool>;
    fn choose_harnesses(&mut self, detection: &SetupDetection) -> SetupResult<HarnessSelection>;
    fn choose_non_git_cwd_default(&mut self, detection: &SetupDetection) -> SetupResult<NonGitCwdDecision>;
    fn choose_mcp_wiring(&mut self, detection: &SetupDetection) -> SetupResult<WireMcpSelection>;
    /// Choose which harness config(s) get the passive-recall lifecycle hooks
    /// installed. Defaulted so frontends opt in incrementally; WS1's decision
    /// surface overrides it for the interactive and flag-driven paths.
    fn choose_hook_wiring(&mut self, _detection: &SetupDetection) -> SetupResult<WireHooksSelection> {
        Ok(WireHooksSelection::Current)
    }
    fn choose_daemon_strategy(&mut self, detection: &SetupDetection) -> SetupResult<DaemonStrategy>;
    fn print_only(&mut self) -> SetupResult<bool>;
    fn note(&mut self, message: &str) -> SetupResult<()>;
}

/// Gather an owned decision bundle without holding I/O borrows across steps.
pub fn collect_setup_decisions(io: &mut dyn SetupIo, detection: &SetupDetection) -> SetupResult<SetupDecisions> {
    Ok(SetupDecisions {
        import_memories: io.confirm_import(detection)?,
        harnesses: io.choose_harnesses(detection)?,
        non_git_cwd_default: io.choose_non_git_cwd_default(detection)?,
        wire_mcp: io.choose_mcp_wiring(detection)?,
        wire_hooks: io.choose_hook_wiring(detection)?,
        daemon: io.choose_daemon_strategy(detection)?,
        print_only: io.print_only()?,
    })
}

/// Flag-driven setup I/O backed by pre-parsed CLI decisions.
#[derive(Debug, Clone)]
pub struct FlagDrivenIo {
    decisions: SetupDecisions,
    notes: Vec<String>,
}

impl FlagDrivenIo {
    pub fn new(decisions: SetupDecisions) -> Self {
        Self { decisions, notes: Vec::new() }
    }

    pub fn notes(&self) -> &[String] {
        &self.notes
    }
}

impl SetupIo for FlagDrivenIo {
    fn confirm_import(&mut self, _detection: &SetupDetection) -> SetupResult<bool> {
        Ok(self.decisions.import_memories)
    }

    fn choose_harnesses(&mut self, _detection: &SetupDetection) -> SetupResult<HarnessSelection> {
        Ok(self.decisions.harnesses)
    }

    fn choose_non_git_cwd_default(&mut self, _detection: &SetupDetection) -> SetupResult<NonGitCwdDecision> {
        Ok(self.decisions.non_git_cwd_default)
    }

    fn choose_mcp_wiring(&mut self, _detection: &SetupDetection) -> SetupResult<WireMcpSelection> {
        Ok(self.decisions.wire_mcp)
    }

    fn choose_hook_wiring(&mut self, _detection: &SetupDetection) -> SetupResult<WireHooksSelection> {
        Ok(self.decisions.wire_hooks)
    }

    fn choose_daemon_strategy(&mut self, _detection: &SetupDetection) -> SetupResult<DaemonStrategy> {
        Ok(self.decisions.daemon)
    }

    fn print_only(&mut self) -> SetupResult<bool> {
        Ok(self.decisions.print_only)
    }

    fn note(&mut self, message: &str) -> SetupResult<()> {
        self.notes.push(message.to_string());
        Ok(())
    }
}
