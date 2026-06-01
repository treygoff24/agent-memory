//! Detection for the shared setup engine.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::import::discovery::{
    discover_claude_memory_root, discover_codex_memory_root, ClaudeMemoryRoot, CodexMemoryRoot, DiscoverySource,
};
use crate::import::sources::{claude, codex};
use crate::socket::{default_runtime_root, probe_live_socket, resolve_socket_path, SocketProbe};

use super::SetupResult;

/// Optional overrides used by tests and non-interactive setup flags.
#[derive(Debug, Clone, Default)]
pub struct SetupDetectionOptions {
    pub claude_root_override: Option<PathBuf>,
    pub codex_root_override: Option<PathBuf>,
    pub socket_path: Option<PathBuf>,
}

/// Machine-readable setup detection summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetupDetection {
    pub claude: HarnessDetection,
    pub codex: HarnessDetection,
    pub daemon: DaemonDetection,
}

impl SetupDetection {
    /// Detect setup state using process environment and default paths.
    pub fn run() -> SetupResult<Self> {
        Self::run_with_options(SetupDetectionOptions::default())
    }

    /// Detect setup state with explicit path overrides.
    pub fn run_with_options(options: SetupDetectionOptions) -> SetupResult<Self> {
        let claude_root = discover_claude_memory_root(options.claude_root_override.as_deref())?;
        let codex_root = discover_codex_memory_root(options.codex_root_override.as_deref())?;
        let daemon = detect_daemon(options.socket_path);

        Ok(Self { claude: detect_claude(claude_root)?, codex: detect_codex(codex_root)?, daemon })
    }
}

/// Per-harness memory-root detection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarnessDetection {
    pub root: Option<PathBuf>,
    pub source: Option<SetupDiscoverySource>,
    pub candidates: usize,
    pub parse_errors: usize,
}

/// Discovery-source values serialized by setup reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupDiscoverySource {
    FlagOverride,
    EnvVar,
    SettingsFile,
    Default,
}

impl From<DiscoverySource> for SetupDiscoverySource {
    fn from(source: DiscoverySource) -> Self {
        match source {
            DiscoverySource::FlagOverride => Self::FlagOverride,
            DiscoverySource::EnvVar => Self::EnvVar,
            DiscoverySource::SettingsFile => Self::SettingsFile,
            DiscoverySource::Default => Self::Default,
        }
    }
}

/// Daemon socket detection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonDetection {
    pub socket_path: PathBuf,
    pub socket_state: SetupSocketState,
}

/// Serializable socket probe state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupSocketState {
    Live,
    Stale,
    Absent,
}

impl From<SocketProbe> for SetupSocketState {
    fn from(probe: SocketProbe) -> Self {
        match probe {
            SocketProbe::Live => Self::Live,
            SocketProbe::Stale => Self::Stale,
            SocketProbe::Absent => Self::Absent,
        }
    }
}

fn detect_claude(root: Option<ClaudeMemoryRoot>) -> SetupResult<HarnessDetection> {
    let Some(root) = root else {
        return Ok(empty_harness_detection());
    };
    let output = claude::parse(&root.path)?;
    Ok(HarnessDetection {
        root: Some(root.path),
        source: Some(root.source.into()),
        candidates: output.candidates.len(),
        parse_errors: output.errors.len(),
    })
}

fn detect_codex(root: Option<CodexMemoryRoot>) -> SetupResult<HarnessDetection> {
    let Some(root) = root else {
        return Ok(empty_harness_detection());
    };
    let output = codex::parse(&root.path)?;
    Ok(HarnessDetection {
        root: Some(root.path),
        source: Some(root.source.into()),
        candidates: output.candidates.len(),
        parse_errors: output.errors.len(),
    })
}

fn empty_harness_detection() -> HarnessDetection {
    HarnessDetection { root: None, source: None, candidates: 0, parse_errors: 0 }
}

fn detect_daemon(socket_path: Option<PathBuf>) -> DaemonDetection {
    let socket_path = socket_path.unwrap_or_else(|| resolve_socket_path(&default_runtime_root()));
    let socket_state = probe_live_socket(&socket_path).into();
    DaemonDetection { socket_path, socket_state }
}
