use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

use memorum_eval::daemon_scaffold::DaemonScaffold;
use memorum_eval::simulator::{AssertionSpec, GovernanceMeta, SimulatorAction, SimulatorAgent, SimulatorConfig};

#[test]
fn simulator_agent_runs_startup_search_write_script_against_daemon() {
    block_on(async {
        let scaffold = DaemonScaffold::fresh().await;
        let mut agent = SimulatorAgent::new(SimulatorConfig::new(scaffold.socket_path()));

        let observations = agent
            .run_script([
                SimulatorAction::Startup { since_event_id: None },
                SimulatorAction::Search { query: "test".to_owned(), namespace: None },
                SimulatorAction::Write {
                    body: "hello eval world".to_owned(),
                    title: None,
                    meta: GovernanceMeta {
                        confidence: 0.95,
                        source_kind: "agent_primary".to_owned(),
                        source_ref: Some("eval_test_1".to_owned()),
                    },
                },
                SimulatorAction::Assert { condition: AssertionSpec::LastWriteStatusIsNotRefused },
            ])
            .await;

        assert_ne!(
            observations.last_write_outcome.as_deref(),
            Some("refused"),
            "write should not be refused: {observations:#?}"
        );
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
