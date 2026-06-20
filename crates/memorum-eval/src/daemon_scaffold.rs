use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::harness_runner::HarnessRunnerError;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DaemonScaffoldConfig;

#[derive(Debug)]
pub struct DaemonScaffold {
    tree_dir: TempTree,
    socket_path: PathBuf,
    child: Option<DaemonChild>,
}

#[derive(Debug)]
pub struct TwoDeviceScaffold {
    pub device_a: DaemonScaffold,
    pub device_b: DaemonScaffold,
    remote_dir: TempTree,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoctorReport {
    pub healthy: bool,
    pub body: String,
}

#[derive(Debug)]
pub struct DaemonChild {
    child: Child,
}

#[derive(Debug)]
struct TempTree {
    path: PathBuf,
}

impl DaemonScaffold {
    pub async fn fresh() -> Self {
        let tree_dir = TempTree::fresh();
        Self::start(tree_dir)
    }

    pub async fn two_device() -> TwoDeviceScaffold {
        let remote_dir = TempTree::fresh();
        git(None, ["init", "--bare", "--initial-branch", "main", remote_dir.path_str().as_str()]);

        let device_a_tree = clone_device_tree(remote_dir.path(), "device-a");
        let device_b_tree = clone_device_tree(remote_dir.path(), "device-b");

        let scaffold = TwoDeviceScaffold {
            device_a: Self::start(device_a_tree),
            device_b: Self::start(device_b_tree),
            remote_dir,
        };
        publish_device_a_baseline(&scaffold);
        align_device_b_with_remote_baseline(&scaffold);
        scaffold
    }

    fn start(tree_dir: TempTree) -> Self {
        // Use a short /tmp socket directory to stay under macOS's 104-char Unix
        // domain socket path cap. The tree itself can stay in the long
        // tempfile path; only the socket name is path-length-sensitive.
        let socket_path = short_socket_path("memd-eval");
        let child = spawn_memoryd(tree_dir.path(), &socket_path);
        wait_for_socket(&socket_path);

        Self { tree_dir, socket_path, child: Some(DaemonChild { child }) }
    }

    pub async fn doctor(&self) -> DoctorReport {
        let mut stream = UnixStream::connect(&self.socket_path)
            .unwrap_or_else(|err| panic!("connect to memoryd socket {}: {err}", self.socket_path.display()));
        stream
            .write_all(
                br#"{"id":"eval-doctor","request":"doctor"}
"#,
            )
            .expect("write doctor request");

        let mut response = String::new();
        BufReader::new(stream).read_line(&mut response).expect("read doctor response");
        DoctorReport { healthy: eval_doctor_healthy(&response), body: response }
    }

    pub fn tree_dir(&self) -> &Path {
        self.tree_dir.path()
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    pub fn into_child_for_test(mut self) -> DaemonChild {
        self.child.take().expect("daemon child is present")
    }
}

fn eval_doctor_healthy(response: &str) -> bool {
    response.contains(r#""healthy":true"#)
        || (response.contains(r#""code":"harness_cli_warning""#)
            && !response.contains(r#""code":"warning""#)
            && !response.contains(r#""code":"repair_required""#)
            && !response.contains(r#""code":"events_log_mirror_lag""#))
}

impl TwoDeviceScaffold {
    pub fn remote_path(&self) -> &Path {
        self.remote_dir.path()
    }
}

impl Drop for DaemonScaffold {
    fn drop(&mut self) {
        if let Some(child) = self.child.take() {
            drop(child);
        }
    }
}

impl DaemonChild {
    pub fn id(&self) -> Option<u32> {
        Some(self.child.id())
    }
}

impl Drop for DaemonChild {
    fn drop(&mut self) {
        if let Ok(Some(_)) = self.child.try_wait() {
            return;
        }

        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl TempTree {
    fn fresh() -> Self {
        let path = std::env::temp_dir().join(format!("memorum-eval-{}", new_ulid_like_id()));
        fs::create_dir_all(&path).unwrap_or_else(|err| panic!("create temp tree {}: {err}", path.display()));
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn path_str(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// Build a short Unix-domain-socket path under `/tmp/<prefix>-<pid>/` to avoid
/// macOS's 104-character socket-name cap. The default tempdir on macOS lives
/// under `/var/folders/...` and is too long once the test name + nonce are
/// appended.
///
/// Socket name combines a monotonic process-local counter with the current
/// nanos. The counter alone guarantees uniqueness across concurrent calls in
/// the same process (covering the T14→T17 cross-scaffold pollution found in
/// the 2026-05-26 audit); the nanos suffix keeps each path human-readable for
/// debug log triage.
fn short_socket_path(prefix: &str) -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).expect("system clock before unix epoch").as_nanos();
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = PathBuf::from(format!("/tmp/{prefix}-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap_or_else(|err| panic!("create short socket dir {}: {err}", dir.display()));
    dir.join(format!("memoryd-{seq}-{nanos}.sock"))
}

fn spawn_memoryd(tree_dir: &Path, socket_path: &Path) -> Child {
    let memoryd = memoryd_binary_path();
    Command::new(memoryd)
        .args([
            "serve",
            "--repo",
            &tree_dir.to_string_lossy(),
            "--runtime",
            &tree_dir.join(".memoryd").to_string_lossy(),
            "--socket",
            &socket_path.to_string_lossy(),
            "--init",
        ])
        .env("MEMORYD_ENABLE_ECHO_DREAM_HARNESS", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn memoryd serve")
}

fn clone_device_tree(remote_path: &Path, device_name: &str) -> TempTree {
    let tree = TempTree::fresh();
    let remote = remote_path.to_string_lossy().into_owned();
    let destination = tree.path_str();
    git(None, ["clone", &remote, &destination]);
    configure_git_identity(tree.path(), device_name);
    tree
}

fn configure_git_identity(repo: &Path, device_name: &str) {
    let email = format!("{device_name}@memorum-eval.local");
    git(Some(repo), ["config", "user.name", device_name]);
    git(Some(repo), ["config", "user.email", &email]);
}

fn publish_device_a_baseline(scaffold: &TwoDeviceScaffold) {
    if git_success(scaffold.device_a.tree_dir(), ["rev-parse", "--verify", "HEAD"]) {
        git(Some(scaffold.device_a.tree_dir()), ["push", "origin", "HEAD:main"]);
    }
}

fn align_device_b_with_remote_baseline(scaffold: &TwoDeviceScaffold) {
    git(Some(scaffold.device_b.tree_dir()), ["fetch", "origin", "main"]);
    git(Some(scaffold.device_b.tree_dir()), ["reset", "--hard", "origin/main"]);
}

fn git<const N: usize>(current_dir: Option<&Path>, args: [&str; N]) {
    let mut command = Command::new("git");
    command.args(args);
    if let Some(current_dir) = current_dir {
        command.current_dir(current_dir);
    }

    let output = command.output().expect("run git command");
    assert!(
        output.status.success(),
        "git command failed: status={}\nstdout={}\nstderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_success<const N: usize>(current_dir: &Path, args: [&str; N]) -> bool {
    Command::new("git")
        .args(args)
        .current_dir(current_dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("run git command")
        .success()
}

fn memoryd_binary_path() -> PathBuf {
    static MEMORYD_BINARY: OnceLock<PathBuf> = OnceLock::new();
    MEMORYD_BINARY.get_or_init(build_memoryd_binary).clone()
}

fn build_memoryd_binary() -> PathBuf {
    let target_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target"));
    let binary = target_dir.join("debug").join("memoryd");

    // Include test-utils so T16's TestInjectEvent surface works,
    // and dev-fixtures so T17 can exercise the deterministic echo dream harness.
    let status = Command::new("cargo")
        .args(["build", "-p", "memoryd", "--features", "test-utils,dev-fixtures"])
        .status()
        .expect("build memoryd binary");
    assert!(status.success(), "cargo build -p memoryd --features test-utils,dev-fixtures failed");
    binary
}

fn wait_for_socket(socket_path: &Path) {
    // Poll-until-connectable: `wait_for_socket_for` retries `UnixStream::connect`
    // on a short backoff and returns the instant the socket accepts. The deadline
    // is set generously (30s) so a slow-but-fine startup is not turned into a hard
    // failure. On macOS, `syspolicyd` inspects a freshly-built binary on first
    // launch, which can delay the socket bind well past a few seconds; the
    // deadline only fires when the daemon genuinely never comes up.
    wait_for_socket_for(socket_path, Duration::from_secs(30), Duration::from_millis(100))
        .unwrap_or_else(|error| panic!("{error}"));
}

fn wait_for_socket_for(
    socket_path: &Path,
    timeout: Duration,
    poll_interval: Duration,
) -> Result<(), HarnessRunnerError> {
    let deadline = std::time::Instant::now() + timeout;
    let mut last_error = None;
    while std::time::Instant::now() < deadline {
        match UnixStream::connect(socket_path) {
            Ok(_) => return Ok(()),
            Err(error) => last_error = Some(error),
        }
        std::thread::sleep(poll_interval);
    }
    Err(HarnessRunnerError::SocketNotReady(format!(
        "memoryd socket did not accept connections within {:?}: {}{}",
        timeout,
        socket_path.display(),
        last_error.map(|error| format!(" ({error})")).unwrap_or_default()
    )))
}

fn new_ulid_like_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let timestamp_ms =
        SystemTime::now().duration_since(UNIX_EPOCH).expect("system clock before unix epoch").as_millis();
    let entropy = ((std::process::id() as u128) << 48) | COUNTER.fetch_add(1, Ordering::Relaxed) as u128;
    format!("{}{}", encode_crockford(timestamp_ms, 10), encode_crockford(entropy, 16))
}

fn encode_crockford(mut value: u128, width: usize) -> String {
    const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let mut encoded = vec![b'0'; width];
    for byte in encoded.iter_mut().rev() {
        *byte = ALPHABET[(value & 31) as usize];
        value >>= 5;
    }
    String::from_utf8(encoded).expect("crockford base32 alphabet is utf8")
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::net::UnixListener;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use super::wait_for_socket_for;

    #[test]
    fn wait_for_socket_requires_accepting_listener_not_only_path_existence() {
        let socket_path = temp_socket_path("regular-file");
        fs::write(&socket_path, b"not a socket").expect("write placeholder");

        let error = wait_for_socket_for(&socket_path, Duration::from_millis(20), Duration::from_millis(1))
            .expect_err("regular file must not be treated as socket readiness")
            .to_string();

        assert!(error.contains("did not accept connections"), "{error}");
        let _ = fs::remove_file(socket_path);
    }

    #[test]
    fn wait_for_socket_returns_when_listener_accepts_connections() {
        let socket_path = temp_socket_path("listener");
        let listener = UnixListener::bind(&socket_path).expect("bind listener");

        wait_for_socket_for(&socket_path, Duration::from_millis(20), Duration::from_millis(1))
            .expect("listener is ready");

        drop(listener);
        let _ = fs::remove_file(socket_path);
    }

    fn temp_socket_path(label: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).expect("system clock before unix epoch").as_nanos();
        std::env::temp_dir().join(format!("memorum-eval-{label}-{nanos}.sock"))
    }
}
