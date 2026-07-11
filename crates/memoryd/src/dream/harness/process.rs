use std::{
    ffi::{OsStr, OsString},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::Duration,
};

use sha2::{Digest, Sha256};

use crate::protocol::PromptTransport;

use super::super::error::{HarnessCliError, JsonStage};
use super::env::MinimalEnvironment;

const STDOUT_CAPTURE_LIMIT_BYTES: usize = 16 * 1024 * 1024;
const STDERR_TAIL_LIMIT_BYTES: usize = 64 * 1024;
const AUTH_DIAGNOSTIC_TAIL_LIMIT_BYTES: usize = 4 * 1024;
pub(super) const DEFAULT_KILL_GRACE: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessCommandPlan {
    pub program: String,
    pub args: Vec<String>,
    pub prompt_transport: PromptTransport,
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
    stdout_reader: std::sync::mpsc::Receiver<std::io::Result<Vec<u8>>>,
    stderr_reader: std::sync::mpsc::Receiver<std::io::Result<Vec<u8>>>,
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
    // Own process group: a forking harness CLI (W0 H1 class) must be killable
    // as a GROUP on timeout, or setsid'd/forked descendants survive the child
    // kill and hold the inherited pipes forever.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        process.process_group(0);
    }
    command.environment.apply_to(&mut process);

    let mut child = process.spawn()?;
    let stdout = child.stdout.take().expect("stdout was piped");
    let stderr = child.stderr.take().expect("stderr was piped");
    // Channel-delivered reads (never bare joins): a grandchild holding the
    // inherited pipe write-ends must not stall capture past the drain grace
    // (W0 H1/V1 pattern, ported from memorum-eval judge.rs).
    let (stdout_tx, stdout_reader) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let _ = stdout_tx.send(read_first_bytes(stdout, STDOUT_CAPTURE_LIMIT_BYTES));
    });
    let (stderr_tx, stderr_reader) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let _ = stderr_tx.send(read_last_bytes(stderr, STDERR_TAIL_LIMIT_BYTES));
    });
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
    // One shared absolute drain deadline for both pipes, after the child is
    // reaped. On a group-killed timeout the pipes close immediately; the grace
    // only matters when an in-group straggler escaped (or a non-unix build
    // cannot group-kill) — then we return Timeout instead of hanging. Never
    // group-kill here: the child is reaped and the pgid may be recycled.
    let drain_deadline = std::time::Instant::now() + PIPE_DRAIN_GRACE;
    let stdout = recv_reader(&handles.stdout_reader, drain_deadline, options.timeout)?;
    let stderr_tail = recv_reader(&handles.stderr_reader, drain_deadline, options.timeout)?;
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
            // Timeout path: group SIGTERM, a grace DELAY (no reap — reaping
            // early would skip the group SIGKILL and let SIGTERM-resistant
            // in-group descendants keep the pipes; round-3 verify finding),
            // then group SIGKILL while the pgid is provably still ours, then
            // reap. Costs at most kill_grace extra wall on the rare timeout
            // path; matches the W0 judge semantics exactly.
            terminate_child_group(child)?;
            thread::sleep(kill_grace);
            #[cfg(unix)]
            {
                // Group SIGKILL is safe pre-reap: the pgid is still ours.
                let _ = send_signal(-(child.id() as i32), SIGKILL);
            }
            child.kill()?;
            let status = child.wait()?;
            return Ok(WaitOutcome { status, timed_out: true });
        }

        thread::sleep(Duration::from_millis(10));
    }
}

fn terminate_child_group(child: &mut std::process::Child) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        // Negative pid = the whole process group (we spawned with
        // process_group(0), so the group id equals the child pid). Fall back
        // to the single child if the group signal fails.
        if send_signal(-(child.id() as i32), SIGTERM).is_ok() || send_signal(child.id() as i32, SIGTERM).is_ok() {
            return Ok(());
        }
    }

    child.kill()
}

#[cfg(unix)]
const SIGTERM: i32 = 15;
#[cfg(unix)]
const SIGKILL: i32 = 9;

#[cfg(unix)]
fn send_signal(pid: i32, signal: i32) -> std::io::Result<()> {
    // SAFETY: `pid` derives from `std::process::Child::id` for a child (or its
    // process group, negated) that we spawned and have not yet reaped, and the
    // signal is a plain number. `kill(2)` does not dereference Rust pointers
    // or retain references across the FFI boundary.
    let result = unsafe { posix_kill(pid, signal) };
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

/// How long capture waits for the pipe readers after the child is reaped.
/// Pipes close instantly in the normal case; the grace only bounds the
/// straggler-holding-the-pipe case.
const PIPE_DRAIN_GRACE: Duration = Duration::from_secs(5);

fn recv_reader(
    rx: &std::sync::mpsc::Receiver<std::io::Result<Vec<u8>>>,
    deadline: std::time::Instant,
    command_timeout: Duration,
) -> Result<Vec<u8>, HarnessCliError> {
    let remaining = deadline.saturating_duration_since(std::time::Instant::now()).max(Duration::from_millis(50));
    match rx.recv_timeout(remaining) {
        Ok(result) => result.map_err(HarnessCliError::Io),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            // Detach: the reader thread dies when the straggler releases the
            // pipe. Bounded failure beats an unbounded join.
            Err(HarnessCliError::Timeout { duration: command_timeout })
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            Err(HarnessCliError::Io(std::io::Error::other("hardened command reader panicked")))
        }
    }
}

pub(super) fn validate_json_if_expected(output: String, expect_json: bool) -> Result<String, HarnessCliError> {
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

pub(super) fn default_scratch_root() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join(".memoryd")
        .join("dream-scratch")
}

pub(super) fn find_executable(program: &str, path_env: Option<&OsStr>) -> Option<PathBuf> {
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

pub(super) fn path_display(path_env: Option<&OsStr>) -> String {
    path_env
        .map(|path| path.to_string_lossy().into_owned())
        .or_else(|| std::env::var("PATH").ok())
        .unwrap_or_else(|| "<unset>".to_owned())
}
