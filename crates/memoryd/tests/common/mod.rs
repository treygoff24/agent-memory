//! Shared test helpers for memoryd integration tests.
//!
//! Cargo does not compile files under `tests/<subdir>/` as their own test
//! binaries, so this module is included via `mod common;` in each test file
//! that needs it. Each helper is `#[allow(dead_code)]` because any given
//! test file uses only a subset.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use memory_substrate::Substrate;
use memoryd::server::{serve_substrate_with, ServerOptions};
use tokio::net::UnixStream;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout};

/// Spawn the daemon's `serve_substrate_with` on the given socket with a tight
/// idle-frame timeout (5s) and return the shutdown sender + server handle.
#[allow(dead_code)]
pub fn spawn_daemon(socket: &Path, substrate: Substrate) -> (watch::Sender<bool>, JoinHandle<anyhow::Result<()>>) {
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let socket = socket.to_path_buf();
    let options = ServerOptions { idle_frame_timeout: Duration::from_secs(5) };
    let task = tokio::spawn(serve_substrate_with(socket, substrate, options, shutdown_rx));
    (shutdown_tx, task)
}

/// Signal graceful shutdown, await server task within 2s, then remove the
/// socket file. Panics if any step fails — these are test invariants.
#[allow(dead_code)]
pub async fn shutdown(shutdown_tx: watch::Sender<bool>, server: JoinHandle<anyhow::Result<()>>, socket: &Path) {
    shutdown_tx.send(true).expect("shutdown signal lands");
    timeout(Duration::from_secs(2), server)
        .await
        .expect("server stops before timeout")
        .expect("server task joins")
        .expect("server returns Ok");
    let _ = std::fs::remove_file(socket);
}

/// Poll the Unix socket until a connection succeeds (up to 2s @ 10ms cadence).
#[allow(dead_code)]
pub async fn wait_for_socket(socket: &Path) {
    for _ in 0..200 {
        if UnixStream::connect(socket).await.is_ok() {
            return;
        }
        sleep(Duration::from_millis(10)).await;
    }
    panic!("daemon did not bind socket at {}", socket.display());
}

/// Build a unique socket path under a short `/tmp` directory.
///
/// The `prefix` becomes the directory suffix (`/tmp/memd-<prefix>-<pid>/`),
/// keeping per-test socket paths below the macOS UDS length cap. The nanosecond
/// `SystemTime` nonce keeps concurrent tests in the same process distinct.
#[allow(dead_code)]
pub fn unique_socket_path(prefix: &str, test_name: &str) -> PathBuf {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).expect("system clock is after epoch").as_nanos();
    let dir = PathBuf::from(format!("/tmp/memd-{prefix}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create short socket directory");
    dir.join(format!("{test_name}-{nonce}.sock"))
}
