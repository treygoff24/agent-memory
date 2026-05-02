use std::future::Future;
use std::path::Path;
use std::pin::pin;
use std::process::Command;
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

use memorum_eval::daemon_scaffold::DaemonScaffold;

#[test]
fn two_device_scaffold_shares_a_git_remote_between_healthy_daemons() {
    block_on(async {
        let scaffold = DaemonScaffold::two_device().await;

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
    });
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

fn block_on<T>(future: impl Future<Output = T>) -> T {
    let waker = noop_waker();
    let mut context = Context::from_waker(&waker);
    let mut future = pin!(future);

    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => return output,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}

fn noop_waker() -> Waker {
    unsafe fn clone(_: *const ()) -> RawWaker {
        raw_waker()
    }

    unsafe fn wake(_: *const ()) {}
    unsafe fn wake_by_ref(_: *const ()) {}
    unsafe fn drop(_: *const ()) {}

    fn raw_waker() -> RawWaker {
        RawWaker::new(std::ptr::null(), &RawWakerVTable::new(clone, wake, wake_by_ref, drop))
    }

    // SAFETY: the no-op raw waker does not dereference its data pointer and its
    // vtable functions are valid for the null data pointer for this synchronous
    // test executor. The futures under test do blocking work and never rely on
    // external wakeups.
    unsafe { Waker::from_raw(raw_waker()) }
}
