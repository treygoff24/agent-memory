use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

use memorum_eval::daemon_scaffold::DaemonScaffold;
use memorum_eval::simulator::{SimulatorAction, SimulatorAgent, SimulatorConfig};

#[test]
fn simulator_startup_receives_startup_response_from_daemon() {
    block_on(async {
        let scaffold = DaemonScaffold::fresh().await;
        let mut agent = SimulatorAgent::new(SimulatorConfig::new(scaffold.socket_path()));

        let observations = agent.run_script([SimulatorAction::Startup { since_event_id: None }]).await;

        let startup_json = observations.last_startup_json.as_deref().expect("startup response should be captured");
        assert!(
            startup_json.contains(r#""startup""#),
            "expected ResponsePayload::Startup-shaped JSON, got: {startup_json}"
        );
        assert!(
            observations.last_startup_block.is_some(),
            "startup response should expose the rendered recall/startup block: {startup_json}"
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

    // SAFETY: the no-op raw waker never dereferences its data pointer. The
    // futures under test do synchronous blocking work and do not need wakeups.
    unsafe { Waker::from_raw(raw_waker()) }
}
