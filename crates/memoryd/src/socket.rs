//! Socket path resolution and liveness probing for memoryd.

use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

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
///
/// Task 5 upgrades this from connect-only liveness to a JSON-RPC status ping.
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

/// Default runtime root for connect-only commands.
pub fn default_runtime_root() -> PathBuf {
    if let Some(value) = std::env::var_os("MEMORUM_RUNTIME") {
        return PathBuf::from(value);
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".local/share/memorum/runtime");
    }
    PathBuf::from(".memoryd")
}
