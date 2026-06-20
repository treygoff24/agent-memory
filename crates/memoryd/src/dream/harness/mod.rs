//! Dream harness CLI integration.
//!
//! This module is split into focused submodules behind a re-export facade so the
//! public `dream::harness::*` paths stay byte-identical for external callers:
//!
//! - `env`: the documented environment allowlists, [`MinimalEnvironment`], and
//!   the internal `AdapterEnv` execution context.
//! - `process`: the hardened-subprocess machinery ([`HardenedCommand`],
//!   [`HardenedOutput`], [`run_hardened_command`], the SIGTERM FFI, capture and
//!   redaction helpers, executable resolution).
//! - `auth`: auth-probe results, candidates, and the probe-racing policy.
//! - `adapters`: the [`HarnessCli`] trait and its concrete implementations
//!   ([`ClaudeCodeCli`], [`CodexCli`], and the test/fixture `EchoCli`).

mod adapters;
mod auth;
mod env;
mod process;

pub use adapters::{ClaudeCodeCli, CodexCli, HarnessCli, HarnessFuture};
pub use auth::AuthProbeResult;
pub use env::{MinimalEnvironment, CLAUDE_ENV_ALLOWLIST, CODEX_ENV_ALLOWLIST, DOCUMENTED_ENV_ALLOWLIST};
pub use process::{run_hardened_command, HardenedCommand, HardenedOutput, HarnessCommandPlan};

#[cfg(any(test, feature = "dev-fixtures"))]
pub use adapters::EchoCli;
