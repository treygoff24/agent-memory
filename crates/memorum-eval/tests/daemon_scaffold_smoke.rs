use std::time::Duration;

use memorum_eval::daemon_scaffold::{DaemonScaffold, DaemonScaffoldConfig};
use tokio::time::timeout;

#[tokio::test]
async fn daemon_scaffold_starts_healthy_isolated_daemon_and_cleans_up_child() {
    let scaffold = fresh_scaffold().await;

    assert!(scaffold.tree_dir().exists(), "temp tree should exist while scaffold is alive");
    assert!(!scaffold.socket_path().as_os_str().is_empty(), "socket path should be populated");
    // Socket lives under a short /tmp/memd-eval-<pid>/ directory to stay
    // under macOS's 104-char Unix-domain-socket name cap. The tree dir
    // (which still uses the long memorum-eval-<id> tempfile name) is the
    // primary uniqueness guarantee; the socket dir disambiguates per
    // process.
    assert!(
        scaffold.socket_path().to_string_lossy().contains("memd-eval-"),
        "socket path should be in the short /tmp/memd-eval-<pid>/ directory"
    );
    assert!(
        scaffold.tree_dir().to_string_lossy().contains("memorum-eval-"),
        "tree dir should be a unique memorum-eval ULID directory"
    );

    let report = scaffold.doctor().await;
    assert!(report.healthy, "doctor report should be healthy: {report:?}");

    let child_id = scaffold.child_id_for_test().expect("daemon child should still have an id before cleanup");
    drop(scaffold);

    assert_daemon_pid_gone(child_id);
}

#[tokio::test]
async fn daemon_scaffold_no_cleanup_preserves_tree_dir() {
    let scaffold =
        timeout(Duration::from_secs(10), DaemonScaffold::fresh_with_config(DaemonScaffoldConfig { no_cleanup: true }))
            .await
            .expect("fresh daemon scaffold should not hang");
    let tree = scaffold.tree_dir().to_path_buf();
    assert!(tree.exists(), "temp tree should exist while scaffold is alive");
    drop(scaffold);

    assert!(tree.exists(), "no_cleanup should preserve scaffold tree {}", tree.display());
    std::fs::remove_dir_all(&tree).expect("cleanup preserved tree");
}

fn assert_daemon_pid_gone(child_id: u32) {
    let status = std::process::Command::new("kill")
        .args(["-0", &child_id.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .expect("query daemon pid");
    assert!(!status.success(), "daemon pid {child_id} should be gone after scaffold drop");
}

async fn fresh_scaffold() -> DaemonScaffold {
    timeout(Duration::from_secs(10), DaemonScaffold::fresh()).await.expect("fresh daemon scaffold should not hang")
}
