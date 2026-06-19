//! Detection for the shared setup engine.

use std::collections::HashSet;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::import::discovery::{
    discover_claude_memory_roots, discover_codex_memory_root, ClaudeMemoryRoot, CodexMemoryRoot, DiscoverySource,
};
use crate::import::sources::{claude, codex};
use crate::paths::default_socket;
use crate::socket::{probe_live_socket, SocketProbe};

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
        // Detect across the union of all Claude profile roots so the wizard's
        // candidate count and its "nothing to import" gate match what `import`
        // will actually parse — not just the single precedence root.
        let override_roots: Vec<PathBuf> = options.claude_root_override.iter().cloned().collect();
        let claude_roots = discover_claude_memory_roots(&override_roots)?;
        let codex_root = discover_codex_memory_root(options.codex_root_override.as_deref())?;
        let daemon = detect_daemon(options.socket_path);

        Ok(Self { claude: detect_claude(&claude_roots)?, codex: detect_codex(codex_root)?, daemon })
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
            // Auto-detected sibling profile roots are a default-precedence-adjacent
            // discovery; the setup report has no distinct rung for them.
            DiscoverySource::DetectedProfile => Self::Default,
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

fn detect_claude(roots: &[ClaudeMemoryRoot]) -> SetupResult<HarnessDetection> {
    let Some(primary) = roots.first() else {
        return Ok(empty_harness_detection());
    };
    // Aggregate candidates across every profile root, deduped the same way the
    // importer dedups (canonical source path + source_key), so a memory reached
    // through several symlinked profiles is counted once. The precedence root is
    // reported for display.
    let mut seen: HashSet<(PathBuf, String)> = HashSet::new();
    let mut candidates = 0usize;
    let mut parse_errors = 0usize;
    for root in roots {
        let output = claude::parse(&root.path)?;
        parse_errors += output.errors.len();
        for candidate in output.candidates {
            let canonical =
                std::fs::canonicalize(&candidate.source_path).unwrap_or_else(|_| candidate.source_path.clone());
            if seen.insert((canonical, candidate.source_key)) {
                candidates += 1;
            }
        }
    }
    Ok(HarnessDetection {
        root: Some(primary.path.clone()),
        source: Some(primary.source.into()),
        candidates,
        parse_errors,
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
    let socket_path = socket_path.unwrap_or_else(default_socket);
    let socket_state = probe_live_socket(&socket_path).into();
    DaemonDetection { socket_path, socket_state }
}
