use std::path::Path;
use std::process::Command;
use std::time::Duration;

use memorum_eval::daemon_scaffold::{DaemonScaffold, TwoDeviceScaffold};
use tokio::time::timeout;

#[tokio::test]
async fn two_device_scaffold_shares_a_git_remote_between_healthy_daemons() {
    let scaffold = two_device_scaffold().await;

    let device_a_report = scaffold.device_a.doctor().await;
    let device_b_report = scaffold.device_b.doctor().await;
    assert!(device_a_report.healthy, "device A should be healthy: {device_a_report:?}");
    assert!(device_b_report.healthy, "device B should be healthy: {device_b_report:?}");

    let device_a_remote = git_output(scaffold.device_a.tree_dir(), ["remote", "get-url", "origin"]);
    let device_b_remote = git_output(scaffold.device_b.tree_dir(), ["remote", "get-url", "origin"]);
    let expected_remote = scaffold.remote_path().to_string_lossy();
    assert_eq!(device_a_remote, expected_remote);
    assert_eq!(device_b_remote, expected_remote);

    std::fs::write(scaffold.device_a.tree_dir().join("device-a-memory.md"), "from device A\n")
        .expect("write device A commit fixture");
    git(scaffold.device_a.tree_dir(), ["add", "device-a-memory.md"]);
    git(scaffold.device_a.tree_dir(), ["commit", "-m", "device A memory"]);
    git(scaffold.device_a.tree_dir(), ["push", "origin", "HEAD:main"]);

    git(scaffold.device_b.tree_dir(), ["pull", "--ff-only", "origin", "main"]);

    let pulled_file = scaffold.device_b.tree_dir().join("device-a-memory.md");
    assert_eq!(std::fs::read_to_string(pulled_file).expect("read pulled file"), "from device A\n");
}

fn git<const N: usize>(repo: &Path, args: [&str; N]) {
    let output = Command::new("git").args(args).current_dir(repo).output().expect("run git command");
    assert!(
        output.status.success(),
        "git command failed in {}: status={}\nstdout={}\nstderr={}",
        repo.display(),
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_output<const N: usize>(repo: &Path, args: [&str; N]) -> String {
    let output = Command::new("git").args(args).current_dir(repo).output().expect("run git command");
    assert!(
        output.status.success(),
        "git command failed in {}: status={}\nstdout={}\nstderr={}",
        repo.display(),
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("git stdout is utf8").trim().to_owned()
}

async fn two_device_scaffold() -> TwoDeviceScaffold {
    timeout(Duration::from_secs(15), DaemonScaffold::two_device())
        .await
        .expect("two-device daemon scaffold should not hang")
}
