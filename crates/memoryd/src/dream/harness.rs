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
];
pub const CLAUDE_ENV_ALLOWLIST: &[&str] = &["ANTHROPIC_API_KEY", "CLAUDE_CONFIG_DIR", "HOME", "PATH", "TERM"];
pub const CODEX_ENV_ALLOWLIST: &[&str] = &["CODEX_HOME", "HOME", "OPENAI_API_KEY", "PATH", "TERM"];

const STDOUT_CAPTURE_LIMIT_BYTES: usize = 16 * 1024 * 1024;
const STDERR_TAIL_LIMIT_BYTES: usize = 64 * 1024;
const DEFAULT_KILL_GRACE: Duration = Duration::from_secs(2);
const AUTH_PROBE_TIMEOUT: Duration = Duration::from_secs(10);

pub type HarnessFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait HarnessCli: Send + Sync {
    fn name(&self) -> &'static str;

    fn prompt_transport(&self) -> PromptTransport;

    fn is_installed(&self) -> bool;

    fn auth_probe(&self) -> HarnessFuture<'_, AuthProbeResult>;

    fn is_authenticated(&self) -> HarnessFuture<'_, Result<bool, HarnessCliError>>;

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

#[derive(Debug, Clone, Default)]
pub struct ClaudeCodeCli {
    path_env: Option<OsString>,
}

impl ClaudeCodeCli {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_path_env(path_env: OsString) -> Self {
        Self { path_env: Some(path_env) }
    }

    pub fn command(&self, _expect_json: bool) -> HarnessCommandPlan {
        HarnessCommandPlan {
            program: "claude".to_owned(),
            args: vec!["--print".to_owned()],
            prompt_transport: PromptTransport::Stdin,
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
        Box::pin(async move {
            if !self.is_installed() {
                return AuthProbeResult::CliMissing { which: "claude", path: path_display(self.path_env.as_deref()) };
            }

            auth_probe(
                HarnessCommandPlan {
                    program: "claude".to_owned(),
                    args: vec!["config".to_owned(), "get".to_owned(), "auth.user".to_owned()],
                    prompt_transport: PromptTransport::Stdin,
                },
                self.path_env.clone(),
                CLAUDE_ENV_ALLOWLIST,
            )
            .await
        })
    }

    fn is_authenticated(&self) -> HarnessFuture<'_, Result<bool, HarnessCliError>> {
        Box::pin(async move { Ok(self.auth_probe().await.is_ok()) })
    }

    fn complete<'a>(
        &'a self,
        prompt: &'a str,
        expect_json: bool,
        timeout: Duration,
    ) -> HarnessFuture<'a, Result<String, HarnessCliError>> {
        Box::pin(async move {
            if !self.is_installed() {
                return Err(HarnessCliError::NotInstalled);
            }
            complete_external(
                self.command(expect_json),
                MinimalEnvironment::for_adapter(self.path_env.clone(), CLAUDE_ENV_ALLOWLIST),
                prompt,
                PassRunOptions { expect_json, timeout },
            )
            .await
        })
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
        Box::pin(async move {
            if !self.is_installed() {
                return AuthProbeResult::CliMissing { which: "codex", path: path_display(self.path_env.as_deref()) };
            }

            auth_probe(
                HarnessCommandPlan {
                    program: "codex".to_owned(),
                    args: vec!["auth".to_owned(), "status".to_owned()],
                    prompt_transport: PromptTransport::Stdin,
                },
                self.path_env.clone(),
                CODEX_ENV_ALLOWLIST,
            )
            .await
        })
    }

    fn is_authenticated(&self) -> HarnessFuture<'_, Result<bool, HarnessCliError>> {
        Box::pin(async move { Ok(self.auth_probe().await.is_ok()) })
    }

    fn complete<'a>(
        &'a self,
        prompt: &'a str,
        expect_json: bool,
        timeout: Duration,
    ) -> HarnessFuture<'a, Result<String, HarnessCliError>> {
        Box::pin(async move {
            if !self.is_installed() {
                return Err(HarnessCliError::NotInstalled);
            }
            complete_external(
                self.command(expect_json),
                MinimalEnvironment::for_adapter(self.path_env.clone(), CODEX_ENV_ALLOWLIST),
                prompt,
                PassRunOptions { expect_json, timeout },
            )
            .await
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct PassRunOptions {
    expect_json: bool,
    timeout: Duration,
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
        },
        prompt,
    )
    .await?;

    Ok(output.stdout)
}

async fn auth_probe(plan: HarnessCommandPlan, path_env: Option<OsString>, env_allowlist: &[&str]) -> AuthProbeResult {
    let result = run_hardened_command(
        HardenedCommand {
            program: PathBuf::from(plan.program),
            args: plan.args,
            prompt_transport: plan.prompt_transport,
            expect_json: false,
            timeout: AUTH_PROBE_TIMEOUT,
            kill_grace: DEFAULT_KILL_GRACE,
            scratch_root: default_scratch_root(),
            environment: MinimalEnvironment::for_adapter(path_env, env_allowlist),
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
    let kill_grace = command.kill_grace;
    let (mut child, handles) = spawn_hardened_child(command, prompt, scratch_dir.path())?;
    let output = capture_hardened_child(&mut child, handles, timeout, kill_grace)?;

    finalize_hardened_output(output, expect_json, timeout)
}

struct SpawnedHardenedChild {
    stdout_reader: thread::JoinHandle<std::io::Result<Vec<u8>>>,
    stderr_reader: thread::JoinHandle<std::io::Result<Vec<u8>>>,
    stdin_writer: Option<thread::JoinHandle<std::io::Result<()>>>,
}

struct CapturedHardenedChild {
    stdout: String,
    stderr_tail: String,
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
    timeout: Duration,
    kill_grace: Duration,
) -> Result<CapturedHardenedChild, HarnessCliError> {
    let outcome = wait_with_timeout(child, timeout, kill_grace)?;
    let stdin_write_result = join_stdin_writer(handles.stdin_writer);
    let stdout = join_reader(handles.stdout_reader)?;
    let stderr_tail = join_reader(handles.stderr_reader)?;
    let stdout = String::from_utf8_lossy(&stdout).into_owned();
    let stderr_tail = redacted_capture_diagnostic("stderr", &stderr_tail);
    let status = outcome.status;
    Ok(CapturedHardenedChild {
        stdout,
        stderr_tail,
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
        return Err(HarnessCliError::SubprocessExit { code: output.status_code, stderr_tail: output.stderr_tail });
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
