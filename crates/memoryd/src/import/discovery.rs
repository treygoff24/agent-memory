//! Discovery: locate Claude Code and OpenAI Codex CLI memory directories.
//!
//! Discovery precedence is locked by the plan (decisions Q-discovery):
//!
//! - Claude: `--from-claude <path>` flag override → `CLAUDE_CONFIG_DIR` env var
//!   → `autoMemoryDirectory` setting in `~/.claude/settings.json` → default
//!   `~/.claude/projects/`.
//! - Codex: `--from-codex <path>` flag override → `CODEX_HOME` env var →
//!   default `~/.codex/memories/`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::{ImportError, ImportResult};

/// Resolved Claude memory root. `path` may not exist on disk — callers should
/// treat `parse` against a missing directory as a zero-candidate corpus, not
/// an error. The `auto_memory_setting` field records whether the path came
/// from a settings.json `autoMemoryDirectory` override so the report can
/// surface it to the operator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeMemoryRoot {
    pub path: PathBuf,
    pub source: DiscoverySource,
}

/// Resolved Codex memory root. Same semantics as `ClaudeMemoryRoot`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexMemoryRoot {
    pub path: PathBuf,
    pub source: DiscoverySource,
}

/// Provenance for a discovered memory root. Surfaced in the import report so
/// the user can tell which precedence rung the importer landed on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoverySource {
    /// `--from-claude` / `--from-codex` CLI flag override.
    FlagOverride,
    /// `CLAUDE_CONFIG_DIR` / `CODEX_HOME` environment variable.
    EnvVar,
    /// Claude `autoMemoryDirectory` setting in `~/.claude/settings.json`.
    SettingsFile,
    /// Built-in default (`~/.claude/projects/` or `~/.codex/memories/`).
    Default,
    /// A sibling Claude profile directory (`~/.claude-*/projects/`) found by
    /// the multi-root auto-detect scan. Claude memory on a multi-profile
    /// machine reaches different subsets through per-profile symlinks, so the
    /// union of these roots is what fully covers the corpus.
    DetectedProfile,
}

#[derive(Debug, Deserialize)]
struct ClaudeSettings {
    #[serde(default)]
    #[serde(rename = "autoMemoryDirectory")]
    auto_memory_directory: Option<String>,
}

/// Probe environment variables in process scope. Tests inject a `HashMap`-like
/// substitute via [`discover_claude_memory_root_with_env`] instead of
/// monkey-patching `std::env`.
pub trait EnvProvider {
    fn get(&self, key: &str) -> Option<String>;
    fn home_dir(&self) -> Option<PathBuf>;
}

/// Production env provider that reads from `std::env` and `dirs::home_dir`.
#[derive(Debug, Default)]
pub struct ProcessEnv;

impl EnvProvider for ProcessEnv {
    fn get(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }

    fn home_dir(&self) -> Option<PathBuf> {
        dirs::home_dir()
    }
}

/// Locate the Claude Code memory directory. Precedence: flag override →
/// `CLAUDE_CONFIG_DIR` env var → `~/.claude/settings.json autoMemoryDirectory`
/// → default `~/.claude/projects/`. Returns `Ok(None)` when no home directory
/// is resolvable and the upstream rungs are empty — this is normal in sandboxed
/// builds, not an error.
pub fn discover_claude_memory_root(flag_override: Option<&Path>) -> ImportResult<Option<ClaudeMemoryRoot>> {
    discover_claude_memory_root_with_env(flag_override, &ProcessEnv)
}

/// Test seam: same as [`discover_claude_memory_root`] but with an injectable
/// `EnvProvider`.
pub fn discover_claude_memory_root_with_env(
    flag_override: Option<&Path>,
    env: &dyn EnvProvider,
) -> ImportResult<Option<ClaudeMemoryRoot>> {
    if let Some(path) = flag_override {
        return Ok(Some(ClaudeMemoryRoot { path: path.to_path_buf(), source: DiscoverySource::FlagOverride }));
    }
    if let Some(dir) = env.get("CLAUDE_CONFIG_DIR").filter(|value| !value.is_empty()) {
        return Ok(Some(ClaudeMemoryRoot {
            path: PathBuf::from(dir).join("projects"),
            source: DiscoverySource::EnvVar,
        }));
    }
    let Some(home) = env.home_dir() else {
        return Ok(None);
    };
    let settings_path = home.join(".claude").join("settings.json");
    if let Some(custom) = read_claude_auto_memory_directory(&settings_path)? {
        return Ok(Some(ClaudeMemoryRoot { path: custom, source: DiscoverySource::SettingsFile }));
    }
    Ok(Some(ClaudeMemoryRoot { path: home.join(".claude").join("projects"), source: DiscoverySource::Default }))
}

/// Locate the UNION of Claude Code memory roots for an import.
///
/// When `flag_overrides` is non-empty, every override is honored verbatim and
/// in order — no scanning. Otherwise the existing single-root precedence root
/// (`CLAUDE_CONFIG_DIR` → settings.json → `~/.claude/projects/`) is the first
/// entry, then the home directory is scanned for sibling `.claude*` profile
/// directories that contain an existing `projects/` subdir, each appended as a
/// [`DiscoverySource::DetectedProfile`] root.
///
/// Roots are deduplicated by canonicalized path so a root reachable two ways
/// (e.g. the precedence root and a sibling that resolve to the same place)
/// appears once, with precedence order preserved. Paths that do not exist on
/// disk cannot be canonicalized; they dedup on their literal form instead,
/// which is fine because the precedence root is always emitted first.
pub fn discover_claude_memory_roots(flag_overrides: &[PathBuf]) -> ImportResult<Vec<ClaudeMemoryRoot>> {
    discover_claude_memory_roots_with_env(flag_overrides, &ProcessEnv)
}

/// Test seam: same as [`discover_claude_memory_roots`] but with an injectable
/// `EnvProvider`.
pub fn discover_claude_memory_roots_with_env(
    flag_overrides: &[PathBuf],
    env: &dyn EnvProvider,
) -> ImportResult<Vec<ClaudeMemoryRoot>> {
    if !flag_overrides.is_empty() {
        return Ok(flag_overrides
            .iter()
            .map(|path| ClaudeMemoryRoot { path: path.clone(), source: DiscoverySource::FlagOverride })
            .collect());
    }

    let mut roots: Vec<ClaudeMemoryRoot> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();

    // The precedence root comes first so it always wins dedup and first-listed
    // ordering downstream.
    if let Some(root) = discover_claude_memory_root_with_env(None, env)? {
        seen.insert(dedup_key(&root.path));
        roots.push(root);
    }

    // Then scan the home dir for sibling profile dirs (`.claude`, `.claude-foo`,
    // …) whose `projects/` subdir exists on disk.
    if let Some(home) = env.home_dir() {
        for profile_projects in scan_sibling_profile_projects(&home) {
            if seen.insert(dedup_key(&profile_projects)) {
                roots.push(ClaudeMemoryRoot { path: profile_projects, source: DiscoverySource::DetectedProfile });
            }
        }
    }

    Ok(roots)
}

/// Canonicalize when the path exists (so two symlink routes collapse to one);
/// fall back to the literal path when it does not, so non-existent roots still
/// dedup against themselves.
fn dedup_key(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Enumerate `<home>/.claude*/projects` directories that exist on disk, in a
/// deterministic (sorted-by-name) order. Returns the `projects/` subdirectory
/// paths, not the profile roots, so callers can parse them directly.
fn scan_sibling_profile_projects(home: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(home) else {
        return Vec::new();
    };
    let mut matches: Vec<PathBuf> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !name.starts_with(".claude") {
            continue;
        }
        let projects = entry.path().join("projects");
        if projects.is_dir() {
            matches.push(projects);
        }
    }
    matches.sort();
    matches
}

fn read_claude_auto_memory_directory(settings_path: &Path) -> ImportResult<Option<PathBuf>> {
    let raw = match std::fs::read_to_string(settings_path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(ImportError::io(settings_path, error)),
    };
    let parsed: ClaudeSettings = match serde_json::from_str(&raw) {
        Ok(value) => value,
        Err(error) => {
            return Err(ImportError::Parse {
                source_key: settings_path.display().to_string(),
                reason: format!("settings.json: {error}"),
            });
        }
    };
    Ok(parsed.auto_memory_directory.filter(|value| !value.is_empty()).map(PathBuf::from))
}

/// Locate the Codex CLI memory directory. Precedence: flag override →
/// `CODEX_HOME` env var → default `~/.codex/memories/`. Mirrors the Claude
/// flow but without a settings.json indirection.
pub fn discover_codex_memory_root(flag_override: Option<&Path>) -> ImportResult<Option<CodexMemoryRoot>> {
    discover_codex_memory_root_with_env(flag_override, &ProcessEnv)
}

/// Test seam for [`discover_codex_memory_root`].
pub fn discover_codex_memory_root_with_env(
    flag_override: Option<&Path>,
    env: &dyn EnvProvider,
) -> ImportResult<Option<CodexMemoryRoot>> {
    if let Some(path) = flag_override {
        return Ok(Some(CodexMemoryRoot { path: path.to_path_buf(), source: DiscoverySource::FlagOverride }));
    }
    if let Some(dir) = env.get("CODEX_HOME").filter(|value| !value.is_empty()) {
        return Ok(Some(CodexMemoryRoot {
            path: PathBuf::from(dir).join("memories"),
            source: DiscoverySource::EnvVar,
        }));
    }
    let Some(home) = env.home_dir() else {
        return Ok(None);
    };
    Ok(Some(CodexMemoryRoot { path: home.join(".codex").join("memories"), source: DiscoverySource::Default }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct FakeEnv {
        vars: HashMap<String, String>,
        home: Option<PathBuf>,
    }

    impl FakeEnv {
        fn new(home: Option<PathBuf>) -> Self {
            Self { vars: HashMap::new(), home }
        }

        fn with(mut self, key: &str, value: &str) -> Self {
            self.vars.insert(key.to_string(), value.to_string());
            self
        }
    }

    impl EnvProvider for FakeEnv {
        fn get(&self, key: &str) -> Option<String> {
            self.vars.get(key).cloned()
        }
        fn home_dir(&self) -> Option<PathBuf> {
            self.home.clone()
        }
    }

    #[test]
    fn claude_discovery_flag_override_beats_env_and_settings_and_default() {
        let env = FakeEnv::new(Some(PathBuf::from("/home/u"))).with("CLAUDE_CONFIG_DIR", "/env/claude");
        let root = discover_claude_memory_root_with_env(Some(Path::new("/flag/claude")), &env)
            .expect("discovery ok")
            .expect("some root");
        assert_eq!(root.path, PathBuf::from("/flag/claude"));
        assert_eq!(root.source, DiscoverySource::FlagOverride);
    }

    #[test]
    fn claude_discovery_env_beats_settings_and_default() {
        let env = FakeEnv::new(Some(PathBuf::from("/home/u"))).with("CLAUDE_CONFIG_DIR", "/env/claude");
        let root = discover_claude_memory_root_with_env(None, &env).expect("discovery ok").expect("some root");
        assert_eq!(root.path, PathBuf::from("/env/claude/projects"));
        assert_eq!(root.source, DiscoverySource::EnvVar);
    }

    #[test]
    fn claude_discovery_falls_back_to_default_when_no_settings_or_env() {
        let tmp = tempfile::tempdir().expect("tmp");
        let env = FakeEnv::new(Some(tmp.path().to_path_buf()));
        let root = discover_claude_memory_root_with_env(None, &env).expect("discovery ok").expect("some root");
        assert_eq!(root.path, tmp.path().join(".claude").join("projects"));
        assert_eq!(root.source, DiscoverySource::Default);
    }

    #[test]
    fn claude_discovery_honors_settings_auto_memory_directory_when_present() {
        let tmp = tempfile::tempdir().expect("tmp");
        let claude_dir = tmp.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).expect("mkdir");
        std::fs::write(claude_dir.join("settings.json"), r#"{"autoMemoryDirectory":"/custom/claude/memory"}"#)
            .expect("write settings");
        let env = FakeEnv::new(Some(tmp.path().to_path_buf()));
        let root = discover_claude_memory_root_with_env(None, &env).expect("discovery ok").expect("some root");
        assert_eq!(root.path, PathBuf::from("/custom/claude/memory"));
        assert_eq!(root.source, DiscoverySource::SettingsFile);
    }

    #[test]
    fn claude_discovery_returns_none_when_no_home_and_no_overrides() {
        let env = FakeEnv::new(None);
        let result = discover_claude_memory_root_with_env(None, &env).expect("discovery ok");
        assert!(result.is_none());
    }

    #[test]
    fn claude_multi_root_flag_overrides_returned_verbatim_in_order() {
        let env = FakeEnv::new(Some(PathBuf::from("/home/u"))).with("CLAUDE_CONFIG_DIR", "/env/claude");
        let overrides = vec![PathBuf::from("/flag/one"), PathBuf::from("/flag/two")];
        let roots = discover_claude_memory_roots_with_env(&overrides, &env).expect("discovery ok");
        assert_eq!(roots.len(), 2);
        assert_eq!(roots[0].path, PathBuf::from("/flag/one"));
        assert_eq!(roots[0].source, DiscoverySource::FlagOverride);
        assert_eq!(roots[1].path, PathBuf::from("/flag/two"));
        assert_eq!(roots[1].source, DiscoverySource::FlagOverride);
    }

    #[test]
    fn claude_multi_root_auto_detect_lists_precedence_root_first() {
        let tmp = tempfile::tempdir().expect("tmp");
        // Precedence root (`~/.claude/projects`) exists.
        std::fs::create_dir_all(tmp.path().join(".claude").join("projects")).expect("mkdir default");
        let env = FakeEnv::new(Some(tmp.path().to_path_buf()));
        let roots = discover_claude_memory_roots_with_env(&[], &env).expect("discovery ok");
        assert!(!roots.is_empty());
        assert_eq!(roots[0].path, tmp.path().join(".claude").join("projects"));
        assert_eq!(roots[0].source, DiscoverySource::Default);
    }

    #[test]
    fn claude_multi_root_auto_detect_picks_up_sibling_profile_projects() {
        let tmp = tempfile::tempdir().expect("tmp");
        // Precedence root exists, plus a sibling `.claude-foo/projects`.
        std::fs::create_dir_all(tmp.path().join(".claude").join("projects")).expect("mkdir default");
        std::fs::create_dir_all(tmp.path().join(".claude-foo").join("projects")).expect("mkdir sibling");
        // A `.claude-bar` without a projects subdir must be ignored.
        std::fs::create_dir_all(tmp.path().join(".claude-bar")).expect("mkdir no-projects");
        let env = FakeEnv::new(Some(tmp.path().to_path_buf()));
        let roots = discover_claude_memory_roots_with_env(&[], &env).expect("discovery ok");

        let sibling = tmp.path().join(".claude-foo").join("projects");
        let detected: Vec<&ClaudeMemoryRoot> =
            roots.iter().filter(|r| r.source == DiscoverySource::DetectedProfile).collect();
        assert_eq!(detected.len(), 1, "exactly one sibling profile detected");
        assert_eq!(detected[0].path, sibling);
        assert!(
            !roots.iter().any(|r| r.path == tmp.path().join(".claude-bar").join("projects")),
            "a profile dir without a projects subdir is not a root"
        );
    }

    #[test]
    fn claude_multi_root_dedups_precedence_root_against_sibling_scan() {
        let tmp = tempfile::tempdir().expect("tmp");
        // The precedence root IS `~/.claude/projects`, which the sibling scan
        // will also enumerate (`.claude` matches `.claude*`). It must appear
        // once, as the precedence (Default) entry, not twice.
        std::fs::create_dir_all(tmp.path().join(".claude").join("projects")).expect("mkdir default");
        let env = FakeEnv::new(Some(tmp.path().to_path_buf()));
        let roots = discover_claude_memory_roots_with_env(&[], &env).expect("discovery ok");

        let default_path = tmp.path().join(".claude").join("projects");
        let canonical = std::fs::canonicalize(&default_path).expect("canonicalize");
        let occurrences =
            roots.iter().filter(|r| std::fs::canonicalize(&r.path).map(|c| c == canonical).unwrap_or(false)).count();
        assert_eq!(occurrences, 1, "precedence root and sibling scan collapse to one entry");
        assert_eq!(roots[0].source, DiscoverySource::Default, "precedence root stays first");
    }

    #[test]
    fn codex_discovery_flag_override_beats_env_and_default() {
        let env = FakeEnv::new(Some(PathBuf::from("/home/u"))).with("CODEX_HOME", "/env/codex");
        let root = discover_codex_memory_root_with_env(Some(Path::new("/flag/codex")), &env)
            .expect("discovery ok")
            .expect("some root");
        assert_eq!(root.path, PathBuf::from("/flag/codex"));
        assert_eq!(root.source, DiscoverySource::FlagOverride);
    }

    #[test]
    fn codex_discovery_env_beats_default() {
        let env = FakeEnv::new(Some(PathBuf::from("/home/u"))).with("CODEX_HOME", "/env/codex");
        let root = discover_codex_memory_root_with_env(None, &env).expect("discovery ok").expect("some root");
        assert_eq!(root.path, PathBuf::from("/env/codex/memories"));
        assert_eq!(root.source, DiscoverySource::EnvVar);
    }

    #[test]
    fn codex_discovery_falls_back_to_default() {
        let env = FakeEnv::new(Some(PathBuf::from("/home/u")));
        let root = discover_codex_memory_root_with_env(None, &env).expect("discovery ok").expect("some root");
        assert_eq!(root.path, PathBuf::from("/home/u/.codex/memories"));
        assert_eq!(root.source, DiscoverySource::Default);
    }
}
