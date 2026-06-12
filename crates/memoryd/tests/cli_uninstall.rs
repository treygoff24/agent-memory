//! Integration coverage for `memoryd uninstall`.
//!
//! These tests drive the real `memoryd` binary so they exercise the same
//! dispatch, flag parsing, and stdout/stderr split a teardown agent sees. The
//! hard invariants under test: stdout carries valid JSON and nothing else;
//! `--print-only` mutates nothing; unwiring removes only the `memorum`/`memoryd`
//! entry (user and project scope) and preserves everything else; `--purge`
//! refusal without the flag. Env is fully isolated — these never touch the real
//! home dir, Claude config, or Codex config.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use common::{assert_success, stderr, stdout};
use memoryd::protocol::{DaemonProcessStatus, RequestEnvelope, ResponseEnvelope, ResponsePayload, StatusResponse};
use serde_json::Value;
use serial_test::serial;

/// `uninstall --print-only` against a config that holds a `memorum` entry must
/// emit a parseable report, leave the config byte-for-byte untouched, and report
/// the unwire step as `expected` (dry-run).
#[test]
#[serial]
fn print_only_mutates_nothing() {
    let env = TestEnv::new();
    let claude_json = env.write_claude_config(CLAUDE_WITH_MEMORUM);
    let before = std::fs::read_to_string(&claude_json).expect("read claude config");

    let output = env.run(["uninstall", "--print-only", "--harness", "claude"]);
    assert_success(&output);

    let report: Value = parse(&output);
    assert_eq!(report["schema_version"], 1);
    let unwire = find_step(&report, "unwire_claude").expect("unwire_claude step present");
    assert_eq!(unwire["status"], "expected", "print-only must not apply the unwire");

    let after = std::fs::read_to_string(&claude_json).expect("read claude config");
    assert_eq!(before, after, "print-only must not modify the config");
}

/// Applying the unwire removes the `memorum`/`memoryd` entry at both the Claude
/// user scope and a project scope, while preserving sibling servers, unrelated
/// projects, and other top-level fields.
#[test]
#[serial]
fn unwire_removes_only_memorum_entries_preserving_others() {
    let env = TestEnv::new();
    let claude_json = env.write_claude_config(CLAUDE_WITH_MEMORUM);

    let output = env.run(["uninstall", "--non-interactive", "--json", "--harness", "claude"]);
    assert_success(&output);

    let report: Value = parse(&output);
    let unwire = find_step(&report, "unwire_claude").expect("unwire_claude step present");
    assert_eq!(unwire["status"], "succeeded");

    let after: Value = serde_json::from_str(&std::fs::read_to_string(&claude_json).expect("read")).expect("json");
    // User scope: memorum gone, sibling preserved.
    let user = &after["mcpServers"];
    assert!(user.get("memorum").is_none(), "user-scope memorum must be removed");
    assert!(user.get("other").is_some(), "sibling server must survive");
    // Project /a: memorum gone (mcpServers dropped when empty), allowedTools kept.
    let project_a = &after["projects"]["/a"];
    assert!(project_a.get("mcpServers").is_none(), "empty project mcpServers should be dropped");
    assert_eq!(project_a["allowedTools"][0], "read");
    // Unrelated top-level field preserved.
    assert_eq!(after["model"], "claude-opus");
}

/// A `memorum`-named entry not commanded by `memoryd` is left untouched, and the
/// step reports `skipped`.
#[test]
#[serial]
fn unwire_leaves_foreign_memorum_entry_untouched() {
    let env = TestEnv::new();
    let claude_json =
        env.write_claude_config(r#"{ "mcpServers": { "memorum": { "command": "some-other-bin", "args": [] } } }"#);
    let before = std::fs::read_to_string(&claude_json).expect("read");

    let output = env.run(["uninstall", "--non-interactive", "--json", "--harness", "claude"]);
    assert_success(&output);

    let report: Value = parse(&output);
    let unwire = find_step(&report, "unwire_claude").expect("unwire_claude step present");
    assert_eq!(unwire["status"], "skipped", "a non-memoryd memorum entry is not ours to remove");

    let after = std::fs::read_to_string(&claude_json).expect("read");
    assert_eq!(before, after);
}

/// Without `--purge`, the purge step is `skipped` with the documented message
/// and the data is preserved. The full report shape is asserted here too.
#[test]
#[serial]
fn purge_is_refused_without_flag() {
    let env = TestEnv::new();
    let repo = env.temp.path().join("repo");
    std::fs::create_dir_all(repo.join(".memorum")).expect("memorum-shaped repo");

    let output =
        env.run(["uninstall", "--non-interactive", "--json", "--harness", "none", "--repo", repo.to_str().unwrap()]);
    assert_success(&output);

    let report: Value = parse(&output);
    // Report shape: schema_version + ordered steps with status.
    assert_eq!(report["schema_version"], 1);
    let purge = find_step(&report, "purge_data").expect("purge_data step present");
    assert_eq!(purge["status"], "skipped");
    assert_eq!(purge["message"], "data preserved; pass --purge to delete");
    assert!(repo.exists(), "data must be preserved without --purge");

    // Every documented step name is present.
    for step in ["detect", "stop_daemon", "remove_launchd", "purge_data", "verify"] {
        assert!(find_step(&report, step).is_some(), "missing step {step}");
    }
}

/// A non-TTY invocation with no machine mode must refuse with guidance and write
/// nothing to stdout — mirroring `init`.
#[test]
#[serial]
fn piped_invocation_without_machine_mode_refuses() {
    let env = TestEnv::new();
    let output = env.run(["uninstall", "--harness", "none"]);
    assert!(!output.status.success(), "non-TTY uninstall without a machine mode must fail");
    assert!(stdout(&output).trim().is_empty(), "refusal must not write stdout: {}", stdout(&output));
    let err = stderr(&output);
    assert!(err.contains("--print-only"), "refusal must point at the dry-run path: {err}");
    assert!(err.contains("--non-interactive"), "refusal must point at the scripted path: {err}");
}

/// Any failed step in the uninstall report is fatal to the process exit status.
#[test]
#[serial]
fn failed_step_exits_nonzero() {
    let env = TestEnv::new();
    let (repo, runtime) = env.memorum_repo_runtime();
    let _socket = FakeUnresponsiveSocket::bind(&runtime.join("memoryd.sock"));

    let output = env.run([
        "uninstall",
        "--non-interactive",
        "--json",
        "--harness",
        "none",
        "--repo",
        path_arg(&repo),
        "--runtime",
        path_arg(&runtime),
    ]);

    assert!(!output.status.success(), "failed stop_daemon step must exit non-zero");
    let report: Value = parse(&output);
    let stop = find_step(&report, "stop_daemon").expect("stop_daemon step present");
    assert_eq!(stop["status"], "failed");
}

/// A failed daemon stop gates destructive purge, preserving repo/runtime data.
#[test]
#[serial]
fn purge_refuses_when_stop_failed_preserving_repo() {
    let env = TestEnv::new();
    let (repo, runtime) = env.memorum_repo_runtime();
    let _socket = FakeUnresponsiveSocket::bind(&runtime.join("memoryd.sock"));

    let output = env.run([
        "uninstall",
        "--non-interactive",
        "--json",
        "--purge",
        "--harness",
        "none",
        "--repo",
        path_arg(&repo),
        "--runtime",
        path_arg(&runtime),
    ]);

    assert!(!output.status.success(), "purge refusal is a failed report and must exit non-zero");
    let report: Value = parse(&output);
    let purge = find_step(&report, "purge_data").expect("purge_data step present");
    assert_eq!(purge["status"], "failed");
    assert_eq!(
        purge["message"],
        "refusing to purge while the daemon may still be running; stop it manually and re-run"
    );
    assert!(repo.exists(), "repo must survive when stop_daemon failed");
    assert!(runtime.exists(), "runtime must survive when stop_daemon failed");
}

/// When an old/lost pid-file daemon still answers on the socket, uninstall asks
/// `Status` for the daemon pid and uses that pid for the existing stop flow.
#[test]
#[serial]
fn stop_daemon_uses_status_rpc_when_pid_file_missing() {
    let env = TestEnv::new();
    let (_repo, runtime) = env.memorum_repo_runtime();
    let mut signal_target = spawn_signal_target();
    let target_pid = signal_target.id();
    let waiter = thread::spawn(move || signal_target.wait().expect("signal target reaped"));
    let _daemon = FakeStatusDaemon::bind(&runtime.join("memoryd.sock"), target_pid, true);

    let output = env.run([
        "uninstall",
        "--non-interactive",
        "--json",
        "--harness",
        "none",
        "--repo",
        path_arg(&_repo),
        "--runtime",
        path_arg(&runtime),
    ]);

    assert_success(&output);
    let report: Value = parse(&output);
    let stop = find_step(&report, "stop_daemon").expect("stop_daemon step present");
    assert_eq!(stop["status"], "succeeded");
    assert!(stop["message"].as_str().unwrap_or_default().contains(&format!("pid {target_pid}")));
    let status = waiter.join().expect("waiter thread joins");
    assert!(!status.success(), "signal target should be terminated by SIGTERM");
}

/// The serve entrypoint writes `<runtime>/memoryd.pid` and removes it on clean
/// shutdown, so direct serve and MCP auto-start daemons share uninstall's pid
/// discovery path.
#[test]
#[serial]
fn serve_writes_and_removes_daemon_pid_file() {
    let env = TestEnv::new();
    let (repo, runtime) = env.memorum_repo_runtime();
    let socket = runtime.join("memoryd.sock");
    let mut child = spawn_memoryd_serve(&repo, &runtime, &socket);
    wait_for_socket(&socket, &mut child);

    let pid_file = runtime.join("memoryd.pid");
    let pid = std::fs::read_to_string(&pid_file).expect("serve writes pid file");
    assert_eq!(pid.trim(), child.id().to_string());

    send_sigterm(child.id());
    child.wait().expect("serve child reaped");
    wait_for_absent(&pid_file);
    assert!(!pid_file.exists(), "serve removes pid file on shutdown");
}

const CLAUDE_WITH_MEMORUM: &str = r#"{
  "model": "claude-opus",
  "mcpServers": {
    "memorum": { "command": "memoryd", "args": ["mcp", "--socket", "/x"] },
    "other": { "command": "other-bin", "args": [] }
  },
  "projects": {
    "/a": {
      "mcpServers": { "memorum": { "command": "memoryd", "args": ["mcp"] } },
      "allowedTools": ["read"]
    }
  }
}"#;

struct TestEnv {
    temp: tempfile::TempDir,
    home: PathBuf,
    claude_config: PathBuf,
    codex_home: PathBuf,
}

impl TestEnv {
    fn new() -> Self {
        let temp = tempfile::Builder::new().prefix("memd-uninstall-").tempdir_in("/tmp").expect("tempdir");
        let home = temp.path().join("home");
        let claude_config = temp.path().join("claude-config");
        let codex_home = temp.path().join("codex-home");
        std::fs::create_dir_all(&home).expect("home");
        std::fs::create_dir_all(&claude_config).expect("claude config dir");
        std::fs::create_dir_all(&codex_home).expect("codex home");
        Self { temp, home, claude_config, codex_home }
    }

    /// Write `$CLAUDE_CONFIG_DIR/.claude.json` and return its path.
    fn write_claude_config(&self, body: &str) -> PathBuf {
        let path = self.claude_config.join(".claude.json");
        std::fs::write(&path, body).expect("write claude config");
        path
    }

    fn run<const N: usize>(&self, args: [&str; N]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_memoryd"))
            .args(args)
            .env("HOME", &self.home)
            .env("CLAUDE_CONFIG_DIR", &self.claude_config)
            .env("CODEX_HOME", &self.codex_home)
            .env_remove("MEMORUM_REPO")
            .output()
            .expect("run memoryd")
    }

    fn memorum_repo_runtime(&self) -> (PathBuf, PathBuf) {
        let repo = self.temp.path().join("repo");
        let runtime = self.temp.path().join("runtime");
        std::fs::create_dir_all(repo.join(".memorum")).expect("memorum-shaped repo");
        std::fs::create_dir_all(&runtime).expect("runtime dir");
        (repo, runtime)
    }
}

fn parse(output: &Output) -> Value {
    let raw = stdout(output);
    serde_json::from_str(&raw).unwrap_or_else(|error| {
        panic!("stdout must be pure JSON ({error})\nstdout:\n{raw}\nstderr:\n{}", stderr(output))
    })
}

fn find_step<'a>(report: &'a Value, name: &str) -> Option<&'a Value> {
    report["steps"].as_array()?.iter().find(|step| step["step"] == name)
}

fn path_arg(path: &Path) -> &str {
    path.to_str().expect("test path is utf8")
}

struct FakeUnresponsiveSocket {
    socket: PathBuf,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl FakeUnresponsiveSocket {
    fn bind(socket: &Path) -> Self {
        run_fake_socket(socket, |_line| None)
    }
}

struct FakeStatusDaemon {
    _inner: FakeUnresponsiveSocket,
}

impl FakeStatusDaemon {
    fn bind(socket: &Path, pid: u32, shutdown_after_response: bool) -> Self {
        let inner = run_fake_socket(socket, move |line| {
            let request = RequestEnvelope::from_json_line(line).expect("status request decodes");
            let response = ResponseEnvelope::success(
                request.id,
                ResponsePayload::Status(StatusResponse {
                    state: "ready".to_string(),
                    guidance: "fake status daemon".to_string(),
                    daemon: Some(DaemonProcessStatus {
                        version: env!("CARGO_PKG_VERSION").to_string(),
                        pid,
                        uptime_seconds: None,
                    }),
                    ..StatusResponse::default()
                }),
            );
            Some((response.to_json_line().expect("status response encodes"), shutdown_after_response))
        });
        Self { _inner: inner }
    }
}

fn run_fake_socket<F>(socket: &Path, mut respond: F) -> FakeUnresponsiveSocket
where
    F: FnMut(&str) -> Option<(String, bool)> + Send + 'static,
{
    let listener = UnixListener::bind(socket).expect("bind fake daemon socket");
    listener.set_nonblocking(true).expect("fake daemon listener is nonblocking");
    let socket = socket.to_path_buf();
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = Arc::clone(&stop);
    let thread_socket = socket.clone();
    let thread = thread::spawn(move || {
        while !thread_stop.load(Ordering::SeqCst) {
            let stream = match listener.accept() {
                Ok((stream, _)) => stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                    continue;
                }
                Err(_) => break,
            };
            if handle_fake_connection(stream, &mut respond) {
                break;
            }
        }
        let _ = std::fs::remove_file(thread_socket);
    });
    FakeUnresponsiveSocket { socket, stop, thread: Some(thread) }
}

fn handle_fake_connection<F>(mut stream: UnixStream, respond: &mut F) -> bool
where
    F: FnMut(&str) -> Option<(String, bool)>,
{
    stream.set_nonblocking(false).expect("fake daemon connection is blocking");
    let mut line = String::new();
    {
        let mut reader = BufReader::new(&mut stream);
        if reader.read_line(&mut line).expect("read fake socket request") == 0 {
            return false;
        }
    }
    if let Some((response, shutdown)) = respond(&line) {
        stream.write_all(response.as_bytes()).expect("write fake status response");
        return shutdown;
    }
    false
}

impl Drop for FakeUnresponsiveSocket {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = UnixStream::connect(&self.socket);
        if let Some(thread) = self.thread.take() {
            thread.join().expect("fake socket thread joins");
        }
        let _ = std::fs::remove_file(&self.socket);
    }
}

fn spawn_signal_target() -> Child {
    Command::new("sleep")
        .arg("60")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn signal target")
}

fn spawn_memoryd_serve(repo: &Path, runtime: &Path, socket: &Path) -> Child {
    Command::new(env!("CARGO_BIN_EXE_memoryd"))
        .arg("serve")
        .arg("--repo")
        .arg(repo)
        .arg("--runtime")
        .arg(runtime)
        .arg("--init")
        .arg("--socket")
        .arg(socket)
        .env("MEMORUM_DISABLE_EMBEDDING_WORKER", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn memoryd serve")
}

fn send_sigterm(pid: u32) {
    let status = Command::new("kill").arg("-TERM").arg(pid.to_string()).status().expect("run kill -TERM");
    assert!(status.success(), "kill -TERM {pid} failed with {status}");
}

fn wait_for_socket(socket: &Path, child: &mut Child) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if UnixStream::connect(socket).is_ok() {
            return;
        }
        if let Some(status) = child.try_wait().expect("poll serve child") {
            panic!("serve exited before binding socket: {status}");
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("serve did not bind socket at {}", socket.display());
}

fn wait_for_absent(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if !path.exists() {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
}
