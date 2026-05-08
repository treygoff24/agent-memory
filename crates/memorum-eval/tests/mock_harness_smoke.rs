use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

use memorum_eval::daemon_scaffold::DaemonScaffold;
use memorum_eval::harness_runner::{MockHarness, TestOutcome};

#[test]
fn mock_harness_skips_test_13_because_semantics_are_not_exercised() {
    block_on(async {
        let scaffold = DaemonScaffold::fresh().await;
        let outcome = MockHarness.run_test(13, &scaffold).expect("mock test #13 should run against fresh daemon");

        let TestOutcome::Skipped { metadata, reason } = outcome else {
            panic!("test #13 should skip in mock mode: {outcome:#?}");
        };

        assert_eq!(metadata.get("mode").map(String::as_str), Some("mock"));
        assert_eq!(reason, "MOCK_HARNESS_SEMANTIC_NOT_EXERCISED");
    });
}

#[test]
fn mock_harness_skips_test_15_because_semantics_are_not_exercised() {
    block_on(async {
        let scaffold = DaemonScaffold::fresh().await;
        let outcome = MockHarness.run_test(15, &scaffold).expect("mock test #15 should run against fresh daemon");

        let TestOutcome::Skipped { metadata, reason } = outcome else {
            panic!("test #15 should skip in mock mode: {outcome:#?}");
        };

        assert_eq!(metadata.get("mode").map(String::as_str), Some("mock"));
        assert_eq!(reason, "MOCK_HARNESS_SEMANTIC_NOT_EXERCISED");
    });
}

#[cfg(not(feature = "stream-i-deps"))]
#[test]
fn mock_harness_skips_test_19_without_stream_i_deps() {
    block_on(async {
        let scaffold = DaemonScaffold::fresh().await;
        let outcome =
            MockHarness.run_test(19, &scaffold).expect("mock test #19 should return a default feature-gated outcome");

        let TestOutcome::Skipped { metadata, reason } = outcome else {
            panic!("test #19 should skip without stream-i-deps: {outcome:#?}");
        };

        assert_eq!(metadata.get("mode").map(String::as_str), Some("mock"));
        assert_eq!(
            reason,
            "stream-i-deps feature disabled — peer-update framing requires `memorum-coordination::framing_tests::assert_framing`"
        );
    });
}

#[cfg(feature = "stream-i-deps")]
#[test]
fn mock_harness_runs_test_19_with_stream_i_deps() {
    block_on(async {
        let scaffold = DaemonScaffold::fresh().await;
        let outcome =
            MockHarness.run_test(19, &scaffold).expect("mock test #19 should run when stream-i-deps is enabled");

        let TestOutcome::Passed { metadata, output } = outcome else {
            panic!("test #19 should pass with stream-i-deps: {outcome:#?}");
        };

        assert_eq!(metadata.get("mode").map(String::as_str), Some("mock"));
        assert_eq!(output.get("attribution_correct").map(String::as_str), Some("true"));
        assert_eq!(output.get("no_directive_execution").map(String::as_str), Some("true"));
        assert_eq!(output.get("awareness_acknowledged").map(String::as_str), Some("true"));
        assert_eq!(output.get("framing_correct").map(String::as_str), Some("true"));
    });
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
