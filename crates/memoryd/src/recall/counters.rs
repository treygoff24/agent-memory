use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecallCounters {
    pub startup_invoked_total: u64,
    #[serde(default)]
    pub startup_failed_total: BTreeMap<String, u64>,
    pub delta_invoked_total: u64,
    #[serde(default)]
    pub delta_failed_total: BTreeMap<String, u64>,
    #[serde(default)]
    pub budget_exhausted_total: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, Default)]
pub struct SharedRecallCounters {
    inner: Arc<Mutex<RecallCounters>>,
}

impl SharedRecallCounters {
    pub fn snapshot(&self) -> RecallCounters {
        self.inner.lock().expect("recall counters lock not poisoned").clone()
    }

    pub fn record_startup_success(&self) {
        self.inner.lock().expect("recall counters lock not poisoned").startup_invoked_total += 1;
    }

    pub fn record_startup_failure(&self, code: &str) {
        let mut counters = self.inner.lock().expect("recall counters lock not poisoned");
        *counters.startup_failed_total.entry(code.to_owned()).or_default() += 1;
    }

    pub fn record_delta_success(&self) {
        self.inner.lock().expect("recall counters lock not poisoned").delta_invoked_total += 1;
    }

    pub fn record_delta_failure(&self, code: &str) {
        let mut counters = self.inner.lock().expect("recall counters lock not poisoned");
        *counters.delta_failed_total.entry(code.to_owned()).or_default() += 1;
    }

    pub fn record_budget_exhausted(&self, section: &str) {
        let mut counters = self.inner.lock().expect("recall counters lock not poisoned");
        *counters.budget_exhausted_total.entry(section.to_owned()).or_default() += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::SharedRecallCounters;

    #[test]
    fn counters_remain_consistent_under_concurrent_recording() {
        let counters = SharedRecallCounters::default();
        std::thread::scope(|scope| {
            for _ in 0..8 {
                let counters = counters.clone();
                scope.spawn(move || {
                    for _ in 0..64 {
                        counters.record_startup_success();
                        counters.record_delta_success();
                        counters.record_budget_exhausted("recent-memory");
                    }
                });
            }
        });

        let snapshot = counters.snapshot();
        assert_eq!(snapshot.startup_invoked_total, 512);
        assert_eq!(snapshot.delta_invoked_total, 512);
        assert_eq!(snapshot.budget_exhausted_total.get("recent-memory"), Some(&512));
    }
}
