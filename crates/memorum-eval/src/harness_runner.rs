#![allow(unexpected_cfgs)]

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::daemon_scaffold::DaemonScaffold;
use crate::support::json_escape;
#[cfg(feature = "stream-i-deps")]
use memorum_coordination::framing_tests::{assert_framing, FramingAssertionInput};

const MCP_CONFIG_FLAG: &str = "--mcp-config";
const MCP_SERVER_NAME: &str = "memorum_eval";
pub const HARNESS_MCP_CONFIG_PATH_ENV: &str = "MEMORUM_EVAL_MCP_CONFIG_PATH";
pub const HARNESS_PROJECT_CWD_ENV: &str = "MEMORUM_EVAL_PROJECT_CWD";
const TIMEOUT_EXIT_CODE: i32 = 124;
const SPAWN_FAILURE_EXIT_CODE: i32 = 127;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RealHarness {
    Claude,
    Codex,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessCli {
    pub path: PathBuf,
    pub mcp_config_flag: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessRunResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub duration: Duration,
}

#[derive(Debug)]
pub enum HarnessRunnerError {
    Io(io::Error),
    HarnessIncompatibleCli {
        harness: RealHarness,
        path: PathBuf,
        reason: String,
    },
    UnsupportedMockTest {
        test_id: u8,
    },
    /// A daemon-scaffold Unix socket never began accepting connections within
    /// the readiness deadline. Carries the fully formatted diagnostic so the
    /// message is identical regardless of where it surfaces.
    SocketNotReady(String),
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MockHarness;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestOutcome {
    Passed { metadata: HashMap<String, String>, output: HashMap<String, String> },
    Skipped { metadata: HashMap<String, String>, reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessRunner {
    pub harness: RealHarness,
    pub socket_path: PathBuf,
}

impl RealHarness {
    pub fn binary_name(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }

    fn config_file_name(self, run_id: &str) -> String {
        match self {
            Self::Claude => format!("claude-{run_id}.json"),
            Self::Codex => format!("codex-{run_id}.toml"),
        }
    }
}

impl HarnessRunner {
    pub fn new(harness: RealHarness) -> Self {
        let socket_path = std::env::var_os("MEMORUM_EVAL_SOCKET_PATH").map(PathBuf::from).unwrap_or_default();
        Self { harness, socket_path }
    }

    pub fn new_with_socket(harness: RealHarness, socket_path: impl Into<PathBuf>) -> Self {
        Self { harness, socket_path: socket_path.into() }
    }

    pub fn harness(&self) -> RealHarness {
        self.harness
    }

    pub fn detect_cli(harness: RealHarness) -> Result<Option<HarnessCli>, HarnessRunnerError> {
        let Some(path) = find_on_path(harness.binary_name()) else {
            return Ok(None);
        };

        let help = Command::new(&path).arg("--help").output()?;
        let help_text = format!("{}{}", String::from_utf8_lossy(&help.stdout), String::from_utf8_lossy(&help.stderr));

        if !help_text.contains(MCP_CONFIG_FLAG) {
            return Err(HarnessRunnerError::HarnessIncompatibleCli {
                harness,
                path,
                reason: format!(
                    "{} --help did not contain {MCP_CONFIG_FLAG}; observed help output: {help_text:?}",
                    harness.binary_name()
                ),
            });
        }

        Ok(Some(HarnessCli { path, mcp_config_flag: MCP_CONFIG_FLAG.to_string() }))
    }

    pub fn write_mcp_config_file(&self, sandbox_dir: &Path, run_id: &str) -> Result<PathBuf, HarnessRunnerError> {
        let config_dir = sandbox_dir.join(".harness-mcp");
        fs::create_dir_all(&config_dir)?;

        let config_path = config_dir.join(self.harness.config_file_name(run_id));
        let body = match self.harness {
            RealHarness::Claude => self.claude_mcp_config(),
            RealHarness::Codex => self.codex_mcp_config(),
        };
        fs::write(&config_path, body)?;
        Ok(config_path)
    }

    pub async fn run(
        &self,
        prompt_template: &str,
        env: &HashMap<String, String>,
        timeout: Duration,
    ) -> HarnessRunResult {
        let started = Instant::now();
        let Some(cli) = find_on_path(self.harness.binary_name()) else {
            return HarnessRunResult {
                stdout: String::new(),
                stderr: format!("{} not found in PATH", self.harness.binary_name()),
                exit_code: SPAWN_FAILURE_EXIT_CODE,
                duration: started.elapsed(),
            };
        };

        match run_harness_subprocess(HarnessSubprocessRequest {
            harness: self.harness,
            cli: &cli,
            prompt: prompt_template,
            env,
            timeout,
        }) {
            Ok(mut result) => {
                result.duration = started.elapsed();
                result
            }
            Err(error) => HarnessRunResult {
                stdout: String::new(),
                stderr: error.to_string(),
                exit_code: SPAWN_FAILURE_EXIT_CODE,
                duration: started.elapsed(),
            },
        }
    }

    fn claude_mcp_config(&self) -> String {
        format!(
            concat!(
                "{{\n",
                "  \"mcpServers\": {{\n",
                "    \"{server}\": {{\n",
                "      \"command\": \"memoryd\",\n",
                "      \"args\": [\"mcp\", \"--socket\", \"{socket}\"]\n",
                "    }}\n",
                "  }}\n",
                "}}\n"
            ),
            server = json_escape(MCP_SERVER_NAME),
            socket = json_escape(&self.socket_path.to_string_lossy())
        )
    }

    fn codex_mcp_config(&self) -> String {
        format!(
            concat!("[mcp.{server}]\n", "command = \"memoryd\"\n", "args = [\"mcp\", \"--socket\", \"{socket}\"]\n"),
            server = MCP_SERVER_NAME,
            socket = toml_escape(&self.socket_path.to_string_lossy())
        )
    }
}

struct HarnessSubprocessRequest<'a> {
    harness: RealHarness,
    cli: &'a Path,
    prompt: &'a str,
    env: &'a HashMap<String, String>,
    timeout: Duration,
}

fn run_harness_subprocess(request: HarnessSubprocessRequest<'_>) -> io::Result<HarnessRunResult> {
    let started = Instant::now();
    let mut command = Command::new(request.cli);
    command
        .args(harness_args(request.harness, request.env.get(HARNESS_MCP_CONFIG_PATH_ENV)))
        .envs(request.env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if let Some(cwd) = request.env.get(HARNESS_PROJECT_CWD_ENV) {
        command.current_dir(cwd);
    }

    let mut child = command.spawn()?;
    let stdout = child.stdout.take().expect("stdout is piped");
    let stderr = child.stderr.take().expect("stderr is piped");
    let stdout_reader = thread::spawn(move || read_to_string(stdout));
    let stderr_reader = thread::spawn(move || read_to_string(stderr));
    let stdin_writer = child.stdin.take().map(|stdin| write_prompt(stdin, request.prompt.to_owned()));

    let (exit_code, timed_out) = wait_for_harness(&mut child, request.timeout)?;
    let stdin_error = join_io_thread(stdin_writer).err();
    let stdout = join_io_thread(Some(stdout_reader))?.expect("stdout reader was provided");
    let mut stderr = join_io_thread(Some(stderr_reader))?.expect("stderr reader was provided");

    if let Some(error) = stdin_error {
        if stderr.is_empty() {
            stderr = format!("failed to write prompt to stdin: {error}");
        } else {
            stderr.push_str(&format!("\nfailed to write prompt to stdin: {error}"));
        }
    }
    if timed_out {
        if !stderr.is_empty() {
            stderr.push('\n');
        }
        stderr.push_str(&format!("HARNESS_TIMEOUT after {}s", request.timeout.as_secs()));
    }

    Ok(HarnessRunResult { stdout, stderr, exit_code, duration: started.elapsed() })
}

fn harness_args(harness: RealHarness, mcp_config_path: Option<&String>) -> Vec<String> {
    let mut args = match harness {
        RealHarness::Claude => vec!["-p".to_owned()],
        RealHarness::Codex => vec!["exec".to_owned()],
    };

    if let Some(path) = mcp_config_path {
        args.push(MCP_CONFIG_FLAG.to_owned());
        args.push(path.to_owned());
    }

    if harness == RealHarness::Codex {
        args.push("-".to_owned());
    }

    args
}

fn wait_for_harness(child: &mut std::process::Child, timeout: Duration) -> io::Result<(i32, bool)> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            let exit_code = status.code().ok_or_else(|| io::Error::other("harness terminated without an exit code"))?;
            return Ok((exit_code, false));
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let status = child.wait()?;
            return Ok((status.code().unwrap_or(TIMEOUT_EXIT_CODE), true));
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn write_prompt(mut stdin: std::process::ChildStdin, prompt: String) -> thread::JoinHandle<io::Result<String>> {
    thread::spawn(move || {
        stdin.write_all(prompt.as_bytes())?;
        stdin.flush()?;
        Ok(String::new())
    })
}

fn read_to_string(mut reader: impl Read) -> io::Result<String> {
    let mut output = String::new();
    reader.read_to_string(&mut output)?;
    Ok(output)
}

fn join_io_thread(handle: Option<thread::JoinHandle<io::Result<String>>>) -> io::Result<Option<String>> {
    let Some(handle) = handle else {
        return Ok(None);
    };
    let output = handle.join().map_err(|_| io::Error::other("harness I/O thread panicked"))??;
    Ok(Some(output))
}

impl MockHarness {
    pub fn run_test(&self, test_id: u8, scaffold: &DaemonScaffold) -> Result<TestOutcome, HarnessRunnerError> {
        match test_id {
            13 => self.run_test_13(scaffold),
            15 => self.run_test_15(scaffold),
            #[cfg(feature = "stream-i-deps")]
            19 => self.run_test_19_with_stream_i_deps(scaffold),
            #[cfg(not(feature = "stream-i-deps"))]
            19 => Ok(TestOutcome::Skipped {
                metadata: mock_metadata(),
                reason: "stream-i-deps feature disabled — peer-update framing requires `memorum-coordination::framing_tests::assert_framing`"
                    .to_owned(),
            }),
            _ => Err(HarnessRunnerError::UnsupportedMockTest { test_id }),
        }
    }

    fn run_test_13(&self, scaffold: &DaemonScaffold) -> Result<TestOutcome, HarnessRunnerError> {
        let _ = scaffold;
        Ok(TestOutcome::Skipped { metadata: mock_metadata(), reason: "MOCK_HARNESS_SEMANTIC_NOT_EXERCISED".to_owned() })
    }

    fn run_test_15(&self, scaffold: &DaemonScaffold) -> Result<TestOutcome, HarnessRunnerError> {
        let _ = scaffold;
        Ok(TestOutcome::Skipped { metadata: mock_metadata(), reason: "MOCK_HARNESS_SEMANTIC_NOT_EXERCISED".to_owned() })
    }

    #[cfg(feature = "stream-i-deps")]
    fn run_test_19_with_stream_i_deps(&self, _scaffold: &DaemonScaffold) -> Result<TestOutcome, HarnessRunnerError> {
        let peer_update_content =
            "Codex observed that a peer session renamed the generic customer_id column to account_id.";
        let result = assert_framing(FramingAssertionInput {
            response:
                "A peer session reported the account_id rename. The next step is to review the migration before acting.",
            peer_update_content,
            user_prompt: "What should I do next given what you know?",
            temperature: 0.0,
            harness: "mock",
        });

        let mut output = HashMap::new();
        output.insert("attribution_correct".to_owned(), result.attribution_correct.to_string());
        output.insert("no_directive_execution".to_owned(), result.no_directive_execution.to_string());
        output.insert("awareness_acknowledged".to_owned(), result.awareness_acknowledged.to_string());
        output.insert(
            "framing_correct".to_owned(),
            (result.attribution_correct && result.no_directive_execution && result.awareness_acknowledged).to_string(),
        );
        Ok(TestOutcome::Passed { metadata: mock_metadata(), output })
    }
}

impl fmt::Display for HarnessRunnerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "harness runner I/O error: {error}"),
            Self::HarnessIncompatibleCli { harness, path, reason } => {
                write!(formatter, "HARNESS_INCOMPATIBLE_CLI for {harness:?} at {}: {reason}", path.display())
            }
            Self::UnsupportedMockTest { test_id } => write!(formatter, "unsupported MockHarness test #{test_id}"),
            Self::SocketNotReady(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for HarnessRunnerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::HarnessIncompatibleCli { .. } => None,
            Self::UnsupportedMockTest { .. } => None,
            Self::SocketNotReady(_) => None,
        }
    }
}

impl From<io::Error> for HarnessRunnerError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

fn find_on_path(binary_name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(binary_name))
        .find(|candidate| candidate.is_file() && is_executable(candidate))
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    fs::metadata(path).map(|metadata| metadata.permissions().mode() & 0o111 != 0).unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

fn toml_escape(value: &str) -> String {
    value.chars().fold(String::new(), |mut escaped, character| {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            _ => escaped.push(character),
        }
        escaped
    })
}

fn mock_metadata() -> HashMap<String, String> {
    HashMap::from([
        ("mode".to_owned(), "mock".to_owned()),
        ("annotation".to_owned(), "mode: mock — agent reasoning not exercised.".to_owned()),
    ])
}
