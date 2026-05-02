use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

use memorum_eval::daemon_scaffold::DaemonScaffold;
use memorum_eval::simulator::{GovernanceMeta, SimulatorAction, SimulatorAgent, SimulatorConfig};

#[test]
fn privacy_filter_rejects_luhn_valid_card_number() {
    block_on(async {
        let scaffold = DaemonScaffold::fresh().await;
        let mut agent = SimulatorAgent::new(SimulatorConfig::new(scaffold.socket_path()));

        let observations = agent
            .run_script([SimulatorAction::Write {
                body: "meta privacy fixture contains 4111111111111111 and must not persist".to_owned(),
                title: None,
                meta: GovernanceMeta {
                    confidence: 0.95,
                    source_kind: "agent_primary".to_owned(),
                    source_ref: Some("meta_privacy_filter_connectivity".to_owned()),
                },
            }])
            .await;

        let response = observations.last_write_json.as_deref().expect("write response should be captured");
        let lower = response.to_ascii_lowercase();
        assert!(
            observations.last_write_outcome.as_deref() == Some("refused")
                || lower.contains(r#""code":"privacy_error""#)
                || lower.contains("privacy refused")
                || lower.contains("policy"),
            "expected privacy/policy refusal shape, got: {response}"
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
