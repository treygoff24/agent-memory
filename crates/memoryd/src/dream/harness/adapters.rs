#[cfg(any(test, feature = "dev-fixtures"))]
use std::collections::BTreeMap;
use std::{
    ffi::OsString,
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    time::Duration,
};

#[cfg(any(test, feature = "dev-fixtures"))]
use sha2::{Digest, Sha256};
use tokio::sync::OnceCell;

use crate::protocol::PromptTransport;

use super::super::error::HarnessCliError;
use super::auth::{auth_probe_any, auth_probe_candidate, probe_external_auth, AuthProbeCandidate, AuthProbeResult};
use super::env::{AdapterEnv, MinimalEnvironment, CLAUDE_ENV_ALLOWLIST, CODEX_ENV_ALLOWLIST};
#[cfg(any(test, feature = "dev-fixtures"))]
use super::process::validate_json_if_expected;
use super::process::{
    default_scratch_root, find_executable, path_display, run_hardened_command, HardenedCommand, HarnessCommandPlan,
    DEFAULT_KILL_GRACE,
};

pub type HarnessFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait HarnessCli: Send + Sync {
    fn name(&self) -> &'static str;

    fn prompt_transport(&self) -> PromptTransport;

    fn is_installed(&self) -> bool;

    fn auth_probe(&self) -> HarnessFuture<'_, AuthProbeResult>;

    /// Default: authenticated iff the auth probe succeeds. Implementors with a
    /// cheaper or fixed answer (test fixtures, the unselected placeholder) override.
    fn is_authenticated(&self) -> HarnessFuture<'_, Result<bool, HarnessCliError>> {
        Box::pin(async move { Ok(self.auth_probe().await.is_ok()) })
    }

    fn complete<'a>(
        &'a self,
        prompt: &'a str,
        expect_json: bool,
        timeout: Duration,
    ) -> HarnessFuture<'a, Result<String, HarnessCliError>>;
}

#[cfg(any(test, feature = "dev-fixtures"))]
#[derive(Debug, Clone, Default)]
pub struct EchoCli {
    canned_outputs_by_prompt_hash: BTreeMap<String, String>,
}

#[cfg(any(test, feature = "dev-fixtures"))]
impl EchoCli {
    pub fn from_prompt_outputs<const N: usize>(outputs: [(&str, &str); N]) -> Self {
        let canned_outputs_by_prompt_hash =
            outputs.into_iter().map(|(prompt, output)| (prompt_hash(prompt), output.to_owned())).collect();

        Self { canned_outputs_by_prompt_hash }
    }
}

#[cfg(any(test, feature = "dev-fixtures"))]
impl HarnessCli for EchoCli {
    fn name(&self) -> &'static str {
        "echo"
    }

    fn prompt_transport(&self) -> PromptTransport {
        PromptTransport::Stdin
    }

    fn is_installed(&self) -> bool {
        true
    }

    fn auth_probe(&self) -> HarnessFuture<'_, AuthProbeResult> {
        Box::pin(async { AuthProbeResult::Ok })
    }

    fn is_authenticated(&self) -> HarnessFuture<'_, Result<bool, HarnessCliError>> {
        Box::pin(async { Ok(true) })
    }

    fn complete<'a>(
        &'a self,
        prompt: &'a str,
        expect_json: bool,
        _timeout: Duration,
    ) -> HarnessFuture<'a, Result<String, HarnessCliError>> {
        Box::pin(async move {
            let output = self.canned_outputs_by_prompt_hash.get(&prompt_hash(prompt)).cloned().ok_or_else(|| {
                HarnessCliError::SubprocessExit {
                    code: Some(1),
                    stderr_tail: "echo fixture has no canned output for prompt hash".to_owned(),
                }
            })?;
            validate_json_if_expected(output, expect_json)
        })
    }
}

#[cfg(any(test, feature = "dev-fixtures"))]
fn prompt_hash(prompt: &str) -> String {
    hex::encode(Sha256::digest(prompt.as_bytes()))
}

/// Maximum number of sibling `~/.claude-*` profile directories scanned when no
/// explicit `CLAUDE_CONFIG_DIR` is set. Bounds the auth-probe budget.
const MAX_CLAUDE_PROFILE_CANDIDATES: usize = 8;

#[derive(Debug, Default)]
pub struct ClaudeCodeCli {
    path_env: Option<OsString>,
    /// Resolved Claude profile, computed once per instance so the auth probe and
    /// the dream `complete()` call agree on the same profile within a run.
    resolution: OnceCell<ClaudeResolution>,
}

/// Outcome of resolving which Claude profile (`CLAUDE_CONFIG_DIR`) the daemon
/// should use for the auth probe and dream completion.
#[derive(Debug, Clone)]
struct ClaudeResolution {
    /// Profile dir to inject as `CLAUDE_CONFIG_DIR` for completion. `None` means
    /// forward the ambient environment — either the operator set an explicit
    /// `CLAUDE_CONFIG_DIR`, or nothing resolved (in which case completion never
    /// runs because the probe is not `Ok`).
    config_dir: Option<PathBuf>,
    /// The auth-probe result to report; already reflects the resolved profile.
    probe_result: AuthProbeResult,
}

/// Filesystem/environment inputs for [`resolve_config_dir`], gathered once so the
/// resolution policy itself stays pure and unit-testable.
struct ClaudeResolveInputs {
    /// Explicit operator-set `CLAUDE_CONFIG_DIR`, if any.
    explicit_config_dir: Option<PathBuf>,
    /// Default config dir (`$HOME/.claude`), or `None` when `$HOME` is unset.
    default_config_dir: Option<PathBuf>,
    /// Sibling `~/.claude-*` profile dirs that carry an auth artifact.
    sibling_config_dirs: Vec<PathBuf>,
}

impl ClaudeCodeCli {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_path_env(path_env: OsString) -> Self {
        Self { path_env: Some(path_env), resolution: OnceCell::new() }
    }

    pub fn command(&self, _expect_json: bool) -> HarnessCommandPlan {
        HarnessCommandPlan {
            program: "claude".to_owned(),
            args: vec!["--print".to_owned()],
            prompt_transport: PromptTransport::Stdin,
        }
    }

    fn auth_probe_candidates(&self) -> Vec<AuthProbeCandidate> {
        vec![
            auth_probe_candidate("claude", &["auth", "status"]),
            auth_probe_candidate("claude", &["config", "get", "auth.user"]),
        ]
    }

    /// Run the per-directory command-candidate auth probe (`auth status`, then
    /// the legacy `config get auth.user` only on an unsupported-command surface)
    /// with `CLAUDE_CONFIG_DIR` set to `config_dir`, or the ambient env when
    /// `config_dir` is `None`.
    async fn probe_dir(&self, config_dir: Option<PathBuf>) -> AuthProbeResult {
        auth_probe_any(self.auth_probe_candidates(), self.path_env.clone(), CLAUDE_ENV_ALLOWLIST, config_dir).await
    }

    async fn resolved(&self) -> &ClaudeResolution {
        self.resolution.get_or_init(|| self.resolve_uncached()).await
    }

    async fn resolve_uncached(&self) -> ClaudeResolution {
        if !self.is_installed() {
            return ClaudeResolution {
                config_dir: None,
                probe_result: AuthProbeResult::CliMissing {
                    which: "claude",
                    path: path_display(self.path_env.as_deref()),
                },
            };
        }

        let home = home_dir();
        let inputs = ClaudeResolveInputs {
            explicit_config_dir: explicit_claude_config_dir(),
            default_config_dir: home.as_ref().map(|home| home.join(".claude")),
            sibling_config_dirs: home.as_deref().map(enumerate_sibling_config_dirs).unwrap_or_default(),
        };
        resolve_config_dir(inputs, |dir| self.probe_dir(dir)).await
    }

    fn completion_adapter_env(&self, config_dir: Option<PathBuf>) -> AdapterEnv {
        AdapterEnv {
            installed: self.is_installed(),
            path_env: self.path_env.clone(),
            allowlist: CLAUDE_ENV_ALLOWLIST,
            config_dir_override: config_dir,
        }
    }
}

impl HarnessCli for ClaudeCodeCli {
    fn name(&self) -> &'static str {
        "claude"
    }

    fn prompt_transport(&self) -> PromptTransport {
        PromptTransport::Stdin
    }

    fn is_installed(&self) -> bool {
        find_executable("claude", self.path_env.as_deref()).is_some()
    }

    fn auth_probe(&self) -> HarnessFuture<'_, AuthProbeResult> {
        Box::pin(async move { self.resolved().await.probe_result.clone() })
    }

    fn complete<'a>(
        &'a self,
        prompt: &'a str,
        expect_json: bool,
        timeout: Duration,
    ) -> HarnessFuture<'a, Result<String, HarnessCliError>> {
        Box::pin(async move {
            let config_dir = self.resolved().await.config_dir.clone();
            complete_for_adapter(
                self.completion_adapter_env(config_dir),
                self.command(expect_json),
                prompt,
                PassRunOptions { expect_json, timeout },
            )
            .await
        })
    }
}

fn explicit_claude_config_dir() -> Option<PathBuf> {
    std::env::var_os("CLAUDE_CONFIG_DIR").filter(|value| !value.is_empty()).map(PathBuf::from)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").filter(|value| !value.is_empty()).map(PathBuf::from)
}

/// Enumerate sibling Claude profile directories: existing `~/.claude-*` dirs
/// (excluding the default `~/.claude`) that contain a `.credentials.json` auth
/// artifact. Sorted for deterministic ordering, capped, and never recursive.
/// Skips non-profile dirs (`~/.claude-shared`, empty scaffolds) that lack creds.
fn enumerate_sibling_config_dirs(home: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(home) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name();
            if !name.to_str()?.starts_with(".claude-") {
                return None;
            }
            let path = entry.path();
            (path.is_dir() && path.join(".credentials.json").exists()).then_some(path)
        })
        .collect();
    dirs.sort();
    dirs.truncate(MAX_CLAUDE_PROFILE_CANDIDATES);
    dirs
}

/// Resolve which Claude profile to use, probing candidates with `probe`.
///
/// Precedence: an explicit operator-set `CLAUDE_CONFIG_DIR` is honored fail-closed
/// (probe it; never scan). Otherwise the default `~/.claude` is tried; on a normal
/// auth failure there, sibling profiles are scanned. Exactly one authenticated
/// sibling wins; multiple authenticated siblings are ambiguous and fail loudly so
/// the daemon never silently dreams against the wrong account.
///
/// This is deliberately distinct from [`auth_probe_any`], which is terminal on a
/// normal auth failure (it only continues on an unsupported-command surface). The
/// profile loop must instead continue past a logged-out profile to the next one.
async fn resolve_config_dir<F, Fut>(inputs: ClaudeResolveInputs, mut probe: F) -> ClaudeResolution
where
    F: FnMut(Option<PathBuf>) -> Fut,
    Fut: Future<Output = AuthProbeResult>,
{
    // 1. Explicit operator override: probe the ambient (already-forwarded) env,
    //    fail-closed — the operator chose this profile, so never fall through to
    //    scanning if it does not authenticate.
    if inputs.explicit_config_dir.is_some() {
        return ClaudeResolution { config_dir: None, probe_result: probe(None).await };
    }

    // 2. Default ~/.claude.
    let Some(default_dir) = inputs.default_config_dir else {
        return ClaudeResolution { config_dir: None, probe_result: probe(None).await };
    };
    let default_result = probe(Some(default_dir.clone())).await;
    match &default_result {
        AuthProbeResult::Ok => {
            return ClaudeResolution { config_dir: Some(default_dir), probe_result: AuthProbeResult::Ok };
        }
        // Only a normal auth failure warrants scanning siblings; a missing binary,
        // timeout, or hard error stops here with the typed result intact.
        AuthProbeResult::AuthFailed { .. } => {}
        _ => return ClaudeResolution { config_dir: None, probe_result: default_result.clone() },
    }

    // 3. Scan sibling profiles for authenticated ones.
    let mut authenticated: Vec<PathBuf> = Vec::new();
    for dir in inputs.sibling_config_dirs {
        match probe(Some(dir.clone())).await {
            AuthProbeResult::Ok => authenticated.push(dir),
            // Preserve a stop-the-world signal; do not mask it as "no profile".
            AuthProbeResult::Timeout => {
                return ClaudeResolution { config_dir: None, probe_result: AuthProbeResult::Timeout };
            }
            AuthProbeResult::Error { message } => {
                return ClaudeResolution { config_dir: None, probe_result: AuthProbeResult::Error { message } };
            }
            AuthProbeResult::AuthFailed { .. } | AuthProbeResult::CliMissing { .. } => {}
        }
    }

    match authenticated.len() {
        0 => ClaudeResolution { config_dir: None, probe_result: default_result },
        1 => ClaudeResolution { config_dir: Some(authenticated.remove(0)), probe_result: AuthProbeResult::Ok },
        _ => {
            let names = authenticated.iter().map(|path| path.display().to_string()).collect::<Vec<_>>().join(", ");
            ClaudeResolution {
                config_dir: None,
                probe_result: AuthProbeResult::Error {
                    message: format!(
                        "multiple authenticated Claude profiles found ({names}); set CLAUDE_CONFIG_DIR in the daemon environment (e.g. the launchd plist) to choose one"
                    ),
                },
            }
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CodexCli {
    path_env: Option<OsString>,
}

impl CodexCli {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_path_env(path_env: OsString) -> Self {
        Self { path_env: Some(path_env) }
    }

    pub fn command(&self, _expect_json: bool) -> HarnessCommandPlan {
        // The hardened process uses an empty scratch directory. Codex requires
        // this opt-out there, and its `--json` mode is event JSONL rather than
        // the final response JSON expected by callers.
        let args = vec!["exec".to_owned(), "--skip-git-repo-check".to_owned(), "-".to_owned()];

        HarnessCommandPlan { program: "codex".to_owned(), args, prompt_transport: PromptTransport::Stdin }
    }

    fn auth_probe_candidates(&self) -> Vec<AuthProbeCandidate> {
        vec![auth_probe_candidate("codex", &["login", "status"]), auth_probe_candidate("codex", &["auth", "status"])]
    }

    fn adapter_env(&self) -> AdapterEnv {
        AdapterEnv {
            installed: self.is_installed(),
            path_env: self.path_env.clone(),
            allowlist: CODEX_ENV_ALLOWLIST,
            config_dir_override: None,
        }
    }
}

impl HarnessCli for CodexCli {
    fn name(&self) -> &'static str {
        "codex"
    }

    fn prompt_transport(&self) -> PromptTransport {
        PromptTransport::Stdin
    }

    fn is_installed(&self) -> bool {
        find_executable("codex", self.path_env.as_deref()).is_some()
    }

    fn auth_probe(&self) -> HarnessFuture<'_, AuthProbeResult> {
        Box::pin(probe_external_auth("codex", self.adapter_env(), self.auth_probe_candidates()))
    }

    fn complete<'a>(
        &'a self,
        prompt: &'a str,
        expect_json: bool,
        timeout: Duration,
    ) -> HarnessFuture<'a, Result<String, HarnessCliError>> {
        Box::pin(complete_for_adapter(
            self.adapter_env(),
            self.command(expect_json),
            prompt,
            PassRunOptions { expect_json, timeout },
        ))
    }
}

#[derive(Debug, Clone, Copy)]
struct PassRunOptions {
    expect_json: bool,
    timeout: Duration,
}

/// Shared `HarnessCli::complete` body for real external adapters: refuse with
/// `NotInstalled` when the binary is absent, otherwise run the adapter's command
/// under a minimal environment scoped to the adapter's allowlist.
async fn complete_for_adapter(
    env: AdapterEnv,
    plan: HarnessCommandPlan,
    prompt: &str,
    options: PassRunOptions,
) -> Result<String, HarnessCliError> {
    if !env.installed {
        return Err(HarnessCliError::NotInstalled);
    }
    let environment = env.min_env();
    complete_external(plan, environment, prompt, options).await
}

async fn complete_external(
    plan: HarnessCommandPlan,
    environment: MinimalEnvironment,
    prompt: &str,
    options: PassRunOptions,
) -> Result<String, HarnessCliError> {
    let output = run_hardened_command(
        HardenedCommand {
            program: PathBuf::from(plan.program),
            args: plan.args,
            prompt_transport: plan.prompt_transport,
            expect_json: options.expect_json,
            timeout: options.timeout,
            kill_grace: DEFAULT_KILL_GRACE,
            scratch_root: default_scratch_root(),
            environment,
            redact_stderr: true,
        },
        prompt,
    )
    .await?;

    Ok(output.stdout)
}

#[cfg(test)]
mod tests {
    use super::super::auth::auth_probe_any_with_runner;
    use super::*;

    fn auth_failed() -> AuthProbeResult {
        AuthProbeResult::AuthFailed { exit_code: Some(1), stderr_tail: "loggedIn:false".to_owned() }
    }

    fn dir_basename(dir: &Option<PathBuf>) -> Option<String> {
        dir.as_deref()?.file_name()?.to_str().map(str::to_owned)
    }

    #[tokio::test]
    async fn auth_probe_any_does_not_fallback_on_non_command_unrecognized_auth_failure() {
        let candidates = ClaudeCodeCli::new().auth_probe_candidates();

        let mut calls: Vec<HarnessCommandPlan> = Vec::new();
        let result = auth_probe_any_with_runner(candidates, |plan| {
            calls.push(plan.clone());
            async move {
                AuthProbeResult::AuthFailed {
                    exit_code: Some(1),
                    stderr_tail: "auth failed: unrecognized account token; run claude login".to_owned(),
                }
            }
        })
        .await;

        assert!(!result.is_ok(), "auth failure containing non-command 'unrecognized' must not fall back to legacy");
        assert_eq!(calls.len(), 1, "should only call preferred candidate");
        assert_eq!(calls[0].args, ["auth", "status"]);
    }

    #[tokio::test]
    async fn resolve_uses_default_when_default_authenticates() {
        let home = PathBuf::from("/home/u");
        let inputs = ClaudeResolveInputs {
            explicit_config_dir: None,
            default_config_dir: Some(home.join(".claude")),
            sibling_config_dirs: vec![home.join(".claude-work")],
        };
        let mut calls = 0usize;
        let resolution = resolve_config_dir(inputs, |_dir| {
            calls += 1;
            async move { AuthProbeResult::Ok }
        })
        .await;

        assert!(resolution.probe_result.is_ok());
        assert_eq!(resolution.config_dir.as_deref(), Some(home.join(".claude").as_path()));
        assert_eq!(calls, 1, "an authenticated default must not scan siblings");
    }

    #[tokio::test]
    async fn resolve_picks_single_authenticated_sibling_when_default_logged_out() {
        let home = PathBuf::from("/home/u");
        let inputs = ClaudeResolveInputs {
            explicit_config_dir: None,
            default_config_dir: Some(home.join(".claude")),
            sibling_config_dirs: vec![home.join(".claude-personal"), home.join(".claude-work")],
        };
        let resolution = resolve_config_dir(inputs, |dir| {
            let authenticated = dir_basename(&dir).as_deref() == Some(".claude-personal");
            async move {
                if authenticated {
                    AuthProbeResult::Ok
                } else {
                    auth_failed()
                }
            }
        })
        .await;

        assert!(resolution.probe_result.is_ok());
        assert_eq!(resolution.config_dir.as_deref(), Some(home.join(".claude-personal").as_path()));
    }

    #[tokio::test]
    async fn resolve_fails_loudly_when_multiple_siblings_authenticate() {
        let home = PathBuf::from("/home/u");
        let inputs = ClaudeResolveInputs {
            explicit_config_dir: None,
            default_config_dir: Some(home.join(".claude")),
            sibling_config_dirs: vec![home.join(".claude-personal"), home.join(".claude-work")],
        };
        let resolution = resolve_config_dir(inputs, |dir| {
            let is_sibling = dir_basename(&dir).is_some_and(|name| name != ".claude");
            async move {
                if is_sibling {
                    AuthProbeResult::Ok
                } else {
                    auth_failed()
                }
            }
        })
        .await;

        assert!(!resolution.probe_result.is_ok());
        assert!(resolution.config_dir.is_none(), "ambiguity must not silently pick a profile");
        match resolution.probe_result {
            AuthProbeResult::Error { message } => {
                assert!(message.contains("multiple authenticated Claude profiles"), "{message}");
                assert!(message.contains("CLAUDE_CONFIG_DIR"), "{message}");
            }
            other => panic!("expected ambiguity Error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn resolve_explicit_env_is_fail_closed_and_never_scans() {
        let home = PathBuf::from("/home/u");
        let inputs = ClaudeResolveInputs {
            explicit_config_dir: Some(home.join(".claude-work")),
            default_config_dir: Some(home.join(".claude")),
            sibling_config_dirs: vec![home.join(".claude-personal")],
        };
        let mut calls: Vec<Option<PathBuf>> = Vec::new();
        let resolution = resolve_config_dir(inputs, |dir| {
            calls.push(dir);
            async move { auth_failed() }
        })
        .await;

        assert!(!resolution.probe_result.is_ok(), "explicit-env auth failure must not fall through to scanning");
        assert!(resolution.config_dir.is_none(), "explicit env forwards ambient; no injected override");
        assert_eq!(calls, vec![None], "explicit env probes the ambient env exactly once and never scans");
    }

    #[tokio::test]
    async fn resolve_default_timeout_does_not_scan_siblings() {
        let home = PathBuf::from("/home/u");
        let inputs = ClaudeResolveInputs {
            explicit_config_dir: None,
            default_config_dir: Some(home.join(".claude")),
            sibling_config_dirs: vec![home.join(".claude-personal")],
        };
        let mut calls = 0usize;
        let resolution = resolve_config_dir(inputs, |_dir| {
            calls += 1;
            async move { AuthProbeResult::Timeout }
        })
        .await;

        assert!(matches!(resolution.probe_result, AuthProbeResult::Timeout));
        assert_eq!(calls, 1, "a timeout on the default profile must not scan siblings");
    }

    #[test]
    fn enumerate_sibling_config_dirs_filters_and_sorts() {
        let home = tempfile::tempdir().expect("home");
        let root = home.path();
        for profile in [".claude-work", ".claude-personal"] {
            std::fs::create_dir_all(root.join(profile)).expect("profile dir");
            std::fs::write(root.join(profile).join(".credentials.json"), "{}").expect("creds");
        }
        // Default dir (wrong prefix), a credential-less scaffold, and a plain file
        // must all be excluded.
        std::fs::create_dir_all(root.join(".claude")).expect("default dir");
        std::fs::write(root.join(".claude").join(".credentials.json"), "{}").expect("default creds");
        std::fs::create_dir_all(root.join(".claude-shared")).expect("shared dir");
        std::fs::write(root.join(".claude-not-a-dir"), "x").expect("decoy file");

        let names: Vec<String> = enumerate_sibling_config_dirs(root)
            .iter()
            .map(|dir| dir.file_name().unwrap().to_str().unwrap().to_owned())
            .collect();

        assert_eq!(names, vec![".claude-personal".to_owned(), ".claude-work".to_owned()]);
    }
}
