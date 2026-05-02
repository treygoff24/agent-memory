use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
use std::time::{SystemTime, UNIX_EPOCH};

use memorum_eval::daemon_scaffold::DaemonScaffold;
use memorum_eval::simulator::{SimulatorAction, SimulatorAgent, SimulatorConfig};

#[test]
fn dream_journal_file_is_not_searchable_as_canonical_memory() {
    block_on(async {
        let scaffold = DaemonScaffold::fresh().await;
        let sentinel = format!("META_DREAM_EXCLUSION_{}_{}", std::process::id(), unix_seconds());
        let dream_path =
            scaffold.tree_dir().join("dreams").join("journal").join("me").join(format!("{}.md", today_utc()));
        std::fs::create_dir_all(dream_path.parent().expect("dream path has parent")).expect("create dream journal dir");
        std::fs::write(&dream_path, format!("dream scratchpad without frontmatter\n{sentinel}\n"))
            .expect("write noncanonical dream journal file");

        let mut agent = SimulatorAgent::new(SimulatorConfig::new(scaffold.socket_path()));
        let observations =
            agent.run_script([SimulatorAction::Search { query: sentinel.clone(), namespace: None }]).await;

        assert_eq!(
            observations.last_search_result_count,
            Some(0),
            "dream journal text should not be indexed as canonical memory: {:#?}",
            observations.last_search_json
        );

        // Stream H is a consumer of public daemon/CLI surfaces. The lower-level
        // Substrate::read_memory_envelope(NotACanonicalMemory) check is not
        // reachable through the currently owned memorum-eval files without adding
        // a new dependency or daemon API, so this meta test preserves the public
        // behavioral assertion: dream scratchpad text is not searchable memory.
    });
}

fn unix_seconds() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).expect("system clock after unix epoch").as_secs()
}

fn today_utc() -> String {
    let days = (unix_seconds() / 86_400) as i64;
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}")
}

fn civil_from_days(days_since_unix_epoch: i64) -> (i32, u32, u32) {
    let z = days_since_unix_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = y + i64::from(month <= 2);
    (year as i32, month as u32, day as u32)
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
