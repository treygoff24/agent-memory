//! Socket path resolution and liveness probing for memoryd.

use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// Coarse socket state used by lifecycle orchestration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SocketProbe {
    /// A daemon answered on the socket.
    Live,
    /// A filesystem entry exists, but no daemon answered.
    Stale,
    /// No socket exists at this path.
    Absent,
}

/// Probe whether a Unix socket accepts connections.
pub fn probe_live_socket(path: &Path) -> SocketProbe {
    if !path.exists() {
        return SocketProbe::Absent;
    }
    match UnixStream::connect(path) {
        Ok(_) => SocketProbe::Live,
        Err(_) => SocketProbe::Stale,
    }
}

/// Resolve the daemon socket inside a runtime root.
pub fn resolve_socket_path(runtime: &Path) -> PathBuf {
    runtime.join("memoryd.sock")
}

/// How long the daemon-readiness poll waits for the socket to go live.
pub const DAEMON_READY_TIMEOUT: Duration = Duration::from_secs(10);

/// Spawn a detached `memoryd serve --init` child bound to `socket`, with stdio
/// silenced. Shared by every code path that brings up a daemon (`mcp
/// --auto-start`, the setup background-daemon step, and the transient import
/// daemon) so the serve invocation cannot drift between call sites. Each caller
/// owns the readiness wait and child lifecycle from here.
pub fn spawn_serve_child(repo: &Path, runtime: &Path, socket: &Path) -> std::io::Result<Child> {
    let exe = std::env::current_exe()?;
    Command::new(exe)
        .arg("serve")
        .arg("--repo")
        .arg(repo)
        .arg("--runtime")
        .arg(runtime)
        .arg("--init")
        .arg("--socket")
        .arg(socket)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

/// Result of waiting for a spawned daemon child to bind its socket.
///
/// `await_socket_ready` never kills the child; the caller decides whether to
/// reap it (transient daemon) or leave it detached (background / auto-start).
#[derive(Debug)]
pub enum DaemonReadiness {
    /// The socket went live before the deadline.
    Ready,
    /// The child process exited before the socket became live.
    ExitedEarly(std::process::ExitStatus),
    /// The deadline elapsed with neither readiness nor an early exit.
    TimedOut,
    /// Polling the child's status failed.
    PollFailed(std::io::Error),
}

/// Poll `socket` for liveness until `timeout` elapses, watching `child` for an
/// early exit. Returns without touching the child so callers keep full control
/// of its lifecycle. Shared by the daemon-startup call sites whose poll loops
/// were byte-for-byte identical.
pub async fn await_socket_ready(child: &mut Child, socket: &Path, timeout: Duration) -> DaemonReadiness {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if matches!(probe_live_socket(socket), SocketProbe::Live) {
            return DaemonReadiness::Ready;
        }
        match child.try_wait() {
            Ok(Some(status)) => return DaemonReadiness::ExitedEarly(status),
            Ok(None) => {}
            Err(error) => return DaemonReadiness::PollFailed(error),
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    DaemonReadiness::TimedOut
}
