use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    future::Future,
    io::{Read, Write},
    path::{Path, PathBuf},
    pin::Pin,
    process::{Command, Stdio},
    thread,
    time::Duration,
};

use sha2::{Digest, Sha256};
use tokio::sync::OnceCell;

use crate::protocol::PromptTransport;

use super::error::{HarnessCliError, JsonStage};

pub const DOCUMENTED_ENV_ALLOWLIST: &[&str] = &[
    "ANTHROPIC_API_KEY",
    "CLAUDE_CONFIG_DIR",
    "CODEX_HOME",
    "GEMINI_API_KEY",
    "GOOGLE_API_KEY",
    "HOME",
    "OPENAI_API_KEY",
    "PATH",
    "TERM",
    "USER",
];
// `USER` is required: Claude's claude.ai auth token lives in the macOS login
// keychain, and the keychain lookup keys off `USER`. Without it `claude auth
// status` reports `loggedIn:false` even with a valid `CLAUDE_CONFIG_DIR`, so the
// hardened dream subprocess could never authenticate. `USER` is public identity,
// not a credential, so forwarding it does not weaken the no-secret-leakage intent.
pub const CLAUDE_ENV_ALLOWLIST: &[&str] = &["ANTHROPIC_API_KEY", "CLAUDE_CONFIG_DIR", "HOME", "PATH", "TERM", "USER"];
pub const CODEX_ENV_ALLOWLIST: &[&str] = &["CODEX_HOME", "HOME", "OPENAI_API_KEY", "PATH", "TERM"];

const STDOUT_CAPTURE_LIMIT_BYTES: usize = 16 * 1024 * 1024;
const STDERR_TAIL_LIMIT_BYTES: usize = 64 * 1024;
const AUTH_DIAGNOSTIC_TAIL_LIMIT_BYTES: usize = 4 * 1024;
const DEFAULT_KILL_GRACE: Duration = Duration::from_secs(2);
const AUTH_PROBE_TIMEOUT: Duration = Duration::from_secs(10);

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthProbeResult {
    Ok,
    CliMissing { which: &'static str, path: String },
    AuthFailed { exit_code: Option<i32>, stderr_tail: String },
    Timeout,
    Error { message: String },
}

impl AuthProbeResult {
    pub fn is_ok(&self) -> bool {
        matches!(self, Self::Ok)
    }

    pub fn operator_message(&self, which: &'static str) -> String {
        match self {
            Self::Ok => format!("{which} CLI: ✓ authenticated"),
            Self::CliMissing { .. } => {
                format!("{which} CLI: ✗ not on PATH (dreams disabled for {which}); try `which {which}` in the daemon environment")
            }
            Self::AuthFailed { exit_code, stderr_tail } => {
                format!("{which} CLI: ✗ auth probe failed (exit={exit_code:?}): {stderr_tail}")
            }
            Self::Timeout => format!("{which} CLI: ✗ auth probe timed out"),
            Self::Error { message } => format!("{which} CLI: ✗ auth probe error: {message}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessCommandPlan {
    pub program: String,
    pub args: Vec<String>,
    pub prompt_transport: PromptTransport,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AuthProbeCandidate {
    plan: HarnessCommandPlan,
    unsupported_markers: &'static [&'static str],
}

const AUTH_DIAGNOSTIC_SUMMARY_MAX_CHARS: usize = 4096;
const AUTH_UNSUPPORTED_COMMAND_MARKERS: &[&str] = &[
    "unknown command",
    "unknown subcommand",
    "unrecognized command",
    "unrecognized subcommand",
    "invalid command",
    "invalid subcommand",
    "unsupported command",
    "unsupported subcommand",
];
const AUTH_FAILURE_MARKERS: &[&str] = &[
    "auth failed",
    "invalid credential",
    "invalid key",
    "invalid token",
    "not authenticated",
    "not logged in",
    "session expired",
    "unrecognized account",
    "unrecognized token",
];

fn auth_probe_candidate(program: &str, args: &[&str]) -> AuthProbeCandidate {
    AuthProbeCandidate {
        plan: HarnessCommandPlan {
            program: program.to_owned(),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            prompt_transport: PromptTransport::Stdin,
        },
        unsupported_markers: AUTH_UNSUPPORTED_COMMAND_MARKERS,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinimalEnvironment {
    values: BTreeMap<String, OsString>,
}

impl MinimalEnvironment {
    pub fn from_current(path_env: Option<OsString>) -> Self {
        let pairs = DOCUMENTED_ENV_ALLOWLIST
            .iter()
            .filter_map(|key| std::env::var_os(key).map(|value| ((*key).to_owned(), value)));
        let mut environment = Self { values: pairs.collect() };

        if let Some(path_env) = path_env {
            environment.values.insert("PATH".to_owned(), path_env);
        }
        environment.values.insert("TERM".to_owned(), OsString::from("dumb"));
        environment.retain_documented_keys_only();
        environment
    }

    pub fn from_pairs<K, V, I>(pairs: I) -> Self
    where
        K: Into<String>,
        V: Into<OsString>,
        I: IntoIterator<Item = (K, V)>,
    {
        let mut environment =
            Self { values: pairs.into_iter().map(|(key, value)| (key.into(), value.into())).collect() };
        environment.values.insert("TERM".to_owned(), OsString::from("dumb"));
        environment.retain_documented_keys_only();
        environment
    }

    pub fn retain_documented_keys_only(&mut self) {
        self.retain_keys(DOCUMENTED_ENV_ALLOWLIST);
    }

    pub fn retain_keys(&mut self, allowlist: &[&str]) {
        self.values.retain(|key, _| allowlist.contains(&key.as_str()));
        self.values.insert("TERM".to_owned(), OsString::from("dumb"));
    }

    pub fn for_adapter(path_env: Option<OsString>, allowlist: &[&str]) -> Self {
        let mut environment = Self::from_current(path_env);
        environment.retain_keys(allowlist);
        environment
    }

    /// Like [`Self::for_adapter`], but injects explicit key/value overrides after
    /// allowlist filtering. Overrides whose key is not in `allowlist` are
    /// dropped, so this can never widen the hardened subprocess environment
    /// beyond the adapter's allowlist (e.g. only `CLAUDE_CONFIG_DIR` is injected
    /// for the Claude adapter).
    pub fn for_adapter_with_overrides(
        path_env: Option<OsString>,
        allowlist: &[&str],
        overrides: &[(&str, OsString)],
    ) -> Self {
        let mut environment = Self::for_adapter(path_env, allowlist);
        for (key, value) in overrides {
            if allowlist.contains(key) {
                environment.values.insert((*key).to_owned(), value.clone());
            }
        }
        environment
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.values.keys().map(String::as_str)
    }

    fn apply_to(&self, command: &mut Command) {
        command.env_clear();
        for (key, value) in &self.values {
            command.env(key, value);
        }
    }
}

#[derive(Debug, Clone)]
pub struct HardenedCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub prompt_transport: PromptTransport,
    pub expect_json: bool,
    pub timeout: Duration,
    pub kill_grace: Duration,
    pub scratch_root: PathBuf,
    pub environment: MinimalEnvironment,
    pub redact_stderr: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HardenedOutput {
    pub stdout: String,
    pub stderr_tail: String,
    pub status_code: Option<i32>,
}

pub async fn run_hardened_command(command: HardenedCommand, prompt: &str) -> Result<HardenedOutput, HarnessCliError> {
    let prompt = prompt.to_owned();
    tokio::task::spawn_blocking(move || run_hardened_command_blocking(command, &prompt))
        .await
        .map_err(|error| std::io::Error::other(format!("hardened command task failed: {error}")))?
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

    pub fn command(&self, expect_json: bool) -> HarnessCommandPlan {
        let args = if expect_json {
            vec!["exec".to_owned(), "--json".to_owned(), "-".to_owned()]
        } else {
            vec!["exec".to_owned(), "-".to_owned()]
        };

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

/// External-adapter execution context shared by the real CLI harnesses: whether
/// the binary is present, the PATH override used to find and run it, and the
/// environment-variable allowlist scoping its hardened subprocess.
struct AdapterEnv {
    installed: bool,
    path_env: Option<OsString>,
    allowlist: &'static [&'static str],
    /// When set, inject `CLAUDE_CONFIG_DIR=<dir>` (allowlist-filtered) into the
    /// hardened subprocess so the auth probe and completion run against the same
    /// resolved Claude profile. `None` forwards the ambient environment.
    config_dir_override: Option<PathBuf>,
}

impl AdapterEnv {
    fn min_env(&self) -> MinimalEnvironment {
        match &self.config_dir_override {
            Some(dir) => MinimalEnvironment::for_adapter_with_overrides(
                self.path_env.clone(),
                self.allowlist,
                &[("CLAUDE_CONFIG_DIR", dir.clone().into_os_string())],
            ),
            None => MinimalEnvironment::for_adapter(self.path_env.clone(), self.allowlist),
        }
    }
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

async fn auth_probe(
    plan: HarnessCommandPlan,
    path_env: Option<OsString>,
    env_allowlist: &[&str],
    config_dir: Option<PathBuf>,
) -> AuthProbeResult {
    let environment = match config_dir {
        Some(dir) => MinimalEnvironment::for_adapter_with_overrides(
            path_env,
            env_allowlist,
            &[("CLAUDE_CONFIG_DIR", dir.into_os_string())],
        ),
        None => MinimalEnvironment::for_adapter(path_env, env_allowlist),
    };
    let result = run_hardened_command(
        HardenedCommand {
            program: PathBuf::from(plan.program),
            args: plan.args,
            prompt_transport: plan.prompt_transport,
            expect_json: false,
            timeout: AUTH_PROBE_TIMEOUT,
            kill_grace: DEFAULT_KILL_GRACE,
            scratch_root: default_scratch_root(),
            environment,
            redact_stderr: false,
        },
        "",
    )
    .await;

    match result {
        Ok(_) => AuthProbeResult::Ok,
        Err(HarnessCliError::SubprocessExit { code, stderr_tail }) => {
            AuthProbeResult::AuthFailed { exit_code: code, stderr_tail }
        }
        Err(HarnessCliError::Timeout { .. }) => AuthProbeResult::Timeout,
        Err(error) => AuthProbeResult::Error { message: error.to_string() },
    }
}

/// Shared `HarnessCli::auth_probe` body for real external adapters: short-circuit
/// with `CliMissing` when the binary is absent, otherwise race the adapter's auth
/// candidates. `which` is the binary name surfaced in the missing diagnostic.
async fn probe_external_auth(
    which: &'static str,
    env: AdapterEnv,
    candidates: Vec<AuthProbeCandidate>,
) -> AuthProbeResult {
    if !env.installed {
        return AuthProbeResult::CliMissing { which, path: path_display(env.path_env.as_deref()) };
    }
    auth_probe_any(candidates, env.path_env, env.allowlist, None).await
}

async fn auth_probe_any(
    candidates: Vec<AuthProbeCandidate>,
    path_env: Option<OsString>,
    env_allowlist: &[&str],
    config_dir: Option<PathBuf>,
) -> AuthProbeResult {
    auth_probe_any_with_runner(candidates, |plan| {
        let path_env = path_env.clone();
        let config_dir = config_dir.clone();
        async move { auth_probe(plan, path_env, env_allowlist, config_dir).await }
    })
    .await
}

/// Prefer the current auth command, and invoke legacy candidates only when the
/// previous command failed because that command surface is unsupported.
async fn auth_probe_any_with_runner<F, Fut>(candidates: Vec<AuthProbeCandidate>, mut runner: F) -> AuthProbeResult
where
    F: FnMut(HarnessCommandPlan) -> Fut,
    Fut: std::future::Future<Output = AuthProbeResult>,
{
    let mut unsupported = Vec::new();
    for candidate in candidates {
        let AuthProbeCandidate { plan, unsupported_markers } = candidate;
        let command_label = command_label(&plan);
        match runner(plan).await {
            AuthProbeResult::Ok => return AuthProbeResult::Ok,
            AuthProbeResult::AuthFailed { stderr_tail, .. }
                if is_unsupported_auth_surface(&stderr_tail, unsupported_markers) =>
            {
                unsupported.push(format!("{command_label}: {stderr_tail}"));
                continue;
            }
            AuthProbeResult::AuthFailed { exit_code, stderr_tail } => {
                return AuthProbeResult::AuthFailed {
                    exit_code,
                    stderr_tail: format!("{command_label} failed: {stderr_tail}"),
                };
            }
            AuthProbeResult::Timeout => {
                return AuthProbeResult::Timeout;
            }
            AuthProbeResult::Error { message } => {
                return AuthProbeResult::Error { message: format!("{command_label} error: {message}") };
            }
            AuthProbeResult::CliMissing { which, path } => return AuthProbeResult::CliMissing { which, path },
        }
    }

    AuthProbeResult::Error {
        message: format!(
            "no supported auth status command was accepted; tried {}",
            summarize_unsupported_attempts(&unsupported)
        ),
    }
}

fn command_label(plan: &HarnessCommandPlan) -> String {
    if plan.args.is_empty() {
        plan.program.clone()
    } else {
        format!("{} {}", plan.program, plan.args.join(" "))
    }
}

fn is_unsupported_auth_surface(stderr_tail: &str, markers: &[&str]) -> bool {
    let lower = stderr_tail.to_ascii_lowercase();
    markers.iter().any(|marker| lower.contains(marker))
        && !AUTH_FAILURE_MARKERS.iter().any(|marker| lower.contains(marker))
}

fn summarize_unsupported_attempts(attempts: &[String]) -> String {
    truncate_for_auth_diagnostic(&attempts.join("; "), AUTH_DIAGNOSTIC_SUMMARY_MAX_CHARS)
}

fn truncate_for_auth_diagnostic(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

fn path_display(path_env: Option<&OsStr>) -> String {
    path_env
        .map(|path| path.to_string_lossy().into_owned())
        .or_else(|| std::env::var("PATH").ok())
        .unwrap_or_else(|| "<unset>".to_owned())
}

#[cfg(any(test, feature = "dev-fixtures"))]
fn prompt_hash(prompt: &str) -> String {
    hex::encode(Sha256::digest(prompt.as_bytes()))
}

fn run_hardened_command_blocking(command: HardenedCommand, prompt: &str) -> Result<HardenedOutput, HarnessCliError> {
    std::fs::create_dir_all(&command.scratch_root)?;
    let scratch_dir = tempfile::Builder::new().prefix("run-").tempdir_in(&command.scratch_root)?;
    let expect_json = command.expect_json;
    let timeout = command.timeout;
    let capture =
        HardenedCaptureOptions { timeout, kill_grace: command.kill_grace, redact_stderr: command.redact_stderr };
    let (mut child, handles) = spawn_hardened_child(command, prompt, scratch_dir.path())?;
    let output = capture_hardened_child(&mut child, handles, capture)?;

    finalize_hardened_output(output, expect_json, timeout)
}

struct SpawnedHardenedChild {
    stdout_reader: thread::JoinHandle<std::io::Result<Vec<u8>>>,
    stderr_reader: thread::JoinHandle<std::io::Result<Vec<u8>>>,
    stdin_writer: Option<thread::JoinHandle<std::io::Result<()>>>,
}

#[derive(Clone, Copy)]
struct HardenedCaptureOptions {
    timeout: Duration,
    kill_grace: Duration,
    redact_stderr: bool,
}

struct CapturedHardenedChild {
    stdout: String,
    stderr_tail: String,
    auth_stdout_tail: Option<String>,
    status_code: Option<i32>,
    stdin_write_result: Result<(), HarnessCliError>,
    timed_out: bool,
    status_success: bool,
}

fn spawn_hardened_child(
    command: HardenedCommand,
    prompt: &str,
    scratch_dir: &Path,
) -> Result<(std::process::Child, SpawnedHardenedChild), HarnessCliError> {
    let mut args = command.args;
    if command.prompt_transport == PromptTransport::Argv {
        args.push(prompt.to_owned());
    }

    let mut process = Command::new(command.program);
    process
        .args(&args)
        .current_dir(scratch_dir)
        .stdin(match command.prompt_transport {
            PromptTransport::Stdin => Stdio::piped(),
            PromptTransport::Argv => Stdio::null(),
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command.environment.apply_to(&mut process);

    let mut child = process.spawn()?;
    let stdout = child.stdout.take().expect("stdout was piped");
    let stderr = child.stderr.take().expect("stderr was piped");
    let stdout_reader = thread::spawn(move || read_first_bytes(stdout, STDOUT_CAPTURE_LIMIT_BYTES));
    let stderr_reader = thread::spawn(move || read_last_bytes(stderr, STDERR_TAIL_LIMIT_BYTES));
    let stdin_writer = if command.prompt_transport == PromptTransport::Stdin {
        child.stdin.take().map(|stdin| spawn_stdin_writer(stdin, prompt.to_owned()))
    } else {
        None
    };

    Ok((child, SpawnedHardenedChild { stdout_reader, stderr_reader, stdin_writer }))
}

fn capture_hardened_child(
    child: &mut std::process::Child,
    handles: SpawnedHardenedChild,
    options: HardenedCaptureOptions,
) -> Result<CapturedHardenedChild, HarnessCliError> {
    let outcome = wait_with_timeout(child, options.timeout, options.kill_grace)?;
    let stdin_write_result = join_stdin_writer(handles.stdin_writer);
    let stdout = join_reader(handles.stdout_reader)?;
    let stderr_tail = join_reader(handles.stderr_reader)?;
    let auth_stdout_tail = (!options.redact_stderr).then(|| auth_diagnostic_tail(&stdout));
    let stdout = String::from_utf8_lossy(&stdout).into_owned();
    let stderr_tail = if options.redact_stderr {
        redacted_capture_diagnostic("stderr", &stderr_tail)
    } else {
        auth_diagnostic_tail(&stderr_tail)
    };
    let status = outcome.status;
    Ok(CapturedHardenedChild {
        stdout,
        stderr_tail,
        auth_stdout_tail,
        status_code: status.code(),
        stdin_write_result,
        timed_out: outcome.timed_out,
        status_success: status.success(),
    })
}

fn finalize_hardened_output(
    output: CapturedHardenedChild,
    expect_json: bool,
    timeout: Duration,
) -> Result<HardenedOutput, HarnessCliError> {
    if output.timed_out {
        return Err(HarnessCliError::Timeout { duration: timeout });
    }

    if !output.status_success {
        return Err(HarnessCliError::SubprocessExit {
            code: output.status_code,
            stderr_tail: auth_exit_diagnostic(output.auth_stdout_tail.as_deref(), &output.stderr_tail),
        });
    }
    if !successful_stdout_allows_stdin_error(&output.stdout, &output.stdin_write_result) {
        output.stdin_write_result?;
    }

    let stdout = validate_json_if_expected(output.stdout, expect_json)?;
    Ok(HardenedOutput { stdout, stderr_tail: output.stderr_tail, status_code: output.status_code })
}

struct WaitOutcome {
    status: std::process::ExitStatus,
    timed_out: bool,
}

fn wait_with_timeout(
    child: &mut std::process::Child,
    timeout: Duration,
    kill_grace: Duration,
) -> Result<WaitOutcome, HarnessCliError> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(WaitOutcome { status, timed_out: false });
        }

        if std::time::Instant::now() >= deadline {
            terminate_child(child)?;
            let kill_deadline = std::time::Instant::now() + kill_grace;
            while std::time::Instant::now() < kill_deadline {
                if let Some(status) = child.try_wait()? {
                    return Ok(WaitOutcome { status, timed_out: true });
                }
                thread::sleep(Duration::from_millis(10));
            }
            child.kill()?;
            let status = child.wait()?;
            return Ok(WaitOutcome { status, timed_out: true });
        }

        thread::sleep(Duration::from_millis(10));
    }
}

fn terminate_child(child: &mut std::process::Child) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        if send_sigterm(child.id()).is_ok() {
            return Ok(());
        }
    }

    child.kill()
}

#[cfg(unix)]
fn send_sigterm(pid: u32) -> std::io::Result<()> {
    const SIGTERM: i32 = 15;

    // SAFETY: `pid` comes from `std::process::Child::id` for a child process
    // we spawned, and `SIGTERM` is a plain signal number. `kill(2)` does not
    // dereference Rust pointers or retain references across the FFI boundary.
    let result = unsafe { posix_kill(pid as i32, SIGTERM) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(unix)]
unsafe extern "C" {
    #[link_name = "kill"]
    fn posix_kill(pid: i32, sig: i32) -> i32;
}

fn read_first_bytes(mut reader: impl Read, limit: usize) -> std::io::Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            return Ok(output);
        }
        let remaining = limit.saturating_sub(output.len());
        if remaining > 0 {
            output.extend_from_slice(&buffer[..read.min(remaining)]);
        }
    }
}

fn read_last_bytes(mut reader: impl Read, limit: usize) -> std::io::Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            return Ok(output);
        }
        output.extend_from_slice(&buffer[..read]);
        if output.len() > limit {
            output.drain(..output.len() - limit);
        }
    }
}

fn spawn_stdin_writer(mut stdin: std::process::ChildStdin, prompt: String) -> thread::JoinHandle<std::io::Result<()>> {
    thread::spawn(move || {
        stdin.write_all(prompt.as_bytes())?;
        stdin.flush()
    })
}

fn join_stdin_writer(handle: Option<thread::JoinHandle<std::io::Result<()>>>) -> Result<(), HarnessCliError> {
    if let Some(handle) = handle {
        handle.join().map_err(|_| std::io::Error::other("hardened command stdin writer panicked"))??;
    }
    Ok(())
}

fn successful_stdout_allows_stdin_error(stdout: &str, result: &Result<(), HarnessCliError>) -> bool {
    !stdout.trim().is_empty()
        && matches!(
            result,
            Err(HarnessCliError::Io(error)) if error.kind() == std::io::ErrorKind::BrokenPipe
        )
}

fn join_reader(handle: thread::JoinHandle<std::io::Result<Vec<u8>>>) -> Result<Vec<u8>, HarnessCliError> {
    handle.join().map_err(|_| std::io::Error::other("hardened command reader panicked"))?.map_err(HarnessCliError::Io)
}

fn validate_json_if_expected(output: String, expect_json: bool) -> Result<String, HarnessCliError> {
    if expect_json {
        serde_json::from_str::<serde_json::Value>(&output).map_err(|_| HarnessCliError::MalformedJson {
            stage: JsonStage::Parse,
            raw: redacted_capture_diagnostic("stdout", output.as_bytes()),
        })?;
    }

    Ok(output)
}

fn redacted_capture_diagnostic(label: &str, bytes: &[u8]) -> String {
    if bytes.is_empty() {
        String::new()
    } else {
        format!("[{label} redacted: {} bytes, sha256:{}]", bytes.len(), short_hash(bytes))
    }
}

fn auth_exit_diagnostic(stdout_tail: Option<&str>, stderr_tail: &str) -> String {
    let Some(stdout_tail) = stdout_tail.filter(|tail| !tail.trim().is_empty()) else {
        return stderr_tail.to_owned();
    };
    if stderr_tail.trim().is_empty() {
        format!("stdout: {stdout_tail}")
    } else {
        format!("stdout: {stdout_tail}\nstderr: {stderr_tail}")
    }
}

fn auth_diagnostic_tail(bytes: &[u8]) -> String {
    let tail_start = bytes.len().saturating_sub(AUTH_DIAGNOSTIC_TAIL_LIMIT_BYTES);
    let tail = String::from_utf8_lossy(&bytes[tail_start..]);
    redact_secret_tokens(&tail)
}

fn redact_secret_tokens(text: &str) -> String {
    let mut redacted = text.to_owned();
    for prefix in ["sk-ant-", "sk-proj-", "sk-live-", "sk-test-", "sk_"] {
        redacted = redact_tokens_with_prefix(&redacted, prefix);
    }
    redacted
}

fn redact_tokens_with_prefix(text: &str, prefix: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(offset) = rest.find(prefix) {
        output.push_str(&rest[..offset]);
        output.push_str("[redacted-secret]");
        let token = &rest[offset..];
        let end = token
            .char_indices()
            .find_map(|(index, ch)| (!is_secret_token_char(ch)).then_some(index))
            .unwrap_or(token.len());
        rest = &token[end..];
    }
    output.push_str(rest);
    output
}

fn is_secret_token_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_')
}

fn short_hash(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    hex::encode(&digest[..8])
}

fn default_scratch_root() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join(".memoryd")
        .join("dream-scratch")
}

fn find_executable(program: &str, path_env: Option<&OsStr>) -> Option<PathBuf> {
    let program_path = Path::new(program);
    if program_path.components().count() > 1 {
        return is_executable_file(program_path).then(|| program_path.to_path_buf());
    }

    let path_env = path_env.map(OsString::from).or_else(|| std::env::var_os("PATH"))?;

    std::env::split_paths(&path_env)
        .filter(|directory| !directory.as_os_str().is_empty())
        .map(|directory| directory.join(program))
        .find(|candidate| is_executable_file(candidate))
}

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    std::fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    std::fs::metadata(path).map(|metadata| metadata.is_file()).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_plan(program: &str, args: &[&str]) -> HarnessCommandPlan {
        HarnessCommandPlan {
            program: program.to_owned(),
            args: args.iter().map(|s| s.to_string()).collect(),
            prompt_transport: PromptTransport::Stdin,
        }
    }

    fn make_candidate(program: &str, args: &[&str], markers: &'static [&'static str]) -> AuthProbeCandidate {
        AuthProbeCandidate { plan: make_plan(program, args), unsupported_markers: markers }
    }

    #[test]
    fn unsupported_auth_surface_requires_command_surface_marker_without_auth_failure_marker() {
        for diagnostic in [
            "error: unknown command status",
            "error: unknown subcommand status",
            "error: unrecognized command status",
            "error: unrecognized subcommand status",
            "error: invalid command status",
            "error: invalid subcommand status",
            "error: unsupported command status",
            "error: unsupported subcommand status",
            "stdout: error: unsupported subcommand status\nstderr:",
        ] {
            assert!(
                is_unsupported_auth_surface(diagnostic, AUTH_UNSUPPORTED_COMMAND_MARKERS),
                "expected unsupported diagnostic: {diagnostic}"
            );
        }

        for diagnostic in [
            "not logged in; run codex login",
            "not authenticated; run claude auth login",
            "auth failed: unrecognized account token",
            "invalid token; run login again",
            "session expired: unknown command permissions",
        ] {
            assert!(
                !is_unsupported_auth_surface(diagnostic, AUTH_UNSUPPORTED_COMMAND_MARKERS),
                "expected auth-failure diagnostic: {diagnostic}"
            );
        }
    }

    #[tokio::test]
    async fn auth_probe_any_runs_legacy_after_unsupported_marker() {
        let candidates = vec![
            make_candidate("codex", &["login", "status"], &["unrecognized subcommand"]),
            make_candidate("codex", &["auth", "status"], &["unrecognized subcommand"]),
        ];

        let mut calls: Vec<HarnessCommandPlan> = Vec::new();
        let result = auth_probe_any_with_runner(candidates, |plan| {
            let call_index = calls.len();
            calls.push(plan.clone());
            async move {
                if call_index == 0 {
                    AuthProbeResult::AuthFailed {
                        exit_code: Some(2),
                        stderr_tail: "error: unrecognized subcommand status\n".to_owned(),
                    }
                } else {
                    AuthProbeResult::Ok
                }
            }
        })
        .await;

        assert!(result.is_ok(), "legacy candidate should succeed after unsupported preferred: {result:?}");
        assert_eq!(calls.len(), 2, "should call both candidates");
        assert_eq!(calls[0].args, ["login", "status"]);
        assert_eq!(calls[1].args, ["auth", "status"]);
    }

    #[tokio::test]
    async fn auth_probe_any_does_not_fallback_on_exit_code_without_unsupported_marker() {
        let candidates = vec![
            make_candidate("codex", &["login", "status"], &["unrecognized subcommand"]),
            make_candidate("codex", &["auth", "status"], &["unrecognized subcommand"]),
        ];

        let mut calls: Vec<HarnessCommandPlan> = Vec::new();
        let result = auth_probe_any_with_runner(candidates, |plan| {
            calls.push(plan.clone());
            async move {
                AuthProbeResult::AuthFailed {
                    exit_code: Some(2),
                    stderr_tail: "not logged in; run codex login\n".to_owned(),
                }
            }
        })
        .await;

        assert!(!result.is_ok(), "exit 2 without unsupported marker should not fall back");
        assert_eq!(calls.len(), 1, "should only call preferred candidate");
        assert_eq!(calls[0].args, ["login", "status"]);
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
    async fn auth_probe_any_does_not_fallback_on_preferred_timeout() {
        let candidates = vec![
            make_candidate("claude", &["auth", "status"], &["unknown command"]),
            make_candidate("claude", &["config", "get", "auth.user"], &["unknown command"]),
        ];

        let mut calls: Vec<HarnessCommandPlan> = Vec::new();
        let result = auth_probe_any_with_runner(candidates, |plan| {
            calls.push(plan.clone());
            async move { AuthProbeResult::Timeout }
        })
        .await;

        assert!(!result.is_ok(), "timeout on preferred must not fall back to legacy");
        assert!(matches!(result, AuthProbeResult::Timeout), "timeout should keep typed semantics: {result:?}");
        assert_eq!(calls.len(), 1, "should only call preferred candidate before timeout");
        assert_eq!(calls[0].args, ["auth", "status"]);
    }

    #[tokio::test]
    async fn auth_probe_any_truncates_multibyte_unicode_diagnostic_without_panic() {
        // Build a candidate list where every candidate is "unsupported," producing
        // a long diagnostic summary that includes multibyte Unicode characters.
        // The all-unsupported path exercises `summarize_unsupported_attempts` and
        // `truncate_for_auth_diagnostic`.
        let mut candidates = Vec::new();
        for i in 0..100 {
            candidates.push(make_candidate("codex", &[&format!("cmd{i}")], &["unsupported"]));
        }

        let mut call_index = 0;
        let result = auth_probe_any_with_runner(candidates, |_plan| {
            let i = call_index;
            call_index += 1;
            async move {
                // Use multibyte Unicode in the stderr tail
                AuthProbeResult::AuthFailed {
                    exit_code: Some(2),
                    stderr_tail: format!("error: unsupported command «テスト🧪» ({i})"),
                }
            }
        })
        .await;

        match &result {
            AuthProbeResult::Error { message } => {
                assert!(
                    message.contains("no supported auth status command was accepted"),
                    "all-unsupported path should produce clear diagnostic, got: {message}"
                );
                // The message includes the multi-byte Unicode from the stderr tails
                assert!(message.contains('«'), "diagnostic should include multibyte chars without crashing: {message}");
                // Check char count (not byte length) because multibyte chars inflate byte count.
                let char_count = message.chars().count();
                assert!(
                    char_count <= AUTH_DIAGNOSTIC_SUMMARY_MAX_CHARS + 300,
                    "diagnostic summary should be bounded by char count, got {char_count} chars ({} bytes)",
                    message.len()
                );
            }
            other => panic!("expected Error after all unsupported, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn auth_probe_any_does_not_fallback_on_preferred_error() {
        let candidates = vec![
            make_candidate("codex", &["login", "status"], &["unrecognized subcommand"]),
            make_candidate("codex", &["auth", "status"], &["unrecognized subcommand"]),
        ];

        let mut calls: Vec<HarnessCommandPlan> = Vec::new();
        let result = auth_probe_any_with_runner(candidates, |plan| {
            calls.push(plan.clone());
            async move { AuthProbeResult::Error { message: "I/O error".to_owned() } }
        })
        .await;

        assert!(!result.is_ok(), "I/O error on preferred must not fall back");
        assert_eq!(calls.len(), 1, "should only call preferred candidate");
        assert_eq!(calls[0].args, ["login", "status"]);
    }

    fn auth_failed() -> AuthProbeResult {
        AuthProbeResult::AuthFailed { exit_code: Some(1), stderr_tail: "loggedIn:false".to_owned() }
    }

    fn dir_basename(dir: &Option<PathBuf>) -> Option<String> {
        dir.as_deref()?.file_name()?.to_str().map(str::to_owned)
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
