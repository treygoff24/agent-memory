use memorum_eval::block_on;
use memorum_eval::daemon_scaffold::DaemonScaffold;

#[test]
fn daemon_scaffold_starts_healthy_isolated_daemon_and_cleans_up_child() {
    block_on(async {
        let scaffold = DaemonScaffold::fresh().await;

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

        let child = scaffold.into_child_for_test();
        let child_id = child.id().expect("daemon child should still have an id before cleanup");
        drop(child);

        let status = std::process::Command::new("kill")
            .args(["-0", &child_id.to_string()])
            .stderr(std::process::Stdio::null())
            .status()
            .expect("query daemon pid");
        assert!(!status.success(), "daemon pid {child_id} should be gone after scaffold drop");
    });
}
